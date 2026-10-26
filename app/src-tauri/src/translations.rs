//! Read in (Notes tab, owner 2026-10-26): a meeting's notes translated into another language by
//! the local notes model, one file per language beside notes.md (`notes.<code>.md`). notes.md
//! stays the one the user edits; a translation knows which notes it came from, so the page can
//! say when they have changed since. One translation runs at a time and never takes `busy`, so a
//! call can still start while it runs.
use crate::{library, library_commands::Library};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{Emitter, Manager};

/// First line of a translation file: `<!-- motetto translation of notes <hash> -->`.
const HEAD: &str = "<!-- motetto translation of notes ";

#[derive(Clone, serde::Serialize)]
pub(crate) struct Progress {
    id: String,
    code: String,
    part: u32,
    of: u32,
    /// The last event: saved, or `error` says why not (`meeting_actions::STOPPED` after Stop).
    done: bool,
    error: Option<String>,
}

#[derive(Default)]
pub(crate) struct Running(Mutex<Option<(Progress, Arc<AtomicBool>)>>);

#[derive(serde::Serialize)]
pub(crate) struct Translation {
    code: String,
    markdown: String,
    /// The notes changed after this was made, so its lines may no longer match theirs.
    stale: bool,
}

/// What a translation depends on: every line of notes.md, with the transcript block's lines blank
/// (they are copied, not translated) and ticks ignored (a task ticked in either view stays in step).
fn source(notes: &str) -> String {
    let mut transcript = false;
    let text = notes
        .split('\n')
        .map(|line| {
            transcript |= line.contains("<details><summary>Transcript</summary>");
            let kept = if transcript { "" } else { line.trim_end() };
            transcript &= !line.contains("</details>");
            let trimmed = kept.trim_start();
            if ["- [x] ", "- [X] ", "* [x] ", "* [X] "]
                .iter()
                .any(|m| trimmed.starts_with(m))
            {
                kept.replacen("[x]", "[ ]", 1).replacen("[X]", "[ ]", 1)
            } else {
                kept.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    // FNV-1a: stable across Rust versions, unlike DefaultHasher.
    let hash = text.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ b as u64).wrapping_mul(0x100000001b3)
    });
    format!("{hash:016x}")
}

fn path(folder: &Path, code: &str) -> Result<PathBuf, String> {
    motetto_core::summarize::language_name(code).ok_or("Choose a supported language")?;
    Ok(folder.join(format!("notes.{code}.md")))
}

fn saved(folder: &Path, code: &str, notes: &str) -> Result<Option<Translation>, String> {
    let file = path(folder, code)?;
    if !file.exists() {
        return Ok(None);
    }
    let text = library::text(&file)?;
    let (head, markdown) = text.split_once('\n').unwrap_or((&text, ""));
    let from = head.strip_prefix(HEAD).and_then(|h| h.strip_suffix(" -->"));
    Ok(Some(Translation {
        code: code.into(),
        markdown: markdown.into(),
        stale: from != Some(source(notes).as_str()),
    }))
}

fn notes_of(root: &Path, id: &str) -> Result<String, String> {
    library::read(root, id).map(|meeting| meeting.notes_md)
}

/// The languages this meeting's notes are saved in, and whether each is out of date.
#[tauri::command]
pub(crate) fn notes_translations(
    app: tauri::AppHandle,
    id: String,
) -> Result<Vec<Translation>, String> {
    let root = &app.state::<Library>().root;
    let (folder, notes) = (library::folder(root, &id)?, notes_of(root, &id)?);
    let mut list = Vec::new();
    for (code, _) in motetto_core::summarize::LANGUAGES {
        if let Some(mut translation) = saved(&folder, code, &notes)? {
            translation.markdown.clear(); // the list only needs the languages
            list.push(translation);
        }
    }
    Ok(list)
}

#[tauri::command]
pub(crate) fn get_translation(
    app: tauri::AppHandle,
    id: String,
    code: String,
) -> Result<Option<Translation>, String> {
    let root = &app.state::<Library>().root;
    saved(&library::folder(root, &id)?, &code, &notes_of(root, &id)?)
}

/// The translation running now, with its progress, so a page opened meanwhile can follow it.
#[tauri::command]
pub(crate) fn translating(state: tauri::State<'_, Running>) -> Option<Progress> {
    let running = state.0.lock().unwrap_or_else(|e| e.into_inner());
    running.as_ref().map(|(progress, _)| progress.clone())
}

#[tauri::command]
pub(crate) fn stop_translation(state: tauri::State<'_, Running>, id: String) {
    let running = state.0.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((progress, stop)) = running.as_ref()
        && progress.id == id
    {
        stop.store(true, Ordering::SeqCst);
    }
}

/// Translates the notes and saves `notes.<code>.md`. Progress goes out as `motetto://translate`
/// events, the last with `done`; Stop ends it with `meeting_actions::STOPPED`.
#[tauri::command]
pub(crate) async fn translate_notes(
    app: tauri::AppHandle,
    id: String,
    code: String,
) -> Result<Translation, String> {
    let library = app.state::<Library>();
    let root = library.root.clone();
    // Not while the meeting records or writes its notes: there are no notes to translate yet.
    let notes = crate::library_commands::edit(&app, &id, |root| notes_of(root, &id))?;
    path(&library::folder(&root, &id)?, &code)?;
    if notes.trim().is_empty() {
        return Err("This meeting has no finished notes to translate yet.".into());
    }
    let stop = Arc::new(AtomicBool::new(false));
    let first = Progress {
        id: id.clone(),
        code: code.clone(),
        part: 0,
        of: 0,
        done: false,
        error: None,
    };
    {
        let state = app.state::<Running>();
        let mut running = state.0.lock().unwrap_or_else(|e| e.into_inner());
        if running.is_some() {
            return Err("One translation at a time. Wait for the other one or stop it.".into());
        }
        *running = Some((first.clone(), stop.clone()));
    }
    // A Stop pressed for an earlier job leaves the shared flag set until the next job starts;
    // with no job running, nothing else is listening to it.
    if !library.busy.load(Ordering::SeqCst) {
        motetto_core::tools::reset();
    }
    let _ = app.emit("motetto://translate", first.clone());
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let report = |part, of| {
            let progress = Progress {
                id: id.clone(),
                code: code.clone(),
                part,
                of,
                done: false,
                error: None,
            };
            if let Some((now, _)) = worker
                .state::<Running>()
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_mut()
            {
                *now = progress.clone();
            }
            let _ = worker.emit("motetto://translate", progress);
        };
        let text =
            motetto_core::translate::notes(&notes, &code, &|| stop.load(Ordering::SeqCst), &report)
                .map_err(|e| {
                    if stop.load(Ordering::SeqCst) || motetto_core::tools::cancelled() {
                        crate::meeting_actions::STOPPED.to_string()
                    } else {
                        format!("Couldn't translate the notes: {e}")
                    }
                })?;
        let root = &worker.state::<Library>().root;
        let folder = library::folder(root, &id)?;
        let file = path(&folder, &code)?;
        let next = file.with_extension("md.new");
        fs::write(&next, format!("{HEAD}{} -->\n{text}", source(&notes)))
            .map_err(|e| format!("Couldn't save the translation: {e}"))?;
        fs::rename(&next, &file).map_err(|e| format!("Couldn't save the translation: {e}"))?;
        // The notes may have been edited while it ran: then it is saved, and says it is stale.
        saved(&folder, &code, &notes_of(root, &id)?)?
            .ok_or_else(|| "Couldn't read the translation back".to_string())
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r);
    let state = app.state::<Running>();
    let last = state.0.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some((mut last, _)) = last {
        last.done = true;
        last.error = result.as_ref().err().cloned();
        let _ = app.emit("motetto://translate", last);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_translation_goes_stale_when_the_notes_change_but_not_on_a_tick() {
        let notes = "### Action items\n- [ ] You: Send it [00:05]\n\n<details><summary>Transcript</summary>\n\n[00:05] Me: send\n</details>\n";
        let ticked = notes.replacen("[ ]", "[x]", 1);
        assert_eq!(source(notes), source(&ticked));
        assert_ne!(
            source(notes),
            source(&notes.replace("Send it", "Send them"))
        );
        // The transcript copy doesn't count, but its line count does (lines must stay aligned).
        assert_eq!(source(notes), source(&notes.replace("send\n", "sent\n")));
        assert_ne!(
            source(notes),
            source(&notes.replace("send\n", "send\nmore\n"))
        );

        let folder =
            std::env::temp_dir().join(format!("motetto-translations-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        assert!(saved(&folder, "ms", notes).unwrap().is_none());
        assert!(path(&folder, "../x").is_err(), "codes are allowlisted");
        fs::write(
            folder.join("notes.ms.md"),
            format!(
                "{HEAD}{} -->\n### Tindakan\n- [ ] You: Hantar [00:05]",
                source(notes)
            ),
        )
        .unwrap();
        let fresh = saved(&folder, "ms", &ticked).unwrap().unwrap();
        assert_eq!(fresh.markdown, "### Tindakan\n- [ ] You: Hantar [00:05]");
        assert!(!fresh.stale);
        assert!(saved(&folder, "ms", "### Edited").unwrap().unwrap().stale);
        fs::remove_dir_all(folder).unwrap();
    }
}
