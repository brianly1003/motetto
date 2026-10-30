//! Persist task completion and in-place note edits without overwriting a newer notes document.
use crate::library;
use std::{
    fs,
    io::Write,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};
use tauri::Emitter;
static NEXT: AtomicU64 = AtomicU64::new(0);

fn update(
    root: &Path,
    id: &str,
    line: usize,
    done: bool,
    expected: &str,
) -> Result<String, String> {
    let dir = library::folder(root, id)?;
    let path = dir.join("notes.md");
    let current = library::text(&path)?;
    if current != expected {
        return Err("Notes changed. Reopen this meeting before editing.".into());
    }
    let mut lines: Vec<_> = current.split('\n').map(str::to_owned).collect();
    let task = lines.get_mut(line).ok_or("Action item no longer exists")?;
    let prefix = task.len() - task.trim_start().len();
    let trimmed = &task[prefix..];
    if !["- [ ] ", "- [x] ", "- [X] ", "* [ ] ", "* [x] ", "* [X] "]
        .iter()
        .any(|p| trimmed.starts_with(p))
    {
        return Err("This line is not an action item".into());
    }
    task.replace_range(prefix + 3..prefix + 4, if done { "x" } else { " " });
    write(&dir, &path, &current, &lines.join("\n"))
}

// The notes keep the transcript copy the summary step wrote; an edit may not change it (SPEC 7.11).
fn transcript_copy(text: &str) -> Option<&str> {
    let start = text.find("<details><summary>Transcript</summary>")?;
    let end = text[start..]
        .find("</details>")
        .map_or(text.len(), |e| start + e);
    Some(&text[start..end])
}

const MAX_NOTES: usize = 1_000_000;

fn save(root: &Path, id: &str, markdown: &str, expected: &str) -> Result<String, String> {
    let dir = library::folder(root, id)?;
    let path = dir.join("notes.md");
    let current = library::text(&path)?;
    if current != expected {
        return Err("Notes changed. Reopen this meeting before editing.".into());
    }
    if crate::bookmarks::locked(root, id)? {
        return Err(crate::bookmarks::LOCKED.into());
    }
    if markdown.len() > MAX_NOTES {
        return Err("These notes are too long to save".into());
    }
    if transcript_copy(markdown) != transcript_copy(&current) {
        return Err("The transcript copy in the notes can't be edited".into());
    }
    write(&dir, &path, &current, markdown)
}

// Write next to the file, check nothing changed meanwhile, then rename over it.
fn write(dir: &Path, path: &Path, current: &str, result: &str) -> Result<String, String> {
    let temporary = dir.join(format!(
        ".tasks-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| e.to_string())?;
    let saved = (|| {
        file.write_all(result.as_bytes())
            .map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        if library::text(path)? != current {
            return Err("Notes changed while saving".into());
        }
        fs::rename(&temporary, path).map_err(|e| e.to_string())
    })();
    if saved.is_err() {
        let _ = fs::remove_file(temporary);
    }
    saved?;
    Ok(result.to_owned())
}
#[tauri::command]
pub fn set_action_item_done(
    app: tauri::AppHandle,
    id: String,
    line: usize,
    done: bool,
    expected: String,
) -> Result<String, String> {
    let result =
        crate::library_commands::edit(&app, &id, |root| update(root, &id, line, done, &expected));
    if result.is_ok() {
        let _ = app.emit("motetto://library-changed", ());
    }
    result
}
// Notes blocks save as the user types and on leaving a block (SPEC 7.11).
#[tauri::command]
pub fn save_notes(
    app: tauri::AppHandle,
    id: String,
    markdown: String,
    expected: String,
) -> Result<String, String> {
    let result =
        crate::library_commands::edit(&app, &id, |root| save(root, &id, &markdown, &expected));
    if result.is_ok() {
        let _ = app.emit("motetto://library-changed", ());
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_document_and_rejects_stale_edits() {
        let root = std::env::temp_dir().join(format!("motetto-tasks-{}", std::process::id()));
        fs::create_dir_all(root.join("1")).unwrap();
        let text = "### Action items\n- [ ] You: Share it [01:20]\n\n### Overview\nKeep this.\n";
        fs::write(root.join("1/notes.md"), text).unwrap();
        let changed = update(&root, "1", 1, true, text).unwrap();
        assert_eq!(changed, text.replacen("[ ]", "[x]", 1));
        assert!(update(&root, "1", 1, false, text).is_err());
        assert!(update(&root, "1", 4, true, &changed).is_err());
        assert_eq!(update(&root, "1", 1, false, &changed).unwrap(), text);
        assert!(update(&root, "../1", 1, true, text).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn saves_edits_but_not_stale_ones_or_the_transcript_copy() {
        let root = std::env::temp_dir().join(format!("motetto-note-save-{}", std::process::id()));
        fs::create_dir_all(root.join("1")).unwrap();
        let text = "### Overview\n- Keep **FCM**.\n\n<details><summary>Transcript</summary>\n\n[00:01] Me: hi\n</details>\n";
        fs::write(root.join("1/notes.md"), text).unwrap();
        let edited = text.replace("Keep", "Keep using");
        assert_eq!(save(&root, "1", &edited, text).unwrap(), edited);
        assert_eq!(fs::read_to_string(root.join("1/notes.md")).unwrap(), edited);
        assert!(save(&root, "1", text, text).is_err(), "stale expected text");
        let tampered = edited.replace("Me: hi", "Me: bye");
        assert!(save(&root, "1", &tampered, &edited).is_err());
        assert!(save(&root, "../1", &edited, &edited).is_err());
        // Locked notes are read-only; unlocking makes them editable again.
        fs::write(
            root.join("1/meeting.json"),
            r#"{"id":"1","title":"T","created_ms":0,"status":"Ready","locked":true}"#,
        )
        .unwrap();
        assert_eq!(
            save(&root, "1", text, &edited).unwrap_err(),
            crate::bookmarks::LOCKED
        );
        crate::bookmarks::set_locked(&root, "1", false).unwrap();
        assert_eq!(save(&root, "1", text, &edited).unwrap(), text);
        fs::remove_dir_all(root).unwrap();
    }
}
