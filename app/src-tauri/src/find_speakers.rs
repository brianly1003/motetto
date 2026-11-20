//! Find speakers (SPEC 7.11): the call side of a meeting's transcript is labelled by voice from
//! the saved audio, without transcribing again, so the user's corrections stay. The speaker
//! models download on first use.
use crate::{catalog::catalog, download, library, model_store, settings::Store};
use motetto_core::events::Stage;
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};
use tauri::Manager;

/// The most people the Find speakers question offers.
pub(crate) const MOST_PEOPLE: usize = 12;

pub(crate) const ONE_VOICE: &str =
    "Only one voice was heard in the call audio, so the transcript is unchanged.";

/// Labels meeting `id`; the transcript is replaced only when two or more voices were heard, or
/// the user said one other person and earlier labels go back to one voice (the
/// one shown before is kept in `transcript.prev.txt`, as Transcribe again does). `people`: how
/// many other people were on the call, when the user said.
pub(crate) fn run(
    app: &tauri::AppHandle,
    root: &Path,
    id: &str,
    people: Option<usize>,
    stage: &(dyn Fn(Stage) + Sync),
) -> Result<(), String> {
    let folder = library::folder(root, id)?;
    if crate::bookmarks::locked(root, id)? {
        return Err(crate::bookmarks::LOCKED.into());
    }
    let parts = crate::rewrite::saved_audio(&folder)?;
    if parts.is_empty() {
        return Err("This meeting has no saved audio to find speakers in".into());
    }
    // One other person needs no diarizer, so no models either.
    if people != Some(1) {
        models(app, stage)?;
    }
    motetto_core::tools::reset();
    stage(Stage::FindingSpeakers);
    let heard = library::text(&folder.join("transcript.txt"))?;
    let edited_path = folder.join(crate::transcript_edits::EDITED);
    let edited = if edited_path.exists() {
        Some(library::text(&edited_path)?)
    } else {
        None
    };
    let Some(found) = motetto_core::speakers::find(&parts, &heard, edited.as_deref(), people)
        .map_err(|e| e.to_string())?
    else {
        // Asked for one other person and the transcript already has one: done, not a failure.
        return if people == Some(1) {
            Ok(())
        } else {
            Err(ONE_VOICE.into())
        };
    };
    stage(Stage::Saving);
    crate::rewrite::save_transcript(&folder, &found.heard, found.edited, None)
}

/// The speaker pair, downloaded now if missing (about 34 MB, pinned and checked like every
/// model); afterwards the settings point the diarizer at it.
fn models(app: &tauri::AppHandle, stage: &(dyn Fn(Stage) + Sync)) -> Result<(), String> {
    let cat = catalog();
    let dir = model_store::dir(app);
    let store = app.state::<Store>();
    let adopted = store.get()?.adopted;
    let missing: Vec<_> = cat
        .speakers
        .iter()
        .filter_map(|id| cat.file(id))
        .filter(|f| model_store::path(&dir, &adopted, f).is_none())
        .collect();
    let total: u64 = missing.iter().map(|f| f.bytes).sum();
    let mut done = 0;
    // Stop on the steps page ends the download too, through the progress callback.
    let stop = AtomicBool::new(false);
    for file in missing {
        stage(Stage::GettingSpeakerModels {
            percent: percent(done, total),
        });
        let outcome = download::fetch(file, &dir, None, &stop, &mut |have, _| {
            if motetto_core::tools::cancelled() {
                stop.store(true, Ordering::SeqCst);
            }
            stage(Stage::GettingSpeakerModels {
                percent: percent(done + have, total),
            });
        })
        .map_err(|e| format!("Couldn't download the speaker models: {e}"))?;
        if !matches!(outcome, download::Outcome::Done(_)) {
            return Err("Stopped".into());
        }
        done += file.bytes;
    }
    store.change(|s| crate::setup::refresh_paths(s, cat, &dir))?;
    Ok(())
}

fn percent(have: u64, total: u64) -> u8 {
    (have * 100 / total.max(1)).min(100) as u8
}
