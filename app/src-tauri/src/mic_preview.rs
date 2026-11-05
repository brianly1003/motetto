//! The mic level before recording, for the screen setup's mic chip: the recording meters only run
//! while recording. A meter only: no file, no listener, stopped when the overlays close (and so
//! before the recording starts). The system default input only; a chosen device shows no meter
//! until the engine can be pointed at it (AudioUnit kAudioOutputUnitProperty_CurrentDevice).
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU32, Ordering},
    mpsc,
};

static LEVEL: std::sync::LazyLock<Arc<AtomicU32>> = std::sync::LazyLock::new(Arc::default);
/// The running preview: its stop signal and thread.
type Running = (mpsc::Sender<()>, std::thread::JoinHandle<()>);
static RUNNING: Mutex<Option<Running>> = Mutex::new(None);

/// Root mean square of the samples, the scale the recording meters use.
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

fn start() -> Result<(), String> {
    let mut running = RUNNING.lock().unwrap_or_else(|e| e.into_inner());
    if running.is_some() {
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        let (stop, wait) = mpsc::channel();
        let (started, result) = mpsc::channel();
        let level = LEVEL.clone();
        let thread = std::thread::Builder::new()
            .name("motetto-mic-preview".into())
            .spawn(move || crate::mic_preview_macos::run(level, started, wait))
            .map_err(|e| e.to_string())?;
        let result = result
            .recv()
            .unwrap_or_else(|_| Err("The mic preview stopped.".into()));
        if result.is_ok() {
            *running = Some((stop, thread));
        } else {
            let _ = thread.join();
        }
        result
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = &mut running;
        Err("No mic preview on this system.".into())
    }
}

/// Stops the preview and waits for the engine to let go of the mic.
pub fn stop() {
    let running = RUNNING.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some((stop, thread)) = running {
        drop(stop);
        let _ = thread.join();
    }
    LEVEL.store(0, Ordering::Relaxed);
}

/// On while the setup is open with the system default mic; off for None or another device.
#[tauri::command]
pub async fn capture_mic_preview(on: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        if on {
            start()
        } else {
            stop();
            Ok(())
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn capture_level() -> f32 {
    f32::from_bits(LEVEL.load(Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rms_of_silence_and_a_square_wave() {
        assert_eq!(rms(&[]), 0.0);
        assert_eq!(rms(&[0.0; 8]), 0.0);
        assert!((rms(&[0.5, -0.5, 0.5, -0.5]) - 0.5).abs() < 1e-6);
    }
}
