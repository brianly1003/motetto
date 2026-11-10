//! Own the local speech process; dropping a preview session also reaps its child.
use motetto_core::{config, whisper};
use std::{
    io::{BufRead, BufReader},
    net::TcpStream,
    process::{Child, Command, Stdio},
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
        // Port 0 lets whisper-server pick and keep its own port, so no other process can take it
        // before audio is sent. It logs `:0`, so the port is read back from the open socket.
        // Windows has no lsof: there the port is picked and released first, leaving that gap
        // (`netstat -ano` for the child's pid would close it).
        let fixed = if cfg!(windows) {
            Some(
                std::net::TcpListener::bind(("127.0.0.1", 0))?
                    .local_addr()?
                    .port(),
            )
        } else {
            None
        };
        let mut command = motetto_core::tools::command("whisper-server");
        command.args([
            "--host",
            "127.0.0.1",
            "--port",
            &fixed.unwrap_or(0).to_string(),
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
        // Also stopped if Motetto dies without quitting (crash, Force Quit).
        if let Err(e) = motetto_core::tools::stop_with_parent(
            child.id(),
            std::path::Path::new(command.get_program()),
        ) {
            eprintln!("Motetto: no watcher for the speech server: {e}");
        }
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
        let client = match wait_ready(&child, fixed, stop).and_then(whisper::Client::new) {
            Ok(client) => client,
            Err(e) => {
                reap(&child, Some(diagnostics));
                return Err(e);
            }
        };
        Ok(Self {
            child,
            client,
            empty_vad,
            diagnostics: Some(diagnostics),
            model,
            vad,
        })
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
        reap(&self.child, self.diagnostics.take());
    }
}

/// Kills and waits for a speech process, forgets it and ends its diagnostics reader.
fn reap(child: &Arc<Mutex<Child>>, diagnostics: Option<JoinHandle<()>>) {
    if let Ok(mut running) = child.lock() {
        let _ = running.kill();
        let _ = running.wait();
    }
    if let Ok(mut active) = ACTIVE.lock() {
        active.retain(|other| !Arc::ptr_eq(other, child));
    }
    if let Some(reader) = diagnostics {
        let _ = reader.join();
    }
}

/// The port once the server accepts connections on it; an error if it exits, Stop is pressed
/// or 90 s pass first.
fn wait_ready(
    child: &Mutex<Child>,
    mut port: Option<u16>,
    stop: &motetto_core::events::Stop,
) -> Result<u16, Box<dyn std::error::Error>> {
    let pid = child.lock().map_err(|_| "Speech process lock failed")?.id();
    let deadline = Instant::now() + Duration::from_secs(90);
    while Instant::now() < deadline && !stop.load(Ordering::SeqCst) {
        if child
            .lock()
            .map_err(|_| "Speech process lock failed")?
            .try_wait()?
            .is_some()
        {
            return Err("The local speech server could not start".into());
        }
        port = port.or_else(|| listening_port(pid));
        if let Some(port) = port
            && TcpStream::connect_timeout(
                &([127, 0, 0, 1], port).into(),
                Duration::from_millis(100),
            )
            .is_ok()
        {
            return Ok(port);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err("The local speech server did not become ready".into())
}

/// The loopback TCP port `pid` listens on, as lsof reports it; None until it binds.
fn listening_port(pid: u32) -> Option<u16> {
    let output = Command::new("/usr/sbin/lsof")
        .args(["-nP", "-a", "-p", &pid.to_string()])
        .args(["-iTCP@127.0.0.1", "-sTCP:LISTEN", "-Fn"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    lsof_port(&String::from_utf8_lossy(&output.stdout))
}

/// The port of the first `n127.0.0.1:PORT` line of `lsof -Fn` output.
fn lsof_port(text: &str) -> Option<u16> {
    text.lines()
        .filter_map(|line| line.strip_prefix('n'))
        .find_map(|name| name.rsplit_once(':')?.1.parse().ok())
        .filter(|&port| port != 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lsof_port_reads_the_listening_address() {
        assert_eq!(lsof_port("p4242\nf3\nn127.0.0.1:63474\n"), Some(63474));
        assert_eq!(lsof_port("p4242\n"), None);
        assert_eq!(lsof_port("p4242\nf3\nn127.0.0.1:0\n"), None);
        assert_eq!(lsof_port("p4242\nf3\nn127.0.0.1:http\n"), None);
    }
    #[test]
    #[cfg(target_os = "macos")]
    fn listening_port_finds_the_childs_socket() {
        // A child with one listening socket, like whisper-server; the test process itself also
        // holds other tests' listeners.
        let port = std::net::TcpListener::bind(("127.0.0.1", 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut nc = Command::new("/usr/bin/nc")
            .args(["-l", "127.0.0.1", &port.to_string()])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut found = None;
        while found.is_none() && Instant::now() < deadline {
            found = listening_port(nc.id());
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = nc.kill();
        let _ = nc.wait();
        assert_eq!(found, Some(port));
        assert_eq!(listening_port(nc.id()), None);
    }
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
