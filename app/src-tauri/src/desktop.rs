//! Tauri commands, windows and native menu adapter.
use crate::library_commands::{self, Library};
use crate::{
    controller::{Action, Controller, Host, Job},
    library, permissions, recording,
};
use motetto_core::events::{AppState, Event, Mode, Readiness, SessionState};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use tauri::{Emitter, Manager};

pub(crate) fn show(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            library_commands::list_meetings,
            library_commands::library_bytes,
            crate::meeting_media::meeting_media,
            crate::note_items::set_action_item_done,
            crate::note_items::save_notes,
            crate::transcript_edits::save_transcript_line,
            crate::meeting_actions::meeting_action,
            crate::meeting_actions::deleted_meetings,
            crate::meeting_actions::rename_meeting,
            crate::meeting_actions::set_locked,
            library_commands::search_meetings,
            library_commands::open_action_items,
            library_commands::get_meeting,
            library_commands::import_audio,
            library_commands::importing,
            library_commands::cancel_import,
            library_commands::stop_processing,
            get_state,
            crate::my_notes::get_my_notes,
            crate::my_notes::save_my_notes,
            crate::my_notes::add_jot,
            mark_moment,
            start_recording,
            stop_recording,
            pause_recording,
            keep_recording,
            discard_recording,
            undo_discard,
            resume_recording,
            quit,
            dismiss_problem,
            set_companion_size,
            take_note,
            companion_ready,
            crate::companion::companion_menu,
            crate::companion::pin_companion,
            crate::companion::snap_companion,
            crate::companion::sharing_mode,
            crate::live::get_levels,
            crate::microphone::microphones,
            crate::microphone::set_microphone,
            crate::microphone::set_mic_volume,
            crate::microphone::open_sound_settings,
            crate::permissions::open_permission_settings,
            crate::live::live_transcript,
            resume_remaining_ms,
            notification_permission,
            answer_ask,
            dont_ask_for_app,
            undo_dont_ask,
            hold_ask,
            set_ask_on_call,
            crate::settings::open_settings,
            crate::settings::get_settings,
            crate::settings::save_settings,
            crate::settings::set_app_mode,
            crate::settings::call_apps,
            crate::settings::recent_mic_apps,
            crate::settings::add_app,
            crate::settings_models::installed_models,
            crate::settings_models::pick_speech_model,
            crate::settings_models::run_speed_test,
            crate::settings_storage::storage_summary,
            crate::settings_storage::delete_all_meetings,
            crate::settings_storage::move_notes_folder,
            crate::settings_system::about,
            crate::settings_system::open_logs_folder
        ])
        .setup(|app| {
            let config = app.path().app_config_dir()?;
            let mut settings = crate::settings::Settings::read(&config)?;
            if settings.notes_folder.is_empty() {
                settings.notes_folder = app
                    .path()
                    .document_dir()?
                    .join("Motetto")
                    .join("imports")
                    .to_string_lossy()
                    .into_owned();
            }
            let root = PathBuf::from(&settings.notes_folder);
            if !config.join("settings.json").exists()
                && std::fs::read_dir(&root).is_ok_and(|mut entries| entries.next().is_some())
            {
                settings.keep_audio = "forever".into();
            }
            settings.save(&config)?;
            motetto_core::config::configure_desktop(config.join("settings.json"));
            app.manage(crate::settings::Store {
                dir: config.clone(),
                value: std::sync::Mutex::new(settings),
                recent: std::sync::Mutex::new(
                    std::fs::read(config.join("recent-mic-apps.json"))
                        .ok()
                        .and_then(|text| serde_json::from_slice(&text).ok())
                        .unwrap_or_default(),
                ),
            });
            std::fs::create_dir_all(&root)?;
            if let Err(error) = crate::settings_storage::retain(
                &root,
                &app.state::<crate::settings::Store>().get()?.keep_audio,
            ) {
                eprintln!("Motetto couldn't apply audio retention: {error}");
            }
            // Nothing records or imports yet at launch, so any busy meeting was cut off by a quit or crash.
            if let Err(e) = library::recover_interrupted(&root) {
                eprintln!("Motetto: couldn't check for interrupted meetings: {e}");
            }
            app.manage(crate::my_notes::Store::new(root.clone()));
            app.manage(crate::live::State::default());
            app.manage(crate::microphone::Clipping::default());
            let busy = Arc::new(AtomicBool::new(false));
            let allow_exit = Arc::new(AtomicBool::new(false));
            let (sender, receiver) = mpsc::channel();
            let jobs = recording::spawn(
                root.clone(),
                app.handle().clone(),
                sender.clone(),
                allow_exit.clone(),
            )?;
            let host = Desktop {
                app: app.handle().clone(),
                root: root.clone(),
                jobs,
            };
            app.manage(Controller::spawn(
                host,
                sender.clone(),
                receiver,
                busy.clone(),
            )?);
            app.manage(Library {
                root,
                busy,
                importing: AtomicBool::new(false),
                allow_exit,
                quit_pending: AtomicBool::new(false),
                job: std::sync::Mutex::new(None),
                edits: std::sync::Mutex::new(()),
            });
            crate::companion::create(app.handle())?;
            if let Some(window) = app.get_webview_window("main") {
                crate::window_native::place_main(&window)?;
            }
            crate::notifications::setup(app.handle())?;
            crate::shortcuts::install(app.handle())?;
            // After the companion exists: a detected call shows its Ask card there.
            crate::calls::spawn(app.handle().clone(), sender)?;
            app.manage(crate::tray::State::default());
            let menu = crate::tray::initial(app.handle())?;
            tauri::tray::TrayIconBuilder::with_id("motetto")
                .icon(crate::tray::icon(false))
                .icon_as_template(true)
                .tooltip("Motetto · On this computer")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => show(app),
                    "record" => dispatch(app.clone(), Action::Start),
                    "resume" => dispatch(app.clone(), Action::Resume),
                    "pause" => dispatch(app.clone(), Action::Pause),
                    "stop" => dispatch(app.clone(), Action::Stop),
                    "mark" => {
                        let app = app.clone();
                        tauri::async_runtime::spawn_blocking(move || {
                            if let Err(error) = app.state::<Controller>().mark() {
                                let _ = app.emit("motetto://notice", error);
                            }
                        });
                    }
                    "notes" | "jot" => {
                        crate::shortcuts::open_notes(app, event.id().as_ref() == "jot")
                    }
                    "settings" => {
                        if let Err(error) = crate::settings::open(app, "general") {
                            let _ = app.emit("motetto://notice", error);
                        }
                    }
                    "quit" => app.exit(0),
                    "ask-record" | "ask-no" => {
                        // The menu is rebuilt per state, so the call id is read when clicked.
                        if let Ok(AppState {
                            state: SessionState::Asking { call_id, .. },
                            ..
                        }) = app.state::<Controller>().state()
                        {
                            let record = event.id.as_ref() == "ask-record";
                            dispatch(app.clone(), Action::AnswerAsk { call_id, record });
                        }
                    }
                    "ask-on-call" => {
                        if let Ok(state) = app.state::<Controller>().state() {
                            dispatch(app.clone(), Action::AskOnCall(!state.ask_on_call));
                        }
                    }
                    id if id.starts_with("ask-again:") => dispatch(
                        app.clone(),
                        Action::AppMode {
                            app_id: id["ask-again:".len()..].to_string(),
                            mode: Mode::Ask,
                        },
                    ),
                    _ => {}
                })
                .build(app)?;
            if std::env::args().any(|arg| arg == "--background")
                && let Some(window) = app.get_webview_window("main")
            {
                window.hide()?;
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::Moved(_)) {
                crate::companion::keep_tucked_on_screen(window);
            }
            if let tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) = event
                && window.label() == "main"
                && let Some(path) = paths.first().cloned()
            {
                let app = window.app_handle().clone();
                tauri::async_runtime::spawn(async move {
                    match library_commands::import_file(app.clone(), Some(path)).await {
                        Ok(Some(meeting)) => {
                            let _ = app.emit("motetto://open-meeting", meeting.id);
                        }
                        Err(e) => {
                            let _ = app.emit("motetto://import-failed", e);
                        }
                        Ok(None) => {}
                    }
                });
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let active = window
                    .app_handle()
                    .state::<Controller>()
                    .state()
                    .is_ok_and(|s| {
                        matches!(
                            s.state,
                            SessionState::Recording | SessionState::Stopped { .. }
                        )
                    });
                if window.label() == "settings"
                    || !active
                    || (window.label() == "main"
                        && crate::companion::available(window.app_handle()))
                {
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("could not build Motetto")
        .run(|app, event| {
            // The visible companion stops macOS restoring the library on a Dock click; do it here.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                show(app);
            }
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                let state = app.state::<Library>();
                if !state.allow_exit.load(Ordering::SeqCst) {
                    api.prevent_exit();
                    if state.importing.load(Ordering::SeqCst) {
                        state.quit_pending.store(true, Ordering::SeqCst);
                        show(app);
                        let _ = app.emit("motetto://quit-pending", ());
                    } else {
                        dispatch(app.clone(), Action::RequestQuit);
                    }
                } else {
                    crate::speech::shutdown();
                }
            }
        });
}

pub(crate) fn dispatch(app: tauri::AppHandle, action: Action) {
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(e) = app.state::<Controller>().action(action) {
            let _ = app.emit("motetto://notice", e);
        }
    });
}
async fn act(app: tauri::AppHandle, action: Action) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<Controller>().action(action))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
fn get_state(controller: tauri::State<'_, Controller>) -> Result<AppState, String> {
    controller.state()
}
#[tauri::command]
async fn start_recording(app: tauri::AppHandle, notes_only: Option<bool>) -> Result<(), String> {
    if notes_only == Some(true) {
        return Err("Notes-only sessions are not implemented yet".into());
    }
    act(app, Action::Start).await
}
#[tauri::command]
async fn stop_recording(app: tauri::AppHandle) -> Result<(), String> {
    act(app, Action::Stop).await
}
#[tauri::command]
async fn pause_recording(app: tauri::AppHandle) -> Result<(), String> {
    act(app, Action::Pause).await
}
/// The Pill's Keep recording, during the countdown after the call's app lets go of the mic.
#[tauri::command]
async fn keep_recording(app: tauri::AppHandle) -> Result<(), String> {
    act(app, Action::Keep).await
}
/// The Stopped Pill's Discard (owner 2026-10-20): no notes, the meeting goes to Recently deleted.
#[tauri::command]
async fn discard_recording(app: tauri::AppHandle) -> Result<(), String> {
    act(app, Action::Discard).await
}
/// Undo in the Pill: the meeting comes back and its notes are written after all.
#[tauri::command]
async fn undo_discard(app: tauri::AppHandle, id: String) -> Result<(), String> {
    act(app.clone(), Action::UndoDiscard).await?;
    // The worker moves it into Recently deleted right after Discard; wait for that (at most 2 s).
    let trashed = app.state::<Library>().root.join(".deleted").join(&id);
    for _ in 0..20 {
        if trashed.exists() {
            break;
        }
        tauri::async_runtime::spawn_blocking(|| {
            std::thread::sleep(std::time::Duration::from_millis(100))
        })
        .await
        .map_err(|e| e.to_string())?;
    }
    let result = async {
        crate::meeting_actions::meeting_action(app.clone(), id.clone(), "restore".into()).await?;
        crate::meeting_actions::meeting_action(app.clone(), id, "regenerate".into()).await
    }
    .await;
    // The Pill has gone by now, so the library says what went wrong.
    if let Err(error) = &result {
        let _ = app.emit(
            "motetto://notice",
            format!("Couldn't bring the recording back: {error}"),
        );
    }
    result
}
#[tauri::command]
async fn resume_recording(app: tauri::AppHandle) -> Result<(), String> {
    act(app, Action::Resume).await
}
#[tauri::command]
async fn quit(app: tauri::AppHandle, stop_and_save: bool) -> Result<(), String> {
    act(app, Action::Quit(stop_and_save)).await
}
#[tauri::command]
async fn dismiss_problem(app: tauri::AppHandle, id: u64) -> Result<(), String> {
    act(app, Action::Dismiss(id)).await
}
#[tauri::command]
async fn answer_ask(app: tauri::AppHandle, call_id: String, record: bool) -> Result<(), String> {
    act(app, Action::AnswerAsk { call_id, record }).await
}
#[tauri::command]
async fn dont_ask_for_app(app: tauri::AppHandle, call_id: String) -> Result<(), String> {
    act(app, Action::DontAsk { call_id }).await
}
#[tauri::command]
async fn undo_dont_ask(app: tauri::AppHandle, app_id: String) -> Result<(), String> {
    act(app, Action::UndoDontAsk { app_id }).await
}
#[tauri::command]
async fn hold_ask(app: tauri::AppHandle, hold: bool) -> Result<(), String> {
    act(app, Action::HoldAsk(hold)).await
}
#[tauri::command]
async fn set_ask_on_call(app: tauri::AppHandle, on: bool) -> Result<(), String> {
    act(app, Action::AskOnCall(on)).await
}

struct Desktop {
    app: tauri::AppHandle,
    root: PathBuf,
    jobs: mpsc::Sender<Job>,
}
impl Host for Desktop {
    fn preferences(&self) -> (u32, u32, bool) {
        self.app
            .state::<crate::settings::Store>()
            .get()
            .map(|s| {
                (
                    s.idle_stop_s,
                    s.discard_short_s,
                    s.companion.default_size == "dot",
                )
            })
            .unwrap_or((30, 20, false))
    }

    fn mark(&self, id: &str, at_ms: u64) -> Result<(), String> {
        crate::bookmarks::mark(&self.root, id, at_ms)
    }
    fn readiness(&self, request: bool) -> Result<Readiness, String> {
        let (microphone, system_audio) = permissions::audio(request)?;
        let disk = std::process::Command::new("df")
            .arg("-Pk")
            .arg(&self.root)
            .output()
            .map_err(|e| format!("Couldn't check free disk space: {e}"))?;
        if !disk.status.success() {
            return Err("Couldn't check free disk space".into());
        }
        let free_bytes = String::from_utf8_lossy(&disk.stdout)
            .lines()
            .nth(1)
            .and_then(|line| line.split_whitespace().nth(3))
            .and_then(|n| n.parse::<u64>().ok())
            .and_then(|n| n.checked_mul(1024))
            .ok_or("Couldn't read free disk space")?;
        let speech_model = motetto_core::config::var("WHISPER_MODEL")
            .is_some_and(|p| std::fs::File::open(p).is_ok());
        Ok(Readiness {
            microphone,
            system_audio,
            notifications: permissions::notifications(false).unwrap_or(false),
            speech_model,
            free_bytes,
        })
    }
    fn event(&self, event: Event) -> Result<(), String> {
        crate::notifications::event(&self.app, &event);
        if let Event::StateChanged(state) = &event {
            crate::settings_system::sound(&self.app, state.state == SessionState::Recording);
            crate::companion::update(&self.app, state)?;
            crate::tray::update(
                &self.app,
                state,
                self.app.state::<Library>().importing.load(Ordering::SeqCst),
            )?;
            if let Some(tray) = self.app.tray_by_id("motetto") {
                let seconds = state.elapsed_ms / 1000;
                // Paused shows the frozen time without the recording dot.
                let title = match state.state {
                    SessionState::Recording => format!("● {:02}:{:02}", seconds / 60, seconds % 60),
                    SessionState::Paused => format!("❚❚ {:02}:{:02}", seconds / 60, seconds % 60),
                    _ => String::new(),
                };
                crate::window_native::tray_title(&tray, title).map_err(|e| e.to_string())?;
                tray.set_tooltip(Some(match state.state {
                    SessionState::Recording => {
                        "Motetto - Recording microphone and system audio; Stop in this menu"
                    }
                    SessionState::Paused => "Motetto - Paused, not recording; Resume in this menu",
                    _ => "Motetto - On this computer",
                }))
                .map_err(|e| e.to_string())?;
            }

            if let Some(window) = self.app.get_webview_window("main") {
                window
                    .set_title(if state.state == SessionState::Recording {
                        "Motetto - Recording microphone and system audio"
                    } else {
                        "Motetto"
                    })
                    .map_err(|e| e.to_string())?;
                if state.quit_pending {
                    show(&self.app);
                }
            }
        }
        self.app
            .emit("motetto://event", event)
            .map_err(|e| e.to_string())
    }
    /// No OS notice (SPEC 7.5): a visible companion or library window is the confirmation.
    fn announce(&self) -> Result<(), String> {
        if crate::companion::available(&self.app) {
            return Ok(());
        }
        let window = self
            .app
            .get_webview_window("main")
            .ok_or("Open Motetto before starting a recording")?;
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
        window
            .set_title("Motetto - Recording microphone and system audio")
            .map_err(|e| e.to_string())
    }
    fn show_main(&self) {
        show(&self.app);
    }
    fn job(&self, job: Job) -> Result<(), String> {
        self.jobs
            .send(job)
            .map_err(|_| "Recording worker stopped".into())
    }
    fn notice(&self, message: &str) {
        if message == crate::controller::STILL_PAUSED {
            // A reminder, not a problem: notify without pulling the library forward.
            crate::notifications::send(&self.app, "Recording paused", message);
            let _ = self.app.emit("motetto://notice", message);
            return;
        }
        if message.starts_with("Recording under ") {
            crate::notifications::send(&self.app, "Recording discarded", message);
        }
        show(&self.app);
        let _ = self.app.emit("motetto://notice", message);
    }
    fn cant_record(&self, body: &str) {
        // Mid-call, nobody pressed anything: never pull a window forward, even if this fails.
        crate::notifications::quiet(&self.app, "Motetto can't record", body);
    }
    fn exit(&self) {
        if self.jobs.send(Job::Exit).is_err() {
            // A disconnected queue means the worker has already stopped; no WAV is still open.
            self.app
                .state::<Library>()
                .allow_exit
                .store(true, Ordering::SeqCst);
            self.app.exit(0);
        }
    }
    fn app_mode(&self, app_id: &str) -> Mode {
        let saved = self
            .app
            .state::<crate::settings::Store>()
            .get()
            .ok()
            .and_then(|s| {
                s.rules()
                    .into_iter()
                    .find(|r| r.app_id == app_id)
                    .map(|r| r.mode)
            });
        saved.unwrap_or_else(|| {
            motetto_core::watch::rules()
                .into_iter()
                .find(|r| r.app_id == app_id)
                .map_or(Mode::Off, |r| r.mode)
        })
    }
    fn set_app_mode(&self, app_id: &str, mode: Mode) -> Result<(), String> {
        self.app.state::<crate::settings::Store>().change(|s| {
            s.app_modes.insert(app_id.to_string(), mode);
        })?;
        let _ = self.app.emit("motetto://settings-changed", ());
        // The tray lists Off apps; rebuild it off this thread (the controller may hold the state).
        crate::tray::invalidate(&self.app);
        let app = self.app.clone();
        tauri::async_runtime::spawn_blocking(move || refresh_tray(&app));
        Ok(())
    }
    fn ask_on_call(&self) -> bool {
        self.app
            .state::<crate::settings::Store>()
            .get()
            .is_ok_and(|s| s.ask_on_call)
    }
    fn set_ask_on_call(&self, on: bool) -> Result<(), String> {
        self.app
            .state::<crate::settings::Store>()
            .change(|s| s.ask_on_call = on)?;
        let _ = self.app.emit("motetto://settings-changed", ());
        Ok(())
    }
}

pub(crate) fn refresh_tray(app: &tauri::AppHandle) {
    let result = app.state::<Controller>().state().and_then(|state| {
        crate::tray::update(
            app,
            &state,
            app.state::<Library>().importing.load(Ordering::SeqCst),
        )
    });
    if let Err(error) = result {
        let _ = app.emit("motetto://notice", error);
    }
}

#[tauri::command]
async fn set_companion_size(
    app: tauri::AppHandle,
    size: motetto_core::events::CompanionSize,
) -> Result<(), String> {
    act(app, Action::Size(size)).await
}
/// ✎ Take a note: open the Sidecar and send the library to the Dock / taskbar (owner, 2026-10-20).
/// Only this button minimizes; a Dock click or any other way in shows the library again.
#[tauri::command]
async fn take_note(window: tauri::WebviewWindow) -> Result<(), String> {
    if window.label() != "companion" {
        return Err("Only the companion can take a note".into());
    }
    let app = window.app_handle().clone();
    act(
        app.clone(),
        Action::Size(motetto_core::events::CompanionSize::Sidecar),
    )
    .await?;
    if let Some(main) = app.get_webview_window("main") {
        main.minimize().map_err(|e| e.to_string())?;
    }
    Ok(())
}
#[tauri::command]
fn companion_ready(window: tauri::WebviewWindow) -> Result<(), String> {
    if window.label() != "companion" {
        return Err("Only the companion can report readiness".into());
    }
    crate::companion::ready(window.app_handle());
    Ok(())
}
#[tauri::command]
async fn notification_permission(request: bool) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || permissions::notifications(request))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn resume_remaining_ms(controller: tauri::State<'_, Controller>) -> Result<u64, String> {
    controller.resume_remaining_ms()
}

#[tauri::command]
async fn mark_moment(app: tauri::AppHandle) -> Result<u64, String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<Controller>().mark())
        .await
        .map_err(|e| e.to_string())?
}
