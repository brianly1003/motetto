//! One persistent recording window; Rust owns visibility and size.
use motetto_core::events::{AppState, CompanionSize};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};
use tauri::{LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

#[derive(Default)]
pub struct State {
    ready: AtomicBool,
    previous: Mutex<Option<(bool, CompanionSize, bool)>>,
    /// Where the window was before the Ask card moved it to the top middle.
    before_ask: Mutex<Option<PhysicalPosition<i32>>>,
}

pub fn create(app: &tauri::AppHandle) -> tauri::Result<()> {
    app.manage(State::default());
    let window = crate::window_native::protect(
        app,
        WebviewWindowBuilder::new(app, "companion", WebviewUrl::App("companion/".into())),
    )
    .title("Motetto recording controls")
    .inner_size(460.0, 72.0)
    .decorations(false)
    .transparent(true)
    // The page draws each size's own shadow; the native one outlines the whole transparent
    // window as a dark rectangle around the Ask card and the Pill.
    .shadow(false)
    .resizable(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .visible_on_all_workspaces(true)
    .focused(false)
    .visible(false)
    .on_page_load(|webview, _| {
        webview
            .app_handle()
            .state::<State>()
            .ready
            .store(false, Ordering::SeqCst);
        if webview
            .app_handle()
            .state::<crate::controller::Controller>()
            .state()
            .is_ok_and(|s| s.companion.visible)
        {
            crate::desktop::show(webview.app_handle());
        }
    })
    .build()?;
    crate::window_native::configure(&window)?;
    if let Some(monitor) = window.primary_monitor()? {
        let area = monitor.work_area();
        let scale = monitor.scale_factor();
        window.set_position(PhysicalPosition::new(
            area.position.x + area.size.width as i32 - (436.0 * scale) as i32,
            area.position.y + (16.0 * scale) as i32,
        ))?;
    }
    Ok(())
}

pub fn ready(app: &tauri::AppHandle) {
    app.state::<State>().ready.store(true, Ordering::SeqCst);
}

pub fn available(app: &tauri::AppHandle) -> bool {
    app.state::<State>().ready.load(Ordering::SeqCst)
        && app
            .get_webview_window("companion")
            .is_some_and(|w| w.is_visible().unwrap_or(false))
}

pub fn update(app: &tauri::AppHandle, state: &AppState) -> Result<(), String> {
    let managed = app.state::<State>();
    let mut previous = managed
        .previous
        .lock()
        .map_err(|_| "Companion state is unavailable")?;
    let next = (
        state.companion.visible,
        state.companion.size,
        compact(state),
    );
    if *previous == Some(next) {
        return Ok(());
    }
    let window = app
        .get_webview_window("companion")
        .ok_or("Recording controls are unavailable")?;
    let (width, height) = dimensions(next.1, next.2);
    let was_ask = previous.is_some_and(|p| p.1 == CompanionSize::Ask);
    let mut before_ask = managed
        .before_ask
        .lock()
        .map_err(|_| "Companion state is unavailable")?;
    let result = (|| -> tauri::Result<()> {
        if next.1 == CompanionSize::Ask {
            if !was_ask {
                *before_ask = Some(window.outer_position()?);
            }
            window.set_size(LogicalSize::new(width, height))?;
            // primary display, not the call's display (SPEC 7.10).
            if let Some(monitor) = window.primary_monitor()? {
                let area = monitor.work_area();
                let (x, y) = top_middle(
                    (area.position.x, area.position.y, area.size.width as i32),
                    width,
                    monitor.scale_factor(),
                );
                window.set_position(PhysicalPosition::new(x, y))?;
            }
        } else {
            if was_ask && let Some(position) = before_ask.take() {
                // Back where it was at the new width, so resize() below keeps x and only
                // clamps it on screen (it anchors on the right edge of the old size).
                window.set_size(LogicalSize::new(width, 72.0))?;
                window.set_position(position)?;
            }
            resize(&window, width, height)?;
        }
        if next.1 == CompanionSize::Tucked && !next.2 {
            snap_to_edge(&window)?;
        }
        if next.0 {
            window.show()?;
        } else {
            window.hide()?;
        }
        Ok(())
    })();
    result.map_err(|e| format!("Couldn't display recording controls: {e}"))?;
    *previous = Some(next);
    Ok(())
}

fn resize(window: &tauri::WebviewWindow, width: f64, height: f64) -> tauri::Result<()> {
    let position = window.outer_position()?;
    let old = window.outer_size()?;
    let scale = window.scale_factor()?;
    let mut height = height;
    let mut x = position.x + old.width as i32 - (width * scale) as i32;
    let mut y = position.y;
    if let Some(monitor) = window.current_monitor()? {
        let area = monitor.work_area();
        height = height.min(f64::from(area.size.height) / scale - 32.0);
        let left = area.position.x + (10.0 * scale) as i32;
        let top = area.position.y + (16.0 * scale) as i32;
        let right = area.position.x + area.size.width as i32 - ((width + 10.0) * scale) as i32;
        let bottom = area.position.y + area.size.height as i32 - ((height + 16.0) * scale) as i32;
        x = x.clamp(left, right.max(left));
        y = y.clamp(top, bottom.max(top));
    }
    if !height.is_finite() {
        height = 560.0; // no monitor reported: the previous fixed Sidecar height
    }
    window.set_size(LogicalSize::new(width, height))?;
    window.set_position(PhysicalPosition::new(x, y))
}

/// Tucked floats 10 px in from the nearer side edge, at the height it was left (DESIGN 4.3).
/// Rectangles are physical pixels: (x, y, width, height); `scale` converts the 10/16 px margins.
fn edge_position(
    area: (i32, i32, i32, i32),
    window: (i32, i32, i32, i32),
    scale: f64,
) -> (i32, i32) {
    let (ax, ay, aw, ah) = area;
    let (x, y, w, h) = window;
    let side = (10.0 * scale) as i32;
    let edge = (16.0 * scale) as i32;
    let left = x + w / 2 < ax + aw / 2;
    let x = if left { ax + side } else { ax + aw - w - side };
    let top = ay + edge;
    (x, y.clamp(top, (ay + ah - h - edge).max(top)))
}

/// The Ask card: centred on the work area (x, y, width), 12 px below its top (DESIGN 4.2).
fn top_middle(area: (i32, i32, i32), width: f64, scale: f64) -> (i32, i32) {
    let (ax, ay, aw) = area;
    (
        ax + (aw - (width * scale) as i32) / 2,
        ay + (12.0 * scale) as i32,
    )
}

/// A fast drag can leave no intersecting monitor. Recover on the closest work area.
fn drag_monitor(window: &tauri::WebviewWindow) -> tauri::Result<Option<tauri::Monitor>> {
    if let Some(monitor) = window.current_monitor()? {
        return Ok(Some(monitor));
    }
    let position = window.outer_position()?;
    let size = window.outer_size()?;
    let center = (
        f64::from(position.x) + f64::from(size.width) / 2.0,
        f64::from(position.y) + f64::from(size.height) / 2.0,
    );
    Ok(window.available_monitors()?.into_iter().min_by(|a, b| {
        let distance = |m: &tauri::Monitor| {
            let area = m.work_area();
            area_distance(
                (
                    area.position.x,
                    area.position.y,
                    area.size.width,
                    area.size.height,
                ),
                center,
            )
        };
        distance(a).total_cmp(&distance(b))
    }))
}

fn area_distance(area: (i32, i32, u32, u32), (x, y): (f64, f64)) -> f64 {
    let (ax, ay, w, h) = area;
    let dx = x - x.clamp(f64::from(ax), f64::from(ax) + f64::from(w));
    let dy = y - y.clamp(f64::from(ay), f64::from(ay) + f64::from(h));
    dx * dx + dy * dy
}

fn inside_position(
    area: (i32, i32, i32, i32),
    window: (i32, i32, i32, i32),
    scale: f64,
) -> (i32, i32) {
    let (ax, ay, aw, ah) = area;
    let (x, y, w, h) = window;
    let left = ax + (10.0 * scale) as i32;
    let top = ay + (16.0 * scale) as i32;
    (
        x.clamp(left, (ax + aw - w - (10.0 * scale) as i32).max(left)),
        y.clamp(top, (ay + ah - h - (16.0 * scale) as i32).max(top)),
    )
}

/// Native movement events also cover a release outside the webview after a fast swipe.
pub fn keep_tucked_on_screen(window: &tauri::Window) {
    if window.label() != "companion" {
        return;
    }
    let Some(controller) = window.try_state::<crate::controller::Controller>() else {
        return;
    };
    if !controller
        .state()
        .is_ok_and(|s| s.companion.size == CompanionSize::Tucked && !compact(&s))
    {
        return;
    }
    let Some(webview) = window.app_handle().get_webview_window("companion") else {
        return;
    };
    if let Err(error) = keep_inside(&webview) {
        eprintln!("Couldn't keep recording controls on screen: {error}");
    }
}

fn keep_inside(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let Some(monitor) = drag_monitor(window)? else {
        return Ok(());
    };
    let area = monitor.work_area();
    let position = window.outer_position()?;
    let size = window.outer_size()?;
    let (x, y) = inside_position(
        (
            area.position.x,
            area.position.y,
            area.size.width as i32,
            area.size.height as i32,
        ),
        (
            position.x,
            position.y,
            size.width as i32,
            size.height as i32,
        ),
        monitor.scale_factor(),
    );
    // A correction emits Moved too; leave an already-safe position alone.
    if (x, y) != (position.x, position.y) {
        window.set_position(PhysicalPosition::new(x, y))?;
    }
    Ok(())
}

fn snap_to_edge(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let Some(monitor) = drag_monitor(window)? else {
        return Ok(());
    };
    let area = monitor.work_area();
    let (position, size) = (window.outer_position()?, window.outer_size()?);
    let (x, y) = edge_position(
        (
            area.position.x,
            area.position.y,
            area.size.width as i32,
            area.size.height as i32,
        ),
        (
            position.x,
            position.y,
            size.width as i32,
            size.height as i32,
        ),
        window.scale_factor()?,
    );
    window.set_position(PhysicalPosition::new(x, y))
}

/// Called by the Tucked capsule after the user drags it (the native drag has ended).
#[tauri::command]
pub fn snap_companion(window: tauri::WebviewWindow) -> Result<(), String> {
    if window.label() != "companion" {
        return Err("Only recording controls can be moved this way".into());
    }
    let state = window.state::<crate::controller::Controller>().state()?;
    if state.companion.size != CompanionSize::Tucked {
        return Ok(());
    }
    snap_to_edge(&window).map_err(|e| e.to_string())
}

/// The plain Pill (with its caption row) unless recording; paused keeps an open Sidecar for notes.
fn compact(state: &AppState) -> bool {
    use motetto_core::events::SessionState;
    state.quit_pending
        || match state.state {
            SessionState::Recording => false,
            SessionState::Paused => state.companion.size != CompanionSize::Sidecar,
            _ => true,
        }
}
/// Height of the Ask card's open menu: 76 px measured in the rendered component (two items, 1 px gap,
/// 5 px padding, 1 px borders); ui/tests/companion.playwright.js asserts it.
const ASK_MENU_HEIGHT: f64 = 76.0;
fn dimensions(size: CompanionSize, compact: bool) -> (f64, f64) {
    if size == CompanionSize::Ask {
        // The 460 px card plus 10 px each side for its hover close button.
        return (480.0, 164.0);
    }
    if compact {
        // Stopped and the quit question show a Pill with its caption row, as in the mockup.
        return (460.0, 72.0);
    }
    match size {
        CompanionSize::Dot => (40.0, 40.0),
        // Dot, Pause and Stop stacked (owner 2026-10-20: pause or stop without opening it).
        CompanionSize::Tucked => (28.0, 104.0),
        // Fills the work area 16 px from top and bottom, as in the mockup (DESIGN 4.3: 40-100 %);
        // resize() clamps it. no saved outer height yet (SPEC sidecar_height_pct).
        CompanionSize::Sidecar => (340.0, f64::INFINITY),
        CompanionSize::Ask | CompanionSize::Pill => (460.0, 72.0),
    }
}
#[tauri::command]
pub fn companion_menu(
    window: tauri::WebviewWindow,
    open: bool,
    extra: Option<f64>,
) -> Result<(), String> {
    // `extra`: the height the page needs below its controls right now (the mic menu, or the Ask
    // card's mic line plus its open menu); without it a menu gets the old fixed room.
    let extra = extra.map(|e| e.clamp(0.0, 600.0));
    if window.label() != "companion" {
        return Err("Only recording controls can resize this window".into());
    }
    let state = window.state::<crate::controller::Controller>().state()?;
    // The Ask card keeps its place (top middle) and grows downward only while its menu is open.
    if state.companion.size == CompanionSize::Ask {
        let (width, height) = dimensions(CompanionSize::Ask, false);
        let height = height + extra.unwrap_or(if open { ASK_MENU_HEIGHT + 8.0 } else { 0.0 });
        return window
            .set_size(LogicalSize::new(width, height))
            .map_err(|e| e.to_string());
    }
    let (width, height) = dimensions(state.companion.size, compact(&state));
    resize(
        &window,
        if open { width.max(460.0) } else { width },
        match (open, extra) {
            (true, Some(extra)) => height + extra,
            (true, None) => height.max(300.0),
            (false, _) => height,
        },
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn pin_companion(app: tauri::AppHandle, pinned: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<crate::controller::Controller>()
            .action(crate::controller::Action::Pin(pinned))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn sharing_mode(app: tauri::AppHandle, active: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<crate::controller::Controller>()
            .action(crate::controller::Action::Sharing(active))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::{area_distance, dimensions, edge_position, inside_position, top_middle};
    use motetto_core::events::CompanionSize;

    #[test]
    fn fast_swipes_stay_inside_work_area_at_any_scale() {
        for scale in [1.0, 2.0] {
            let w = (28.0 * scale) as i32;
            let h = (104.0 * scale) as i32;
            let area = (-1920, 30, 1920, 1050);
            for (x, y) in [(-9000, -9000), (9000, 9000), (-900, 400)] {
                let (cx, cy) = inside_position(area, (x, y, w, h), scale);
                assert!(cx >= -1920 + (10.0 * scale) as i32);
                assert!(cx + w <= -(10.0 * scale) as i32);
                assert!(cy >= 30 + (16.0 * scale) as i32);
                assert!(cy + h <= 1080 - (16.0 * scale) as i32);
                assert_eq!(inside_position(area, (cx, cy, w, h), scale), (cx, cy));
            }
        }
        assert_eq!(
            inside_position((0, 0, 20, 20), (999, 999, 28, 104), 1.0),
            (10, 16)
        );
        let left = (-1920, 0, 1920, 1080);
        let right = (0, 0, 1920, 1080);
        assert!(area_distance(left, (-3000.0, 400.0)) < area_distance(right, (-3000.0, 400.0)));
        assert!(area_distance(right, (4000.0, 400.0)) < area_distance(left, (4000.0, 400.0)));
    }

    #[test]
    fn ask_card_is_fixed_and_sits_top_middle_of_the_work_area() {
        assert_eq!(dimensions(CompanionSize::Ask, true), (480.0, 164.0));
        assert_eq!(dimensions(CompanionSize::Ask, false), (480.0, 164.0));
        assert_eq!(dimensions(CompanionSize::Pill, true), (460.0, 72.0));
        // 1440 px wide at 2x under a 25 px menu bar: 960 px card, centred, 12 px down.
        assert_eq!(
            top_middle((0, 50, 2880), 480.0, 2.0),
            ((2880 - 960) / 2, 50 + 24)
        );
        assert_eq!(top_middle((-1920, 0, 1920), 480.0, 1.0), (-1920 + 720, 12));
    }

    #[test]
    fn tucked_snaps_to_the_nearer_side_edge_and_stays_on_screen() {
        // A 1440 x 900 work area under a 25 px menu bar, at 2x: the capsule is 48 x 128 px.
        let area = (0, 50, 2880, 1750);
        // Dropped left of centre: 10 px (20 physical) in from the left, height kept.
        assert_eq!(edge_position(area, (900, 600, 48, 128), 2.0), (20, 600));
        // Right of centre: its right side 10 px in from the right edge.
        assert_eq!(
            edge_position(area, (2000, 600, 48, 128), 2.0),
            (2880 - 48 - 20, 600)
        );
        // Dragged above the menu bar or below the Dock: kept 16 px inside the work area.
        assert_eq!(edge_position(area, (2000, 0, 48, 128), 2.0).1, 50 + 32);
        assert_eq!(
            edge_position(area, (2000, 1790, 48, 128), 2.0).1,
            50 + 1750 - 128 - 32
        );
        // A second display to the left (negative x) uses its own edges.
        assert_eq!(
            edge_position((-1920, 0, 1920, 1080), (-300, 400, 24, 64), 1.0),
            (-24 - 10, 400)
        );
    }
}
