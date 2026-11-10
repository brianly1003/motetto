//! The app's error log, `motetto.log` in the folder Settings › About opens. Each entry has the
//! time (UTC), the Rust file and line that saw the failure and the message; an Objective-C
//! exception adds the method that threw and Apple's call stack, and a panic adds a backtrace.
//! Only error text is written here, never transcript text.
use std::{
    fs::OpenOptions,
    io::Write,
    panic::Location,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

static LOG: OnceLock<PathBuf> = OnceLock::new();
/// The log moves to `motetto.1.log` past this size, so two files at most.
const ROTATE_AT: u64 = 5_000_000;

/// Opens the log in `dir` and records panics there too.
pub fn start(dir: PathBuf) {
    let _ = std::fs::create_dir_all(&dir);
    let _ = LOG.set(dir.join("motetto.log"));
    write(&format!("Motetto {} started", env!("CARGO_PKG_VERSION")));
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let at = info.location().map(place).unwrap_or_default();
        let trace = std::backtrace::Backtrace::force_capture();
        write(&format!("{at} panic: {message}\n{trace}"));
        previous(info);
    }));
}

/// Logs a failure with the file and line of the call.
#[track_caller]
pub fn error(message: &str) {
    write(&format!("{} {message}", place(Location::caller())));
}

/// Runs `f`, turning an Objective-C exception into an error that names where it was thrown:
/// "reason, in -[Class method:] (file.rs:line)". The full Apple call stack goes to the log.
#[cfg(target_os = "macos")]
#[track_caller]
pub fn catch<R>(f: impl FnOnce() -> R) -> Result<R, String> {
    let at = place(Location::caller());
    objc2::exception::catch(std::panic::AssertUnwindSafe(f)).map_err(|e| {
        let (what, stack) = describe(e.as_deref());
        write(&format!("{at} {what}\n{}", stack.join("\n")));
        format!("{what} ({at})")
    })
}

#[cfg(target_os = "macos")]
fn describe(e: Option<&objc2::exception::Exception>) -> (String, Vec<String>) {
    use objc2_foundation::NSException;
    let Some(ex) = e.and_then(|e| e.downcast_ref::<NSException>()) else {
        return (format!("{e:?}"), Vec::new());
    };
    let stack: Vec<String> = ex
        .callStackSymbols()
        .iter()
        .map(|s| s.to_string())
        .collect();
    let reason = ex.reason().map(|r| r.to_string()).unwrap_or_default();
    let mut what = format!("{}: {reason}", ex.name());
    if let Some(method) = thrower(&stack) {
        what.push_str(&format!(", in {method}"));
    }
    (what, stack)
}

/// The first Objective-C method on the stack outside the raising machinery: the API that threw.
fn thrower(stack: &[String]) -> Option<&str> {
    stack
        .iter()
        .filter(|frame| !frame.contains("CoreFoundation") && !frame.contains("libobjc"))
        .find_map(|frame| {
            let start = frame.find("-[").or_else(|| frame.find("+["))?;
            let end = start + frame[start..].find(']')?;
            Some(&frame[start..=end])
        })
}

fn place(at: &Location) -> String {
    let file = at.file().rsplit(['/', '\\']).next().unwrap_or_default();
    format!("{file}:{}", at.line())
}

fn write(entry: &str) {
    eprintln!("Motetto: {entry}");
    let Some(path) = LOG.get() else { return };
    static LOCK: Mutex<()> = Mutex::new(());
    let _held = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if std::fs::metadata(path).is_ok_and(|m| m.len() > ROTATE_AT) {
        let _ = std::fs::rename(path, path.with_extension("1.log"));
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let _ = writeln!(file, "{} {entry}", utc(secs));
    }
}

/// "YYYY-MM-DD hh:mm:ssZ" from Unix seconds (days to civil date, proleptic Gregorian).
fn utc(secs: u64) -> String {
    let (days, rest) = ((secs / 86_400) as i64, secs % 86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let (h, m, s) = (rest / 3600, rest % 3600 / 60, rest % 60);
    format!("{year:04}-{month:02}-{day:02} {h:02}:{m:02}:{s:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_dates() {
        assert_eq!(utc(0), "1970-01-01 00:00:00Z");
        assert_eq!(utc(951_782_400), "2000-02-29 00:00:00Z");
        assert_eq!(utc(1_700_000_000), "2023-11-14 22:13:20Z");
    }

    #[test]
    fn thrower_skips_the_raising_frames() {
        let stack = [
            "0   CoreFoundation  0x1 __exceptionPreprocess + 176",
            "1   libobjc.A.dylib 0x2 objc_exception_throw + 60",
            "2   CoreFoundation  0x3 +[NSException raise:format:] + 128",
            "3   AVFAudio        0x4 AVAE_RaiseException(NSString*, ...) + 56",
            "4   AVFAudio        0x5 -[AVAudioPlayerNode playAtTime:] + 312",
            "5   motetto-app     0x6 motetto_app::playback_macos::Engine::try_play + 9",
        ]
        .map(String::from);
        assert_eq!(thrower(&stack), Some("-[AVAudioPlayerNode playAtTime:]"));
        assert_eq!(thrower(&stack[..4]), None);
    }
}
