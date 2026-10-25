//! Native settings actions; no shell interpolation or background network requests.
use tauri::Manager;

pub fn login(app: &tauri::AppHandle, enabled: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let file = app
            .path()
            .home_dir()
            .map_err(|e| e.to_string())?
            .join("Library/LaunchAgents/local.motetto.desktop.plist");
        if enabled {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            if !exe.to_string_lossy().contains(".app/Contents/MacOS/") {
                return Err(
                    "Open at login requires the installed Motetto.app. Build and open the app first."
                        .into(),
                );
            }
            let escaped = exe
                .to_string_lossy()
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;");
            std::fs::create_dir_all(file.parent().ok_or("Missing login folder")?)
                .map_err(|e| e.to_string())?;
            let text = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\"><plist version=\"1.0\"><dict><key>Label</key><string>local.motetto.desktop</string><key>ProgramArguments</key><array><string>{escaped}</string><string>--background</string></array><key>RunAtLoad</key><true/></dict></plist>"
            );
            let tmp = file.with_extension("plist.tmp");
            std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
            std::fs::rename(tmp, file).map_err(|e| e.to_string())?;
        } else if file.try_exists().map_err(|e| e.to_string())? {
            std::fs::remove_file(file).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, enabled);
        Err("Open at login is awaiting Windows integration.".into())
    }
}
/// Settings › About; the full texts ship in `resources/LICENSES.md`.
const LICENCES: &str = "whisper.cpp (MIT) · llama.cpp (MIT) · Qwen3 / Qwen3.5 models (Apache-2.0) · Whisper models (MIT) · Silero VAD (MIT) · LibriVox sample (public domain) · Tauri (MIT or Apache-2.0) · Next.js (MIT)";
#[tauri::command]
pub fn about(app: tauri::AppHandle) -> serde_json::Value {
    serde_json::json!({"version":app.package_info().version.to_string(),"licences":LICENCES,"platform":std::env::consts::OS})
}
pub fn reveal(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = std::process::Command::new("explorer");
    let result = command.arg(path).status().map_err(|e| e.to_string())?;
    if !result.success() {
        return Err("Couldn't open the folder.".into());
    }
    Ok(())
}
#[tauri::command]
pub fn open_logs_folder(app: tauri::AppHandle) -> Result<(), String> {
    let dir = app.path().app_log_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    reveal(&dir)
}

/// Optional earcons supplement the persistent visible recording indicator.
pub fn sound(app: &tauri::AppHandle, recording: bool) {
    static RECORDING: std::sync::Mutex<bool> = std::sync::Mutex::new(false);
    let Ok(mut previous) = RECORDING.lock() else {
        return;
    };
    if *previous == recording {
        return;
    }
    *previous = recording;
    if !app
        .state::<crate::settings::Store>()
        .get()
        .is_ok_and(|s| s.sounds)
    {
        return;
    }
    #[cfg(target_os = "macos")]
    std::thread::spawn(move || {
        let _ = std::process::Command::new("afplay")
            .arg(if recording {
                "/System/Library/Sounds/Tink.aiff"
            } else {
                "/System/Library/Sounds/Pop.aiff"
            })
            .status();
    });
}
