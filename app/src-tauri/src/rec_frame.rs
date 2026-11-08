//! The red frame around the recorded window or area (mockup `.recframe`): a click-through,
//! always-on-top window that no capture ever sees, following the target while a picture part
//! records. A whole display gets no frame; pausing or stopping ends the part and hides it.
use motetto_core::video::ScreenTarget;
use std::sync::Mutex;

/// The target of the picture part recording now; None between parts.
static TARGET: Mutex<Option<ScreenTarget>> = Mutex::new(None);
/// The window's margin around the target: the 2 pt border and its 1 pt light edge outside it.
const EDGE: f64 = 3.0;
/// Room under the frame for its "● REC" tab.
const TAB: f64 = 24.0;
#[cfg(target_os = "macos")]
const LABEL: &str = "recframe";

/// A picture part starts (`Some`) or ends (`None`).
pub fn set(target: Option<ScreenTarget>) {
    *TARGET.lock().unwrap_or_else(|e| e.into_inner()) = target.filter(framed);
}

/// A window or an area is outlined; a whole display is not (mockup `renderRecFrame`).
fn framed(target: &ScreenTarget) -> bool {
    !matches!(target, ScreenTarget::Display(_))
}

/// The frame window as (x, y, width, height) in global points around the target's rect.
fn around(r: crate::clicks::TargetRect) -> (f64, f64, f64, f64) {
    (
        r.x - EDGE,
        r.y - EDGE,
        r.width + 2.0 * EDGE,
        r.height + 2.0 * EDGE + TAB,
    )
}

/// Polls the target four times a second, so the frame follows a window that moves; the window
/// frame lookup is a quick Window Server query, not a shareable-content listing.
#[cfg(target_os = "macos")]
pub fn follow(app: tauri::AppHandle) -> Result<(), String> {
    std::thread::Builder::new()
        .name("motetto-rec-frame".into())
        .spawn(move || {
            let mut shown = None;
            loop {
                std::thread::sleep(std::time::Duration::from_millis(250));
                let target = *TARGET.lock().unwrap_or_else(|e| e.into_inner());
                let rect = target.as_ref().and_then(crate::clicks::target).map(around);
                if rect != shown && place(&app, rect).is_ok() {
                    shown = rect;
                }
            }
        })
        .map(|_| ())
        .map_err(|e| e.to_string())
}
#[cfg(not(target_os = "macos"))]
pub fn follow(_app: tauri::AppHandle) -> Result<(), String> {
    let _ = (set, around);
    Ok(())
}

#[cfg(target_os = "macos")]
fn place(app: &tauri::AppHandle, rect: Option<(f64, f64, f64, f64)>) -> Result<(), String> {
    use tauri::{LogicalPosition, LogicalSize, Manager};
    let Some((x, y, width, height)) = rect else {
        if let Some(window) = app.get_webview_window(LABEL) {
            window.hide().map_err(|e| e.to_string())?;
        }
        return Ok(());
    };
    let window = match app.get_webview_window(LABEL) {
        Some(window) => window,
        None => build(app)?,
    };
    window
        .set_position(LogicalPosition::new(x, y))
        .map_err(|e| e.to_string())?;
    window
        .set_size(LogicalSize::new(width, height))
        .map_err(|e| e.to_string())?;
    window.show().map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
fn build(app: &tauri::AppHandle) -> Result<tauri::WebviewWindow, String> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};
    let window = crate::window_native::protect(
        app,
        WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("recframe/".into())),
    )
    // Never in a capture by any app, even with Shown on: it is drawn around the picture.
    .content_protected(true)
    .title("Motetto recording frame")
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .resizable(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .visible_on_all_workspaces(true)
    .focusable(false)
    .focused(false)
    .visible(false)
    .build()
    .map_err(|e| e.to_string())?;
    crate::window_native::overlay(&window).map_err(|e| e.to_string())?;
    window
        .set_ignore_cursor_events(true)
        .map_err(|e| e.to_string())?;
    Ok(window)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clicks::TargetRect;

    #[test]
    fn windows_and_areas_are_framed_displays_are_not() {
        set(Some(ScreenTarget::Display(1)));
        assert_eq!(*TARGET.lock().unwrap(), None);
        set(Some(ScreenTarget::Window(7)));
        assert_eq!(*TARGET.lock().unwrap(), Some(ScreenTarget::Window(7)));
        set(None);
        assert_eq!(*TARGET.lock().unwrap(), None);
        let rect = TargetRect {
            x: 100.0,
            y: 50.0,
            width: 640.0,
            height: 400.0,
        };
        // The border sits outside the target, with the tab below it.
        assert_eq!(around(rect), (97.0, 47.0, 646.0, 430.0));
    }
}
