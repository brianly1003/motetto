//! The screen setup: a transparent window over every display hosts the setup bar, so the mode,
//! mic, options and start timer are chosen before the first frame. The page runs the countdown
//! and calls `start_capture` (or `pick_capture` during a recording) once it ends.
use crate::controller::{Action, Controller};
use motetto_core::{
    events::{MicChoice, RecordSetup, ScreenPick},
    video::{self, AreaRect},
};
use serde::{Deserialize, Serialize};
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
}

/// A display as (id, x, y, width, height) in global points, top-left origin.
type Display = (u32, f64, f64, f64, f64);

/// Pickable windows (front first) on the display that holds each one's centre; a window
/// spanning two displays is offered on one of them.
fn on_displays(windows: &[video::WindowInfo], displays: &[Display]) -> Vec<CaptureWindow> {
    video::pickable(windows)
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

/// Opens one overlay per display, replacing any open ones. `mode` opens on that mode instead of
/// the remembered one. Display changes while it is open are not followed: reopening fixes them.
pub fn open(app: &tauri::AppHandle, kind: CaptureKind, mode: Option<&str>) -> Result<(), String> {
    if mode.is_some_and(|m| !MODES.contains(&m)) {
        return Err("Choose audio only, the entire screen, a window or an area.".into());
    }
    close(app);
    let displays = displays(app);
    if displays.is_empty() {
        return Err("No display found for the screen setup.".into());
    }
    // The keyboard (Esc, Return) goes to the main display's bar.
    let main = displays
        .iter()
        .position(|d| d.1 == 0.0 && d.2 == 0.0)
        .unwrap_or(0);
    for (i, &(id, x, y, width, height)) in displays.iter().enumerate() {
        let mut url = format!("capture/?display={id}&kind={kind:?}");
        if let Some(mode) = mode {
            url.push_str(&format!("&mode={mode}"));
        }
        let window = crate::window_native::protect(
            app,
            WebviewWindowBuilder::new(app, format!("capture-{id}"), WebviewUrl::App(url.into())),
        )
        // Never in a capture by any app, even with Shown on.
        .content_protected(true)
        .title("Motetto screen setup")
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
    Ok(())
}

pub fn close(app: &tauri::AppHandle) {
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
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn capture_windows(app: tauri::AppHandle) -> Result<Vec<CaptureWindow>, String> {
    blocking(move || {
        #[cfg(target_os = "macos")]
        {
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
        app.state::<Controller>()
            .action(Action::SetScreen(Some(screen)))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

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
            [(10, 2, 160.0, 100.0), (12, 1, 100.0, 100.0)]
        );
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
    }

    #[test]
    fn export_bindings() {
        let config = ts_rs::Config::default().with_out_dir("../../ui/lib/bindings");
        CaptureKind::export_all(&config).unwrap();
        CaptureMemory::export_all(&config).unwrap();
        CaptureWindow::export_all(&config).unwrap();
    }
}
