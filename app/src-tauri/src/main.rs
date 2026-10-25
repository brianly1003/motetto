//! Desktop library shell over the local pipeline. No recording starts from app launch.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod bookmarks;
mod calls;
mod catalog;
mod companion;
mod controller;
mod desktop;
mod download;
mod library;
mod library_commands;
mod live;
mod meeting_actions;
mod meeting_media;
mod microphone;
mod model_store;
mod my_notes;
mod note_items;
mod notes_engine;
mod notifications;
mod permissions;
mod recording;
mod rewrite;
mod settings;
mod settings_models;
mod settings_storage;
mod settings_system;
mod shortcuts;
mod sidecars;
mod speech;
mod transcript_edits;
mod tray;
mod window_native;

fn main() {
    motetto_core::tools::configure_desktop();
    // The bundled programs sit next to the executable (Contents/MacOS on macOS).
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
    {
        motetto_core::tools::configure_bundled(dir);
    }
    desktop::run();
}
