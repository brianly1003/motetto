//! Local playback manifests and bounded waveform previews for the document view.
use crate::{library, library_commands::Library};
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
pub struct MeetingMedia {
    pub tracks: Vec<AudioTrack>,
    #[ts(type = "number")]
    pub duration_ms: u64,
    pub folder: String,
}

fn files(dir: &Path) -> Result<Vec<(PathBuf, u64)>, String> {
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
                // Plays the mic without loudspeaker echo when transcribing made one, so the
                // audio says what the transcript says; the recorded mic stays as it was.
                let file = dir.join(name);
                let cleaned = file.with_extension("aec.wav");
                files.push((if cleaned.exists() { cleaned } else { file }, offset));
            }
        }
    } else if dir.join("source.16k.wav").exists() {
        files.push((dir.join("source.16k.wav"), 0));
    }
    Ok(files)
}
pub fn duration(root: &Path, id: &str) -> Result<Option<u64>, String> {
    let dir = library::folder(root, id)?;
    let mut duration = None;
    for (file, offset) in files(&dir)? {
        if !file.try_exists().map_err(|e| e.to_string())? {
            continue;
        }
        let path = file.canonicalize().map_err(|e| e.to_string())?;
        if path.parent() != Some(dir.as_path()) {
            return Err("Audio points outside its meeting".into());
        }
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

pub fn read(root: &Path, id: &str) -> Result<MeetingMedia, String> {
    let dir = library::folder(root, id)?;
    let files = files(&dir)?;
    let mut tracks = Vec::new();
    for (file, offset_ms) in files {
        if !file.try_exists().map_err(|e| e.to_string())? {
            continue;
        }
        let path = file.canonicalize().map_err(|e| e.to_string())?;
        if path.parent() != Some(dir.as_path()) {
            return Err("Audio points outside its meeting".into());
        }
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
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
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
    let duration_ms = tracks
        .iter()
        .map(|t| t.offset_ms.saturating_add(t.duration_ms))
        .max()
        .unwrap_or(0);
    Ok(MeetingMedia {
        tracks,
        duration_ms,
        folder: dir.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub async fn meeting_media(app: tauri::AppHandle, id: String) -> Result<MeetingMedia, String> {
    let root = app.state::<Library>().root.clone();
    let media = tauri::async_runtime::spawn_blocking(move || read(&root, &id))
        .await
        .map_err(|e| e.to_string())??;
    for track in &media.tracks {
        app.asset_protocol_scope()
            .allow_file(&track.path)
            .map_err(|e| e.to_string())?;
    }
    Ok(media)
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
