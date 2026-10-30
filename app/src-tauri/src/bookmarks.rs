//! Serialized meeting metadata writes preserve bookmarks across worker status updates.
use crate::library::{self, MeetingSummary};
use std::{
    fs,
    io::Write,
    path::Path,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
static WRITER: Mutex<()> = Mutex::new(());
static NEXT: AtomicU64 = AtomicU64::new(0);

pub fn save(root: &Path, value: &MeetingSummary) -> Result<(), String> {
    let _guard = WRITER
        .lock()
        .map_err(|_| "Meeting storage is unavailable")?;
    let mut value = value.clone();
    let path = library::folder(root, &value.id)?.join("meeting.json");
    if path.try_exists().map_err(|e| e.to_string())? {
        // Marks and the title belong to the user once the meeting exists; a worker's status
        // update must not undo a Mark or a rename made while the notes were being written.
        // The notes model's title (SPEC 7.6) still goes through, unless the user named it meanwhile.
        let saved = library::summary(root, &value.id)?;
        value.marks_ms = saved.marks_ms;
        if saved.title_source == library::TitleSource::User
            || value.title_source != library::TitleSource::Model
        {
            value.title = saved.title;
            value.title_source = saved.title_source;
        }
        value.locked = saved.locked;
    }
    write(root, &value)
}

/// SPEC 7.6: one line, at most 80 characters; an empty title is refused (the UI keeps the old one).
pub fn clean_title(title: &str) -> Result<String, String> {
    let title = title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .take(80)
        .collect::<String>();
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err("A meeting needs a title".into());
    }
    Ok(title)
}

pub fn rename(root: &Path, id: &str, title: &str) -> Result<MeetingSummary, String> {
    let title = clean_title(title)?;
    let _guard = WRITER
        .lock()
        .map_err(|_| "Meeting storage is unavailable")?;
    let mut value = library::summary(root, id)?;
    if value.locked {
        return Err(LOCKED.into());
    }
    if value.title != title {
        value.title = title;
        value.title_source = library::TitleSource::User;
        write(root, &value)?;
    }
    Ok(value)
}

pub fn mark(root: &Path, id: &str, at_ms: u64) -> Result<(), String> {
    let _guard = WRITER
        .lock()
        .map_err(|_| "Meeting storage is unavailable")?;
    let mut value = library::summary(root, id)?;
    if !value.marks_ms.contains(&at_ms) {
        value.marks_ms.push(at_ms);
        value.marks_ms.sort_unstable();
        write(root, &value)?;
    }
    Ok(())
}

pub const LOCKED: &str = "These notes are locked. Unlock them to edit.";

/// Lock or unlock a meeting's notes for review (SPEC 7.11); the user's choice, kept like the title.
pub fn set_locked(root: &Path, id: &str, locked: bool) -> Result<MeetingSummary, String> {
    let _guard = WRITER
        .lock()
        .map_err(|_| "Meeting storage is unavailable")?;
    let mut value = library::summary(root, id)?;
    if value.locked != locked {
        value.locked = locked;
        write(root, &value)?;
    }
    Ok(value)
}

/// Keep or Remove in Quick check (SPEC 7.13): the lines still marked "Couldn't confirm in the call".
/// Undo puts a line back, so any list is taken, within the notes' own limits.
pub fn set_unconfirmed(
    root: &Path,
    id: &str,
    lines: Vec<String>,
) -> Result<MeetingSummary, String> {
    if lines.len() > 200 || lines.iter().any(|l| l.chars().count() > 500) {
        return Err("Too many or too long lines to keep".into());
    }
    let _guard = WRITER
        .lock()
        .map_err(|_| "Meeting storage is unavailable")?;
    let mut value = library::summary(root, id)?;
    if value.locked {
        return Err(LOCKED.into());
    }
    if value.unconfirmed != lines {
        value.unconfirmed = lines;
        write(root, &value)?;
    }
    Ok(value)
}

// A meeting without metadata (an older import) is unlocked; unreadable metadata refuses the edit.
pub fn locked(root: &Path, id: &str) -> Result<bool, String> {
    if !library::folder(root, id)?.join("meeting.json").exists() {
        return Ok(false);
    }
    Ok(library::summary(root, id)?.locked)
}

fn write(root: &Path, value: &MeetingSummary) -> Result<(), String> {
    let dir = library::folder(root, &value.id)?;
    let temporary = dir.join(format!(
        ".meeting-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| e.to_string())?;
    let result = file
        .write_all(&bytes)
        .and_then(|_| file.sync_all())
        .and_then(|_| fs::rename(&temporary, dir.join("meeting.json")));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|e| format!("Couldn't save meeting metadata: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unconfirmed_lines_are_saved_and_locked_notes_refuse_them() {
        let root = std::env::temp_dir().join(format!(
            "motetto-unconfirmed-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("123")).unwrap();
        fs::write(
            root.join("123/meeting.json"),
            r#"{"id":"123","title":"Call","created_ms":0,"status":"Ready","unconfirmed":["A [00:01]","B"]}"#,
        )
        .unwrap();
        let saved = set_unconfirmed(&root, "123", vec!["B".into()]).unwrap();
        assert_eq!(saved.unconfirmed, ["B"]);
        assert_eq!(library::summary(&root, "123").unwrap().unconfirmed, ["B"]);
        assert!(set_unconfirmed(&root, "123", vec!["x".repeat(501)]).is_err());
        set_locked(&root, "123", true).unwrap();
        assert_eq!(set_unconfirmed(&root, "123", vec![]).unwrap_err(), LOCKED);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn stale_worker_updates_keep_marks_and_old_meetings_load() {
        let root = std::env::temp_dir().join(format!(
            "motetto-marks-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("123")).unwrap();
        fs::write(
            root.join("123/meeting.json"),
            r#"{"id":"123","title":"Call","created_ms":0,"status":"Recording"}"#,
        )
        .unwrap();
        let mut stale = library::summary(&root, "123").unwrap();
        assert!(stale.marks_ms.is_empty());
        mark(&root, "123", 25_000).unwrap();
        mark(&root, "123", 25_000).unwrap();
        stale.status = "Ready".into();
        save(&root, &stale).unwrap();
        assert_eq!(
            library::summary(&root, "123").unwrap().marks_ms,
            vec![25_000]
        );
        assert!(mark(&root, "../123", 10).is_err());
        fs::write(root.join("123/meeting.json"), "damaged metadata").unwrap();
        assert!(mark(&root, "123", 30_000).is_err());
        assert!(save(&root, &stale).is_err());
        assert_eq!(
            fs::read_to_string(root.join("123/meeting.json")).unwrap(),
            "damaged metadata"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rename_cleans_the_title_and_survives_a_stale_worker_save() {
        assert_eq!(
            clean_title("  Push \n vendor\tdecision ").unwrap(),
            "Push vendor decision"
        );
        assert_eq!(clean_title(&"é".repeat(95)).unwrap().chars().count(), 80);
        assert!(clean_title(" \n\t ").is_err());
        let root = std::env::temp_dir().join(format!(
            "motetto-rename-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("123")).unwrap();
        fs::write(
            root.join("123/meeting.json"),
            r#"{"id":"123","title":"Recorded meeting","created_ms":0,"status":"Finishing"}"#,
        )
        .unwrap();
        // The worker read the summary before the user renamed the meeting.
        let mut stale = library::summary(&root, "123").unwrap();
        assert_eq!(
            rename(&root, "123", "Weekly sync").unwrap().title,
            "Weekly sync"
        );
        stale.status = "Ready".into();
        save(&root, &stale).unwrap();
        let saved = library::summary(&root, "123").unwrap();
        assert_eq!(
            (saved.title.as_str(), saved.status.as_str()),
            ("Weekly sync", "Ready")
        );
        assert!(rename(&root, "123", "   ").is_err());
        assert!(rename(&root, "../123", "Escape").is_err());
        assert!(rename(&root, "999", "Missing").is_err());
        assert_eq!(library::summary(&root, "123").unwrap().title, "Weekly sync");
        // The notes model never replaces a title the user typed...
        let mut worker = library::summary(&root, "123").unwrap();
        worker.titled(Some("Model title".into()));
        save(&root, &worker).unwrap();
        assert_eq!(library::summary(&root, "123").unwrap().title, "Weekly sync");
        // ...but names a placeholder, even from a summary read before it was saved, and may
        // rename its own title on Write the notes again.
        fs::write(
            root.join("123/meeting.json"),
            r#"{"id":"123","title":"Recorded meeting","created_ms":0,"status":"Finishing"}"#,
        )
        .unwrap();
        for title in ["Keep FCM for IGB push", "FCM device table plan"] {
            let mut worker = library::summary(&root, "123").unwrap();
            worker.titled(Some(title.into()));
            save(&root, &worker).unwrap();
            let saved = library::summary(&root, "123").unwrap();
            assert_eq!(
                (saved.title.as_str(), saved.title_source),
                (title, library::TitleSource::Model)
            );
        }
        // No title from the model keeps what is there.
        let mut worker = library::summary(&root, "123").unwrap();
        worker.titled(None);
        save(&root, &worker).unwrap();
        assert_eq!(
            library::summary(&root, "123").unwrap().title,
            "FCM device table plan"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
