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
pub use screen::{screen_windows, window_display};
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

/// Records both tracks until `stop` is set or writing fails; returns tracks that got audio.
pub fn record(base: &str, stop: Stop) -> Result<Vec<String>, Box<dyn Error>> {
    record_with_levels(base, stop, None, None, None).map(|captured| captured.files)
}

/// The desktop's recording: optional meters, and `mic` (a `mics::Mic` id) to record instead of
/// the system default input. Picture capture uses a separate stream so its filter never
/// changes the audio scope; picture errors are reported separately while WAV recording continues.
pub fn record_with_levels(
    base: &str,
    stop: Stop,
    meters: Option<Meters>,
    mic: Option<&str>,
    screen: Option<&ScreenCapture>,
) -> Result<Captured, Box<dyn Error>> {
    record_tracks(base, stop, meters, mic, screen, true)
}

/// The microphone alone (a rehearsal, SPEC 14.6): system audio is never captured, and only
/// `<base>.mic.wav` is written.
pub fn record_mic_only(
    base: &str,
    stop: Stop,
    mic: Option<&str>,
) -> Result<Captured, Box<dyn Error>> {
    record_tracks(base, stop, None, mic, None, false)
}

fn record_tracks(
    base: &str,
    stop: Stop,
    meters: Option<Meters>,
    mic: Option<&str>,
    screen: Option<&ScreenCapture>,
    system: bool,
) -> Result<Captured, Box<dyn Error>> {
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
        .with_captures_audio(system)
        .with_channel_count(1)
        .with_captures_microphone(true)?;
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
        let prepared = picture_filter(&content, screen).and_then(|(filter, width, height)| {
            let writer = Writer::create(
                std::path::Path::new(&screen.path),
                width,
                height,
                screen.preset,
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
            Err(error) => {
                if let Some(outcome) = outcome.as_mut() {
                    outcome.error = Some(error);
                }
            }
        }
    }
    let (errors, stopped) = mpsc::channel();
    let mut stream =
        SCStream::new_with_delegate(&audio_filter, &config, AudioDelegate(errors.clone()))?;
    let mut tracks = vec![];
    for (suffix, kind) in [
        ("system", SCStreamOutputType::Audio),
        ("mic", SCStreamOutputType::Microphone),
    ] {
        if suffix == "system" && !system {
            continue;
        }
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
            },
            kind,
        )?;
        tracks.push((path, wav));
    }

    if system {
        println!("Recording system audio + mic. Press Enter to stop.");
    }
    stream.start_capture()?;
    if let Some(picture) = picture.as_mut()
        && let Err(error) = picture.start_capture()
        && let Some(video) = &encoding
    {
        video.fail(error.to_string());
    }
    let mut reported = None;
    let mut video_done = false;
    let mut result = wait_for_stop(&stop, &stopped, &AtomicBool::new(false), || {
        for (path, wav) in &tracks {
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
    let stop_result = stream
        .stop_capture()
        .map_err(|e| format!("cannot stop capture: {e}"));
    // Readers load the last level; a stopped capture has none.
    for level in meters.iter().flat_map(|m| &m.levels) {
        level.store(0, Ordering::Relaxed);
    }
    result = result.and(stop_result);
    for pending in stopped.try_iter() {
        result = result.and(pending);
    }

    let mut files = vec![];
    for (path, wav) in tracks {
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
    if files.is_empty() {
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
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(2));
            timer.store(true, Ordering::SeqCst);
        });
        let captured = record_mic_only(&base, stop, None).unwrap();
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
        let result = record_with_levels(&base, stop, None, None, Some(&screen));
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
