//! macOS call signals: which processes run audio input/output (Core Audio process objects,
//! macOS 14.2+) and which windows have which titles (CoreGraphics). All `unsafe` for call
//! detection lives here.

use crate::watch::AudioProcess;
use core_foundation::{
    array::CFArray,
    base::{CFType, TCFType},
    dictionary::CFDictionary,
    number::CFNumber,
    string::{CFString, CFStringRef},
};
use objc2::rc::autoreleasepool;
use objc2_app_kit::NSRunningApplication;
use std::{collections::HashMap, ffi::c_void, mem::size_of, ptr};

const fn fourcc(code: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*code)
}

const SYSTEM_OBJECT: u32 = 1; // kAudioObjectSystemObject
const SCOPE_GLOBAL: u32 = fourcc(b"glob");
const PROCESS_OBJECT_LIST: u32 = fourcc(b"prs#");
const PROCESS_PID: u32 = fourcc(b"ppid");
const PROCESS_BUNDLE_ID: u32 = fourcc(b"pbid");
const PROCESS_RUNNING_INPUT: u32 = fourcc(b"piri");
const PROCESS_RUNNING_OUTPUT: u32 = fourcc(b"piro");

const ON_SCREEN_ONLY: u32 = 1 << 0; // kCGWindowListOptionOnScreenOnly
const EXCLUDE_DESKTOP: u32 = 1 << 4; // kCGWindowListExcludeDesktopElements

#[repr(C)]
struct PropertyAddress {
    selector: u32,
    scope: u32,
    element: u32,
}

#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    fn AudioObjectGetPropertyDataSize(
        object: u32,
        address: *const PropertyAddress,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
    ) -> i32;
    fn AudioObjectGetPropertyData(
        object: u32,
        address: *const PropertyAddress,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
        data: *mut c_void,
    ) -> i32;
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> *const c_void;
    static kCGWindowOwnerPID: CFStringRef;
    static kCGWindowName: CFStringRef;
    static kCGWindowNumber: CFStringRef;
}

fn address(selector: u32) -> PropertyAddress {
    PropertyAddress {
        selector,
        scope: SCOPE_GLOBAL,
        element: 0,
    }
}

fn check(call: &str, selector: u32, status: i32) -> Result<(), String> {
    let code = String::from_utf8_lossy(&selector.to_be_bytes()).into_owned();
    match status {
        0 => Ok(()),
        _ => Err(format!("{call}('{code}') failed with OSStatus {status}")),
    }
}

/// A fixed-size property (`T` is a plain integer or a CF reference).
fn get<T: Copy + Default>(object: u32, selector: u32) -> Result<T, String> {
    let mut value = T::default();
    let mut size = size_of::<T>() as u32;
    // SAFETY: `value` is a live `T` and `size` is its exact byte size, so Core Audio writes at
    // most `size` bytes into it; the address struct outlives the call.
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            &address(selector),
            0,
            ptr::null(),
            &mut size,
            (&mut value as *mut T).cast(),
        )
    };
    check("AudioObjectGetPropertyData", selector, status)?;
    Ok(value)
}

fn process_objects() -> Result<Vec<u32>, String> {
    let addr = address(PROCESS_OBJECT_LIST);
    let mut size = 0u32;
    // SAFETY: the address and `size` outlive the call; no qualifier is passed.
    let status =
        unsafe { AudioObjectGetPropertyDataSize(SYSTEM_OBJECT, &addr, 0, ptr::null(), &mut size) };
    check(
        "AudioObjectGetPropertyDataSize",
        PROCESS_OBJECT_LIST,
        status,
    )?;
    let mut ids = vec![0u32; size as usize / size_of::<u32>()];
    let mut size = (ids.len() * size_of::<u32>()) as u32;
    if size == 0 {
        return Ok(ids);
    }
    // SAFETY: `ids` holds exactly `size` bytes of `u32`s; Core Audio writes at most `size`
    // bytes and reports how many it wrote.
    let status = unsafe {
        AudioObjectGetPropertyData(
            SYSTEM_OBJECT,
            &addr,
            0,
            ptr::null(),
            &mut size,
            ids.as_mut_ptr().cast(),
        )
    };
    check("AudioObjectGetPropertyData", PROCESS_OBJECT_LIST, status)?;
    // The list can shrink between the two calls.
    ids.truncate(size as usize / size_of::<u32>());
    Ok(ids)
}

fn bundle_id_of(object: u32) -> Result<String, String> {
    let raw: usize = get(object, PROCESS_BUNDLE_ID)?;
    if raw == 0 {
        return Ok(String::new());
    }
    // SAFETY: Core Audio returns a +1 `CFStringRef` (Copy rule); wrapping under the create
    // rule releases it when `s` drops.
    let s = unsafe { CFString::wrap_under_create_rule(raw as CFStringRef) };
    Ok(s.to_string())
}

fn app_bundle_id(pid: i32) -> Option<String> {
    // NSRunningApplication is documented thread-safe, unlike the rest of AppKit.
    NSRunningApplication::runningApplicationWithProcessIdentifier(pid)?
        .bundleIdentifier()
        .map(|s| s.to_string())
}

/// One process object, or `None` when it vanished between listing and reading.
fn process(object: u32) -> Option<AudioProcess> {
    let pid: i32 = get(object, PROCESS_PID).ok()?;
    let pid = u32::try_from(pid).ok().filter(|&p| p > 0)?;
    let input: u32 = get(object, PROCESS_RUNNING_INPUT).ok()?;
    let output: u32 = get(object, PROCESS_RUNNING_OUTPUT).ok()?;
    let mut bundle_id = bundle_id_of(object).ok()?;
    if bundle_id.is_empty() {
        bundle_id = app_bundle_id(pid as i32).unwrap_or_default();
    }
    Some(AudioProcess {
        pid,
        bundle_id,
        input: input != 0,
        output: output != 0,
    })
}

/// Every process Core Audio knows about, with whether it runs input (mic) or output now.
pub(crate) fn processes() -> Result<Vec<AudioProcess>, String> {
    // A process that exits mid-read fails its property reads (kAudioHardwareBadObjectError);
    // skipping it is the same as it not being listed.
    // The pool drains the autoreleased NSRunningApplication and NSString objects each poll.
    autoreleasepool(|_| Ok(process_objects()?.into_iter().filter_map(process).collect()))
}

/// (bundle id, title) for each on-screen window owned by one of `bundle_ids`. Titles need
/// Screen & System Audio Recording; without it they are absent and nothing is returned.
pub(crate) fn browser_titles(bundle_ids: &[String]) -> Result<Vec<(String, String)>, String> {
    autoreleasepool(|_| titles_in_pool(bundle_ids))
}

fn titles_in_pool(bundle_ids: &[String]) -> Result<Vec<(String, String)>, String> {
    // SAFETY: plain C call with valid option flags; the result is a +1 CFArrayRef or NULL.
    let raw = unsafe { CGWindowListCopyWindowInfo(ON_SCREEN_ONLY | EXCLUDE_DESKTOP, 0) };
    if raw.is_null() {
        return Err("CGWindowListCopyWindowInfo returned no window list".into());
    }
    // SAFETY: `raw` is a non-null CFArray of CFDictionary we own (Copy rule); the wrapper
    // releases it on drop.
    let windows: CFArray<CFDictionary<CFString, CFType>> =
        unsafe { CFArray::wrap_under_create_rule(raw.cast()) };
    // SAFETY: the key constants are immutable CFStrings that live for the whole process.
    let (pid_key, name_key) = unsafe {
        (
            CFString::wrap_under_get_rule(kCGWindowOwnerPID),
            CFString::wrap_under_get_rule(kCGWindowName),
        )
    };
    let mut owners: HashMap<i32, Option<String>> = HashMap::new();
    let mut titles = Vec::new();
    for window in windows.iter() {
        let Some(pid) = window
            .find(&pid_key)
            .and_then(|v| v.downcast::<CFNumber>())
            .and_then(|n| n.to_i32())
        else {
            continue;
        };
        let Some(title) = window
            .find(&name_key)
            .and_then(|v| v.downcast::<CFString>())
            .map(|s| s.to_string())
            .filter(|t| !t.is_empty())
        else {
            continue;
        };
        let owner = owners.entry(pid).or_insert_with(|| app_bundle_id(pid));
        if let Some(id) = owner
            .as_ref()
            .filter(|id| bundle_ids.iter().any(|b| b.eq_ignore_ascii_case(id)))
        {
            titles.push((id.clone(), title));
        }
    }
    Ok(titles)
}

/// Window ids of the on-screen windows, front to back (the Window Server's own order).
pub(crate) fn window_order() -> Result<Vec<u32>, String> {
    autoreleasepool(|_| {
        // SAFETY: plain C call with valid option flags; the result is a +1 CFArrayRef or NULL.
        let raw = unsafe { CGWindowListCopyWindowInfo(ON_SCREEN_ONLY | EXCLUDE_DESKTOP, 0) };
        if raw.is_null() {
            return Err("CGWindowListCopyWindowInfo returned no window list".into());
        }
        // SAFETY: `raw` is a non-null CFArray of CFDictionary we own (Copy rule); the wrapper
        // releases it on drop.
        let windows: CFArray<CFDictionary<CFString, CFType>> =
            unsafe { CFArray::wrap_under_create_rule(raw.cast()) };
        // SAFETY: the key constant is an immutable CFString that lives for the whole process.
        let number_key = unsafe { CFString::wrap_under_get_rule(kCGWindowNumber) };
        Ok(windows
            .iter()
            .filter_map(|w| {
                w.find(&number_key)
                    .and_then(|v| v.downcast::<CFNumber>())
                    .and_then(|n| n.to_i32())
                    .and_then(|n| u32::try_from(n).ok())
            })
            .collect())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selectors_are_big_endian_fourcc() {
        assert_eq!(SCOPE_GLOBAL, 0x676c_6f62);
        assert_eq!(PROCESS_OBJECT_LIST, 0x7072_7323);
    }

    #[test]
    #[ignore = "reads this Mac's audio processes; run by hand during a call"]
    fn lists_audio_processes_with_bundle_ids() {
        let list = processes().unwrap();
        assert!(list.iter().all(|p| p.pid > 0));
        eprintln!("{list:#?}");
    }
}
