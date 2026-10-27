//! Picture filters and source enumeration, independent from the system-audio filter.
use super::*;
pub(super) fn picture_filter(
    content: &SCShareableContent,
    screen: &ScreenCapture,
) -> Result<(SCContentFilter, u32, u32), String> {
    let (filter, width, height) = match screen.target {
        ScreenTarget::Window(id) => {
            let window = content
                .windows()
                .into_iter()
                .find(|w| w.window_id() == id)
                .ok_or("the call window is no longer available")?;
            let frame = window.frame();
            (
                SCContentFilter::create()
                    .with_window(&window)
                    .build()
                    .map_err(|e| e.to_string())?,
                frame.size.width * screen.scale,
                frame.size.height * screen.scale,
            )
        }
        ScreenTarget::Display(id) => {
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
            (
                SCContentFilter::create()
                    .with_display(&display)
                    .with_excluding_applications(&own, &[])
                    .build()
                    .map_err(|e| e.to_string())?,
                f64::from(display.width()),
                f64::from(display.height()),
            )
        }
    };
    let (width, height) = video::fit_size(width, height, screen.preset);
    Ok((filter, width, height))
}

/// Available windows, including off-screen windows so a watcher can distinguish hiding from loss.
pub fn screen_windows() -> Result<Vec<video::WindowInfo>, Box<dyn Error>> {
    Ok(SCShareableContent::get()?
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
                width: frame.size.width,
                height: frame.size.height,
                on_screen: w.is_on_screen(),
                own: app.is_some_and(|a| a.process_id() as u32 == std::process::id()),
            }
        })
        .collect())
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
