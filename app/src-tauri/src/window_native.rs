//! Native window placement and macOS companion behavior across full-screen Spaces.
#[cfg(target_os = "macos")]
pub fn configure(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    // Callers include shortcut and command threads; AppKit aborts the app when these setters
    // run anywhere but the main thread, so they always go there.
    let target = window.clone();
    window.run_on_main_thread(move || {
        use objc2::{msg_send, runtime::AnyObject};
        let Ok(native) = target.ns_window() else {
            return;
        };
        let native = native as *mut AnyObject;
        // SAFETY: this closure runs on AppKit's main thread; ns_window is the live NSWindow owned
        // by Tauri. Preserve its existing flags and add NSWindowCollectionBehaviorFullScreenAuxiliary.
        // Tauri's always-on-top is the floating level (3), under other apps' floating panels. Use
        // NSMainMenuWindowLevel - 1 (23): above every app window, below the menu bar and the menus
        // and popovers it opens (Sound, Control Center), which the screen-saver level (1000) covered.
        unsafe {
            let behavior: usize = msg_send![native, collectionBehavior];
            let _: () = msg_send![native, setCollectionBehavior: behavior | (1 << 8)];
            let _: () = msg_send![native, setLevel: 23isize];
        }
    })
}
#[cfg(not(target_os = "macos"))]
pub fn configure(_window: &tauri::WebviewWindow) -> tauri::Result<()> {
    Ok(())
}

/// What every window was last set to: 0 hidden, 1 shown, 2 unsure (a window refused), so the
/// next change applies again.
static SHOWN: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Every window Motetto builds starts hidden from screen shares unless the user chose Shown
/// (SPEC 14.1).
pub fn protect<'a, R: tauri::Runtime, M: tauri::Manager<R>>(
    _app: &M,
    builder: tauri::WebviewWindowBuilder<'a, R, M>,
) -> tauri::WebviewWindowBuilder<'a, R, M> {
    builder.content_protected(SHOWN.load(std::sync::atomic::Ordering::SeqCst) != 1)
}

/// Sets every window, even after one refuses; any refusal is reported and the state is marked
/// unsure so the same choice is applied again next time.
fn apply_all<E: ToString>(
    shown: bool,
    windows: impl IntoIterator<Item = Result<(), E>>,
) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    SHOWN.store(u8::from(shown), Ordering::SeqCst);
    let mut first = None;
    for result in windows {
        if let Err(error) = result {
            first.get_or_insert(error.to_string());
        }
    }
    match first {
        Some(error) => {
            SHOWN.store(2, Ordering::SeqCst);
            Err(error)
        }
        None => Ok(()),
    }
}

/// Applies the Hidden / Shown switch to every open window when it changes.
pub fn follow_share_visible(app: &tauri::AppHandle, shown: bool) -> Result<(), String> {
    use tauri::Manager;
    if SHOWN.load(std::sync::atomic::Ordering::SeqCst) == u8::from(shown) {
        return Ok(());
    }
    // Stored before the windows are set, so a window built meanwhile starts with the new choice.
    SHOWN.store(u8::from(shown), std::sync::atomic::Ordering::SeqCst);
    apply_all(
        shown,
        app.webview_windows()
            .values()
            .map(|window| window.set_content_protected(!shown))
            .collect::<Vec<_>>(),
    )
}

/// The app that has the keyboard now (its process id), when it isn't Motetto.
#[cfg(target_os = "macos")]
pub fn frontmost_app() -> Option<i32> {
    use objc2::{class, msg_send, runtime::AnyObject};
    // SAFETY: NSWorkspace's shared instance and its frontmost application are autoreleased
    // objects owned by AppKit; we only read the pid and keep no pointer.
    let pid: i32 = unsafe {
        let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        let front: *mut AnyObject = msg_send![workspace, frontmostApplication];
        if front.is_null() {
            return None;
        }
        msg_send![front, processIdentifier]
    };
    (pid > 0 && pid as u32 != std::process::id()).then_some(pid)
}
#[cfg(not(target_os = "macos"))]
pub fn frontmost_app() -> Option<i32> {
    // Windows: GetForegroundWindow before editing, SetForegroundWindow after (plan task W2).
    None
}
/// Gives the keyboard back to the app that had it before the prompter's editor.
#[cfg(target_os = "macos")]
pub fn activate_app(pid: i32) {
    use objc2::{class, msg_send, runtime::AnyObject};
    // SAFETY: runningApplicationWithProcessIdentifier returns an autoreleased object or nil for
    // a pid that has quit; nil is checked before the activate message.
    unsafe {
        let running: *mut AnyObject = msg_send![
            class!(NSRunningApplication),
            runningApplicationWithProcessIdentifier: pid
        ];
        if !running.is_null() {
            let _: bool = msg_send![running, activateWithOptions: 0usize];
        }
    }
}
#[cfg(not(target_os = "macos"))]
pub fn activate_app(_pid: i32) {}

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
    fn a_refusing_window_does_not_stop_the_others() {
        let mut set = Vec::new();
        let results = (0..3).map(|i| {
            set.push(i);
            if i == 1 { Err("refused") } else { Ok(()) }
        });
        assert_eq!(super::apply_all(true, results), Err("refused".into()));
        assert_eq!(set, [0, 1, 2]);
        // Unsure: the next change to the same value applies again.
        assert_eq!(super::SHOWN.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert_eq!(
            super::apply_all(false, Vec::<Result<(), String>>::new()),
            Ok(())
        );
        assert_eq!(super::SHOWN.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[test]
    fn every_window_starts_protected() {
        // Hidden from shares from the first frame (SPEC 14.1): each window built in code goes
        // through `protect`, and the one in tauri.conf.json is content protected.
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut builders = 0;
        for entry in std::fs::read_dir(&src).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "rs") || path.ends_with("window_native.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            for (at, _) in text.match_indices("WebviewWindowBuilder::new(") {
                builders += 1;
                let before = &text[at.saturating_sub(120)..at];
                assert!(
                    before.contains("window_native::protect("),
                    "{} builds a window without window_native::protect",
                    path.display()
                );
            }
        }
        assert!(
            builders >= 3,
            "companion, settings and onboarding windows: {builders}"
        );
        let config = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json"),
        )
        .unwrap();
        assert!(config.contains("\"contentProtected\": true"));
    }

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
