//! Saved desktop preferences, separate from the CLI environment configuration.
use motetto_core::{
    events::Mode,
    watch::{self, AppRule},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::{Emitter, Manager};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct Companion {
    pub default_size: String,
    pub captions: bool,
}
impl Default for Companion {
    fn default() -> Self {
        Self {
            default_size: "pill".into(),
            captions: true,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Spelling {
    pub spelling: String,
    pub heard: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub notes_folder: String,
    pub open_at_login: bool,
    pub notes_language: String,
    pub ask_on_call: bool,
    pub app_modes: BTreeMap<String, Mode>,
    pub apps: Vec<AppRule>,
    pub idle_stop_s: u32,
    pub discard_short_s: u32,
    pub speech_model: String,
    pub notes_model: String,
    pub call_language: String,
    pub part_chars: u32,
    pub keep_audio: String,
    pub recording_reminder: bool,
    /// The microphone Motetto records (a `motetto_core::mics::Mic` id); None follows the system.
    pub mic_device: Option<String>,
    pub sounds: bool,
    pub companion: Companion,
    pub spellings: Vec<Spelling>,
    pub shortcuts: BTreeMap<String, String>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            notes_folder: String::new(),
            open_at_login: false,
            notes_language: "same_as_call".into(),
            ask_on_call: true,
            app_modes: BTreeMap::new(),
            apps: Vec::new(),
            idle_stop_s: 30,
            discard_short_s: 20,
            speech_model: motetto_core::config::var("WHISPER_MODEL").unwrap_or_default(),
            // A first run with no model set starts with the one suggested for this memory, if installed.
            notes_model: motetto_core::config::var("OLLAMA_MODEL")
                .filter(|m| !m.is_empty())
                .or_else(crate::settings_models::installed_suggestion)
                .unwrap_or_default(),
            call_language: "detect".into(),
            part_chars: motetto_core::config::var("SUMMARY_CHUNK_CHARS")
                .and_then(|n| n.parse().ok())
                .filter(|n| (8000..=48000).contains(n))
                .unwrap_or(24000),
            keep_audio: "30_days".into(),
            recording_reminder: true,
            mic_device: None,
            sounds: true,
            companion: Companion::default(),
            spellings: motetto_core::config::terms()
                .map(|terms| {
                    terms
                        .split(", ")
                        .map(|name| Spelling {
                            spelling: name.into(),
                            heard: String::new(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            shortcuts: ["S", "M", "J", "N", "Space", "H", "P"]
                .into_iter()
                .map(|key| (key.into(), format!("Ctrl+Alt+{key}")))
                .collect(),
        }
    }
}
impl Settings {
    pub fn read(dir: &Path) -> Result<Self, String> {
        match std::fs::read_to_string(dir.join("settings.json")) {
            Ok(text) => {
                let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
                    format!("Couldn't read settings. Preserve settings.json and repair it: {e}")
                })?;
                let mut settings: Self = serde_json::from_value(value.clone())
                    .map_err(|e| format!("Couldn't read settings: {e}"))?;
                // Existing users never opted into automatic audio deletion.
                if value.get("keep_audio").is_none() {
                    settings.keep_audio = "forever".into();
                }
                settings.validate()?;
                Ok(settings)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("Couldn't read settings: {e}")),
        }
    }
    // Read-only callers can fall back; writes always use the validated Store.
    pub fn load(dir: &Path) -> Self {
        Self::read(dir).unwrap_or_default()
    }
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let tmp = dir.join("settings.json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?,
        )
        .map_err(|e| format!("Couldn't save settings: {e}"))?;
        std::fs::rename(tmp, dir.join("settings.json"))
            .map_err(|e| format!("Couldn't save settings: {e}"))
    }
    pub fn rules(&self) -> Vec<AppRule> {
        let mut rules = watch::rules();
        rules.extend(self.apps.clone());
        for rule in &mut rules {
            if let Some(mode) = self.app_modes.get(&rule.app_id) {
                rule.mode = *mode;
            }
        }
        rules
    }
    fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || !(10..=300).contains(&self.idle_stop_s)
            || self.discard_short_s > 120
            || !(8000..=48000).contains(&self.part_chars)
        {
            return Err("Use 10-300 idle seconds, 0-120 discard seconds and 8000-48000 characters per part.".into());
        }
        if self
            .mic_device
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 255 || id.contains('\0'))
        {
            return Err("That microphone id isn't valid.".into());
        }
        if !["detect", "en", "vi", "zh", "ja"].contains(&self.call_language.as_str())
            || (self.notes_language != "same_as_call"
                && motetto_core::summarize::language_name(&self.notes_language).is_none())
            || !["30_days", "forever", "after_notes"].contains(&self.keep_audio.as_str())
            || !["pill", "dot"].contains(&self.companion.default_size.as_str())
        {
            return Err("Choose a supported language, retention or companion size.".into());
        }
        if self.spellings.len() > 500
            || self.spellings.iter().any(|x| {
                x.spelling.trim().is_empty()
                    || x.spelling.chars().count() > 40
                    || x.heard.chars().count() > 80
                    || x.spelling
                        .chars()
                        .any(|c| !c.is_alphanumeric() && !" -.&'()/+#".contains(c))
                    || x.heard.contains(['\n', '\r'])
            })
        {
            return Err(
                "Use up to 500 names, each at most 40 letters, digits, spaces or name punctuation."
                    .into(),
            );
        }
        if self.shortcuts.len() != 7
            || ["S", "M", "J", "N", "Space", "H", "P"]
                .iter()
                .any(|k| !self.shortcuts.contains_key(*k))
            || self
                .shortcuts
                .values()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != 7
        {
            return Err("Each shortcut must be different.".into());
        }
        let mut shortcuts = Vec::new();
        for accelerator in self.shortcuts.values() {
            let shortcut = accelerator
                .parse::<tauri_plugin_global_shortcut::Shortcut>()
                .map_err(|_| "Use a valid shortcut such as Ctrl+Alt+S.")?;
            if shortcuts.contains(&shortcut) {
                return Err("Each shortcut must be different.".into());
            }
            shortcuts.push(shortcut);
        }
        Ok(())
    }
}

pub struct Store {
    pub dir: PathBuf,
    pub value: Mutex<Settings>,
    pub recent: Mutex<BTreeMap<String, u64>>,
}
impl Store {
    pub fn get(&self) -> Result<Settings, String> {
        self.value
            .lock()
            .map(|s| s.clone())
            .map_err(|_| "Settings unavailable".into())
    }
    pub fn change(&self, change: impl FnOnce(&mut Settings)) -> Result<Settings, String> {
        let mut saved = self.value.lock().map_err(|_| "Settings unavailable")?;
        let mut next = saved.clone();
        change(&mut next);
        next.validate()?;
        if next != *saved {
            next.save(&self.dir)?;
        }
        *saved = next.clone();
        Ok(next)
    }
}
pub fn remember_apps(app: &tauri::AppHandle, snapshot: &watch::Snapshot) {
    let store = app.state::<Store>();
    if let Ok(mut recent) = store.recent.lock() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let old = recent.clone();
        recent.retain(|_, time| now.saturating_sub(*time) < 7 * 86400);
        for process in &snapshot.processes {
            if process.input
                && process.pid != std::process::id()
                && process.bundle_id != "local.motetto.desktop"
                && process.bundle_id.len() <= 255
                && (recent.contains_key(&process.bundle_id) || recent.len() < 500)
            {
                recent.insert(process.bundle_id.clone(), now / 60 * 60);
            }
        }
        if *recent != old
            && let Ok(text) = serde_json::to_vec(&*recent)
        {
            let tmp = store.dir.join("recent-mic-apps.json.tmp");
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(tmp, store.dir.join("recent-mic-apps.json"));
            }
        }
    }
}
#[tauri::command]
pub fn get_settings(store: tauri::State<'_, Store>) -> Result<Settings, String> {
    store.get()
}
#[tauri::command]
pub async fn save_settings(app: tauri::AppHandle, settings: Settings) -> Result<Settings, String> {
    tauri::async_runtime::spawn_blocking(move || {
        settings.validate()?;
        let store = app.state::<Store>();
        let old = store.get()?;
        if settings.notes_folder != old.notes_folder {
            return Err("Use Choose to move the notes folder.".into());
        }
        if settings.apps != old.apps {
            return Err("Use Add app to change call detection rules.".into());
        }
        let library = app.state::<crate::library_commands::Library>();
        let needs_idle = settings.speech_model != old.speech_model
            || settings.notes_model != old.notes_model
            || settings.call_language != old.call_language
            || settings.notes_language != old.notes_language
            || settings.part_chars != old.part_chars
            || settings.spellings != old.spellings;
        if needs_idle && library.busy.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return Err(
                "Wait until the recording or import finishes before changing settings.".into(),
            );
        }
        let mut result = (|| {
            if settings.speech_model != old.speech_model
                && !Path::new(&settings.speech_model).is_file()
            {
                return Err(
                    "Download the speech model first. The current model is still in use.".into(),
                );
            }
            if settings.notes_model != old.notes_model {
                crate::settings_models::check_notes_model(&settings.notes_model)?;
            }
            if settings.open_at_login != old.open_at_login {
                crate::settings_system::login(&app, settings.open_at_login)?;
            }
            if settings.shortcuts != old.shortcuts {
                crate::shortcuts::replace(&app, &old.shortcuts, &settings.shortcuts)?;
            }
            store.change(|s| {
                // Calls choices may have changed in the controller since this window loaded.
                // So may the mic, from the Pill or the recording page (`microphone.rs`).
                let ask = s.ask_on_call;
                let modes = s.app_modes.clone();
                let mic = s.mic_device.clone();
                *s = settings.clone();
                s.ask_on_call = ask;
                s.app_modes = modes;
                s.mic_device = mic;
            })
        })();
        if let Err(reason) = &mut result {
            if settings.open_at_login != old.open_at_login
                && let Err(error) = crate::settings_system::login(&app, old.open_at_login)
            {
                reason.push_str(&format!(" Couldn't restore the login preference: {error}"));
            }
            if settings.shortcuts != old.shortcuts
                && let Err(error) =
                    crate::shortcuts::replace(&app, &settings.shortcuts, &old.shortcuts)
            {
                reason.push_str(&format!(" Couldn't restore shortcuts: {error}"));
            }
        }
        if needs_idle {
            library
                .busy
                .store(false, std::sync::atomic::Ordering::SeqCst);
        }
        if result.is_ok() {
            crate::tray::invalidate(&app);
            crate::desktop::refresh_tray(&app);
            let _ = app.emit("motetto://settings-changed", ());
        }
        result
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn set_app_mode(app: tauri::AppHandle, app_id: String, mode: Mode) -> Result<(), String> {
    if !app
        .state::<Store>()
        .get()?
        .rules()
        .iter()
        .any(|r| r.app_id == app_id)
    {
        return Err("Unknown call app".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<crate::controller::Controller>()
            .action(crate::controller::Action::AppMode { app_id, mode })
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub fn recent_mic_apps(store: tauri::State<'_, Store>) -> Result<Vec<String>, String> {
    let known = store.get()?.rules();
    Ok(store
        .recent
        .lock()
        .map_err(|_| "Recent apps unavailable")?
        .keys()
        .filter(|id| watch::match_app(&known, id).is_none())
        .cloned()
        .collect())
}
#[tauri::command]
pub fn add_app(app: tauri::AppHandle, app_id: String) -> Result<Settings, String> {
    let store = app.state::<Store>();
    if !store
        .recent
        .lock()
        .map_err(|_| "Recent apps unavailable")?
        .contains_key(&app_id)
    {
        return Err("Choose an app that recently used the microphone.".into());
    }
    if store.get()?.rules().iter().any(|r| r.app_id == app_id) {
        return Err("That app is already listed.".into());
    }
    let saved = store.change(|s| {
        s.apps.push(AppRule {
            app_id: app_id.clone(),
            name: app_id.clone(),
            mac_ids: vec![app_id.clone()],
            win_ids: vec![app_id],
            title_patterns: vec![],
            single_signal: false,
            mode: Mode::Ask,
        })
    })?;
    let _ = app.emit("motetto://settings-changed", ());
    Ok(saved)
}
#[tauri::command]
pub fn call_apps(store: tauri::State<'_, Store>) -> Result<Vec<AppRule>, String> {
    Ok(store.get()?.rules())
}

pub fn open(app: &tauri::AppHandle, tab: &str) -> Result<(), String> {
    if ![
        "general",
        "audio",
        "calls",
        "models",
        "spellings",
        "privacy",
        "about",
    ]
    .contains(&tab)
    {
        return Err("Unknown settings tab".into());
    }
    if let Some(window) = app.get_webview_window("settings") {
        window.show().map_err(|e| e.to_string())?;
        window.unminimize().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
        window
            .emit("motetto://settings-tab", tab)
            .map_err(|e| e.to_string())?;
    } else {
        tauri::WebviewWindowBuilder::new(
            app,
            "settings",
            tauri::WebviewUrl::App(format!("settings/?tab={tab}").into()),
        )
        .title("Settings")
        .inner_size(960., 760.)
        .prevent_overflow()
        .center()
        .min_inner_size(760., 520.)
        .build()
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}
#[tauri::command]
pub fn open_settings(app: tauri::AppHandle, tab: Option<String>) -> Result<(), String> {
    open(&app, tab.as_deref().unwrap_or("general"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_settings_preserve_audio_and_failed_saves_preserve_memory() {
        let root =
            std::env::temp_dir().join(format!("motetto-settings-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("settings.json"), r#"{"ask_on_call":false}"#).unwrap();
        let settings = Settings::read(&root).unwrap();
        assert_eq!(settings.keep_audio, "forever");
        let store = Store {
            dir: root.clone(),
            value: Mutex::new(settings.clone()),
            recent: Mutex::default(),
        };
        std::fs::create_dir(root.join("settings.json.tmp")).unwrap();
        assert!(store.change(|s| s.sounds = false).is_err());
        assert_eq!(store.get().unwrap(), settings);
        std::fs::remove_dir(root.join("settings.json.tmp")).unwrap();
        std::fs::write(root.join("settings.json"), "{broken").unwrap();
        assert!(Settings::read(&root).is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("settings.json")).unwrap(),
            "{broken"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn migration_validation_and_bindings() {
        let old: Settings =
            serde_json::from_str(r#"{"ask_on_call":false,"app_modes":{"zoom":"off"}}"#).unwrap();
        assert!(!old.ask_on_call);
        assert_eq!(old.app_modes["zoom"], Mode::Off);
        assert_eq!(old.idle_stop_s, 30);
        old.validate().unwrap();
        let mut invalid = old.clone();
        invalid.part_chars = 0;
        assert!(invalid.validate().is_err());
        invalid = old;
        invalid.spellings.push(Spelling {
            spelling: "Ignore: [rules]".into(),
            heard: String::new(),
        });
        assert!(invalid.validate().is_err());
        Settings::export_all(&ts_rs::Config::default().with_out_dir("../../ui/lib/bindings"))
            .unwrap();
    }
}
