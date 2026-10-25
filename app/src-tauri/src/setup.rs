//! First-run setup (SPEC 5.2): speech model, voice detection, speed test, then the notes model,
//! one file at a time on one background thread that keeps going with the window closed.
use crate::{
    catalog::{Catalog, FileKind, ModelFile, SpeedResult, TierId, catalog},
    download::{self, Failure, Outcome},
    library_commands::Library,
    model_store,
    settings::{Settings, Store},
};
use motetto_core::events::{Event, Phase, SetupState, Stop};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Mutex, atomic::Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{Emitter, Manager};
use ts_rs::TS;

/// The bundled sample's length; the speed test divides by it.
const SAMPLE_SECONDS: f32 = 10.0;
/// Live transcription of two tracks needs this with room for the notes model (SPEC 5.2).
const LIVE_RTF: f32 = 0.5;
const TRY_WAIT: &str = "Available when the speech model has downloaded.";
const TRY_SCREEN: &str = "Allow Screen & System Audio Recording in step 2 to test.";
const BUSY: &str = "Wait until the current recording or import finishes.";

/// The order of SPEC 5.2: speech, voice detection, the speed test, then notes. A slow result asks
/// before the notes model downloads, except on Light, which continues without live captions.
pub fn next(state: &SetupState, installed: &dyn Fn(&str) -> bool) -> Phase {
    let cat = catalog();
    let tier = cat.tier(state.tier);
    for id in [&tier.speech, &cat.vad] {
        if !installed(id) {
            return Phase::Downloading { id: id.clone() };
        }
    }
    match state.speed.as_ref().filter(|s| s.tier == state.tier) {
        None => return Phase::SpeedTest,
        Some(speed) if !speed.keeps_up && state.tier != TierId::Light && !state.keep_slow => {
            return Phase::AwaitingAnswer {
                offer: state.tier.down(),
            };
        }
        Some(_) => {}
    }
    if !installed(&tier.notes) {
        return Phase::Downloading {
            id: tier.notes.clone(),
        };
    }
    Phase::Done
}

/// The saved part of setup. A slow result is saved together with its question
/// (`record_speed`), so a slow result without one was answered "Keep".
pub fn restore(s: &Settings, suggested: TierId) -> SetupState {
    let tier = s.tier.unwrap_or(suggested);
    SetupState {
        tier,
        phase: Phase::Idle,
        files: Vec::new(),
        speed: s.speed_test.clone(),
        keep_slow: s
            .speed_test
            .as_ref()
            .is_some_and(|r| r.tier == tier && !r.keeps_up)
            && s.slow_question.is_none(),
    }
}

pub fn speed_result(tier: TierId, seconds: f32) -> SpeedResult {
    let rtf = seconds / SAMPLE_SECONDS;
    SpeedResult {
        tier,
        rtf,
        keeps_up: rtf <= LIVE_RTF,
        measured_at: now(),
    }
}

/// Stores a speed test with its question; Light that can't keep up transcribes after Stop.
pub fn record_speed(s: &mut Settings, result: SpeedResult) {
    // Settings › Models can test another tier; that result isn't this setup's answer.
    if s.tier.is_some_and(|tier| tier != result.tier) {
        return;
    }
    s.slow_question =
        (!result.keeps_up && result.tier != TierId::Light).then(|| result.tier.down());
    s.live_transcription = result.keeps_up || result.tier != TierId::Light;
    s.speed_test = Some(result);
}

fn installed(cat: &Catalog, dir: &Path, adopted: &BTreeMap<String, String>, id: &str) -> bool {
    cat.file(id)
        .is_some_and(|f| model_store::path(dir, adopted, f).is_some())
}

/// The engines' paths, from the chosen ids and only for a verified file (downloaded through its
/// SHA-256, or adopted): an exact-size file in the models folder or at its adopted path.
/// Empty means not ready, so a vanished or deleted file clears its path.
pub fn refresh_paths(s: &mut Settings, cat: &Catalog, dir: &Path) {
    let resolve = |id: &str| {
        cat.file(id)
            .and_then(|f| model_store::path(dir, &s.adopted, f))
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let (speech, vad, notes) = (
        resolve(&s.speech_model),
        resolve(&cat.vad),
        resolve(&s.notes_model),
    );
    s.speech_path = speech;
    s.vad_path = vad;
    s.notes_path = notes;
}

/// A tier's files that are verified become the models in use; the old ones stay in use until then.
fn choose_tier_models(s: &mut Settings, cat: &Catalog, dir: &Path, tier: TierId) {
    let tier = cat.tier(tier);
    if installed(cat, dir, &s.adopted, &tier.speech) {
        s.speech_model = tier.speech.clone();
    }
    if installed(cat, dir, &s.adopted, &tier.notes) {
        s.notes_model = tier.notes.clone();
    }
    refresh_paths(s, cat, dir);
}

/// (id, bytes on disk, total) for a tier's files, read from disk: a verified file counts in
/// full, an unfinished one by its `.part`.
fn files(
    cat: &Catalog,
    dir: &Path,
    adopted: &BTreeMap<String, String>,
    tier: TierId,
) -> Vec<(String, u64, u64)> {
    let tier = cat.tier(tier);
    [&tier.speech, &cat.vad, &tier.notes]
        .into_iter()
        .filter_map(|id| cat.file(id))
        .map(|f| {
            let bytes = if model_store::path(dir, adopted, f).is_some() {
                f.bytes
            } else {
                std::fs::metadata(part(dir, f)).map_or(0, |m| m.len().min(f.bytes))
            };
            (f.id.clone(), bytes, f.bytes)
        })
        .collect()
}

fn part(dir: &Path, file: &ModelFile) -> PathBuf {
    dir.join(format!("{}.part", file.file))
}

enum Got {
    Downloaded,
    Adopted(PathBuf),
    Paused,
}

/// One file: a whisper file the command line already fetched is used where it is when it
/// matches (`~/.cache/whisper`), otherwise it is downloaded and checked.
fn obtain(
    file: &ModelFile,
    dir: &Path,
    cache: Option<&Path>,
    stop: &Stop,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Got, Failure> {
    if file.kind != FileKind::Notes
        && let Some(found) = cache.and_then(|c| model_store::adopt(file, &[c.join(&file.file)]))
    {
        progress(file.bytes, file.bytes);
        return Ok(Got::Adopted(found));
    }
    match download::fetch(file, dir, None, stop, progress)? {
        Outcome::Done(_) => Ok(Got::Downloaded),
        Outcome::Paused => Ok(Got::Paused),
    }
}

fn now() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

fn scratch(name: &str) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(format!(
        "motetto-{name}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// 16 kHz mono samples, whatever the WAV holds.
fn samples(source: &Path, scratch: &Path) -> Result<Vec<f32>, String> {
    let decoded = scratch.join("decoded.16k.wav");
    motetto_core::decode_audio(source, &decoded).map_err(|e| e.to_string())?;
    hound::WavReader::open(decoded)
        .map_err(|e| e.to_string())?
        .samples::<f32>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

/// The runner's next step: queued files first, then the tier's pipeline if it was asked for.
fn plan(inner: &Inner, s: &Settings, have: &dyn Fn(&str) -> bool) -> Phase {
    if inner.paused {
        Phase::Paused
    } else if let Some(id) = inner.queue.first() {
        Phase::Downloading { id: id.clone() }
    } else if inner.tier_run && s.tier.is_some() {
        next(&restore(s, TierId::Balanced), have)
    } else {
        Phase::Idle
    }
}

/// A Settings download. A paused first-run download stays paused: the run that fetches this
/// file stops when the queue is empty.
fn enqueue(inner: &mut Inner, id: String) {
    if !inner.queue.contains(&id) {
        inner.queue.push(id);
    }
    if inner.paused {
        inner.paused = false;
        inner.tier_run = false;
    }
    if let Phase::Failed { .. } = inner.phase {
        inner.phase = Phase::Idle;
    }
}

/// Saved on every phase change; only a tier run makes the tier's verified files the models in
/// use, so a queued download never undoes a model picked in Settings.
fn persist(s: &mut Settings, cat: &Catalog, dir: &Path, tier_run: bool) {
    match s.tier {
        Some(tier) if tier_run => choose_tier_models(s, cat, dir, tier),
        _ => refresh_paths(s, cat, dir),
    }
}

/// What the runner is doing; the rest of setup lives in settings.
pub struct Setup {
    inner: Mutex<Inner>,
    /// Ends the current download or speed-test start (Pause, or a tier change).
    stop: Stop,
}
struct Inner {
    running: bool,
    paused: bool,
    /// The tier's pipeline was asked for (Download, Resume, an answer, or the next launch); a
    /// run for queued files only ends with the queue and never touches the tier.
    tier_run: bool,
    phase: Phase,
    /// Single files asked for in Settings › Models, before the tier's.
    queue: Vec<String>,
}
impl Default for Setup {
    fn default() -> Self {
        Self {
            inner: Mutex::new(Inner {
                running: false,
                paused: false,
                tier_run: false,
                phase: Phase::Idle,
                queue: Vec::new(),
            }),
            stop: Stop::default(),
        }
    }
}

fn emit(app: &tauri::AppHandle, event: Event) {
    let _ = app.emit("motetto://event", event);
}

/// The state as the window shows it: the runner's phase while it runs, else what the disk and
/// settings say (a step that would run but isn't running reads as Paused).
pub fn snapshot(app: &tauri::AppHandle) -> Result<SetupState, String> {
    let s = app.state::<Store>().get()?;
    let cat = catalog();
    let dir = model_store::dir(app);
    let suggested = match s.tier {
        Some(tier) => tier,
        None => crate::hardware::read().suggested,
    };
    let mut state = restore(&s, suggested);
    state.files = files(cat, &dir, &s.adopted, state.tier);
    let setup = app.state::<Setup>();
    let inner = setup.inner.lock().map_err(|_| "Setup unavailable")?;
    state.phase = if inner.running || matches!(inner.phase, Phase::Paused | Phase::Failed { .. }) {
        inner.phase.clone()
    } else if s.tier.is_none() {
        Phase::Idle
    } else {
        match next(&state, &|id| installed(cat, &dir, &s.adopted, id)) {
            Phase::Downloading { .. } | Phase::SpeedTest => Phase::Paused,
            phase => phase,
        }
    };
    Ok(state)
}

/// Starts the runner unless it is already running; it picks up any change on its next step.
fn kick(app: &tauri::AppHandle) -> Result<(), String> {
    let setup = app.state::<Setup>();
    let mut inner = setup.inner.lock().map_err(|_| "Setup unavailable")?;
    if inner.running {
        return Ok(());
    }
    inner.running = true;
    setup.stop.store(false, Ordering::SeqCst);
    drop(inner);
    let runner = app.clone();
    std::thread::Builder::new()
        .name("setup".into())
        .spawn(move || run(&runner))
        .map(|_| ())
        .map_err(|e| {
            if let Ok(mut inner) = setup.inner.lock() {
                inner.running = false;
            }
            e.to_string()
        })
}

/// Next launch: an unfinished setup with a `.part` on disk continues in the background.
pub fn resume(app: &tauri::AppHandle) {
    let Ok(s) = app.state::<Store>().get() else {
        return;
    };
    let dir = model_store::dir(app);
    let unfinished = catalog().files.iter().any(|f| part(&dir, f).exists());
    if !s.onboarding_done && s.tier.is_some() && unfinished {
        if let Ok(mut inner) = app.state::<Setup>().inner.lock() {
            inner.tier_run = true;
        }
        if let Err(error) = kick(app) {
            eprintln!("Motetto: couldn't resume setup: {error}");
        }
    }
}

fn run(app: &tauri::AppHandle) {
    let setup = app.state::<Setup>();
    let store = app.state::<Store>();
    let cat = catalog();
    let dir = model_store::dir(app);
    let cache = std::env::home_dir().map(|h| h.join(".cache/whisper"));
    let mut last = None;
    while let Ok(s) = store.get() {
        let (phase, tier_run) = {
            let Ok(mut inner) = setup.inner.lock() else {
                break;
            };
            let have = |id: &str| installed(cat, &dir, &s.adopted, id);
            inner.queue.retain(|id| !have(id));
            let phase = plan(&inner, &s, &have);
            let tier_run = inner.tier_run;
            inner.running = matches!(phase, Phase::Downloading { .. } | Phase::SpeedTest);
            if !inner.running {
                inner.tier_run = false;
            }
            inner.phase = phase.clone();
            // Under the lock, so a Pause between here and the download is never lost.
            setup.stop.store(inner.paused, Ordering::SeqCst);
            (phase, tier_run)
        };
        if last.as_ref() != Some(&phase) {
            changed(app, &dir, &phase, tier_run);
            last = Some(phase.clone());
        }
        let failed = match &phase {
            Phase::Downloading { id } => {
                let Some(file) = cat.file(id) else { break };
                let mut progress = |bytes, total| {
                    emit(
                        app,
                        Event::DownloadProgress {
                            id: id.clone(),
                            bytes,
                            total,
                        },
                    )
                };
                match obtain(file, &dir, cache.as_deref(), &setup.stop, &mut progress) {
                    Ok(Got::Paused) => None,
                    Ok(got) => {
                        let _ = store.change(|s| {
                            if let Got::Adopted(found) = got {
                                s.adopted
                                    .insert(id.clone(), found.to_string_lossy().into_owned());
                            }
                            refresh_paths(s, cat, &dir);
                        });
                        emit(app, Event::DownloadDone { id: id.clone() });
                        None
                    }
                    Err(failure) => {
                        emit(
                            app,
                            Event::DownloadFailed {
                                id: id.clone(),
                                reason: failure.to_string(),
                                checksum: failure == Failure::Checksum,
                            },
                        );
                        Some((id.clone(), failure.to_string()))
                    }
                }
            }
            Phase::SpeedTest => {
                let tier = s.tier.unwrap_or(TierId::Balanced);
                // One speech server at a time: a recording or import goes first.
                let library = app.state::<Library>();
                let mut held = false;
                while !setup.stop.load(Ordering::SeqCst) {
                    if !library.busy.swap(true, Ordering::SeqCst) {
                        held = true;
                        break;
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }
                let result = if held {
                    let result = speed_test(app, tier, &setup.stop);
                    library.busy.store(false, Ordering::SeqCst);
                    result
                } else {
                    Err("paused".into())
                };
                match result {
                    Ok(result) => {
                        let _ = store.change(|s| record_speed(s, result.clone()));
                        emit(app, Event::SpeedTest(result));
                        None
                    }
                    // A Pause during the server's start isn't a failure.
                    Err(_) if setup.stop.load(Ordering::SeqCst) => None,
                    Err(reason) => Some(("speed-test".to_string(), reason)),
                }
            }
            _ => break,
        };
        if let Some((id, reason)) = failed {
            if let Ok(mut inner) = setup.inner.lock() {
                inner.running = false;
                inner.tier_run = false;
                inner.phase = Phase::Failed { id, reason };
            }
            changed(
                app,
                &dir,
                &snapshot(app).map(|s| s.phase).unwrap_or(Phase::Idle),
                tier_run,
            );
            break;
        }
    }
}

/// Every phase change is saved (the tier's verified models become the ones in use) and sent.
fn changed(app: &tauri::AppHandle, dir: &Path, phase: &Phase, tier_run: bool) {
    let store = app.state::<Store>();
    let _ = store.change(|s| persist(s, catalog(), dir, tier_run));
    if let Ok(state) = snapshot(app) {
        emit(app, Event::SetupChanged(state));
    }
    if let Phase::AwaitingAnswer { .. } = phase
        && !app
            .get_webview_window("onboarding")
            .is_some_and(|w| w.is_visible().unwrap_or(false))
    {
        let tier = store
            .get()
            .ok()
            .and_then(|s| s.tier)
            .map_or("this model set", tier_name);
        crate::notifications::setup_slow(app, &format!("This computer is slow for {tier}"));
    }
}

fn tier_name(tier: TierId) -> &'static str {
    match tier {
        TierId::Best => "Best",
        TierId::Balanced => "Balanced",
        TierId::Light => "Light",
    }
}

/// Transcribes the bundled sample once to warm up, then times a second run (SPEC 5.2).
fn speed_test(app: &tauri::AppHandle, tier: TierId, stop: &Stop) -> Result<SpeedResult, String> {
    let s = app.state::<Store>().get()?;
    let cat = catalog();
    let dir = model_store::dir(app);
    let find = |id: &str| {
        cat.file(id)
            .and_then(|f| model_store::path(&dir, &s.adopted, f))
            .map(|p| p.to_string_lossy().into_owned())
    };
    let model = find(&cat.tier(tier).speech).ok_or("Download the speech model first.")?;
    let scratch = scratch("speed")?;
    let result = (|| {
        let audio = samples(&crate::sidecars::sample_path(app), &scratch)?;
        let server = crate::speech::Server::start_with(stop, model, find(&cat.vad))
            .map_err(|e| e.to_string())?;
        let run = || {
            server
                .client
                .transcribe(&audio, 0, Some("en"), None)
                .map_err(|e| e.to_string())
        };
        run()?;
        let started = Instant::now();
        run()?;
        Ok(speed_result(tier, started.elapsed().as_secs_f32()))
    })();
    let _ = std::fs::remove_dir_all(scratch);
    result
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Heard {
    pub text: String,
}

/// Capture goes through ScreenCaptureKit, which needs Screen & System Audio Recording even for
/// the microphone track.
fn can_test(p: &crate::permissions::Permissions) -> Result<(), String> {
    if p.system_audio == crate::permissions::Grant::Allowed {
        Ok(())
    } else {
        Err(TRY_SCREEN.into())
    }
}

fn heard(segments: &[motetto_core::whisper::Segment]) -> String {
    segments
        .iter()
        .map(|s| s.text.trim())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Ten seconds from the microphone, transcribed and thrown away (first run's Try it).
fn test_recording_now(app: &tauri::AppHandle) -> Result<Heard, String> {
    let s = app.state::<Store>().get()?;
    if s.speech_path.is_empty() {
        return Err(TRY_WAIT.into());
    }
    can_test(&crate::permissions::read(s.screen_requested)?)?;
    let library = app.state::<Library>();
    if library.busy.swap(true, Ordering::SeqCst) {
        return Err(BUSY.into());
    }
    let scratch = scratch("test");
    let result = scratch.clone().and_then(|dir| {
        let base = dir.join("test").to_string_lossy().into_owned();
        let stop = Stop::default();
        let timer = stop.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(10));
            timer.store(true, Ordering::SeqCst);
        });
        #[cfg(target_os = "macos")]
        let captured = motetto_core::capture::record_with_levels(
            &base,
            stop.clone(),
            None,
            crate::microphone::for_capture(app).as_deref(),
        );
        #[cfg(not(target_os = "macos"))]
        let captured = motetto_core::capture::record(&base, stop.clone());
        stop.store(true, Ordering::SeqCst);
        captured.map_err(|e| e.to_string())?;
        let audio = samples(Path::new(&format!("{base}.mic.wav")), &dir)?;
        let idle = Stop::default();
        let mut server = crate::speech::Server::start(&idle).map_err(|e| e.to_string())?;
        let segments = server
            .transcribe(&audio, 0, None, &idle)
            .map_err(|e| e.to_string())?;
        Ok(Heard {
            text: heard(&segments),
        })
    });
    if let Ok(dir) = scratch {
        let _ = std::fs::remove_dir_all(dir);
    }
    library.busy.store(false, Ordering::SeqCst);
    result
}

/// The first-run window (SPEC 7.2), reopened where it was left.
pub fn open_window(app: &tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("onboarding") {
        window.show().map_err(|e| e.to_string())?;
        window.unminimize().map_err(|e| e.to_string())?;
        return window.set_focus().map_err(|e| e.to_string());
    }
    tauri::WebviewWindowBuilder::new(
        app,
        "onboarding",
        tauri::WebviewUrl::App("onboarding/".into()),
    )
    .title("Set up Motetto")
    .inner_size(740., 660.)
    .resizable(false)
    .center()
    .build()
    .map(|_| ())
    .map_err(|e| e.to_string())
}

async fn blocking<T: Send + 'static>(
    job: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(job)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn model_catalog(app: tauri::AppHandle) -> Result<Catalog, String> {
    blocking(move || {
        let adopted = app.state::<Store>().get()?.adopted;
        let dir = model_store::dir(&app);
        let mut cat = catalog().clone();
        for file in &mut cat.files {
            file.on_disk = model_store::path(&dir, &adopted, file).is_some();
        }
        Ok(cat)
    })
    .await
}

#[tauri::command]
pub async fn setup_state(app: tauri::AppHandle) -> Result<SetupState, String> {
    blocking(move || snapshot(&app)).await
}

/// First run's Download (and Resume): the tier's files and speed test, in order.
#[tauri::command]
pub async fn start_setup(app: tauri::AppHandle, tier: TierId) -> Result<(), String> {
    blocking(move || {
        // `dl_disk` before starting: the whole tier must fit, not only its first file.
        let cat = catalog();
        let dir = model_store::dir(&app);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let adopted = app.state::<Store>().get()?.adopted;
        let have: u64 = files(cat, &dir, &adopted, tier).iter().map(|f| f.1).sum();
        let free = download::free_bytes(&dir)
            .map_err(|e| format!("Couldn't check free disk space: {e}"))?;
        download::space_ok(cat.tier_bytes(tier).saturating_sub(have), free)
            .map_err(|f| f.to_string())?;
        app.state::<Store>().change(|s| {
            if s.tier != Some(tier) {
                s.slow_question = None;
            }
            s.tier = Some(tier);
        })?;
        let setup = app.state::<Setup>();
        {
            let mut inner = setup.inner.lock().map_err(|_| "Setup unavailable")?;
            if let Phase::Failed { .. } = inner.phase {
                inner.phase = Phase::Idle;
            }
            inner.paused = false;
            inner.tier_run = true;
            // A download the new tier doesn't need stops; the runner moves on to the tier's.
            let wanted = cat.tier(tier);
            if let Phase::Downloading { id } = &inner.phase
                && ![&wanted.speech, &wanted.notes, &cat.vad].contains(&id)
                && !inner.queue.contains(id)
            {
                setup.stop.store(true, Ordering::SeqCst);
            }
        }
        kick(&app)
    })
    .await
}

/// Keeps the `.part`; Resume (or the next launch) continues it.
#[tauri::command]
pub fn pause_download(setup: tauri::State<'_, Setup>) -> Result<(), String> {
    let mut inner = setup.inner.lock().map_err(|_| "Setup unavailable")?;
    if inner.running {
        inner.paused = true;
        setup.stop.store(true, Ordering::SeqCst);
    }
    Ok(())
}

/// `speed_slow`: "Use {next tier}" (true) or "Keep {tier}" (false).
#[tauri::command]
pub async fn answer_slow(app: tauri::AppHandle, use_lower: bool) -> Result<(), String> {
    blocking(move || {
        let s = app.state::<Store>().change(|s| {
            if use_lower && let Some(offer) = s.slow_question {
                s.tier = Some(offer);
            }
            s.slow_question = None;
        })?;
        if s.tier.is_none() {
            return Ok(());
        }
        if let Ok(mut inner) = app.state::<Setup>().inner.lock() {
            inner.paused = false;
            inner.tier_run = true;
        }
        kick(&app)
    })
    .await
}

/// Settings › Models: the speed test for any tier whose speech model is on disk.
#[tauri::command]
pub async fn run_speed_test(app: tauri::AppHandle, tier: TierId) -> Result<SpeedResult, String> {
    blocking(move || {
        let library = app.state::<Library>();
        if library.busy.swap(true, Ordering::SeqCst) {
            return Err(BUSY.into());
        }
        let result = speed_test(&app, tier, &Stop::default());
        library.busy.store(false, Ordering::SeqCst);
        let result = result?;
        app.state::<Store>()
            .change(|s| record_speed(s, result.clone()))?;
        emit(&app, Event::SpeedTest(result.clone()));
        Ok(result)
    })
    .await
}

/// Settings › Models: one catalog file, on the same one-at-a-time runner.
#[tauri::command]
pub async fn download_model(app: tauri::AppHandle, id: String) -> Result<(), String> {
    blocking(move || {
        if catalog().file(&id).is_none() {
            return Err("Choose a model from the list.".into());
        }
        let setup = app.state::<Setup>();
        {
            let mut inner = setup.inner.lock().map_err(|_| "Setup unavailable")?;
            enqueue(&mut inner, id);
        }
        kick(&app)
    })
    .await
}

/// Removes files from the models folder (and their `.part`); a file the user already had stays
/// where it is and is only no longer used. Returns the bytes freed.
fn remove(app: &tauri::AppHandle, ids: &[String]) -> Result<u64, String> {
    let setup = app.state::<Setup>();
    let inner = setup.inner.lock().map_err(|_| "Setup unavailable")?;
    if let Phase::Downloading { id } = &inner.phase
        && inner.running
        && ids.contains(id)
    {
        return Err("Pause the download before deleting it.".into());
    }
    let library = app.state::<Library>();
    if library.busy.swap(true, Ordering::SeqCst) {
        return Err(BUSY.into());
    }
    let cat = catalog();
    let dir = model_store::dir(app);
    let mut freed = 0;
    let mut result = Ok(());
    for file in ids.iter().filter_map(|id| cat.file(id)) {
        for path in [dir.join(&file.file), part(&dir, file)] {
            let bytes = std::fs::metadata(&path).map_or(0, |m| m.len());
            match std::fs::remove_file(&path) {
                Ok(()) => freed += bytes,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => result = Err(format!("Couldn't delete {}: {e}", file.label)),
            }
        }
    }
    let saved = app.state::<Store>().change(|s| {
        for id in ids {
            s.adopted.remove(id);
        }
        refresh_paths(s, cat, &dir);
    });
    library.busy.store(false, Ordering::SeqCst);
    drop(inner);
    result?;
    saved?;
    let _ = app.emit("motetto://settings-changed", ());
    Ok(freed)
}

#[tauri::command]
pub async fn delete_model(app: tauri::AppHandle, id: String) -> Result<(), String> {
    blocking(move || {
        if catalog().file(&id).is_none() {
            return Err("Choose a model from the list.".into());
        }
        remove(&app, &[id]).map(|_| ())
    })
    .await
}

/// Every catalog file the chosen models don't use; during first run the tier's files stay too.
#[tauri::command]
pub async fn delete_unused_models(app: tauri::AppHandle) -> Result<u64, String> {
    blocking(move || {
        let s = app.state::<Store>().get()?;
        let cat = catalog();
        let mut keep = vec![
            s.speech_model.clone(),
            s.notes_model.clone(),
            cat.vad.clone(),
        ];
        if let (false, Some(tier)) = (s.onboarding_done, s.tier) {
            let tier = cat.tier(tier);
            keep.extend([tier.speech.clone(), tier.notes.clone()]);
        }
        let unused: Vec<String> = cat
            .files
            .iter()
            .map(|f| f.id.clone())
            .filter(|id| !keep.contains(id))
            .collect();
        remove(&app, &unused)
    })
    .await
}

#[tauri::command]
pub async fn test_recording(app: tauri::AppHandle) -> Result<Heard, String> {
    blocking(move || test_recording_now(&app)).await
}

/// Saved on every step change; `consent` is the step 1 box.
#[tauri::command]
pub fn set_onboarding_step(
    store: tauri::State<'_, Store>,
    step: u8,
    consent: Option<bool>,
) -> Result<(), String> {
    if step > 3 {
        return Err("Unknown setup step".into());
    }
    store
        .change(|s| {
            s.onboarding_step = step;
            if consent == Some(true) && s.consent_confirmed_at.is_none() {
                s.consent_confirmed_at = Some(now());
            }
        })
        .map(|_| ())
}

/// Done: the library opens. A notes model still downloading keeps going.
#[tauri::command]
pub async fn finish_onboarding(app: tauri::AppHandle) -> Result<(), String> {
    // Off the main thread: the tray rebuild waits for the controller's state.
    blocking(move || {
        app.state::<Store>().change(|s| s.onboarding_done = true)?;
        crate::tray::invalidate(&app);
        crate::desktop::refresh_tray(&app);
        let _ = app.emit("motetto://settings-changed", ());
        crate::desktop::show(&app);
        if let Some(window) = app.get_webview_window("onboarding") {
            window.hide().map_err(|e| e.to_string())?;
        }
        Ok(())
    })
    .await
}

/// After granting Screen & System Audio Recording, which macOS applies only to a new process.
#[tauri::command]
pub fn relaunch(app: tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<crate::controller::Controller>().state()?.state;
    if state != motetto_core::events::SessionState::Idle
        || app.state::<Library>().busy.load(Ordering::SeqCst)
    {
        return Err(BUSY.into());
    }
    crate::speech::shutdown();
    crate::notes_engine::shutdown();
    app.restart()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::{Grant, Permissions};

    fn state(tier: TierId, speed: Option<(f32, TierId)>) -> SetupState {
        SetupState {
            tier,
            phase: Phase::Idle,
            files: vec![],
            speed: speed.map(|(seconds, tier)| speed_result(tier, seconds)),
            keep_slow: false,
        }
    }
    fn downloading(id: &str) -> Phase {
        Phase::Downloading { id: id.into() }
    }

    #[test]
    fn setup_order() {
        let best = state(TierId::Best, None);
        assert_eq!(next(&best, &|_| false), downloading("speech-large-v3"));
        assert_eq!(
            next(&best, &|id| id == "speech-large-v3"),
            downloading("vad-silero")
        );
        let speech_and_vad = |id: &str| id == "speech-large-v3" || id == "vad-silero";
        assert_eq!(next(&best, &speech_and_vad), Phase::SpeedTest);
        let fast = state(TierId::Best, Some((2.0, TierId::Best)));
        assert_eq!(next(&fast, &speech_and_vad), downloading("notes-qwen3-8b"));
        assert_eq!(next(&fast, &|_| true), Phase::Done);
        // A result for another tier doesn't count: the new tier is tested again.
        let other = state(TierId::Balanced, Some((2.0, TierId::Best)));
        assert_eq!(next(&other, &|_| true), Phase::SpeedTest);
    }

    #[test]
    fn slow_speed_awaits_answer() {
        let slow = state(TierId::Best, Some((8.0, TierId::Best)));
        assert_eq!(slow.speed.as_ref().unwrap().rtf, 0.8);
        assert_eq!(
            next(&slow, &|_| true),
            Phase::AwaitingAnswer {
                offer: TierId::Balanced
            }
        );
        // Saved with its question; "Keep" clears the question and setup goes on.
        let mut s = Settings {
            tier: Some(TierId::Best),
            ..Settings::default()
        };
        record_speed(&mut s, slow.speed.clone().unwrap());
        assert_eq!(s.slow_question, Some(TierId::Balanced));
        assert!(!restore(&s, TierId::Best).keep_slow);
        s.slow_question = None;
        let kept = restore(&s, TierId::Best);
        assert!(kept.keep_slow);
        assert_eq!(
            next(&kept, &|id| id != "notes-qwen3-8b"),
            downloading("notes-qwen3-8b")
        );
        // Exactly 0.5 still keeps up.
        assert!(speed_result(TierId::Best, 5.0).keeps_up);
    }

    #[test]
    fn light_too_slow_disables_live() {
        let slow = state(TierId::Light, Some((9.0, TierId::Light)));
        assert_eq!(
            next(&slow, &|id| id != "notes-qwen3.5-2b"),
            downloading("notes-qwen3.5-2b")
        );
        let mut s = Settings::default();
        assert!(s.live_transcription);
        record_speed(&mut s, slow.speed.unwrap());
        assert!(!s.live_transcription);
        assert_eq!(s.slow_question, None);
        record_speed(&mut s, speed_result(TierId::Light, 3.0));
        assert!(s.live_transcription);
    }

    fn inner() -> Inner {
        Setup::default().inner.into_inner().unwrap()
    }

    #[test]
    fn settings_download_never_restarts_the_tier() {
        // Setup finished, the notes model was deleted, then another file is downloaded.
        let s = Settings {
            tier: Some(TierId::Best),
            onboarding_done: true,
            speed_test: Some(speed_result(TierId::Best, 2.0)),
            ..Settings::default()
        };
        let mut runner = inner();
        enqueue(&mut runner, "speech-small".into());
        let deleted = |id: &str| id != "notes-qwen3-8b" && id != "speech-small";
        assert_eq!(plan(&runner, &s, &deleted), downloading("speech-small"));
        runner.queue.clear();
        assert_eq!(plan(&runner, &s, &deleted), Phase::Idle);
        // Only an asked-for tier run fetches the tier's files.
        runner.tier_run = true;
        assert_eq!(plan(&runner, &s, &deleted), downloading("notes-qwen3-8b"));
    }

    #[test]
    fn queuing_keeps_a_pause() {
        let s = Settings {
            tier: Some(TierId::Best),
            ..Settings::default()
        };
        let mut runner = inner();
        runner.tier_run = true;
        runner.paused = true;
        assert_eq!(plan(&runner, &s, &|_| false), Phase::Paused);
        enqueue(&mut runner, "vad-silero".into());
        assert_eq!(plan(&runner, &s, &|_| false), downloading("vad-silero"));
        // When the queued file is done, the paused first-run download does not resume.
        runner.queue.clear();
        assert_eq!(plan(&runner, &s, &|_| false), Phase::Idle);
    }

    #[test]
    fn queue_only_run_keeps_models_picked_in_settings() {
        let dir = folder("persist");
        let cat = tiny_catalog();
        std::fs::write(dir.join("s.bin"), b"speech").unwrap();
        std::fs::write(dir.join("n.bin"), b"notes").unwrap();
        let mut s = Settings {
            tier: Some(TierId::Best),
            speech_model: "picked".into(),
            notes_model: "picked-notes".into(),
            ..Settings::default()
        };
        persist(&mut s, &cat, &dir, false);
        assert_eq!(
            (s.speech_model.as_str(), s.notes_model.as_str()),
            ("picked", "picked-notes")
        );
        persist(&mut s, &cat, &dir, true);
        assert_eq!(
            (s.speech_model.as_str(), s.notes_model.as_str()),
            ("s", "n")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn another_tiers_speed_test_is_not_the_setup_answer() {
        let mut s = Settings {
            tier: Some(TierId::Light),
            ..Settings::default()
        };
        record_speed(&mut s, speed_result(TierId::Light, 3.0));
        record_speed(&mut s, speed_result(TierId::Best, 9.0));
        assert_eq!(s.speed_test.as_ref().unwrap().tier, TierId::Light);
        assert_eq!(s.slow_question, None);
        assert!(s.live_transcription);
    }

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("motetto-setup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
    fn tiny_catalog() -> Catalog {
        use sha2::{Digest, Sha256};
        let file = |id: &str, kind, data: &[u8]| ModelFile {
            id: id.into(),
            kind,
            file: format!("{id}.bin"),
            label: id.into(),
            url: String::new(),
            bytes: data.len() as u64,
            sha256: format!("{:x}", Sha256::digest(data)),
            on_disk: false,
        };
        Catalog {
            files: vec![
                file("s", FileKind::Speech, b"speech"),
                file("v", FileKind::Vad, b"vad"),
                file("n", FileKind::Notes, b"notes"),
            ],
            tiers: vec![crate::catalog::Tier {
                id: TierId::Best,
                speech: "s".into(),
                notes: "n".into(),
                suits_mac: String::new(),
                suits_pc: String::new(),
            }],
            vad: "v".into(),
        }
    }

    #[test]
    fn paths_follow_verified_files_only() {
        let dir = folder("paths");
        let cat = tiny_catalog();
        let mut s = Settings {
            speech_model: "s".into(),
            notes_model: "n".into(),
            // Saved paths are never trusted: they are re-read from disk.
            speech_path: "/gone/s.bin".into(),
            notes_path: "/gone/n.bin".into(),
            ..Settings::default()
        };
        refresh_paths(&mut s, &cat, &dir);
        assert_eq!(
            (
                s.speech_path.as_str(),
                s.vad_path.as_str(),
                s.notes_path.as_str()
            ),
            ("", "", "")
        );
        // A `.part` or a file of the wrong size is not ready.
        std::fs::write(dir.join("s.bin.part"), b"speech").unwrap();
        std::fs::write(dir.join("n.bin"), b"note").unwrap();
        refresh_paths(&mut s, &cat, &dir);
        assert_eq!((s.speech_path.as_str(), s.notes_path.as_str()), ("", ""));
        std::fs::rename(dir.join("s.bin.part"), dir.join("s.bin")).unwrap();
        std::fs::write(dir.join("n.bin"), b"notes").unwrap();
        std::fs::write(dir.join("v.bin"), b"vad").unwrap();
        refresh_paths(&mut s, &cat, &dir);
        assert_eq!(s.speech_path, dir.join("s.bin").to_string_lossy());
        assert_eq!(s.vad_path, dir.join("v.bin").to_string_lossy());
        assert_eq!(s.notes_path, dir.join("n.bin").to_string_lossy());
        // Deleting a file clears its path.
        std::fs::remove_file(dir.join("n.bin")).unwrap();
        refresh_paths(&mut s, &cat, &dir);
        assert_eq!(s.notes_path, "");
        assert_eq!(s.speech_path, dir.join("s.bin").to_string_lossy());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn tier_models_change_only_once_verified() {
        let dir = folder("choose");
        let cat = tiny_catalog();
        let mut s = Settings {
            speech_model: "old".into(),
            ..Settings::default()
        };
        choose_tier_models(&mut s, &cat, &dir, TierId::Best);
        assert_eq!(
            (s.speech_model.as_str(), s.notes_model.as_str()),
            ("old", "")
        );
        std::fs::write(dir.join("s.bin"), b"speech").unwrap();
        choose_tier_models(&mut s, &cat, &dir, TierId::Best);
        assert_eq!(s.speech_model, "s");
        assert_eq!(s.speech_path, dir.join("s.bin").to_string_lossy());
        assert_eq!(s.notes_path, "");
        assert_eq!(
            files(&cat, &dir, &s.adopted, TierId::Best),
            vec![("s".into(), 6, 6), ("v".into(), 0, 3), ("n".into(), 0, 5)]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_recording_needs_screen_audio_and_joins_text() {
        let allowed = Permissions {
            microphone: Grant::Allowed,
            system_audio: Grant::Allowed,
            notifications: Grant::NotAsked,
        };
        assert!(can_test(&allowed).is_ok());
        for grant in [Grant::Denied, Grant::NotAsked] {
            let p = Permissions {
                system_audio: grant,
                ..allowed.clone()
            };
            assert_eq!(can_test(&p), Err(TRY_SCREEN.into()));
        }
        let segment = |text: &str| motetto_core::whisper::Segment {
            start_ms: 0,
            end_ms: 0,
            text: text.into(),
        };
        assert_eq!(
            heard(&[segment(" Hello"), segment(" "), segment("there. ")]),
            "Hello there."
        );
        assert_eq!(heard(&[]), "");
        let config = ts_rs::Config::default().with_out_dir("../../ui/lib/bindings");
        Heard::export_all(&config).unwrap();
        SetupState::export_all(&config).unwrap();
    }

    /// The runner's download step against Hugging Face for the 0.9 MB voice detection model:
    /// `cargo test real_vad_download -- --ignored --nocapture`.
    #[test]
    #[ignore = "downloads 0.9 MB from Hugging Face"]
    fn real_vad_download_is_marked_verified() {
        let dir = folder("real-vad");
        let cat = catalog();
        let vad = cat.file(&cat.vad).unwrap();
        let started = Instant::now();
        let mut last = (0, 0);
        let got = obtain(vad, &dir, None, &Stop::default(), &mut |b, t| last = (b, t));
        assert!(matches!(got, Ok(Got::Downloaded)), "{:?}", got.err());
        let mut s = Settings::default();
        refresh_paths(&mut s, cat, &dir);
        println!(
            "vad {} bytes in {:.1}s, progress {last:?}, vad_path {}",
            vad.bytes,
            started.elapsed().as_secs_f32(),
            s.vad_path
        );
        assert_eq!(s.vad_path, dir.join(&vad.file).to_string_lossy());
        assert!(!part(&dir, vad).exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
