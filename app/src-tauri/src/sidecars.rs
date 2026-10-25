use std::path::PathBuf;
use tauri::{AppHandle, Manager, path::BaseDirectory};

/// The bundled speed-test recording.
pub fn sample_path(app: &AppHandle) -> PathBuf {
    app.path()
        .resolve("resources/speed-sample.wav", BaseDirectory::Resource)
        .unwrap_or_default()
}
