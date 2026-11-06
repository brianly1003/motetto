//! Picture filters and source enumeration, independent from the system-audio filter.
use super::*;
use screencapturekit::cg::CGRect;
/// The picture's filter and output size, plus the display-local source rect for an area.
pub(super) fn picture_filter(
    content: &SCShareableContent,
    screen: &ScreenCapture,
) -> Result<(SCContentFilter, u32, u32, Option<CGRect>), String> {
    // A display minus Motetto's own windows, for a whole display or an area of one.
    let display_filter = |id: u32| {
        let display = content
            .displays()
            .into_iter()
            .find(|d| d.display_id() == id)
            .ok_or("the display is no longer available")?;
        let apps = content.applications();
        let own: Vec<_> = apps
            .iter()
            .filter(|a| a.process_id() as u32 == std::process::id())
            .collect();
        let filter = SCContentFilter::create()
            .with_display(&display)
            .with_excluding_applications(&own, &[])
            .build()
            .map_err(|e| e.to_string())?;
        Ok::<_, String>((filter, display))
    };
    match screen.target {
        ScreenTarget::Window(id) => {
            let window = content
                .windows()
                .into_iter()
                .find(|w| w.window_id() == id)
                .ok_or("the window is no longer available")?;
            // Motetto's own windows are never recorded, whoever asks for one.
            if window
                .owning_application()
                .is_some_and(|a| a.process_id() as u32 == std::process::id())
            {
                return Err("Motetto's own windows are never recorded".into());
            }
            let frame = window.frame();
            let filter = SCContentFilter::create()
                .with_window(&window)
                .build()
                .map_err(|e| e.to_string())?;
            let (width, height) = video::fit_size(
                frame.size.width * screen.scale,
                frame.size.height * screen.scale,
                screen.preset,
            );
            Ok((filter, width, height, None))
        }
        ScreenTarget::Display(id) => {
            let (filter, display) = display_filter(id)?;
            let (width, height) = video::fit_size(
                f64::from(display.width()),
                f64::from(display.height()),
                screen.preset,
            );
            Ok((filter, width, height, None))
        }
        ScreenTarget::Area { display, rect } => {
            let (filter, display) = display_filter(display)?;
            let size = display.frame().size;
            let (rect, width, height) =
                video::area_rect(rect, size.width, size.height, screen.scale, screen.preset)?;
            let source = CGRect::new(rect.x, rect.y, rect.width, rect.height);
            Ok((filter, width, height, Some(source)))
        }
    }
}

/// Available windows, including off-screen windows so a watcher can distinguish hiding from loss.
/// `layer` is the front-to-back index; windows the Window Server does not list keep ScreenCaptureKit's
/// order after the listed ones.
pub fn screen_windows() -> Result<Vec<video::WindowInfo>, Box<dyn Error>> {
    let order = crate::platform::macos_calls::window_order().unwrap_or_default();
    let mut windows: Vec<_> = SCShareableContent::get()?
        .windows()
        .into_iter()
        .map(|w| {
            let app = w.owning_application();
            let frame = w.frame();
            video::WindowInfo {
                id: w.window_id(),
                bundle_id: app
                    .as_ref()
                    .map(|a| a.bundle_identifier())
                    .unwrap_or_default(),
                title: w.title().unwrap_or_default(),
                x: frame.origin.x,
                y: frame.origin.y,
                width: frame.size.width,
                height: frame.size.height,
                app: app
                    .as_ref()
                    .map(|a| a.application_name())
                    .unwrap_or_default(),
                layer: 0,
                on_screen: w.is_on_screen(),
                own: app.is_some_and(|a| a.process_id() as u32 == std::process::id()),
            }
        })
        .collect();
    // Stable sort: listed windows by their index, unlisted ones after them in the original order.
    let rank = |id: u32| order.iter().position(|o| *o == id).unwrap_or(order.len());
    windows.sort_by_key(|w| rank(w.id));
    for (i, w) in windows.iter_mut().enumerate() {
        w.layer = i as i64;
    }
    Ok(windows)
}

/// The display containing a window's centre, for converting its point-sized frame to pixels.
pub fn window_display(id: u32) -> Option<u32> {
    let content = SCShareableContent::get().ok()?;
    let window = content
        .windows()
        .into_iter()
        .find(|w| w.window_id() == id)?;
    let frame = window.frame();
    let (x, y) = (
        frame.origin.x + frame.size.width / 2.0,
        frame.origin.y + frame.size.height / 2.0,
    );
    content
        .displays()
        .into_iter()
        .find(|d| {
            let f = d.frame();
            x >= f.origin.x
                && x < f.origin.x + f.size.width
                && y >= f.origin.y
                && y < f.origin.y + f.size.height
        })
        .map(|d| d.display_id())
}

/// A window's frame as (x, y, width, height) in global points, top-left origin, from the Window
/// Server: quick, unlike listing shareable content.
pub fn window_frame(id: u32) -> Option<(f64, f64, f64, f64)> {
    crate::platform::macos_calls::window_bounds(id)
}
