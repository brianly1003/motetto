//! Local meeting pipeline shared by the CLI and desktop app.

#[cfg(target_os = "macos")]
pub mod capture;
mod platform;
#[cfg(target_os = "windows")]
pub use platform::windows as capture;
pub mod aec;
pub mod config;
pub mod correct;
mod decode;
pub use decode::to_wav as decode_audio;
mod diarize;
pub mod events;
pub mod mics;
pub mod ollama;
pub mod progress;
pub mod session;
pub mod slice;
pub mod summarize;
pub mod transcribe;
pub mod translate;
pub mod whisper;

pub mod tools;
pub mod watch;

pub mod echo;

pub mod video;
