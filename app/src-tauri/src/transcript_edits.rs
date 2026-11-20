//! Transcript line edits (SPEC 7.11): only the text after `[mm:ss] Speaker: ` changes, or a call
//! voice's `Speaker N` when lines move to another voice; only in transcript.edited.txt, and
//! transcript.txt stays as heard.
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
    write(&dir, &heard, lines.join("\n"))
}

/// The spelling guard on a written transcript, after the spellings changed from `old` to `new`
/// pairs: a line only the guard had changed (its heard text respelled with `old`) is respelled
/// from what was heard with `new`, so a removed spelling or heard form goes back; a line the user
/// edited keeps the edit and takes `new` on top. Into the edited copy (transcript.txt stays as
/// heard). True when a line changed.
pub fn respell(
    root: &Path,
    id: &str,
    old: &[(String, String)],
    new: &[(String, String)],
) -> Result<bool, String> {
    let dir = library::folder(root, id)?;
    if crate::bookmarks::locked(root, id)? || !dir.join("transcript.txt").exists() {
        return Ok(false);
    }
    let heard = library::text(&dir.join("transcript.txt"))?;
    let before = current(&dir)?;
    let after = respelled(&heard, &before, old, new);
    if after == before {
        return Ok(false);
    }
    write(&dir, &heard, after)?;
    Ok(true)
}

fn respelled(
    heard: &str,
    shown: &str,
    old: &[(String, String)],
    new: &[(String, String)],
) -> String {
    use crate::learned_edits::respell;
    let (heard_lines, shown_lines): (Vec<_>, Vec<_>) =
        (heard.split('\n').collect(), shown.split('\n').collect());
    if heard_lines.len() != shown_lines.len() {
        return respell(shown, new);
    }
    heard_lines
        .iter()
        .zip(&shown_lines)
        .map(|(h, e)| {
            if e == h || *e == respell(h, old) {
                respell(h, new)
            } else {
                respell(e, new)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// Moves lines to another call voice: each line's `Speaker N` becomes `Speaker {voice}`, its time
// and words kept. Only call voices move; You (the microphone) is never relabelled.
fn move_lines(root: &Path, id: &str, indices: &[usize], voice: u32) -> Result<String, String> {
    let dir = library::folder(root, id)?;
    if crate::bookmarks::locked(root, id)? {
        return Err(crate::bookmarks::LOCKED.into());
    }
    if !(1..=99).contains(&voice) || indices.is_empty() {
        return Err("Choose who said these lines".into());
    }
    let heard = library::text(&dir.join("transcript.txt"))?;
    let mut lines: Vec<String> = current(&dir)?.split('\n').map(str::to_owned).collect();
    let mut rows: Vec<&mut String> = lines.iter_mut().filter(|l| !l.trim().is_empty()).collect();
    for &index in indices {
        let line = rows
            .get_mut(index)
            .ok_or("This transcript line no longer exists")?;
        let head = prefix(line).ok_or("This line has no time")?;
        let time = head.find("] ").map(|i| i + 2).unwrap_or(0);
        if !head[time..].starts_with("Speaker ") {
            return Err("Only a call voice's lines can move".into());
        }
        **line = format!("{}Speaker {voice}: {}", &head[..time], &line[head.len()..]);
    }
    write(&dir, &heard, lines.join("\n"))
}

// Writes the edited copy atomically, or removes it when the transcript is back to what was heard.
fn write(dir: &Path, heard: &str, next: String) -> Result<String, String> {
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

/// Puts a transcript whose rows are out of time order back in order, both copies with the same
/// moves so they stay line for line (`motetto_core::speakers::time_order`). The screen shows the
/// file's order and edits address lines by position, so the files themselves are reordered, once.
/// True when anything moved; a locked meeting or copies of different lengths are left as they are.
pub fn keep_in_time_order(root: &Path, id: &str) -> Result<bool, String> {
    let dir = library::folder(root, id)?;
    let heard_path = dir.join("transcript.txt");
    if !heard_path.exists() || crate::bookmarks::locked(root, id)? {
        return Ok(false);
    }
    let heard = library::text(&heard_path)?;
    let rows: Vec<&str> = heard.split('\n').collect();
    let order = motetto_core::speakers::time_order(&rows);
    if order.iter().enumerate().all(|(i, &j)| i == j) {
        return Ok(false);
    }
    let edited_path = dir.join(EDITED);
    let edited = if edited_path.exists() {
        Some(library::text(&edited_path)?)
    } else {
        None
    };
    let edited_rows: Option<Vec<&str>> = edited.as_deref().map(|e| e.split('\n').collect());
    if edited_rows.as_ref().is_some_and(|e| e.len() != rows.len()) {
        return Ok(false);
    }
    let arrange = |rows: &[&str]| {
        order
            .iter()
            .map(|&i| rows[i])
            .collect::<Vec<_>>()
            .join("\n")
    };
    // Both written to temporary files first, then renamed one after the other.
    let mut renames = vec![(stage(&dir, &arrange(&rows))?, heard_path)];
    if let Some(e) = &edited_rows {
        renames.push((stage(&dir, &arrange(e))?, edited_path));
    }
    for (from, to) in renames {
        fs::rename(&from, &to).map_err(|e| e.to_string())?;
    }
    Ok(true)
}

// A temporary file in `dir` holding `text`, synced to disk.
fn stage(dir: &Path, text: &str) -> Result<std::path::PathBuf, String> {
    let path = dir.join(format!(
        ".transcript-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let written = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| e.to_string())?;
        file.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())
    })();
    if written.is_err() {
        let _ = fs::remove_file(&path);
    }
    written.map(|()| path)
}

#[tauri::command]
pub fn move_transcript_lines(
    app: tauri::AppHandle,
    id: String,
    indices: Vec<usize>,
    voice: u32,
) -> Result<String, String> {
    let result =
        crate::library_commands::edit(&app, &id, |root| move_lines(root, &id, &indices, voice));
    if result.is_ok() {
        let _ = app.emit("motetto://library-changed", ());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_removed_spelling_goes_back_but_a_hand_edit_stays() {
        let pair = |h: &str, sp: &str| (h.to_owned(), sp.to_owned());
        let heard = "[00:01] Them: nó trả lời là jest\n[00:02] Me: clam cái promotion\n[00:03] Me: clam nữa";
        let old = [pair("jest", "yes"), pair("clam", "claim")];
        // Line 3 was edited by hand after the guard ran.
        let shown = "[00:01] Them: nó trả lời là yes\n[00:02] Me: claim cái promotion\n[00:03] Me: claim nữa nhé";
        // `claim` removed: its line goes back to what was heard; the hand edit stays.
        assert_eq!(
            respelled(heard, shown, &old, &old[..1]),
            "[00:01] Them: nó trả lời là yes\n[00:02] Me: clam cái promotion\n[00:03] Me: claim nữa nhé"
        );
        // Adding a pair applies everywhere, also on top of a hand edit.
        let more = [
            pair("jest", "yes"),
            pair("clam", "claim"),
            pair("nhé", "nha"),
        ];
        assert!(respelled(heard, shown, &old, &more).ends_with("claim nữa nha"));
    }
    #[test]
    fn an_out_of_order_transcript_is_put_in_time_order_in_both_copies() {
        let root = std::env::temp_dir().join(format!("motetto-order-{}", std::process::id()));
        fs::create_dir_all(root.join("1")).unwrap();
        let heard = "[89:14] Speaker 3: a\n[89:26] Speaker 4: b\n\n[89:17] Me: c\n[89:29] Me: d";
        let edited = "[89:14] Speaker 3: A\n[89:26] Speaker 5: B\n\n[89:17] Me: C\n[89:29] Me: D";
        fs::write(root.join("1/transcript.txt"), heard).unwrap();
        fs::write(root.join("1").join(EDITED), edited).unwrap();
        assert!(keep_in_time_order(&root, "1").unwrap());
        assert_eq!(
            fs::read_to_string(root.join("1/transcript.txt")).unwrap(),
            "[89:14] Speaker 3: a\n[89:17] Me: c\n[89:26] Speaker 4: b\n\n[89:29] Me: d"
        );
        assert_eq!(
            current(&root.join("1")).unwrap(),
            "[89:14] Speaker 3: A\n[89:17] Me: C\n[89:26] Speaker 5: B\n\n[89:29] Me: D"
        );
        assert!(!keep_in_time_order(&root, "1").unwrap(), "already in order");
        // Copies of different lengths are left alone.
        fs::write(
            root.join("1/transcript.txt"),
            "[00:09] Me: x\n[00:01] Me: y",
        )
        .unwrap();
        fs::write(root.join("1").join(EDITED), "[00:09] Me: x").unwrap();
        assert!(!keep_in_time_order(&root, "1").unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn moves_only_the_chosen_lines_to_another_voice() {
        let root = std::env::temp_dir().join(format!("motetto-move-{}", std::process::id()));
        fs::create_dir_all(root.join("1")).unwrap();
        let heard = "[00:00] Speaker 2: Bên em đủ rồi anh ơi.\n\n[00:02] Speaker 2: Ok: đủ\n[00:04] Me: rồi ha.\n";
        fs::write(root.join("1/transcript.txt"), heard).unwrap();
        let moved = move_lines(&root, "1", &[1], 5).unwrap();
        assert_eq!(
            moved,
            heard.replace("[00:02] Speaker 2:", "[00:02] Speaker 5:")
        );
        assert_eq!(current(&root.join("1")).unwrap(), moved);
        // A word edit afterwards keeps the new voice.
        assert!(
            save(&root, "1", 1, "Ok đủ")
                .unwrap()
                .contains("[00:02] Speaker 5: Ok đủ\n")
        );
        assert_eq!(
            move_lines(&root, "1", &[2], 3).unwrap_err(),
            "Only a call voice's lines can move"
        );
        assert!(
            move_lines(&root, "1", &[3], 3).is_err(),
            "index out of range"
        );
        assert!(
            move_lines(&root, "1", &[0], 0).is_err() && move_lines(&root, "1", &[], 3).is_err()
        );
        // Moving back to what was heard removes the edited copy.
        save(&root, "1", 1, "Ok: đủ").unwrap();
        move_lines(&root, "1", &[1], 2).unwrap();
        assert!(!root.join("1").join(EDITED).exists());
        fs::remove_dir_all(root).unwrap();
    }

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
