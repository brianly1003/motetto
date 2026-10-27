//! Desktop child-tool search paths without changing the process environment or CLI behavior,
//! and Cancel for the desktop import: every child tool is waited on here, so it can be stopped.
use std::{
    ffi::OsString,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Output, Stdio},
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

static DESKTOP_PATH: OnceLock<OsString> = OnceLock::new();
static BUNDLED_DIR: OnceLock<PathBuf> = OnceLock::new();

/// The programs the desktop ships next to the app; with a bundled folder set they never come
/// from PATH.
pub const BUNDLED: [&str; 4] = [
    "whisper-server",
    "whisper-cli",
    "whisper-vad-speech-segments",
    "llama-server",
];

/// Desktop only: run the `BUNDLED` programs from `dir`.
pub fn configure_bundled(dir: PathBuf) {
    let _ = BUNDLED_DIR.set(dir);
}

/// Where the bundled `program` runs from, when the desktop set a bundled folder.
pub fn bundled(program: &str) -> Option<PathBuf> {
    BUNDLED_DIR.get().map(|dir| bundled_path(dir, program))
}

fn bundled_path(dir: &Path, program: &str) -> PathBuf {
    dir.join(if cfg!(windows) {
        format!("{program}.exe")
    } else {
        program.to_string()
    })
}

/// Where `program` runs from. A bundled program is never looked up on PATH: a missing one is an
/// error, not a fallback to whatever else is installed.
fn resolve(program: &str, bundled: Option<&Path>) -> Result<PathBuf, io::Error> {
    match bundled {
        Some(dir) if BUNDLED.contains(&program) => {
            let path = bundled_path(dir, program);
            if path.is_file() {
                Ok(path)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("Motetto's bundled {program} is missing; reinstall Motetto"),
                ))
            }
        }
        _ => Ok(PathBuf::from(program)),
    }
}

/// Finder launches omit Homebrew. Extend only desktop child environments, never global PATH.
pub fn configure_desktop() {
    #[cfg(target_os = "macos")]
    DESKTOP_PATH.get_or_init(|| search_path(std::env::var_os("PATH").unwrap_or_default()));
}

#[cfg(any(target_os = "macos", test))]
fn search_path(existing: OsString) -> OsString {
    use std::path::PathBuf;
    let mut paths: Vec<_> = std::env::split_paths(&existing).collect();
    for directory in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"] {
        let directory = PathBuf::from(directory);
        if !paths.contains(&directory) {
            paths.push(directory);
        }
    }
    std::env::join_paths(paths).expect("existing PATH components and fixed directories are valid")
}

pub fn command(program: &str) -> Command {
    let bundled = BUNDLED_DIR.get().map(PathBuf::as_path);
    // A missing bundled file still gets its path, so spawning fails with NotFound.
    let path = resolve(program, bundled)
        .unwrap_or_else(|_| bundled_path(bundled.unwrap_or(Path::new("")), program));
    let mut command = Command::new(path);
    if let Some(path) = DESKTOP_PATH.get() {
        command.env("PATH", path);
    }
    command
}

/// `Command::spawn`, but a bundled program that is missing says so instead of the bare OS error.
pub fn spawn(command: &mut Command) -> io::Result<Child> {
    command.spawn().map_err(|error| explain(command, error))
}

/// The `resolve` message for a bundled program `command` could not start; else `error` as is.
pub fn explain(command: &Command, error: io::Error) -> io::Error {
    explain_in(BUNDLED_DIR.get().map(PathBuf::as_path), command, error)
}

fn explain_in(bundled: Option<&Path>, command: &Command, error: io::Error) -> io::Error {
    let name = Path::new(command.get_program()).file_stem();
    match (bundled, name.and_then(|n| n.to_str())) {
        (Some(dir), Some(name)) if error.kind() == io::ErrorKind::NotFound => {
            resolve(name, Some(dir)).err().unwrap_or(error)
        }
        _ => error,
    }
}

// One processing job runs at a time (the desktop's busy flag), so one flag is enough; each job
// resets it before it starts, so a late Cancel can't stop the next one.
// process-wide flag; pass a token per job if jobs ever run side by side.
static CANCELLED: AtomicBool = AtomicBool::new(false);

/// Stops the running child tool and fails every later stage with `ErrorKind::Interrupted`.
pub fn cancel() {
    CANCELLED.store(true, Ordering::SeqCst);
}
/// Called when a processing job starts.
pub fn reset() {
    CANCELLED.store(false, Ordering::SeqCst);
}
pub fn cancelled() -> bool {
    CANCELLED.load(Ordering::SeqCst)
}
/// Between stages: an error once Cancel was pressed.
pub fn check() -> io::Result<()> {
    if cancelled() { Err(stopped()) } else { Ok(()) }
}
fn stopped() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "cancelled")
}

/// `Command::status`, but Cancel kills the child.
pub fn status(command: &mut Command) -> io::Result<ExitStatus> {
    check()?;
    wait(&mut spawn(command)?, &CANCELLED)
}

/// `status`, with each stderr line passed to `line` as it arrives (whisper-cli's progress).
pub fn status_lines(command: &mut Command, line: &(dyn Fn(&str) + Sync)) -> io::Result<ExitStatus> {
    use io::BufRead;
    check()?;
    let mut child = spawn(command.stderr(Stdio::piped()))?;
    let stderr = child.stderr.take();
    std::thread::scope(|scope| {
        if let Some(pipe) = stderr {
            // Ends when the child exits or is killed and its end of the pipe closes.
            scope.spawn(move || {
                io::BufReader::new(pipe)
                    .lines()
                    .map_while(Result::ok)
                    .for_each(|text| line(&text));
            });
        }
        wait(&mut child, &CANCELLED)
    })
}

/// `Command::output` with `input` on stdin, but Cancel kills the child.
pub fn output(command: &mut Command, input: Option<&[u8]>) -> io::Result<Output> {
    check()?;
    let stdin = if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    };
    let mut child = spawn(
        command
            .stdin(stdin)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )?;
    // Pipes drain on their own threads so a chatty child can't fill one and stall.
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut bytes = vec![];
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut bytes);
            }
            bytes
        })
    };
    let stdout = drain(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let stderr = drain(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    if let (Some(input), Some(mut pipe)) = (input, child.stdin.take()) {
        pipe.write_all(input)?;
    }
    let status = wait(&mut child, &CANCELLED)?;
    Ok(Output {
        status,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

fn wait(child: &mut Child, cancel: &AtomicBool) -> io::Result<ExitStatus> {
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if cancel.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(stopped());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    fn missing_error() -> io::Error {
        io::Error::from(io::ErrorKind::NotFound)
    }
    #[test]
    fn resolve_prefers_bundled_and_never_falls_back() {
        let dir = std::env::temp_dir().join(format!("motet-bundled-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = bundled_path(&dir, "whisper-cli");
        fs::write(&file, "").unwrap();
        assert_eq!(resolve("whisper-cli", Some(&dir)).unwrap(), file);
        let error = resolve("llama-server", Some(&dir)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert_eq!(
            error.to_string(),
            "Motetto's bundled llama-server is missing; reinstall Motetto"
        );
        let missing = Command::new(bundled_path(&dir, "llama-server"));
        let error = explain_in(Some(&dir), &missing, missing_error());
        assert_eq!(
            error.to_string(),
            "Motetto's bundled llama-server is missing; reinstall Motetto"
        );
        let other = Command::new("ffmpeg");
        assert_eq!(
            explain_in(Some(&dir), &other, missing_error()).to_string(),
            missing_error().to_string()
        );
        let cli = explain_in(None, &missing, missing_error());
        assert_eq!(cli.to_string(), missing_error().to_string());
        assert_eq!(
            resolve("ffmpeg", Some(&dir)).unwrap(),
            PathBuf::from("ffmpeg")
        );
        assert_eq!(
            resolve("whisper-cli", None).unwrap(),
            PathBuf::from("whisper-cli")
        );
        let _ = fs::remove_dir_all(&dir);
    }
    #[test]
    #[cfg(target_os = "macos")]
    #[ignore = "requires installed whisper-cli, whisper-server and Ollama; run with Finder PATH"]
    fn desktop_tools_start_with_finder_path() {
        configure_desktop();
        for program in ["whisper-cli", "whisper-server", "ollama"] {
            let output = command(program)
                .arg("--help")
                .output()
                .unwrap_or_else(|error| panic!("{program}: {error}"));
            assert!(output.status.success(), "{program}: {}", output.status);
        }
    }
    #[test]
    #[cfg(unix)]
    fn cancel_kills_the_running_child() {
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let started = std::time::Instant::now();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            flag.store(true, Ordering::SeqCst);
        });
        let mut child = Command::new("sleep").arg("10").spawn().unwrap();
        let error = wait(&mut child, &cancel).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(child.try_wait().unwrap().is_some(), "the child was reaped");
    }
    #[test]
    #[cfg(unix)]
    fn output_keeps_stdin_stdout_and_status() {
        let out = output(Command::new("cat").arg("-"), Some(b"hello")).unwrap();
        assert!(out.status.success());
        assert_eq!(out.stdout, b"hello");
    }
    #[test]
    #[cfg(unix)]
    fn status_lines_passes_each_stderr_line_as_it_comes() {
        let lines = std::sync::Mutex::new(vec![]);
        let script = "echo 'progress =  5%' >&2; echo out; echo 'progress = 100%' >&2";
        let status = status_lines(Command::new("sh").args(["-c", script]), &|line| {
            lines.lock().unwrap().push(line.to_string())
        })
        .unwrap();
        assert!(status.success());
        assert_eq!(
            *lines.lock().unwrap(),
            ["progress =  5%", "progress = 100%"]
        );
    }
    #[test]
    fn finder_path_gets_local_tools_without_replacing_existing_order() {
        let result = search_path(OsString::from("/usr/bin:/bin:/usr/sbin:/sbin"));
        let paths: Vec<_> = std::env::split_paths(&result).collect();
        assert_eq!(paths[0], PathBuf::from("/usr/bin"));
        assert!(paths.contains(&PathBuf::from("/opt/homebrew/bin")));
        assert!(paths.contains(&PathBuf::from("/usr/local/bin")));
        assert_eq!(search_path(result.clone()), result);
    }
}
