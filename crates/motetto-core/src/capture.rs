//! Capture stage: system audio + mic from one ScreenCaptureKit stream, one WAV per track.

use crate::events::Stop;
use crate::mics::Meters;
pub use crate::platform::macos_video::displays;
use crate::platform::macos_video::{Encoding, Input, Picture, Writer};
use crate::video::{self, Captured, ScreenCapture, ScreenTarget, VideoOutcome};
use screencapturekit::prelude::*;
use std::error::Error;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

mod screen;
mod track;
use screen::picture_filter;
pub use screen::{screen_windows, window_display, window_frame};
use track::{Track, Wav};

fn wait_for_stop(
    stop: &Stop,
    errors: &mpsc::Receiver<Result<(), String>>,
    picture_ended: &AtomicBool,
    mut flush: impl FnMut() -> Result<(), String>,
) -> Result<(), String> {
    let mut flushed = Instant::now();
    while !stop.load(Ordering::Relaxed) && !picture_ended.load(Ordering::Acquire) {
        match errors.recv_timeout(Duration::from_millis(50)) {
            Ok(result) => return result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if flushed.elapsed() >= Duration::from_secs(1) {
                    flush()?;
                    flushed = Instant::now();
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("capture error channel closed".into());
            }
        }
    }
    Ok(())
}

/// Which audio tracks a recording writes: `mic` is Me (`.mic.wav`), `system` is Them
/// (`.system.wav`). A track that is off is never captured and gets no file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tracks {
    pub mic: bool,
    pub system: bool,
}

/// Records both tracks until `stop` is set or writing fails; returns tracks that got audio.
pub fn record(base: &str, stop: Stop) -> Result<Vec<String>, Box<dyn Error>> {
    let both = Tracks {
        mic: true,
        system: true,
    };
    record_with_levels(base, stop, None, None, None, both).map(|captured| captured.files)
}

/// The desktop's recording: optional meters, and `mic` (a `mics::Mic` id) to record instead of
/// the system default input. Picture capture uses a separate stream so its filter never
/// changes the audio scope; picture errors are reported separately while WAV recording continues.
/// With both `tracks` off only the picture is recorded, and its MP4 has a video track alone.
pub fn record_with_levels(
    base: &str,
    stop: Stop,
    meters: Option<Meters>,
    mic: Option<&str>,
    screen: Option<&ScreenCapture>,
    tracks: Tracks,
) -> Result<Captured, Box<dyn Error>> {
    record_tracks(base, stop, meters, mic, screen, tracks)
}

/// The microphone alone (a rehearsal, SPEC 14.6): system audio is never captured, and only
/// `<base>.mic.wav` is written.
pub fn record_mic_only(
    base: &str,
    stop: Stop,
    mic: Option<&str>,
    meters: Option<Meters>,
) -> Result<Captured, Box<dyn Error>> {
    let mic_only = Tracks {
        mic: true,
        system: false,
    };
    record_tracks(base, stop, meters, mic, None, mic_only)
}

fn record_tracks(
    base: &str,
    stop: Stop,
    meters: Option<Meters>,
    mic: Option<&str>,
    screen: Option<&ScreenCapture>,
    tracks: Tracks,
) -> Result<Captured, Box<dyn Error>> {
    let audio = tracks.mic || tracks.system;
    if !audio && screen.is_none() {
        return Err("nothing to record: turn on the microphone, app sounds or the screen".into());
    }
    let content = SCShareableContent::get()?;
    let display = content
        .displays()
        .into_iter()
        .next()
        .ok_or("no display found")?;
    let audio_filter = SCContentFilter::create()
        .with_display(&display)
        .with_excluding_windows(&[])
        .build()?;
    // SCK needs a video filter; 2x2 frames at 1fps keeps it ~free.
    let config = SCStreamConfiguration::new()
        .with_width(2)
        .with_height(2)
        .with_minimum_frame_interval(&CMTime::new(1, 1))
        .with_captures_audio(tracks.system)
        .with_channel_count(1)
        .with_captures_microphone(tracks.mic)?;
    let config = match mic {
        Some(id) => config.with_microphone_capture_device_id(id)?,
        None => config,
    };

    let mut outcome = screen.map(|s| VideoOutcome {
        path: s.path.clone(),
        width: 0,
        height: 0,
        moments_ms: vec![],
        lost: false,
        error: None,
    });
    let mut encoding = None;
    let mut picture = None;
    if let Some(screen) = screen {
        let prepared =
            picture_filter(&content, screen).and_then(|(filter, width, height, area)| {
                // Without audio, the first frame starts the MP4's clock.
                let writer = Writer::create(
                    std::path::Path::new(&screen.path),
                    width,
                    height,
                    screen.preset,
                    tracks,
                )?;
                let video = Arc::new(Encoding::new(writer));
                let config = SCStreamConfiguration::new()
                    .with_width(width)
                    .with_height(height)
                    .with_minimum_frame_interval(&CMTime::new(1, screen.preset.fps()))
                    .with_captures_audio(false)
                    .with_shows_cursor(screen.cursor)
                    .with_pixel_format(PixelFormat::BGRA)
                    .with_preserves_aspect_ratio(true)
                    .map_err(|e| e.to_string())?;
                let mut config = match area {
                    Some(rect) => config.with_source_rect(rect),
                    None => config,
                };
                // Drawn click rings need macOS 15; older systems record without them.
                if screen.clicks {
                    let _ = config.set_shows_mouse_clicks(true);
                }
                let mut stream =
                    SCStream::new_with_delegate(&filter, &config, PictureDelegate(video.clone()))
                        .map_err(|e| e.to_string())?;
                stream
                    .add_output_handler(Picture(video.clone()), SCStreamOutputType::Screen)
                    .map_err(|e| e.to_string())?;
                Ok((stream, video, width, height))
            });
        match prepared {
            Ok((stream, video, width, height)) => {
                if let Some(outcome) = outcome.as_mut() {
                    outcome.width = width;
                    outcome.height = height;
                }
                picture = Some(stream);
                encoding = Some(video);
            }
            // Video only: nothing else would be recorded, so fail now instead of at Stop.
            Err(error) if !audio => return Err(error.into()),
            Err(error) => {
                if let Some(outcome) = outcome.as_mut() {
                    outcome.error = Some(error);
                }
            }
        }
    }
    let (errors, stopped) = mpsc::channel();
    // The audio stream is skipped when no track needs it; the picture then keeps its own clock.
    let mut stream = audio
        .then(|| SCStream::new_with_delegate(&audio_filter, &config, AudioDelegate(errors.clone())))
        .transpose()?;
    let mut wavs = vec![];
    for (suffix, kind, on) in [
        ("system", SCStreamOutputType::Audio, tracks.system),
        ("mic", SCStreamOutputType::Microphone, tracks.mic),
    ] {
        let Some(stream) = stream.as_mut().filter(|_| on) else {
            continue;
        };
        let path = format!("{base}.{suffix}.wav");
        let wav: Wav = Arc::default();
        stream.add_output_handler(
            Track {
                path: path.clone(),
                video: encoding.clone(),
                input: if suffix == "system" {
                    Input::Them
                } else {
                    Input::Me
                },
                wav: wav.clone(),
                errors: errors.clone(),
                level: meters
                    .as_ref()
                    .map(|m| m.levels[usize::from(suffix == "system")].clone()),
                clips: meters
                    .as_ref()
                    .filter(|_| suffix == "mic")
                    .map(|m| m.mic_clips.clone()),
                listen: meters
                    .as_ref()
                    .filter(|_| suffix == "mic")
                    .map(|m| m.mic_listener.clone()),
            },
            kind,
        )?;
        wavs.push((path, wav));
    }

    if tracks.system && tracks.mic {
        println!("Recording system audio + mic. Press Enter to stop.");
    }
    if let Some(stream) = stream.as_mut() {
        stream.start_capture()?;
    }
    if let Some(picture) = picture.as_mut()
        && let Err(error) = picture.start_capture()
        && let Some(video) = &encoding
    {
        video.fail(error.to_string());
    }
    let mut reported = None;
    let mut video_done = false;
    // Video only: a finished (failed or lost) picture ends the recording like a stop.
    let picture_ended = AtomicBool::new(false);
    let mut result = wait_for_stop(&stop, &stopped, &picture_ended, || {
        for (path, wav) in &wavs {
            let mut wav = wav
                .lock()
                .map_err(|_| "audio writer lock poisoned".to_string())?;
            if let Some(writer) = wav.as_mut() {
                writer
                    .flush()
                    .map_err(|e| format!("cannot flush {path}: {e}; check free disk space"))?;
            }
        }
        if let Some(video) = outcome.as_mut() {
            if let Some(encoder) = &encoding {
                let (moments, error) = encoder.snapshot();
                video.moments_ms = moments;
                video.error = error;
                if encoder.failed() && !video_done {
                    if let Some(ScreenCapture {
                        target: ScreenTarget::Window(id),
                        ..
                    }) = screen
                    {
                        video.lost = screen_windows()
                            .ok()
                            .is_some_and(|windows| !windows.iter().any(|w| w.id == *id));
                    }
                    if let Some(picture) = picture.as_mut() {
                        let _ = picture.stop_capture();
                    }
                    let (moments, error) = encoder.finish();
                    video.moments_ms = moments;
                    video.error = if video.lost { None } else { error };
                    video_done = true;
                    picture_ended.store(!audio, Ordering::Release);
                }
            }
            if reported.as_ref() != Some(video) {
                if let Some(progress) = screen.and_then(|s| s.progress.as_ref()) {
                    if video.lost || video.error.is_some() {
                        let _ = progress.send(video.clone());
                    } else {
                        let _ = progress.try_send(video.clone());
                    }
                }
                reported = Some(video.clone());
            }
        }
        Ok(())
    });
    let stop_result = stream.as_mut().map_or(Ok(()), |stream| {
        stream
            .stop_capture()
            .map_err(|e| format!("cannot stop capture: {e}"))
    });
    // Readers load the last level; a stopped capture has none.
    for level in meters.iter().flat_map(|m| &m.levels) {
        level.store(0, Ordering::Relaxed);
    }
    result = result.and(stop_result);
    for pending in stopped.try_iter() {
        result = result.and(pending);
    }

    let mut files = vec![];
    for (path, wav) in wavs {
        let finalized =
            (|| -> Result<(), String> {
                let mut wav = wav
                    .lock()
                    .map_err(|_| "audio writer lock poisoned".to_string())?;
                if let Some(w) = wav.take() {
                    w.finalize().map_err(|e| format!(
                    "cannot finalize {path}: {e}; check free disk space and folder permissions"
                ))?;
                    files.push(path);
                }
                Ok(())
            })();
        // Attempt both tracks even if the first one failed; keep the first error.
        result = result.and(finalized);
    }
    if let Some(video) = outcome.as_mut() {
        if !video_done
            && let Some(picture) = picture.as_mut()
            && let Err(error) = picture.stop_capture()
            && let Some(encoder) = &encoding
        {
            encoder.fail(error.to_string());
        }
        if !video_done && let Some(encoding) = &encoding {
            let (moments, error) = encoding.finish();
            video.moments_ms = moments;
            video.error = error;
        }
        if let Some(progress) = screen.and_then(|s| s.progress.as_ref()) {
            let _ = progress.send(video.clone());
        }
    }
    result?;
    if let Some(error) = outcome
        .as_ref()
        .filter(|_| !audio)
        .and_then(|v| v.error.clone())
    {
        return Err(error.into());
    }
    if audio && files.is_empty() {
        return Err(
            "no audio captured - check Screen Recording / Microphone permission for your terminal"
                .into(),
        );
    }
    Ok(Captured {
        files,
        video: outcome,
    })
}

struct AudioDelegate(mpsc::Sender<Result<(), String>>);
impl SCStreamDelegateTrait for AudioDelegate {
    fn did_stop_with_error(&self, error: screencapturekit::error::SCError) {
        let _ = self.0.send(Err(format!("audio capture stopped: {error}")));
    }
}
struct PictureDelegate(Arc<Encoding>);
impl SCStreamDelegateTrait for PictureDelegate {
    fn did_stop_with_error(&self, error: screencapturekit::error::SCError) {
        self.0.fail(error.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "needs Microphone permission; records the microphone for two seconds"]
    fn records_the_mic_only() {
        let dir = std::env::temp_dir().join(format!("motetto-mic-only-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let base = dir.join("rehearsal").to_string_lossy().into_owned();
        let stop = Stop::default();
        let timer = stop.clone();
        // Listing shareable content takes 6-14 s on some Macs: a fixed 2 s timer stopped before
        // the stream began. Two seconds from the first mic buffer (its WAV appearing) instead.
        let first = format!("{base}.mic.wav");
        std::thread::spawn(move || {
            let waited = Instant::now();
            while !std::path::Path::new(&first).exists() && waited.elapsed().as_secs() < 60 {
                std::thread::sleep(Duration::from_millis(50));
            }
            std::thread::sleep(Duration::from_secs(2));
            timer.store(true, Ordering::SeqCst);
        });
        let captured = record_mic_only(&base, stop, None, None).unwrap();
        assert_eq!(captured.files, [format!("{base}.mic.wav")]);
        assert!(!std::path::Path::new(&format!("{base}.system.wav")).exists());
        let reader = hound::WavReader::open(format!("{base}.mic.wav")).unwrap();
        let seconds = reader.duration() as f64 / f64::from(reader.spec().sample_rate);
        assert!(seconds > 1.0, "{seconds} s");
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    #[ignore = "needs Screen Recording and Microphone permission; records the main display for five seconds"]
    fn records_a_display_part_with_picture() {
        let display = SCShareableContent::get()
            .unwrap()
            .displays()
            .into_iter()
            .next()
            .unwrap();
        let dir = std::env::temp_dir().join(format!(
            "motetto-screen-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let base = dir.join("part-0000").to_string_lossy().into_owned();
        let screen = ScreenCapture {
            target: ScreenTarget::Display(display.display_id()),
            preset: video::Preset::Smaller,
            cursor: true,
            clicks: false,
            path: format!("{base}.screen.mp4"),
            scale: 1.0,
            progress: None,
        };
        let stop = Stop::default();
        let timer = stop.clone();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(5));
            timer.store(true, Ordering::Relaxed);
        });
        eprintln!(
            "Recording the main display and both audio tracks for five seconds; output: {}",
            dir.display()
        );
        let both = Tracks {
            mic: true,
            system: true,
        };
        let result = record_with_levels(&base, stop, None, None, Some(&screen), both);
        thread.join().unwrap();
        let captured = result.unwrap();
        assert_eq!(captured.files.len(), 2);
        let video = captured.video.unwrap();
        assert_eq!(video.error, None);
        assert!(!video.lost);
        assert_eq!(video.moments_ms.first(), Some(&0));
        assert!(std::fs::metadata(&screen.path).unwrap().len() > 10_000);
        match std::process::Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_name",
                "-of",
                "csv=p=0",
                &screen.path,
            ])
            .output()
        {
            Ok(output) => {
                assert!(output.status.success());
                let codecs = String::from_utf8(output.stdout).unwrap();
                assert_eq!(codecs.lines().filter(|c| *c == "h264").count(), 1);
                assert_eq!(codecs.lines().filter(|c| *c == "aac").count(), 2);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("ffprobe unavailable; codec check skipped")
            }
            Err(e) => panic!("cannot run ffprobe: {e}"),
        }
    }

    #[test]
    #[ignore = "needs Screen Recording permission; records a still area of the main display for four seconds"]
    fn a_still_area_lasts_until_stop() {
        let display = SCShareableContent::get()
            .unwrap()
            .displays()
            .into_iter()
            .next()
            .unwrap();
        // The display's bottom-right corner: wallpaper or the Dock, which stand still.
        let (right, bottom) = (f64::from(display.width()), f64::from(display.height()));
        let dir = std::env::temp_dir().join(format!("motetto-still-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        let base = dir.join("part-0000").to_string_lossy().into_owned();
        let screen = ScreenCapture {
            target: ScreenTarget::Area {
                display: display.display_id(),
                rect: video::AreaRect {
                    x: right - 400.0,
                    y: bottom - 300.0,
                    width: 400.0,
                    height: 300.0,
                },
            },
            preset: video::Preset::Smaller,
            cursor: false,
            clicks: false,
            path: format!("{base}.screen.mp4"),
            scale: 1.0,
            progress: None,
        };
        let stop = Stop::default();
        let timer = stop.clone();
        // Starting the stream can take seconds; the four count from the first frame written.
        let written = std::path::PathBuf::from(&screen.path);
        let thread = std::thread::spawn(move || {
            let waiting = std::time::Instant::now();
            while !std::fs::metadata(&written).is_ok_and(|m| m.len() > 0)
                && waiting.elapsed() < Duration::from_secs(60)
            {
                std::thread::sleep(Duration::from_millis(50));
            }
            std::thread::sleep(Duration::from_secs(4));
            timer.store(true, Ordering::Relaxed);
        });
        let silent = Tracks {
            mic: false,
            system: false,
        };
        let result = record_with_levels(&base, stop, None, None, Some(&screen), silent);
        thread.join().unwrap();
        assert_eq!(result.unwrap().video.unwrap().error, None);
        let length = crate::trim::duration_ms(std::path::Path::new(&screen.path)).unwrap();
        eprintln!("still area: {length} ms");
        assert!(length >= 3500, "{length} ms");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    #[ignore = "needs Screen Recording permission; records an area of the main display for three seconds"]
    fn silent_area_plays() {
        let display = SCShareableContent::get()
            .unwrap()
            .displays()
            .into_iter()
            .next()
            .unwrap();
        let dir = std::env::temp_dir().join(format!(
            "motetto-area-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let base = dir.join("part-0000").to_string_lossy().into_owned();
        let screen = ScreenCapture {
            target: ScreenTarget::Area {
                display: display.display_id(),
                rect: video::AreaRect {
                    x: 0.0,
                    y: 0.0,
                    width: 400.0,
                    height: 300.0,
                },
            },
            preset: video::Preset::Smaller,
            cursor: true,
            clicks: false,
            path: format!("{base}.screen.mp4"),
            scale: 1.0,
            progress: None,
        };
        let stop = Stop::default();
        let timer = stop.clone();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(3));
            timer.store(true, Ordering::Relaxed);
        });
        let silent = Tracks {
            mic: false,
            system: false,
        };
        let result = record_with_levels(&base, stop, None, None, Some(&screen), silent);
        thread.join().unwrap();
        let captured = result.unwrap();
        assert!(captured.files.is_empty());
        assert!(!std::path::Path::new(&format!("{base}.mic.wav")).exists());
        assert!(!std::path::Path::new(&format!("{base}.system.wav")).exists());
        let video = captured.video.unwrap();
        assert_eq!(video.error, None);
        assert_eq!((video.width, video.height), (400, 300));
        assert!(std::fs::metadata(&screen.path).unwrap().len() > 5_000);
        match std::process::Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_name",
                "-of",
                "csv=p=0",
                &screen.path,
            ])
            .output()
        {
            Ok(output) => {
                assert!(output.status.success());
                let codecs = String::from_utf8(output.stdout).unwrap();
                assert_eq!(codecs.lines().collect::<Vec<_>>(), ["h264"]);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("ffprobe unavailable; track check skipped")
            }
            Err(e) => panic!("cannot run ffprobe: {e}"),
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    #[ignore = "needs Screen Recording and Microphone permission; records an area and the mic"]
    fn mic_without_app_sounds_gives_the_video_one_audio_track() {
        let display = SCShareableContent::get()
            .unwrap()
            .displays()
            .into_iter()
            .next()
            .unwrap();
        let dir = std::env::temp_dir().join(format!("motetto-mic-area-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let base = dir.join("part-0000").to_string_lossy().into_owned();
        let screen = ScreenCapture {
            target: ScreenTarget::Area {
                display: display.display_id(),
                rect: video::AreaRect {
                    x: 0.0,
                    y: 0.0,
                    width: 400.0,
                    height: 300.0,
                },
            },
            preset: video::Preset::Smaller,
            cursor: true,
            clicks: true,
            path: format!("{base}.screen.mp4"),
            scale: 1.0,
            progress: None,
        };
        let stop = Stop::default();
        let timer = stop.clone();
        let first = format!("{base}.mic.wav");
        std::thread::spawn(move || {
            let waited = Instant::now();
            while !std::path::Path::new(&first).exists() && waited.elapsed().as_secs() < 60 {
                std::thread::sleep(Duration::from_millis(50));
            }
            std::thread::sleep(Duration::from_secs(3));
            timer.store(true, Ordering::Relaxed);
        });
        let mic_only = Tracks {
            mic: true,
            system: false,
        };
        let captured =
            record_with_levels(&base, stop, None, None, Some(&screen), mic_only).unwrap();
        assert_eq!(captured.files, [format!("{base}.mic.wav")]);
        assert_eq!(captured.video.unwrap().error, None);
        let probe = std::process::Command::new("ffprobe")
            .args(["-v", "error", "-show_entries", "stream=codec_name"])
            .args(["-of", "csv=p=0", &screen.path])
            .output();
        match probe {
            Ok(output) => {
                let codecs = String::from_utf8(output.stdout).unwrap();
                assert_eq!(codecs.lines().collect::<Vec<_>>(), ["h264", "aac"]);
            }
            Err(e) => eprintln!("ffprobe unavailable ({e}); track check skipped"),
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn video_only_with_a_tiny_area_fails_at_once() {
        // Without Screen Recording permission the call fails earlier, which is also immediate.
        let display = SCShareableContent::get()
            .ok()
            .and_then(|c| c.displays().into_iter().next())
            .map_or(0, |d| d.display_id());
        let dir = std::env::temp_dir().join(format!("motetto-tiny-area-{}", std::process::id()));
        let base = dir.join("part-0000").to_string_lossy().into_owned();
        let screen = ScreenCapture {
            target: ScreenTarget::Area {
                display,
                rect: video::AreaRect {
                    x: 0.0,
                    y: 0.0,
                    width: 20.0,
                    height: 20.0,
                },
            },
            preset: video::Preset::Smaller,
            cursor: false,
            clicks: false,
            path: format!("{base}.screen.mp4"),
            scale: 1.0,
            progress: None,
        };
        let silent = Tracks {
            mic: false,
            system: false,
        };
        // A backstop stop after 60 s; only an immediate error ends the call before it.
        let stop = Stop::default();
        let timer = stop.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(60));
            timer.store(true, Ordering::Relaxed);
        });
        let started = Instant::now();
        let result = record_with_levels(&base, stop, None, None, Some(&screen), silent);
        let Err(error) = result else {
            panic!("a 20 by 20 point area was recorded");
        };
        let error = error.to_string();
        assert!(display == 0 || error.contains("too small"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(40));
        assert!(!std::path::Path::new(&screen.path).exists());
    }

    #[test]
    fn lost_window_ends_the_wait_like_a_stop() {
        let (_sender, receiver) = mpsc::channel();
        let stop = Stop::default();
        assert_eq!(
            wait_for_stop(&stop, &receiver, &AtomicBool::new(true), || Ok(())),
            Ok(())
        );
        assert!(!stop.load(Ordering::Relaxed));
    }

    #[test]
    fn stop_signal_and_write_error_end_the_wait() {
        let (sender, receiver) = mpsc::channel();
        let stop = Stop::default();
        sender.send(Err("disk full".into())).unwrap();
        assert_eq!(
            wait_for_stop(&stop, &receiver, &AtomicBool::new(false), || Ok(())),
            Err("disk full".into())
        );
        stop.store(true, Ordering::Relaxed);
        assert_eq!(
            wait_for_stop(&stop, &receiver, &AtomicBool::new(false), || Ok(())),
            Ok(())
        );
    }

    #[test]
    fn periodic_flush_failure_reaches_the_recorder() {
        let (_sender, receiver) = mpsc::channel();
        assert_eq!(
            wait_for_stop(
                &Stop::default(),
                &receiver,
                &AtomicBool::new(false),
                || Err("flush failed".into())
            ),
            Err("flush failed".into())
        );
    }
}
