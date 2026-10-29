//! The script prompter (SPEC 14.3): one click-through window under the camera. The mouse goes
//! through it except over the controls the page reports; it takes the keyboard only to edit.
use serde::{Deserialize, Serialize};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use tauri::{Emitter, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub enum Layout {
    Strip,
    Column,
    Tucked,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}
impl Rect {
    fn contains(&self, (x, y): (f64, f64)) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Outside,
    Over,
    Control,
}

/// Where the pointer is: `window` and `cursor` in physical screen pixels, `controls` as the page
/// measured them (logical pixels from the window's top left).
pub fn hit(controls: &[[f64; 4]], window: Rect, cursor: (f64, f64), scale: f64) -> Hit {
    if !window.contains(cursor) {
        return Hit::Outside;
    }
    let over_control = controls.iter().any(|[x, y, w, h]| {
        Rect {
            x: window.x + x * scale,
            y: window.y + y * scale,
            w: w * scale,
            h: h * scale,
        }
        .contains(cursor)
    });
    if over_control {
        Hit::Control
    } else {
        Hit::Over
    }
}

/// The window's frame in a display's work area (logical pixels) for a layout.
pub fn place(area: Rect, layout: Layout, editing: bool) -> Rect {
    match layout {
        Layout::Strip => {
            let w = 560.0;
            Rect {
                x: area.x + (area.w - w) / 2.0,
                // Under the camera: 8 px below the menu bar or the top of the screen.
                y: area.y + 8.0,
                w,
                // Three lines of the 22 px face, two of them wrapping, as in the mockup.
                h: if editing { 380.0 } else { 204.0 },
            }
        }
        Layout::Column => Rect {
            x: area.x + area.w - 300.0 - 16.0,
            y: area.y + 16.0,
            w: 300.0,
            h: (area.h - 32.0).min(if editing { 520.0 } else { 260.0 }),
        },
        Layout::Tucked => Rect {
            x: area.x + area.w - 28.0 - 10.0,
            y: area.y + area.h * 0.4,
            w: 28.0,
            h: 64.0,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flags {
    pub focusable: bool,
    pub ignore_cursor: bool,
}
/// Editing takes the keyboard and the mouse; otherwise only a control takes the mouse.
pub fn flags(editing: bool, hit: Hit) -> Flags {
    Flags {
        focusable: editing,
        ignore_cursor: !editing && hit != Hit::Control,
    }
}

#[derive(Default)]
pub struct State {
    controls: Mutex<Vec<[f64; 4]>>,
    layout: Mutex<Option<Layout>>,
    editing: AtomicBool,
    /// Each show starts one watcher; a newer show or a hide ends the older one.
    watcher: AtomicU64,
    /// The app that had the keyboard before editing, given it back on Esc.
    previous_app: Mutex<Option<i32>>,
    checks: Mutex<motetto_core::share_check::Cache>,
    /// The page drew the marker: the capture may start.
    marker: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    /// (sharing, shown) last seen, so a share starting or Hidden coming back checks again.
    last: Mutex<(bool, bool)>,
    /// Each check gets a number; an answer arriving after a newer check started is dropped.
    check_number: AtomicU64,
    /// The page's last check answer (`tell`); edits wait for a safe one.
    check: Mutex<&'static str>,
    /// The page has loaded and listens: events sent before that would be lost.
    ready: AtomicBool,
    /// The user moved the strip to another display: the check can't tell which one is shared.
    moved: AtomicBool,
    script: Mutex<Option<String>>,
}

/// The page asks on load: everything Rust told it before it listened (review: lost events).
#[derive(Serialize, TS)]
pub struct PageState {
    pub script: Option<String>,
    pub check: String,
    pub editing: bool,
    pub layout: Layout,
    pub rehearsing: bool,
}

/// Edits show the whole script, so only when the share can't see it (SPEC 14.2).
pub fn may_edit(check: &str, shown: bool) -> bool {
    shown || matches!(check, "hidden" | "off" | "moved")
}

const LABEL: &str = "prompter";

fn window(app: &tauri::AppHandle) -> Result<tauri::WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(LABEL) {
        return Ok(window);
    }
    let window = crate::window_native::protect(
        app,
        WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("prompter/".into())),
    )
    .title("Motetto script")
    .inner_size(560.0, 204.0)
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
    crate::window_native::configure(&window).map_err(|e| e.to_string())?;
    window
        .set_ignore_cursor_events(true)
        .map_err(|e| e.to_string())?;
    Ok(window)
}

fn layout(app: &tauri::AppHandle) -> Layout {
    let state = app.state::<State>();
    let saved = *state.layout.lock().unwrap_or_else(|e| e.into_inner());
    saved.unwrap_or_else(|| {
        app.state::<crate::settings::Store>()
            .get()
            .ok()
            .map_or(Layout::Strip, |s| s.prompter.layout)
    })
}

fn arrange(app: &tauri::AppHandle, window: &tauri::WebviewWindow) -> Result<(), String> {
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or(window.primary_monitor().ok().flatten())
        .ok_or("No display found")?;
    arrange_on(app, window, &monitor)
}

/// Places the window on `monitor` in logical points: the work area divided by that display's
/// own scale (a physical position would be converted with the window's current scale).
fn arrange_on(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
    monitor: &tauri::Monitor,
) -> Result<(), String> {
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let frame = place(
        Rect {
            x: area.position.x as f64 / scale,
            y: area.position.y as f64 / scale,
            w: area.size.width as f64 / scale,
            h: area.size.height as f64 / scale,
        },
        layout(app),
        app.state::<State>().editing.load(Ordering::SeqCst),
    );
    window
        .set_position(LogicalPosition::new(frame.x, frame.y))
        .and_then(|_| window.set_size(LogicalSize::new(frame.w, frame.h)))
        .map_err(|e| e.to_string())
}

/// The check-failed card's "Move to another display": the next display, where the share
/// isn't looking. False with one display.
#[tauri::command]
pub fn prompter_move_display(app: tauri::AppHandle) -> Result<bool, String> {
    let window = window(&app)?;
    let monitors = window.available_monitors().map_err(|e| e.to_string())?;
    let current = window.current_monitor().map_err(|e| e.to_string())?;
    let at = current
        .as_ref()
        .and_then(|c| monitors.iter().position(|m| m.position() == c.position()))
        .unwrap_or(0);
    if monitors.len() < 2 {
        return Ok(false);
    }
    arrange_on(&app, &window, &monitors[(at + 1) % monitors.len()])?;
    // The check can't tell which display is shared: moving is the user's answer until hidden.
    let state = app.state::<State>();
    state.moved.store(true, Ordering::SeqCst);
    state.check_number.fetch_add(1, Ordering::SeqCst);
    tell(&app, "moved");
    Ok(true)
}

/// While shown and not editing, ~30 times a second: let the mouse through except over a control,
/// and tell the page when the pointer is over the strip so it fades.
fn watch(app: tauri::AppHandle) {
    let id = app.state::<State>().watcher.fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::spawn(move || {
        let mut last: Option<Hit> = None;
        while app.state::<State>().watcher.load(Ordering::SeqCst) == id {
            std::thread::sleep(std::time::Duration::from_millis(33));
            let Some(window) = app.get_webview_window(LABEL) else {
                break;
            };
            let state = app.state::<State>();
            // No fading while the marker may be on screen: a faded marker could read as hidden.
            if state.editing.load(Ordering::SeqCst)
                || *state.check.lock().unwrap_or_else(|e| e.into_inner()) == "checking"
            {
                if last == Some(Hit::Over) {
                    let _ = window.emit("motetto://prompter-through", false);
                }
                last = None;
                continue;
            }
            let (Ok(position), Ok(size), Ok(cursor), Ok(scale)) = (
                window.outer_position(),
                window.outer_size(),
                window.cursor_position(),
                window.scale_factor(),
            ) else {
                continue;
            };
            let controls = state
                .controls
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let now = hit(
                &controls,
                Rect {
                    x: position.x as f64,
                    y: position.y as f64,
                    w: size.width as f64,
                    h: size.height as f64,
                },
                (cursor.x, cursor.y),
                scale,
            );
            if last == Some(now) {
                continue;
            }
            let _ = window.set_ignore_cursor_events(flags(false, now).ignore_cursor);
            let _ = window.emit("motetto://prompter-through", now == Hit::Over);
            last = Some(now);
        }
    });
}

/// The script the strip shows now, if any.
pub fn shown_script(app: &tauri::AppHandle) -> Option<String> {
    app.state::<State>()
        .script
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// A new script on the strip: the next words it sends replace the last script's.
fn forget_hint(app: &tauri::AppHandle) {
    app.state::<crate::follow::State>()
        .hint
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
}

#[tauri::command]
pub fn prompter_show(app: tauri::AppHandle, name: Option<String>) -> Result<(), String> {
    let window = window(&app)?;
    if let Some(name) = &name {
        app.state::<crate::settings::Store>()
            .change(|s| s.prompter.last_script = Some(name.clone()))?;
    }
    let state = app.state::<State>();
    if let Some(name) = &name {
        *state.script.lock().unwrap_or_else(|e| e.into_inner()) = Some(name.clone());
        forget_hint(&app);
    }
    arrange(&app, &window)?;
    window.show().map_err(|e| e.to_string())?;
    let _ = window.emit("motetto://prompter-script", name);
    watch(app.clone());
    crate::shortcuts::prompting(&app, true);
    check(app);
    Ok(())
}

/// Another script on the strip (its chip menu), from its start; it is the one ⌃⌥T opens from
/// now on. The window, its place and the share check stay as they are.
#[tauri::command]
pub fn prompter_script(app: tauri::AppHandle, name: String) -> Result<(), String> {
    if app
        .state::<crate::rehearse::State>()
        .0
        .lock()
        .is_ok_and(|r| r.is_some())
    {
        return Err("Stop rehearsing to switch scripts.".into());
    }
    let name = crate::scripts::checked_name(&name)?;
    app.state::<crate::settings::Store>()
        .change(|s| s.prompter.last_script = Some(name.clone()))?;
    *app.state::<State>()
        .script
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(name.clone());
    forget_hint(&app);
    let _ = app.emit("motetto://prompter-script", Some(name));
    Ok(())
}

/// "Open Scripts in Motetto" (the chip menu): the library comes forward on its Scripts page.
#[tauri::command]
pub fn open_scripts(app: tauri::AppHandle) {
    crate::desktop::show(&app);
    let _ = app.emit("motetto://open-scripts", ());
}

/// What the page shows: "hidden", "visible", "unknown", "checking", "shown", "off" or "moved".
fn tell(app: &tauri::AppHandle, check: &'static str) {
    *app.state::<State>()
        .check
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = check;
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.emit("motetto://prompter-check", check);
    }
}

#[tauri::command]
pub fn prompter_state(app: tauri::AppHandle) -> PageState {
    let state = app.state::<State>();
    state.ready.store(true, Ordering::SeqCst);
    PageState {
        script: state
            .script
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .or_else(|| {
                app.state::<crate::settings::Store>()
                    .get()
                    .ok()
                    .and_then(|s| s.prompter.last_script)
            }),
        // Nothing checked yet counts as checking: the text waits.
        check: match *state.check.lock().unwrap_or_else(|e| e.into_inner()) {
            "" => "checking".into(),
            check => check.into(),
        },
        editing: state.editing.load(Ordering::SeqCst),
        layout: layout(&app),
        rehearsing: {
            let rehearse = app.state::<crate::rehearse::State>();
            rehearse.1.load(Ordering::SeqCst) || rehearse.0.lock().is_ok_and(|r| r.is_some())
        },
    }
}

fn shown(app: &tauri::AppHandle) -> bool {
    app.state::<crate::controller::Controller>()
        .state()
        .is_ok_and(|s| s.share_visible)
}

/// The share self-check (SPEC 14.2), on its own thread: marker, one frame, search, drop.
/// A newer check (a toggle, a share starting) makes this one's answer stale: it is dropped.
fn check(app: tauri::AppHandle) {
    let state = app.state::<State>();
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    let number = state.check_number.fetch_add(1, Ordering::SeqCst) + 1;
    if shown(&app) {
        return tell(&app, "shown");
    }
    if state.moved.load(Ordering::SeqCst) {
        return tell(&app, "moved");
    }
    if !app
        .state::<crate::settings::Store>()
        .get()
        .is_ok_and(|s| s.share_check)
    {
        return tell(&app, "off");
    }
    tell(&app, "checking");
    std::thread::spawn(move || {
        use motetto_core::share_check::{Check, PxRect, capture, find};
        let state = app.state::<State>();
        let current = || state.check_number.load(Ordering::SeqCst) == number;
        // The page must be listening before it's asked to draw the marker.
        for _ in 0..60 {
            if state.ready.load(Ordering::SeqCst) || !current() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let os = os_version();
        let answer = (|| {
            let monitor = window
                .current_monitor()
                .map_err(|e| e.to_string())?
                .ok_or("No display found")?;
            let (scale, at, size) = (monitor.scale_factor(), *monitor.position(), *monitor.size());
            let display = monitor.name().cloned().unwrap_or_default();
            // The cache lock is never held while waiting for the page or the capture.
            let known = state
                .checks
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&os, &display);
            if let Some(known) = known {
                return Ok::<_, String>((known, display));
            }
            let (sender, drawn) = std::sync::mpsc::channel();
            *state.marker.lock().unwrap_or_else(|e| e.into_inner()) = Some(sender);
            let _ = window.emit("motetto://prompter-marker", true);
            let found = drawn
                .recv_timeout(std::time::Duration::from_secs(1))
                .map_err(|_| "The marker wasn't drawn".to_string())
                .and_then(|_| {
                    // Two frames for the compositor to put the marker on screen.
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    let (position, inner) = (
                        window.outer_position().map_err(|e| e.to_string())?,
                        window.outer_size().map_err(|e| e.to_string())?,
                    );
                    let rgba = capture(
                        (at.x as f64 / scale, at.y as f64 / scale),
                        size.width,
                        size.height,
                    )?;
                    Ok(find(
                        &rgba,
                        size.width as usize,
                        PxRect {
                            x: (position.x - at.x).max(0) as usize,
                            y: (position.y - at.y).max(0) as usize,
                            w: inner.width as usize,
                            h: inner.height as usize,
                        },
                        scale.round() as usize,
                    ))
                });
            let _ = window.emit("motetto://prompter-marker", false);
            Ok((Check::from(found), display))
        })();
        // Shown during the capture means protection was off: the frame says nothing.
        if !current() || shown(&app) {
            return;
        }
        let answer = match answer {
            Ok((answer, display)) => {
                state
                    .checks
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .put(&os, &display, answer);
                answer
            }
            Err(_) => Check::Unknown,
        };
        tell(
            &app,
            match answer {
                Check::Hidden => "hidden",
                Check::Visible => "visible",
                Check::Unknown => "unknown",
            },
        );
    });
}

fn os_version() -> String {
    std::process::Command::new("/usr/bin/sw_vers")
        .arg("-productVersion")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

#[tauri::command]
pub fn prompter_marker_drawn(state: tauri::State<'_, State>) {
    if let Some(sender) = state
        .marker
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
    {
        let _ = sender.send(());
    }
}

/// Every published state: a share starting, or Hidden coming back, checks again while shown.
pub fn on_state(app: &tauri::AppHandle, state: &motetto_core::events::AppState) {
    let now = (state.sharing.is_some(), state.share_visible);
    let before = std::mem::replace(
        &mut *app
            .state::<State>()
            .last
            .lock()
            .unwrap_or_else(|e| e.into_inner()),
        now,
    );
    let visible = app
        .get_webview_window(LABEL)
        .is_some_and(|w| w.is_visible().unwrap_or(false));
    if visible && (now.0 && !before.0 || now.1 != before.1) {
        check(app.clone());
    }
}
#[tauri::command]
pub fn prompter_hide(app: tauri::AppHandle) -> Result<(), String> {
    // The strip announces the rehearsal microphone; stop it before hiding its controls.
    let rehearsal = app.state::<crate::rehearse::State>();
    if rehearsal.1.load(Ordering::SeqCst)
        || rehearsal
            .0
            .lock()
            .map_err(|_| "Rehearsal is unavailable")?
            .is_some()
    {
        return Err("Stop rehearsing before closing the script.".into());
    }
    prompter_edit(app.clone(), false, true)?;
    let state = app.state::<State>();
    state.watcher.fetch_add(1, Ordering::SeqCst);
    state.moved.store(false, Ordering::SeqCst);
    crate::shortcuts::prompting(&app, false);
    if let Some(window) = app.get_webview_window(LABEL) {
        window.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}
#[tauri::command]
pub fn prompter_layout(app: tauri::AppHandle, layout: Layout) -> Result<(), String> {
    let rehearsing = app
        .state::<crate::rehearse::State>()
        .0
        .lock()
        .is_ok_and(|r| r.is_some());
    if rehearsing && layout == Layout::Tucked {
        return Err(
            "The script stays open while you rehearse: it shows that the microphone is on.".into(),
        );
    }
    *app.state::<State>()
        .layout
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(layout);
    app.state::<crate::settings::Store>()
        .change(|s| s.prompter.layout = layout)?;
    if let Some(window) = app.get_webview_window(LABEL) {
        arrange(&app, &window)?;
        let _ = window.emit("motetto://prompter-layout", layout);
    }
    Ok(())
}
/// `restore`: give the keyboard back to the app that had it (Esc); false after a click outside,
/// where the clicked app already has it.
#[tauri::command]
pub fn prompter_edit(app: tauri::AppHandle, on: bool, restore: bool) -> Result<(), String> {
    let state = app.state::<State>();
    if on
        && !may_edit(
            *state.check.lock().unwrap_or_else(|e| e.into_inner()),
            shown(&app),
        )
    {
        return Err("The script can't open while your share may see it.".into());
    }
    if state.editing.swap(on, Ordering::SeqCst) == on {
        return Ok(());
    }
    let Some(window) = app.get_webview_window(LABEL) else {
        return Ok(());
    };
    let flags = flags(on, Hit::Outside);
    if on {
        *state.previous_app.lock().unwrap_or_else(|e| e.into_inner()) =
            crate::window_native::frontmost_app();
    }
    window
        .set_focusable(flags.focusable)
        .and_then(|_| window.set_ignore_cursor_events(flags.ignore_cursor))
        .map_err(|e| e.to_string())?;
    arrange(&app, &window)?;
    let _ = window.emit("motetto://prompter-editing", on);
    if on {
        window.set_focus().map_err(|e| e.to_string())?;
    } else if let Some(pid) = state
        .previous_app
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        && restore
    {
        crate::window_native::activate_app(pid);
    }
    Ok(())
}
#[tauri::command]
pub fn prompter_controls(state: tauri::State<'_, State>, rects: Vec<[f64; 4]>) {
    // At most 16 controls; `hit` only counts them inside the window.
    *state.controls.lock().unwrap_or_else(|e| e.into_inner()) =
        rects.into_iter().take(16).collect();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_controls_and_strip() {
        let window = Rect {
            x: 952.0,
            y: 66.0,
            w: 1120.0,
            h: 264.0,
        }; // 560 × 132 logical at scale 2
        let handle = [[14.0, 9.0, 18.0, 18.0]];
        assert_eq!(
            hit(&handle, window, (952.0 + 28.0, 66.0 + 18.0), 2.0),
            Hit::Control
        );
        // Edges count as inside.
        assert_eq!(
            hit(&handle, window, (952.0 + 64.0, 66.0 + 54.0), 2.0),
            Hit::Control
        );
        assert_eq!(
            hit(&handle, window, (952.0 + 66.0, 66.0 + 18.0), 2.0),
            Hit::Over
        );
        assert_eq!(hit(&handle, window, (952.0, 66.0), 2.0), Hit::Over);
        assert_eq!(hit(&handle, window, (951.0, 66.0), 2.0), Hit::Outside);
        assert_eq!(hit(&[], window, (2072.0, 330.0), 2.0), Hit::Over);
        assert_eq!(hit(&[], window, (2073.0, 330.0), 2.0), Hit::Outside);
    }

    #[test]
    fn place_strip_top_centre() {
        let area = Rect {
            x: 0.0,
            y: 25.0,
            w: 1512.0,
            h: 957.0,
        };
        let strip = place(area, Layout::Strip, false);
        assert_eq!(
            (strip.x, strip.y, strip.w, strip.h),
            (476.0, 33.0, 560.0, 204.0)
        );
        assert_eq!(place(area, Layout::Strip, true).h, 380.0);
        // A display left of the main one keeps negative coordinates.
        let left = place(Rect { x: -1920.0, ..area }, Layout::Strip, false);
        assert_eq!(left.x, -1920.0 + 476.0);
    }

    #[test]
    fn place_column_right_edge() {
        let area = Rect {
            x: 0.0,
            y: 25.0,
            w: 1512.0,
            h: 957.0,
        };
        let column = place(area, Layout::Column, false);
        assert_eq!((column.x, column.y, column.w), (1196.0, 41.0, 300.0));
        let short = place(Rect { h: 400.0, ..area }, Layout::Column, true);
        assert_eq!(short.h, 368.0);
        let tucked = place(area, Layout::Tucked, false);
        assert_eq!((tucked.x, tucked.w, tucked.h), (1474.0, 28.0, 64.0));
    }

    #[test]
    fn the_strip_may_listen_and_be_dragged() {
        // Without this, Tauri drops every event to the strip (check, rehearsal, what was heard)
        // while its commands still work: it looks alive and misses the rehearsal announcement.
        let capabilities = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("capabilities/main.json"),
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&capabilities).unwrap();
        let windows: Vec<&str> = value["windows"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|w| w.as_str())
            .collect();
        assert!(windows.contains(&LABEL), "{windows:?}");
        let permissions = value["permissions"].to_string();
        for needed in [
            "core:event:allow-listen",
            "core:window:allow-start-dragging",
        ] {
            assert!(permissions.contains(needed), "{needed}");
        }
    }

    #[test]
    fn exports_bindings() {
        let config = ts_rs::Config::default().with_out_dir("../../ui/lib/bindings");
        PageState::export_all(&config).unwrap();
        Layout::export_all(&config).unwrap();
    }

    #[test]
    fn edits_wait_for_a_safe_check() {
        for safe in ["hidden", "off", "moved"] {
            assert!(may_edit(safe, false), "{safe}");
        }
        for unsafe_check in ["", "checking", "visible", "unknown"] {
            assert!(!may_edit(unsafe_check, false), "{unsafe_check}");
            // Shown: the user chose to let the call see Motetto.
            assert!(may_edit(unsafe_check, true));
        }
    }

    #[test]
    fn edit_exit_restores_click_through() {
        assert_eq!(
            flags(true, Hit::Over),
            Flags {
                focusable: true,
                ignore_cursor: false
            }
        );
        for hit in [Hit::Outside, Hit::Over] {
            assert_eq!(
                flags(false, hit),
                Flags {
                    focusable: false,
                    ignore_cursor: true
                }
            );
        }
        assert!(!flags(false, Hit::Control).ignore_cursor);
        assert!(!flags(false, Hit::Control).focusable);
    }
}
