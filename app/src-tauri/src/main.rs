//! Desktop library shell over the local pipeline. No recording starts from app launch.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod bookmarks;
mod calls;
mod companion;
mod controller;
mod desktop;
mod library;
mod library_commands;
mod live;
mod meeting_actions;
mod meeting_media;
mod microphone;
mod my_notes;
mod note_items;
mod notifications;
mod permissions;
mod recording;
mod rewrite;
mod settings;
mod settings_models;
mod settings_storage;
mod settings_system;
mod shortcuts;
mod speech;
mod transcript_edits;
mod translations;
mod tray;
mod window_native;

fn main() {
    motetto_core::tools::configure_desktop();
    desktop::run();
}
