//! Saved desktop preferences, separate from the CLI environment configuration.
use crate::catalog::{Catalog, FileKind, ModelFile, SpeedResult, TierId, catalog};
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub notes_folder: String,
    pub open_at_login: bool,
    pub appearance: String,
    pub notes_language: String,
    pub ask_on_call: bool,
    pub app_modes: BTreeMap<String, Mode>,
    pub apps: Vec<AppRule>,
    pub idle_stop_s: u32,
    pub discard_short_s: u32,
    /// Catalog ids (empty until chosen); the `*_path` fields are what the engines read.
    pub speech_model: String,
    pub notes_model: String,
    pub onboarding_done: bool,
    pub onboarding_step: u8,
    pub consent_confirmed_at: Option<String>,
    pub tier: Option<TierId>,
    pub speed_test: Option<SpeedResult>,
    pub live_transcription: bool,
    /// The tier a pending `speed_slow` notice would offer.
    pub slow_question: Option<TierId>,
    pub screen_requested: bool,
    /// Catalog id -> path of a verified file the user already had.
    pub adopted: BTreeMap<String, String>,
    /// Resolved and written only by Rust after verification; empty means not ready.
    pub speech_path: String,
    pub vad_path: String,
    pub notes_path: String,
    pub call_language: String,
    pub part_chars: u32,
    pub keep_audio: String,
    pub screen_offer_on_ask: bool,
    pub screen_source: String,
    pub screen_on_own_share: String,
    pub screen_preset: String,
    pub screen_cursor: bool,
    pub keep_video: String,
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
            appearance: "system".into(),
            notes_language: "same_as_call".into(),
            ask_on_call: true,
            app_modes: BTreeMap::new(),
            apps: Vec::new(),
            idle_stop_s: 30,
            discard_short_s: 20,
            speech_model: String::new(),
            notes_model: String::new(),
            onboarding_done: false,
            onboarding_step: 0,
            consent_confirmed_at: None,
            tier: None,
            speed_test: None,
            live_transcription: true,
            slow_question: None,
            screen_requested: false,
            adopted: BTreeMap::new(),
            speech_path: String::new(),
            vad_path: String::new(),
            notes_path: String::new(),
            call_language: "detect".into(),
            part_chars: motetto_core::config::var("SUMMARY_CHUNK_CHARS")
                .and_then(|n| n.parse().ok())
                .filter(|n| (8000..=48000).contains(n))
                .unwrap_or(24000),
            keep_audio: "30_days".into(),
            screen_offer_on_ask: true,
            screen_source: "call_window".into(),
            screen_on_own_share: "ask".into(),
            screen_preset: "sharp".into(),
            screen_cursor: true,
            keep_video: "same_as_audio".into(),
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
    /// Reads and, once, migrates an M4 file: the result is saved at once so the SHA-256 of a
    /// multi-GB file is computed at most one time.
    pub fn read(dir: &Path) -> Result<Self, String> {
        Self::read_with(
            dir,
            catalog(),
            std::env::home_dir(),
            motetto_core::config::var("WHISPER_VAD_MODEL"),
        )
    }
    fn read_with(
        dir: &Path,
        cat: &Catalog,
        home: Option<PathBuf>,
        vad_env: Option<String>,
    ) -> Result<Self, String> {
        let mut settings = Self::parse(dir)?;
        if settings.migrate(cat, home, vad_env) {
            // A failed save only means the next read tries again.
            let _ = settings.save(dir);
        }
        Ok(settings)
    }
    fn parse(dir: &Path) -> Result<Self, String> {
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
    /// M4 stored a speech model path and an Ollama name; both are catalog ids now. A path to a
    /// verified catalog file is adopted (so it isn't downloaded again), as is the old voice
    /// detection file; anything else is cleared. True when something changed and must be saved.
    fn migrate(&mut self, cat: &Catalog, home: Option<PathBuf>, vad_env: Option<String>) -> bool {
        let speech_old = cat.file(&self.speech_model).is_none() && !self.speech_model.is_empty();
        let notes_old = cat.file(&self.notes_model).is_none() && !self.notes_model.is_empty();
        if !speech_old && !notes_old {
            return false;
        }
        let cache = home.map(|h| h.join(".cache/whisper"));
        let adopt = |file: &ModelFile, first: Option<PathBuf>| {
            let candidates: Vec<PathBuf> = first
                .into_iter()
                .chain(cache.as_ref().map(|c| c.join(&file.file)))
                .collect();
            crate::model_store::adopt(file, &candidates)
                .map(|found| (file.id.clone(), found.to_string_lossy().into_owned()))
        };
        if speech_old {
            let old = std::mem::take(&mut self.speech_model);
            let name = Path::new(&old).file_name().and_then(|n| n.to_str());
            if let Some(file) = cat
                .files
                .iter()
                .find(|f| f.kind == FileKind::Speech && Some(f.file.as_str()) == name)
                && let Some((id, path)) = adopt(file, Some(PathBuf::from(&old)))
            {
                self.adopted.insert(id.clone(), path);
                self.speech_model = id;
            }
        }
        if notes_old {
            self.notes_model.clear();
        }
        if let Some(file) = cat.file(&cat.vad)
            && !self.adopted.contains_key(&file.id)
            && let Some((id, path)) =
                adopt(file, vad_env.filter(|v| !v.is_empty()).map(PathBuf::from))
        {
            self.adopted.insert(id, path);
        }
        true
    }
    // Read-only callers (the tray) fall back to defaults and never migrate, so they never hash.
    pub fn load(dir: &Path) -> Self {
        Self::parse(dir).unwrap_or_default()
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
        if !["system", "light", "dark"].contains(&self.appearance.as_str()) {
            return Err("Choose System, Light or Dark for appearance.".into());
        }
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
            || !["call_window", "display"].contains(&self.screen_source.as_str())
            || !["ask", "keep_window"].contains(&self.screen_on_own_share.as_str())
            || !["sharp", "smaller"].contains(&self.screen_preset.as_str())
            || !["same_as_audio", "7_days", "forever"].contains(&self.keep_video.as_str())
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

/// Setup and the verified model paths are written by Rust only (first run, the runner); a
/// settings window's save never changes them.
fn keep_setup(s: &mut Settings, saved: Settings) {
    s.onboarding_done = saved.onboarding_done;
    s.onboarding_step = saved.onboarding_step;
    s.consent_confirmed_at = saved.consent_confirmed_at;
    s.tier = saved.tier;
    s.speed_test = saved.speed_test;
    s.live_transcription = saved.live_transcription;
    s.slow_question = saved.slow_question;
    s.screen_requested = saved.screen_requested;
    s.adopted = saved.adopted;
    s.speech_path = saved.speech_path;
    s.vad_path = saved.vad_path;
    s.notes_path = saved.notes_path;
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
        let models = crate::model_store::dir(&app);
        let mut result = (|| {
            let on_disk = |id: &str| {
                catalog()
                    .file(id)
                    .and_then(|f| crate::model_store::path(&models, &old.adopted, f))
                    .is_some()
            };
            if settings.speech_model != old.speech_model && !on_disk(&settings.speech_model) {
                return Err(
                    "Download the speech model first. The current model is still in use.".into(),
                );
            }
            if settings.notes_model != old.notes_model && !on_disk(&settings.notes_model) {
                return Err(
                    "Download the notes model first. The current model is still in use.".into(),
                );
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
                let rust_owned = s.clone();
                *s = settings.clone();
                s.ask_on_call = ask;
                s.app_modes = modes;
                s.mic_device = mic;
                keep_setup(s, rust_owned);
                crate::setup::refresh_paths(s, catalog(), &models);
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
            apply_appearance(&app, &settings.appearance);
            crate::desktop::dispatch(
                app.clone(),
                crate::controller::Action::ShareVideoAsk(settings.screen_on_own_share == "ask"),
            );
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

pub fn apply_appearance(app: &tauri::AppHandle, appearance: &str) {
    app.set_theme(match appearance {
        "light" => Some(tauri::Theme::Light),
        "dark" => Some(tauri::Theme::Dark),
        _ => None,
    });
}
#[tauri::command]
pub fn open_settings(app: tauri::AppHandle, tab: Option<String>) -> Result<(), String> {
    open(&app, tab.as_deref().unwrap_or("general"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn appearance_defaults_validates_and_persists() {
        let mut settings: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings.appearance, "system");
        for appearance in ["system", "light", "dark"] {
            settings.appearance = appearance.into();
            settings.validate().unwrap();
            let saved = serde_json::to_string(&settings).unwrap();
            let restored: Settings = serde_json::from_str(&saved).unwrap();
            assert_eq!(restored.appearance, appearance);
        }
        settings.appearance = "sepia".into();
        assert!(settings.validate().is_err());
    }
    #[test]
    fn screen_settings_default_and_validate() {
        let mut settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(settings.screen_offer_on_ask && settings.screen_cursor);
        assert_eq!(settings.keep_video, "same_as_audio");
        assert_eq!(settings.screen_source, "call_window");
        settings.screen_preset = "huge".into();
        assert!(settings.validate().is_err());
        settings.screen_preset = "smaller".into();
        assert!(settings.validate().is_ok());
    }

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
    fn migrates_m4_settings() {
        let root = std::env::temp_dir().join(format!("motet-settings-m4-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).unwrap();
        std::fs::write(
            root.join("settings.json"),
            r#"{"speech_model":"/Users/x/.cache/whisper/ggml-large-v3-q5_0.bin","notes_model":"qwen3:8b","ask_on_call":false}"#,
        )
        .unwrap();
        let settings = Settings::read_with(&root, catalog(), None, None).unwrap();
        assert_eq!(settings.speech_model, "");
        assert_eq!(settings.notes_model, "");
        assert!(!settings.onboarding_done);
        assert!(settings.live_transcription && settings.adopted.is_empty());
        assert!(!settings.ask_on_call);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn migration_adopts_once_and_persists() {
        use sha2::{Digest, Sha256};
        let root =
            std::env::temp_dir().join(format!("motet-settings-adopt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).unwrap();
        let file = |id: &str, kind, name: &str, data: &[u8]| ModelFile {
            id: id.into(),
            kind,
            file: name.into(),
            label: id.into(),
            url: String::new(),
            bytes: data.len() as u64,
            sha256: format!("{:x}", Sha256::digest(data)),
            on_disk: false,
        };
        let cat = Catalog {
            files: vec![
                file("s", FileKind::Speech, "s.bin", b"speech"),
                file("v", FileKind::Vad, "v.bin", b"vad"),
            ],
            tiers: vec![],
            vad: "v".into(),
        };
        let speech = root.join("s.bin");
        let vad = root.join("v.bin");
        std::fs::write(&speech, b"speech").unwrap();
        std::fs::write(&vad, b"vad").unwrap();
        let old = serde_json::json!({"speech_model": speech, "notes_model": "qwen3:8b"});
        std::fs::write(root.join("settings.json"), old.to_string()).unwrap();
        let vad_env = Some(vad.to_string_lossy().into_owned());
        let first = Settings::read_with(&root, &cat, None, vad_env).unwrap();
        assert_eq!(first.speech_model, "s");
        assert_eq!(first.notes_model, "");
        assert_eq!(first.adopted["s"], speech.to_string_lossy());
        assert_eq!(first.adopted["v"], vad.to_string_lossy());
        // Saved: the next read has catalog ids and must not hash again, even with the files gone.
        std::fs::remove_file(&speech).unwrap();
        let again = Settings::read_with(&root, &cat, None, None).unwrap();
        assert_eq!(again.speech_model, "s");
        assert_eq!(again.adopted, first.adopted);
        // The tray's load never migrates.
        std::fs::write(root.join("settings.json"), old.to_string()).unwrap();
        assert_ne!(Settings::load(&root).speech_model, "s");
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
