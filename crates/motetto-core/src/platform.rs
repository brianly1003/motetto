//! Platform capture backends and their shared packet-to-WAV rules.

#[cfg(any(target_os = "windows", test))]
mod audio;
#[cfg(target_os = "macos")]
pub(crate) mod macos_calls;
#[cfg(target_os = "macos")]
pub(crate) mod macos_mics;
#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "macos")]
pub(crate) mod macos_video;
