//! Own the local speech process; dropping a preview session also reaps its child.
use motetto_core::{config, whisper};
use std::{
    io::{BufRead, BufReader},
    net::{TcpListener, TcpStream},
    process::{Child, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

pub struct Server {
    child: Arc<Mutex<Child>>,
    pub client: whisper::Client,
    empty_vad: Arc<AtomicBool>,
    diagnostics: Option<JoinHandle<()>>,
    model: String,
    vad: Option<String>,
}
/// Every running speech process: captions and a rehearsal's following can each have one, and
/// quitting must not leave either behind.
static ACTIVE: Mutex<Vec<Arc<Mutex<Child>>>> = Mutex::new(Vec::new());
pub fn shutdown() {
    let children = ACTIVE
        .lock()
        .map(|mut a| std::mem::take(&mut *a))
        .unwrap_or_default();
    for child in children {
        if let Ok(mut child) = child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
impl Server {
    pub fn start(stop: &motetto_core::events::Stop) -> Result<Self, Box<dyn std::error::Error>> {
        let model = config::var("WHISPER_MODEL").ok_or("The speech model is not configured")?;
        let vad = config::var("WHISPER_VAD_MODEL").filter(|v| !v.is_empty());
        Self::start_with(stop, model, vad)
    }
    /// A given model rather than the configured one (the speed test of a tier not chosen yet).
    pub fn start_with(
        stop: &motetto_core::events::Stop,
        model: String,
        vad: Option<String>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        drop(listener);
        let mut command = motetto_core::tools::command("whisper-server");
        command.args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "-m",
            &model,
            "-l",
            config::call_language().as_deref().unwrap_or("auto"),
            // Per-slice language detection stays on; -nlp drops the server's second detection
            // pass (language_probabilities), which Motetto never reads: ~4.8 s -> ~3.0 s per slice.
            "-nlp",
        ]);
        if let Some(vad) = &vad {
            command.args(["--vad", "--vad-model", vad]);
        }
        let mut child = motetto_core::tools::spawn(
            command
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped()),
        )?;
        let empty_vad = Arc::new(AtomicBool::new(false));
        let detected = empty_vad.clone();
        let stderr = child
            .stderr
            .take()
            .ok_or("Speech diagnostics unavailable")?;
        let diagnostics = std::thread::spawn(move || {
            // Consume diagnostics without saving speech text or growing an in-memory log.
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if let Some(count) = line.strip_prefix(
                    "whisper_vad_segments_from_probs: Final speech segments after filtering: ",
                ) {
                    detected.store(count.trim() == "0", Ordering::SeqCst);
                }
            }
        });
        let child = Arc::new(Mutex::new(child));
        ACTIVE
            .lock()
            .map_err(|_| "Speech process lock failed")?
            .push(child.clone());
        let server = Self {
            child,
            client: whisper::Client::new(port)?,
            empty_vad,
            diagnostics: Some(diagnostics),
            model,
            vad,
        };
        let deadline = Instant::now() + Duration::from_secs(90);
        while Instant::now() < deadline && !stop.load(std::sync::atomic::Ordering::SeqCst) {
            if server
                .child
                .lock()
                .map_err(|_| "Speech process lock failed")?
                .try_wait()?
                .is_some()
            {
                return Err("The local speech server could not start".into());
            }
            if TcpStream::connect_timeout(
                &format!("127.0.0.1:{port}").parse()?,
                Duration::from_millis(100),
            )
            .is_ok()
            {
                return Ok(server);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Err("The local speech server did not become ready".into())
    }

    pub fn transcribe(
        &mut self,
        audio: &[f32],
        offset: u64,
        prompt: Option<&str>,
        stop: &motetto_core::events::Stop,
    ) -> Result<Vec<whisper::Segment>, Box<dyn std::error::Error>> {
        self.empty_vad.store(false, Ordering::SeqCst);
        let result =
            self.client
                .transcribe(audio, offset, config::call_language().as_deref(), prompt);
        let mut exited = false;
        if result.is_err() {
            // A disconnected socket can reach us just before the OS reports process exit.
            for _ in 0..20 {
                exited = self
                    .child
                    .lock()
                    .map_err(|_| "Speech process lock failed")?
                    .try_wait()?
                    .is_some();
                if exited || stop.load(Ordering::SeqCst) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        }
        if exited {
            if let Some(reader) = self.diagnostics.take() {
                let _ = reader.join();
            }
            // Some whisper-server versions exit when auto language detection follows empty VAD.
            // Only a confirmed empty VAD result is silence; every other failure stays an error.
            if self.empty_vad.load(Ordering::SeqCst) && !stop.load(Ordering::SeqCst) {
                *self = Self::start_with(stop, self.model.clone(), self.vad.clone())?;
                return Ok(Vec::new());
            }
        }
        result
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Ok(mut active) = ACTIVE.lock() {
            active.retain(|other| !Arc::ptr_eq(other, &self.child));
        }
        if let Some(reader) = self.diagnostics.take() {
            let _ = reader.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires whisper-server, WHISPER_MODEL, WHISPER_VAD_MODEL and MOTETTO_TEST_SPEECH WAV"]
    fn managed_server_transcribes_synthetic_speech_and_reaps_child() {
        let source = std::env::var("MOTETTO_TEST_SPEECH").unwrap();
        let samples = hound::WavReader::open(source)
            .unwrap()
            .samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let stop = Default::default();
        // The models e2e.sh exports: a global ~/.config/motetto/.env would win over them in
        // `Server::start`, and cargo runs this from the crate folder, away from e2e's ./.env.
        let mut server = Server::start_with(
            &stop,
            std::env::var("WHISPER_MODEL").unwrap(),
            std::env::var("WHISPER_VAD_MODEL").ok(),
        )
        .unwrap();
        // Nonzero background noise passes the preview amplitude gate but VAD finds no speech.
        assert!(
            server
                .transcribe(&vec![0.006; 48_000], 0, None, &stop)
                .unwrap()
                .is_empty()
        );
        let child = server.child.clone();
        let segments = server.transcribe(&samples, 60_000, None, &stop).unwrap();
        assert!(!segments.is_empty());
        assert!(segments.iter().all(|segment| segment.start_ms >= 60_000));
        drop(server);
        assert!(child.lock().unwrap().try_wait().unwrap().is_some());
    }
}
