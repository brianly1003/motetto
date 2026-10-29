//! Microphones for the desktop picker: which inputs exist, which one Motetto records, and whether it
//! clips (owner 2026-10-23, DESIGN 4.15). Motetto shows each mic's facts (kind, rate) and lets the
//! user choose; it never judges a mic's quality, since it can't rank mics reliably. Clipping is
//! measured, so it is the one problem Motetto names.

use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, atomic::AtomicU32, mpsc::SyncSender};
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
/// how many mic samples clipped since it last looked, and who listens to the mic live. The CLI
/// watches nothing.
#[derive(Clone, Default)]
pub struct Meters {
    pub levels: [Arc<AtomicU32>; 2],
    pub mic_clips: Arc<AtomicU32>,
    pub mic_listener: Listener,
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
