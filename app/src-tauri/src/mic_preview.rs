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
/// Whether the setup is open, and the running preview. One lock, so a start and a stop never
/// overlap, and a start that arrives after the overlays closed is refused.
struct Preview {
    open: bool,
    running: Option<Running>,
}
static PREVIEW: Mutex<Preview> = Mutex::new(Preview {
    open: false,
    running: None,
});

#[derive(Debug, PartialEq, Eq)]
enum Start {
    /// The setup is closed (or closing): the mic stays untouched.
    Refuse,
    Already,
    Go,
}
fn decide(open: bool, running: bool) -> Start {
    match (open, running) {
        (false, _) => Start::Refuse,
        (true, true) => Start::Already,
        (true, false) => Start::Go,
    }
}

/// Root mean square of the samples, the scale the recording meters use.
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

fn start() -> Result<(), String> {
    let mut preview = PREVIEW.lock().unwrap_or_else(|e| e.into_inner());
    match decide(preview.open, preview.running.is_some()) {
        Start::Refuse => return Err("The screen setup is closed.".into()),
        Start::Already => return Ok(()),
        Start::Go => {}
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
            preview.running = Some((stop, thread));
        } else {
            let _ = thread.join();
        }
        result
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = &mut preview;
        Err("No mic preview on this system.".into())
    }
}

fn stop_locked(preview: &mut Preview) {
    if let Some((stop, thread)) = preview.running.take() {
        drop(stop);
        let _ = thread.join();
    }
    LEVEL.store(0, Ordering::Relaxed);
}

/// Stops the preview and waits for the engine to let go of the mic.
pub fn stop() {
    stop_locked(&mut PREVIEW.lock().unwrap_or_else(|e| e.into_inner()));
}

/// The overlays opened (true) or closed (false); closing stops the preview for good.
pub fn set_open(open: bool) {
    let mut preview = PREVIEW.lock().unwrap_or_else(|e| e.into_inner());
    preview.open = open;
    if !open {
        stop_locked(&mut preview);
    }
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
    fn a_start_after_the_setup_closed_is_refused() {
        assert_eq!(decide(false, false), Start::Refuse);
        assert_eq!(decide(false, true), Start::Refuse);
        assert_eq!(decide(true, true), Start::Already);
        assert_eq!(decide(true, false), Start::Go);
    }

    #[test]
    fn rms_of_silence_and_a_square_wave() {
        assert_eq!(rms(&[]), 0.0);
        assert_eq!(rms(&[0.0; 8]), 0.0);
        assert!((rms(&[0.5, -0.5, 0.5, -0.5]) - 0.5).abs() < 1e-6);
    }
}
