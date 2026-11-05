//! Mouse clicks and the reach for Stop during a screen recording. Clicks are appended to
//! `screen-clicks.json` as `[{ "ms": u64 }]` in meeting time (blue dots on the saved video's
//! timeline); the pointer is sampled at 4 Hz against the recorded target so trim can suggest
//! cutting the moment the user reached for Stop. The pure parts are tested; AppKit sits in `native`.
use std::path::Path;

/// Longest cut suggested for the reach, in milliseconds.
const REACH_CAP_MS: u64 = 3000;
/// The pointer samples kept: the last 10 s at 4 Hz.
#[cfg(any(target_os = "macos", test))]
const RING: usize = 40;

/// The recorded target in global points, top-left origin (ScreenCaptureKit window frames).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TargetRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Meeting time of a click: the part's offset plus the event's time since the part began.
/// Both host times are seconds since boot.
pub fn to_meeting_ms(part_offset_ms: u64, part_start_host_s: f64, event_s: f64) -> u64 {
    part_offset_ms + ((event_s - part_start_host_s).max(0.0) * 1000.0).round() as u64
}

/// How long before Stop the pointer left the target: from the first outside sample after the
/// last inside one. 0 when the pointer never left or no sample exists, capped at 3 s.
pub fn reach_ms(samples: &[(u64, bool)], stop_ms: u64) -> u64 {
    let Some(last_inside) = samples.iter().rposition(|&(_, inside)| inside) else {
        return 0;
    };
    match samples.get(last_inside + 1) {
        Some(&(left_ms, _)) => stop_ms.saturating_sub(left_ms).min(REACH_CAP_MS),
        None => 0,
    }
}

/// AppKit gives the pointer in global points with a bottom-left origin; the target is top-left.
#[cfg(any(target_os = "macos", test))]
fn to_top_left(x: f64, y: f64, primary_height: f64) -> (f64, f64) {
    (x, primary_height - y)
}

#[cfg(any(target_os = "macos", test))]
fn contains(rect: &TargetRect, x: f64, y: f64) -> bool {
    x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height
}

/// The file's text after adding clicks; a missing or unreadable file starts a new list.
#[cfg(any(target_os = "macos", test))]
fn append_json(existing: Option<&str>, new: &[u64]) -> String {
    let mut all: Vec<serde_json::Value> = existing
        .and_then(|text| serde_json::from_str(text).ok())
        .unwrap_or_default();
    all.extend(new.iter().map(|ms| serde_json::json!({ "ms": ms })));
    serde_json::Value::Array(all).to_string()
}

#[cfg(target_os = "macos")]
pub use native::start;

#[cfg(not(target_os = "macos"))]
pub struct Clicks;

/// Click capture is macOS only for now; elsewhere the recording simply has no dots.
#[cfg(not(target_os = "macos"))]
pub fn start(
    _app: &tauri::AppHandle,
    _dir: &Path,
    _part_offset_ms: u64,
    _target: Option<TargetRect>,
) -> Clicks {
    Clicks
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{NSEvent, NSEventMask};
    use objc2_foundation::NSProcessInfo;
    use std::path::PathBuf;
    use std::ptr::NonNull;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};

    #[repr(C)]
    struct CgRect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGMainDisplayID() -> u32;
        fn CGDisplayBounds(display: u32) -> CgRect;
    }

    /// Height of the primary display in points: the flip between AppKit's and the target's origin.
    fn primary_height() -> f64 {
        // SAFETY: both are plain CoreGraphics queries with no pointers or ownership, safe on any thread.
        unsafe { CGDisplayBounds(CGMainDisplayID()).height }
    }

    fn uptime_s() -> f64 {
        NSProcessInfo::processInfo().systemUptime()
    }

    /// The monitor's token. AppKit's monitor calls are made on the main thread only, and the token
    /// is only ever taken out and used there.
    struct Token(Retained<AnyObject>);
    // SAFETY: the token is created, read and removed only inside main-thread closures; other
    // threads just move the box that holds it.
    unsafe impl Send for Token {}

    #[derive(Default)]
    struct Shared {
        clicks: Vec<u64>,
        samples: Vec<(u64, bool)>,
    }

    /// Handle for one part's monitor; dropping it removes the monitor and flushes the clicks.
    pub struct Clicks {
        app: tauri::AppHandle,
        monitor: Arc<Mutex<Option<Token>>>,
        done: Arc<AtomicBool>,
        worker: Option<JoinHandle<()>>,
        shared: Arc<Mutex<Shared>>,
        file: PathBuf,
    }

    impl Clicks {
        /// The reach to suggest for a Stop click at `stop_ms` (meeting time); 0 without samples.
        pub fn reach(&self, stop_ms: u64) -> u64 {
            self.shared
                .lock()
                .map(|s| reach_ms(&s.samples, stop_ms))
                .unwrap_or(0)
        }
    }

    fn flush(shared: &Mutex<Shared>, file: &Path) {
        let Ok(mut state) = shared.lock() else {
            return;
        };
        if state.clicks.is_empty() {
            return;
        }
        let text = append_json(std::fs::read_to_string(file).ok().as_deref(), &state.clicks);
        if std::fs::write(file, text).is_ok() {
            state.clicks.clear();
        }
    }

    /// Watch left clicks anywhere (needs no permission for the app's own windows; others may need
    /// Input Monitoring, which leaves the dots out) and, with a target, sample the pointer.
    pub fn start(
        app: &tauri::AppHandle,
        dir: &Path,
        part_offset_ms: u64,
        target: Option<TargetRect>,
    ) -> Clicks {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let begun = uptime_s();
        let monitor = Arc::new(Mutex::new(None));
        let (handler_shared, slot) = (shared.clone(), monitor.clone());
        // AppKit wants its monitor calls on the main thread, which also delivers the handler.
        let _ = app.run_on_main_thread(move || {
            let handler = RcBlock::new(move |event: NonNull<NSEvent>| {
                // SAFETY: AppKit passes a live NSEvent for the duration of this call.
                let at = unsafe { event.as_ref() }.timestamp();
                if let Ok(mut s) = handler_shared.lock() {
                    s.clicks.push(to_meeting_ms(part_offset_ms, begun, at));
                }
            });
            // AppKit copies the block; the token is removed in Drop.
            let token = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(
                NSEventMask::LeftMouseDown,
                &handler,
            );
            if let (Ok(mut slot), Some(token)) = (slot.lock(), token) {
                *slot = Some(Token(token));
            }
        });
        let done = Arc::new(AtomicBool::new(false));
        let file = dir.join("screen-clicks.json");
        let worker = {
            let (done, shared, file) = (done.clone(), shared.clone(), file.clone());
            let height = primary_height();
            let began = Instant::now();
            std::thread::spawn(move || {
                let mut tick = 0u32;
                while !done.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(250));
                    tick += 1;
                    if let Some(rect) = &target {
                        let at = NSEvent::mouseLocation();
                        let (x, y) = to_top_left(at.x, at.y, height);
                        let ms = part_offset_ms + began.elapsed().as_millis() as u64;
                        if let Ok(mut s) = shared.lock() {
                            if s.samples.len() == RING {
                                s.samples.remove(0);
                            }
                            s.samples.push((ms, contains(rect, x, y)));
                        }
                    }
                    if tick.is_multiple_of(8) {
                        flush(&shared, &file);
                    }
                }
            })
        };
        Clicks {
            app: app.clone(),
            monitor,
            done,
            worker: Some(worker),
            shared,
            file,
        }
    }

    impl Drop for Clicks {
        fn drop(&mut self) {
            // Queued behind the install on the main thread; it never waits, so Drop cannot
            // deadlock there. The token is taken out, so it is removed exactly once.
            let slot = self.monitor.clone();
            let _ = self.app.run_on_main_thread(move || {
                if let Ok(mut slot) = slot.lock()
                    && let Some(Token(monitor)) = slot.take()
                {
                    // SAFETY: the token came from addGlobalMonitor and is removed once, here.
                    unsafe { NSEvent::removeMonitor(&monitor) };
                }
            });
            self.done.store(true, Ordering::Release);
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
            flush(&self.shared, &self.file);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reach_is_time_since_pointer_left() {
        let samples = [(0, true), (9000, true), (9400, false), (10000, false)];
        assert_eq!(reach_ms(&samples, 10_200), 800);
    }

    #[test]
    fn reach_is_capped() {
        assert_eq!(reach_ms(&[(0, true), (1000, false)], 9000), 3000);
    }

    #[test]
    fn reach_zero_when_still_inside() {
        assert_eq!(reach_ms(&[(0, false), (500, true)], 900), 0);
        assert_eq!(reach_ms(&[], 900), 0);
    }

    #[test]
    fn meeting_ms_adds_offset() {
        assert_eq!(to_meeting_ms(72_000, 10.0, 11.5), 73_500);
    }

    #[test]
    fn pointer_flips_to_a_top_left_origin() {
        assert_eq!(to_top_left(100.0, 900.0, 1000.0), (100.0, 100.0));
        let rect = TargetRect {
            x: 50.0,
            y: 50.0,
            width: 100.0,
            height: 100.0,
        };
        assert!(contains(&rect, 100.0, 100.0));
        assert!(!contains(&rect, 100.0, 200.0));
    }

    #[test]
    fn clicks_append_to_the_list() {
        assert_eq!(append_json(None, &[5]), r#"[{"ms":5}]"#);
        assert_eq!(
            append_json(Some(r#"[{"ms":5}]"#), &[7]),
            r#"[{"ms":5},{"ms":7}]"#
        );
        assert_eq!(append_json(Some("junk"), &[]), "[]");
    }
}
