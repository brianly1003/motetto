//! Global recording shortcuts use the same controller as the tray and webview.
use crate::{
    controller::{Action, Controller},
    desktop::dispatch,
};
use motetto_core::events::{CompanionSize, SessionState};
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

/// Keys that only mean something while the script strip is open: registered with it, so they
/// don't take Ctrl+Option+arrows or +/- from window managers the rest of the time.
pub const WHILE_PROMPTING: [&str; 6] = ["Down", "Up", "ShiftDown", "Backslash", "Equal", "Minus"];

/// A shortcut another app holds is a notice, never a failed launch.
pub fn install(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let keys = app.state::<crate::settings::Store>().get()?.shortcuts;
    if let Err(error) = register(app, &keys) {
        eprintln!("Motetto: {error}");
        let _ = app.emit("motetto://notice", error);
    }
    Ok(())
}
fn register_one(app: &tauri::AppHandle, key: &str, accelerator: &str) -> Result<(), String> {
    let key = key.to_string();
    app.global_shortcut()
        .on_shortcut(accelerator, move |app, _, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }
            let app = app.clone();
            let key = key.clone();
            tauri::async_runtime::spawn_blocking(move || {
                if let Err(error) = handle(&app, &key) {
                    let _ = app.emit("motetto://notice", error);
                }
            });
        })
        .map_err(|e| format!("Shortcut {accelerator} is unavailable. Choose another: {e}"))
}
/// Registers every key (the strip's own only while it is open); the first failure is returned
/// after the others are registered.
fn register(
    app: &tauri::AppHandle,
    keys: &std::collections::BTreeMap<String, String>,
) -> Result<(), String> {
    let prompting = app
        .get_webview_window("prompter")
        .is_some_and(|w| w.is_visible().unwrap_or(false));
    let mut first = Ok(());
    for (key, accelerator) in keys.iter().filter(|(_, a)| !a.is_empty()) {
        if WHILE_PROMPTING.contains(&key.as_str()) && !prompting {
            continue;
        }
        if let Err(error) = register_one(app, key, accelerator) {
            first = first.and(Err(error));
        }
    }
    first
}
/// The strip opened or closed: its own keys come and go with it.
pub fn prompting(app: &tauri::AppHandle, open: bool) {
    let Ok(keys) = app
        .state::<crate::settings::Store>()
        .get()
        .map(|s| s.shortcuts)
    else {
        return;
    };
    for key in WHILE_PROMPTING {
        let Some(accelerator) = keys.get(key).filter(|a| !a.is_empty()) else {
            continue;
        };
        let shortcuts = app.global_shortcut();
        let result = if open {
            if shortcuts.is_registered(accelerator.as_str()) {
                continue;
            }
            register_one(app, key, accelerator)
        } else {
            shortcuts
                .unregister(accelerator.as_str())
                .map_err(|e| e.to_string())
        };
        if let Err(error) = result {
            let _ = app.emit("motetto://notice", error);
        }
    }
}
pub fn replace(
    app: &tauri::AppHandle,
    old: &std::collections::BTreeMap<String, String>,
    next: &std::collections::BTreeMap<String, String>,
) -> Result<(), String> {
    app.global_shortcut()
        .unregister_all()
        .map_err(|e| e.to_string())?;
    if let Err(error) = register(app, next) {
        let _ = app.global_shortcut().unregister_all();
        register(app, old).map_err(|e| format!("{error}. Couldn't restore shortcuts: {e}"))?;
        return Err(error);
    }
    Ok(())
}
/// The prompter's keys work with or without a recording (SPEC 14.7).
fn prompter(app: &tauri::AppHandle, key: &str) -> Option<Result<(), String>> {
    use crate::prompter::{Layout, prompter_edit, prompter_layout, prompter_show};
    let window = app.get_webview_window("prompter");
    let shown = window
        .as_ref()
        .is_some_and(|w| w.is_visible().unwrap_or(false));
    Some(match key {
        "T" if !shown => prompter_show(app.clone(), None),
        "T" if app
            .state::<crate::rehearse::State>()
            .0
            .lock()
            .is_ok_and(|r| r.is_some()) =>
        {
            Err(
                "The script stays open while you rehearse: it shows that the microphone is on."
                    .into(),
            )
        }
        "T" => {
            let tucked = app
                .state::<crate::settings::Store>()
                .get()
                .is_ok_and(|s| s.prompter.layout == Layout::Tucked);
            prompter_layout(
                app.clone(),
                if tucked {
                    Layout::Strip
                } else {
                    Layout::Tucked
                },
            )
        }
        "V" => {
            let controller = app.state::<Controller>();
            controller
                .state()
                .and_then(|s| controller.action(Action::ShareVisible(!s.share_visible)))
        }
        "E" => (if shown {
            Ok(())
        } else {
            prompter_show(app.clone(), None)
        })
        .and_then(|_| prompter_edit(app.clone(), true, false)),
        "Down" | "Up" | "ShiftDown" | "Backslash" | "Equal" | "Minus" => match window {
            Some(window) if shown => window
                .emit("motetto://prompter-key", key)
                .map_err(|e| e.to_string()),
            _ => Ok(()),
        },
        "R" => {
            let running = app
                .state::<crate::rehearse::State>()
                .0
                .lock()
                .is_ok_and(|r| r.is_some());
            if running {
                crate::rehearse::stop(app).map(|_| ())
            } else {
                match app
                    .state::<crate::settings::Store>()
                    .get()
                    .ok()
                    .and_then(|s| s.prompter.last_script)
                {
                    Some(script) => {
                        crate::rehearse::rehearse_start(app.clone(), script).map(|_| ())
                    }
                    None => Err(
                        "Make a script first: in Motetto, open Scripts and paste your talk track."
                            .into(),
                    ),
                }
            }
        }
        _ => return None,
    })
}
fn handle(app: &tauri::AppHandle, key: &str) -> Result<(), String> {
    if let Some(result) = prompter(app, key) {
        return result;
    }
    let controller = app.state::<Controller>();
    let state = controller.state()?;
    if key == "S" {
        return controller.action(match state.state {
            SessionState::Recording | SessionState::Paused => Action::Stop,
            SessionState::Stopped { .. } => Action::Resume,
            SessionState::Idle => Action::Start,
            _ => return Ok(()),
        });
    }
    if state.state != SessionState::Recording {
        return Ok(());
    }
    match key {
        "M" => {
            controller.mark()?;
        }
        "J" | "N" => open_notes(app, key == "J"),
        "Space" => controller.action(Action::Size(
            if state.companion.size == CompanionSize::Tucked {
                CompanionSize::Sidecar
            } else {
                CompanionSize::Tucked
            },
        ))?,
        "H" => controller.action(Action::Sharing(state.sharing.is_none()))?,
        "P" if state.sharing.is_some() => {
            controller.action(Action::Size(if state.companion.peeking {
                CompanionSize::Dot
            } else {
                CompanionSize::Sidecar
            }))?
        }
        _ => {}
    }
    Ok(())
}
pub fn open_notes(app: &tauri::AppHandle, jot: bool) {
    let sharing = app
        .state::<Controller>()
        .state()
        .is_ok_and(|state| state.sharing.is_some());
    if !jot || !sharing {
        dispatch(app.clone(), Action::Size(CompanionSize::Sidecar));
    }
    if let Some(window) = app.get_webview_window("companion") {
        let _ = window.set_focus();
    }
    let _ = app.emit("motetto://open-notes", if jot { "jot" } else { "notes" });
}

#[cfg(test)]
mod tests {
    use super::WHILE_PROMPTING;
    use crate::settings::SHORTCUT_KEYS;

    #[test]
    fn navigation_keys_wait_for_the_strip() {
        let keys: Vec<_> = SHORTCUT_KEYS.iter().map(|(k, _)| *k).collect();
        assert!(WHILE_PROMPTING.iter().all(|k| keys.contains(k)));
        // Opening the strip, Shown/Hidden and editing must work before the strip is open.
        for always in ["S", "M", "J", "N", "Space", "H", "P", "T", "V", "E", "R"] {
            assert!(!WHILE_PROMPTING.contains(&always), "{always}");
        }
    }
}
