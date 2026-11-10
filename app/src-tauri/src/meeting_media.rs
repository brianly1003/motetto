//! Local playback manifests and bounded waveform previews for the document view.
use crate::{library, library_commands::Library};
use motetto_core::transcribe::MicMode;
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::Manager;
use ts_rs::TS;

#[derive(Debug, Serialize, TS)]
pub struct AudioTrack {
    pub path: String,
    pub name: String,
    pub speaker: String,
    #[ts(type = "number")]
    pub offset_ms: u64,
    #[ts(type = "number")]
    pub duration_ms: u64,
    #[ts(type = "number")]
    pub bytes: u64,
    pub waveform: Vec<f32>,
}
#[derive(Debug, Serialize, TS)]
pub struct VideoPart {
    pub path: String,
    #[ts(type = "number")]
    pub offset_ms: u64,
    #[ts(type = "number")]
    pub duration_ms: u64,
    pub source: String,
    /// `file` for an imported video, else what was recorded (`window`, `display`).
    pub kind: String,
    pub while_sharing: bool,
    pub width: u32,
    pub height: u32,
    #[ts(type = "number")]
    pub bytes: u64,
}
#[derive(Debug, Serialize, TS)]
pub struct MeetingMedia {
    pub tracks: Vec<AudioTrack>,
    pub videos: Vec<VideoPart>,
    pub moments: Vec<crate::screen::Moment>,
    /// How the mic was prepared for the transcript (Settings › Audio, or Transcribe again).
    pub mic_mode: MicMode,
    /// A processed mic copy exists, so the Audio tab can switch between it and the recorded mic.
    pub mic_cleaned: bool,
    /// Mouse clicks while a screen recording ran, in meeting time (the scrubber's dots).
    #[ts(type = "Array<number>")]
    pub clicks: Vec<u64>,
    #[ts(type = "number")]
    pub duration_ms: u64,
    pub folder: String,
}

/// The mode the meeting's transcript was made with; Standard when `meeting.json` doesn't say.
fn mic_mode(dir: &Path) -> MicMode {
    library::text(&dir.join("meeting.json"))
        .ok()
        .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
        .and_then(|meeting| serde_json::from_value(meeting["mic_mode"].clone()).ok())
        .unwrap_or_default()
}

/// The mic file to play for `mode`: the copy transcribing made (the denoised one for Reduce
/// noise, else the echo-cancelled one), or the recorded mic for Original and when there is none.
/// Chosen by the meeting's mode, never by whichever copy exists: a copy left by another mode
/// must not play.
fn mic_file(file: PathBuf, mode: MicMode, original: bool) -> PathBuf {
    let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if original || mode == MicMode::Original || !name.ends_with(".mic.wav") {
        return file;
    }
    let clean = file.with_extension("clean.wav");
    let aec = file.with_extension("aec.wav");
    if mode == MicMode::ReduceNoise && clean.exists() {
        clean
    } else if aec.exists() {
        aec
    } else {
        file
    }
}

fn files(dir: &Path, original: bool) -> Result<Vec<(PathBuf, u64)>, String> {
    let mode = mic_mode(dir);
    let mut files = Vec::new();
    if dir.join("parts.json").exists() {
        let parts: Vec<(Vec<String>, u64)> =
            serde_json::from_str(&library::text(&dir.join("parts.json"))?)
                .map_err(|e| e.to_string())?;
        for (paths, offset) in parts {
            for path in paths {
                // Stored paths can precede a folder move. Only the local basename is used.
                let name = Path::new(&path)
                    .file_name()
                    .ok_or("Invalid audio filename")?;
                // Plays the mic as transcribing prepared it, so the audio says what the
                // transcript says; the recorded mic stays as it was.
                files.push((mic_file(dir.join(name), mode, original), offset));
            }
        }
    } else if dir.join("source.16k.wav").exists() {
        files.push((dir.join("source.16k.wav"), 0));
    }
    Ok(files)
}
/// The meeting's existing audio files with their offsets, each checked to sit in its folder.
pub fn audio(dir: &Path) -> Result<Vec<(PathBuf, u64)>, String> {
    audio_as(dir, false)
}
/// `audio`, with the recorded mic in place of its processed copy when `original`.
pub fn audio_as(dir: &Path, original: bool) -> Result<Vec<(PathBuf, u64)>, String> {
    let mut checked = Vec::new();
    for (file, offset) in files(dir, original)? {
        if !file.try_exists().map_err(|e| e.to_string())? {
            continue;
        }
        let path = file.canonicalize().map_err(|e| e.to_string())?;
        if path.parent() != Some(dir) {
            return Err("Audio points outside its meeting".into());
        }
        checked.push((path, offset));
    }
    Ok(checked)
}
pub fn duration(root: &Path, id: &str) -> Result<Option<u64>, String> {
    let dir = library::folder(root, id)?;
    let mut duration = None;
    for (path, offset) in audio(&dir)? {
        let reader = hound::WavReader::open(path).map_err(|e| e.to_string())?;
        if reader.spec().sample_rate == 0 {
            return Err("Invalid audio rate".into());
        }
        let end = offset.saturating_add(
            u64::from(reader.duration()) * 1000 / u64::from(reader.spec().sample_rate),
        );
        duration = Some(duration.unwrap_or(0).max(end));
    }
    Ok(duration)
}

fn video_parts(dir: &Path) -> Vec<(crate::screen::ScreenPart, PathBuf)> {
    let parts: Vec<crate::screen::ScreenPart> = crate::screen_files::read(dir, "screen.json");
    parts
        .into_iter()
        .filter_map(|part| {
            if !crate::screen_files::video_name(&part.file)
                && !crate::screen_files::imported_video(&part.file)
            {
                return None;
            }
            let path = crate::screen_files::safe_file(dir, &part.file)?;
            Some((part, path))
        })
        .collect()
}

/// Initial document tabs need availability, without reading audio samples for waveforms.
pub fn has_video(dir: &Path) -> bool {
    if audio(dir).is_err() {
        return false;
    }
    // A part recorded with no mic and no app sounds has a picture and no audio of its own.
    video_parts(dir)
        .into_iter()
        .any(|(_, path)| fs::metadata(path).is_ok_and(|m| m.len() > 0))
}

#[cfg(test)]
pub fn read(root: &Path, id: &str) -> Result<MeetingMedia, String> {
    read_as(root, id, false)
}
/// `read`, with the recorded mic in place of its processed copy when `original`.
pub fn read_as(root: &Path, id: &str, original: bool) -> Result<MeetingMedia, String> {
    read_with(root, id, original, motetto_core::trim::duration_ms)
}

/// `length` reads a video file's length (AVFoundation; tests stand in for it).
fn read_with(
    root: &Path,
    id: &str,
    original: bool,
    length: fn(&Path) -> Result<u64, String>,
) -> Result<MeetingMedia, String> {
    let dir = library::folder(root, id)?;
    let mut tracks = Vec::new();
    let played = audio_as(&dir, original)?;
    let mic_cleaned = audio_as(&dir, false)? != audio_as(&dir, true)?;
    for (path, offset_ms) in played {
        let mut reader = hound::WavReader::open(&path).map_err(|e| e.to_string())?;
        let spec = reader.spec();
        if spec.sample_rate == 0 || spec.channels == 0 {
            return Err("Invalid audio format".into());
        }
        let frames = reader.duration();
        let duration_ms = u64::from(frames) * 1000 / u64::from(spec.sample_rate);
        let mut waveform = Vec::with_capacity(200);
        // Peak of 8 windows of 25 ms spread across each bin: one 5 ms window at the bin start
        // mostly landed between syllables, so speech drew as a flat line. Bounded reads only
        // (about 40 s of audio per track), never the whole meeting.
        let count = (spec.sample_rate as usize / 40).max(1) * usize::from(spec.channels);
        for bin in 0..200u64 {
            let mut peak = 0.0f32;
            for window in 0..8u64 {
                reader
                    .seek((u64::from(frames) * (bin * 8 + window) / 1600) as u32)
                    .map_err(|e| e.to_string())?;
                peak = if spec.sample_format == hound::SampleFormat::Float {
                    reader
                        .samples::<f32>()
                        .take(count)
                        .try_fold(peak, |peak, value| value.map(|v| peak.max(v.abs())))
                } else {
                    let scale = 2f32.powi(i32::from(spec.bits_per_sample) - 1);
                    reader
                        .samples::<i32>()
                        .take(count)
                        .try_fold(peak, |peak, value| {
                            value.map(|v| peak.max((v as f32 / scale).abs()))
                        })
                }
                .map_err(|e| e.to_string())?;
            }
            waveform.push(if peak.is_finite() {
                peak.clamp(0.0, 1.0)
            } else {
                0.0
            });
        }
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let speaker = if name.contains(".mic.") {
            "You"
        } else if name.contains(".system.") {
            "Them"
        } else {
            "Audio"
        };
        tracks.push(AudioTrack {
            bytes: fs::metadata(&path).map_err(|e| e.to_string())?.len(),
            path: path.to_string_lossy().into_owned(),
            name,
            speaker: speaker.into(),
            offset_ms,
            duration_ms,
            waveform,
        });
    }
    let mut videos = vec![];
    for (part, path) in video_parts(&dir) {
        let heard = tracks
            .iter()
            .filter(|t| {
                t.offset_ms == part.offset_ms
                    && t.name.split('.').next() == part.file.split('.').next()
            })
            .map(|t| t.duration_ms)
            .max();
        // A part without audio of its own (no mic, no app sounds) is placed by its video's length.
        let length = heard.or_else(|| length(&path).ok());
        let Some(duration_ms) = length else {
            continue;
        };
        let bytes = fs::metadata(&path).map_err(|e| e.to_string())?.len();
        if bytes == 0 {
            continue;
        }
        videos.push(VideoPart {
            path: path.to_string_lossy().into_owned(),
            offset_ms: part.offset_ms,
            duration_ms,
            source: part.source.name,
            kind: part.source.kind,
            while_sharing: part.while_sharing,
            width: part.width,
            height: part.height,
            bytes,
        });
    }
    videos.sort_by_key(|v| v.offset_ms);
    let duration_ms = tracks
        .iter()
        .map(|t| t.offset_ms.saturating_add(t.duration_ms))
        .chain(
            videos
                .iter()
                .map(|v| v.offset_ms.saturating_add(v.duration_ms)),
        )
        .max()
        .unwrap_or(0);
    let shown = |ms: u64| {
        videos
            .iter()
            .any(|v| ms >= v.offset_ms && ms < v.offset_ms.saturating_add(v.duration_ms))
    };
    let moments: Vec<crate::screen::Moment> =
        crate::screen_files::read(&dir, "screen-moments.json");
    let moments = moments.into_iter().filter(|m| shown(m.ms)).collect();
    let clicks: Vec<crate::trim::Click> = crate::screen_files::read(&dir, "screen-clicks.json");
    let clicks = clicks
        .into_iter()
        .map(|c| c.ms)
        .filter(|ms| shown(*ms))
        .collect();
    Ok(MeetingMedia {
        mic_mode: mic_mode(&dir),
        mic_cleaned,
        videos,
        moments,
        clicks,
        tracks,
        duration_ms,
        folder: dir.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub async fn meeting_media(
    app: tauri::AppHandle,
    id: String,
    original: Option<bool>,
) -> Result<MeetingMedia, String> {
    let root = app.state::<Library>().root.clone();
    let original = original.unwrap_or(false);
    let media = tauri::async_runtime::spawn_blocking(move || read_as(&root, &id, original))
        .await
        .map_err(|e| e.to_string())??;
    for path in media
        .tracks
        .iter()
        .map(|t| &t.path)
        .chain(media.videos.iter().map(|v| &v.path))
    {
        app.asset_protocol_scope()
            .allow_file(path)
            .map_err(|e| e.to_string())?;
    }
    Ok(media)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mic_playback_follows_the_meetings_mode() {
        let dir = std::env::temp_dir().join(format!("motetto-micmode-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mic = dir.join("part-0000.mic.wav");
        let (aec, clean) = (
            dir.join("part-0000.mic.aec.wav"),
            dir.join("part-0000.mic.clean.wav"),
        );
        for file in [&mic, &aec, &clean] {
            fs::write(file, b"").unwrap();
        }
        let play = |mode, original| mic_file(mic.clone(), mode, original);
        // Original plays the recorded mic even when copies exist.
        assert_eq!(play(MicMode::Original, false), mic);
        // Standard ignores a denoised copy left by an earlier Reduce noise.
        assert_eq!(play(MicMode::Standard, false), aec);
        assert_eq!(play(MicMode::ReduceNoise, false), clean);
        // The Audio tab's Original switch, whatever the mode.
        assert_eq!(play(MicMode::ReduceNoise, true), mic);
        // Without its copy, Reduce noise falls back to the echo-cancelled mic, then the recording.
        fs::remove_file(&clean).unwrap();
        assert_eq!(play(MicMode::ReduceNoise, false), aec);
        fs::remove_file(&aec).unwrap();
        assert_eq!(play(MicMode::ReduceNoise, false), mic);
        // The call side is never swapped.
        let system = dir.join("part-0000.system.wav");
        assert_eq!(
            mic_file(system.clone(), MicMode::ReduceNoise, false),
            system
        );
        // The mode comes from meeting.json; missing or unknown is Standard.
        assert_eq!(mic_mode(&dir), MicMode::Standard);
        fs::write(dir.join("meeting.json"), r#"{"mic_mode":"reduce_noise"}"#).unwrap();
        assert_eq!(mic_mode(&dir), MicMode::ReduceNoise);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn waveform_duration_offsets_and_scope() {
        let root = std::env::temp_dir().join(format!("motetto-media-{}", std::process::id()));
        fs::create_dir_all(root.join("1")).unwrap();
        let path = root.join("1/part-0000.mic.wav");
        let mut wav = hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: 1,
                sample_rate: 16000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..16000 {
            wav.write_sample(16384i16).unwrap();
        }
        wav.finalize().unwrap();
        fs::write(
            root.join("1/parts.json"),
            serde_json::to_vec(&vec![(vec![path.to_string_lossy().into_owned()], 500u64)]).unwrap(),
        )
        .unwrap();
        let media = read(&root, "1").unwrap();
        assert_eq!(media.duration_ms, 1500);
        assert_eq!(media.tracks[0].speaker, "You");
        assert_eq!(media.tracks[0].waveform.len(), 200);
        assert!(media.tracks[0].waveform.iter().all(|v| *v == 0.5));
        assert!(media.videos.is_empty());
        let dir = library::folder(&root, "1").unwrap();
        assert!(!has_video(&dir));
        let video = "part-0000.screen.mp4";
        fs::write(root.join("1").join(video), b"video fixture").unwrap();
        crate::screen_files::write(
            &root.join("1"),
            "screen.json",
            &vec![crate::screen::ScreenPart {
                file: video.into(),
                offset_ms: 500,
                source: crate::screen::ScreenSourceInfo {
                    kind: "window".into(),
                    name: "Zoom window".into(),
                },
                while_sharing: false,
                width: 1280,
                height: 720,
            }],
        )
        .unwrap();
        assert!(has_video(&dir));
        fs::write(
            root.join("1/meeting.json"),
            r#"{"id":"1","title":"Example","created_ms":0,"status":"Ready"}"#,
        )
        .unwrap();
        assert!(library::read(&root, "1").unwrap().has_video);
        assert!(library::list(&root).unwrap()[0].has_video);
        assert!(
            library::search(&root, "Example").unwrap()[0]
                .meeting
                .has_video
        );
        crate::screen_files::delete(&root, "1").unwrap();
        assert!(!library::list(&root).unwrap()[0].has_video);
        crate::screen_files::restore(&root, "1").unwrap();
        assert!(library::list(&root).unwrap()[0].has_video);
        fs::write(root.join("1").join(video), b"").unwrap();
        assert!(!has_video(&dir));
        fs::write(root.join("1").join(video), b"video fixture").unwrap();
        let mut parts: Vec<crate::screen::ScreenPart> =
            crate::screen_files::read(&root.join("1"), "screen.json");
        // A part with no audio of its own (no mic, no app sounds) still has its picture.
        parts[0].offset_ms = 999;
        crate::screen_files::write(&root.join("1"), "screen.json", &parts).unwrap();
        assert!(has_video(&dir));
        parts[0].offset_ms = 500;
        crate::screen_files::write(&root.join("1"), "screen.json", &parts).unwrap();
        let with_video = read(&root, "1").unwrap();
        assert_eq!(with_video.videos.len(), 1);
        assert_eq!(with_video.videos[0].offset_ms, 500);
        assert_eq!(with_video.videos[0].duration_ms, 1000);
        assert!(with_video.clicks.is_empty());
        // Clicks become dots only where there is a picture.
        fs::write(
            root.join("1/screen-clicks.json"),
            r#"[{"ms":600},{"ms":5000}]"#,
        )
        .unwrap();
        assert_eq!(read(&root, "1").unwrap().clicks, vec![600]);
        fs::remove_file(root.join("1").join(video)).unwrap();
        assert!(read(&root, "1").unwrap().videos.is_empty());
        assert!(!library::read(&root, "1").unwrap().has_video);
        let mut stale = library::summary(&root, "1").unwrap();
        stale.has_video = true;
        library::save(&root, &stale).unwrap();
        assert!(!library::list(&root).unwrap()[0].has_video);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&path, root.join("1").join(video)).unwrap();
            assert!(!has_video(&dir));
            fs::remove_file(root.join("1").join(video)).unwrap();
        }
        // The echo-cancelled mic plays in its place, still as You.
        fs::copy(&path, root.join("1/part-0000.mic.aec.wav")).unwrap();
        let media = read(&root, "1").unwrap();
        assert!(media.tracks[0].path.ends_with("part-0000.mic.aec.wav"));
        assert_eq!(media.tracks[0].speaker, "You");
        fs::remove_file(root.join("1/part-0000.mic.aec.wav")).unwrap();
        #[cfg(unix)]
        {
            fs::remove_file(&path).unwrap();
            std::os::unix::fs::symlink("/etc/passwd", &path).unwrap();
            assert!(read(&root, "1").unwrap_err().contains("outside"));
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn imported_video_plays_on_the_import_audio_clock() {
        let root =
            std::env::temp_dir().join(format!("motetto-media-import-{}", std::process::id()));
        fs::create_dir_all(root.join("1")).unwrap();
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut wav = hound::WavWriter::create(root.join("1/source.16k.wav"), spec).unwrap();
        for _ in 0..32000 {
            wav.write_sample(0i16).unwrap();
        }
        wav.finalize().unwrap();
        fs::write(root.join("1/source.mov"), b"video fixture").unwrap();
        let part = |file: &str| crate::screen::ScreenPart {
            file: file.into(),
            offset_ms: 0,
            source: crate::screen::ScreenSourceInfo {
                kind: "file".into(),
                name: "Demo.mov".into(),
            },
            while_sharing: false,
            width: 1920,
            height: 1080,
        };
        crate::screen_files::write(&root.join("1"), "screen.json", &vec![part("source.mov")])
            .unwrap();
        let dir = library::folder(&root, "1").unwrap();
        assert!(has_video(&dir));
        fs::write(
            root.join("1/meeting.json"),
            r#"{"id":"1","title":"Imported","created_ms":0,"status":"Ready"}"#,
        )
        .unwrap();
        assert!(library::list(&root).unwrap()[0].has_video);
        let media = read(&root, "1").unwrap();
        assert_eq!(media.videos.len(), 1);
        assert_eq!(
            (media.videos[0].duration_ms, media.videos[0].kind.as_str()),
            (2000, "file")
        );
        // Only the import's own copy plays, never another file named in the manifest.
        crate::screen_files::write(
            &root.join("1"),
            "screen.json",
            &vec![part("source.16k.wav")],
        )
        .unwrap();
        assert!(!has_video(&dir));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn waveform_sees_speech_between_bin_starts() {
        // 20 s, 100 ms per bin: a 30 ms burst 60 ms into every bin, silence elsewhere.
        let root = std::env::temp_dir().join(format!("motetto-media-burst-{}", std::process::id()));
        fs::create_dir_all(root.join("1")).unwrap();
        let path = root.join("1/part-0000.system.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut wav = hound::WavWriter::create(&path, spec).unwrap();
        for i in 0..320_000 {
            wav.write_sample(if (960..1440).contains(&(i % 1600)) {
                0.5f32
            } else {
                0.0
            })
            .unwrap();
        }
        wav.finalize().unwrap();
        fs::write(
            root.join("1/parts.json"),
            serde_json::to_vec(&vec![(vec![path.to_string_lossy().into_owned()], 0u64)]).unwrap(),
        )
        .unwrap();
        let media = read(&root, "1").unwrap();
        assert_eq!(media.tracks[0].speaker, "Them");
        assert!(media.tracks[0].waveform.iter().all(|v| *v == 0.5));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn export_bindings() {
        MeetingMedia::export_all(&ts_rs::Config::default().with_out_dir("../../ui/lib/bindings"))
            .unwrap();
    }
    #[test]
    fn a_silent_part_after_one_with_audio_keeps_its_place() {
        let root = std::env::temp_dir().join(format!("motetto-media-mixed-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("1")).unwrap();
        let wav = root.join("1/part-0000.system.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&wav, spec).unwrap();
        for _ in 0..32000 {
            writer.write_sample(100i16).unwrap();
        }
        writer.finalize().unwrap();
        crate::screen_files::write(
            &root.join("1"),
            "parts.json",
            &vec![(vec![wav.to_string_lossy().into_owned()], 0u64)],
        )
        .unwrap();
        // The second part was recorded after None, with app sounds off: no WAV of its own.
        fs::write(root.join("1/part-0000.screen.mp4"), "2000").unwrap();
        fs::write(root.join("1/part-0001.screen.mp4"), "3000").unwrap();
        let part = |file: &str, offset_ms| crate::screen::ScreenPart {
            file: file.into(),
            offset_ms,
            source: crate::screen::ScreenSourceInfo {
                kind: "window".into(),
                name: "Safari".into(),
            },
            while_sharing: false,
            width: 2,
            height: 2,
        };
        crate::screen_files::write(
            &root.join("1"),
            "screen.json",
            &vec![
                part("part-0000.screen.mp4", 0),
                part("part-0001.screen.mp4", 2000),
            ],
        )
        .unwrap();
        fs::write(
            root.join("1/screen-clicks.json"),
            r#"[{"ms":500},{"ms":4000}]"#,
        )
        .unwrap();
        let length = |path: &Path| -> Result<u64, String> {
            fs::read_to_string(path)
                .map_err(|e| e.to_string())?
                .parse()
                .map_err(|_| "not a length".to_string())
        };
        let media = read_with(&root, "1", false, length).unwrap();
        let spans: Vec<_> = media
            .videos
            .iter()
            .map(|v| (v.offset_ms, v.duration_ms))
            .collect();
        assert_eq!(spans, vec![(0, 2000), (2000, 3000)]);
        assert_eq!(media.duration_ms, 5000);
        assert_eq!(media.clicks, vec![500, 4000]);
        fs::remove_dir_all(root).unwrap();
    }
}
