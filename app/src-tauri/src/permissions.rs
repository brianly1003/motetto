//! Native permission queries; unsafe Objective-C calls stay behind this boundary.
use motetto_core::events::PermissionKind;
use serde::{Deserialize, Serialize};
use tauri::Manager;
use ts_rs::TS;

/// What the OS says, never a guess from a stream that opened (SPEC 4.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub enum Grant {
    Allowed,
    Denied,
    NotAsked,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Permissions {
    pub microphone: Grant,
    pub system_audio: Grant,
    pub notifications: Grant,
}

/// `AVAuthorizationStatus`: 0 not determined, 3 authorized; restricted and denied are Denied.
pub fn grant_from_status(status: isize) -> Grant {
    match status {
        0 => Grant::NotAsked,
        3 => Grant::Allowed,
        _ => Grant::Denied,
    }
}
/// macOS has no "denied" answer for screen capture; once asked in this install, not allowed is
/// Denied.
fn screen_grant(allowed: bool, requested: bool) -> Grant {
    match (allowed, requested) {
        (true, _) => Grant::Allowed,
        (false, true) => Grant::Denied,
        (false, false) => Grant::NotAsked,
    }
}
/// `UNAuthorizationStatus`: 0 not determined, 1 denied, 2-4 authorized, provisional, ephemeral.
fn notification_grant(status: isize) -> Grant {
    match status {
        0 => Grant::NotAsked,
        2..=4 => Grant::Allowed,
        _ => Grant::Denied,
    }
}

/// `screen_requested`: the Screen & System Audio prompt was shown once in this install.
pub fn read(screen_requested: bool) -> Result<Permissions, String> {
    Ok(Permissions {
        microphone: grant_from_status(microphone_status()?),
        system_audio: screen_grant(screen_allowed(), screen_requested),
        // Unreadable (a cargo binary outside Motetto.app) can't deliver notices: not allowed.
        notifications: notification_status(false).map_or(Grant::Denied, notification_grant),
    })
}

/// Shows the OS prompt for `kind` when it was never asked; the caller reads the result after.
pub fn request(kind: PermissionKind) -> Result<(), String> {
    match kind {
        PermissionKind::Microphone => request_microphone(),
        PermissionKind::SystemAudio => {
            request_screen();
            Ok(())
        }
        PermissionKind::Notifications => notification_status(true).map(|_| ()),
    }
}

/// `request` shows the macOS prompts for undecided access; `false` only queries.
pub fn audio(request: bool) -> Result<(bool, bool), String> {
    let status = microphone_status()?;
    if request && status == 0 {
        request_microphone_async()?;
    }
    let screen = screen_allowed();
    if request && !screen {
        request_screen();
    }
    // Prompting is not permission. A later user start/resume performs fresh checks.
    Ok((status == 3, screen))
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::{
        msg_send,
        runtime::{AnyClass, AnyObject, Bool},
    };
    #[link(name = "AVFoundation", kind = "framework")]
    unsafe extern "C" {
        static AVMediaTypeAudio: *const AnyObject;
    }
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGRequestScreenCaptureAccess() -> bool;
    }
    fn device() -> Result<&'static AnyClass, String> {
        AnyClass::get(c"AVCaptureDevice")
            .ok_or_else(|| "Microphone permission API is unavailable".into())
    }
    pub fn microphone_status() -> Result<isize, String> {
        let device = device()?;
        // SAFETY: AVCaptureDevice's class method and the AVMediaTypeAudio constant come from the
        // linked AVFoundation framework; NSInteger is isize.
        Ok(unsafe { msg_send![device, authorizationStatusForMediaType: AVMediaTypeAudio] })
    }
    /// Asks for the microphone; `done` gets the answer on a framework thread.
    pub fn request_microphone(done: impl Fn(bool) + 'static) -> Result<(), String> {
        let device = device()?;
        let callback = block2::RcBlock::new(move |granted: Bool| done(granted.as_bool()));
        // SAFETY: AVFoundation retains the completion block until it runs; BOOL is objc2::Bool.
        unsafe {
            let _: () = msg_send![device, requestAccessForMediaType: AVMediaTypeAudio, completionHandler: &*callback];
        }
        Ok(())
    }
    pub fn screen_allowed() -> bool {
        // SAFETY: a CoreGraphics query with no arguments.
        unsafe { CGPreflightScreenCaptureAccess() }
    }
    pub fn request_screen() {
        // SAFETY: CoreGraphics shows its own prompt once per install; no arguments.
        unsafe {
            CGRequestScreenCaptureAccess();
        }
    }
}

#[cfg(target_os = "macos")]
fn microphone_status() -> Result<isize, String> {
    mac::microphone_status()
}
#[cfg(target_os = "macos")]
fn screen_allowed() -> bool {
    mac::screen_allowed()
}
#[cfg(target_os = "macos")]
fn request_screen() {
    mac::request_screen()
}
#[cfg(target_os = "macos")]
fn request_microphone_async() -> Result<(), String> {
    mac::request_microphone(|_| {})
}
/// Waits for the answer (at most a minute) so the caller reads the new status.
#[cfg(target_os = "macos")]
fn request_microphone() -> Result<(), String> {
    if microphone_status()? != 0 {
        return Ok(());
    }
    let (tx, rx) = std::sync::mpsc::channel();
    mac::request_microphone(move |granted| {
        let _ = tx.send(granted);
    })?;
    let _ = rx.recv_timeout(std::time::Duration::from_secs(60));
    Ok(())
}
#[cfg(not(target_os = "macos"))]
fn microphone_status() -> Result<isize, String> {
    Err("Desktop recording permission checks are currently implemented for macOS only".into())
}
#[cfg(not(target_os = "macos"))]
fn screen_allowed() -> bool {
    false
}
#[cfg(not(target_os = "macos"))]
fn request_screen() {}
#[cfg(not(target_os = "macos"))]
fn request_microphone_async() -> Result<(), String> {
    Ok(())
}
#[cfg(not(target_os = "macos"))]
fn request_microphone() -> Result<(), String> {
    microphone_status().map(|_| ())
}

/// Whether notices can be shown; `request` shows the prompt when never asked.
pub fn notifications(request: bool) -> Result<bool, String> {
    notification_status(request).map(|status| notification_grant(status) == Grant::Allowed)
}

/// The plugin's desktop permission method always returns Granted; query macOS itself. A request
/// answers 2 (allowed) or 1 (not allowed), a query the `UNAuthorizationStatus`.
#[cfg(target_os = "macos")]
fn notification_status(request: bool) -> Result<isize, String> {
    use objc2::{
        msg_send,
        runtime::{AnyClass, AnyObject, Bool},
    };
    use objc2_foundation::NSBundle;
    use std::{sync::mpsc, time::Duration};
    #[link(name = "UserNotifications", kind = "framework")]
    unsafe extern "C" {}
    // UNUserNotificationCenter requires an application bundle, not a raw cargo binary.
    if !NSBundle::mainBundle()
        .bundlePath()
        .to_string()
        .ends_with(".app")
    {
        return Err("Notification permission requires the packaged Motetto.app. Recording controls remain visible in development.".into());
    }
    let class =
        AnyClass::get(c"UNUserNotificationCenter").ok_or("Notifications are unavailable")?;
    let (tx, rx) = mpsc::channel();
    // SAFETY: UserNotifications owns the center and retains the completion blocks. Callback
    // objects are used only for the callback's duration; enum/options values are NSUInteger.
    unsafe {
        let center: *mut AnyObject = msg_send![class, currentNotificationCenter];
        if request {
            let callback = block2::RcBlock::new(move |granted: Bool, error: *mut AnyObject| {
                let _ = tx.send(if error.is_null() { Ok(if granted.as_bool() { 2 } else { 1 }) } else { Err("macOS couldn't authorize notifications. Check System Settings > Notifications > Motetto.".to_string()) });
            });
            let _: () = msg_send![center, requestAuthorizationWithOptions: 6_usize, completionHandler: &*callback];
        } else {
            let callback = block2::RcBlock::new(move |settings: *mut AnyObject| {
                let status: isize = msg_send![settings, authorizationStatus];
                let _ = tx.send(Ok(status));
            });
            let _: () = msg_send![center, getNotificationSettingsWithCompletionHandler: &*callback];
        }
    }
    let result = rx
        .recv_timeout(Duration::from_secs(if request { 60 } else { 3 }))
        .map_err(|_| "Notification permission check timed out. Retry in Motetto.".to_string())?;
    // An ad-hoc signed bundle can fail the request even when System Settings allows Motetto,
    // so trust the current setting before reporting the error.
    match result {
        Err(error) if request => match notification_status(false) {
            Ok(status) if notification_grant(status) == Grant::Allowed => Ok(status),
            _ => Err(error),
        },
        result => result,
    }
}
#[cfg(not(target_os = "macos"))]
fn notification_status(_request: bool) -> Result<isize, String> {
    Ok(0)
}

/// SPEC 4.3: the OS pane where the user turns `kind` back on.
fn pane(kind: PermissionKind) -> &'static str {
    use PermissionKind::*;
    if cfg!(windows) {
        match kind {
            Microphone => "ms-settings:privacy-microphone",
            Notifications => "ms-settings:notifications",
            SystemAudio => "ms-settings:privacy", // Windows never blocks loopback capture.
        }
    } else {
        match kind {
            Microphone => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
            }
            SystemAudio => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
            }
            Notifications => "x-apple.systempreferences:com.apple.Notifications-Settings.extension",
        }
    }
}

#[tauri::command]
pub fn open_permission_settings(kind: PermissionKind) -> Result<(), String> {
    open_pane(pane(kind))
}

fn open_pane(pane: &str) -> Result<(), String> {
    let opener = if cfg!(windows) { "explorer" } else { "open" };
    std::process::Command::new(opener)
        .arg(pane)
        .status()
        .map(|_| ())
        .map_err(|e| format!("Couldn't open settings: {e}"))
}

/// Speech Recognition, for script following only (SPEC 14.5): never needed to record, so it is
/// not a recording permission. Its own OS pane.
fn speech_pane() -> &'static str {
    if cfg!(windows) {
        "ms-settings:privacy-speech"
    } else {
        "x-apple.systempreferences:com.apple.preference.security?Privacy_SpeechRecognition"
    }
}

/// Settings › Privacy: whether the strip may follow with Apple's on-device recogniser, as macOS
/// says now. Windows has no such permission (it follows with Whisper).
#[tauri::command]
pub fn speech_permission() -> Grant {
    #[cfg(target_os = "macos")]
    return match crate::follow_macos::access() {
        crate::follow_macos::Access::Allowed => Grant::Allowed,
        crate::follow_macos::Access::Denied => Grant::Denied,
        crate::follow_macos::Access::Unknown => Grant::NotAsked,
    };
    #[cfg(not(target_os = "macos"))]
    Grant::NotAsked
}

#[tauri::command]
pub fn open_speech_settings() -> Result<(), String> {
    open_pane(speech_pane())
}

#[tauri::command]
pub async fn permissions(app: tauri::AppHandle) -> Result<Permissions, String> {
    tauri::async_runtime::spawn_blocking(move || {
        read(
            app.state::<crate::settings::Store>()
                .get()?
                .screen_requested,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
/// Shows the OS prompt when it was never asked and returns what the OS says afterwards.
#[tauri::command]
pub async fn request_permission(
    app: tauri::AppHandle,
    kind: PermissionKind,
) -> Result<Grant, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<crate::settings::Store>();
        if kind == PermissionKind::SystemAudio {
            store.change(|s| s.screen_requested = true)?;
        }
        request(kind)?;
        let now = read(store.get()?.screen_requested)?;
        Ok(match kind {
            PermissionKind::Microphone => now.microphone,
            PermissionKind::SystemAudio => now.system_audio,
            PermissionKind::Notifications => now.notifications,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grant_from_status() {
        assert_eq!(super::grant_from_status(0), Grant::NotAsked);
        assert_eq!(super::grant_from_status(2), Grant::Denied);
        assert_eq!(super::grant_from_status(1), Grant::Denied);
        assert_eq!(super::grant_from_status(3), Grant::Allowed);
    }
    #[test]
    fn screen_and_notifications_are_tri_state() {
        assert_eq!(screen_grant(true, false), Grant::Allowed);
        assert_eq!(screen_grant(false, false), Grant::NotAsked);
        assert_eq!(screen_grant(false, true), Grant::Denied);
        assert_eq!(notification_grant(0), Grant::NotAsked);
        assert_eq!(notification_grant(1), Grant::Denied);
        assert!(
            [2, 3, 4]
                .map(notification_grant)
                .iter()
                .all(|g| *g == Grant::Allowed)
        );
        Permissions::export_all(&ts_rs::Config::default().with_out_dir("../../ui/lib/bindings"))
            .unwrap();
    }
    #[test]
    fn each_permission_opens_its_own_pane() {
        let panes = [
            PermissionKind::Microphone,
            PermissionKind::SystemAudio,
            PermissionKind::Notifications,
        ]
        .map(super::pane);
        assert!(panes[0] != panes[1] && panes[1] != panes[2] && panes[0] != panes[2]);
        // Speech Recognition (script following) has its own pane too, never a recording one.
        assert!(!panes.contains(&super::speech_pane()));
        assert!(panes.iter().all(|p| p.starts_with(if cfg!(windows) {
            "ms-settings:"
        } else {
            "x-apple.systempreferences:"
        })));
    }
}
