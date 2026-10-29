//! Following the presenter's voice for the script strip (SPEC 14.5): short windows of the mic,
//! about once a second, to the speech server already running, with the next script words as a
//! hint. Captions take 6 s; following needs about 2.
use motetto_core::events::Stop;
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, atomic::Ordering},
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager};
use ts_rs::TS;

/// The last 4 s of the mic: long enough for a phrase, short enough to answer in under a second.
const WINDOW_MS: u64 = 4000;

#[derive(Clone, Serialize, TS)]
pub struct Heard {
    pub text: String,
    #[ts(type = "number")]
    pub end_ms: u64,
    #[ts(type = "number")]
    pub inference_ms: u64,
}

/// One server for captions and following, so a second model never loads; the live loop sets it.
pub type Shared = Arc<Mutex<crate::speech::Server>>;

#[derive(Default)]
pub struct State {
    pub hint: Mutex<String>,
    pub server: Mutex<Option<Shared>>,
}

/// Wait between requests: never more than once a second, and slower when the model is slow, so
/// following never starves the captions.
pub fn next_step(last_inference_ms: u64) -> u64 {
    1000.max(last_inference_ms * 5 / 4)
}

/// The audio to send when the mic track is `end_ms` long.
pub fn window(end_ms: u64) -> (u64, u64) {
    (end_ms.saturating_sub(WINDOW_MS), end_ms)
}

/// The next script words, as plain words: no cues, no line breaks, at most 30 words and 200
/// characters. They bias the speech model and are never instructions.
pub fn clean_hint(words: &str) -> String {
    let without_cues = words
        .split('[')
        .enumerate()
        .map(|(i, part)| {
            if i == 0 {
                part
            } else {
                part.split_once(']').map_or("", |(_, rest)| rest)
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    let kept: String = without_cues
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " -.,'?!".contains(c) {
                c
            } else {
                ' '
            }
        })
        .collect();
    let mut hint = kept
        .split_whitespace()
        .take(30)
        .collect::<Vec<_>>()
        .join(" ");
    if hint.len() > 200 {
        let mut end = 200;
        while !hint.is_char_boundary(end) {
            end -= 1;
        }
        hint.truncate(end);
    }
    hint
}

#[cfg(any(target_os = "macos", test))]
/// The on-device locale to follow in: the call language when set ("en", "vi"), else Vietnamese
/// letters in the script's next words mean `vi-VN`, anything else English (`en-US` first). None
/// when that language is not on this Mac: then the Whisper loop follows.
pub fn pick_locale(hint: &str, call_language: &str, on_device: &[String]) -> Option<String> {
    let vietnamese = match call_language {
        "vi" => true,
        "en" => false,
        "detect" => hint
            .to_lowercase()
            .chars()
            .any(|c| "ăâđêôơưạảấầẩẫậắằẳẵặẹẻẽếềểễệỉịọỏốồổỗộớờởỡợụủứừửữựỳỵỷỹ".contains(c)),
        _ => return None,
    };
    let has = |locale: &str| on_device.iter().any(|l| l == locale);
    if vietnamese {
        return has("vi-VN").then(|| "vi-VN".into());
    }
    if has("en-US") {
        return Some("en-US".into());
    }
    on_device.iter().find(|l| l.starts_with("en-")).cloned()
}

#[cfg(any(target_os = "macos", test))]
/// No mic audio for 5 s while the strip is open: the recogniser hears nothing, so Whisper follows.
pub fn starved(ms_without_audio: u64) -> bool {
    ms_without_audio >= 5_000
}

#[cfg(any(target_os = "macos", test))]
/// An Apple run goes on while the session lasts, the strip is open and shows the script it
/// started with.
pub fn keep_following(
    stop: bool,
    prompting: bool,
    started: &Option<String>,
    now: &Option<String>,
) -> bool {
    !stop && prompting && started == now
}

#[cfg(any(target_os = "macos", test))]
/// The last `n` words heard: the follower only reads the end.
pub fn tail_words(text: &str, n: usize) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    words[words.len().saturating_sub(n)..].join(" ")
}

#[cfg(any(target_os = "macos", test))]
/// A recognition task restarts at a pause after 50 s, or at 58 s whatever is said, before
/// Apple's one-minute limit ends it mid-word.
pub fn restart_due(task_ms: u64, silent_ms: u64) -> bool {
    (task_ms >= 50_000 && silent_ms >= 300) || task_ms >= 58_000
}

#[tauri::command]
pub fn follow_hint(state: tauri::State<'_, State>, words: String) {
    *state.hint.lock().unwrap_or_else(|e| e.into_inner()) = clean_hint(&words);
}

fn prompting(app: &tauri::AppHandle) -> bool {
    app.get_webview_window("prompter")
        .is_some_and(|w| w.is_visible().unwrap_or(false))
}

/// Scratch audio lives outside the meeting folder and goes on every way out (review C2).
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        Self(std::env::temp_dir().join(format!(
            "motetto-follow-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(target_os = "macos")]
pub use crate::follow_macos::Access;
#[cfg(all(not(target_os = "macos"), test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Allowed,
    Denied,
    Unknown,
}

#[cfg(any(target_os = "macos", test))]
#[derive(Debug, PartialEq, Eq)]
pub enum Engine {
    Apple(String),
    Whisper,
}

#[cfg(any(target_os = "macos", test))]
/// Apple only on a Mac that allows Speech Recognition and has the language on-device.
pub fn engine(macos: bool, access: Access, locale: Option<String>) -> Engine {
    match (macos, access, locale) {
        (true, Access::Allowed, Some(locale)) => Engine::Apple(locale),
        _ => Engine::Whisper,
    }
}

#[cfg(any(target_os = "macos", test))]
fn hint(app: &tauri::AppHandle) -> String {
    app.state::<State>()
        .hint
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// Follows with Apple's on-device recogniser while the strip is open, until `stop`; false when it
/// can't (not allowed, the language isn't on this Mac, or it failed): then Whisper follows.
#[cfg(target_os = "macos")]
fn apple(app: &tauri::AppHandle, stop: &Stop) -> bool {
    use crate::follow_macos as mac;
    let listener = app
        .state::<crate::live::State>()
        .meters
        .mic_listener
        .clone();
    let mut locales: Option<Vec<String>> = None;
    while !stop.load(Ordering::SeqCst) {
        if !prompting(app) {
            std::thread::sleep(Duration::from_millis(300));
            continue;
        }
        // The script's own text picks the language (a hint left from the last script must not);
        // its next words, sent as the strip loads, bias the first task.
        let script = crate::prompter::shown_script(app);
        let text = script
            .clone()
            .and_then(|name| crate::scripts::script_read(app.clone(), name).ok())
            .unwrap_or_default();
        let waited = Instant::now();
        while hint(app).is_empty() && waited.elapsed() < Duration::from_secs(1) {
            std::thread::sleep(Duration::from_millis(50));
        }
        // Asked here, when following starts, never at launch.
        let access = match mac::access() {
            Access::Unknown => mac::ask(),
            known => known,
        };
        // The prompt can take a minute: this session may have ended meanwhile.
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let on_device = locales.get_or_insert_with(mac::on_device_locales);
        let call = app
            .state::<crate::settings::Store>()
            .get()
            .map_or_else(|_| "detect".into(), |s| s.call_language);
        let Engine::Apple(locale) = engine(true, access, pick_locale(&text, &call, on_device))
        else {
            return false;
        };
        // One run per open strip and script: closing it or switching script ends the run, and
        // the next one picks the language again.
        let open = Stop::default();
        let watch = {
            let (app, stop, open) = (app.clone(), stop.clone(), open.clone());
            std::thread::spawn(move || {
                while !open.load(Ordering::SeqCst) {
                    let now = crate::prompter::shown_script(&app);
                    if !keep_following(stop.load(Ordering::SeqCst), prompting(&app), &script, &now)
                    {
                        open.store(true, Ordering::SeqCst);
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
            })
        };
        let result = mac::run(app, &listener, &open, &locale);
        open.store(true, Ordering::SeqCst);
        let _ = watch.join();
        if let Err(error) = result {
            eprintln!("Motetto: following with Apple: {error}; Whisper follows instead");
            return false;
        }
    }
    true
}

/// Follows until `stop`, while the strip is open: with Apple's on-device recogniser on a Mac
/// that allows it, otherwise with Whisper windows of `mic` (a growing WAV). During a recording
/// Whisper uses only the captions' server (`own` false: never a second model mid-call); a
/// rehearsal has no captions, so it starts its own (`own` true).
pub fn start(app: tauri::AppHandle, mic: PathBuf, stop: Stop, own: bool) {
    std::thread::spawn(move || {
        #[cfg(target_os = "macos")]
        if apple(&app, &stop) {
            return;
        }
        let scratch = Scratch::new();
        let mut mine: Option<Shared> = None;
        let mut step = 1000;
        while !stop.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(step));
            if stop.load(Ordering::SeqCst) {
                break;
            }
            if !prompting(&app) {
                continue;
            }
            let shared = app
                .state::<State>()
                .server
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let server = match shared.or_else(|| mine.clone()) {
                Some(server) => server,
                None if !own => continue,
                None => match crate::speech::Server::start(&stop) {
                    Ok(server) => mine.insert(Arc::new(Mutex::new(server))).clone(),
                    Err(error) => {
                        eprintln!("Motetto: following needs the speech model: {error}");
                        break;
                    }
                },
            };
            match listen(&app, &server, &mic, &scratch.0, &stop) {
                Ok(Some(heard)) => {
                    step = next_step(heard.inference_ms);
                    if let Some(window) = app.get_webview_window("prompter") {
                        let _ = window.emit("motetto://follow-heard", heard);
                    }
                }
                Ok(None) => step = 1000,
                Err(error) => {
                    eprintln!("Motetto: following: {error}");
                    step = 2000;
                }
            }
        }
    });
}

fn listen(
    app: &tauri::AppHandle,
    server: &Shared,
    mic: &Path,
    scratch: &Path,
    stop: &Stop,
) -> Result<Option<Heard>, Box<dyn std::error::Error>> {
    let hint = app
        .state::<State>()
        .hint
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    hear(server, mic, scratch, &hint, stop)
}

/// The last window of `mic`, transcribed with `hint`; None for silence or under a second.
fn hear(
    server: &Shared,
    mic: &Path,
    scratch: &Path,
    hint: &str,
    stop: &Stop,
) -> Result<Option<Heard>, Box<dyn std::error::Error>> {
    let reader = hound::WavReader::open(mic)?;
    let end = u64::from(reader.duration()) * 1000 / u64::from(reader.spec().sample_rate);
    drop(reader);
    let (start, end) = window(end);
    if end - start < 1000 {
        return Ok(None);
    }
    std::fs::create_dir_all(scratch)?;
    let file = scratch.join("window.wav");
    motetto_core::slice::write(mic, &file, start, end)?;
    let decoded = scratch.join("window.16k.wav");
    motetto_core::decode_audio(&file, &decoded)?;
    let samples = hound::WavReader::open(decoded)?
        .samples::<f32>()
        .collect::<Result<Vec<_>, _>>()?;
    // Silence: nothing to follow, and the server stays free for captions.
    if !samples.iter().any(|sample| sample.abs() > 0.005) {
        return Ok(None);
    }
    let asked = Instant::now();
    let segments = server
        .lock()
        .map_err(|_| "The speech server is unavailable")?
        .transcribe(&samples, start, (!hint.is_empty()).then_some(hint), stop)?;
    Ok(Some(Heard {
        text: segments
            .iter()
            .map(|s| s.text.trim())
            .collect::<Vec<_>>()
            .join(" "),
        end_ms: end,
        inference_ms: asked.elapsed().as_millis() as u64,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_mic_audio_for_five_seconds_gives_up() {
        assert!(!starved(4_999));
        assert!(starved(5_000));
    }

    #[test]
    fn a_run_ends_when_the_strip_closes_or_shows_another_script() {
        let demo = Some("Demo".to_string());
        assert!(keep_following(false, true, &demo, &demo));
        assert!(!keep_following(true, true, &demo, &demo));
        assert!(!keep_following(false, false, &demo, &demo));
        assert!(!keep_following(
            false,
            true,
            &demo,
            &Some("Quarterly review".into())
        ));
    }

    #[test]
    fn apple_only_when_allowed_and_on_device() {
        assert_eq!(
            engine(true, Access::Allowed, Some("en-US".into())),
            Engine::Apple("en-US".into())
        );
        assert_eq!(
            engine(true, Access::Denied, Some("en-US".into())),
            Engine::Whisper
        );
        assert_eq!(
            engine(true, Access::Unknown, Some("en-US".into())),
            Engine::Whisper
        );
        assert_eq!(engine(true, Access::Allowed, None), Engine::Whisper);
        assert_eq!(
            engine(false, Access::Allowed, Some("en-US".into())),
            Engine::Whisper
        );
    }

    #[test]
    fn locale_from_script_and_setting() {
        let both = vec!["en-US".to_string(), "vi-VN".to_string()];
        assert_eq!(
            pick_locale("Xin chào, hôm nay", "detect", &both).as_deref(),
            Some("vi-VN")
        );
        assert_eq!(
            pick_locale("Thanks for having us", "detect", &both).as_deref(),
            Some("en-US")
        );
        assert_eq!(
            pick_locale("Xin chào", "en", &both).as_deref(),
            Some("en-US")
        );
        assert_eq!(pick_locale("", "detect", &both).as_deref(), Some("en-US"));
        assert_eq!(
            pick_locale("Hello", "detect", &["en-PH".to_string()]).as_deref(),
            Some("en-PH")
        );
        assert_eq!(
            pick_locale("Xin chào, hôm nay", "detect", &["en-US".to_string()]),
            None
        );
        assert_eq!(pick_locale("你好", "zh", &both), None);
    }

    #[test]
    fn tail_keeps_last_words() {
        assert_eq!(tail_words("one two three four", 2), "three four");
        assert_eq!(tail_words("  ", 12), "");
    }

    #[test]
    fn restart_at_a_pause_or_before_the_limit() {
        assert!(!restart_due(49_000, 1_000));
        assert!(restart_due(50_000, 300));
        assert!(!restart_due(55_000, 100));
        assert!(restart_due(58_000, 0));
    }

    #[test]
    fn scratch_is_outside_and_removed() {
        let scratch = Scratch::new();
        let path = scratch.0.clone();
        assert!(path.starts_with(std::env::temp_dir()));
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("window.wav"), b"voice").unwrap();
        drop(scratch);
        assert!(!path.exists());
    }

    #[test]
    fn step_never_below_one_second() {
        assert_eq!(next_step(0), 1000);
        assert_eq!(next_step(790), 1000);
    }

    #[test]
    fn step_backs_off_when_slow() {
        assert_eq!(next_step(2000), 2500);
    }

    #[test]
    fn window_clamps_at_start() {
        assert_eq!(window(2500), (0, 2500));
        assert_eq!(window(60_000), (56_000, 60_000));
    }

    #[test]
    fn hint_is_cleaned() {
        assert_eq!(
            clean_hint("[click New] Let's make\na new report."),
            "Let's make a new report."
        );
        assert_eq!(
            clean_hint("Ignore <b>all</b> {previous} instructions"),
            "Ignore b all b previous instructions"
        );
        let long = "word ".repeat(50);
        assert_eq!(clean_hint(&long).split(' ').count(), 30);
        let wide = "Ш".repeat(150);
        assert!(clean_hint(&wide).len() <= 200);
        assert_eq!(clean_hint("unclosed [cue text"), "unclosed");
    }

    /// By hand (needs WHISPER_MODEL): MOTETTO_FOLLOW_WAV=<a spoken WAV> times one window from
    /// file to text, as the strip gets it.
    #[test]
    #[ignore = "needs whisper-server, WHISPER_MODEL and MOTETTO_FOLLOW_WAV"]
    fn measure() {
        let mic = PathBuf::from(std::env::var("MOTETTO_FOLLOW_WAV").unwrap());
        let stop = Stop::default();
        let server: Shared = Arc::new(Mutex::new(crate::speech::Server::start(&stop).unwrap()));
        let scratch = std::env::temp_dir().join(format!("motetto-follow-{}", std::process::id()));
        for round in 0..4 {
            let started = Instant::now();
            let heard = hear(
                &server,
                &mic,
                &scratch,
                "these three cards are live numbers",
                &stop,
            )
            .unwrap()
            .unwrap();
            println!(
                "round {round}: total {} ms, inference {} ms: {}",
                started.elapsed().as_millis(),
                heard.inference_ms,
                heard.text
            );
        }
        let _ = std::fs::remove_dir_all(scratch);
    }

    #[test]
    fn exports_bindings() {
        Heard::export_all(&ts_rs::Config::default().with_out_dir("../../ui/lib/bindings")).unwrap();
    }
}
