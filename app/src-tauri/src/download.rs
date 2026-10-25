//! Model downloads (SPEC 5.2): one file at a time to `.part`, resumed with Range, checked by
//! SHA-256. The app's only network use, so the URL and every redirect must be Hugging Face HTTPS.
use crate::catalog::ModelFile;
use sha2::{Digest, Sha256};
use std::{
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, RecvTimeoutError},
    },
    thread,
    time::{Duration, Instant},
};

const GB: u64 = 1_000_000_000;
const ORIGIN: &str = "https://huggingface.co";
const MAX_REDIRECTS: usize = 5;
/// How often a running download looks at `stop` and may report progress.
const TICK: Duration = Duration::from_millis(100);
#[cfg(not(test))]
const STALL: Duration = Duration::from_secs(30);
#[cfg(test)]
const STALL: Duration = Duration::from_secs(1);
#[cfg(not(test))]
const WAITS: [Duration; 3] = [
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
];
#[cfg(test)]
const WAITS: [Duration; 3] = [Duration::ZERO; 3];

pub enum Outcome {
    Done(PathBuf),
    Paused,
}

/// Why a download stopped; `Display` gives the SPEC 7.8 `dl_*` sentences.
#[derive(Debug, PartialEq)]
pub enum Failure {
    Disk { need: u64, free: u64 },
    Checksum,
    Network(String),
    Host(String),
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Disk { need, free } => write!(
                f,
                "This needs {} free; {} is available.",
                size(*need),
                size(*free)
            ),
            Self::Checksum => f.write_str("The file didn't match its fingerprint and was removed."),
            Self::Network(reason) => write!(f, "Download stopped: {reason}."),
            Self::Host(host) => write!(
                f,
                "Download stopped: {host} is not an allowed download host."
            ),
        }
    }
}

fn size(bytes: u64) -> String {
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else {
        format!("{} MB", (bytes + 500_000) / 1_000_000)
    }
}

/// How one request ended when it didn't finish the file.
enum Halt {
    Retry(String),
    Restart,
    Paused,
    Fail(Failure),
}

struct Job<'a> {
    file: &'a ModelFile,
    dir: &'a Path,
    part: PathBuf,
    url: String,
    checked: bool,
    agent: ureq::Agent,
    stop: &'a AtomicBool,
}

/// Download `file` into `dir` through `dir/{file}.part`, renamed only after its SHA-256 matched.
/// `base` stands in for `https://huggingface.co` in tests and turns the HTTPS and host checks off.
/// `progress` gets (bytes on disk, total) up to 10 times a second and once at the end.
pub fn fetch(
    file: &ModelFile,
    dir: &Path,
    base: Option<&str>,
    stop: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Outcome, Failure> {
    let part = dir.join(format!("{}.part", file.file));
    let result = run(file, dir, base, stop, progress);
    let on_disk = match &result {
        Ok(Outcome::Done(path)) => path,
        _ => &part,
    };
    progress(fs::metadata(on_disk).map_or(0, |m| m.len()), file.bytes);
    result
}

fn run(
    file: &ModelFile,
    dir: &Path,
    base: Option<&str>,
    stop: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Outcome, Failure> {
    let url = match base {
        Some(base) => file.url.replacen(ORIGIN, base, 1),
        None if allowed(&file.url) => file.url.clone(),
        None => return Err(Failure::Host(host(&file.url).into())),
    };
    fs::create_dir_all(dir).map_err(|e| io_failure("create", dir, e))?;
    let job = Job {
        file,
        dir,
        part: dir.join(format!("{}.part", file.file)),
        url,
        checked: base.is_none(),
        // Redirects are followed by hand so each hop is checked; no proxy from the environment.
        agent: ureq::Agent::config_builder()
            .proxy(None)
            .https_only(base.is_none())
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(STALL))
            .timeout_recv_response(Some(STALL))
            .build()
            .into(),
        stop,
    };
    let (mut retries, mut restarted) = (0, false);
    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(Outcome::Paused);
        }
        match job.attempt(progress) {
            Ok(()) => break,
            Err(Halt::Paused) => return Ok(Outcome::Paused),
            Err(Halt::Fail(failure)) => return Err(failure),
            Err(Halt::Restart) if !restarted => {
                restarted = true;
                fs::remove_file(&job.part).map_err(|e| io_failure("delete", &job.part, e))?;
            }
            Err(Halt::Restart) => {
                return Err(Failure::Network("the server refused to resume".into()));
            }
            Err(Halt::Retry(reason)) => {
                let Some(&wait) = WAITS.get(retries) else {
                    return Err(Failure::Network(reason));
                };
                retries += 1;
                if sleep(wait, stop) {
                    return Ok(Outcome::Paused);
                }
            }
        }
    }
    let target = dir.join(&file.file);
    fs::rename(&job.part, &target).map_err(|e| io_failure("move", &job.part, e))?;
    Ok(Outcome::Done(target))
}

impl Job<'_> {
    fn attempt(&self, progress: &mut dyn FnMut(u64, u64)) -> Result<(), Halt> {
        let total = self.file.bytes;
        let mut have = fs::metadata(&self.part).map_or(0, |m| m.len());
        if have > total {
            fs::remove_file(&self.part).map_err(|e| fail("delete", &self.part, e))?;
            have = 0;
        }
        let free = free_bytes(self.dir).map_err(|e| {
            Halt::Fail(Failure::Network(format!(
                "couldn't check free disk space ({e})"
            )))
        })?;
        space_ok(total - have, free).map_err(Halt::Fail)?;
        let response = self.open(have)?;
        let mut hasher = Sha256::new();
        let mut out = match response.status().as_u16() {
            200 => {
                have = 0;
                File::create(&self.part).map_err(|e| fail("write", &self.part, e))?
            }
            206 => {
                let expected = format!("bytes {have}-{}/{total}", total.saturating_sub(1));
                let range = response.headers().get("content-range");
                if range.and_then(|v| v.to_str().ok()) != Some(expected.as_str()) {
                    return Err(Halt::Fail(Failure::Network(
                        "the server sent the wrong part of the file".into(),
                    )));
                }
                // The digest covers the whole file, so the bytes already on disk go in first.
                let mut file = OpenOptions::new()
                    .read(true)
                    .append(true)
                    .create(true)
                    .open(&self.part)
                    .map_err(|e| fail("write", &self.part, e))?;
                io::copy(&mut file, &mut hasher).map_err(|e| fail("read", &self.part, e))?;
                file
            }
            416 if have == total => {
                let mut file = File::open(&self.part).map_err(|e| fail("read", &self.part, e))?;
                io::copy(&mut file, &mut hasher).map_err(|e| fail("read", &self.part, e))?;
                return self.verify(hasher);
            }
            416 => return Err(Halt::Restart),
            status => return Err(Halt::Retry(format!("the server answered {status}"))),
        };

        let (tx, rx) = mpsc::sync_channel(4);
        let mut body = response.into_body().into_reader();
        // ureq 3 has no per-read timeout, so a stalled read parks this thread until the
        // socket closes; the download itself moves on after STALL.
        thread::spawn(move || {
            let mut buf = vec![0; 64 * 1024];
            loop {
                let read = body.read(&mut buf).map(|n| buf[..n].to_vec());
                let end = !matches!(&read, Ok(chunk) if !chunk.is_empty());
                if tx.send(read).is_err() || end {
                    break;
                }
            }
        });
        let (mut idle, mut shown) = (Instant::now(), None::<Instant>);
        loop {
            if self.stop.load(Ordering::Relaxed) {
                return Err(Halt::Paused);
            }
            match rx.recv_timeout(TICK) {
                Ok(Ok(chunk)) if chunk.is_empty() => break,
                Ok(Ok(chunk)) => {
                    have += chunk.len() as u64;
                    // Longer than the catalog size, so it can't be the pinned file.
                    if have > total {
                        return self.verify(Sha256::new());
                    }
                    out.write_all(&chunk)
                        .map_err(|e| fail("write", &self.part, e))?;
                    hasher.update(&chunk);
                    idle = Instant::now();
                    if shown.is_none_or(|at| at.elapsed() >= TICK) {
                        progress(have, total);
                        shown = Some(idle);
                    }
                }
                Ok(Err(e)) => return Err(Halt::Retry(format!("the connection broke ({e})"))),
                Err(RecvTimeoutError::Timeout) if idle.elapsed() >= STALL => {
                    let secs = STALL.as_secs();
                    return Err(Halt::Retry(format!("no data for {secs} seconds")));
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        // A body that ends before the file is complete is a stall, never done.
        if have < total {
            return Err(Halt::Retry("the connection ended early".into()));
        }
        out.sync_all().map_err(|e| fail("write", &self.part, e))?;
        self.verify(hasher)
    }

    /// GET, with Range when resuming; each redirect hop re-sends it and, in the app, is checked.
    fn open(&self, have: u64) -> Result<ureq::http::Response<ureq::Body>, Halt> {
        let mut url = self.url.clone();
        for _ in 0..=MAX_REDIRECTS {
            let mut request = self.agent.get(&url);
            if have > 0 {
                request = request.header("Range", format!("bytes={have}-"));
            }
            let response = request
                .call()
                .map_err(|e| Halt::Retry(format!("the request failed ({e})")))?;
            if !response.status().is_redirection() {
                return Ok(response);
            }
            let location = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| Halt::Retry("a redirect had no address".into()))?;
            url = match location.strip_prefix('/') {
                Some(path) if !path.starts_with('/') => format!("{}/{path}", origin(&url)),
                _ => location.to_string(),
            };
            if self.checked && !allowed(&url) {
                return Err(Halt::Fail(Failure::Host(host(&url).into())));
            }
        }
        Err(Halt::Fail(Failure::Network("too many redirects".into())))
    }

    /// A mismatch deletes the `.part`, so Retry starts from 0.
    fn verify(&self, hasher: Sha256) -> Result<(), Halt> {
        if format!("{:x}", hasher.finalize()) == self.file.sha256 {
            return Ok(());
        }
        fs::remove_file(&self.part).map_err(|e| fail("delete", &self.part, e))?;
        Err(Halt::Fail(Failure::Checksum))
    }
}

fn io_failure(what: &str, path: &Path, e: io::Error) -> Failure {
    let name = path
        .file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy();
    Failure::Network(format!("couldn't {what} {name} ({e})"))
}

fn fail(what: &str, path: &Path, e: io::Error) -> Halt {
    Halt::Fail(io_failure(what, path, e))
}

/// Waits between retries; true when `stop` was set meanwhile.
fn sleep(time: Duration, stop: &AtomicBool) -> bool {
    let end = Instant::now() + time;
    while !stop.load(Ordering::Relaxed) {
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        thread::sleep(left.min(TICK));
    }
    stop.load(Ordering::Relaxed)
}

/// The authority of an absolute URL, userinfo and port included so `allowed` can refuse them.
fn host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.split(['/', '?', '#']).next().unwrap_or(rest)
}

fn origin(url: &str) -> &str {
    let end = url.find("://").map_or(0, |i| i + 3) + host(url).len();
    &url[..end]
}

/// HTTPS to `huggingface.co`, `*.huggingface.co` or `*.hf.co`, without userinfo or a port.
fn allowed(url: &str) -> bool {
    let host = host(url).to_ascii_lowercase();
    url.starts_with("https://")
        && !host.contains(['@', ':'])
        && (host == "huggingface.co"
            || host.ends_with(".huggingface.co")
            || host.ends_with(".hf.co"))
}

/// Free space must cover the bytes still to fetch plus 1 GB, checked before each request.
pub fn space_ok(remaining: u64, free: u64) -> Result<(), Failure> {
    let need = remaining.saturating_add(GB);
    if free >= need {
        Ok(())
    } else {
        Err(Failure::Disk { need, free })
    }
}

/// Free bytes on the volume holding `dir`, from the system's own tool.
#[cfg(not(windows))]
pub fn free_bytes(dir: &Path) -> io::Result<u64> {
    // Absolute path, so the desktop PATH can't swap in another `df`.
    let out = output(Command::new("/bin/df").arg("-Pk").arg(dir))?;
    // "Available" sits just before the "Capacity" percentage; a device name may contain spaces.
    let fields: Vec<&str> = out
        .lines()
        .nth(1)
        .unwrap_or("")
        .split_whitespace()
        .collect();
    fields
        .iter()
        .position(|f| f.ends_with('%'))
        .and_then(|i| fields.get(i.checked_sub(1)?))
        .and_then(|n| n.parse::<u64>().ok())
        .and_then(|n| n.checked_mul(1024))
        .ok_or_else(|| io::Error::other("df didn't report free space"))
}

/// Free bytes on the drive holding `dir`, from PowerShell's `DriveInfo`.
#[cfg(windows)]
pub fn free_bytes(dir: &Path) -> io::Result<u64> {
    use std::os::windows::process::CommandExt;
    use std::path::{Component, Prefix};
    // drive letters only; a models folder on a UNC share is refused.
    let drive = match dir.components().next() {
        Some(Component::Prefix(p)) => match p.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => Some(letter as char),
            _ => None,
        },
        _ => None,
    }
    .ok_or_else(|| io::Error::other("the models folder isn't on a drive letter"))?;
    // Only the drive letter reaches the script, so a folder name can't change the command.
    let script = format!("[IO.DriveInfo]::new('{drive}:').AvailableFreeSpace");
    // CREATE_NO_WINDOW: no console flashes over the app.
    let out = output(
        Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .creation_flags(0x0800_0000),
    )?;
    out.trim().parse().map_err(io::Error::other)
}

fn output(command: &mut Command) -> io::Result<String> {
    let out = command.output()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(io::Error::other(err.trim().to_string()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::FileKind;
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use std::{fs, thread};

    const SIZE: usize = 64 * 1024;
    const HALF: usize = SIZE / 2;

    fn data() -> Vec<u8> {
        (0..SIZE).map(|i| (i * 7 % 251) as u8).collect()
    }

    fn model() -> ModelFile {
        ModelFile {
            id: "tiny".into(),
            kind: FileKind::Speech,
            file: "tiny.bin".into(),
            label: "Tiny".into(),
            url: "https://huggingface.co/motet/test/resolve/abc/tiny.bin".into(),
            bytes: SIZE as u64,
            sha256: format!("{:x}", Sha256::digest(data())),
            on_disk: false,
        }
    }

    /// One scripted reply per connection: head, body bytes, then how long to keep the socket open.
    struct Reply(String, Vec<u8>, Duration);

    fn ok(body: &[u8]) -> Reply {
        let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n", body.len());
        Reply(head, body.to_vec(), Duration::ZERO)
    }

    fn partial(from: usize) -> Reply {
        let head = format!(
            "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {from}-{}/{SIZE}\r\nContent-Length: {}\r\n",
            SIZE - 1,
            SIZE - from
        );
        Reply(head, data()[from..].to_vec(), Duration::ZERO)
    }

    fn status(line: &str, extra: &str) -> Reply {
        let head = format!("HTTP/1.1 {line}\r\nContent-Length: 0\r\n{extra}");
        Reply(head, Vec::new(), Duration::ZERO)
    }

    /// Serves `replies` in order, one per connection, and records each request head.
    fn serve(replies: Vec<Reply>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        thread::spawn(move || {
            for (reply, stream) in replies.into_iter().zip(listener.incoming()) {
                let mut stream = stream.unwrap();
                let log = log.clone();
                thread::spawn(move || {
                    let mut head = Vec::new();
                    let mut byte = [0];
                    while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
                        head.push(byte[0]);
                    }
                    log.lock()
                        .unwrap()
                        .push(String::from_utf8_lossy(&head).into_owned());
                    let Reply(head, body, hold) = reply;
                    let _ = stream.write_all(format!("{head}Connection: close\r\n\r\n").as_bytes());
                    let _ = stream.write_all(&body);
                    let _ = stream.flush();
                    thread::sleep(hold);
                });
            }
        });
        (base, seen)
    }

    fn range(request: &str) -> Option<String> {
        request.lines().find_map(|l| {
            l.to_ascii_lowercase()
                .strip_prefix("range: ")
                .map(String::from)
        })
    }

    fn folder(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("motet-download-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn run(dir: &Path, base: &str) -> Result<Outcome, Failure> {
        fetch(
            &model(),
            dir,
            Some(base),
            &AtomicBool::new(false),
            &mut |_, _| {},
        )
    }

    fn part(dir: &Path) -> PathBuf {
        dir.join("tiny.bin.part")
    }

    fn assert_done(result: Result<Outcome, Failure>, dir: &Path) {
        match result {
            Ok(Outcome::Done(path)) => assert_eq!(path, dir.join("tiny.bin")),
            Ok(Outcome::Paused) => panic!("paused"),
            Err(e) => panic!("failed: {e}"),
        }
        assert_eq!(fs::read(dir.join("tiny.bin")).unwrap(), data());
        assert!(!part(dir).exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn fresh_download_verifies_and_renames() {
        let dir = folder("fresh");
        let (base, seen) = serve(vec![ok(&data())]);
        let mut calls = Vec::new();
        let result = fetch(
            &model(),
            &dir,
            Some(&base),
            &AtomicBool::new(false),
            &mut |n, t| calls.push((n, t)),
        );
        assert_eq!(calls.last(), Some(&(SIZE as u64, SIZE as u64)));
        assert!(calls.len() <= 3, "{calls:?}");
        let seen = seen.lock().unwrap();
        assert!(seen[0].starts_with("GET /motet/test/resolve/abc/tiny.bin "));
        assert_eq!(range(&seen[0]), None);
        assert_done(result, &dir);
    }

    #[test]
    fn resume_appends_on_matching_206() {
        let dir = folder("resume");
        fs::write(part(&dir), &data()[..1000]).unwrap();
        let (base, seen) = serve(vec![partial(1000)]);
        let result = run(&dir, &base);
        assert_eq!(
            range(&seen.lock().unwrap()[0]).as_deref(),
            Some("bytes=1000-")
        );
        assert_done(result, &dir);
    }

    #[test]
    fn wrong_content_range_fails() {
        let dir = folder("content-range");
        fs::write(part(&dir), &data()[..1000]).unwrap();
        // The whole file as a 206 although bytes 1000- were asked for.
        let (base, _) = serve(vec![partial(0)]);
        assert!(matches!(run(&dir, &base), Err(Failure::Network(_))));
        assert_eq!(fs::read(part(&dir)).unwrap(), &data()[..1000]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn ignored_range_200_restarts_from_zero() {
        let dir = folder("ignored-range");
        fs::write(part(&dir), vec![0xEE; 1000]).unwrap();
        let (base, seen) = serve(vec![ok(&data())]);
        let result = run(&dir, &base);
        assert_eq!(
            range(&seen.lock().unwrap()[0]).as_deref(),
            Some("bytes=1000-")
        );
        assert_done(result, &dir);
    }

    #[test]
    fn range_416_at_full_size_checks_hash() {
        let dir = folder("416-full");
        fs::write(part(&dir), data()).unwrap();
        let (base, seen) = serve(vec![status("416 Range Not Satisfiable", "")]);
        let result = run(&dir, &base);
        assert_eq!(
            range(&seen.lock().unwrap()[0]),
            Some(format!("bytes={SIZE}-"))
        );
        assert_done(result, &dir);
    }

    #[test]
    fn range_416_short_restarts_once() {
        let dir = folder("416-short");
        fs::write(part(&dir), &data()[..1000]).unwrap();
        let (base, seen) = serve(vec![status("416 Range Not Satisfiable", ""), ok(&data())]);
        let result = run(&dir, &base);
        let seen = seen.lock().unwrap();
        assert_eq!(range(&seen[0]).as_deref(), Some("bytes=1000-"));
        assert_eq!(range(&seen[1]), None);
        assert_done(result, &dir);

        let dir = folder("416-twice");
        fs::write(part(&dir), &data()[..1000]).unwrap();
        let mut short = ok(&data());
        short.1.truncate(1000);
        let (base, _) = serve(vec![
            status("416 Range Not Satisfiable", ""),
            short,
            status("416 Range Not Satisfiable", ""),
        ]);
        assert!(matches!(run(&dir, &base), Err(Failure::Network(_))));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn oversized_part_is_deleted() {
        let dir = folder("oversized");
        fs::write(part(&dir), vec![1; SIZE + 10]).unwrap();
        let (base, seen) = serve(vec![ok(&data())]);
        let result = run(&dir, &base);
        assert_eq!(range(&seen.lock().unwrap()[0]), None);
        assert_done(result, &dir);
    }

    #[test]
    fn stall_retries_then_fails_keeping_part() {
        let dir = folder("stall");
        let hold = STALL * 3;
        let mut first = ok(&data());
        first.1.truncate(HALF);
        first.2 = hold;
        let mut replies = vec![first];
        for _ in 0..3 {
            let mut again = partial(HALF);
            again.1.clear();
            again.2 = hold;
            replies.push(again);
        }
        let (base, seen) = serve(replies);
        let result = run(&dir, &base);
        assert!(matches!(result, Err(Failure::Network(_))));
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 4);
        assert_eq!(range(&seen[3]), Some(format!("bytes={HALF}-")));
        assert_eq!(fs::read(part(&dir)).unwrap(), &data()[..HALF]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn short_body_is_not_done() {
        let dir = folder("short");
        let mut short = ok(&data());
        short.1.truncate(HALF);
        let (base, seen) = serve(vec![short, partial(HALF)]);
        let result = run(&dir, &base);
        assert_eq!(
            range(&seen.lock().unwrap()[1]),
            Some(format!("bytes={HALF}-"))
        );
        assert_done(result, &dir);
    }

    #[test]
    fn wrong_hash_deletes_part() {
        let dir = folder("hash");
        let (base, _) = serve(vec![ok(&[0; SIZE])]);
        assert_eq!(run(&dir, &base).err(), Some(Failure::Checksum));
        assert!(!part(&dir).exists());
        assert!(!dir.join("tiny.bin").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn pause_keeps_part() {
        let dir = folder("pause");
        let mut slow = ok(&data());
        slow.1.truncate(HALF);
        slow.2 = STALL * 3;
        let (base, _) = serve(vec![slow]);
        let stop = AtomicBool::new(false);
        let result = fetch(&model(), &dir, Some(&base), &stop, &mut |n, _| {
            if n > 0 {
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        });
        assert!(matches!(result, Ok(Outcome::Paused)));
        let kept = fs::read(part(&dir)).unwrap();
        assert!(!kept.is_empty() && data().starts_with(&kept));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn redirect_resends_range() {
        let dir = folder("redirect");
        fs::write(part(&dir), &data()[..1000]).unwrap();
        let (cdn, cdn_seen) = serve(vec![partial(1000)]);
        let moved = status("302 Found", &format!("Location: {cdn}/cdn/tiny.bin\r\n"));
        let (base, _) = serve(vec![moved]);
        let result = run(&dir, &base);
        let cdn_seen = cdn_seen.lock().unwrap();
        assert!(cdn_seen[0].starts_with("GET /cdn/tiny.bin "));
        assert_eq!(range(&cdn_seen[0]).as_deref(), Some("bytes=1000-"));
        assert_done(result, &dir);
    }

    #[test]
    fn redirect_to_other_host_refused() {
        for url in [
            "https://huggingface.co/ggml-org/whisper/resolve/abc/model.bin",
            "https://cdn-lfs.huggingface.co/repos/ab/cd/file",
            "https://cas-bridge.xethub.hf.co/xet-bridge-us/abc?X-Amz=1",
            "https://HuggingFace.co/x",
        ] {
            assert!(allowed(url), "{url}");
        }
        for url in [
            "http://huggingface.co/x",
            "https://evilhuggingface.co/x",
            "https://huggingface.co.evil.com/x",
            "https://evilhf.co/x",
            "https://huggingface.co@evil.com/x",
            "https://huggingface.co:8443/x",
            "https://127.0.0.1/x",
            "ftp://huggingface.co/x",
            "huggingface.co/x",
        ] {
            assert!(!allowed(url), "{url}");
        }
        // The app's own path refuses a catalog URL off the allowlist before any request.
        let mut off = model();
        off.url = "https://example.com/tiny.bin".into();
        let dir = folder("host");
        let result = fetch(&off, &dir, None, &AtomicBool::new(false), &mut |_, _| {});
        assert_eq!(result.err(), Some(Failure::Host("example.com".into())));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn full_disk_keeps_part_and_fails() {
        assert_eq!(
            space_ok(10, GB + 9),
            Err(Failure::Disk {
                need: GB + 10,
                free: GB + 9
            })
        );
        assert_eq!(space_ok(10, GB + 10), Ok(()));
        assert_eq!(
            Failure::Disk {
                need: 5_000_000_000,
                free: 1_260_000_000
            }
            .to_string(),
            "This needs 5.0 GB free; 1.3 GB is available."
        );
        assert_eq!(
            Failure::Disk {
                need: GB,
                free: 420_000_000
            }
            .to_string(),
            "This needs 1.0 GB free; 420 MB is available."
        );
        assert!(free_bytes(&std::env::temp_dir()).unwrap() > 0);
    }

    #[test]
    fn failures_read_as_the_spec_sentences() {
        assert_eq!(
            Failure::Checksum.to_string(),
            "The file didn't match its fingerprint and was removed."
        );
        assert_eq!(
            Failure::Network("no data for 30 seconds".into()).to_string(),
            "Download stopped: no data for 30 seconds."
        );
        assert_eq!(
            Failure::Host("evil.com".into()).to_string(),
            "Download stopped: evil.com is not an allowed download host."
        );
    }
}
