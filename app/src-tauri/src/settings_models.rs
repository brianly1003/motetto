//! Discover installed local models and measure actual speech processing time.
use serde::Serialize;
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;

#[derive(Serialize)]
pub struct Models {
    speech: Vec<String>,
    notes: Vec<String>,
    warning: Option<String>,
    /// The notes model for this computer's memory (SPEC model tiers) and that memory in GB.
    suggested: Option<String>,
    memory_gb: Option<u64>,
}

/// SPEC model tiers by memory (owner 2026-10-25: the SPEC tiers, not a 4B ceiling): Best, 16 GB or
/// more, Qwen3 8B; Balanced, 8 GB, Qwen3.5 4B (2 min for a 35-minute call against 14 for the 8B on
/// an M1 Max, 5 GB, 2026-10-25); Light below that, Qwen3 1.7B.
pub fn suggested_notes_model(memory_gb: u64) -> &'static str {
    match memory_gb {
        16.. => "qwen3:8b",
        8.. => "qwen3.5:4b",
        _ => "qwen3:1.7b",
    }
}

/// Physical memory in whole GB from `sysctl hw.memsize`; None where it can't be read. Windows
/// picks its tier from the GPU (SPEC), which isn't built yet.
pub fn memory_gb() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("/usr/sbin/sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()?;
        let bytes: u64 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
        Some(bytes >> 30)
    }
    #[cfg(not(target_os = "macos"))]
    None
}

pub fn notes_models() -> Result<Vec<String>, String> {
    let output = motetto_core::tools::command("ollama")
        .env("OLLAMA_HOST", motetto_core::ollama::host())
        .arg("list")
        .output()
        .map_err(|e| format!("Start local Ollama and retry: {e}"))?;
    if !output.status.success() {
        return Err("Start local Ollama to list installed notes models.".into());
    }
    Ok(parse_models(&String::from_utf8_lossy(&output.stdout)))
}
fn parse_models(text: &str) -> Vec<String> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let mut cols = line.split_whitespace();
            let name = cols.next()?;
            let size = cols.nth(1)?;
            (size != "-"
                && !name.ends_with("cloud")
                && !name.contains("://")
                && !name.starts_with('-'))
            .then(|| name.to_owned())
        })
        .collect()
}
#[tauri::command]
pub async fn installed_models(app: tauri::AppHandle) -> Result<Models, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let settings = app.state::<crate::settings::Store>().get()?;
        let mut speech = vec![];
        if let Some(home) = std::env::home_dir() {
            for dir in [
                home.join(".cache/whisper"),
                app.path()
                    .app_data_dir()
                    .map_err(|e| e.to_string())?
                    .join("models"),
            ] {
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file()
                            && path.extension().is_some_and(|e| e == "bin")
                            && !path
                                .file_name()
                                .is_some_and(|n| n.to_string_lossy().contains("silero"))
                        {
                            speech.push(path.to_string_lossy().into_owned());
                        }
                    }
                }
            }
        }
        if !settings.speech_path.is_empty() {
            speech.push(settings.speech_path);
        }
        speech.sort();
        speech.dedup();
        let (notes, warning) = match notes_models() {
            Ok(models) => (models, None),
            Err(e) => (vec![], Some(e)),
        };
        let memory_gb = memory_gb();
        Ok(Models {
            speech,
            notes,
            warning,
            suggested: memory_gb.map(|gb| suggested_notes_model(gb).to_string()),
            memory_gb,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn pick_speech_model(app: tauri::AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("Whisper GGML model", &["bin"])
            .blocking_pick_file()
            .map(|file| {
                file.into_path()
                    .map(|p| p.to_string_lossy().into_owned())
                    .map_err(|e| e.to_string())
            })
            .transpose()
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn run_speed_test(app: tauri::AppHandle) -> Result<f64, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let library = app.state::<crate::library_commands::Library>();
        if library.busy.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return Err("Wait until the current recording or import finishes.".into());
        }
        let result = speed_test();
        library
            .busy
            .store(false, std::sync::atomic::Ordering::SeqCst);
        let _ = app.emit("motetto://library-changed", ());
        result
    })
    .await
    .map_err(|e| e.to_string())?
}
fn speed_test() -> Result<f64, String> {
    #[cfg(target_os = "macos")]
    {
        let dir = std::env::temp_dir().join(format!(
            "motetto-speed-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos()
        ));
        std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
        let result = (|| {
            let sample = dir.join("sample.aiff");
            let status=std::process::Command::new("say").args(["-r","160","-o"]).arg(&sample).arg("This is a local speech speed test. We discussed the project timeline and agreed to review the next steps together at our meeting tomorrow.").status().map_err(|e|e.to_string())?;
            if !status.success() {
                return Err("Couldn't generate the local test sample.".into());
            }
            let wav = dir.join("sample.wav");
            motetto_core::decode_audio(&sample, &wav).map_err(|e| e.to_string())?;
            let mut reader = hound::WavReader::open(wav).map_err(|e| e.to_string())?;
            let audio = reader
                .samples::<f32>()
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            let seconds = audio.len() as f64 / 16000.;
            let stop = Default::default();
            let server = crate::speech::Server::start(&stop).map_err(|e| e.to_string())?;
            let started = std::time::Instant::now();
            server
                .client
                .transcribe(&audio, 0, Some("en"), None)
                .map_err(|e| e.to_string())?;
            Ok(started.elapsed().as_secs_f64() / seconds)
        })();
        let cleanup = std::fs::remove_dir_all(dir).map_err(|e| e.to_string());
        match result {
            Err(e) => Err(e),
            Ok(rtf) => {
                cleanup?;
                Ok(rtf)
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    Err("The speech speed test sample is not available on Windows yet.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_notes_model_follows_the_memory_tiers() {
        for (gb, model) in [
            (64, "qwen3:8b"),
            (16, "qwen3:8b"),
            (15, "qwen3.5:4b"),
            (8, "qwen3.5:4b"),
            (7, "qwen3:1.7b"),
        ] {
            assert_eq!(suggested_notes_model(gb), model, "{gb} GB");
        }
    }
    #[test]
    fn model_discovery_rejects_cloud_rows_and_option_names() {
        assert_eq!(
            parse_models(
                "NAME ID SIZE MODIFIED\nqwen3:8b abc 5.2 GB today\nremote:latest abc - today\nx-cloud abc 1 GB today\n--help abc 1 GB today\n"
            ),
            vec!["qwen3:8b"]
        );
    }
}
