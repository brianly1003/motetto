//! Screen recording rules: picture sizes, the call's window, screen changes.

use crate::watch::{AppRule, title_matches};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    Sharp,
    Smaller,
}

impl Preset {
    pub fn parse(value: &str) -> Self {
        if value == "smaller" {
            Self::Smaller
        } else {
            Self::Sharp
        }
    }

    pub fn bounds(self) -> (u32, u32) {
        match self {
            Self::Sharp => (1920, 1080),
            Self::Smaller => (1280, 720),
        }
    }

    pub fn fps(self) -> i32 {
        match self {
            Self::Sharp => 15,
            Self::Smaller => 10,
        }
    }

    pub fn video_bps(self) -> i32 {
        match self {
            Self::Sharp => 350_000,
            Self::Smaller => 200_000,
        }
    }
}

pub fn fit_size(width: f64, height: f64, preset: Preset) -> (u32, u32) {
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return (2, 2);
    }
    let (max_w, max_h) = preset.bounds();
    let scale = (f64::from(max_w) / width)
        .min(f64::from(max_h) / height)
        .min(1.0);
    (
        ((width * scale) as u32 & !1).max(2),
        ((height * scale) as u32 & !1).max(2),
    )
}

pub struct WindowInfo {
    pub id: u32,
    pub bundle_id: String,
    pub title: String,
    pub width: f64,
    pub height: f64,
    pub on_screen: bool,
    pub own: bool,
}

pub fn pick_window(windows: &[WindowInfo], rule: &AppRule, rules: &[AppRule]) -> Option<u32> {
    // Any browser window could be a bank or mail tab, not the call: only rules that name the call
    // (an app of its own, or a title like Meet's) offer a window; a browser call records a display.
    if rule.app_id == "browser" {
        return None;
    }
    let ids = if rule.mac_ids.is_empty() {
        &rules.iter().find(|r| r.app_id == "browser")?.mac_ids
    } else {
        &rule.mac_ids
    };
    windows
        .iter()
        .filter(|w| {
            w.on_screen
                && !w.own
                && w.width >= 200.0
                && w.height >= 150.0
                && w.width.is_finite()
                && w.height.is_finite()
                && ids.contains(&w.bundle_id)
                && (rule.title_patterns.is_empty()
                    || rule
                        .title_patterns
                        .iter()
                        .any(|p| title_matches(p, &w.title)))
        })
        .max_by(|a, b| (a.width * a.height).total_cmp(&(b.width * b.height)))
        .map(|w| w.id)
}

pub const GREY: usize = 640;

/// Average a BGRA frame into 32 by 20 cells, ignoring row padding.
pub fn shrink(bgra: &[u8], width: usize, height: usize, stride: usize) -> [u8; GREY] {
    let mut out = [0; GREY];
    if width == 0
        || height == 0
        || width.checked_mul(4).is_none_or(|n| stride < n)
        || stride.checked_mul(height).is_none_or(|n| bgra.len() < n)
    {
        return out;
    }
    for row in 0..20 {
        let y0 = row * height / 20;
        let y1 = ((row + 1) * height / 20).max(y0 + 1).min(height);
        for col in 0..32 {
            let x0 = col * width / 32;
            let x1 = ((col + 1) * width / 32).max(x0 + 1).min(width);
            let mut sum = 0_u64;
            for y in y0..y1 {
                for x in x0..x1 {
                    let p = &bgra[y * stride + x * 4..][..4];
                    sum += 299 * u64::from(p[2]) + 587 * u64::from(p[1]) + 114 * u64::from(p[0]);
                }
            }
            out[row * 32 + col] = (sum / (1000 * ((y1 - y0) * (x1 - x0)) as u64)) as u8;
        }
    }
    out
}

#[derive(Default)]
pub struct Moments {
    last: Option<(u64, [u8; GREY])>,
    accepted: u64,
}

impl Moments {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn offer(&mut self, ms: u64, grey: &[u8; GREY]) -> bool {
        if let Some((last_ms, last)) = &self.last {
            let diff: u64 = grey
                .iter()
                .zip(last)
                .map(|(a, b)| u64::from(a.abs_diff(*b)))
                .sum();
            if ms.saturating_sub(*last_ms) < 10_000
                || self.accepted >= 2 + ms / 60_000
                || diff * 100 <= 18 * 255 * GREY as u64
            {
                return false;
            }
        }
        self.last = Some((ms, *grey));
        self.accepted += 1;
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenTarget {
    Window(u32),
    Display(u32),
}

pub struct ScreenCapture {
    pub target: ScreenTarget,
    pub preset: Preset,
    pub cursor: bool,
    pub path: String,
    pub scale: f64,
    pub progress: Option<std::sync::mpsc::SyncSender<VideoOutcome>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoOutcome {
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub moments_ms: Vec<u64>,
    pub lost: bool,
    pub error: Option<String>,
}

pub struct Captured {
    pub files: Vec<String>,
    pub video: Option<VideoOutcome>,
}

/// Place an audio packet on the shared host clock without retaining a whole track in memory.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn audio_alignment(
    written: u32,
    elapsed: f64,
    rate: u32,
) -> Result<(usize, usize), &'static str> {
    if !elapsed.is_finite() || rate == 0 {
        return Err("invalid audio timestamp or rate");
    }
    let desired = (elapsed * f64::from(rate)).round() as i64;
    let written = i64::from(written);
    let gap = desired.saturating_sub(written).max(0) as usize;
    if gap > rate as usize * 60 {
        return Err("audio timestamp jumped by over a minute");
    }
    Ok((gap, written.saturating_sub(desired).max(0) as usize))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn late_audio_is_padded_and_early_or_overlapping_audio_is_trimmed() {
        for rate in [16000, 44100, 48000] {
            assert_eq!(audio_alignment(0, 0.1, rate), Ok((rate as usize / 10, 0)));
            assert_eq!(audio_alignment(rate / 10, 0.1, rate), Ok((0, 0)));
            assert_eq!(audio_alignment(0, -0.1, rate), Ok((0, rate as usize / 10)));
            assert!(audio_alignment(0, 61.0, rate).is_err());
        }
        assert!(audio_alignment(0, f64::NAN, 48000).is_err());
    }

    #[test]
    fn fit_size_fits_bounds_keeps_aspect_and_never_upscales() {
        assert_eq!(fit_size(3456.0, 2234.0, Preset::Sharp), (1670, 1080));
        assert_eq!(fit_size(3456.0, 2234.0, Preset::Smaller), (1112, 720));
        assert_eq!(fit_size(800.0, 601.0, Preset::Sharp), (800, 600));
        for (w, h) in [(1.0, 1.0), (0.0, 100.0), (f64::NAN, 100.0)] {
            assert_eq!(fit_size(w, h, Preset::Sharp), (2, 2));
        }
        assert_eq!(Preset::parse("other"), Preset::Sharp);
        assert_eq!(Preset::parse("smaller").video_bps(), 200_000);
    }

    fn window(id: u32, bundle: &str, title: &str, width: f64, height: f64) -> WindowInfo {
        WindowInfo {
            id,
            bundle_id: bundle.into(),
            title: title.into(),
            width,
            height,
            on_screen: true,
            own: false,
        }
    }

    #[test]
    fn pick_window_takes_the_largest_call_window() {
        let rules = crate::watch::rules();
        let zoom = rules.iter().find(|r| r.app_id == "zoom").unwrap();
        let mut windows = vec![
            window(1, "us.zoom.xos", "Zoom", 1400.0, 900.0),
            window(2, "us.zoom.xos", "Toolbar", 300.0, 60.0),
            window(3, "us.zoom.xos", "Own", 1600.0, 1000.0),
        ];
        windows[2].own = true;
        assert_eq!(pick_window(&windows, zoom, &rules), Some(1));
        windows[0].on_screen = false;
        assert_eq!(pick_window(&windows, zoom, &rules), None);
    }

    #[test]
    fn pick_window_uses_browser_ids_and_titles_for_meet() {
        let rules = crate::watch::rules();
        let meet = rules.iter().find(|r| r.app_id == "meet").unwrap();
        let windows = [
            window(5, "com.google.Chrome", "Meet - Weekly sync", 1200.0, 800.0),
            window(6, "com.google.Chrome", "Inbox", 1500.0, 900.0),
        ];
        assert_eq!(pick_window(&windows, meet, &rules), Some(5));
        let browser = rules.iter().find(|r| r.app_id == "browser").unwrap();
        assert_eq!(pick_window(&windows, browser, &rules), None);
    }

    #[test]
    fn shrink_averages_luma_per_cell() {
        let stride = 64 * 4 + 16;
        let mut frame = vec![0x80; stride * 40];
        for y in 0..40 {
            for x in 0..64 {
                frame[y * stride + x * 4..][..4].fill(if x < 32 { 255 } else { 0 });
            }
        }
        let out = shrink(&frame, 64, 40, stride);
        assert_eq!((out[0], out[15], out[16], out[31]), (255, 255, 0, 0));
        assert_eq!(shrink(&[255; 4], 1, 1, 4), [255; GREY]);
        assert_eq!(shrink(&[], 64, 40, stride), [0; GREY]);
    }

    #[test]
    fn moments_need_a_big_change_ten_seconds_apart_and_a_rate_cap() {
        let mut m = Moments::new();
        assert!(m.offer(0, &[0; GREY]));
        assert!(!m.offer(20_000, &[0; GREY]));
        assert!(!m.offer(5_000, &[255; GREY]));
        assert!(m.offer(12_000, &[255; GREY]));
        assert!(!m.offer(24_000, &[0; GREY]));
        assert!(m.offer(61_000, &[0; GREY]));
        assert!(!m.offer(60_000, &[255; GREY]));
    }
}
