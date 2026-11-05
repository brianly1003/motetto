//! Trim a saved screen recording at both ends without re-encoding (SPEC 12.7 `trim_video`,
//! `restore_trim`). The kept video and WAVs are built in `.trim-tmp/`, then replace the item's
//! parts; the full original waits in Recently deleted as `<id>-full` for 30 days. A second trim
//! keeps the first original, so restoring always gives back the whole recording.
//!
//! The original is moved under `<id>-full.partial` and renamed to `<id>-full` only once all of
//! it is there, so `<id>-full` is always whole. A `.partial` left by a crash holds originals
//! that belong back in the meeting: the next trim or restore moves them back first. Files in
//! `.trim-tmp/old` are never originals: they are only dropped while a whole original exists.
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
/// The original while it is being moved; never listed or expired.
const STAGING: &str = ".partial";
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
/// How far past the video's length the end handle may be: the strip's length comes from the
/// summary, which can run a few ms longer than the joined parts. Clamped to the video.
const END_SLACK_MS: u64 = 250;

#[derive(Serialize, Deserialize)]
struct Click {
    ms: u64,
}

/// Joins (file, from_ms, to_ms) pieces into one video at the path.
type Export = fn(&[(PathBuf, u64, u64)], &Path) -> Result<(), String>;

/// The video steps, so tests can stand in for AVFoundation.
pub(crate) struct Media {
    pub(crate) length: fn(&Path) -> Result<u64, String>,
    pub(crate) export: Export,
}
const NATIVE: Media = Media {
    length: cut::duration_ms,
    export: cut::export,
};

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

/// `<id>-full` (or with `suffix`, its staging name) in Recently deleted, when it is a real folder.
fn in_bin(root: &Path, id: &str, suffix: &str) -> Result<(PathBuf, bool), String> {
    let path = crate::meeting_actions::bin(root)?.join(format!("{id}{FULL}{suffix}"));
    match path.symlink_metadata() {
        Err(_) => Ok((path, false)),
        Ok(meta) if meta.file_type().is_dir() => Ok((path, true)),
        Ok(_) => Err("Invalid full video in Recently deleted".into()),
    }
}

/// The whole original, if there is one. A `<id>-full` without its summary, manifest and video
/// is refused rather than trusted.
fn original(root: &Path, id: &str) -> Result<Option<PathBuf>, String> {
    let (path, exists) = in_bin(root, id, "")?;
    if !exists {
        return Ok(None);
    }
    let has_video = fs::read_dir(&path)
        .map_err(|e| e.to_string())?
        .flatten()
        .any(|e| screen_files::video_name(&e.file_name().to_string_lossy()));
    if !has_video || !path.join(SUMMARY).is_file() || !path.join("screen.json").is_file() {
        return Err(
            "The full video in Recently deleted is incomplete, so nothing was changed.".into(),
        );
    }
    Ok(Some(path))
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

/// The original media in a full-original folder, checked to be nothing else.
fn originals(source: &Path) -> Result<Vec<String>, String> {
    let mut names = vec![];
    for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
        let name = entry
            .map_err(|e| e.to_string())?
            .file_name()
            .to_string_lossy()
            .into_owned();
        if name == DELETED_AT || name == SUMMARY {
            continue;
        }
        if !(name.starts_with("part-") || MANIFESTS.contains(&name.as_str()))
            || safe_file(source, &name).is_none()
        {
            return Err("The full video's folder holds unexpected files".into());
        }
        names.push(name);
    }
    Ok(names)
}

/// Removes an emptied full-original folder; one still holding media is left alone.
fn forget(path: &Path) {
    let _ = fs::remove_file(path.join(DELETED_AT));
    let _ = fs::remove_file(path.join(SUMMARY));
    let _ = fs::remove_dir(path);
}

/// Moves a staged original's media back into the meeting and puts its pre-trim times back.
fn unstage(root: &Path, id: &str, dir: &Path, staged: &Path) -> Result<(), String> {
    let before: Option<MeetingSummary> = library::text(&staged.join(SUMMARY))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok());
    move_files(staged, dir, &originals(staged)?)?;
    if let Some(before) = before {
        crate::bookmarks::update(root, id, |s| {
            s.duration_ms = before.duration_ms;
            s.reach_ms = before.reach_ms;
            s.marks_ms = before.marks_ms.clone();
            s.facts = before.facts.clone();
        })?;
    }
    forget(staged);
    Ok(())
}

/// Finishes what a crash interrupted: a staged original goes back into its meeting.
fn recover(root: &Path, id: &str, dir: &Path) -> Result<(), String> {
    match in_bin(root, id, STAGING)? {
        (staged, true) => unstage(root, id, dir, &staged),
        _ => Ok(()),
    }
}

/// Clears `.trim-tmp`: new files always, `old/` (earlier trimmed files) only when `whole` says
/// a whole original exists.
fn clear(tmp: &Path, whole: bool) {
    for entry in fs::read_dir(tmp).into_iter().flatten().flatten() {
        let path = entry.path();
        if entry.file_name() == "old" {
            if whole {
                let _ = fs::remove_dir_all(&path);
            } else {
                let _ = fs::remove_dir(&path);
            }
        } else if entry.file_type().is_ok_and(|t| t.is_dir()) {
            let _ = fs::remove_dir_all(&path);
        } else {
            let _ = fs::remove_file(&path);
        }
    }
    let _ = fs::remove_dir(tmp);
}

/// An empty `.trim-tmp` in the meeting folder with an empty `old/` for the files replaced.
fn scratch(dir: &Path, whole: bool) -> Result<PathBuf, String> {
    let tmp = dir.join(TMP);
    if let Ok(meta) = tmp.symlink_metadata() {
        if !meta.is_dir() {
            return Err("Invalid trim folder".into());
        }
        clear(&tmp, whole);
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

/// Where each part's WAVs start on the recording's timeline, from `parts.json` (by file name).
fn audio_offsets(dir: &Path) -> Vec<(String, u64)> {
    let parts: Vec<(Vec<String>, u64)> = screen_files::read(dir, "parts.json");
    parts
        .into_iter()
        .flat_map(|(files, offset)| {
            files.into_iter().filter_map(move |file| {
                Some((Path::new(&file).file_name()?.to_str()?.to_string(), offset))
            })
        })
        .collect()
}

/// Writes the kept video and each track's kept audio into `tmp`; returns the new files' names.
/// A WAV is cut at the same recording times as its video, from its own offset in `parts.json`
/// (the recorder gives both the part's offset, so they match today).
fn build(
    dir: &Path,
    tmp: &Path,
    videos: &[(ScreenPart, PathBuf)],
    plan: &[(usize, u64, u64)],
    media: &Media,
) -> Result<Vec<String>, String> {
    let pieces: Vec<_> = plan
        .iter()
        .map(|&(index, from, to)| (videos[index].1.clone(), from, to))
        .collect();
    (media.export)(&pieces, &tmp.join(VIDEO))
        .map_err(|e| format!("Couldn't trim the video: {e}"))?;
    let offsets = audio_offsets(dir);
    let mut names = vec![VIDEO.to_string()];
    for side in ["mic", "system"] {
        let spans: Vec<_> = plan
            .iter()
            .map(|&(index, from, to)| {
                let video = &videos[index].0;
                let name = format!("{}.{side}.wav", video.file.trim_end_matches(".screen.mp4"));
                let offset = offsets
                    .iter()
                    .find(|(file, _)| *file == name)
                    .map_or(video.offset_ms, |(_, offset)| *offset);
                let start = (video.offset_ms + from).saturating_sub(offset);
                (safe_file(dir, &name), start, start + (to - from))
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
    trim_with(root, id, from_ms, to_ms, &NATIVE)
}

fn trim_with(
    root: &Path,
    id: &str,
    from_ms: u64,
    to_ms: u64,
    media: &Media,
) -> Result<MeetingSummary, String> {
    let dir = own_folder(root, id)?;
    let summary = library::summary(root, id)?;
    if summary.kind != MeetingKind::Screen {
        return Err("Only screen recordings can be trimmed".into());
    }
    if summary.status != "Ready" {
        return Err("Wait until the recording is saved before trimming it".into());
    }
    recover(root, id, &dir)?;
    let full = original(root, id)?;
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
        .map(|(part, path)| Ok((part.offset_ms, (media.length)(path)?)))
        .collect::<Result<Vec<_>, String>>()?;
    let total = spans.iter().map(|(offset, length)| offset + length).max();
    let total = total.unwrap_or(0);
    let to_ms = if to_ms > total && to_ms <= total + END_SLACK_MS {
        total
    } else {
        to_ms
    };
    let range = TrimRange::new(from_ms, to_ms, total)?;
    let plan = cut::plan_parts(&spans, range);
    let tmp = scratch(&dir, full.is_some())?;
    let new = match build(&dir, &tmp, &videos, &plan, media) {
        Ok(new) => new,
        Err(e) => {
            clear(&tmp, full.is_some());
            return Err(e);
        }
    };
    let held = match replace(root, id, &dir, &tmp, &new, full.as_deref()) {
        Ok(held) => held,
        Err(e) => {
            clear(&tmp, full.is_some());
            return Err(e);
        }
    };
    let shift = |ms: u64| cut::shift(&spans, range, ms);
    let first = &videos[plan[0].0].0;
    let finished = rewrite(&dir, &held, first, shift).and_then(|()| {
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
    match finished {
        Ok(summary) => {
            // A re-trim starts the original's 30 days again, as the toast says.
            if full.is_some() {
                let _ = fs::write(
                    held_original(root, id).join(DELETED_AT),
                    crate::meeting_actions::now_ms()?.to_string(),
                );
            }
            clear(&tmp, true);
            Ok(summary)
        }
        Err(e) => match undo(root, id, &dir, &tmp, &held, full.is_none()) {
            Ok(()) => {
                clear(&tmp, true);
                Err(e)
            }
            Err(undone) => Err(format!("{e} Putting the files back failed too: {undone}")),
        },
    }
}

fn held_original(root: &Path, id: &str) -> PathBuf {
    root.join(".deleted").join(format!("{id}{FULL}"))
}

/// Moves the item's parts out and the new files in. On a first trim the parts are staged, then
/// become the whole original; on a later one they go to `.trim-tmp/old`. Returns where they
/// went; on failure everything is put back.
fn replace(
    root: &Path,
    id: &str,
    dir: &Path,
    tmp: &Path,
    new: &[String],
    full: Option<&Path>,
) -> Result<PathBuf, String> {
    let old = parts(dir)?;
    let held = match full {
        Some(_) => {
            let held = tmp.join("old");
            move_files(dir, &held, &old)?;
            held
        }
        None => {
            let (staged, exists) = in_bin(root, id, STAGING)?;
            if exists {
                return Err("An earlier trim is still being put back".into());
            }
            fs::create_dir(&staged).map_err(|e| e.to_string())?;
            let saved = fs::write(
                staged.join(DELETED_AT),
                crate::meeting_actions::now_ms()?.to_string(),
            )
            .and_then(|()| fs::copy(dir.join(SUMMARY), staged.join(SUMMARY)));
            if let Err(e) = saved {
                forget(&staged);
                return Err(format!("Couldn't keep the full video: {e}"));
            }
            if let Err(e) = move_files(dir, &staged, &old) {
                forget(&staged);
                return Err(e);
            }
            let whole = held_original(root, id);
            if let Err(e) = fs::rename(&staged, &whole) {
                return Err(match unstage(root, id, dir, &staged) {
                    Ok(()) => e.to_string(),
                    Err(back) => format!("{e} {back}"),
                });
            }
            whole
        }
    };
    if let Err(e) = move_files(tmp, dir, new) {
        return Err(match undo(root, id, dir, tmp, &held, full.is_none()) {
            Ok(()) => e,
            Err(back) => format!("{e} {back}"),
        });
    }
    Ok(held)
}

/// Puts the item back as it was before this trim: whatever is in the folder now goes to
/// `.trim-tmp/new`, and the held parts come back (a first trim's through staging).
fn undo(
    root: &Path,
    id: &str,
    dir: &Path,
    tmp: &Path,
    held: &Path,
    first: bool,
) -> Result<(), String> {
    let thrown = tmp.join("new");
    fs::create_dir_all(&thrown).map_err(|e| e.to_string())?;
    move_files(dir, &thrown, &parts(dir)?)?;
    if first {
        let (staged, _) = in_bin(root, id, STAGING)?;
        fs::rename(held, &staged).map_err(|e| e.to_string())?;
        unstage(root, id, dir, &staged)
    } else {
        move_files(held, dir, &originals(held)?)
    }
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
/// The original is staged first, so a crash midway is finished by the next trim or restore.
pub(crate) fn restore(root: &Path, id: &str) -> Result<MeetingSummary, String> {
    let dir = own_folder(root, id)?;
    recover(root, id, &dir)?;
    let whole = original(root, id)?.ok_or("There is no full video to restore")?;
    originals(&whole)?;
    let current = parts(&dir)?;
    // Whole original: anything an earlier failed trim left in `.trim-tmp/old` can go.
    let tmp = scratch(&dir, true)?;
    let held = tmp.join("old");
    if let Err(e) = move_files(&dir, &held, &current) {
        clear(&tmp, true);
        return Err(e);
    }
    let (staged, _) = in_bin(root, id, STAGING)?;
    if let Err(e) = fs::rename(&whole, &staged) {
        let _ = move_files(&held, &dir, &current);
        return Err(e.to_string());
    }
    unstage(root, id, &dir, &staged)?;
    clear(&tmp, true);
    library::summary(root, id)
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

    /// A part's length is the number its fake video holds.
    fn length(path: &Path) -> Result<u64, String> {
        Ok(fs::read_to_string(path).unwrap().parse().unwrap())
    }
    /// The joined video holds its kept length, so a second trim can read it.
    fn export(parts: &[(PathBuf, u64, u64)], dst: &Path) -> Result<(), String> {
        let kept: u64 = parts.iter().map(|(_, from, to)| to - from).sum();
        fs::write(dst, kept.to_string()).map_err(|e| e.to_string())
    }
    /// Exports, then breaks meeting.json, so the summary update after the swap fails.
    fn export_then_break(parts: &[(PathBuf, u64, u64)], dst: &Path) -> Result<(), String> {
        export(parts, dst)?;
        let meeting = dst.parent().unwrap().parent().unwrap().join(SUMMARY);
        fs::write(meeting, "broken").map_err(|e| e.to_string())
    }
    const FAKE: Media = Media { length, export };
    const BREAKS: Media = Media {
        length,
        export: export_then_break,
    };

    fn root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("motetto-trim-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        root
    }

    /// A screen recording of two parts, (0, 3000) with a mic and (3000, 2000) without.
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
            ("part-0000.screen.mp4", "3000"),
            ("part-0001.screen.mp4", "2000"),
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
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let wav = dir.join("part-0000.mic.wav");
        let mut writer = hound::WavWriter::create(&wav, spec).unwrap();
        for i in 0..48_000 {
            writer.write_sample((i % 1000) as i16 + 1).unwrap();
        }
        writer.finalize().unwrap();
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
        let wav = wav.to_string_lossy().into_owned();
        screen_files::write(&dir, "parts.json", &vec![(vec![wav], 0u64)]).unwrap();
        library::folder(root, "1").unwrap()
    }

    /// Every part file and manifest with its bytes, to compare whole states.
    fn snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
        parts(dir)
            .unwrap()
            .into_iter()
            .map(|name| (name.clone(), fs::read(dir.join(&name)).unwrap()))
            .collect()
    }

    #[test]
    fn trims_keep_the_first_original_and_restore_puts_it_back() {
        let root = root("roundtrip");
        let dir = meeting(&root, "screen");
        let before = snapshot(&dir);
        // The end handle may run 200 ms past the video; it is held to the video's end.
        let trimmed = trim_with(&root, "1", 2000, 5200, &FAKE).unwrap();
        assert_eq!((trimmed.duration_ms, trimmed.reach_ms), (Some(3000), None));
        assert_eq!(trimmed.marks_ms, [2500]);
        assert!(trim_with(&root, "1", 0, 1500, &FAKE).is_ok_and(|s| s.duration_ms == Some(1500)));
        let full = root.join(".deleted/1-full");
        assert_eq!(
            fs::read(full.join("part-0001.screen.mp4")).unwrap(),
            b"2000"
        );
        assert!(full.join(DELETED_AT).exists() && full.join(SUMMARY).exists());
        assert!(!root.join(".deleted/1-full.partial").exists() && !dir.join(TMP).exists());
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
        assert_eq!(fs::read(dir.join(VIDEO)).unwrap(), b"1500");
        let screen: Vec<ScreenPart> = screen_files::read(&dir, "screen.json");
        assert_eq!((screen.len(), screen[0].offset_ms), (1, 0));
        // Clicks 2500 and 4200 of the recording sat at 500 and 2200 after the first trim.
        let clicks: Vec<Click> = screen_files::read(&dir, "screen-clicks.json");
        assert_eq!(clicks.iter().map(|c| c.ms).collect::<Vec<_>>(), [500]);
        // The mic: 1 s of part 0 from 2000 ms, then silence for part 1, then the second cut.
        let mut mic = hound::WavReader::open(dir.join("part-0000.mic.wav")).unwrap();
        assert_eq!(mic.duration(), 24_000);
        assert_eq!(mic.samples::<i32>().next().unwrap().unwrap(), 1);
        assert_eq!(fs::read_to_string(dir.join("notes.md")).unwrap(), "kept");
        // Recently deleted lists it as the meeting's full video, under the current title.
        crate::bookmarks::rename(&root, "1", "Demo").unwrap();
        let row = listed(&root, "1").unwrap();
        assert_eq!((row.id.as_str(), row.title.as_str()), ("1-full", "Demo"));
        assert_eq!(row.deleted_kind, DeletedKind::TrimmedOriginal);
        assert_eq!(original_of("1-full"), Some("1"));
        assert!(original_of("1").is_none() && original_of("1-full.partial").is_none());
        let summary = restore(&root, "1").unwrap();
        assert_eq!(
            (summary.reach_ms, summary.marks_ms),
            (Some(800), vec![500, 4500])
        );
        assert_eq!(summary.title, "Demo");
        assert_eq!(snapshot(&dir), before);
        assert!(!full.exists() && !dir.join(TMP).exists());
        assert!(restore(&root, "1").is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_failure_after_the_swap_puts_everything_back() {
        let root = root("undo");
        let dir = meeting(&root, "screen");
        let before = snapshot(&dir);
        let summary = fs::read(dir.join(SUMMARY)).unwrap();
        // First trim: the original comes back from Recently deleted.
        assert!(trim_with(&root, "1", 1000, 4000, &BREAKS).is_err());
        assert_eq!(snapshot(&dir), before);
        assert!(!root.join(".deleted/1-full").exists());
        assert!(!root.join(".deleted/1-full.partial").exists() && !dir.join(TMP).exists());
        // Second trim: the first trim's files come back, and the original stays whole.
        fs::write(dir.join(SUMMARY), &summary).unwrap();
        trim_with(&root, "1", 1000, 4000, &FAKE).unwrap();
        let once = snapshot(&dir);
        assert!(trim_with(&root, "1", 500, 2000, &BREAKS).is_err());
        assert_eq!(snapshot(&dir), once);
        assert!(!dir.join(TMP).exists());
        fs::write(dir.join(SUMMARY), &summary).unwrap();
        // Restore works with earlier trimmed files left in .trim-tmp/old.
        fs::create_dir_all(dir.join(TMP).join("old")).unwrap();
        fs::write(dir.join(TMP).join("old").join(VIDEO), "left over").unwrap();
        restore(&root, "1").unwrap();
        assert_eq!(snapshot(&dir), before);
        assert!(!dir.join(TMP).exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_incomplete_or_staged_original_never_costs_media() {
        let root = root("partial");
        let dir = meeting(&root, "screen");
        let before = snapshot(&dir);
        fs::create_dir_all(root.join(".deleted/1-full")).unwrap();
        assert!(restore(&root, "1").unwrap_err().contains("incomplete"));
        assert!(
            trim_with(&root, "1", 1000, 4000, &FAKE)
                .unwrap_err()
                .contains("incomplete")
        );
        assert_eq!(snapshot(&dir), before);
        fs::remove_dir(root.join(".deleted/1-full")).unwrap();
        // A crash while staging: half the original sits in .partial; it goes back first.
        let staged = root.join(".deleted/1-full.partial");
        fs::create_dir_all(&staged).unwrap();
        for name in ["part-0001.screen.mp4", "screen.json"] {
            fs::rename(dir.join(name), staged.join(name)).unwrap();
        }
        assert!(restore(&root, "1").unwrap_err().contains("no full video"));
        assert_eq!(snapshot(&dir), before);
        assert!(!staged.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn only_saved_screen_recordings_are_trimmed() {
        let root = root("kind");
        meeting(&root, "meeting");
        assert!(
            trim_with(&root, "1", 0, 2000, &FAKE)
                .unwrap_err()
                .contains("Only screen")
        );
        crate::bookmarks::update(&root, "1", |s| {
            s.kind = MeetingKind::Screen;
            s.status = "Finishing".into();
        })
        .unwrap();
        assert!(
            trim_with(&root, "1", 0, 2000, &FAKE)
                .unwrap_err()
                .contains("saved")
        );
        assert!(trim_with(&root, "2", 0, 2000, &FAKE).is_err());
        assert!(!root.join(".deleted/1-full").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
