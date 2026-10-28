//! Is Motetto in the user's screen share? (SPEC 14.2) The prompter draws a 4 × 4 checker of
//! two colours for one frame; one captured frame of that display is searched for it, then dropped.
use std::collections::HashMap;

/// The checker's two colours (RGB), alternating like a chessboard.
pub const MARKER: [[u8; 3]; 2] = [[255, 0, 255], [0, 255, 0]];
/// Colour difference per channel still counted as the marker. Measured 2026-10-28 (macOS 26.6):
/// captures are sRGB with dithering up to 15 off. A faded copy of the window still counts: it is
/// in the share, and missing it would wrongly say Hidden.
const TOLERANCE: u8 = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PxRect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// The marker isn't in the capture: the share can't see Motetto.
    Hidden,
    /// The marker is in the capture.
    Visible,
    /// The check couldn't run (no permission, capture error): treated as not hidden.
    Unknown,
}
impl From<Result<bool, String>> for Check {
    fn from(found: Result<bool, String>) -> Self {
        match found {
            Ok(false) => Check::Hidden,
            Ok(true) => Check::Visible,
            Err(_) => Check::Unknown,
        }
    }
}

fn near(pixel: &[u8], colour: [u8; 3]) -> bool {
    pixel[..3]
        .iter()
        .zip(colour)
        .all(|(&p, c)| p.abs_diff(c) <= TOLERANCE)
}

/// Whether the checker, each cell `cell` pixels wide, appears anywhere inside `rect` of an RGBA
/// image `width` pixels wide.
pub fn find(rgba: &[u8], width: usize, rect: PxRect, cell: usize) -> bool {
    let cell = cell.max(1);
    let height = rgba.len() / 4 / width.max(1);
    let at = |x: usize, y: usize| &rgba[(y * width + x) * 4..(y * width + x) * 4 + 4];
    let (x_end, y_end) = ((rect.x + rect.w).min(width), (rect.y + rect.h).min(height));
    let span = 4 * cell;
    for y in rect.y..y_end.saturating_sub(span - 1) {
        for x in rect.x..x_end.saturating_sub(span - 1) {
            // Sample each cell's centre; the first cell decides cheaply.
            let hit = (0..4).all(|row| {
                (0..4).all(|col| {
                    near(
                        at(x + col * cell + cell / 2, y + row * cell + cell / 2),
                        MARKER[(row + col) % 2],
                    )
                })
            });
            if hit {
                return true;
            }
        }
    }
    false
}

/// One answer per (OS version, display) for the session: the answer only changes with the OS.
#[derive(Default)]
pub struct Cache {
    answers: HashMap<(String, String), Check>,
}
impl Cache {
    pub fn get(&self, os: &str, display: &str) -> Option<Check> {
        self.answers.get(&(os.into(), display.into())).copied()
    }
    /// Unknown answers are not kept: permission may be granted meanwhile.
    pub fn put(&mut self, os: &str, display: &str, answer: Check) {
        if answer != Check::Unknown {
            self.answers.insert((os.into(), display.into()), answer);
        }
    }
}

/// One frame of the display whose top-left corner is at `origin` (points, global), captured at
/// `width` × `height` pixels as RGBA. The caller searches it once and drops it (SPEC 14.2).
#[cfg(target_os = "macos")]
pub fn capture(origin: (f64, f64), width: u32, height: u32) -> Result<Vec<u8>, String> {
    use screencapturekit::prelude::{SCContentFilter, SCShareableContent, SCStreamConfiguration};
    use screencapturekit::screenshot_manager::{CGImageExt, SCScreenshotManager};
    let content = SCShareableContent::get().map_err(|e| e.to_string())?;
    let display = content
        .displays()
        .into_iter()
        .find(|d| {
            let frame = d.frame();
            (frame.origin.x - origin.0).abs() < 1.0 && (frame.origin.y - origin.1).abs() < 1.0
        })
        .ok_or("The display isn't available to capture")?;
    let filter = SCContentFilter::create()
        .with_display(&display)
        .with_excluding_windows(&[])
        .build()
        .map_err(|e| e.to_string())?;
    let config = SCStreamConfiguration::new()
        .with_width(width)
        .with_height(height)
        .with_shows_cursor(false);
    let image = SCScreenshotManager::capture_image(&filter, &config).map_err(|e| e.to_string())?;
    let rgba = image.rgba_data().map_err(|e| e.to_string())?;
    // A frame of another size would be searched with the wrong rows and could say Hidden.
    if rgba.len() != width as usize * height as usize * 4 {
        return Err("The captured frame has an unexpected size".into());
    }
    Ok(rgba)
}
#[cfg(not(target_os = "macos"))]
pub fn capture(_origin: (f64, f64), _width: u32, _height: u32) -> Result<Vec<u8>, String> {
    // Windows: one Windows.Graphics.Capture frame of the monitor (plan task W3).
    Err("The share check isn't available on this system yet".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(width: usize, height: usize) -> Vec<u8> {
        [128, 128, 128, 255].repeat(width * height)
    }
    fn draw(rgba: &mut [u8], width: usize, x: usize, y: usize, cell: usize, alpha: f32) {
        for row in 0..4 * cell {
            for col in 0..4 * cell {
                let colour = MARKER[(row / cell + col / cell) % 2];
                let i = ((y + row) * width + x + col) * 4;
                for c in 0..3 {
                    rgba[i + c] = (colour[c] as f32 * alpha + 128.0 * (1.0 - alpha)).round() as u8;
                }
            }
        }
    }

    /// By hand on macOS with Screen Recording allowed for the terminal: a frame of the main
    /// display comes back at the asked size (the privacy half is the native check N5).
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn captures_the_main_display() {
        let rgba = capture((0.0, 0.0), 320, 200).unwrap();
        assert_eq!(rgba.len(), 320 * 200 * 4);
    }

    /// By hand: with a 4 × 4 pt checker drawn at (300, 300) on the main display by a normal
    /// window, the capture finds it, so capture colours are sRGB like the page's. With the window
    /// private (`MOTETTO_MARKER=hidden`) it must not.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn finds_a_marker_on_screen() {
        let expect = std::env::var("MOTETTO_MARKER").unwrap_or_default();
        let (w, h, scale) = (
            std::env::var("MOTETTO_W").unwrap().parse::<u32>().unwrap(),
            std::env::var("MOTETTO_H").unwrap().parse::<u32>().unwrap(),
            std::env::var("MOTETTO_SCALE")
                .unwrap()
                .parse::<usize>()
                .unwrap(),
        );
        let rgba = capture((0.0, 0.0), w, h).unwrap();
        let rect = PxRect {
            x: 290 * scale,
            y: 290 * scale,
            w: 30 * scale,
            h: 30 * scale,
        };
        if std::env::var("MOTETTO_DUMP").is_ok() {
            for y in (rect.y..rect.y + rect.h).step_by(1) {
                for x in rect.x..rect.x + rect.w {
                    let i = (y * w as usize + x) * 4;
                    let p = &rgba[i..i + 3];
                    if p[0] > 200 && p[2] > 200 && p[1] < 120 || p[1] > 200 && p[0] < 120 {
                        eprintln!("{x},{y} {p:?}");
                    }
                }
            }
        }
        assert_eq!(find(&rgba, w as usize, rect, scale), expect != "hidden");
    }

    #[test]
    fn finds_marker_at_offset() {
        let mut rgba = image(200, 100);
        draw(&mut rgba, 200, 133, 41, 2, 1.0);
        let all = PxRect {
            x: 0,
            y: 0,
            w: 200,
            h: 100,
        };
        assert!(find(&rgba, 200, all, 2));
        // Only inside the prompter's rectangle.
        assert!(!find(
            &rgba,
            200,
            PxRect {
                x: 0,
                y: 0,
                w: 100,
                h: 100
            },
            2
        ));
        assert!(find(
            &rgba,
            200,
            PxRect {
                x: 130,
                y: 40,
                w: 20,
                h: 20
            },
            2
        ));
        // A rectangle reaching past the image edge is clipped, not a panic.
        assert!(find(
            &rgba,
            200,
            PxRect {
                x: 120,
                y: 30,
                w: 500,
                h: 500
            },
            2
        ));
        assert!(!find(&image(200, 100), 200, all, 2));
    }

    #[test]
    fn dithered_or_faded_marker_counts_but_not_a_trace() {
        let all = PxRect {
            x: 0,
            y: 0,
            w: 64,
            h: 64,
        };
        let mut faded = image(64, 64);
        draw(&mut faded, 64, 10, 10, 1, 0.85);
        assert!(
            find(&faded, 64, all, 1),
            "a faded window is still in the share"
        );
        let mut dithered = image(64, 64);
        draw(&mut dithered, 64, 10, 10, 1, 1.0);
        for (i, v) in dithered.iter_mut().enumerate() {
            if i % 4 != 3 && *v == 0 {
                *v = 15;
            }
        }
        assert!(find(&dithered, 64, all, 1), "measured dithering");
        let mut trace = image(64, 64);
        draw(&mut trace, 64, 10, 10, 1, 0.3);
        assert!(!find(&trace, 64, all, 1));
        // Two colours in the wrong places are not the checker either.
        let mut solid = image(64, 64);
        for i in 0..64 * 64 {
            solid[i * 4..i * 4 + 3].copy_from_slice(&MARKER[0]);
        }
        assert!(!find(
            &solid,
            64,
            PxRect {
                x: 0,
                y: 0,
                w: 64,
                h: 64
            },
            1
        ));
    }

    #[test]
    fn failed_capture_is_unknown_not_hidden() {
        assert_eq!(Check::from(Err("no permission".into())), Check::Unknown);
        assert_eq!(Check::from(Ok(false)), Check::Hidden);
        assert_eq!(Check::from(Ok(true)), Check::Visible);
    }

    #[test]
    fn cache_hit_skips_capture() {
        let mut cache = Cache::default();
        assert_eq!(cache.get("26.6", "1"), None);
        cache.put("26.6", "1", Check::Hidden);
        assert_eq!(cache.get("26.6", "1"), Some(Check::Hidden));
        // Another display or OS version asks again.
        assert_eq!(cache.get("26.6", "2"), None);
        assert_eq!(cache.get("26.7", "1"), None);
        // An unknown answer is never kept: permission may be granted meanwhile.
        cache.put("26.6", "2", Check::Unknown);
        assert_eq!(cache.get("26.6", "2"), None);
    }
}
