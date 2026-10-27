//! Library IPC and import orchestration.
use crate::{desktop::refresh_tray, library};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;

pub(crate) struct Library {
    pub(crate) root: PathBuf,
    pub(crate) busy: Arc<AtomicBool>,
    pub(crate) importing: AtomicBool,
    pub(crate) allow_exit: Arc<AtomicBool>,
    pub(crate) quit_pending: AtomicBool,
    /// The meeting an import or Write the notes again is writing; `id` is None while the file
    /// picker is open. Only this meeting (and a live call's) is closed to edits.
    pub(crate) job: Mutex<Option<Job>>,
    /// Edits wait for each other, never for processing (owner, 2026-10-20).
    pub(crate) edits: Mutex<()>,
}

#[derive(Clone, serde::Serialize)]
pub(crate) struct Job {
    pub(crate) id: Option<String>,
    /// An import is cancelled (its meeting goes); the others are stopped (`stop_processing`).
    pub(crate) import: bool,
    /// Transcribe again rather than Write the notes again; the page shows its own steps.
    pub(crate) transcribe: bool,
}

impl Library {
    /// Starts a job, after edits in flight have saved, so none lands under it.
    pub(crate) fn set_job(&self, job: Option<Job>) {
        let _edits = self.edits.lock().unwrap_or_else(|e| e.into_inner());
        *self.job.lock().unwrap_or_else(|e| e.into_inner()) = job;
    }
    fn job_id(&self) -> Option<String> {
        self.job
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()?
            .id
            .clone()
    }
}

/// Notes, transcript lines, ticked items, delete and restore: other meetings stay usable while a
/// call records or a file imports; only the meeting being written is refused.
pub(crate) fn edit<T>(
    app: &tauri::AppHandle,
    id: &str,
    change: impl FnOnce(&Path) -> Result<T, String>,
) -> Result<T, String> {
    let state = app.state::<Library>();
    let session = app.state::<crate::controller::Controller>().state()?;
    if crate::meeting_actions::live_id(&session) == Some(id)
        || state.job_id().as_deref() == Some(id)
    {
        return Err("This meeting is still recording or writing its notes".into());
    }
    let _edits = state.edits.lock().unwrap_or_else(|e| e.into_inner());
    change(&state.root)
}

#[tauri::command]
pub(crate) fn list_meetings(
    state: tauri::State<'_, Library>,
) -> Result<Vec<library::MeetingSummary>, String> {
    #[cfg(debug_assertions)]
    eprintln!("motetto: main window connected");
    library::list(&state.root)
}
#[tauri::command]
pub(crate) fn library_bytes(state: tauri::State<'_, Library>) -> Result<u64, String> {
    library::bytes(&state.root)
}
#[tauri::command]
pub(crate) fn get_meeting(
    id: String,
    state: tauri::State<'_, Library>,
) -> Result<library::Meeting, String> {
    library::read(&state.root, &id)
}
#[tauri::command]
pub(crate) async fn open_action_items(
    app: tauri::AppHandle,
) -> Result<Vec<(library::MeetingSummary, String)>, String> {
    let root = app.state::<Library>().root.clone();
    tauri::async_runtime::spawn_blocking(move || library::open_items(&root))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(crate) fn importing(state: tauri::State<'_, Library>) -> Option<Job> {
    state.job.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

const MEDIA: &[&str] = &[
    "wav", "mp3", "m4a", "aac", "mp4", "mov", "flac", "aiff", "ogg", "opus", "webm",
];

#[tauri::command]
pub(crate) async fn import_audio(
    app: tauri::AppHandle,
) -> Result<Option<library::MeetingSummary>, String> {
    import_file(app, None).await
}

/// Imports a picked file, or one dropped on the library window. Drops arrive from the native
/// window event, so the web page never hands Rust a path.
pub(crate) async fn import_file(
    app: tauri::AppHandle,
    dropped: Option<PathBuf>,
) -> Result<Option<library::MeetingSummary>, String> {
    if let Some(path) = &dropped
        && !path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| MEDIA.contains(&e.to_ascii_lowercase().as_str()))
    {
        return Err("Drop an audio or video file (wav, mp3, m4a, mp4, mov…).".into());
    }
    let state = app.state::<Library>();
    if state.busy.swap(true, Ordering::SeqCst) {
        return Err("A meeting is already being processed".into());
    }
    motetto_core::tools::reset();
    state.importing.store(true, Ordering::SeqCst);
    state.set_job(Some(Job {
        id: None,
        import: true,
        transcribe: false,
    }));
    refresh_tray(&app);
    let _ = app.emit("motetto://library-changed", ());
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let source = match dropped {
            Some(path) => path,
            None => {
                let Some(file) = worker
                    .dialog()
                    .file()
                    .add_filter("Audio or video", MEDIA)
                    .blocking_pick_file()
                else {
                    return Ok(None);
                };
                file.into_path().map_err(|e| e.to_string())?
            }
        };
        let library = worker.state::<Library>();
        // The meeting shows in the list as soon as its folder exists; the rest of the app stays usable.
        library::import(&library.root, &source, |id| {
            library.set_job(Some(Job {
                id: Some(id.into()),
                import: true,
                transcribe: false,
            }));
            let _ = worker.emit("motetto://library-changed", ());
        })
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r);
    if result.as_ref().is_ok_and(|r| r.is_some())
        && let Err(error) = app.state::<crate::settings::Store>().get().and_then(|s| {
            crate::settings_storage::retain(&state.root, &s.keep_audio, &s.keep_video)
        })
    {
        let _ = app.emit(
            "motetto://notice",
            format!("Notes are ready, but audio retention failed: {error}"),
        );
    }
    state.set_job(None);
    state.importing.store(false, Ordering::SeqCst);
    state.busy.store(false, Ordering::SeqCst);
    refresh_tray(&app);
    let _ = app.emit("motetto://library-changed", ());
    match &result {
        Ok(Some(meeting)) => crate::notifications::send(
            &app,
            &meeting.title,
            "Your meeting notes are ready. Open Motetto to read them.",
        ),
        Err(_) => crate::notifications::send(
            &app,
            "Couldn’t write the notes",
            "Open Motetto to review the problem. Your source file is unchanged.",
        ),
        _ => {}
    }
    if state.quit_pending.load(Ordering::SeqCst) {
        state.allow_exit.store(true, Ordering::SeqCst);
        app.exit(0);
    }
    result
}

/// Stop on a meeting's steps page (owner 2026-10-25): the notes or transcript being written for
/// `id` stop. Writing or transcribing again keeps what was there (its staging is dropped); after
/// a call the meeting waits in Needs attention with its recording and transcript. Only that
/// meeting's job is stopped: an id that isn't being processed does nothing.
#[tauri::command]
pub(crate) fn stop_processing(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let rewriting = app
        .state::<Library>()
        .job
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .is_some_and(|job| !job.import && job.id.as_deref() == Some(id.as_str()));
    let finishing = app.state::<crate::controller::Controller>().state()?.state
        == motetto_core::events::SessionState::Finishing { meeting_id: id };
    if rewriting || finishing {
        motetto_core::tools::cancel();
    }
    Ok(())
}

/// Cancel on the import's progress page: the running tool stops and the half-made meeting goes.
#[tauri::command]
pub(crate) fn cancel_import(state: tauri::State<'_, Library>) {
    if state
        .job
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .is_some_and(|job| job.import)
    {
        motetto_core::tools::cancel();
    }
}

#[tauri::command]
pub(crate) async fn search_meetings(
    app: tauri::AppHandle,
    query: String,
) -> Result<Vec<library::SearchHit>, String> {
    if query.len() > 1024 {
        return Err("Search is too long".into());
    }
    let root = app.state::<Library>().root.clone();
    tauri::async_runtime::spawn_blocking(move || library::search(&root, &query))
        .await
        .map_err(|e| e.to_string())?
}
