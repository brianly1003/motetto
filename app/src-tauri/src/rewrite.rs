//! Writing a meeting again from what it kept: Transcribe again (the saved audio) and Write the
//! notes again (the transcript). Both run one at a time under `busy` and report `Stage`s.
use crate::library;
use motetto_core::events::Stage;
use std::{fs, path::Path};

/// A fresh `.regenerating` folder. One left behind means a run was cut off (the app quit or
/// crashed mid-way, 2026-10-21); `busy` allows one run at a time, so no live run owns it.
fn staging(folder: &Path) -> Result<std::path::PathBuf, String> {
    let staging = folder.join(".regenerating");
    if fs::symlink_metadata(&staging).is_ok() {
        fs::remove_dir_all(&staging).map_err(|e| format!("Cannot prepare new notes: {e}"))?;
    }
    fs::create_dir(&staging).map_err(|e| format!("Cannot prepare new notes: {e}"))?;
    Ok(staging)
}

/// The audio a meeting kept, as transcribe parts: a recording's `parts.json` (each track by its
/// file name in this folder, since stored paths can predate a folder move), or an import's
/// `source.<extension>`. Empty when the audio was deleted.
fn saved_audio(folder: &Path) -> Result<Vec<(Vec<String>, u64)>, String> {
    if folder.join("parts.json").exists() {
        let parts: Vec<(Vec<String>, u64)> =
            serde_json::from_str(&library::text(&folder.join("parts.json"))?)
                .map_err(|e| format!("This recording's parts can't be read: {e}"))?;
        // A part is listed before capture starts (recording.rs); after a crash a track it never
        // opened is missing.
        return Ok(parts
            .into_iter()
            .map(|(files, offset)| {
                let files = files
                    .iter()
                    .filter_map(|file| Path::new(file).file_name().map(|name| folder.join(name)))
                    .filter(|file| file.exists())
                    .map(|file| file.to_string_lossy().into_owned())
                    .collect::<Vec<_>>();
                (files, offset)
            })
            .filter(|(files, _)| !files.is_empty())
            .collect());
    }
    let mut sources = fs::read_dir(folder)
        .map_err(|e| e.to_string())?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name.starts_with("source.")
                && !name.starts_with("source.16k.")
                && !name.ends_with(".csv")
        })
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    sources.truncate(1);
    Ok(if sources.is_empty() {
        vec![]
    } else {
        vec![(sources, 0)]
    })
}

/// Transcribe again (owner 2026-10-21): whisper runs on the saved audio once more and replaces
/// the transcript, the user's corrections included (the UI asks first); what was shown before is
/// kept in `transcript.prev.txt`. The notes don't change: Write the notes again is separate.
pub(crate) fn retranscribe(root: &Path, id: &str, stage: &dyn Fn(Stage)) -> Result<(), String> {
    let folder = library::folder(root, id)?;
    if crate::bookmarks::locked(root, id)? {
        return Err(crate::bookmarks::LOCKED.into());
    }
    let parts = saved_audio(&folder)?;
    if parts.is_empty() {
        return Err("This meeting has no saved audio to transcribe".into());
    }
    motetto_core::tools::reset();
    stage(Stage::Transcribing);
    let heard =
        motetto_core::transcribe::transcript_parts(&parts, true).map_err(|e| e.to_string())?;
    let transcript = heard.text;
    if transcript.trim().is_empty() {
        return Err("No speech was heard in the saved audio. The transcript is unchanged.".into());
    }
    let fixed = match motetto_core::config::var("FIX_MODEL").filter(|v| !v.is_empty()) {
        Some(model) => Some(
            motetto_core::correct::transcript(&model, &transcript).map_err(|e| e.to_string())?,
        ),
        None => None,
    };
    stage(Stage::Saving);
    let previous = crate::transcript_edits::current(&folder)?;
    fs::write(folder.join("transcript.prev.txt"), previous).map_err(|e| e.to_string())?;
    let next = folder.join("transcript.txt.new");
    fs::write(&next, &transcript).map_err(|e| e.to_string())?;
    fs::rename(&next, folder.join("transcript.txt")).map_err(|e| e.to_string())?;
    for stale in [crate::transcript_edits::EDITED, "transcript.fixed.txt"] {
        if folder.join(stale).exists() {
            fs::remove_file(folder.join(stale)).map_err(|e| e.to_string())?;
        }
    }
    if let Some(fixed) = fixed {
        fs::write(folder.join("transcript.fixed.txt"), fixed).map_err(|e| e.to_string())?;
    }
    let mut summary = library::summary(root, id)?;
    summary.language = heard.language;
    library::save(root, &summary)
}

/// `stage` reports the step being done, for the meeting's Writing the notes again page.
pub(crate) fn regenerate(root: &Path, id: &str, stage: &dyn Fn(Stage)) -> Result<(), String> {
    let folder = library::folder(root, id)?;
    if crate::bookmarks::locked(root, id)? {
        return Err(crate::bookmarks::LOCKED.into());
    }
    let mut meeting = library::read(root, id)?;
    let mut language = meeting.summary.language.clone();
    // A recording that was never processed (Discard, then restored) is transcribed first, from the
    // parts it saved, as recording.rs would have done.
    if meeting.transcript.trim().is_empty() && folder.join("parts.json").exists() {
        let parts = saved_audio(&folder)?;
        motetto_core::tools::reset();
        stage(Stage::Transcribing);
        let heard =
            motetto_core::transcribe::transcript_parts(&parts, true).map_err(|e| e.to_string())?;
        fs::write(folder.join("transcript.txt"), &heard.text).map_err(|e| e.to_string())?;
        meeting.transcript = heard.text;
        language = heard.language;
    }
    // The user's corrections win over the optional model fix (SPEC 6.3).
    let transcript = if folder.join(crate::transcript_edits::EDITED).exists() {
        meeting.transcript.clone()
    } else if folder.join("transcript.fixed.txt").exists() {
        library::text(&folder.join("transcript.fixed.txt"))?
    } else {
        meeting.transcript.clone()
    };
    if meeting.transcript.trim().is_empty() {
        return Err("This meeting has no transcript to summarize".into());
    }
    motetto_core::tools::reset(); // a Stop pressed for an earlier job doesn't stop this one
    let staging = staging(&folder)?;
    let result = (|| -> Result<(), String> {
        let started = std::time::Instant::now();
        let spellings = library::used_spellings();
        let notes = if folder.join("my-notes.md").exists() {
            library::text(&folder.join("my-notes.md"))?
        } else {
            String::new()
        };
        stage(Stage::Summarizing { part: 1, of: 1 });
        let title = motetto_core::summarize::notes(
            &staging.join("notes").to_string_lossy(),
            &transcript,
            &notes,
            language.as_deref(),
            stage,
        )
        .map_err(|e| e.to_string())?;
        if !notes.trim().is_empty() {
            use std::io::Write;
            let mut output = fs::OpenOptions::new()
                .append(true)
                .open(staging.join("notes.md"))
                .map_err(|e| e.to_string())?;
            write!(output, "\n\n## My notes\n\n{notes}").map_err(|e| e.to_string())?;
        }
        if library::read(root, id)?.notes_md != meeting.notes_md {
            return Err(
                "Notes changed while regenerating. Your edits were kept; try again.".into(),
            );
        }
        stage(Stage::Saving);
        fs::write(staging.join("previous.md"), meeting.notes_md).map_err(|e| e.to_string())?;
        fs::rename(staging.join("previous.md"), folder.join("notes.prev.md"))
            .map_err(|e| e.to_string())?;
        fs::rename(staging.join("notes.md"), folder.join("notes.md")).map_err(|e| e.to_string())?;
        let mut summary = library::summary(root, id)?;
        summary.notes_ms = Some(started.elapsed().as_millis() as u64);
        summary.spellings = spellings;
        summary.titled(title);
        summary.language = language;
        // Notes written: a recording that needed them (Discard, then Undo) is ready now.
        summary.status = "Ready".into();
        library::save(root, &summary)
    })();
    let _ = fs::remove_dir_all(staging);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn regeneration_without_transcript_preserves_notes() {
        let root =
            std::env::temp_dir().join(format!("motetto-no-transcript-{}", std::process::id()));
        fs::create_dir_all(root.join("123")).unwrap();
        fs::write(
            root.join("123/meeting.json"),
            r#"{"id":"123","title":"Test","created_ms":123,"status":"Ready"}"#,
        )
        .unwrap();
        fs::write(root.join("123/notes.md"), "Keep edited notes").unwrap();
        assert!(
            regenerate(&root, "123", &|_| {})
                .unwrap_err()
                .contains("no transcript")
        );
        assert_eq!(
            fs::read_to_string(root.join("123/notes.md")).unwrap(),
            "Keep edited notes"
        );
        assert!(!root.join("123/.regenerating").exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn saved_audio_finds_parts_by_name_and_an_import_source() {
        let root = std::env::temp_dir().join(format!("motetto-saved-audio-{}", std::process::id()));
        // A recording whose stored paths point at a folder that has since moved; one track missing.
        fs::create_dir_all(root.join("rec")).unwrap();
        fs::write(root.join("rec/part-0000.mic.wav"), b"x").unwrap();
        fs::write(
            root.join("rec/parts.json"),
            r#"[[["/old/place/part-0000.mic.wav","/old/place/part-0000.system.wav"],0],[["/old/place/part-0001.mic.wav"],60000]]"#,
        )
        .unwrap();
        assert_eq!(
            saved_audio(&root.join("rec")).unwrap(),
            vec![(
                vec![
                    root.join("rec/part-0000.mic.wav")
                        .to_string_lossy()
                        .into_owned()
                ],
                0
            )]
        );
        // An import keeps its original; the 16 kHz copy and whisper's CSV are not sources.
        fs::create_dir_all(root.join("imp")).unwrap();
        for name in [
            "source.m4a",
            "source.16k.wav",
            "source.csv",
            "transcript.txt",
        ] {
            fs::write(root.join("imp").join(name), b"x").unwrap();
        }
        assert_eq!(
            saved_audio(&root.join("imp")).unwrap(),
            vec![(
                vec![root.join("imp/source.m4a").to_string_lossy().into_owned()],
                0
            )]
        );
        // Audio deleted: nothing to transcribe, and the transcript is left alone.
        fs::create_dir_all(root.join("789")).unwrap();
        fs::write(
            root.join("789/meeting.json"),
            r#"{"id":"789","title":"Test","created_ms":1,"status":"Ready"}"#,
        )
        .unwrap();
        fs::write(root.join("789/transcript.txt"), "[00:01] Me: keep").unwrap();
        assert!(saved_audio(&root.join("789")).unwrap().is_empty());
        let error = retranscribe(&root, "789", &|_| {}).unwrap_err();
        assert!(error.contains("no saved audio"), "{error}");
        assert_eq!(
            fs::read_to_string(root.join("789/transcript.txt")).unwrap(),
            "[00:01] Me: keep"
        );
        assert!(!root.join("789/transcript.prev.txt").exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn staging_replaces_a_folder_left_by_a_cut_off_run() {
        let root = std::env::temp_dir().join(format!("motetto-staging-{}", std::process::id()));
        fs::create_dir_all(root.join(".regenerating")).unwrap();
        fs::write(root.join(".regenerating/notes.md"), "half written").unwrap();
        fs::write(root.join("notes.md"), "Keep these notes").unwrap();
        let staging = staging(&root).unwrap();
        assert!(fs::read_dir(&staging).unwrap().next().is_none());
        assert_eq!(
            fs::read_to_string(root.join("notes.md")).unwrap(),
            "Keep these notes"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
