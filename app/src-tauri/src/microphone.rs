//! The microphone picker (owner 2026-10-23, DESIGN 4.15): what Motetto records from, switching it
//! (mid-call too: the next part records from the new mic), its input volume, the system's Sound
//! settings, and whether the mic has been too loud lately.

use crate::controller::{Action, Controller};
use motetto_core::mics::{self, MicReport};
use std::sync::{Mutex, atomic::Ordering};
use std::time::Instant;
use tauri::{Emitter, Manager};

/// Recent clipping, one entry per 2 s whichever window asks (`mics::too_loud`).
#[derive(Default)]
pub struct Clipping(Mutex<(Vec<bool>, Option<Instant>, u32)>);

impl Clipping {
    /// Adds the clips counted since the last look; a new entry at most every 2 s, so three
    /// windows polling don't shorten the window.
    fn too_loud(&self, clips: u32) -> bool {
        let Ok(mut state) = self.0.lock() else {
            return false;
        };
        let (recent, last, pending) = &mut *state;
        *pending = pending.saturating_add(clips);
        if last.is_none_or(|at| at.elapsed().as_millis() >= 2_000) {
            recent.push(*pending > 0);
            let keep = recent.len().saturating_sub(5);
            recent.drain(..keep);
            *pending = 0;
            *last = Some(Instant::now());
        }
        mics::too_loud(recent)
    }
}

fn report(app: &tauri::AppHandle) -> Result<MicReport, String> {
    let chosen = app.state::<crate::settings::Store>().get()?.mic_device;
    let clips = app
        .state::<crate::live::State>()
        .meters
        .mic_clips
        .swap(0, Ordering::Relaxed);
    let loud = app.state::<Clipping>().too_loud(clips);
    Ok(mics::report(mics::list()?, chosen, loud))
}

/// The mic a capture should use: the chosen one while it's connected, else the system default
/// (said once, so a recording never silently uses another mic than the one picked).
pub fn for_capture(app: &tauri::AppHandle) -> Option<String> {
    let chosen = app
        .state::<crate::settings::Store>()
        .get()
        .ok()?
        .mic_device?;
    let connected = mics::list()
        .map(|all| all.iter().any(|m| m.id == chosen))
        .unwrap_or(false);
    if !connected {
        let _ = app.emit(
            "motetto://notice",
            "Your chosen microphone isn't connected. Recording from the system default.",
        );
    }
    connected.then_some(chosen)
}

#[tauri::command]
pub async fn microphones(app: tauri::AppHandle) -> Result<MicReport, String> {
    tauri::async_runtime::spawn_blocking(move || report(&app))
        .await
        .map_err(|e| e.to_string())?
}

/// Saves the choice (None = system default); during a recording the next part uses it at once.
#[tauri::command]
pub async fn set_microphone(
    app: tauri::AppHandle,
    id: Option<String>,
) -> Result<MicReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<crate::settings::Store>()
            .change(|s| s.mic_device = id.clone())?;
        if let Ok(mut clipping) = app.state::<Clipping>().0.lock() {
            clipping.0.clear();
        }
        app.state::<Controller>().action(Action::SwitchMic)?;
        let _ = app.emit("motetto://mic-changed", ());
        report(&app)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Settings › Audio › Microphone mode, from the screen setup bar's mic menu: how Motetto prepares
/// its copy of the mic after the call (meetings started from now on).
#[tauri::command]
pub async fn set_mic_mode(
    app: tauri::AppHandle,
    mode: motetto_core::transcribe::MicMode,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<crate::settings::Store>()
            .change(|s| s.mic_mode = mode)
            .map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The recording's own mic, None included (a screen recording, or a call where only the other
/// side matters): the next part records with it. The saved choice in Settings stays as it is.
#[tauri::command]
pub async fn set_mic_choice(
    app: tauri::AppHandle,
    choice: motetto_core::events::MicChoice,
) -> Result<MicReport, String> {
    use motetto_core::events::MicChoice;
    if let MicChoice::Device { id } = &choice
        && (id.is_empty() || id.len() > 255 || id.contains('\0'))
    {
        return Err("Unknown microphone".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Controller>().action(Action::SetMic(choice))?;
        let _ = app.emit("motetto://mic-changed", ());
        report(&app)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The same input volume as the system's Sound settings (0.0-1.0).
#[tauri::command]
pub async fn set_mic_volume(
    app: tauri::AppHandle,
    id: String,
    volume: f32,
) -> Result<MicReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        mics::set_volume(&id, volume)?;
        if let Ok(mut clipping) = app.state::<Clipping>().0.lock() {
            clipping.0.clear();
        }
        let _ = app.emit("motetto://mic-changed", ());
        report(&app)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn open_sound_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.Sound-Settings.extension?input")
        .status();
    #[cfg(target_os = "windows")]
    let status = std::process::Command::new("explorer")
        .arg("ms-settings:sound")
        .status();
    match status {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Couldn't open Sound settings: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipping_counts_once_per_two_seconds_however_many_windows_ask() {
        let clipping = Clipping::default();
        // Three windows ask within the same 2 s: one entry, holding all their clips.
        assert!(!clipping.too_loud(5));
        assert!(!clipping.too_loud(7));
        assert!(!clipping.too_loud(0));
        assert_eq!(clipping.0.lock().unwrap().0, vec![true]);
        // Later entries arrive as time passes.
        clipping.0.lock().unwrap().1 = Some(Instant::now() - std::time::Duration::from_secs(3));
        assert!(!clipping.too_loud(1));
        clipping.0.lock().unwrap().1 = Some(Instant::now() - std::time::Duration::from_secs(3));
        assert!(clipping.too_loud(1));
    }
}
