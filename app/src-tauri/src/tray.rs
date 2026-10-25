//! Native recording actions follow session state; elapsed-time ticks do not rebuild the menu.
use motetto_core::events::{AppState, SessionState};
use std::sync::Mutex;
use tauri::{
    Manager,
    menu::{CheckMenuItemBuilder, Menu, MenuBuilder, MenuItem, MenuItemBuilder},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Record,
    Stop,
    Paused,
    Resume,
    Asking,
    None,
}
fn action(state: &SessionState, importing: bool) -> Action {
    match state {
        SessionState::Recording => Action::Stop,
        SessionState::Paused => Action::Paused,
        SessionState::Stopped { .. } => Action::Resume,
        SessionState::Asking { .. } => Action::Asking,
        SessionState::Idle if !importing => Action::Record,
        _ => Action::None,
    }
}
/// What the menu was last built from; elapsed-time ticks change none of it.
struct Built {
    action: Action,
    ask_on_call: bool,
}
#[derive(Default)]
struct Inner {
    built: Option<Built>,
    header: Option<MenuItem<tauri::Wry>>,
    label: String,
    stale: bool,
}
#[derive(Default)]
pub struct State(Mutex<Inner>);

/// Apps set to Off as (app_id, display name), read from settings.json at rebuild time.
fn off_apps(app: &tauri::AppHandle) -> Vec<(String, String)> {
    let Ok(dir) = app.path().app_config_dir() else {
        return vec![];
    };
    let settings = crate::calls::Settings::load(&dir);
    let rules = motetto_core::watch::rules();
    settings
        .app_modes
        .iter()
        .filter(|(_, mode)| **mode == motetto_core::events::Mode::Off)
        .map(|(id, _)| {
            let name = rules
                .iter()
                .find(|r| &r.app_id == id)
                .map(|r| r.name.clone());
            (id.clone(), name.unwrap_or_else(|| id.clone()))
        })
        .collect()
}

/// The next update rebuilds the menu even if the state is unchanged (an app's mode was saved).
pub fn invalidate(app: &tauri::AppHandle) {
    if let Ok(mut state) = app.state::<State>().0.lock() {
        state.stale = true;
    }
}

fn header_text(state: &AppState) -> String {
    let seconds = state.elapsed_ms / 1000;
    match &state.state {
        SessionState::Recording => format!(
            "Recording · {} · {:02}:{:02}",
            state.app.as_deref().unwrap_or("Manual"),
            seconds / 60,
            seconds % 60
        ),
        SessionState::Paused => format!(
            "Paused · {} · {:02}:{:02} · not recording",
            state.app.as_deref().unwrap_or("Manual"),
            seconds / 60,
            seconds % 60
        ),
        SessionState::Asking { app, .. } => format!("{app} call · not recording"),
        SessionState::Stopped { .. } => "Stopped · notes start soon".into(),
        SessionState::Finishing { .. } => "Writing notes".into(),
        _ => "Motetto · ready for calls".into(),
    }
}
fn menu(
    app: &tauri::AppHandle,
    action: Action,
    ask_on_call: bool,
    off: &[(String, String)],
    header_label: &str,
) -> tauri::Result<(Menu<tauri::Wry>, MenuItem<tauri::Wry>)> {
    let header = MenuItemBuilder::with_id("status", header_label)
        .enabled(false)
        .build(app)?;
    let mut builder = MenuBuilder::new(app).item(&header);
    let settings = app
        .state::<crate::settings::Store>()
        .get()
        .unwrap_or_default();
    if !settings.onboarding_done {
        builder = builder
            .item(&MenuItemBuilder::with_id("setup", "Finish setting up Motetto").build(app)?);
    }
    for (id, label, shortcut) in match action {
        Action::Record => vec![("record", "Record now", "Ctrl+Alt+S")],
        Action::Asking => vec![
            ("ask-record", "Record this call", ""),
            ("ask-no", "Not for this call", ""),
        ],
        Action::Stop => vec![
            ("stop", "Stop recording", "Ctrl+Alt+S"),
            ("pause", "Pause recording", ""),
            ("mark", "Mark this moment", "Ctrl+Alt+M"),
            ("jot", "Jot a note…", "Ctrl+Alt+J"),
            ("notes", "Open My notes", "Ctrl+Alt+N"),
        ],
        Action::Paused => vec![
            ("resume", "Resume recording", ""),
            ("stop", "Stop recording", "Ctrl+Alt+S"),
            ("notes", "Open My notes", "Ctrl+Alt+N"),
        ],
        Action::Resume => vec![("resume", "Resume recording", "Ctrl+Alt+S")],
        Action::None => vec![],
    } {
        let key = match id {
            "record" | "stop" => Some("S"),
            "resume" if !shortcut.is_empty() => Some("S"),
            "mark" => Some("M"),
            "jot" => Some("J"),
            "notes" => Some("N"),
            _ => None,
        };
        let shortcut = key
            .and_then(|key| settings.shortcuts.get(key))
            .map_or(shortcut, |s| s.as_str());
        let item = MenuItemBuilder::with_id(id, label);
        let item = if shortcut.is_empty() {
            item
        } else {
            item.accelerator(shortcut)
        };
        builder = builder.item(&item.build(app)?);
    }
    if matches!(action, Action::Record | Action::Stop) {
        builder = builder.item(
            &CheckMenuItemBuilder::with_id("ask-on-call", "Ask when a call starts")
                .checked(ask_on_call)
                .build(app)?,
        );
    }
    if action == Action::Record {
        for (id, name) in off {
            builder = builder.item(
                &MenuItemBuilder::with_id(
                    format!("ask-again:{id}"),
                    format!("Ask again about {name}"),
                )
                .build(app)?,
            );
        }
    }
    let menu = builder
        .separator()
        .item(
            &MenuItemBuilder::with_id("open", "Open Motetto")
                .accelerator("CmdOrCtrl+O")
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id("settings", "Settings…")
                .accelerator("CmdOrCtrl+,")
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id("quit", "Quit Motetto")
                .accelerator("CmdOrCtrl+Q")
                .build(app)?,
        )
        .build()?;
    Ok((menu, header))
}

pub fn initial(app: &tauri::AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    // The saved switch, so a launch with asking off doesn't show the tick until the first update.
    let ask_on_call = crate::calls::Settings::load(&app.path().app_config_dir()?).ask_on_call;
    let (menu, header) = menu(
        app,
        Action::Record,
        ask_on_call,
        &off_apps(app),
        "Motetto · ready for calls",
    )?;
    if let Ok(mut state) = app.state::<State>().0.lock() {
        state.header = Some(header);
        state.stale = true;
    }
    Ok(menu)
}
pub fn update(app: &tauri::AppHandle, state: &AppState, importing: bool) -> Result<(), String> {
    let managed = app.state::<State>();
    let mut previous = managed
        .0
        .lock()
        .map_err(|_| "Tray menu state is unavailable")?;
    let next = action(&state.state, importing);
    let changed = previous.stale
        || previous
            .built
            .as_ref()
            .is_none_or(|b| b.action != next || b.ask_on_call != state.ask_on_call);
    let label = header_text(state);
    if changed {
        let tray = app
            .tray_by_id("motetto")
            .ok_or("Tray menu is unavailable")?;
        let off = off_apps(app);
        let (menu, header) =
            menu(app, next, state.ask_on_call, &off, &label).map_err(|e| e.to_string())?;
        previous.header = Some(header);
        tray.set_menu(Some(menu)).map_err(|e| e.to_string())?;
        tray.set_icon(Some(icon(next == Action::Stop)))
            .map_err(|e| e.to_string())?;
        tray.set_icon_as_template(true).map_err(|e| e.to_string())?;
        previous.built = Some(Built {
            action: next,
            ask_on_call: state.ask_on_call,
        });
        previous.label = label.clone();
        previous.stale = false;
    }
    if previous.label != label {
        if let Some(header) = &previous.header {
            header.set_text(&label).map_err(|e| e.to_string())?;
        }
        previous.label = label;
    }
    Ok(())
}
pub fn icon(recording: bool) -> tauri::image::Image<'static> {
    let _ = recording;
    tauri::image::Image::new_owned(include_bytes!("../icons/tray.rgba").to_vec(), 32, 32)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_the_action_for_the_current_state_is_offered() {
        assert_eq!(action(&SessionState::Idle, false), Action::Record);
        assert_eq!(action(&SessionState::Recording, false), Action::Stop);
        assert_eq!(action(&SessionState::Paused, false), Action::Paused);
        assert_eq!(
            action(
                &SessionState::Stopped {
                    resume_until_ms: 10_000
                },
                false
            ),
            Action::Resume
        );
        assert_eq!(
            action(
                &SessionState::Finishing {
                    meeting_id: "1".into()
                },
                false
            ),
            Action::None
        );
        assert_eq!(action(&SessionState::Idle, true), Action::None);
        assert_eq!(
            action(
                &SessionState::Asking {
                    call_id: "1".into(),
                    app: "Call".into()
                },
                false
            ),
            Action::Asking
        );
    }
}
