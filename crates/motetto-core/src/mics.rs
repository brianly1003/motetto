//! Microphones for the desktop picker: which inputs exist, which one Motetto records, and whether it
//! clips (owner 2026-10-23, DESIGN 4.15). Motetto shows each mic's facts (kind, rate) and lets the
//! user choose; it never judges a mic's quality, since it can't rank mics reliably. Clipping is
//! measured, so it is the one problem Motetto names.

use serde::{Deserialize, Serialize};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU32, AtomicU64},
    mpsc::SyncSender,
};
use ts_rs::TS;

/// Where the mic's samples also go as they are captured (the script strip's recogniser): the
/// recorded channel as f32 with its sample rate. Never waited on; a full or gone listener drops.
/// Each listener has its own number, so one that ends never clears the next one's.
pub type Listener = Arc<Mutex<Option<(u64, SyncSender<(Vec<f32>, u32)>)>>>;

/// Starts listening with `tx`, replacing any listener; returns its number for `unlisten`.
pub fn listen(listener: &Listener, tx: SyncSender<(Vec<f32>, u32)>) -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    *listener.lock().unwrap_or_else(|e| e.into_inner()) = Some((id, tx));
    id
}

/// Stops listener `id`, only if it is still the one listening.
pub fn unlisten(listener: &Listener, id: u64) {
    let mut slot = listener.lock().unwrap_or_else(|e| e.into_inner());
    if slot.as_ref().is_some_and(|(current, _)| *current == id) {
        *slot = None;
    }
}

/// What the desktop watches while recording: each track's level (mic, system audio) as f32 bits,
/// how many mic samples clipped since it last looked, who listens to the mic live, and how many
/// mic buffers have arrived (`MicStall`). The CLI watches nothing.
#[derive(Clone, Default)]
pub struct Meters {
    pub levels: [Arc<AtomicU32>; 2],
    pub mic_clips: Arc<AtomicU32>,
    pub mic_listener: Listener,
    pub mic_buffers: Arc<AtomicU64>,
}

/// The mic a part would record now: `chosen` while connected, else the system default (None
/// when there is no input at all).
pub fn wanted(chosen: Option<&str>, mics: &[Mic]) -> Option<String> {
    chosen
        .filter(|id| mics.iter().any(|m| m.id == *id))
        .or_else(|| mics.iter().find(|m| m.is_default).map(|m| m.id.as_str()))
        .map(str::to_string)
}

/// How long a new answer from `wanted` must hold before the part moves to it: plugging a headset
/// in can change the default input more than once.
pub const MIC_SETTLE_MS: u64 = 1000;

/// Follows the mic a recording should use, as call apps do: unplugging the recording's mic moves
/// it to the system default, and plugging the chosen one (or a new default) back moves it there.
pub struct MicFollow {
    opened: Option<String>,
    changed: Option<(Option<String>, u64)>,
}

impl MicFollow {
    /// `opened`: the mic this part records, as `wanted` gave it when the part started.
    pub fn new(opened: Option<String>) -> Self {
        Self {
            opened,
            changed: None,
        }
    }

    /// True when `wanted` has differed from the opened mic, the same way, for `MIC_SETTLE_MS`.
    pub fn moved(&mut self, wanted: Option<String>, now_ms: u64) -> bool {
        if wanted == self.opened {
            self.changed = None;
            return false;
        }
        match &self.changed {
            Some((pending, since)) if *pending == wanted => {
                now_ms.saturating_sub(*since) >= MIC_SETTLE_MS
            }
            _ => {
                self.changed = Some((wanted, now_ms));
                false
            }
        }
    }
}

/// How long a mic that was sending may go without a buffer before it counts as stopped. A
/// capturing mic sends a buffer every ~10-20 ms, silence included.
pub const MIC_STALL_MS: u64 = 5000;

/// Notices a mic that stops sending mid-recording: ScreenCaptureKit can stop delivering it
/// (after an audio device change) with no error while app sounds go on.
pub struct MicStall {
    count: u64,
    since_ms: u64,
    heard: bool,
}

impl MicStall {
    /// Starts from the current buffer count; it must move once before a stop counts.
    pub fn new(count: u64, now_ms: u64) -> Self {
        Self {
            count,
            since_ms: now_ms,
            heard: false,
        }
    }

    /// True when the count has not moved for `MIC_STALL_MS` after it moved at least once.
    pub fn stalled(&mut self, count: u64, now_ms: u64) -> bool {
        if count != self.count {
            (self.count, self.since_ms, self.heard) = (count, now_ms, true);
            return false;
        }
        self.heard && now_ms.saturating_sub(self.since_ms) >= MIC_STALL_MS
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub enum Transport {
    BuiltIn,
    Bluetooth,
    Usb,
    /// Call apps' own devices (Teams Audio, ZoomAudioDevice) and aggregates: never offered.
    Virtual,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Mic {
    /// The device UID; ScreenCaptureKit records the microphone with this id.
    pub id: String,
    pub name: String,
    pub transport: Transport,
    #[ts(type = "number")]
    pub rate_hz: u32,
    /// Input volume 0.0-1.0, or None when the device sets its own level.
    pub volume: Option<f32>,
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub enum MicIssue {
    TooLoud,
}

/// What the picker shows: the mics to offer, the one Motetto records and its problem. Motetto never
/// picks a replacement: the user chooses from the list (owner 2026-10-23).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct MicReport {
    pub mics: Vec<Mic>,
    /// The user's choice; None follows the system default.
    pub chosen: Option<String>,
    /// The mic a recording uses now: the chosen one if connected, else the system default.
    pub active: Option<Mic>,
    pub issue: Option<MicIssue>,
}

/// Builds the report from Core Audio's list, the saved choice and the recent clipping.
pub fn report(all: Vec<Mic>, chosen: Option<String>, too_loud: bool) -> MicReport {
    let mics: Vec<Mic> = all
        .into_iter()
        .filter(|m| m.transport != Transport::Virtual)
        .collect();
    let active = chosen
        .as_deref()
        .and_then(|id| mics.iter().find(|m| m.id == id))
        .or_else(|| mics.iter().find(|m| m.is_default))
        .cloned();
    let issue = (active.is_some() && too_loud).then_some(MicIssue::TooLoud);
    MicReport {
        mics,
        chosen,
        active,
        issue,
    }
}

/// A sample at or past this is clipped: float mics stop at 1.0, 16-bit ones at 32767/32768.
pub const CLIP: f32 = 0.99;

/// Too loud when clipping showed up in 3 of the last 5 checks (2 s apart): one loud laugh or a
/// knock is not a reason to interrupt anyone.
pub fn too_loud(recent: &[bool]) -> bool {
    recent
        .iter()
        .rev()
        .take(5)
        .filter(|&&clipped| clipped)
        .count()
        >= 3
}

/// The volume "Lower input volume" sets: a third less, never under a quarter.
pub fn quieter(volume: f32) -> f32 {
    (volume * 0.67).max(0.25)
}

/// Every input device; empty where Motetto can't list them (Windows records the default mic).
pub fn list() -> Result<Vec<Mic>, String> {
    #[cfg(target_os = "macos")]
    return crate::platform::macos_mics::list();
    #[cfg(not(target_os = "macos"))]
    Ok(vec![])
}

/// Sets a mic's input volume (0.0-1.0), the same setting as the system's Sound settings.
pub fn set_volume(id: &str, volume: f32) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return crate::platform::macos_mics::set_volume(id, volume);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (id, volume);
        Err("Choose the input volume in Windows sound settings.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recording_follows_an_unplugged_and_returning_mic() {
        let built_in = || mic("built-in", Transport::BuiltIn, 48_000, false);
        let earpods = || mic("earpods", Transport::Usb, 44_100, true);
        // Plugged in, chosen or default: EarPods. Unplugged: the default, even if chosen.
        assert_eq!(
            wanted(None, &[built_in(), earpods()]).as_deref(),
            Some("earpods")
        );
        let unplugged = [mic("built-in", Transport::BuiltIn, 48_000, true)];
        assert_eq!(
            wanted(Some("earpods"), &unplugged).as_deref(),
            Some("built-in")
        );
        assert_eq!(wanted(Some("gone"), &[]), None);
        // A chosen mic wins over the default while connected.
        assert_eq!(
            wanted(Some("built-in"), &[built_in(), earpods()]).as_deref(),
            Some("built-in")
        );

        let mut follow = MicFollow::new(Some("earpods".into()));
        assert!(!follow.moved(Some("earpods".into()), 0));
        // Unplugged: moves once the answer has held for MIC_SETTLE_MS.
        assert!(!follow.moved(Some("built-in".into()), 1000));
        assert!(!follow.moved(Some("built-in".into()), 1000 + MIC_SETTLE_MS - 1));
        assert!(follow.moved(Some("built-in".into()), 1000 + MIC_SETTLE_MS));
        // A flicker back to the opened mic, or to a third answer, starts the wait again.
        let mut follow = MicFollow::new(Some("built-in".into()));
        assert!(!follow.moved(Some("earpods".into()), 0));
        assert!(!follow.moved(Some("built-in".into()), 500));
        assert!(!follow.moved(Some("earpods".into()), 900));
        assert!(!follow.moved(Some("usb".into()), 1500));
        assert!(!follow.moved(Some("usb".into()), 2400));
        assert!(follow.moved(Some("usb".into()), 2500));
    }

    #[test]
    fn a_mic_that_stops_sending_is_noticed_once_it_had_sent() {
        // Not yet heard: a mic still starting never counts as stopped.
        let mut stall = MicStall::new(40, 0);
        assert!(!stall.stalled(40, MIC_STALL_MS * 3));
        assert!(!stall.stalled(41, MIC_STALL_MS * 3));
        // Buffers keep coming: fine, however long.
        assert!(!stall.stalled(500, MIC_STALL_MS * 10));
        assert!(!stall.stalled(500, MIC_STALL_MS * 11 - 1));
        assert!(stall.stalled(500, MIC_STALL_MS * 11));
        // It comes back: the clock starts again.
        assert!(!stall.stalled(501, MIC_STALL_MS * 12));
        assert!(!stall.stalled(501, MIC_STALL_MS * 13 - 1));
    }

    fn mic(id: &str, transport: Transport, rate_hz: u32, is_default: bool) -> Mic {
        Mic {
            id: id.into(),
            name: id.into(),
            transport,
            rate_hz,
            volume: None,
            is_default,
        }
    }

    fn all() -> Vec<Mic> {
        vec![
            mic("shokz", Transport::Bluetooth, 16_000, true),
            mic("teams", Transport::Virtual, 48_000, false),
            mic("usb", Transport::Usb, 48_000, false),
            mic("builtin", Transport::BuiltIn, 48_000, false),
        ]
    }

    #[test]
    fn the_default_is_active_and_a_bluetooth_mic_is_no_problem() {
        let r = report(all(), None, false);
        assert_eq!(r.active.unwrap().id, "shokz");
        assert_eq!(r.issue, None);
        assert!(r.mics.iter().all(|m| m.id != "teams"));
    }

    #[test]
    fn the_choice_wins_unless_it_is_unplugged() {
        let r = report(all(), Some("usb".into()), true);
        assert_eq!(r.active.unwrap().id, "usb");
        assert_eq!(r.issue, Some(MicIssue::TooLoud));
        let r = report(all(), Some("gone".into()), false);
        assert_eq!(r.active.unwrap().id, "shokz");
        assert_eq!(r.chosen.as_deref(), Some("gone"));
    }

    #[test]
    fn clipping_must_last_before_it_counts_as_too_loud() {
        assert!(!too_loud(&[true, true, false, false, false]));
        assert!(too_loud(&[true, false, true, false, true]));
        assert!(!too_loud(&[
            true, true, true, false, false, false, false, false
        ]));
        assert!(!too_loud(&[]));
        assert_eq!(quieter(0.9), 0.9 * 0.67);
        assert_eq!(quieter(0.3), 0.25);
    }

    #[test]
    fn export_mic_contract() {
        MicReport::export_all(&ts_rs::Config::default().with_out_dir("../../ui/lib/bindings"))
            .unwrap();
    }
}
