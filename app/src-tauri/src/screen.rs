//! Screen source selection and the native window watcher; audio scope never follows the picture.
use crate::{
    controller::{Action, Controller},
    settings::Settings,
};
use motetto_core::{
    events::{ScreenChoice, ScreenPick, ScreenState, SessionState},
    video::{self, ScreenCapture, ScreenTarget},
    watch,
};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    sync::{Mutex, mpsc},
    time::Duration,
};
use tauri::Manager;
use ts_rs::TS;

static CURRENT_WINDOW: Mutex<Option<(motetto_core::events::Stop, u32)>> = Mutex::new(None);
pub fn set_window(part: &motetto_core::events::Stop, window: u32) {
    if let Ok(mut current) = CURRENT_WINDOW.lock() {
        *current = Some((part.clone(), window));
    }
}
fn current_window(part: &motetto_core::events::Stop) -> u32 {
    CURRENT_WINDOW
        .lock()
        .ok()
        .and_then(|c| {
            c.as_ref()
                .filter(|(token, _)| std::sync::Arc::ptr_eq(token, part))
                .map(|(_, id)| *id)
        })
        .unwrap_or(0)
}
static DISPLAYS: Mutex<Vec<(u32, String, f64)>> = Mutex::new(Vec::new());

#[derive(Debug, Clone, Serialize, TS)]
pub struct ScreenOption {
    pub pick: ScreenPick,
    pub sub: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ScreenSourceInfo {
    pub kind: String,
    pub name: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ScreenPart {
    pub file: String,
    #[ts(type = "number")]
    pub offset_ms: u64,
    pub source: ScreenSourceInfo,
    pub while_sharing: bool,
    pub width: u32,
    pub height: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct Moment {
    #[ts(type = "number")]
    pub ms: u64,
    pub yours: bool,
}

pub enum Resolved {
    Capture(ScreenCapture),
    NoWindow,
    LowDisk,
}

pub fn resolve(pick: &ScreenPick, settings: &Settings, path: &str, dir: &Path) -> Resolved {
    if crate::download::free_bytes(dir).unwrap_or(0) < 2_000_000_000 {
        return Resolved::LowDisk;
    }
    #[cfg(target_os = "macos")]
    {
        let rules = watch::rules();
        let target = match &pick.choice {
            ScreenChoice::Display { id } => ScreenTarget::Display(*id),
            ScreenChoice::CallWindow { app_id } => {
                let id = rules.iter().find(|r| &r.app_id == app_id).and_then(|rule| {
                    motetto_core::capture::screen_windows()
                        .ok()
                        .and_then(|windows| video::pick_window(&windows, rule, &rules))
                });
                let Some(id) = id else {
                    return Resolved::NoWindow;
                };
                ScreenTarget::Window(id)
            }
        };
        // Unknown backing scales use 1x; source enumeration populates the native display cache.
        let scale = DISPLAYS
            .lock()
            .ok()
            .and_then(|d| match target {
                ScreenTarget::Display(id) => d.iter().find(|d| d.0 == id).map(|d| d.2),
                ScreenTarget::Window(id) => motetto_core::capture::window_display(id)
                    .and_then(|id| d.iter().find(|d| d.0 == id).map(|d| d.2)),
            })
            .unwrap_or(1.0);
        Resolved::Capture(ScreenCapture {
            target,
            preset: video::Preset::parse(&settings.screen_preset),
            cursor: settings.screen_cursor,
            path: path.into(),
            scale,
            progress: None,
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (pick, settings, path);
        Resolved::NoWindow
    }
}

pub fn sources(app: &tauri::AppHandle) -> Vec<ScreenOption> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        return vec![];
    }
    #[cfg(target_os = "macos")]
    {
        let windows = motetto_core::capture::screen_windows().unwrap_or_default();
        let state = app.state::<Controller>().state().ok();
        let active = state.as_ref().and_then(|s| match &s.state {
            SessionState::Asking { app, .. } => Some(app.as_str()),
            _ => s.app.as_deref(),
        });
        let rules = watch::rules();
        let mut options: Vec<_> = rules
            .iter()
            .filter(|rule| active.is_none_or(|name| name == rule.name))
            .filter(|rule| video::pick_window(&windows, rule, &rules).is_some())
            .map(|rule| ScreenOption {
                pick: ScreenPick {
                    choice: ScreenChoice::CallWindow {
                        app_id: rule.app_id.clone(),
                    },
                    name: format!("{} window", rule.name),
                },
                sub: "Only the call, with whatever people share. Recommended.".into(),
            })
            .collect();
        let (tx, rx) = mpsc::channel();
        if app
            .run_on_main_thread(move || {
                if let Some(mtm) = objc2::MainThreadMarker::new() {
                    let _ = tx.send(motetto_core::capture::displays(mtm));
                }
            })
            .is_ok()
            && let Ok(displays) = rx.recv_timeout(Duration::from_secs(2))
        {
            if let Ok(mut cache) = DISPLAYS.lock() {
                *cache = displays.clone();
            }
            options.extend(displays.into_iter().map(|(id, name, _)| ScreenOption {
                pick: ScreenPick {
                    choice: ScreenChoice::Display { id },
                    name,
                },
                sub: "Everything on it, including notifications and other apps.".into(),
            }));
        }
        options
    }
}

fn watch_decision(
    state: &ScreenState,
    found: Option<u32>,
    present: bool,
    current: u32,
    free: u64,
) -> Option<Action> {
    if state.current().is_some() && free < 2_000_000_000 {
        return Some(Action::ScreenDiskLow);
    }
    if state.while_sharing.is_some()
        || !matches!(state.pick.choice, ScreenChoice::CallWindow { .. })
    {
        return None;
    }
    if state.waiting {
        return found.map(|_| Action::ScreenFound);
    }
    // A minimized/covered window still exists; its last frame remains the picture.
    if current != 0 && !present {
        return Some(Action::ScreenLost);
    }
    None
}

pub fn watch(app: tauri::AppHandle) -> Result<(), String> {
    std::thread::Builder::new()
        .name("motetto-screen".into())
        .spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(2));
                let Some((state, token)) = app.state::<Controller>().screen_snapshot() else {
                    continue;
                };
                if state.state != SessionState::Recording {
                    continue;
                }
                let Some(screen) = state.screen else {
                    continue;
                };
                #[cfg(target_os = "macos")]
                {
                    let Ok(windows) = motetto_core::capture::screen_windows() else {
                        continue;
                    };
                    let rules = watch::rules();
                    let found = match &screen.pick.choice {
                        ScreenChoice::CallWindow { app_id } => rules
                            .iter()
                            .find(|r| &r.app_id == app_id)
                            .and_then(|r| video::pick_window(&windows, r, &rules)),
                        _ => None,
                    };
                    let current = current_window(&token);
                    let present = windows.iter().any(|w| w.id == current);
                    let root = &app.state::<crate::library_commands::Library>().root;
                    let free = state
                        .meeting_id
                        .and_then(|id| crate::library::folder(root, &id).ok())
                        .and_then(|d| crate::download::free_bytes(&d).ok())
                        .unwrap_or(u64::MAX);
                    if let Some(action) = watch_decision(&screen, found, present, current, free) {
                        app.state::<Controller>().screen_watch(token, action);
                    }
                }
            }
        })
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn window() -> ScreenState {
        ScreenState {
            pick: ScreenPick {
                choice: ScreenChoice::CallWindow {
                    app_id: "zoom".into(),
                },
                name: "Zoom window".into(),
            },
            while_sharing: None,
            waiting: false,
        }
    }
    #[test]
    fn a_window_lookup_from_an_old_part_is_not_current() {
        let old = motetto_core::events::Stop::default();
        let current = motetto_core::events::Stop::default();
        set_window(&old, 12);
        assert_eq!(current_window(&old), 12);
        assert_eq!(current_window(&current), 0);
        set_window(&current, 34);
        assert_eq!(current_window(&old), 0);
        assert_eq!(current_window(&current), 34);
    }

    #[test]
    fn hidden_window_is_kept_and_low_disk_wins() {
        let mut state = window();
        assert!(watch_decision(&state, None, true, 1, u64::MAX).is_none());
        assert!(matches!(
            watch_decision(&state, None, false, 1, u64::MAX),
            Some(Action::ScreenLost)
        ));
        state.waiting = true;
        assert!(matches!(
            watch_decision(&state, Some(2), false, 0, u64::MAX),
            Some(Action::ScreenFound)
        ));
        state.waiting = false;
        state.while_sharing = Some(ScreenPick {
            choice: ScreenChoice::Display { id: 1 },
            name: "Display".into(),
        });
        assert!(watch_decision(&state, None, false, 1, u64::MAX).is_none());
        assert!(matches!(
            watch_decision(&state, None, false, 1, 0),
            Some(Action::ScreenDiskLow)
        ));
    }
    #[test]
    fn export_bindings() {
        let config = ts_rs::Config::default().with_out_dir("../../ui/lib/bindings");
        ScreenOption::export_all(&config).unwrap();
        ScreenPart::export_all(&config).unwrap();
        Moment::export_all(&config).unwrap();
    }
}
