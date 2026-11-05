//! Trim a saved screen recording at both ends without re-encoding (SPEC 12.7 `trim_video`,
//! `restore_trim`). The kept video and WAVs are built in `.trim-tmp/`, then replace the item's
//! parts; the full original waits in Recently deleted as `<id>-full` for 30 days. A second trim
//! keeps the first original, so restoring always gives back the whole recording.
use crate::{
    library::{self, DeletedKind, MeetingKind, MeetingSummary},
    library_commands::Library,
    meeting_actions::DELETED_AT,
    screen::{Moment, ScreenPart},
    screen_files::{self, move_files, safe_file},
};
use motetto_core::trim::{self as cut, TrimRange};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::Ordering,
};
use tauri::{Emitter, Manager};

/// A trimmed original's folder in Recently deleted is its meeting's id with this suffix.
pub(crate) const FULL: &str = "-full";
const TMP: &str = ".trim-tmp";
const VIDEO: &str = "part-0000.screen.mp4";
/// The manifests that place parts on the recording's timeline; a trim rewrites them.
const MANIFESTS: [&str; 4] = [
    "parts.json",
    "screen.json",
    "screen-moments.json",
    "screen-clicks.json",
];
/// The pre-trim summary, kept beside the original so a restore puts its times back.
const SUMMARY: &str = "meeting.json";

#[derive(Serialize, Deserialize)]
struct Click {
    ms: u64,
}

/// The meeting a Recently deleted entry `<id>-full` (a trimmed original) belongs to.
pub(crate) fn original_of(entry: &str) -> Option<&str> {
    let id = entry.strip_suffix(FULL)?;
    (!id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())).then_some(id)
}

/// The meeting's own folder, never a link to one elsewhere.
fn own_folder(root: &Path, id: &str) -> Result<PathBuf, String> {
    if !root.join(id).exists() {
        return Err("Restore the recording from Recently deleted first".into());
    }
    let dir = library::folder(root, id)?;
    if !root
        .join(id)
        .symlink_metadata()
        .map_err(|e| e.to_string())?
        .file_type()
        .is_dir()
    {
        return Err("Cannot change a linked meeting's video".into());
    }
    Ok(dir)
}

/// The full original in Recently deleted, when there is one; never a link.
fn original(root: &Path, id: &str) -> Result<Option<PathBuf>, String> {
    let path = crate::meeting_actions::bin(root)?.join(format!("{id}{FULL}"));
    match path.symlink_metadata() {
        Err(_) => Ok(None),
        Ok(meta) if meta.file_type().is_dir() => Ok(Some(path)),
        Ok(_) => Err("Invalid full video in Recently deleted".into()),
    }
}

/// Every part's media and the manifests: what a trim replaces and a restore puts back.
fn parts(dir: &Path) -> Result<Vec<String>, String> {
    let mut names = vec![];
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let name = entry
            .map_err(|e| e.to_string())?
            .file_name()
            .to_string_lossy()
            .into_owned();
        if name.starts_with("part-") || MANIFESTS.contains(&name.as_str()) {
            if safe_file(dir, &name).is_none() {
                return Err("Cannot move linked or invalid video files".into());
            }
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

/// Removes the new files a failed step left in `.trim-tmp`; old files held in `old/` stay.
fn clear(tmp: &Path) {
    for entry in fs::read_dir(tmp).into_iter().flatten().flatten() {
        if entry.file_name() != "old" {
            let _ = fs::remove_file(entry.path());
        }
    }
    let _ = fs::remove_dir(tmp.join("old"));
    let _ = fs::remove_dir(tmp);
}

/// An empty `.trim-tmp` in the meeting folder with an empty `old/` for the files replaced.
fn scratch(dir: &Path) -> Result<PathBuf, String> {
    let tmp = dir.join(TMP);
    if tmp.symlink_metadata().is_ok() {
        if !tmp.symlink_metadata().is_ok_and(|m| m.is_dir()) {
            return Err("Invalid trim folder".into());
        }
        clear(&tmp);
        if tmp.exists() {
            return Err(format!(
                "Files from an earlier trim are still in {}. Move them out, then trim again.",
                tmp.display()
            ));
        }
    }
    fs::create_dir_all(tmp.join("old")).map_err(|e| e.to_string())?;
    Ok(tmp)
}

/// Writes the kept video and each track's kept audio into `tmp`; returns the new files' names.
fn build(
    dir: &Path,
    tmp: &Path,
    videos: &[(ScreenPart, PathBuf)],
    plan: &[(usize, u64, u64)],
) -> Result<Vec<String>, String> {
    let pieces: Vec<_> = plan
        .iter()
        .map(|&(index, from, to)| (videos[index].1.clone(), from, to))
        .collect();
    cut::export(&pieces, &tmp.join(VIDEO)).map_err(|e| format!("Couldn't trim the video: {e}"))?;
    let mut names = vec![VIDEO.to_string()];
    for side in ["mic", "system"] {
        let spans: Vec<_> = plan
            .iter()
            .map(|&(index, from, to)| {
                let part = videos[index].0.file.trim_end_matches(".screen.mp4");
                (safe_file(dir, &format!("{part}.{side}.wav")), from, to)
            })
            .collect();
        if spans.iter().any(|(file, ..)| file.is_some()) {
            let name = format!("part-0000.{side}.wav");
            cut::join_wavs(&spans, &tmp.join(&name))
                .map_err(|e| format!("Couldn't trim the audio: {e}"))?;
            names.push(name);
        }
    }
    Ok(names)
}

pub(crate) fn trim(
    root: &Path,
    id: &str,
    from_ms: u64,
    to_ms: u64,
) -> Result<MeetingSummary, String> {
    let dir = own_folder(root, id)?;
    let summary = library::summary(root, id)?;
    if summary.kind != MeetingKind::Screen {
        return Err("Only screen recordings can be trimmed".into());
    }
    if summary.status != "Ready" {
        return Err("Wait until the recording is saved before trimming it".into());
    }
    let mut videos: Vec<(ScreenPart, PathBuf)> =
        screen_files::read::<Vec<ScreenPart>>(&dir, "screen.json")
            .into_iter()
            .filter(|part| screen_files::video_name(&part.file))
            .filter_map(|part| Some((safe_file(&dir, &part.file)?, part)))
            .map(|(path, part)| (part, path))
            .collect();
    videos.sort_by_key(|(part, _)| part.offset_ms);
    if videos.is_empty() {
        return Err("This recording has no video to trim".into());
    }
    let spans = videos
        .iter()
        .map(|(part, path)| Ok((part.offset_ms, cut::duration_ms(path)?)))
        .collect::<Result<Vec<_>, String>>()?;
    let total = spans.iter().map(|(offset, length)| offset + length).max();
    let range = TrimRange::new(from_ms, to_ms, total.unwrap_or(0))?;
    let plan = cut::plan_parts(&spans, range);
    let tmp = scratch(&dir)?;
    let result = build(&dir, &tmp, &videos, &plan)
        .and_then(|new| replace(root, id, &dir, &tmp, &new))
        .and_then(|held| {
            let shift = |ms: u64| cut::shift(&spans, range, ms);
            let first = &videos[plan[0].0].0;
            rewrite(&dir, &held, first, shift)?;
            crate::bookmarks::update(root, id, |s| {
                s.duration_ms = Some(plan.iter().map(|(_, from, to)| to - from).sum());
                // The reach for Stop is gone with the cut.
                s.reach_ms = None;
                s.marks_ms = s.marks_ms.iter().filter_map(|ms| shift(*ms)).collect();
                if let Some(facts) = s.facts.as_mut()
                    && first.width > 0
                {
                    (facts.width, facts.height) = (first.width, first.height);
                }
            })
        });
    if result.is_ok() {
        let _ = fs::remove_dir_all(&tmp);
    } else {
        clear(&tmp);
    }
    result
}

/// Moves the item's parts out (to the full original on a first trim, else to `old/`) and the
/// new files in. Returns where the replaced files went; on failure everything is put back.
fn replace(
    root: &Path,
    id: &str,
    dir: &Path,
    tmp: &Path,
    new: &[String],
) -> Result<PathBuf, String> {
    let old = parts(dir)?;
    let (held, first) = match original(root, id)? {
        Some(_) => (tmp.join("old"), false),
        None => {
            let path = crate::meeting_actions::bin(root)?.join(format!("{id}{FULL}"));
            fs::create_dir(&path).map_err(|e| e.to_string())?;
            let saved = fs::write(
                path.join(DELETED_AT),
                crate::meeting_actions::now_ms()?.to_string(),
            )
            .and_then(|()| fs::copy(dir.join(SUMMARY), path.join(SUMMARY)));
            if let Err(e) = saved {
                forget(&path);
                return Err(format!("Couldn't keep the full video: {e}"));
            }
            (path, true)
        }
    };
    if let Err(e) = move_files(dir, &held, &old) {
        if first {
            forget(&held);
        }
        return Err(e);
    }
    if let Err(e) = move_files(tmp, dir, new) {
        let back = move_files(&held, dir, &old);
        if first && back.is_ok() {
            forget(&held);
        }
        return Err(back.err().map_or(e.clone(), |b| format!("{e} {b}")));
    }
    Ok(held)
}

/// Removes an emptied full-original folder; one still holding media is left alone.
fn forget(path: &Path) {
    let _ = fs::remove_file(path.join(DELETED_AT));
    let _ = fs::remove_file(path.join(SUMMARY));
    let _ = fs::remove_dir(path);
}

/// The manifests for the single trimmed part: the video at 0, its WAVs, and the moments and
/// clicks moved to the trimmed timeline (those that were cut are dropped).
fn rewrite(
    dir: &Path,
    held: &Path,
    first: &ScreenPart,
    shift: impl Fn(u64) -> Option<u64>,
) -> Result<(), String> {
    let had = |name: &str| held.join(name).exists();
    screen_files::write(
        dir,
        "screen.json",
        &vec![ScreenPart {
            file: VIDEO.into(),
            offset_ms: 0,
            ..first.clone()
        }],
    )?;
    if had("parts.json") {
        let wavs: Vec<String> = ["mic", "system"]
            .iter()
            .map(|side| dir.join(format!("part-0000.{side}.wav")))
            .filter(|path| path.exists())
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        let parts: Vec<(Vec<String>, u64)> = if wavs.is_empty() {
            vec![]
        } else {
            vec![(wavs, 0)]
        };
        screen_files::write(dir, "parts.json", &parts)?;
    }
    if had("screen-moments.json") {
        let moments: Vec<Moment> = screen_files::read(held, "screen-moments.json");
        let moments: Vec<Moment> = moments
            .into_iter()
            .filter_map(|m| {
                Some(Moment {
                    ms: shift(m.ms)?,
                    ..m
                })
            })
            .collect();
        screen_files::write(dir, "screen-moments.json", &moments)?;
    }
    if had("screen-clicks.json") {
        let clicks: Vec<Click> = screen_files::read(held, "screen-clicks.json");
        let clicks: Vec<Click> = clicks
            .into_iter()
            .filter_map(|c| Some(Click { ms: shift(c.ms)? }))
            .collect();
        screen_files::write(dir, "screen-clicks.json", &clicks)?;
    }
    Ok(())
}

/// Puts the full original back exactly as it was before the first trim; the trimmed files go.
pub(crate) fn restore(root: &Path, id: &str) -> Result<MeetingSummary, String> {
    let dir = own_folder(root, id)?;
    let source = original(root, id)?.ok_or("There is no full video to restore")?;
    let before: MeetingSummary = serde_json::from_str(&library::text(&source.join(SUMMARY))?)
        .map_err(|e| format!("Cannot read the full video's details: {e}"))?;
    let mut originals = vec![];
    for entry in fs::read_dir(&source).map_err(|e| e.to_string())? {
        let name = entry
            .map_err(|e| e.to_string())?
            .file_name()
            .to_string_lossy()
            .into_owned();
        if name == DELETED_AT || name == SUMMARY {
            continue;
        }
        if !(name.starts_with("part-") || MANIFESTS.contains(&name.as_str()))
            || safe_file(&source, &name).is_none()
        {
            return Err("The full video's folder holds unexpected files".into());
        }
        originals.push(name);
    }
    let current = parts(&dir)?;
    let tmp = scratch(&dir)?;
    let held = tmp.join("old");
    if let Err(e) = move_files(&dir, &held, &current) {
        clear(&tmp);
        return Err(e);
    }
    if let Err(e) = move_files(&source, &dir, &originals) {
        if move_files(&held, &dir, &current).is_ok() {
            clear(&tmp);
        }
        return Err(e);
    }
    let summary = crate::bookmarks::update(root, id, |s| {
        s.duration_ms = before.duration_ms;
        s.reach_ms = before.reach_ms;
        s.marks_ms = before.marks_ms.clone();
        s.facts = before.facts.clone();
    });
    forget(&source);
    let _ = fs::remove_dir_all(&tmp);
    summary
}

/// The Recently deleted row for a meeting's full original: its pre-trim details under the
/// meeting's current title (the UI shows "Full video of {title}").
pub(crate) fn listed(root: &Path, id: &str) -> Option<MeetingSummary> {
    let folder = root.join(".deleted").join(format!("{id}{FULL}"));
    let mut row: MeetingSummary =
        serde_json::from_str(&library::text(&folder.join(SUMMARY)).ok()?).ok()?;
    if let Ok(now) =
        library::summary(root, id).or_else(|_| library::summary(&root.join(".deleted"), id))
    {
        row.title = now.title;
    }
    row.id = format!("{id}{FULL}");
    row.deleted_kind = DeletedKind::TrimmedOriginal;
    row.has_video = true;
    Some(row)
}

#[tauri::command]
pub async fn trim_video(
    app: tauri::AppHandle,
    id: String,
    from_ms: u64,
    to_ms: u64,
) -> Result<MeetingSummary, String> {
    guarded(app, id, move |root, id| trim(root, id, from_ms, to_ms)).await
}

#[tauri::command]
pub async fn restore_trim(app: tauri::AppHandle, id: String) -> Result<MeetingSummary, String> {
    guarded(app, id, restore).await
}

/// Like Delete video: refused while a call records or notes are written (`busy`), and never
/// on the meeting being recorded.
async fn guarded(
    app: tauri::AppHandle,
    id: String,
    change: impl FnOnce(&Path, &str) -> Result<MeetingSummary, String> + Send + 'static,
) -> Result<MeetingSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let library = app.state::<Library>();
        if library.busy.swap(true, Ordering::SeqCst) {
            return Err(
                "Wait until recording and processing finish before changing video files.".into(),
            );
        }
        // The edit check refuses a live meeting; the export itself runs outside the edit lock,
        // so other meetings' edits don't wait for it.
        let result = crate::library_commands::edit(&app, &id, |_| Ok(()))
            .and_then(|()| change(&library.root, &id));
        library.busy.store(false, Ordering::SeqCst);
        let _ = app.emit("motetto://library-changed", ());
        result
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meeting(root: &Path, kind: &str) -> PathBuf {
        let dir = root.join("1");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(SUMMARY),
            format!(
                r#"{{"id":"1","title":"Clip","created_ms":1,"status":"Ready","kind":"{kind}","marks_ms":[500,4500],"reach_ms":800}}"#
            ),
        )
        .unwrap();
        for (name, bytes) in [
            ("part-0000.screen.mp4", "video 0"),
            ("part-0000.mic.wav", "audio 0"),
            ("part-0001.screen.mp4", "video 1"),
            (
                "screen-clicks.json",
                r#"[{"ms":100},{"ms":2500},{"ms":4200}]"#,
            ),
            (
                "screen-moments.json",
                r#"[{"ms":0,"yours":false},{"ms":4100,"yours":true}]"#,
            ),
            ("notes.md", "kept"),
        ] {
            fs::write(dir.join(name), bytes).unwrap();
        }
        let part = |file: &str, offset_ms| ScreenPart {
            file: file.into(),
            offset_ms,
            source: crate::screen::ScreenSourceInfo {
                kind: "window".into(),
                name: "Safari".into(),
            },
            while_sharing: false,
            width: 1280,
            height: 800,
        };
        screen_files::write(
            &dir,
            "screen.json",
            &vec![
                part("part-0000.screen.mp4", 0),
                part("part-0001.screen.mp4", 3000),
            ],
        )
        .unwrap();
        let wav = dir.join("part-0000.mic.wav").to_string_lossy().into_owned();
        screen_files::write(&dir, "parts.json", &vec![(vec![wav], 0u64)]).unwrap();
        library::folder(root, "1").unwrap()
    }

    /// One trim's file shuffle, with fixed bytes standing in for the exported video and WAV.
    fn swap(root: &Path, dir: &Path, bytes: &str) {
        let tmp = scratch(dir).unwrap();
        let new = vec![VIDEO.to_string(), "part-0000.mic.wav".to_string()];
        for name in &new {
            fs::write(tmp.join(name), bytes).unwrap();
        }
        let held = replace(root, "1", dir, &tmp, &new).unwrap();
        // Kept 2000..4500 of parts (0, 3000) and (3000, 2000).
        let spans = [(0, 3000), (3000, 2000)];
        let range = TrimRange::new(2000, 4500, 5000).unwrap();
        let first: Vec<ScreenPart> = screen_files::read(&held, "screen.json");
        rewrite(dir, &held, &first[0], |ms| cut::shift(&spans, range, ms)).unwrap();
        crate::bookmarks::update(root, "1", |s| {
            s.reach_ms = None;
            s.duration_ms = Some(2500);
        })
        .unwrap();
        fs::remove_dir_all(tmp).unwrap();
    }

    #[test]
    fn trims_keep_the_first_original_and_restore_puts_it_back() {
        let root = std::env::temp_dir().join(format!("motetto-trim-app-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let dir = meeting(&root, "screen");
        let before: Vec<(String, Vec<u8>)> = parts(&dir)
            .unwrap()
            .into_iter()
            .map(|name| (name.clone(), fs::read(dir.join(&name)).unwrap()))
            .collect();
        swap(&root, &dir, "first trim");
        let full = root.join(".deleted/1-full");
        assert_eq!(
            fs::read(full.join("part-0001.screen.mp4")).unwrap(),
            b"video 1"
        );
        assert!(full.join(DELETED_AT).exists() && full.join(SUMMARY).exists());
        assert_eq!(
            parts(&dir).unwrap(),
            [
                "part-0000.mic.wav",
                "part-0000.screen.mp4",
                "parts.json",
                "screen-clicks.json",
                "screen-moments.json",
                "screen.json"
            ]
        );
        let screen: Vec<ScreenPart> = screen_files::read(&dir, "screen.json");
        assert_eq!(
            (screen.len(), screen[0].offset_ms, screen[0].width),
            (1, 0, 1280)
        );
        let clicks: Vec<Click> = screen_files::read(&dir, "screen-clicks.json");
        assert_eq!(clicks.iter().map(|c| c.ms).collect::<Vec<_>>(), [500, 2200]);
        let moments: Vec<Moment> = screen_files::read(&dir, "screen-moments.json");
        assert_eq!(moments.iter().map(|m| m.ms).collect::<Vec<_>>(), [2100]);
        let listed_parts: Vec<(Vec<String>, u64)> = screen_files::read(&dir, "parts.json");
        assert_eq!(listed_parts.len(), 1);
        assert!(listed_parts[0].0[0].ends_with("part-0000.mic.wav"));
        assert_eq!(fs::read_to_string(dir.join("notes.md")).unwrap(), "kept");
        // Recently deleted lists it as the meeting's full video, under the current title.
        crate::bookmarks::rename(&root, "1", "Demo").unwrap();
        let row = listed(&root, "1").unwrap();
        assert_eq!((row.id.as_str(), row.title.as_str()), ("1-full", "Demo"));
        assert_eq!(row.deleted_kind, DeletedKind::TrimmedOriginal);
        assert_eq!(original_of("1-full"), Some("1"));
        assert!(original_of("1").is_none() && original_of("../1-full").is_none());
        // A second trim replaces the trimmed files and keeps the first original.
        swap(&root, &dir, "second trim");
        assert_eq!(fs::read(dir.join(VIDEO)).unwrap(), b"second trim");
        assert_eq!(fs::read(full.join(VIDEO)).unwrap(), b"video 0");
        assert!(!dir.join(TMP).exists());
        let summary = restore(&root, "1").unwrap();
        assert_eq!(
            (summary.reach_ms, summary.marks_ms),
            (Some(800), vec![500, 4500])
        );
        assert_eq!(summary.title, "Demo");
        for (name, bytes) in &before {
            assert_eq!(&fs::read(dir.join(name)).unwrap(), bytes, "{name}");
        }
        assert_eq!(parts(&dir).unwrap().len(), before.len());
        assert!(!full.exists() && !dir.join(TMP).exists());
        assert!(restore(&root, "1").is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn only_saved_screen_recordings_are_trimmed() {
        let root = std::env::temp_dir().join(format!("motetto-trim-kind-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        meeting(&root, "meeting");
        assert!(
            trim(&root, "1", 0, 2000)
                .unwrap_err()
                .contains("Only screen")
        );
        crate::bookmarks::update(&root, "1", |s| {
            s.kind = MeetingKind::Screen;
            s.status = "Finishing".into();
        })
        .unwrap();
        assert!(trim(&root, "1", 0, 2000).unwrap_err().contains("saved"));
        assert!(trim(&root, "2", 0, 2000).is_err());
        assert!(!root.join(".deleted/1-full").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
