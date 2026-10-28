//! Meeting audio playback outside the web view on macOS. WebKit plays the WAVs in its GPU
//! process, reading their bytes through the page's process; a play() there can wait on the GPU
//! process while that process waits on a pending read, and the window freezes for good. Here
//! the tracks play from disk in AVAudioEngine on one clock. Elsewhere `playback_open` answers
//! false and the page keeps its audio elements.
use crate::{library, library_commands::Library, meeting_media};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Sender},
};
use tauri::Manager;

enum Command {
    Open(String, Vec<(PathBuf, u64)>),
    Set {
        id: String,
        playing: bool,
        at_ms: Option<f64>,
        rate: f64,
    },
    Position(String),
    Close(String),
}
type Job = (Command, Sender<Result<f64, String>>);

/// One worker thread owns the engine; AVFoundation objects never cross threads.
pub struct Playback(Sender<Job>);

impl Playback {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::spawn(move || {
            let mut player = Player::default();
            for (command, reply) in rx {
                let _ = reply.send(player.run(command));
            }
        });
        Self(tx)
    }
    fn ask(&self, command: Command) -> Result<f64, String> {
        let (tx, rx) = mpsc::channel();
        self.0
            .send((command, tx))
            .map_err(|_| "Playback stopped".to_string())?;
        rx.recv().map_err(|_| "Playback stopped".to_string())?
    }
}

/// Commands for a meeting that is no longer open (a late pause after switching) do nothing.
#[cfg(target_os = "macos")]
fn current<'a>(
    id: &str,
    open: &'a mut Option<(String, crate::playback_macos::Engine)>,
) -> Option<&'a mut crate::playback_macos::Engine> {
    open.as_mut()
        .filter(|(open, _)| open == id)
        .map(|(_, engine)| engine)
}

#[derive(Default)]
struct Player {
    #[cfg(target_os = "macos")]
    open: Option<(String, crate::playback_macos::Engine)>,
}

impl Player {
    /// AVFoundation reports misuse (or a device it can no longer drive) by throwing, which would
    /// abort the app: a throw closes the player and comes back as an error.
    #[cfg(target_os = "macos")]
    fn run(&mut self, command: Command) -> Result<f64, String> {
        objc2::exception::catch(std::panic::AssertUnwindSafe(|| self.command(command)))
            .unwrap_or_else(|e| {
                // Stopping a broken engine can throw too.
                let _ = objc2::exception::catch(std::panic::AssertUnwindSafe(|| self.open = None));
                Err(e.map_or("Audio playback failed".to_string(), |e| {
                    format!("Audio playback failed: {e:?}")
                }))
            })
    }
    #[cfg(target_os = "macos")]
    fn command(&mut self, command: Command) -> Result<f64, String> {
        match command {
            Command::Open(id, tracks) => {
                self.open = None;
                self.open = Some((id, crate::playback_macos::Engine::open(&tracks)?));
                Ok(1.0)
            }
            Command::Set {
                id,
                playing,
                at_ms,
                rate,
            } => match current(&id, &mut self.open) {
                Some(engine) => engine.set(playing, at_ms, rate),
                None => Ok(at_ms.unwrap_or(0.0)),
            },
            Command::Position(id) => current(&id, &mut self.open).map_or(Ok(0.0), |e| e.poll()),
            Command::Close(id) => {
                if current(&id, &mut self.open).is_some() {
                    self.open = None;
                }
                Ok(0.0)
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    fn run(&mut self, command: Command) -> Result<f64, String> {
        Ok(match command {
            Command::Open(..) => 0.0,
            Command::Set { at_ms, .. } => at_ms.unwrap_or(0.0),
            Command::Position(_) | Command::Close(_) => 0.0,
        })
    }
}

/// Opens other audio (a rehearsal) under its own key; `playback_set` and the rest then take it.
pub fn open_tracks(
    app: &tauri::AppHandle,
    key: String,
    tracks: Vec<(PathBuf, u64)>,
) -> Result<(), String> {
    app.state::<Playback>()
        .ask(Command::Open(key, tracks))
        .map(|_| ())
}

/// True when this meeting's audio plays natively; false means the page plays it.
#[tauri::command]
pub async fn playback_open(app: tauri::AppHandle, id: String) -> Result<bool, String> {
    if !cfg!(target_os = "macos") {
        return Ok(false);
    }
    let root = app.state::<Library>().root.clone();
    let lookup = id.clone();
    let tracks = tauri::async_runtime::spawn_blocking(move || {
        meeting_media::audio(&library::folder(&root, &lookup)?)
    })
    .await
    .map_err(|e| e.to_string())??;
    let playback = app.state::<Playback>();
    Ok(playback.ask(Command::Open(id, tracks))? > 0.0)
}

/// Plays or pauses at `rate`, from `at_ms` when given; returns the meeting position in ms.
#[tauri::command]
pub fn playback_set(
    playback: tauri::State<'_, Playback>,
    id: String,
    playing: bool,
    at_ms: Option<f64>,
    rate: f64,
) -> Result<f64, String> {
    if !(0.5..=4.0).contains(&rate) || at_ms.is_some_and(|ms| !ms.is_finite() || ms < 0.0) {
        return Err("Invalid playback request".into());
    }
    playback.ask(Command::Set {
        id,
        playing,
        at_ms,
        rate,
    })
}

#[tauri::command]
pub fn playback_position(playback: tauri::State<'_, Playback>, id: String) -> Result<f64, String> {
    playback.ask(Command::Position(id))
}

#[tauri::command]
pub fn playback_close(playback: tauri::State<'_, Playback>, id: String) -> Result<(), String> {
    playback.ask(Command::Close(id)).map(|_| ())
}
