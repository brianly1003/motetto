//! Native window placement and macOS companion behavior across full-screen Spaces.
#[cfg(target_os = "macos")]
pub fn configure(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    use objc2::{msg_send, runtime::AnyObject};
    let native = window.ns_window()? as *mut AnyObject;
    // SAFETY: setup calls this on AppKit's main thread; ns_window is the live NSWindow owned
    // by Tauri. Preserve its existing flags and add NSWindowCollectionBehaviorFullScreenAuxiliary.
    // Tauri's always-on-top is the floating level (3), under other apps' floating panels. Use
    // NSMainMenuWindowLevel - 1 (23): above every app window, below the menu bar and the menus
    // and popovers it opens (Sound, Control Center), which the screen-saver level (1000) covered.
    unsafe {
        let behavior: usize = msg_send![native, collectionBehavior];
        let _: () = msg_send![native, setCollectionBehavior: behavior | (1 << 8)];
        let _: () = msg_send![native, setLevel: 23isize];
    }
    Ok(())
}
#[cfg(not(target_os = "macos"))]
pub fn configure(_window: &tauri::WebviewWindow) -> tauri::Result<()> {
    Ok(())
}

/// Keep the logo a native template image while the recording dot and timer use recording red.
#[cfg(target_os = "macos")]
pub fn tray_title(tray: &tauri::tray::TrayIcon, title: String) -> tauri::Result<()> {
    use objc2::{class, msg_send, runtime::AnyObject};
    use objc2_foundation::NSString;
    tray.with_inner_tray_icon(move |inner| {
        let Some(item) = inner.ns_status_item() else {
            return;
        };
        let Some(main_thread) = objc2::MainThreadMarker::new() else {
            return;
        };
        let Some(button) = item.button(main_thread) else {
            return;
        };
        let title = NSString::from_str(&title);
        let key = NSString::from_str("NSColor");
        // SAFETY: with_inner_tray_icon runs on AppKit's main thread. The live status button
        // retains its attributed title; release our owned string after assigning it.
        unsafe {
            let color: *mut AnyObject = msg_send![class!(NSColor), systemRedColor];
            let attributes: *mut AnyObject =
                msg_send![class!(NSDictionary), dictionaryWithObject: color, forKey: &*key];
            let value: *mut AnyObject = msg_send![class!(NSAttributedString), alloc];
            let value: *mut AnyObject =
                msg_send![value, initWithString: &*title, attributes: attributes];
            let _: () = msg_send![&*button, setAttributedTitle: value];
            let _: () = msg_send![value, release];
        }
    })
}
#[cfg(not(target_os = "macos"))]
pub fn tray_title(tray: &tauri::tray::TrayIcon, title: String) -> tauri::Result<()> {
    tray.set_title(Some(title))
}

/// Start below the default recording Pill, within the OS work area (Dock/taskbar excluded).
pub fn place_main(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let Some(monitor) = window.current_monitor()?.or(window.primary_monitor()?) else {
        return Ok(());
    };
    let area = monitor.work_area();
    let outer = window.outer_size()?;
    let inner = window.inner_size()?;
    let frame = outer.height.saturating_sub(inner.height);
    let scale = window.scale_factor()?;
    let (y, height) = main_vertical(
        area.position.y,
        area.size.height,
        outer.height,
        (520.0 * scale) as u32 + frame,
        scale,
    );
    if height < outer.height {
        window.set_size(tauri::PhysicalSize::new(
            inner.width,
            height.saturating_sub(frame),
        ))?;
    }
    window.set_position(tauri::PhysicalPosition::new(window.outer_position()?.x, y))
}

fn main_vertical(y: i32, height: u32, window: u32, minimum: u32, scale: f64) -> (i32, u32) {
    let bottom = (16.0 * scale) as u32;
    // 16 px above the 72 px companion window, then 16 px between it and the library.
    let top = ((104.0 * scale) as u32).min(height.saturating_sub(minimum + bottom));
    (
        y + top as i32,
        window.min(height.saturating_sub(top + bottom)),
    )
}

#[cfg(test)]
mod tests {
    use super::main_vertical;

    #[test]
    fn library_clears_the_pill_and_stays_inside_the_work_area() {
        assert_eq!(main_vertical(25, 850, 828, 548, 1.0), (129, 730));
        assert_eq!(main_vertical(50, 1700, 1656, 1096, 2.0), (258, 1460));
        // Smaller display: preserve the minimum height by reducing the gap above the library.
        assert_eq!(main_vertical(0, 600, 828, 548, 1.0), (36, 548));
        // Negative display origins and a window that already fits keep their size.
        assert_eq!(main_vertical(-900, 900, 600, 548, 1.0), (-796, 600));
    }
}
