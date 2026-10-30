//! Transcript line edits (SPEC 7.11): only the text after `[mm:ss] Speaker: ` changes, and only in
//! transcript.edited.txt; transcript.txt stays as heard.
use crate::library;
use std::{
    fs,
    io::Write,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};
use tauri::Emitter;
static NEXT: AtomicU64 = AtomicU64::new(0);

pub const EDITED: &str = "transcript.edited.txt";
const MAX_LINE: usize = 2000;

// `[mm:ss] Speaker: `, or `[mm:ss] ` on an unlabelled import: the part of a line an edit never
// touches. Labels are those the Transcript tab shows (Me, Them, Speaker N), so a colon in an
// unlabelled line's text stays text.
fn prefix(line: &str) -> Option<&str> {
    Some(motetto_core::correct::prefix(line))
        .filter(|head| line.starts_with('[') && !head.is_empty())
}

// The transcript as shown: the edited copy when there is one.
pub fn current(dir: &Path) -> Result<String, String> {
    let edited = dir.join(EDITED);
    let path = if edited.exists() {
        edited
    } else {
        dir.join("transcript.txt")
    };
    library::text(&path)
}

// `index` counts non-empty lines, as the Transcript tab does. An empty text marks the line
// removed (it keeps its time and speaker); a text equal to what was heard reverts it.
fn save(root: &Path, id: &str, index: usize, text: &str) -> Result<String, String> {
    let dir = library::folder(root, id)?;
    if crate::bookmarks::locked(root, id)? {
        return Err(crate::bookmarks::LOCKED.into());
    }
    if text.contains(['\n', '\r']) {
        return Err("A transcript line can't contain a line break".into());
    }
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() > MAX_LINE {
        return Err("This line is too long".into());
    }
    let heard = library::text(&dir.join("transcript.txt"))?;
    let before = current(&dir)?;
    let mut lines: Vec<String> = before.split('\n').map(str::to_owned).collect();
    let line = lines
        .iter_mut()
        .filter(|line| !line.trim().is_empty())
        .nth(index)
        .ok_or("This transcript line no longer exists")?;
    let head = prefix(line).ok_or("This line has no time")?.to_owned();
    *line = format!("{head}{text}");
    let next = lines.join("\n");
    let edited = dir.join(EDITED);
    if next == heard {
        if edited.exists() {
            fs::remove_file(&edited).map_err(|e| e.to_string())?;
        }
        return Ok(next);
    }
    let temporary = dir.join(format!(
        ".transcript-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let written = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|e| e.to_string())?;
        file.write_all(next.as_bytes()).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        fs::rename(&temporary, &edited).map_err(|e| e.to_string())
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written?;
    Ok(next)
}

#[tauri::command]
pub fn save_transcript_line(
    app: tauri::AppHandle,
    id: String,
    index: usize,
    text: String,
) -> Result<String, String> {
    let result = crate::library_commands::edit(&app, &id, |root| save(root, &id, index, &text));
    if result.is_ok() {
        let _ = app.emit("motetto://library-changed", ());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edits_only_the_text_and_never_the_heard_file() {
        let root = std::env::temp_dir().join(format!("motetto-transcript-{}", std::process::id()));
        fs::create_dir_all(root.join("1")).unwrap();
        let heard = "[00:16] Me: After studying push beams, I see.\n\n[01:00] Them: Ok: fine\n";
        fs::write(root.join("1/transcript.txt"), heard).unwrap();
        let edited = save(&root, "1", 0, "  After studying  Pusher Beams, I see. ").unwrap();
        assert_eq!(
            edited,
            heard.replace("push beams", "Pusher Beams"),
            "time, speaker and blank lines kept"
        );
        assert_eq!(
            fs::read_to_string(root.join("1/transcript.txt")).unwrap(),
            heard
        );
        assert_eq!(current(&root.join("1")).unwrap(), edited);
        // The second non-empty line; its text holds a colon of its own.
        let removed = save(&root, "1", 1, "").unwrap();
        assert!(removed.contains("\n[01:00] Them: \n"));
        assert!(save(&root, "1", 2, "x").is_err(), "index out of range");
        assert!(save(&root, "1", 0, "two\nlines").is_err());
        assert!(save(&root, "../1", 0, "x").is_err());
        // Reverting every line deletes the edited copy.
        save(&root, "1", 1, "Ok: fine").unwrap();
        save(&root, "1", 0, "After studying push beams, I see.").unwrap();
        assert!(!root.join("1").join(EDITED).exists());
        // A single-file import has no speaker labels: its time stays, and a colon is text.
        let import = "[00:03] Alo, lò, lò.\n[00:05] Note: nghe anh.\n";
        fs::write(root.join("1/transcript.txt"), import).unwrap();
        fs::remove_file(root.join("1").join(EDITED)).ok();
        assert_eq!(
            save(&root, "1", 0, "Alô, lô.").unwrap(),
            "[00:03] Alô, lô.\n[00:05] Note: nghe anh.\n"
        );
        assert!(
            save(&root, "1", 1, "Nghe anh.")
                .unwrap()
                .contains("\n[00:05] Nghe anh.\n")
        );
        // Locked meetings are read-only.
        fs::write(
            root.join("1/meeting.json"),
            r#"{"id":"1","title":"T","created_ms":0,"status":"Ready","locked":true}"#,
        )
        .unwrap();
        assert_eq!(
            save(&root, "1", 0, "x").unwrap_err(),
            crate::bookmarks::LOCKED
        );
        fs::remove_dir_all(root).unwrap();
    }
}
