//! The notes model: a bundled `llama-server` on 127.0.0.1, started on the first prompt and
//! stopped 5 minutes after the last answer. Stop kills the server instead of streaming: a
//! watcher thread checks `tools::cancelled()` every 100 ms and kills the child, which fails the
//! blocked request at once; the next prompt starts a fresh server (a reload of a few seconds).
use motetto_core::tools;
use std::{
    error::Error,
    hash::{BuildHasher, Hasher, RandomState},
    io::{self, BufRead},
    path::{Path, PathBuf},
    process::{Child, Stdio},
    sync::{
        Arc, Mutex, OnceLock,
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
        forget_pid();
    }
}

/// `llama-server.pid` in the app data folder: the running server, so a launch after a crash or
/// Force Quit can stop the one left behind.
static PID_FILE: OnceLock<PathBuf> = OnceLock::new();

/// At launch: remember where the pid file lives and stop a server an earlier run left running.
pub fn configure(data_dir: &Path) {
    let file = PID_FILE.get_or_init(|| data_dir.join("llama-server.pid"));
    let Ok(text) = std::fs::read_to_string(file) else {
        return;
    };
    let bundled = tools::bundled("llama-server");
    if let Some(pid) = bundled.and_then(|b| left_behind(&text, &b, running_program)) {
        eprintln!("Motetto: stopping notes engine {pid} left by an earlier run");
        let _ = std::process::Command::new("kill")
            .arg(pid.to_string())
            .status();
    }
    let _ = std::fs::remove_file(file);
}

/// The pid in `text` when that process runs the bundled `llama-server`; never another program
/// that has since been given the same pid.
fn left_behind(text: &str, bundled: &Path, program: impl Fn(u32) -> Option<String>) -> Option<u32> {
    let pid = text.trim().parse::<u32>().ok().filter(|&pid| pid > 1)?;
    (Path::new(program(pid)?.trim()) == bundled).then_some(pid)
}

/// The executable `pid` runs, as `ps` names it (the full path it was started from on macOS).
fn running_program(pid: u32) -> Option<String> {
    // Windows: no `ps`; a server left there is not stopped yet (taskkill /FI would do it).
    let output = std::process::Command::new("ps")
        .args(["-o", "comm=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn remember_pid(pid: u32) {
    if let Some(file) = PID_FILE.get() {
        let _ = std::fs::write(file, pid.to_string());
    }
}

fn forget_pid() {
    if let Some(file) = PID_FILE.get() {
        let _ = std::fs::remove_file(file);
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
            forget_pid();
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
        .args(["-c", &CONTEXT_TOKENS.to_string(), "--no-webui"])
        .args(["--parallel", "1", "-n", &MAX_ANSWER_TOKENS.to_string()])
        .args(["-ngl", "99", "--jinja"])
        // From the environment, not the command line, where `ps` shows it to every user.
        .env("LLAMA_API_KEY", &key)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Held across the check and the spawn so shutdown can't miss a child that is starting.
    let mut active = ACTIVE.lock().map_err(|_| "notes engine lock failed")?;
    if SHUTTING_DOWN.load(Ordering::SeqCst) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "Motetto is quitting").into());
    }
    let mut child =
        tools::spawn(&mut command).map_err(|e| format!("The notes engine could not start: {e}"))?;
    remember_pid(child.id());
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
        .map_err(|e| format!("The notes engine did not answer: {e}"))
        .and_then(|response| {
            if response.status() == 200 {
                stream(io::BufReader::new(response.into_body().into_reader()))
            } else {
                let body = response.into_body().read_to_string().unwrap_or_default();
                answer(&body)
            }
        });
    done.store(true, Ordering::SeqCst);
    let _ = watcher.join();
    Ok(reply?)
}

/// The streamed answer (server-sent events), reporting the engine's own counts as they come
/// (SPEC 6.6): prompt tokens read, then words written. A Stop kills the server, which ends the
/// stream with an error here.
fn stream(reader: impl BufRead) -> Result<String, String> {
    let (mut text, mut words, mut chars, mut in_word) = (String::new(), 0u64, 0u64, false);
    for line in reader.lines() {
        let line = line.map_err(|e| format!("The notes engine did not answer: {e}"))?;
        let Some(data) = line.strip_prefix("data:").map(str::trim) else {
            continue;
        };
        if data == "[DONE]" {
            return Ok(text);
        }
        let value: serde_json::Value =
            serde_json::from_str(data).map_err(|_| "The notes engine sent an unreadable answer")?;
        if let Some(error) = value.get("error") {
            let message = error.get("message").and_then(|m| m.as_str());
            return Err(format!(
                "The notes engine failed: {}",
                message.unwrap_or("unknown error")
            ));
        }
        if let Some(read) = value.get("prompt_progress") {
            let number = |key: &str| read.get(key).and_then(|n| n.as_u64()).unwrap_or(0);
            // Tokens already cached from an earlier prompt aren't read again.
            let cache = number("cache");
            motetto_core::progress::reading(
                number("processed").saturating_sub(cache),
                number("total").saturating_sub(cache),
            );
        }
        if let Some(piece) = value["choices"][0]["delta"]["content"].as_str() {
            // Counted per piece: recounting the whole answer each time is quadratic in its length.
            for c in piece.chars() {
                let space = c.is_whitespace();
                if !space && !in_word {
                    words += 1;
                }
                in_word = !space;
                chars += 1;
            }
            text.push_str(piece);
            motetto_core::progress::writing(words, chars);
        }
    }
    // The server always ends with [DONE]; a stream cut short is a failure, not an answer.
    Err("The notes engine stopped before the end of its answer".into())
}

fn request_body(prompt: &str) -> serde_json::Value {
    serde_json::json!({
        "messages": [{ "role": "user", "content": prompt }],
        "temperature": 0.7,
        "top_p": 0.8,
        "top_k": 20,
        "chat_template_kwargs": { "enable_thinking": false },
        "stream": true,
        "return_progress": true,
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
    fn stream_joins_the_answer_and_counts_what_the_engine_reports() {
        // The summarizer's plan claims the counts for its thread, as when notes are written.
        let plan = motetto_core::progress::NotesPlan::new(&[1000], 0.0);
        let events = "data: {\"prompt_progress\":{\"total\":15513,\"cache\":13,\"processed\":2061}}\n\n\
            data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n\
            data: {\"choices\":[{\"delta\":{\"content\":\"### Cập\"}}]}\n\n\
            data: {\"choices\":[{\"delta\":{\"content\":\" nhật luồng\"}}]}\n\n\
            data: [DONE]\n\n";
        assert_eq!(stream(events.as_bytes()).unwrap(), "### Cập nhật luồng");
        let counts = motetto_core::progress::notes_counts();
        assert_eq!((counts.read, counts.read_of), (2048, 15500));
        assert_eq!((counts.words, counts.written), (4, 18));
        plan.end();
        // Cut short (a killed server): an error, never half the notes.
        assert!(
            stream("data: {\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\n".as_bytes()).is_err()
        );
        assert!(
            stream("data: {\"error\":{\"message\":\"busy\"}}\n".as_bytes())
                .unwrap_err()
                .contains("busy")
        );
    }

    #[test]
    fn request_disables_thinking() {
        let body = request_body("Hi");
        assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
        assert_eq!(body["temperature"], 0.7);
        assert_eq!(body["top_p"], 0.8);
        assert_eq!(body["top_k"], 20);
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
    fn only_the_bundled_server_left_behind_is_stopped() {
        let bundled = Path::new("/Applications/Motetto.app/Contents/MacOS/llama-server");
        let ps = |name: &'static str| move |_| Some(format!("{name}\n"));
        let server = ps("/Applications/Motetto.app/Contents/MacOS/llama-server");
        assert_eq!(left_behind("4242\n", bundled, server), Some(4242));
        // The pid now belongs to another program, or to no process at all.
        assert_eq!(left_behind("4242", bundled, ps("/usr/bin/vim")), None);
        assert_eq!(left_behind("4242", bundled, ps("llama-server")), None);
        assert_eq!(left_behind("4242", bundled, |_| None), None);
        // A damaged file, or a pid that would reach other processes.
        assert_eq!(left_behind("", bundled, server), None);
        assert_eq!(left_behind("-1", bundled, server), None);
        assert_eq!(left_behind("1", bundled, server), None);
        assert_eq!(left_behind("0", bundled, server), None);
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
    /// Measures notes progress (SPEC 6.6) on a real transcript: every report, timed, then the end.
    /// Score with the skill's scripts/progress/score-notes.py.
    #[test]
    #[ignore = "needs MOTETTO_TEST_GGUF, MOTETTO_TEST_TRANSCRIPT and MOTETTO_TEST_LOG"]
    fn notes_progress_on_a_real_call() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("motetto-notes-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let built = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/binaries"))
            .unwrap()
            .filter_map(Result::ok)
            .find(|e| e.file_name().to_string_lossy().starts_with("llama-server-"))
            .unwrap();
        std::os::unix::fs::symlink(built.path(), dir.join("llama-server")).unwrap();
        tools::configure_bundled(dir.clone());
        fn ask(prompt: &str) -> Result<String, Failure> {
            ask_with(&std::env::var("MOTETTO_TEST_GGUF").unwrap(), prompt)
        }
        motetto_core::summarize::configure_engine(motetto_core::summarize::Engine {
            ask,
            context_tokens: CONTEXT_TOKENS,
        });
        let transcript =
            std::fs::read_to_string(std::env::var("MOTETTO_TEST_TRANSCRIPT").unwrap()).unwrap();
        let log =
            Mutex::new(std::fs::File::create(std::env::var("MOTETTO_TEST_LOG").unwrap()).unwrap());
        let started = Instant::now();
        let base = dir.join("notes").to_string_lossy().into_owned();
        motetto_core::summarize::notes(&base, &transcript, "", None, None, &|stage| {
            let line = serde_json::to_string(&stage).unwrap();
            writeln!(
                log.lock().unwrap(),
                "{:.1}\t{line}",
                started.elapsed().as_secs_f64()
            )
            .unwrap();
        })
        .unwrap();
        writeln!(
            log.lock().unwrap(),
            "{:.1}\tend",
            started.elapsed().as_secs_f64()
        )
        .unwrap();
        stop_server(false);
    }

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
