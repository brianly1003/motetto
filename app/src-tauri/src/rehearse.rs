//! Rehearse a script with the microphone only (SPEC 14.6): not a meeting, never in the library.
//! The audio lives in the app's data folder and is deleted when the report closes, unless kept.
use motetto_core::events::{SessionState, Stop};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
use tauri::{Emitter, Manager};
use ts_rs::TS;

#[derive(Clone, Serialize, TS)]
pub struct Rehearsal {
    pub id: String,
    pub script: String,
    /// While it runs: false. After Stop: how long it was.
    #[ts(type = "number | null")]
    pub duration_ms: Option<u64>,
}

pub struct Running {
    rehearsal: Rehearsal,
    stop: Stop,
    started: Instant,
    capture: std::thread::JoinHandle<Result<(), String>>,
}

/// The rehearsal running now, and whether one is starting (its strip is opening): the lock is
/// never held while the strip opens, since the strip's page asks Rust for state as it loads.
pub struct State(pub Mutex<Option<Running>>, pub AtomicBool);

fn root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("rehearsals"))
}

/// A rehearsal id: digits only, so it can never name a path outside the rehearsals folder.
fn folder(root: &Path, id: &str) -> Result<PathBuf, String> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
        return Err("Invalid rehearsal".into());
    }
    Ok(root.join(id))
}

/// At launch: anything left in the rehearsals folder was never kept (Motetto quit during a
/// rehearsal or with its report open), so it goes.
pub fn sweep(root: &Path) {
    let _ = fs::remove_dir_all(root);
}

/// Refused while a meeting records: one microphone, one recording at a time.
pub fn may_start(state: &SessionState, running: bool) -> Result<(), String> {
    if running {
        return Err("A rehearsal is already running.".into());
    }
    match state {
        SessionState::Idle => Ok(()),
        _ => Err("Stop the recording to rehearse.".into()),
    }
}

#[tauri::command]
pub fn rehearse_start(app: tauri::AppHandle, script: String) -> Result<Rehearsal, String> {
    let managed = app.state::<State>();
    {
        let running = managed.0.lock().map_err(|_| "Rehearsal is unavailable")?;
        let session = app.state::<crate::controller::Controller>().state()?.state;
        may_start(
            &session,
            running.is_some() || managed.1.load(Ordering::SeqCst),
        )?;
        managed.1.store(true, Ordering::SeqCst);
    }
    let started = start(&app, script);
    managed.1.store(false, Ordering::SeqCst);
    started
}

fn start(app: &tauri::AppHandle, script: String) -> Result<Rehearsal, String> {
    let app = app.clone();
    let id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis()
        .to_string();
    let dir = folder(&root(&app)?, &id)?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let base = dir.join("rehearsal").to_string_lossy().into_owned();
    let rehearsal = Rehearsal {
        id,
        script: script.clone(),
        duration_ms: None,
    };
    // Announced on the strip before the microphone opens (never record silently), and never
    // tucked: the Tucked capsule has no room for the announcement and its Stop.
    let tucked = app
        .state::<crate::settings::Store>()
        .get()
        .is_ok_and(|s| s.prompter.layout == crate::prompter::Layout::Tucked);
    if tucked {
        crate::prompter::prompter_layout(app.clone(), crate::prompter::Layout::Strip)?;
    }
    crate::prompter::prompter_show(app.clone(), Some(script))?;
    let _ = app.emit("motetto://rehearsal", &rehearsal);
    let stop = Stop::default();
    let mic = crate::microphone::for_capture(&app);
    let capture = {
        let (base, stop) = (base.clone(), stop.clone());
        // The live meters carry the mic listener the strip's recogniser reads.
        let meters = app.state::<crate::live::State>().meters.clone();
        std::thread::spawn(move || {
            #[cfg(target_os = "macos")]
            let captured =
                motetto_core::capture::record_mic_only(&base, stop, mic.as_deref(), Some(meters));
            #[cfg(not(target_os = "macos"))]
            let captured = {
                // Windows: a mic-only WASAPI capture is part of plan task W5; until then the
                // system track is recorded too and never read.
                let _ = mic;
                motetto_core::capture::record(&base, stop)
            };
            captured.map(|_| ()).map_err(|e| e.to_string())
        })
    };
    crate::follow::start(
        app.clone(),
        format!("{base}.mic.wav").into(),
        stop.clone(),
        true,
    );
    *app.state::<State>()
        .0
        .lock()
        .map_err(|_| "Rehearsal is unavailable")? = Some(Running {
        rehearsal: rehearsal.clone(),
        stop,
        started: Instant::now(),
        capture,
    });
    Ok(rehearsal)
}

#[tauri::command]
pub async fn rehearse_stop(app: tauri::AppHandle) -> Result<Rehearsal, String> {
    tauri::async_runtime::spawn_blocking(move || stop_asked(&app))
        .await
        .map_err(|e| e.to_string())?
}

/// You stopped it (Stop or the shortcut): the strip closes and the report opens in the library.
/// A recording starting stops it with `stop` instead, and keeps the strip for the call.
pub fn stop_asked(app: &tauri::AppHandle) -> Result<Rehearsal, String> {
    let rehearsal = stop(app)?;
    crate::prompter::prompter_hide(app.clone())?;
    Ok(rehearsal)
}

/// Stops the rehearsal running now, if any; also called when a recording starts.
pub fn stop(app: &tauri::AppHandle) -> Result<Rehearsal, String> {
    let running = app
        .state::<State>()
        .0
        .lock()
        .map_err(|_| "Rehearsal is unavailable")?
        .take()
        .ok_or("No rehearsal is running.")?;
    running.stop.store(true, Ordering::SeqCst);
    let captured = running
        .capture
        .join()
        .map_err(|_| "The rehearsal recording stopped unexpectedly")?;
    let base = folder(&root(app)?, &running.rehearsal.id)?.join("rehearsal.mic.wav");
    // The audio's own length: capture starts a moment after the clock does.
    let duration_ms = hound::WavReader::open(&base)
        .map(|r| u64::from(r.duration()) * 1000 / u64::from(r.spec().sample_rate))
        .unwrap_or_else(|_| running.started.elapsed().as_millis() as u64);
    let rehearsal = Rehearsal {
        duration_ms: Some(duration_ms),
        ..running.rehearsal
    };
    let _ = app.emit("motetto://rehearsal", &rehearsal);
    // The report opens in the library, wherever it was.
    crate::desktop::show(app);
    if let Err(error) = captured {
        let _ = discard(app, &rehearsal.id);
        return Err(format!(
            "The rehearsal couldn't record the microphone: {error}"
        ));
    }
    Ok(rehearsal)
}

/// Listen back: on macOS the native player opens it as `rehearsal:<id>` and this answers None
/// (WebKit's WAV playback can freeze the window); elsewhere the page plays the returned file.
#[tauri::command]
pub async fn rehearsal_audio(app: tauri::AppHandle, id: String) -> Result<Option<String>, String> {
    let mic = folder(&root(&app)?, &id)?.join("rehearsal.mic.wav");
    if cfg!(target_os = "macos") {
        tauri::async_runtime::spawn_blocking(move || {
            crate::playback::open_tracks(&app, format!("rehearsal:{id}"), vec![(mic, 0)])
        })
        .await
        .map_err(|e| e.to_string())??;
        return Ok(None);
    }
    app.asset_protocol_scope()
        .allow_file(&mic)
        .map_err(|e| e.to_string())?;
    Ok(Some(mic.to_string_lossy().into_owned()))
}

/// What you said, as `[mm:ss] Me: …` lines, from the whole rehearsal at full quality.
#[tauri::command]
pub async fn rehearsal_transcript(app: tauri::AppHandle, id: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = folder(&root(&app)?, &id)?;
        let mic = dir.join("rehearsal.mic.wav").to_string_lossy().into_owned();
        motetto_core::transcribe::transcript_parts(&[(vec![mic], 0)], true, &|_| {})
            .map(|t| t.text)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A shorter version of a line that ran long, from the notes model on this computer; None when
/// its answer isn't a shorter line (SPEC 14.6).
#[tauri::command]
pub async fn rehearsal_shorten(line: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        motetto_core::shorten::line(&line).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The report closed without Keep: the audio goes.
#[tauri::command]
pub fn rehearsal_discard(app: tauri::AppHandle, id: String) -> Result<(), String> {
    discard(&app, &id)
}
fn discard(app: &tauri::AppHandle, id: &str) -> Result<(), String> {
    let dir = folder(&root(app)?, id)?;
    match fs::remove_dir_all(dir) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
}

/// Keep this recording: moves it next to the script, `Scripts/<name>.rehearsals/<id>/`.
#[tauri::command]
pub fn rehearsal_keep(app: tauri::AppHandle, id: String, script: String) -> Result<(), String> {
    let library = app.state::<crate::library_commands::Library>().root.clone();
    keep(&root(&app)?, &library, &id, &script)
}
fn keep(root: &Path, library: &Path, id: &str, script: &str) -> Result<(), String> {
    let source = folder(root, id)?;
    let name = crate::scripts::checked_name(script)?;
    let target = library
        .join("Scripts")
        .join(format!("{name}.rehearsals"))
        .join(id);
    let parent = target.parent().ok_or("Invalid folder")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    if fs::rename(&source, &target).is_err() {
        // Another volume: copy the audio under a temporary name, rename it into place, and
        // only then remove the original, so a failed copy never leaves a half recording.
        let partial = parent.join(format!(".{id}.partial"));
        let copied = (|| -> std::io::Result<()> {
            fs::create_dir_all(&partial)?;
            for entry in fs::read_dir(&source)? {
                let entry = entry?;
                if entry.file_type()?.is_file() {
                    fs::copy(entry.path(), partial.join(entry.file_name()))?;
                }
            }
            fs::rename(&partial, &target)
        })();
        if let Err(error) = copied {
            let _ = fs::remove_dir_all(&partial);
            return Err(format!("Couldn't keep the recording: {error}"));
        }
        fs::remove_dir_all(&source).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Only a recording starting ends a rehearsal; a call being asked about may be declined.
pub fn ends_rehearsal(state: &SessionState) -> bool {
    *state == SessionState::Recording
}

/// A recording starting ends a rehearsal: one microphone at a time.
pub fn on_state(app: &tauri::AppHandle, state: &motetto_core::events::AppState) {
    let rehearsing = app
        .state::<State>()
        .0
        .lock()
        .is_ok_and(|running| running.is_some());
    // A recording starting, not a call being asked about: the Ask card may be declined.
    if rehearsing && ends_rehearsal(&state.state) {
        let app = app.clone();
        std::thread::spawn(move || {
            let _ = stop(&app);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("motetto-rehearse-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn exports_bindings() {
        Rehearsal::export_all(&ts_rs::Config::default().with_out_dir("../../ui/lib/bindings"))
            .unwrap();
    }

    #[test]
    fn rehearse_refused_while_recording() {
        assert!(may_start(&SessionState::Idle, false).is_ok());
        assert_eq!(
            may_start(&SessionState::Recording, false).unwrap_err(),
            "Stop the recording to rehearse."
        );
        assert!(may_start(&SessionState::Paused, false).is_err());
        assert!(may_start(&SessionState::Idle, true).is_err());
    }

    #[test]
    fn only_a_recording_ends_a_rehearsal() {
        assert!(ends_rehearsal(&SessionState::Recording));
        for state in [
            SessionState::Idle,
            SessionState::Paused,
            SessionState::Asking {
                call_id: "c1".into(),
                app: "Zoom".into(),
            },
        ] {
            assert!(!ends_rehearsal(&state), "{state:?}");
        }
    }

    #[test]
    fn ids_never_leave_the_folder() {
        let root = Path::new("/r");
        assert!(folder(root, "../x").is_err());
        assert!(folder(root, "").is_err());
        assert!(folder(root, "12/3").is_err());
        assert_eq!(folder(root, "1793").unwrap(), Path::new("/r/1793"));
    }

    #[test]
    fn kept_rehearsal_moves_atomically() {
        let (root, library) = (temp("root"), temp("library"));
        fs::create_dir_all(root.join("42")).unwrap();
        fs::write(root.join("42/rehearsal.mic.wav"), b"audio").unwrap();
        keep(&root, &library, "42", "Team X demo").unwrap();
        assert!(!root.join("42").exists());
        let kept = library.join("Scripts/Team X demo.rehearsals/42/rehearsal.mic.wav");
        assert_eq!(fs::read(kept).unwrap(), b"audio");
        assert!(keep(&root, &library, "42", "../bad").is_err());
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(library).unwrap();
    }

    #[test]
    fn quit_during_rehearsal_deletes_audio() {
        let root = temp("sweep");
        fs::create_dir_all(root.join("7")).unwrap();
        fs::write(root.join("7/rehearsal.mic.wav"), b"audio").unwrap();
        sweep(&root);
        assert!(!root.exists());
        sweep(&root); // nothing left: no error
    }
}
