//! Bounded local storage accounting, retention and explicit destructive actions.
use crate::{library, library_commands::Library, settings::Store};
use serde::Serialize;
use std::{fs, path::Path, sync::atomic::Ordering};
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use ts_rs::TS;

#[derive(Default, Serialize, TS)]
pub struct StorageSummary {
    pub meetings: u32,
    #[ts(type = "number")]
    pub audio_bytes: u64,
    #[ts(type = "number")]
    pub video_bytes: u64,
    #[ts(type = "number")]
    pub notes_bytes: u64,
    #[ts(type = "number")]
    pub free_bytes: u64,
}
fn video(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|n| n.to_string_lossy().ends_with(".screen.mp4"))
        || path.components().any(|c| c.as_os_str() == "deleted-video")
}
fn audio(path: &Path) -> bool {
    if video(path) {
        return false;
    }
    path.extension().is_some_and(|s| {
        [
            "wav", "mp3", "m4a", "aac", "mp4", "mov", "flac", "aiff", "ogg", "opus", "webm", "csv",
        ]
        .iter()
        .any(|ext| s.eq_ignore_ascii_case(ext))
    })
}
fn count(path: &Path, total: &mut StorageSummary) -> Result<(), String> {
    for item in fs::read_dir(path).map_err(|e| e.to_string())? {
        let item = item.map_err(|e| e.to_string())?;
        let kind = item.file_type().map_err(|e| e.to_string())?;
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            count(&item.path(), total)?;
        } else if kind.is_file() {
            let bytes = item.metadata().map_err(|e| e.to_string())?.len();
            if video(&item.path()) {
                total.video_bytes = total.video_bytes.saturating_add(bytes);
            } else if audio(&item.path()) {
                total.audio_bytes = total.audio_bytes.saturating_add(bytes);
            } else {
                total.notes_bytes = total.notes_bytes.saturating_add(bytes);
            }
        }
    }
    Ok(())
}
fn meeting_dirs(root: &Path) -> Result<Vec<std::path::PathBuf>, String> {
    let mut dirs = vec![];
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            continue;
        }
        if name == ".deleted" {
            dirs.extend(meeting_dirs(&entry.path())?);
        } else if !name.is_empty() && name.chars().all(|c| c.is_ascii_digit()) {
            dirs.push(entry.path());
        }
    }
    Ok(dirs)
}
pub fn summary(root: &Path) -> Result<StorageSummary, String> {
    let mut total = StorageSummary::default();
    for dir in meeting_dirs(root)? {
        total.meetings += 1;
        count(&dir, &mut total)?;
    }
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("df")
            .arg("-Pk")
            .arg(root)
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err("Couldn't check free disk space".into());
        }
        total.free_bytes = String::from_utf8_lossy(&out.stdout)
            .lines()
            .nth(1)
            .and_then(|l| l.split_whitespace().nth(3))
            .and_then(|s| s.parse::<u64>().ok())
            .and_then(|n| n.checked_mul(1024))
            .ok_or("Couldn't read free disk space")?;
    }
    Ok(total)
}
#[tauri::command]
pub async fn storage_summary(app: tauri::AppHandle) -> Result<StorageSummary, String> {
    let root = app.state::<Library>().root.clone();
    tauri::async_runtime::spawn_blocking(move || summary(&root))
        .await
        .map_err(|e| e.to_string())?
}
fn wipe(root: &Path) -> Result<(), String> {
    for folder in meeting_dirs(root)? {
        fs::remove_dir_all(folder).map_err(|e| {
            format!("Couldn't delete every meeting: {e}. Some meetings may already be deleted.")
        })?;
    }
    Ok(())
}
#[tauri::command]
pub async fn delete_all_meetings(app: tauri::AppHandle, confirm: bool) -> Result<(), String> {
    if !confirm {
        return Err("Confirm permanent deletion first.".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Library>();
        if state.busy.swap(true, Ordering::SeqCst) {
            return Err(
                "Wait until recording and processing finish before deleting meetings.".into(),
            );
        }
        let result = wipe(&state.root);
        state.busy.store(false, Ordering::SeqCst);
        let _ = app.emit("motetto://library-changed", ());
        result
    })
    .await
    .map_err(|e| e.to_string())?
}
fn copy_verified(source: &Path, target: &Path) -> Result<(), String> {
    fs::create_dir(target).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        let to = target.join(entry.file_name());
        if kind.is_symlink() {
            return Err("Linked files cannot be moved. The original folder is unchanged.".into());
        }
        if kind.is_dir() {
            copy_verified(&entry.path(), &to)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &to).map_err(|e| e.to_string())?;
            use std::io::Read;
            let mut a = fs::File::open(entry.path()).map_err(|e| e.to_string())?;
            let mut b = fs::File::open(&to).map_err(|e| e.to_string())?;
            let mut left = [0u8; 65536];
            let mut right = [0u8; 65536];
            loop {
                let n = a.read(&mut left).map_err(|e| e.to_string())?;
                b.read_exact(&mut right[..n]).map_err(|e| e.to_string())?;
                if left[..n] != right[..n] {
                    return Err("The copied file did not verify. The original is unchanged.".into());
                }
                if n == 0 {
                    break;
                }
            }
            if a.metadata().map_err(|e| e.to_string())?.len()
                != b.metadata().map_err(|e| e.to_string())?.len()
            {
                return Err("The copied file size changed. The original is unchanged.".into());
            }
        } else {
            return Err("Special files cannot be moved.".into());
        }
    }
    Ok(())
}
#[tauri::command]
pub async fn move_notes_folder(app: tauri::AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(file) = app.dialog().file().blocking_pick_folder() else {
            return Ok(None);
        };
        let target = file
            .into_path()
            .map_err(|e| e.to_string())?
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let state = app.state::<Library>();
        if state.busy.swap(true, Ordering::SeqCst) {
            return Err(
                "Stop the recording and wait for processing before moving the notes folder.".into(),
            );
        }
        let result = (|| {
            let source = state.root.canonicalize().map_err(|e| e.to_string())?;
            if source == target {
                return Ok(None);
            }
            if target.starts_with(&source) || source.starts_with(&target) {
                return Err("Choose a folder outside the current notes folder.".into());
            }
            if fs::read_dir(&target)
                .map_err(|e| e.to_string())?
                .next()
                .is_some()
            {
                return Err(
                    "Choose an empty folder so existing files are never overwritten.".into(),
                );
            }
            let staging = target.join(".motetto-move");
            if let Err(e) = copy_verified(&source, &staging) {
                let _ = fs::remove_dir_all(&staging);
                return Err(e);
            }
            for item in fs::read_dir(&staging).map_err(|e| e.to_string())? {
                let item = item.map_err(|e| e.to_string())?;
                fs::rename(item.path(), target.join(item.file_name()))
                    .map_err(|e| e.to_string())?;
            }
            fs::remove_dir(&staging).map_err(|e| e.to_string())?;
            app.state::<Store>()
                .change(|s| s.notes_folder = target.to_string_lossy().into_owned())?;
            // shortcut: retain originals for recovery; remove after a verified relaunch when migration recovery is built.
            state.allow_exit.store(true, Ordering::SeqCst);
            app.restart();
        })();
        state.busy.store(false, Ordering::SeqCst);
        result
    })
    .await
    .map_err(|e| e.to_string())?
}
fn remove_audio(dir: &Path) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_dir() && entry.file_name() != "deleted-video" {
            remove_audio(&entry.path())?;
        } else if kind.is_file() && audio(&entry.path()) {
            fs::remove_file(entry.path()).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
fn remove_video(dir: &Path) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type().map_err(|e| e.to_string())?.is_file()
            && (crate::screen_files::video_name(&name)
                || ["screen.json", "screen-moments.json"].contains(&name.as_str()))
        {
            fs::remove_file(entry.path()).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
fn purge_video(dir: &Path) -> Result<(), String> {
    let deleted = dir.join("deleted-video");
    if !deleted
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_dir())
    {
        return Ok(());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    for entry in fs::read_dir(deleted).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_type().map_err(|e| e.to_string())?.is_dir()
            && let Ok(time) = entry.file_name().to_string_lossy().parse::<u128>()
            && now.saturating_sub(time) >= 30 * 86400 * 1000
        {
            fs::remove_dir_all(entry.path()).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
/// Remove audio only from Ready meetings after their notes are complete; never follow links.
pub fn retain(root: &Path, policy: &str, keep_video: &str) -> Result<(), String> {
    for dir in meeting_dirs(root)? {
        purge_video(&dir)?;
        let Ok(meta) = library::text(&dir.join("meeting.json")) else {
            continue;
        };
        let Ok(meeting) = serde_json::from_str::<library::MeetingSummary>(&meta) else {
            continue;
        };
        if meeting.status != "Ready" {
            continue;
        }
        let age = fs::metadata(dir.join("notes.md"))
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok());
        let Some(age) = age else {
            continue;
        };
        if policy == "after_notes" || (policy == "30_days" && age.as_secs() >= 30 * 86400) {
            let audio = remove_audio(&dir);
            let video = remove_video(&dir);
            let recovery = dir.join("deleted-video");
            let removed = if recovery
                .symlink_metadata()
                .is_ok_and(|m| m.file_type().is_dir())
            {
                fs::remove_dir_all(recovery).map_err(|e| e.to_string())
            } else {
                Ok(())
            };
            audio.and(video).and(removed)?;
        } else if keep_video == "7_days" && age.as_secs() >= 7 * 86400 {
            remove_video(&dir)?;
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seven_day_video_retention_keeps_audio_and_purges_old_recovery() {
        let root = std::env::temp_dir().join(format!("motetto-video-days-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("1/deleted-video/1")).unwrap();
        let dir = root.join("1");
        fs::write(
            dir.join("meeting.json"),
            r#"{"id":"1","title":"Fixture","created_ms":0,"status":"Ready"}"#,
        )
        .unwrap();
        fs::write(dir.join("notes.md"), b"notes").unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(8 * 86400);
        fs::File::options()
            .write(true)
            .open(dir.join("notes.md"))
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(old))
            .unwrap();
        for name in [
            "part-0000.screen.mp4",
            "screen.json",
            "screen-moments.json",
            "part-0000.mic.wav",
        ] {
            fs::write(dir.join(name), b"fixture").unwrap();
        }
        fs::write(
            dir.join("deleted-video/1/part-0000.screen.mp4"),
            b"old video",
        )
        .unwrap();
        retain(&root, "forever", "7_days").unwrap();
        assert!(!dir.join("part-0000.screen.mp4").exists());
        assert!(!dir.join("screen.json").exists());
        assert!(dir.join("part-0000.mic.wav").exists());
        assert!(!dir.join("deleted-video/1").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn video_is_counted_apart_and_never_outlives_audio() {
        let root =
            std::env::temp_dir().join(format!("motetto-video-retain-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("1")).unwrap();
        let dir = root.join("1");
        let meta = serde_json::json!({"id":"1", "title":"Fixture", "created_ms":0, "status":"Ready", "marks_ms":[]});
        fs::write(dir.join("meeting.json"), serde_json::to_vec(&meta).unwrap()).unwrap();
        fs::write(dir.join("notes.md"), b"notes").unwrap();
        fs::write(dir.join("call.mp4"), b"audio").unwrap();
        fs::write(dir.join("part-0000.screen.mp4"), [0; 10]).unwrap();
        let total = summary(&root).unwrap();
        assert_eq!(total.video_bytes, 10);
        assert_eq!(total.audio_bytes, 5);
        fs::create_dir_all(dir.join("deleted-video/1")).unwrap();
        fs::write(
            dir.join("deleted-video/1/part-0000.screen.mp4"),
            b"old video",
        )
        .unwrap();
        retain(&root, "after_notes", "forever").unwrap();
        assert!(!dir.join("deleted-video").exists());
        assert!(!dir.join("part-0000.screen.mp4").exists());
        assert!(!dir.join("call.mp4").exists());
        assert!(dir.join("notes.md").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn retention_keeps_documents_and_unfinished_audio() {
        let root = std::env::temp_dir().join(format!("motetto-retain-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for (id, status) in [("1", "Ready"), ("2", "Finishing"), ("3", "NeedsAttention")] {
            fs::create_dir_all(root.join(id)).unwrap();
            fs::write(
                root.join(id).join("meeting.json"),
                serde_json::to_vec(
                    &serde_json::json!({"id":id,"title":"Meeting","created_ms":0,"status":status}),
                )
                .unwrap(),
            )
            .unwrap();
            fs::write(root.join(id).join("notes.md"), "notes").unwrap();
            fs::write(root.join(id).join("mic.wav"), "audio").unwrap();
            fs::write(root.join(id).join("transcript.txt"), "transcript").unwrap();
        }
        retain(&root, "forever", "same_as_audio").unwrap();
        retain(&root, "30_days", "same_as_audio").unwrap();
        assert!(root.join("1/mic.wav").exists());
        retain(&root, "after_notes", "same_as_audio").unwrap();
        assert!(!root.join("1/mic.wav").exists());
        for file in ["1/notes.md", "1/transcript.txt", "2/mic.wav", "3/mic.wav"] {
            assert!(root.join(file).exists());
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn storage_copy_and_delete_do_not_follow_links() {
        StorageSummary::export_all(&ts_rs::Config::default().with_out_dir("../../ui/lib/bindings"))
            .unwrap();
        let root =
            std::env::temp_dir().join(format!("motetto-settings-storage-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("old/123")).unwrap();
        fs::write(root.join("old/123/notes.md"), "notes").unwrap();
        fs::write(root.join("old/123/mic.wav"), "audio").unwrap();
        copy_verified(&root.join("old"), &root.join("copy")).unwrap();
        assert_eq!(fs::read(root.join("copy/123/notes.md")).unwrap(), b"notes");
        assert_eq!(summary(&root.join("old")).unwrap().audio_bytes, 5);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("old/123"), root.join("copy/456")).unwrap();
        }
        wipe(&root.join("copy")).unwrap();
        assert!(root.join("old/123/notes.md").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
