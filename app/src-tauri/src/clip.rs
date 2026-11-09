//! Save a clip (SPEC 15): the commands behind the panel. The audio work is in
//! `motetto_core::clip`; this finds the raw parts, keeps previews in the app cache and saves
//! through a save panel like Export MP4. The meeting folder is only read.
use crate::{library, library_commands::Library};
use motetto_core::clip::{self, Part, Track};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;
use ts_rs::TS;

pub const IMPORTED: &str = "Save a clip works on recordings made in Motetto.";

/// The imported file of an import's folder (`source.mp3`, `source.wav`...), not its `.16k.wav`
/// copy or whisper's `source.csv`.
pub(crate) fn imported_source(dir: &Path) -> Option<PathBuf> {
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|path| {
            path.is_file()
                && path.file_stem().and_then(|s| s.to_str()) == Some("source")
                && path.extension().and_then(|s| s.to_str()) != Some("csv")
        })
}

/// An import has no recorded parts: its file, decoded once at its own rate into the app cache,
/// is one Them part (the transcript and the waveform call it Them too).
fn imported_parts(app: &tauri::AppHandle, dir: &Path, id: &str) -> Result<Vec<Part>, String> {
    static DECODING: Mutex<()> = Mutex::new(());
    let source = imported_source(dir).ok_or(IMPORTED)?;
    let cache = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("clip-sources");
    let wav = cache.join(format!("{id}.wav"));
    {
        // Levels and the preview ask at once; one decodes, the other waits for its file.
        let _one = DECODING.lock().map_err(|e| e.to_string())?;
        if !wav.is_file() {
            fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
            let partial = cache.join(format!("{id}.partial.wav"));
            motetto_core::decode_as_recorded(&source, &partial)
                .and_then(|()| Ok(fs::rename(&partial, &wav)?))
                .map_err(|e| {
                    let _ = fs::remove_file(&partial);
                    e.to_string()
                })?;
        }
    }
    let reader = hound::WavReader::open(&wav).map_err(|e| e.to_string())?;
    Ok(vec![Part {
        offset_ms: 0,
        duration_ms: u64::from(reader.duration()) * 1000
            / u64::from(reader.spec().sample_rate.max(1)),
        mic: None,
        system: Some(wav),
    }])
}

#[derive(Debug, Serialize, TS)]
pub struct ClipPreview {
    pub path: String,
    pub rate: u32,
    pub bits: u16,
    pub float: bool,
    /// None when the clip is silent (no peak to measure).
    pub peak_db: Option<f32>,
    #[ts(type = "number")]
    pub clipped: u64,
    #[ts(type = "number")]
    pub duration_ms: u64,
}

/// The recording's parts with their raw track files. Only `part-NNNN.mic.wav` and
/// `part-NNNN.system.wav` count: the echo-removed `.mic.aec.wav` and the `.16k.wav` copies are
/// for playback and transcription, not for a clip of what was recorded.
pub(crate) fn parts(dir: &Path) -> Result<Vec<Part>, String> {
    let listed: Vec<(Vec<String>, u64)> = crate::screen_files::read(dir, "parts.json");
    if listed.is_empty() {
        return Err(IMPORTED.into());
    }
    let mut parts = Vec::new();
    for (files, offset_ms) in listed {
        let pick = |suffix: &str| {
            files
                .iter()
                .filter_map(|f| Path::new(f).file_name()?.to_str())
                .find(|name| {
                    name.ends_with(suffix) && !name.contains(".aec.") && !name.contains(".16k.")
                })
                .map(|name| dir.join(name))
                .filter(|path| path.is_file())
        };
        let (mic, system) = (pick(".mic.wav"), pick(".system.wav"));
        let duration_ms = [&mic, &system]
            .into_iter()
            .flatten()
            .filter_map(|path| hound::WavReader::open(path).ok())
            .map(|r| u64::from(r.duration()) * 1000 / u64::from(r.spec().sample_rate.max(1)))
            .max()
            .unwrap_or(0);
        parts.push(Part {
            offset_ms,
            duration_ms,
            mic,
            system,
        });
    }
    Ok(parts)
}

fn total_ms(parts: &[Part]) -> u64 {
    parts
        .iter()
        .map(|p| p.offset_ms + p.duration_ms)
        .max()
        .unwrap_or(0)
}

/// `<title> mm-ss.wav`, the start of the clip in the name so several clips of one meeting sort.
pub(crate) fn clip_file_name(title: &str, from_ms: u64) -> String {
    let s = from_ms / 1000;
    format!(
        "{} {:02}-{:02}.wav",
        crate::video_export::clean_title(title, "Clip"),
        s / 60,
        s % 60
    )
}

/// Writes the clip beside `dst` and renames it over, then the words with the same stem; a
/// failure leaves no partial file and no words for a clip that wasn't saved.
pub(crate) fn save_to(
    parts: &[Part],
    track: Track,
    from_ms: u64,
    to_ms: u64,
    words: Option<&str>,
    dst: &Path,
    write: impl Fn(&[Part], Track, u64, u64, &Path) -> Result<(), String>,
) -> Result<(), String> {
    let name = dst
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Choose a file name")?;
    let partial = dst.with_file_name(format!(".{name}.partial.wav"));
    let result = write(parts, track, from_ms, to_ms, &partial)
        .and_then(|()| fs::rename(&partial, dst).map_err(|e| e.to_string()));
    if result.is_err() {
        let _ = fs::remove_file(&partial);
        return result;
    }
    if let Some(words) = words.map(str::trim).filter(|w| !w.is_empty()) {
        let txt = dst.with_extension("txt");
        let tmp = dst.with_file_name(format!(".{name}.partial.txt"));
        fs::write(&tmp, format!("{words}\n"))
            .and_then(|()| fs::rename(&tmp, &txt))
            .map_err(|e| {
                let _ = fs::remove_file(&tmp);
                e.to_string()
            })?;
    }
    Ok(())
}

fn previews(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("clips"))
}

/// Removes every preview and decoded import (on close, on another meeting, and at start).
pub(crate) fn clear_previews(app: &tauri::AppHandle) {
    if let Ok(dir) = previews(app) {
        let _ = fs::remove_dir_all(dir.with_file_name("clip-sources"));
        let _ = fs::remove_dir_all(dir);
    }
}

fn load(app: &tauri::AppHandle, id: &str) -> Result<(Vec<Part>, library::MeetingSummary), String> {
    let root = app.state::<Library>().root.clone();
    let summary = library::summary(&root, id)?;
    let dir = library::folder(&root, id)?;
    let parts = match parts(&dir) {
        Err(e) if e == IMPORTED => imported_parts(app, &dir, id)?,
        other => other?,
    };
    Ok((parts, summary))
}

fn checked(parts: &[Part], track: &str, from_ms: u64, to_ms: u64) -> Result<Track, String> {
    clip::check_range(from_ms, to_ms, total_ms(parts))?;
    Track::parse(track)
}

#[tauri::command]
pub async fn clip_levels(
    app: tauri::AppHandle,
    id: String,
    track: String,
    from_ms: u64,
    to_ms: u64,
    bins: usize,
) -> Result<Vec<f32>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (parts, _) = load(&app, &id)?;
        let to_ms = to_ms.min(total_ms(&parts));
        if to_ms.saturating_sub(from_ms) > clip::MAX_MS + 10_000 {
            return Err("A clip can be at most 10 minutes".into());
        }
        clip::levels(&parts, Track::parse(&track)?, from_ms, to_ms, bins)
    })
    .await
    .map_err(|e| e.to_string())?
}

static PREVIEW: AtomicU64 = AtomicU64::new(0);

/// The exact clip, written into the app cache so the panel can play what Save would write.
#[tauri::command]
pub async fn clip_preview(
    app: tauri::AppHandle,
    id: String,
    track: String,
    from_ms: u64,
    to_ms: u64,
) -> Result<ClipPreview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (parts, _) = load(&app, &id)?;
        let track = checked(&parts, &track, from_ms, to_ms)?;
        let dir = previews(&app)?;
        // One preview at a time: the old one goes before the new one is written.
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(format!(
            "preview-{}.wav",
            PREVIEW.fetch_add(1, Ordering::Relaxed)
        ));
        clip::write(&parts, track, from_ms, to_ms, &path)?;
        let facts = clip::measure(&path)?;
        app.asset_protocol_scope()
            .allow_file(&path)
            .map_err(|e| e.to_string())?;
        Ok(ClipPreview {
            path: path.to_string_lossy().into_owned(),
            rate: facts.rate,
            bits: facts.bits,
            float: facts.float,
            peak_db: facts.peak_db.is_finite().then_some(facts.peak_db),
            clipped: facts.clipped,
            duration_ms: facts.duration_ms,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn clip_preview_clear(app: tauri::AppHandle) {
    clear_previews(&app);
}

/// Asks where to save, then writes the clip and its words; None when the panel was cancelled.
#[tauri::command]
pub async fn save_clip(
    app: tauri::AppHandle,
    id: String,
    track: String,
    from_ms: u64,
    to_ms: u64,
    words: Option<String>,
) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (parts, summary) = load(&app, &id)?;
        let track = checked(&parts, &track, from_ms, to_ms)?;
        let Some(file) = app
            .dialog()
            .file()
            .add_filter("WAV audio", &["wav"])
            .set_file_name(clip_file_name(&summary.title, from_ms))
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let dst = file.into_path().map_err(|e| e.to_string())?;
        // Trim moves the parts while it runs; the same flag keeps a clip from reading half of them.
        let library = app.state::<Library>();
        if library.busy.swap(true, Ordering::SeqCst) {
            return Err("Wait until recording and processing finish before saving a clip.".into());
        }
        let result = save_to(
            &parts,
            track,
            from_ms,
            to_ms,
            words.as_deref(),
            &dst,
            clip::write,
        );
        library.busy.store(false, Ordering::SeqCst);
        result?;
        if let Ok(mut saved) = SAVED.lock() {
            *saved = Some(dst.clone());
        }
        Ok(Some(dst.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The last clip saved in this run; Show in Finder reveals only that, never a path from the page.
static SAVED: Mutex<Option<PathBuf>> = Mutex::new(None);

#[tauri::command]
pub fn reveal_clip() -> Result<(), String> {
    let saved = SAVED.lock().map_err(|e| e.to_string())?.clone();
    let path = saved.ok_or("No clip was saved yet")?;
    // Select the file in its folder; `open <file>` alone would play it instead.
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("open")
        .arg("-R")
        .arg(&path)
        .status();
    #[cfg(target_os = "windows")]
    let status = std::process::Command::new("explorer")
        .arg(format!("/select,{}", path.display()))
        .status();
    match status {
        Ok(_) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("motetto-appclip-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn wav(path: &Path, rate: u32, frames: u32) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for _ in 0..frames {
            w.write_sample(0.1f32).unwrap();
        }
        w.finalize().unwrap();
    }

    #[test]
    fn parts_use_the_raw_tracks_only() {
        let dir = temp("parts");
        for name in [
            "part-0000.mic.wav",
            "part-0000.system.wav",
            "part-0000.mic.aec.wav",
            "part-0000.mic.16k.wav",
            "part-0001.mic.wav",
        ] {
            wav(&dir.join(name), 1000, 2000);
        }
        fs::write(
            dir.join("parts.json"),
            r#"[[["part-0000.mic.16k.wav","part-0000.mic.aec.wav","part-0000.mic.wav","part-0000.system.wav"],0],[["/elsewhere/part-0001.mic.wav"],5000]]"#,
        )
        .unwrap();
        let got = parts(&dir).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(
            got[0].mic.as_deref(),
            Some(dir.join("part-0000.mic.wav").as_path())
        );
        assert_eq!(
            got[0].system.as_deref(),
            Some(dir.join("part-0000.system.wav").as_path())
        );
        assert_eq!(
            (got[0].duration_ms, got[1].offset_ms, got[1].system.clone()),
            (2000, 5000, None)
        );
        assert_eq!(total_ms(&got), 7000);
    }

    #[test]
    fn an_imported_meeting_has_no_parts() {
        assert_eq!(parts(&temp("imported")), Err(IMPORTED.to_string()));
    }

    #[test]
    fn an_import_clips_its_own_file() {
        let dir = temp("source");
        assert_eq!(imported_source(&dir), None);
        for name in [
            "source.16k.wav",
            "source.csv",
            "transcript.txt",
            "source.mp3",
        ] {
            fs::write(dir.join(name), "x").unwrap();
        }
        assert_eq!(imported_source(&dir), Some(dir.join("source.mp3")));
    }

    #[test]
    fn file_name_holds_the_start() {
        assert_eq!(
            clip_file_name("FCM vs Pusher: Beams", 74_900),
            "FCM vs Pusher- Beams 01-14.wav"
        );
        assert_eq!(clip_file_name("  ", 0), "Clip 00-00.wav");
    }

    #[test]
    fn save_writes_through_a_partial_and_the_words_only_after() {
        let dir = temp("save");
        wav(&dir.join("m.wav"), 1000, 2000);
        let parts = [Part {
            offset_ms: 0,
            duration_ms: 2000,
            mic: Some(dir.join("m.wav")),
            system: None,
        }];
        let dst = dir.join("Clip 00-00.wav");
        save_to(
            &parts,
            Track::Me,
            0,
            1000,
            Some(" hello there "),
            &dst,
            clip::write,
        )
        .unwrap();
        assert_eq!(hound::WavReader::open(&dst).unwrap().duration(), 1000);
        assert_eq!(
            fs::read_to_string(dir.join("Clip 00-00.txt")).unwrap(),
            "hello there\n"
        );

        let failed = dir.join("Failed 00-00.wav");
        let broken = |_: &[Part], _: Track, _: u64, _: u64, path: &Path| {
            fs::write(path, b"half").unwrap();
            Err("disk full".to_string())
        };
        assert_eq!(
            save_to(&parts, Track::Me, 0, 1000, Some("words"), &failed, broken),
            Err("disk full".into())
        );
        let mut names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, ["Clip 00-00.txt", "Clip 00-00.wav", "m.wav"]);
    }

    #[test]
    fn export_bindings() {
        ClipPreview::export_all(&ts_rs::Config::default().with_out_dir("../../ui/lib/bindings"))
            .unwrap();
    }
}
