//! The screen setup: a transparent window over every display hosts the setup bar, so the mode,
//! mic, options and start timer are chosen before the first frame. The page runs the countdown
//! and calls `start_capture` (or `pick_capture` during a recording) once it ends.
use crate::controller::{Action, Controller};
use crate::prompter::{Hit, Rect, hit};
use motetto_core::{
    events::{MicChoice, RecordSetup, ScreenPick},
    video::{self, AreaRect},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
use tauri::{Emitter, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder};
use ts_rs::TS;

/// Record now (a call, with notes) or Record the screen (a clip); each remembers its own setup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub enum CaptureKind {
    Call,
    Clip,
}

pub const MODES: [&str; 4] = ["audio", "display", "window", "area"];
pub const TIMERS: [u8; 4] = [0, 3, 5, 10];
/// Picture presets, as in Settings › Screen video.
pub const PICTURES: [&str; 3] = ["smaller", "sharp", "high"];

/// The setup bar's last choices for one kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct CaptureMemory {
    /// One of `MODES`.
    pub mode: String,
    pub mic: MicChoice,
    pub app_sounds: bool,
    pub notes: bool,
    /// Seconds before the first frame, one of `TIMERS`.
    pub timer: u8,
    pub clicks: bool,
    pub area: Option<AreaRect>,
    pub remember: bool,
    /// One of `PICTURES` for a screen recording; None follows Settings › Screen video.
    #[ts(optional)]
    pub picture: Option<String>,
}
impl CaptureMemory {
    /// Record now: audio only, the Settings mic, notes, no timer.
    pub fn call() -> Self {
        Self {
            mode: "audio".into(),
            mic: MicChoice::Default,
            app_sounds: true,
            notes: true,
            timer: 0,
            clicks: true,
            area: None,
            remember: true,
            picture: None,
        }
    }
    /// Record the screen: a window, no mic, just the video, 3 s to bring the window forward.
    pub fn clip() -> Self {
        Self {
            mode: "window".into(),
            mic: MicChoice::Off,
            notes: false,
            timer: 3,
            ..Self::call()
        }
    }
    pub fn valid(&self) -> bool {
        MODES.contains(&self.mode.as_str())
            && TIMERS.contains(&self.timer)
            && match &self.mic {
                MicChoice::Device { id } => !id.is_empty() && id.len() <= 255 && !id.contains('\0'),
                _ => true,
            }
            && self.area.is_none_or(|a| {
                [a.x, a.y, a.width, a.height].iter().all(|v| v.is_finite())
                    && a.width > 0.0
                    && a.height > 0.0
            })
            && self
                .picture
                .as_deref()
                .is_none_or(|p| PICTURES.contains(&p))
    }
}
impl Default for CaptureMemory {
    fn default() -> Self {
        Self::call()
    }
}

/// A window the setup can point at, in points relative to the display holding its centre.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct CaptureWindow {
    pub id: u32,
    pub app: String,
    pub title: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub display: u32,
    /// Motetto's own window: the setup shows it can't be chosen.
    pub own: bool,
}

/// A display as (id, x, y, width, height) in global points, top-left origin.
type Display = (u32, f64, f64, f64, f64);

/// The overlays' own title, so they are never listed as a window to point at.
const TITLE: &str = "Motetto screen setup";

/// Pickable windows (front first) on the display that holds each one's centre; a window
/// spanning two displays is offered on one of them. Motetto's own windows (not the overlays)
/// are listed as `own`, so pointing at one says why it can't be chosen.
fn on_displays(windows: &[video::WindowInfo], displays: &[Display]) -> Vec<CaptureWindow> {
    let all: Vec<_> = windows
        .iter()
        .filter(|w| w.title != TITLE)
        .map(|w| video::WindowInfo {
            own: false,
            ..w.clone()
        })
        .collect();
    video::pickable(&all)
        .into_iter()
        .filter_map(|w| {
            let (cx, cy) = (w.x + w.width / 2.0, w.y + w.height / 2.0);
            let &(display, dx, dy, ..) = displays.iter().find(|(_, x, y, width, height)| {
                cx >= *x && cx < x + width && cy >= *y && cy < y + height
            })?;
            Some(CaptureWindow {
                id: w.id,
                app: w.app,
                title: w.title,
                x: w.x - dx,
                y: w.y - dy,
                width: w.width,
                height: w.height,
                display,
                own: windows.iter().any(|o| o.id == w.id && o.own),
            })
        })
        .collect()
}

/// The mic the recording uses: the Settings mic for Default (or the system default when none is
/// set or it isn't connected), the chosen device, or none.
fn resolve_mic(mic: MicChoice, settings_mic: impl FnOnce() -> Option<String>) -> MicChoice {
    match mic {
        MicChoice::Default => {
            settings_mic().map_or(MicChoice::Default, |id| MicChoice::Device { id })
        }
        other => other,
    }
}

/// Saves the memory when Remember is on; with it off, only that choice is kept.
fn remember(saved: &mut CaptureMemory, memory: CaptureMemory) {
    if memory.remember {
        *saved = memory;
    } else {
        saved.remember = false;
    }
}

fn displays(app: &tauri::AppHandle) -> Vec<Display> {
    #[cfg(target_os = "macos")]
    {
        let _ = app;
        crate::window_native::displays()
    }
    // Windows: the screen setup is macOS-only in this cut; Record now still gets its bar here.
    #[cfg(not(target_os = "macos"))]
    {
        app.available_monitors()
            .unwrap_or_default()
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let (p, s, f) = (m.position(), m.size(), m.scale_factor());
                let (x, y) = (f64::from(p.x) / f, f64::from(p.y) / f);
                (
                    i as u32,
                    x,
                    y,
                    f64::from(s.width) / f,
                    f64::from(s.height) / f,
                )
            })
            .collect()
    }
}

/// Points the Dock (or the Windows taskbar) takes at the bottom of a display: the gap between the
/// display's bottom and its work area's. Zero when it hides, sits at a side or isn't found.
fn bottom_inset(display_bottom: f64, work_bottom: f64, height: f64) -> f64 {
    let inset = display_bottom - work_bottom;
    if inset.is_finite() && inset > 0.0 && inset < height / 3.0 {
        inset.round()
    } else {
        0.0
    }
}

/// A Dock that hides leaves the work area whole but still slides up over the bottom when it shows:
/// reserve its height (the icon size plus 20 pt of shelf, 62 pt for 42 pt icons). A Dock at a
/// side reserves nothing at the bottom.
fn hidden_dock(autohide: bool, orientation: &str, tile_size: f64) -> f64 {
    if autohide && orientation == "bottom" && tile_size.is_finite() {
        tile_size.clamp(16.0, 128.0) + 20.0
    } else {
        0.0
    }
}

/// The Dock's own settings (`defaults read com.apple.dock`), read once per setup.
#[cfg(target_os = "macos")]
fn dock_settings() -> f64 {
    let read = |key: &str| {
        std::process::Command::new("/usr/bin/defaults")
            .args(["read", "com.apple.dock", key])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let autohide = read("autohide").is_some_and(|v| v == "1");
    if !autohide {
        return 0.0;
    }
    // macOS leaves out a key that was never changed: the Dock sits at the bottom, 48 pt icons.
    let orientation = read("orientation").unwrap_or_else(|| "bottom".into());
    let tile_size = read("tilesize")
        .and_then(|v| v.parse().ok())
        .unwrap_or(48.0);
    hidden_dock(autohide, &orientation, tile_size)
}
#[cfg(not(target_os = "macos"))]
fn dock_settings() -> f64 {
    0.0
}

/// The work area of the monitor at (x, y) in points, so the bar sits above the Dock.
fn dock(app: &tauri::AppHandle, &(_, x, y, _, height): &Display, hidden: f64) -> f64 {
    let monitors = app.available_monitors().unwrap_or_default();
    let shown = monitors
        .iter()
        .find_map(|m| {
            let f = m.scale_factor();
            let p = m.position();
            let near = |a: i32, b: f64| (f64::from(a) / f - b).abs() < 2.0;
            (near(p.x, x) && near(p.y, y)).then(|| {
                let work = m.work_area();
                let bottom = f64::from(work.position.y + work.size.height as i32) / f;
                bottom_inset(y + height, bottom, height)
            })
        })
        .unwrap_or(0.0);
    shown.max(hidden)
}

/// Opening and closing one at a time, so a second open never races the first for a label.
static OPENING: std::sync::Mutex<()> = std::sync::Mutex::new(());
/// Esc reaches the overlays from Rust, whichever display or app has the keyboard: the page
/// closes a menu, goes back from the countdown or closes, and answers `capture_esc_ack`.
const ESCAPE: &str = "Escape";
/// The last Esc sent to the pages, and the last one a page answered.
static ESC_SENT: AtomicU64 = AtomicU64::new(0);
static ESC_ANSWERED: AtomicU64 = AtomicU64::new(0);
/// How long a page has to answer before Rust closes every overlay itself.
const ESC_WAIT: std::time::Duration = std::time::Duration::from_millis(500);

/// No page answered this Esc: the setup must not trap the screen, so it closes.
fn unanswered(sent: u64, answered: u64) -> bool {
    answered < sent
}

/// Each overlay's controls (logical rects in its window, from the page) and whether the mouse
/// passes through elsewhere (Audio only). Picking modes take every click.
static CONTROLS: Mutex<BTreeMap<String, Controls>> = Mutex::new(BTreeMap::new());
type Controls = (bool, Vec<[f64; 4]>);
/// Each open starts one watcher; a newer open or a close ends the older one.
static WATCHER: AtomicU64 = AtomicU64::new(0);

/// The setup hid the library window to keep it out of the way: it comes back unless a recording
/// started (owner 2026-11-06).
static HID_MAIN: AtomicBool = AtomicBool::new(false);

fn hide_main(app: &tauri::AppHandle) {
    if let Some(main) = app.get_webview_window("main")
        && main.is_visible().unwrap_or(false)
        && !main.is_minimized().unwrap_or(false)
        && main.hide().is_ok()
    {
        HID_MAIN.store(true, Ordering::SeqCst);
    }
}

/// Brings the library back if the setup hid it; `false` once a recording started.
fn restore_main(app: &tauri::AppHandle, show: bool) {
    if HID_MAIN.swap(false, Ordering::SeqCst) && show {
        crate::desktop::show(app);
    }
}

/// Whether an overlay takes the mouse: always while picking; in Audio only, over its controls.
fn takes_mouse(through: bool, hit: Hit) -> bool {
    !through || hit == Hit::Control
}

/// ~30 times a second, as the prompter does: a window that ignores the cursor gets no events, so
/// the pointer is polled against each overlay's controls.
fn watch(app: &tauri::AppHandle) {
    let id = WATCHER.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    std::thread::spawn(move || {
        let mut last: BTreeMap<String, bool> = BTreeMap::new();
        while WATCHER.load(Ordering::SeqCst) == id {
            std::thread::sleep(std::time::Duration::from_millis(33));
            let controls = CONTROLS.lock().unwrap_or_else(|e| e.into_inner()).clone();
            for (label, window) in app.webview_windows() {
                if !label.starts_with("capture-") {
                    continue;
                }
                let (through, rects) = controls.get(&label).cloned().unwrap_or_default();
                let (Ok(position), Ok(size), Ok(cursor), Ok(scale)) = (
                    window.outer_position(),
                    window.outer_size(),
                    window.cursor_position(),
                    window.scale_factor(),
                ) else {
                    continue;
                };
                let frame = Rect {
                    x: position.x as f64,
                    y: position.y as f64,
                    w: size.width as f64,
                    h: size.height as f64,
                };
                let takes = takes_mouse(through, hit(&rects, frame, (cursor.x, cursor.y), scale));
                if last.get(&label) != Some(&takes) {
                    let _ = window.set_ignore_cursor_events(!takes);
                    last.insert(label, takes);
                }
            }
        }
    });
}

fn escape(app: &tauri::AppHandle) {
    let sent = ESC_SENT.fetch_add(1, Ordering::SeqCst) + 1;
    let _ = app.emit("motetto://capture-esc", sent);
    std::thread::sleep(ESC_WAIT);
    if unanswered(sent, ESC_ANSWERED.load(Ordering::SeqCst)) {
        close(app);
        restore_main(app, true);
    }
}

/// Opens one overlay per display, replacing any open ones. `mode` opens on that mode instead of
/// the remembered one. Display changes while it is open are not followed: reopening fixes them.
/// Any failure closes every overlay already built, so none is left covering a display.
pub fn open(app: &tauri::AppHandle, kind: CaptureKind, mode: Option<&str>) -> Result<(), String> {
    if mode.is_some_and(|m| !MODES.contains(&m)) {
        return Err("Choose audio only, the entire screen, a window or an area.".into());
    }
    let _one = OPENING.lock().unwrap_or_else(|e| e.into_inner());
    close_all(app);
    let result = build(app, kind, mode);
    if result.is_err() {
        close_all(app);
        restore_main(app, true);
    } else {
        hide_main(app);
    }
    result
}

fn build(app: &tauri::AppHandle, kind: CaptureKind, mode: Option<&str>) -> Result<(), String> {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
    // destroy() finishes on the main thread: wait (at most 1 s) until the old labels are free.
    for _ in 0..100 {
        if !app
            .webview_windows()
            .keys()
            .any(|l| l.starts_with("capture-"))
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let displays = displays(app);
    if displays.is_empty() {
        return Err("No display found for the screen setup.".into());
    }
    // The keyboard (Esc, Return) goes to the main display's bar.
    let main = displays
        .iter()
        .position(|d| d.1 == 0.0 && d.2 == 0.0)
        .unwrap_or(0);
    app.global_shortcut()
        .on_shortcut(ESCAPE, |app, _, event| {
            if event.state == ShortcutState::Pressed {
                let app = app.clone();
                tauri::async_runtime::spawn_blocking(move || escape(&app));
            }
        })
        .map_err(|e| e.to_string())?;
    crate::window_native::activate_self(app);
    let hidden = dock_settings();
    for (i, display) in displays.iter().enumerate() {
        let &(id, x, y, width, height) = display;
        // The main display's page holds the setup bar; the others point at windows and areas.
        let mut url = format!(
            "capture/?display={id}&kind={kind:?}&main={}&main_display={}&dock={}",
            u8::from(i == main),
            displays[main].0,
            dock(app, display, hidden)
        );
        if let Some(mode) = mode {
            url.push_str(&format!("&mode={mode}"));
        }
        let window = crate::window_native::protect(
            app,
            WebviewWindowBuilder::new(app, format!("capture-{id}"), WebviewUrl::App(url.into())),
        )
        // Never in a capture by any app, even with Shown on.
        .content_protected(true)
        .title(TITLE)
        .position(x, y)
        .inner_size(width, height)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible_on_all_workspaces(true)
        .build()
        .map_err(|e| e.to_string())?;
        crate::window_native::overlay(&window).map_err(|e| e.to_string())?;
        // macOS may move a window off the menu bar while building it: put it back.
        let _ = window.set_position(LogicalPosition::new(x, y));
        let _ = window.set_size(LogicalSize::new(width, height));
        if i == main {
            let _ = window.set_focus();
        }
    }
    crate::mic_preview::set_open(true);
    watch(app);
    Ok(())
}

pub fn close(app: &tauri::AppHandle) {
    let _one = OPENING.lock().unwrap_or_else(|e| e.into_inner());
    close_all(app);
}

fn close_all(app: &tauri::AppHandle) {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    WATCHER.fetch_add(1, Ordering::SeqCst);
    CONTROLS.lock().unwrap_or_else(|e| e.into_inner()).clear();
    // The mic is free again before a recording starts, and a late preview start is refused.
    crate::mic_preview::set_open(false);
    let shortcuts = app.global_shortcut();
    if shortcuts.is_registered(ESCAPE) {
        let _ = shortcuts.unregister(ESCAPE);
    }
    for (label, window) in app.webview_windows() {
        if label.starts_with("capture-") {
            let _ = window.destroy();
        }
    }
}

async fn blocking<T: Send + 'static>(
    job: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(job)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn open_capture(
    app: tauri::AppHandle,
    kind: CaptureKind,
    mode: Option<String>,
) -> Result<(), String> {
    blocking(move || open(&app, kind, mode.as_deref())).await
}

#[tauri::command]
pub async fn close_capture(app: tauri::AppHandle) -> Result<(), String> {
    blocking(move || {
        close(&app);
        restore_main(&app, true);
        Ok(())
    })
    .await
}

/// A page handled Esc number `n` (closed a menu, went back, or closed the setup).
#[tauri::command]
pub fn capture_esc_ack(n: u64) {
    ESC_ANSWERED.fetch_max(n, Ordering::SeqCst);
}

/// The page's controls (at most 16) and whether clicks elsewhere pass to the apps below.
#[tauri::command]
pub fn capture_controls(window: tauri::WebviewWindow, through: bool, rects: Vec<[f64; 4]>) {
    CONTROLS.lock().unwrap_or_else(|e| e.into_inner()).insert(
        window.label().into(),
        (through, rects.into_iter().take(16).collect()),
    );
}

#[tauri::command]
pub async fn capture_windows(app: tauri::AppHandle) -> Result<Vec<CaptureWindow>, String> {
    blocking(move || {
        #[cfg(target_os = "macos")]
        {
            // The setup's sizes and the recording's both need each display's scale.
            crate::screen::refresh_displays(&app);
            let windows = motetto_core::capture::screen_windows().map_err(|e| e.to_string())?;
            Ok(on_displays(&windows, &displays(&app)))
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (app, on_displays);
            Ok(vec![])
        }
    })
    .await
}

#[tauri::command]
pub fn capture_memory(
    store: tauri::State<'_, crate::settings::Store>,
    kind: CaptureKind,
) -> Result<CaptureMemory, String> {
    let settings = store.get()?;
    Ok(match kind {
        CaptureKind::Call => settings.capture_call,
        CaptureKind::Clip => settings.capture_clip,
    })
}

/// Called once the countdown ends (or at once for Now): the overlays close before the first
/// frame, the memory is saved when Remember is on, and the recording starts with the setup.
#[tauri::command]
pub async fn start_capture(
    app: tauri::AppHandle,
    kind: CaptureKind,
    setup: RecordSetup,
    memory: Option<CaptureMemory>,
) -> Result<(), String> {
    blocking(move || {
        close(&app);
        #[cfg(target_os = "macos")]
        crate::screen::refresh_displays(&app);
        let mut setup = setup;
        setup.mic = resolve_mic(setup.mic, || crate::microphone::for_capture(&app));
        if let Some(memory) = memory {
            let saved = app
                .state::<crate::settings::Store>()
                .change(|s| match kind {
                    CaptureKind::Call => remember(&mut s.capture_call, memory),
                    CaptureKind::Clip => remember(&mut s.capture_clip, memory),
                });
            // A memory that can't be saved never blocks the recording.
            if let Err(error) = saved {
                let _ = app.emit("motetto://notice", error);
            }
        }
        let result = app.state::<Controller>().action(Action::Start(Some(setup)));
        // Recording: the Pill shows it, the library stays out of the way.
        restore_main(&app, false);
        // The page that asked is closed: say why nothing started where it can be seen.
        if let Err(error) = &result {
            crate::desktop::show(&app);
            let _ = app.emit("motetto://notice", error.clone());
        }
        result
    })
    .await
}

/// During a recording the setup picks a new source instead of starting: the screen menu's
/// Choose a window… and Choose an area….
#[tauri::command]
pub async fn pick_capture(app: tauri::AppHandle, screen: ScreenPick) -> Result<(), String> {
    blocking(move || {
        close(&app);
        restore_main(&app, true);
        app.state::<Controller>()
            .action(Action::SetScreen(Some(screen)))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bar_clears_a_bottom_dock_only() {
        // A 1117-point display with a 70-point Dock at the bottom.
        assert_eq!(bottom_inset(1117.0, 1047.0, 1117.0), 70.0);
        // Hidden or at a side: the work area reaches the bottom.
        assert_eq!(bottom_inset(1117.0, 1117.0, 1117.0), 0.0);
        // A second display below the first, and nonsense, never lift the bar off the screen.
        assert_eq!(bottom_inset(2234.0, 1117.0, 1117.0), 0.0);
        assert_eq!(bottom_inset(1117.0, f64::NAN, 1117.0), 0.0);
    }

    #[test]
    fn a_hidden_dock_still_gets_its_room() {
        // 42 pt icons take 62 pt when the Dock shows, as measured with it always shown.
        assert_eq!(hidden_dock(true, "bottom", 42.0), 62.0);
        assert_eq!(hidden_dock(false, "bottom", 42.0), 0.0);
        assert_eq!(hidden_dock(true, "left", 42.0), 0.0);
        assert_eq!(hidden_dock(true, "bottom", f64::NAN), 0.0);
        assert_eq!(hidden_dock(true, "bottom", 900.0), 148.0);
    }

    fn window(id: u32, x: f64, own: bool) -> video::WindowInfo {
        video::WindowInfo {
            id,
            bundle_id: "com.example.notes".into(),
            title: format!("Note {id}"),
            x,
            y: 100.0,
            width: 800.0,
            height: 600.0,
            app: "Notes".into(),
            layer: 0,
            level: 0,
            on_screen: true,
            own,
        }
    }

    #[test]
    fn windows_are_relative_to_the_display_holding_their_centre() {
        let displays = [
            (1, 0.0, 0.0, 1440.0, 900.0),
            (2, 1440.0, 0.0, 1920.0, 1080.0),
        ];
        let windows = [
            window(10, 1600.0, false),
            window(11, 100.0, true),
            window(12, 100.0, false),
            window(13, 5000.0, false),
        ];
        let listed = on_displays(&windows, &displays);
        assert_eq!(
            listed
                .iter()
                .map(|w| (w.id, w.display, w.x, w.y))
                .collect::<Vec<_>>(),
            [
                (10, 2, 160.0, 100.0),
                (11, 1, 100.0, 100.0),
                (12, 1, 100.0, 100.0)
            ]
        );
        assert_eq!(listed.iter().filter(|w| w.own).count(), 1);
        assert!(listed[1].own);
        let mut overlay = window(14, 100.0, true);
        overlay.title = TITLE.into();
        assert!(on_displays(&[overlay], &displays).is_empty());
    }

    #[test]
    fn audio_only_lets_the_mouse_through_except_over_controls() {
        assert!(takes_mouse(false, Hit::Over));
        assert!(takes_mouse(false, Hit::Outside));
        assert!(takes_mouse(true, Hit::Control));
        assert!(!takes_mouse(true, Hit::Over));
        assert!(!takes_mouse(true, Hit::Outside));
    }

    #[test]
    fn esc_closes_when_no_page_answers() {
        assert!(unanswered(1, 0));
        assert!(!unanswered(1, 1));
        assert!(unanswered(3, 2));
        assert!(!unanswered(2, 3));
    }

    #[test]
    fn default_mic_follows_settings_and_off_stays_off() {
        assert_eq!(
            resolve_mic(MicChoice::Default, || Some("usb".into())),
            MicChoice::Device { id: "usb".into() }
        );
        assert_eq!(resolve_mic(MicChoice::Default, || None), MicChoice::Default);
        assert_eq!(
            resolve_mic(MicChoice::Off, || panic!("not asked")),
            MicChoice::Off
        );
        let device = MicChoice::Device { id: "b".into() };
        assert_eq!(resolve_mic(device.clone(), || Some("a".into())), device);
    }

    #[test]
    fn memory_is_kept_only_when_remembered() {
        let mut saved = CaptureMemory::clip();
        let mut next = CaptureMemory::clip();
        next.mode = "area".into();
        next.remember = false;
        remember(&mut saved, next.clone());
        assert_eq!(saved.mode, "window");
        assert!(!saved.remember);
        next.remember = true;
        remember(&mut saved, next.clone());
        assert_eq!(saved, next);
        let mut bad = CaptureMemory::call();
        bad.area = Some(AreaRect {
            x: 0.0,
            y: 0.0,
            width: f64::NAN,
            height: 10.0,
        });
        assert!(!bad.valid());
        let mut picture = CaptureMemory::clip();
        picture.picture = Some("high".into());
        assert!(picture.valid());
        picture.picture = Some("huge".into());
        assert!(!picture.valid());
    }

    #[test]
    fn export_bindings() {
        let config = ts_rs::Config::default().with_out_dir("../../ui/lib/bindings");
        CaptureKind::export_all(&config).unwrap();
        CaptureMemory::export_all(&config).unwrap();
        CaptureWindow::export_all(&config).unwrap();
    }
}
