//! The notes model: a bundled `llama-server` on 127.0.0.1, started on the first prompt and
//! stopped 5 minutes after the last answer. Stop kills the server instead of streaming: a
//! watcher thread checks `tools::cancelled()` every 100 ms and kills the child, which fails the
//! blocked request at once; the next prompt starts a fresh server (a reload of a few seconds).
use motetto_core::tools;
use std::{
    error::Error,
    hash::{BuildHasher, Hasher, RandomState},
    io::{self, BufRead},
    process::{Child, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// Context the server runs with and `summarize` sizes the transcript parts for.
pub const CONTEXT_TOKENS: u32 = 32_768;
/// `-n`: the cap on one answer, which also bounds a model that loops.
const MAX_ANSWER_TOKENS: u32 = 8_192;
/// What callers see when no notes model has been resolved (settings `notes_path` is empty).
pub const NOT_READY: &str = motetto_core::summarize::NOTES_NOT_READY;
const IDLE: Duration = Duration::from_secs(5 * 60);
const LOAD_LIMIT: Duration = Duration::from_secs(60);
type Failure = Box<dyn Error + Send + Sync>;

struct Server {
    child: Arc<Mutex<Child>>,
    model: String,
    port: u16,
    key: String,
    last: Instant,
    id: u64,
}
static SERVER: Mutex<Option<Server>> = Mutex::new(None);
/// The running child, reachable while `ask` holds `SERVER`, so exit can still kill it.
static ACTIVE: Mutex<Option<Arc<Mutex<Child>>>> = Mutex::new(None);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

impl Drop for Server {
    fn drop(&mut self) {
        kill(&self.child);
    }
}
fn kill(child: &Mutex<Child>) {
    if let Ok(mut child) = child.lock() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

/// For app exit: no server starts afterwards.
pub fn shutdown() {
    stop_server(true);
}
fn stop_server(quitting: bool) {
    if let Ok(mut active) = ACTIVE.lock() {
        if quitting {
            SHUTTING_DOWN.store(true, Ordering::SeqCst);
        }
        if let Some(child) = active.take() {
            kill(&child);
        }
    }
}

pub fn ask(prompt: &str) -> Result<String, Failure> {
    let model = motetto_core::config::notes_path().ok_or(NOT_READY)?;
    ask_with(&model, prompt)
}

fn ask_with(model: &str, prompt: &str) -> Result<String, Failure> {
    tools::check()?;
    // Under 16 GB the model and a transcription slice don't fit together: let the slice finish.
    if crate::hardware::memory_gb().is_some_and(|gb| gb < 16) {
        wait_while(crate::live::slicing, tools::cancelled)?;
    }
    let mut guard = SERVER.lock().map_err(|_| "notes engine lock failed")?;
    let running = guard.as_ref().is_some_and(|s| {
        s.model == model
            && s.child
                .lock()
                .is_ok_and(|mut c| c.try_wait().ok() == Some(None))
    });
    if !running {
        *guard = None;
        *guard = Some(start(model)?);
        idle_timer(guard.as_ref().map_or(0, |s| s.id));
    }
    let server = guard.as_mut().ok_or("notes engine lock failed")?;
    let result = request(server, prompt);
    server.last = Instant::now();
    if result.is_err() && tools::cancelled() {
        *guard = None;
        return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled").into());
    }
    result
}

/// Sleeps while `busy()`; an `Interrupted` error as soon as `cancelled()`.
fn wait_while(busy: impl Fn() -> bool, cancelled: impl Fn() -> bool) -> io::Result<()> {
    while busy() {
        if cancelled() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

fn agent(timeout: Duration) -> ureq::Agent {
    // Loopback only: never a proxy from the environment.
    ureq::Agent::config_builder()
        .proxy(None)
        .http_status_as_error(false)
        .timeout_global(Some(timeout))
        .build()
        .into()
}

fn key() -> String {
    (0..2)
        .map(|_| format!("{:016x}", RandomState::new().build_hasher().finish()))
        .collect()
}

fn start(model: &str) -> Result<Server, Failure> {
    let key = key();
    // Port 0 lets llama-server pick and keep its own port: no gap for another process to take it
    // before the key and transcript are sent. The port is read from its log.
    let mut command = tools::command("llama-server");
    command
        .args(["-m", model, "--host", "127.0.0.1", "--port", "0"])
        .args(["--api-key", &key, "-c", &CONTEXT_TOKENS.to_string()])
        .args(["--parallel", "1", "-n", &MAX_ANSWER_TOKENS.to_string()])
        .args(["-ngl", "99", "--jinja"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Held across the check and the spawn so shutdown can't miss a child that is starting.
    let mut active = ACTIVE.lock().map_err(|_| "notes engine lock failed")?;
    if SHUTTING_DOWN.load(Ordering::SeqCst) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "Motetto is quitting").into());
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("The notes engine could not start: {e}"))?;
    let (sender, ports) = std::sync::mpsc::channel();
    for stream in [
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn io::Read + Send>),
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn io::Read + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let sender = sender.clone();
        std::thread::spawn(move || {
            // Keep draining bytes to the end, even past a line that isn't UTF-8, so the server
            // never blocks on a full pipe; the text is not kept.
            let mut reader = io::BufReader::new(stream);
            let mut line = Vec::new();
            loop {
                line.clear();
                if !matches!(reader.read_until(b'\n', &mut line), Ok(1..)) {
                    break;
                }
                let text = String::from_utf8_lossy(&line);
                if let Some(rest) = text.split("listening on http://127.0.0.1:").nth(1) {
                    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
                    if let Ok(port) = digits.parse::<u16>() {
                        let _ = sender.send(port);
                    }
                }
            }
        });
    }
    let child = Arc::new(Mutex::new(child));
    *active = Some(child.clone());
    drop(active);
    // From here a failed wait drops the server, which kills the child.
    let mut server = Server {
        child,
        model: model.to_owned(),
        port: 0,
        key,
        last: Instant::now(),
        id: NEXT_ID.fetch_add(1, Ordering::SeqCst),
    };
    let deadline = Instant::now() + LOAD_LIMIT;
    while server.port == 0 {
        tools::check()?;
        if Instant::now() >= deadline {
            return Err("The notes engine did not report its port".into());
        }
        if server
            .child
            .lock()
            .map_err(|_| "notes engine lock failed")?
            .try_wait()?
            .is_some()
        {
            return Err("The notes engine stopped while starting".into());
        }
        if let Ok(port) = ports.recv_timeout(Duration::from_millis(100)) {
            server.port = port;
        }
    }
    let port = server.port;
    let probe = agent(Duration::from_secs(2));
    while Instant::now() < deadline {
        tools::check()?;
        if server
            .child
            .lock()
            .map_err(|_| "notes engine lock failed")?
            .try_wait()?
            .is_some()
        {
            return Err("The notes engine stopped while loading the model".into());
        }
        let health = probe
            .get(&format!("http://127.0.0.1:{port}/health"))
            .header("Authorization", &format!("Bearer {}", server.key))
            .call();
        if health.is_ok_and(|r| r.status() == 200) {
            return Ok(server);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err("The notes engine did not finish loading the model".into())
}

/// Stops server `id` once it has been idle for `IDLE`; ends when another server replaces it.
fn idle_timer(id: u64) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(10));
            let Ok(mut guard) = SERVER.lock() else { return };
            match guard.as_ref() {
                Some(s) if s.id == id && s.last.elapsed() >= IDLE => {
                    *guard = None;
                    return;
                }
                Some(s) if s.id == id => {}
                _ => return,
            }
        }
    });
}

fn request(server: &Server, prompt: &str) -> Result<String, Failure> {
    let done = Arc::new(AtomicBool::new(false));
    let watcher = {
        let (child, done) = (server.child.clone(), done.clone());
        std::thread::spawn(move || {
            while !done.load(Ordering::SeqCst) {
                if tools::cancelled() {
                    kill(&child);
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        })
    };
    let reply = agent(Duration::from_secs(600))
        .post(&format!(
            "http://127.0.0.1:{}/v1/chat/completions",
            server.port
        ))
        .header("Authorization", &format!("Bearer {}", server.key))
        .header("Content-Type", "application/json")
        .send(request_body(prompt).to_string())
        .and_then(|mut r| r.body_mut().read_to_string());
    done.store(true, Ordering::SeqCst);
    let _ = watcher.join();
    Ok(answer(&reply.map_err(|e| {
        format!("The notes engine did not answer: {e}")
    })?)?)
}

fn request_body(prompt: &str) -> serde_json::Value {
    serde_json::json!({
        "messages": [{ "role": "user", "content": prompt }],
        "temperature": 0,
        "chat_template_kwargs": { "enable_thinking": false },
    })
}

fn answer(body: &str) -> Result<String, String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|_| "The notes engine sent an unreadable answer")?;
    if let Some(error) = value.get("error") {
        let message = error.get("message").and_then(|m| m.as_str());
        return Err(format!(
            "The notes engine failed: {}",
            message.unwrap_or("unknown error")
        ));
    }
    value["choices"][0]["message"]["content"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "The notes engine sent no answer".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_disables_thinking() {
        let body = request_body("Hi");
        assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
        assert_eq!(body["temperature"], 0);
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "Hi");
    }

    #[test]
    fn answer_reads_first_choice() {
        let body = r#"{"choices":[{"message":{"role":"assistant","content":"Action items"}},{"message":{"content":"x"}}]}"#;
        assert_eq!(answer(body).unwrap(), "Action items");
    }

    #[test]
    fn answer_rejects_error_body() {
        let error = answer(r#"{"error":{"code":401,"message":"Invalid API Key"}}"#).unwrap_err();
        assert!(error.contains("Invalid API Key"));
        assert!(answer("not json").is_err());
        assert!(answer(r#"{"choices":[]}"#).is_err());
    }

    #[test]
    fn ask_without_model_is_model_missing() {
        // No desktop settings are installed in tests, so no notes model is resolved.
        assert_eq!(ask("Hi").unwrap_err().to_string(), NOT_READY);
    }

    #[test]
    fn waiting_for_a_slice_ends_on_stop() {
        let started = Instant::now();
        let error = wait_while(|| true, || true).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(wait_while(|| false, || true).is_ok());
    }

    /// Needs the bundled llama-server and a GGUF: `MOTETTO_TEST_GGUF=<file>`. Run alone, since
    /// Stop is one global flag.
    #[cfg(unix)]
    #[test]
    #[ignore = "needs the bundled llama-server and MOTETTO_TEST_GGUF"]
    fn server_killed_on_drop() {
        let model = std::env::var("MOTETTO_TEST_GGUF").unwrap();
        // The built file carries the target triple; the app bundle renames it.
        let dir = std::env::temp_dir().join(format!("motetto-llama-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let built = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/binaries"))
            .unwrap()
            .filter_map(Result::ok)
            .find(|e| e.file_name().to_string_lossy().starts_with("llama-server-"))
            .unwrap();
        std::os::unix::fs::symlink(built.path(), dir.join("llama-server")).unwrap();
        tools::configure_bundled(dir);
        let started = Instant::now();
        let prompt = "Write a one-line summary of this call.\n\nMe: Can you send the budget by Friday?\nThem: Sure, I'll send it Thursday.";
        let text = ask_with(&model, prompt).unwrap();
        eprintln!("answer in {:.1}s: {text}", started.elapsed().as_secs_f32());
        assert!(!text.trim().is_empty());
        let pid = SERVER
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .child
            .lock()
            .unwrap()
            .id();
        stop_server(false);
        std::thread::sleep(Duration::from_millis(300));
        let alive = std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .unwrap()
            .success();
        assert!(!alive, "llama-server {pid} is still running");
        // Stop during a long answer ends the call within a second and reports Interrupted.
        let slow = std::thread::spawn(|| {
            let model = std::env::var("MOTETTO_TEST_GGUF").unwrap();
            ask_with(&model, "Count from 1 to 5000, one number per line.")
        });
        std::thread::sleep(Duration::from_secs(8));
        let stopped = Instant::now();
        tools::cancel();
        let error = slow.join().unwrap().unwrap_err();
        eprintln!("stop took {:.2}s", stopped.elapsed().as_secs_f32());
        assert!(stopped.elapsed() < Duration::from_secs(1));
        let kind = error.downcast_ref::<io::Error>().map(io::Error::kind);
        assert_eq!(kind, Some(io::ErrorKind::Interrupted));
        tools::reset();
        stop_server(false);
    }
}
