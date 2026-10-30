//! Local document actions. Deleted meetings remain recoverable; regeneration preserves notes.
use crate::{library, library_commands::Library};
use motetto_core::events::{AppState, Event, SessionState};
use std::{fs, path::Path, sync::atomic::Ordering};
use tauri::{Emitter, Manager};

/// The error Write the notes again and Transcribe again end with after Stop; the UI shows a toast.
pub(crate) const STOPPED: &str = "stopped";

/// When a meeting went to Recently deleted, in ms, inside its folder: renaming a folder doesn't
/// reliably change its own times, so the 30 days count from this file (`purge_expired`).
const DELETED_AT: &str = "deleted-at";
const KEEP_DELETED_MS: u128 = 30 * 86_400_000;

fn now_ms() -> Result<u128, String> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis())
}

pub(crate) fn move_meeting(root: &Path, id: &str, restore: bool) -> Result<(), String> {
    let deleted = root.join(".deleted");
    fs::create_dir_all(&deleted).map_err(|e| e.to_string())?;
    if deleted.canonicalize().map_err(|e| e.to_string())?.parent()
        != Some(root.canonicalize().map_err(|e| e.to_string())?.as_path())
    {
        return Err("Deleted folder is outside the library".into());
    }
    let (from, to) = if restore {
        (deleted.as_path(), root)
    } else {
        (root, deleted.as_path())
    };
    let source = library::folder(from, id)?;
    let destination = to.join(id);
    if destination.exists() {
        return Err("A meeting already exists at the destination".into());
    }
    fs::rename(source, &destination).map_err(|e| e.to_string())?;
    // The meeting has moved either way; a missing stamp only restarts its 30 days at the next purge.
    if restore {
        let _ = fs::remove_file(destination.join(DELETED_AT));
    } else {
        let _ = fs::write(destination.join(DELETED_AT), now_ms()?.to_string());
    }
    Ok(())
}

/// SPEC 7.6: meetings in Recently deleted for 30 days are removed for good, at launch and every
/// 24 h. One without a stamp (deleted before stamps existed) gets one now, so nothing goes early.
/// Every meeting is tried; the first failure is returned.
pub(crate) fn purge_expired(root: &Path, now: u128) -> Result<(), String> {
    let deleted = root.join(".deleted");
    if !deleted.exists() {
        return Ok(());
    }
    let mut first = Ok(());
    for entry in fs::read_dir(&deleted).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let id = entry.file_name().to_string_lossy().into_owned();
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let stamp = entry.path().join(DELETED_AT);
        let result = match fs::read_to_string(&stamp)
            .ok()
            .and_then(|text| text.trim().parse::<u128>().ok())
        {
            Some(at) if now.saturating_sub(at) >= KEEP_DELETED_MS => purge_meeting(root, &id),
            Some(_) => Ok(()),
            None if entry.file_type().is_ok_and(|t| t.is_dir()) => {
                fs::write(&stamp, now.to_string()).map_err(|e| e.to_string())
            }
            None => Ok(()),
        };
        if first.is_ok() {
            first = result
                .map_err(|e| format!("Couldn't clear meeting {id} from Recently deleted: {e}"));
        }
    }
    first
}

/// Runs `purge_expired` now and every 24 h after, on its own thread for the app's lifetime.
pub(crate) fn purge_daily(root: std::path::PathBuf) {
    std::thread::spawn(move || {
        loop {
            if let Err(error) = now_ms().and_then(|now| purge_expired(&root, now)) {
                eprintln!("Motetto: {error}");
            }
            std::thread::sleep(std::time::Duration::from_secs(86_400));
        }
    });
}

fn purge_meeting(root: &Path, id: &str) -> Result<(), String> {
    let deleted = root.join(".deleted");
    if fs::symlink_metadata(&deleted)
        .map_err(|e| e.to_string())?
        .file_type()
        .is_symlink()
        || deleted.canonicalize().map_err(|e| e.to_string())?.parent()
            != Some(root.canonicalize().map_err(|e| e.to_string())?.as_path())
    {
        return Err("Invalid deleted meetings folder".into());
    }
    let folder = library::folder(&deleted, id)?;
    if fs::symlink_metadata(deleted.join(id))
        .map_err(|e| e.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err("Cannot permanently delete a linked meeting".into());
    }
    fs::remove_dir_all(folder).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn deleted_meetings(
    state: tauri::State<'_, Library>,
) -> Result<Vec<library::MeetingSummary>, String> {
    let deleted = state.root.join(".deleted");
    if !deleted.exists() {
        return Ok(vec![]);
    }
    if deleted.canonicalize().map_err(|e| e.to_string())?.parent()
        != Some(
            state
                .root
                .canonicalize()
                .map_err(|e| e.to_string())?
                .as_path(),
        )
    {
        return Err("Deleted folder is outside the library".into());
    }
    library::list(&deleted)
}

/// SPEC 7.6 `rename_meeting`: titles live in meeting.json only, so the folder never moves.
/// Not gated on `busy`: renaming is safe while a meeting records or writes its notes.
#[tauri::command]
pub fn rename_meeting(
    app: tauri::AppHandle,
    id: String,
    title: String,
) -> Result<library::MeetingSummary, String> {
    let summary = crate::bookmarks::rename(&app.state::<Library>().root, &id, &title)?;
    let _ = app.emit("motetto://library-changed", ());
    Ok(summary)
}

#[tauri::command]
pub fn set_locked(
    app: tauri::AppHandle,
    id: String,
    locked: bool,
) -> Result<library::MeetingSummary, String> {
    let summary = crate::bookmarks::set_locked(&app.state::<Library>().root, &id, locked)?;
    let _ = app.emit("motetto://library-changed", ());
    Ok(summary)
}

#[tauri::command]
pub fn set_unconfirmed(
    app: tauri::AppHandle,
    id: String,
    lines: Vec<String>,
) -> Result<library::MeetingSummary, String> {
    crate::bookmarks::set_unconfirmed(&app.state::<Library>().root, &id, lines)
}

fn run(root: &Path, id: &str, action: &str) -> Result<(), String> {
    match action {
        "delete" => move_meeting(root, id, false),
        "restore" => move_meeting(root, id, true),
        "purge" => purge_meeting(root, id),
        "reveal" => {
            let folder = library::folder(root, id)?;
            #[cfg(target_os = "macos")]
            let status = std::process::Command::new("/usr/bin/open")
                .arg("-R")
                .arg(folder)
                .status();
            #[cfg(target_os = "windows")]
            let status = std::process::Command::new("explorer.exe")
                .arg(folder)
                .status();
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            let status = std::process::Command::new("xdg-open").arg(folder).status();
            if status.map_err(|e| e.to_string())?.success() {
                Ok(())
            } else {
                Err("Could not open the meeting folder".into())
            }
        }
        "regenerate" => crate::rewrite::regenerate(root, id, None, &|_| {}),
        "retranscribe" => crate::rewrite::retranscribe(root, id, &|_| {}),
        _ => Err("Unknown meeting action".into()),
    }
}

/// The meeting a call is recording or writing notes for; the controller holds `busy` meanwhile.
pub(crate) fn live_id(state: &AppState) -> Option<&str> {
    match &state.state {
        SessionState::Finishing { meeting_id } => Some(meeting_id),
        SessionState::Recording | SessionState::Paused | SessionState::Stopped { .. } => {
            state.meeting_id.as_deref()
        }
        _ => None,
    }
}

#[tauri::command]
pub async fn meeting_action(
    app: tauri::AppHandle,
    id: String,
    action: String,
    language: Option<String>,
) -> Result<(), String> {
    // Write the notes again in a language picked for this meeting (SPEC 7.12).
    if let Some(code) = &language
        && (action != "regenerate"
            || (code != "same_as_call" && motetto_core::summarize::language_name(code).is_none()))
    {
        return Err("Unknown notes language".into());
    }
    let state = app.state::<Library>();
    let root = state.root.clone();
    // Delete, restore, purge and reveal touch one folder, so other meetings stay manageable while a
    // call records or a file imports; the meeting being written does not (owner, 2026-10-20).
    if action != "regenerate" && action != "retranscribe" {
        let worker = app.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            crate::library_commands::edit(&worker, &id, |root| run(root, &id, &action))
        })
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r);
        let _ = app.emit("motetto://library-changed", ());
        return result;
    }
    if state.busy.swap(true, Ordering::SeqCst) {
        return Err(
            "Wait for the current recording or import to finish before writing the notes again"
                .into(),
        );
    }
    state.importing.store(true, Ordering::SeqCst);
    state.set_job(Some(crate::library_commands::Job {
        id: Some(id.clone()),
        import: false,
        transcribe: action == "retranscribe",
    }));
    let _ = app.emit("motetto://library-changed", ());
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let stage = |stage| {
            let _ = worker.emit("motetto://event", Event::stage(id.clone(), stage));
        };
        if action == "retranscribe" {
            crate::rewrite::retranscribe(&root, &id, &stage)
        } else {
            crate::rewrite::regenerate(&root, &id, language.as_deref(), &stage)
        }
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r)
    // Stop on the steps page: nothing was replaced; the page says so instead of an error.
    .map_err(|e| {
        if motetto_core::tools::cancelled() {
            STOPPED.to_string()
        } else {
            e
        }
    });
    state.set_job(None);
    state.importing.store(false, Ordering::SeqCst);
    state.busy.store(false, Ordering::SeqCst);
    let _ = app.emit("motetto://library-changed", ());
    if state.quit_pending.load(Ordering::SeqCst) {
        state.allow_exit.store(true, Ordering::SeqCst);
        app.exit(0);
    }
    result
}

#[tauri::command]
pub async fn delete_video(app: tauri::AppHandle, id: String) -> Result<(), String> {
    video_action(app, id, false).await
}
#[tauri::command]
pub async fn restore_video(app: tauri::AppHandle, id: String) -> Result<(), String> {
    video_action(app, id, true).await
}
async fn video_action(app: tauri::AppHandle, id: String, restore: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let library = app.state::<Library>();
        if library.busy.swap(true, Ordering::SeqCst) {
            return Err(
                "Wait until recording and processing finish before changing video files.".into(),
            );
        }
        let result = if restore {
            crate::screen_files::restore(&library.root, &id)
        } else {
            crate::screen_files::delete(&library.root, &id)
        };
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
    #[test]
    fn only_the_call_in_progress_is_live() {
        let mut state = AppState {
            meeting_id: Some("123".into()),
            ..AppState::default()
        };
        assert_eq!(live_id(&state), None);
        for session in [SessionState::Recording, SessionState::Paused] {
            state.state = session;
            assert_eq!(live_id(&state), Some("123"));
        }
        state.state = SessionState::Finishing {
            meeting_id: "456".into(),
        };
        assert_eq!(live_id(&state), Some("456"));
    }
    #[test]
    fn deleted_meetings_go_after_30_days_and_restore_clears_the_clock() {
        let root = std::env::temp_dir().join(format!("motetto-expire-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for id in ["1", "2", "3"] {
            fs::create_dir_all(root.join(id)).unwrap();
            fs::write(root.join(id).join("notes.md"), id).unwrap();
            move_meeting(&root, id, false).unwrap();
        }
        let stamped = |id: &str| -> u128 {
            fs::read_to_string(root.join(".deleted").join(id).join(DELETED_AT))
                .unwrap()
                .parse()
                .unwrap()
        };
        let at = stamped("1");
        fs::write(
            root.join(".deleted/2").join(DELETED_AT),
            (at - KEEP_DELETED_MS).to_string(),
        )
        .unwrap();
        // Deleted before stamps existed: its 30 days start now.
        fs::remove_file(root.join(".deleted/3").join(DELETED_AT)).unwrap();
        purge_expired(&root, at).unwrap();
        assert!(root.join(".deleted/1").exists(), "inside 30 days");
        assert!(!root.join(".deleted/2").exists(), "30 days and over");
        assert_eq!(stamped("3"), at);
        purge_expired(&root, at + KEEP_DELETED_MS - 1).unwrap();
        assert!(root.join(".deleted/3").exists());
        // Restored meetings carry no stamp back into the library.
        move_meeting(&root, "1", true).unwrap();
        assert!(!root.join("1").join(DELETED_AT).exists());
        assert_eq!(fs::read_to_string(root.join("1/notes.md")).unwrap(), "1");
        purge_expired(&root, at + 10 * KEEP_DELETED_MS).unwrap();
        assert!(root.join("1").exists() && !root.join(".deleted/3").exists());
        // A link in the bin is never followed or removed through.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("1"), root.join(".deleted/4")).unwrap();
            assert!(purge_expired(&root, at + 10 * KEEP_DELETED_MS).is_ok());
            assert!(root.join("1/notes.md").exists());
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn permanent_delete_only_removes_deleted_meetings() {
        let root = std::env::temp_dir().join(format!("motetto-purge-{}", std::process::id()));
        fs::create_dir_all(root.join("123")).unwrap();
        fs::write(root.join("123/notes.md"), "Keep active meeting").unwrap();
        fs::create_dir_all(root.join(".deleted/456/audio")).unwrap();
        fs::write(root.join(".deleted/456/audio/mic.wav"), b"fixture").unwrap();
        assert!(purge_meeting(&root, "123").is_err());
        assert!(purge_meeting(&root, "../123").is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("123"), root.join(".deleted/789")).unwrap();
            assert!(purge_meeting(&root, "789").is_err());
            std::os::unix::fs::symlink(root.join("123"), root.join(".deleted/456/link")).unwrap();
        }
        purge_meeting(&root, "456").unwrap();
        assert!(!root.join(".deleted/456").exists());
        assert_eq!(
            fs::read_to_string(root.join("123/notes.md")).unwrap(),
            "Keep active meeting"
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn delete_restore_preserves_files_and_refuses_collisions_and_traversal() {
        let root = std::env::temp_dir().join(format!("motetto-actions-{}", std::process::id()));
        fs::create_dir_all(root.join("123")).unwrap();
        fs::write(root.join("123/notes.md"), "Keep this").unwrap();
        move_meeting(&root, "123", false).unwrap();
        assert!(!root.join("123").exists());
        assert!(move_meeting(&root, "../123", true).is_err());
        fs::create_dir(root.join("123")).unwrap();
        assert!(move_meeting(&root, "123", true).is_err());
        fs::remove_dir(root.join("123")).unwrap();
        move_meeting(&root, "123", true).unwrap();
        assert_eq!(
            fs::read_to_string(root.join("123/notes.md")).unwrap(),
            "Keep this"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
