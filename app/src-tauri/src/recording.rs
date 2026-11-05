//! Serial capture and notes worker; closing every WAV precedes Resume, processing and discard.
use crate::{
    controller::{Job, Message},
    library,
};
use motetto_core::{
    config, correct,
    events::{Event, MicChoice, RecordSetup, ScreenChoice, Stage},
    summarize, transcribe,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};
use tauri::Emitter;
use tauri::Manager;

pub fn spawn(
    root: PathBuf,
    app: tauri::AppHandle,
    results: mpsc::Sender<Message>,
    allow_exit: Arc<AtomicBool>,
) -> Result<mpsc::Sender<Job>, String> {
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("motetto-recording".into())
        .spawn(move || {
            let mut recording: Option<Recording> = None;
            for job in receiver {
                match job {
                    Job::Capture {
                        id,
                        offset_ms,
                        stop,
                        screen,
                        while_sharing,
                        setup,
                        mouse_stop,
                    } => {
                        if recording.as_ref().is_none_or(|r| r.id != id) {
                            recording = Some(Recording {
                                id: id.clone(),
                                parts: vec![],
                                failure: None,
                            });
                        }
                        let Some(record) = recording.as_mut() else {
                            continue;
                        };
                        let (mic_on, system_on) = tracks(setup.as_ref());
                        let result = capture_part(
                            &root,
                            record,
                            offset_ms,
                            stop,
                            setup.as_ref(),
                            |base, stop| {
                                // The meeting exists from its first second: list it before capture, and
                                // open it in the library (owner 2026-10-20: Record now from the tray
                                // left the old meeting selected). Resume parts don't pull the view.
                                let _ = app.emit("motetto://library-changed", ());
                                if offset_ms == 0 {
                                    let _ = app.emit("motetto://open-meeting", id.clone());
                                }
                                // Live captions off (set by the speed test on a slow Mac): the call
                                // is transcribed after Stop only. They need both tracks: a clip has
                                // none, and one track is transcribed after Stop (one-track captions:
                                // later).
                                let settings = app.state::<crate::settings::Store>().get();
                                // A screen recording's page names its frame rate, as recorded.
                                if let (0, Ok(settings)) = (offset_ms, &settings) {
                                    let fps =
                                        motetto_core::video::Preset::parse(&settings.screen_preset)
                                            .fps();
                                    let _ = crate::bookmarks::update(&root, &id, |s| {
                                        if let Some(facts) = s.facts.as_mut() {
                                            facts.fps = fps.unsigned_abs();
                                        }
                                    });
                                }
                                if live_captions(settings.as_ref().ok())
                                    && mic_on
                                    && system_on
                                    && setup.as_ref().is_none_or(|s| s.notes)
                                {
                                    crate::live::start(
                                        app.clone(),
                                        id.clone(),
                                        base.into(),
                                        offset_ms,
                                        stop.clone(),
                                    )?;
                                }
                                // The script strip follows the voice with or without live captions
                                // (Apple on a Mac; Whisper only on the captions' server).
                                if mic_on {
                                    crate::follow::start(
                                        app.clone(),
                                        format!("{base}.mic.wav").into(),
                                        stop.clone(),
                                        false,
                                    );
                                }
                                #[cfg(target_os = "macos")]
                                let result = {
                                    let mic = match setup.as_ref().map(|s| &s.mic) {
                                        None | Some(MicChoice::Default) => {
                                            crate::microphone::for_capture(&app)
                                        }
                                        Some(MicChoice::Device { id }) => Some(id.clone()),
                                        Some(MicChoice::Off) => None,
                                    };
                                    let (mut picture, progress) = crate::recording_screen::prepare(
                                        &app,
                                        &results,
                                        &id,
                                        base,
                                        offset_ms,
                                        &stop,
                                        screen.as_ref().map(|pick| (pick, while_sharing)),
                                    )
                                    .unwrap_or_else(|reason| {
                                        let _ = results.send(Message::ScreenEnded {
                                            id: id.clone(),
                                            part: stop.clone(),
                                            lost: false,
                                            restart: false,
                                            reason: Some(reason),
                                        });
                                        (None, None)
                                    });
                                    // Chosen on the setup bar: the pointer always, click rings if asked.
                                    if let (Some(picture), Some(setup)) =
                                        (picture.as_mut(), setup.as_ref())
                                    {
                                        picture.cursor = true;
                                        picture.clicks = setup.clicks;
                                    }
                                    if stop.load(Ordering::Acquire) {
                                        picture.take();
                                        if let Some(progress) = progress {
                                            let _ = progress.join();
                                        }
                                        let empty = motetto_core::video::VideoOutcome {
                                            path: format!("{base}.screen.mp4"),
                                            width: 0,
                                            height: 0,
                                            moments_ms: vec![],
                                            lost: false,
                                            error: None,
                                        };
                                        crate::recording_screen::finish(
                                            base,
                                            offset_ms,
                                            screen.as_ref(),
                                            while_sharing,
                                            &empty,
                                            &app,
                                        );
                                        return Ok(vec![]);
                                    }
                                    // Click dots for a recorded picture, and the pointer against its
                                    // target for the reach for Stop.
                                    let clicks = picture.as_ref().and_then(|picture| {
                                        let dir = std::path::Path::new(base).parent()?;
                                        let target = crate::clicks::target(&picture.target);
                                        Some(crate::clicks::start(&app, dir, offset_ms, target))
                                    });
                                    let part_began = std::time::Instant::now();
                                    let result = motetto_core::capture::record_with_levels(
                                        base,
                                        stop.clone(),
                                        Some(app.state::<crate::live::State>().meters.clone()),
                                        mic.as_deref(),
                                        picture.as_ref(),
                                        motetto_core::capture::Tracks {
                                            mic: mic_on,
                                            system: system_on,
                                        },
                                    )
                                    .map_err(|e| e.to_string());
                                    picture.take();
                                    // The last part decides: one not ended by a Stop click clears an
                                    // earlier part's suggestion.
                                    if let Some(clicks) = &clicks {
                                        let stop_ms =
                                            offset_ms + part_began.elapsed().as_millis() as u64;
                                        let reach = if mouse_stop.load(Ordering::SeqCst) {
                                            clicks.reach(stop_ms)
                                        } else {
                                            0
                                        };
                                        let _ = crate::bookmarks::set_reach(&root, &id, reach);
                                    }
                                    drop(clicks);
                                    if let Some(progress) = progress {
                                        let _ = progress.join();
                                    }
                                    // A silent clip ends with its window: the session stops it on the
                                    // ScreenEnded sent above. Wait for that Stop briefly instead of
                                    // calling the end unexpected.
                                    if !mic_on
                                        && !system_on
                                        && result.as_ref().is_ok_and(|captured| {
                                            captured.video.as_ref().is_some_and(|v| v.lost)
                                        })
                                    {
                                        let waited = std::time::Instant::now();
                                        while !stop.load(Ordering::Acquire)
                                            && waited.elapsed() < std::time::Duration::from_secs(2)
                                        {
                                            std::thread::sleep(std::time::Duration::from_millis(
                                                50,
                                            ));
                                        }
                                    }
                                    if let Ok(captured) = &result
                                        && let Some(video) = &captured.video
                                    {
                                        crate::recording_screen::finish(
                                            base,
                                            offset_ms,
                                            screen.as_ref(),
                                            while_sharing,
                                            video,
                                            &app,
                                        );
                                    }
                                    result.map(|captured| captured.files)
                                };
                                #[cfg(target_os = "windows")]
                                let result = motetto_core::capture::record(base, stop.clone())
                                    .map_err(|e| e.to_string());
                                let ended = stop.swap(true, Ordering::SeqCst);
                                crate::speech::shutdown();
                                if result.is_ok() && !ended {
                                    return Err(
                                    "Audio capture ended unexpectedly. Partial audio is retained."
                                        .into(),
                                );
                                }
                                result
                            },
                        );
                        if let Err(mut reason) = result {
                            if let Err(e) = status(&root, &id, "NeedsAttention") {
                                reason.push_str(&format!(" Status could not be saved: {e}"));
                            }
                            record.failure = Some(reason.clone());
                            let _ = results.send(Message::CaptureFailed { id, reason });
                        }
                    }
                    Job::SaveClip { id } => {
                        // A failed capture stays NeedsAttention with its partial files.
                        let failed = recording
                            .as_ref()
                            .is_some_and(|r| r.id == id && r.failure.is_some());
                        if !failed && root.join(&id).is_dir() {
                            let _ = app.emit(
                                "motetto://notice",
                                match save_clip(&root, &id) {
                                    Ok(()) => CLIP_SAVED.into(),
                                    Err(e) => format!("Couldn't save the screen recording: {e}"),
                                },
                            );
                        }
                        recording = None;
                        let _ = app.emit("motetto://library-changed", ());
                    }
                    Job::Finish { id, apps } => {
                        let mut result = match recording.as_ref().filter(|r| r.id == id) {
                            Some(record) => finish(&root, record, apps, |stage| {
                                let _ =
                                    app.emit("motetto://event", Event::stage(id.clone(), stage));
                            }),
                            None => Err(
                                "No capture was saved. Check recording permissions and try again."
                                    .into(),
                            ),
                        };
                        // Stopped on the steps page: the meeting says so and offers Write the notes.
                        let failed = if motetto_core::tools::cancelled() {
                            "NotesStopped"
                        } else {
                            "NeedsAttention"
                        };
                        if let Err(reason) = &mut result
                            && let Err(e) = status(&root, &id, failed)
                        {
                            reason.push_str(&format!(" Status could not be saved: {e}"));
                        }
                        if result.is_ok() {
                            match app.state::<crate::settings::Store>().get().and_then(|s| {
                                crate::settings_storage::retain(&root, &s.keep_audio, &s.keep_video)
                            }) {
                                Ok(()) => {}
                                Err(error) => {
                                    let _ = app.emit(
                                        "motetto://notice",
                                        format!(
                                            "Notes are ready, but audio retention failed: {error}"
                                        ),
                                    );
                                }
                            }
                        }
                        let _ = app.emit("motetto://library-changed", ());
                        let _ = results.send(Message::Finished { id, result });
                        recording = None;
                    }
                    Job::Discard { id } => {
                        // Never delete partial audio after a capture/write failure.
                        if recording
                            .as_ref()
                            .is_some_and(|r| r.id == id && r.failure.is_none())
                        {
                            let result = library::folder(&root, &id).and_then(|dir| {
                                let notes = dir.join("my-notes.md");
                                if notes.exists() && !library::text(&notes)?.trim().is_empty() {
                                    status(&root, &id, "NeedsAttention")?;
                                    let _ = app.emit(
                                        "motetto://notice",
                                        "The short recording was kept with your notes.",
                                    );
                                    Ok(())
                                } else {
                                    fs::remove_dir_all(dir).map_err(|e| e.to_string())
                                }
                            });
                            if let Err(e) = result {
                                let _ = app.emit(
                                    "motetto://notice",
                                    format!("Couldn't discard the short recording: {e}"),
                                );
                            }
                        }
                        recording = None;
                        let _ = app.emit("motetto://library-changed", ());
                    }
                    Job::Trash { id } => {
                        // Recoverable: Recently deleted keeps the audio and parts.json, and Write the
                        // notes again transcribes them later. It still needs its notes, hence the status.
                        let result = status(&root, &id, "NeedsAttention")
                            .and_then(|_| crate::meeting_actions::move_meeting(&root, &id, false));
                        if let Err(e) = result {
                            let _ = app.emit(
                                "motetto://notice",
                                format!("Couldn't discard the recording: {e}"),
                            );
                        }
                        recording = None;
                        let _ = app.emit("motetto://library-changed", ());
                    }
                    Job::Exit => {
                        allow_exit.store(true, Ordering::SeqCst);
                        app.exit(0);
                        break;
                    }
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(sender)
}
struct Recording {
    id: String,
    parts: Vec<(Vec<String>, u64)>,
    failure: Option<String>,
}
const CLIP_SAVED: &str = "Saved to your library. Just the video: no transcript, no notes.";
/// The tracks a setup records, as (mic, system); without a setup both, as before.
fn tracks(setup: Option<&RecordSetup>) -> (bool, bool) {
    setup.map_or((true, true), |s| (s.mic != MicChoice::Off, s.app_sounds))
}
fn capture_part(
    root: &Path,
    record: &mut Recording,
    offset_ms: u64,
    stop: motetto_core::events::Stop,
    setup: Option<&RecordSetup>,
    capture: impl FnOnce(&str, motetto_core::events::Stop) -> Result<Vec<String>, String>,
) -> Result<(), String> {
    if let Some(e) = &record.failure {
        return Err(e.clone());
    }
    let dir = create(root, &record.id, setup)?;
    if stop.load(Ordering::SeqCst) {
        return Ok(());
    }
    let base = dir.join(format!("part-{:04}", record.parts.len()));
    let base = base.to_string_lossy().into_owned();
    // Listed before capture, so a crash or a failed capture still leaves its audio findable by
    // Recover and Write the notes again; the entry is narrowed to the files written afterwards.
    // A track that is off gets no file, so it isn't listed.
    let (mic, system) = tracks(setup);
    let tracks: Vec<String> = [("mic", mic), ("system", system)]
        .into_iter()
        .filter(|(_, on)| *on)
        .map(|(side, _)| format!("{base}.{side}.wav"))
        .collect();
    record.parts.push((tracks.clone(), offset_ms));
    write_parts(&dir, &record.parts)?;
    let captured = capture(&base, stop.clone()).and_then(|files| {
        if stop.load(Ordering::SeqCst) {
            Ok(files)
        } else {
            Err("Audio capture ended unexpectedly. Partial audio is retained.".into())
        }
    });
    let files = match &captured {
        Ok(files) => files.clone(),
        Err(_) => tracks
            .into_iter()
            .filter(|f| Path::new(f).exists())
            .collect(),
    };
    if files.is_empty() {
        record.parts.pop();
    } else if let Some(part) = record.parts.last_mut() {
        part.0 = files;
    }
    let listed = write_parts(&dir, &record.parts);
    captured?;
    listed
}
fn write_parts(dir: &Path, parts: &[(Vec<String>, u64)]) -> Result<(), String> {
    fs::write(
        dir.join("parts.json"),
        serde_json::to_vec_pretty(parts).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
fn create(root: &Path, id: &str, setup: Option<&RecordSetup>) -> Result<PathBuf, String> {
    let dir = root.join(id);
    if !dir.exists() {
        fs::create_dir(&dir).map_err(|e| e.to_string())?;
        let created_ms = id
            .parse::<u128>()
            .map_err(|e| e.to_string())?
            .checked_div(1_000_000)
            .unwrap_or(0) as u64;
        // Just the video: a screen recording, named by its start time until renamed.
        let clip = setup.filter(|s| s.video_only());
        let facts = clip.and_then(|s| {
            let pick = s.screen.as_ref()?;
            Some(library::ScreenFacts {
                source: pick.name.clone(),
                what: match pick.choice {
                    ScreenChoice::CallWindow { .. } | ScreenChoice::Window { .. } => "window",
                    ScreenChoice::Display { .. } => "display",
                    ScreenChoice::Area { .. } => "area",
                }
                .into(),
                mic_off: s.mic == MicChoice::Off,
                app_sounds: s.app_sounds,
                clicks: s.clicks,
                fps: 0,
                width: 0,
                height: 0,
            })
        });
        library::save(
            root,
            &library::MeetingSummary {
                id: id.into(),
                title: if clip.is_some() {
                    clip_title(created_ms)
                } else {
                    "Recorded meeting".into()
                },
                created_ms,
                status: "Recording".into(),
                marks_ms: vec![],
                duration_ms: None,
                notes_ms: None,
                spellings: vec![],
                locked: false,
                title_source: Default::default(),
                language: None,
                notes_language: None,
                apps: vec![],
                unconfirmed: vec![],
                reach_ms: None,
                kind: if clip.is_some() {
                    library::MeetingKind::Screen
                } else {
                    library::MeetingKind::Meeting
                },
                facts,
                deleted_kind: Default::default(),
                has_video: false,
            },
        )?;
    }
    library::folder(root, id)
}
/// "Screen recording at HH:MM" in local time. The system's `date` reads the time zone, so no
/// time-zone crate is needed; without it the title has no time.
fn clip_title(created_ms: u64) -> String {
    let clock = std::process::Command::new("/bin/date")
        .args(["-r", &(created_ms / 1000).to_string(), "+%H:%M"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|text| text.trim().to_string())
        .filter(|text| text.len() == 5);
    match clock {
        Some(clock) => format!("Screen recording at {clock}"),
        None => "Screen recording".into(),
    }
}
/// A video-only recording is ready at Stop: no transcript, no notes. Its size comes from the
/// first picture part that has one.
fn save_clip(root: &Path, id: &str) -> Result<(), String> {
    let dir = library::folder(root, id)?;
    let parts: Vec<crate::screen::ScreenPart> = crate::screen_files::read(&dir, "screen.json");
    let mut summary = library::summary(root, id)?;
    summary.status = "Ready".into();
    summary.kind = library::MeetingKind::Screen;
    if let (Some(facts), Some(part)) = (
        summary.facts.as_mut(),
        parts.iter().find(|p| p.width > 0 && p.height > 0),
    ) {
        facts.width = part.width;
        facts.height = part.height;
    }
    library::save(root, &summary)
}
fn status(root: &Path, id: &str, value: &str) -> Result<library::MeetingSummary, String> {
    let mut summary = library::summary(root, id)?;
    summary.status = value.into();
    library::save(root, &summary)?;
    Ok(summary)
}
fn finish(
    root: &Path,
    record: &Recording,
    apps: Vec<String>,
    stage: impl Fn(Stage) + Sync,
) -> Result<String, String> {
    if let Some(reason) = &record.failure {
        return Err(format!(
            "Capture failed: {reason}. Partial audio is retained."
        ));
    }
    if record.parts.is_empty() {
        return Err("No audio was captured. Check microphone and system audio permissions.".into());
    }
    let mut summary = library::summary(root, &record.id)?;
    summary.status = "Finishing".into();
    summary.apps = apps;
    library::save(root, &summary)?;
    // Cancel is for imports only; a late one must not stop these notes.
    motetto_core::tools::reset();
    let dir = library::folder(root, &record.id)?;
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        stage(Stage::Transcribing { progress: None });
        let heard = transcribe::transcript_parts(&record.parts, true, &|progress| {
            stage(Stage::Transcribing {
                progress: Some(progress),
            })
        })?;
        let transcript = heard.text;
        fs::write(dir.join("transcript.txt"), &transcript)?;
        let text = match config::var("FIX_MODEL").filter(|v| !v.is_empty()) {
            Some(model) => {
                let fixed = correct::transcript(&model, &transcript)?;
                fs::write(dir.join("transcript.fixed.txt"), &fixed)?;
                fixed
            }
            None => transcript,
        };
        stage(Stage::Summarizing {
            part: 1,
            of: 1,
            progress: None,
        });
        let started = std::time::Instant::now();
        let spellings = library::used_spellings();
        let my_notes = dir.join("my-notes.md");
        let mine = if my_notes.exists() {
            library::text(&my_notes)?
        } else {
            String::new()
        };
        let written = summarize::notes(
            &dir.join("notes").to_string_lossy(),
            &text,
            &mine,
            heard.language.as_deref(),
            None,
            &stage,
        )?;
        let mut metadata = library::summary(root, &record.id)?;
        metadata.titled(written.title);
        metadata.unconfirmed = written.unconfirmed;
        metadata.language = heard.language;
        metadata.notes_language = config::notes_language();
        metadata.notes_ms = Some(started.elapsed().as_millis() as u64);
        metadata.spellings = spellings;
        library::save(root, &metadata)?;
        if !mine.trim().is_empty() {
            use std::io::Write;
            let mut file = fs::OpenOptions::new()
                .append(true)
                .open(dir.join("notes.md"))?;
            write!(file, "\n### My notes\n\n{mine}\n")?;
        }
        stage(Stage::Saving);
        Ok(())
    })();
    result.map_err(|e| e.to_string())?;
    Ok(status(root, &record.id, "Ready")?.title)
}

/// Live captions run unless the settings turn them off; unreadable settings keep them on.
fn live_captions(settings: Option<&crate::settings::Settings>) -> bool {
    settings.is_none_or(|s| s.live_transcription)
}

#[cfg(test)]
mod tests {
    use super::*;
    use motetto_core::events::Stop;

    #[test]
    fn live_captions_follow_the_setting() {
        let mut settings = crate::settings::Settings::default();
        assert!(live_captions(Some(&settings)));
        settings.live_transcription = false;
        assert!(!live_captions(Some(&settings)));
        assert!(live_captions(None));
    }
    fn root() -> PathBuf {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "motetto-worker-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        root
    }
    #[test]
    fn resumed_parts_preserve_prior_audio_and_manifest_offsets() {
        let root = root();
        let mut record = Recording {
            id: "123".into(),
            parts: vec![],
            failure: None,
        };
        for offset in [0, 25_000] {
            capture_part(
                &root,
                &mut record,
                offset,
                Stop::default(),
                None,
                |base, stop| {
                    let file = format!("{base}.mic.wav");
                    fs::write(&file, b"audio").unwrap();
                    stop.store(true, Ordering::SeqCst);
                    Ok(vec![file])
                },
            )
            .unwrap();
        }
        assert_ne!(record.parts[0].0, record.parts[1].0);
        assert_eq!(fs::read(&record.parts[0].0[0]).unwrap(), b"audio");
        let saved: Vec<(Vec<String>, u64)> =
            serde_json::from_slice(&fs::read(root.join("123/parts.json")).unwrap()).unwrap();
        assert_eq!(saved, record.parts);
        assert_eq!(saved[1].1, 25_000);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn parts_are_listed_before_capture_and_kept_when_it_fails() {
        let root = root();
        let mut record = Recording {
            id: "123".into(),
            parts: vec![],
            failure: None,
        };
        let listed = |root: &Path| -> Vec<(Vec<String>, u64)> {
            serde_json::from_slice(&fs::read(root.join("123/parts.json")).unwrap()).unwrap()
        };
        // While capture runs (a crash would stop here), both tracks are already listed.
        capture_part(
            &root,
            &mut record,
            0,
            Stop::default(),
            None,
            |base, stop| {
                assert_eq!(
                    listed(&root),
                    [(
                        vec![format!("{base}.mic.wav"), format!("{base}.system.wav")],
                        0
                    )]
                );
                fs::write(format!("{base}.mic.wav"), b"audio").unwrap();
                stop.store(true, Ordering::SeqCst);
                Ok(vec![format!("{base}.mic.wav")])
            },
        )
        .unwrap();
        // A failed part keeps only the audio it wrote; one that wrote nothing is not listed.
        let error = capture_part(
            &root,
            &mut record,
            25_000,
            Stop::default(),
            None,
            |base, _| {
                fs::write(format!("{base}.system.wav"), b"partial").unwrap();
                Err("disk full".into())
            },
        );
        assert_eq!(error, Err("disk full".into()));
        let parts = listed(&root);
        assert_eq!(parts, record.parts);
        assert_eq!(parts.len(), 2);
        assert!(parts[1].0.len() == 1 && parts[1].0[0].ends_with("part-0001.system.wav"));
        assert_eq!(parts[1].1, 25_000);
        let ended = capture_part(&root, &mut record, 50_000, Stop::default(), None, |_, _| {
            Ok(vec![])
        });
        assert!(ended.unwrap_err().contains("ended unexpectedly"));
        assert_eq!(listed(&root).len(), 2);
        fs::remove_dir_all(root).unwrap();
    }
    fn clip(mic: MicChoice, app_sounds: bool) -> RecordSetup {
        RecordSetup {
            mic,
            app_sounds,
            notes: false,
            clicks: true,
            screen: Some(motetto_core::events::ScreenPick {
                choice: ScreenChoice::Window {
                    window_id: 7,
                    app: "Safari".into(),
                },
                name: "Safari".into(),
            }),
        }
    }
    #[test]
    fn mic_off_writes_no_mic_wav() {
        let root = root();
        let mut record = Recording {
            id: "123".into(),
            parts: vec![],
            failure: None,
        };
        let setup = clip(MicChoice::Off, true);
        let listed = |root: &Path| -> Vec<(Vec<String>, u64)> {
            serde_json::from_slice(&fs::read(root.join("123/parts.json")).unwrap()).unwrap()
        };
        capture_part(
            &root,
            &mut record,
            0,
            Stop::default(),
            Some(&setup),
            |base, stop| {
                // Listed before capture: the system track only.
                assert_eq!(listed(&root), [(vec![format!("{base}.system.wav")], 0)]);
                fs::write(format!("{base}.system.wav"), b"audio").unwrap();
                stop.store(true, Ordering::SeqCst);
                Ok(vec![format!("{base}.system.wav")])
            },
        )
        .unwrap();
        let parts = listed(&root);
        assert_eq!(parts.len(), 1);
        assert!(parts[0].0.iter().all(|f| !f.ends_with(".mic.wav")));
        assert!(!root.join("123/part-0000.mic.wav").exists());
        // Mic on, app sounds off: the mic track only. Without a setup: both, as before.
        assert_eq!(
            tracks(Some(&clip(MicChoice::Default, false))),
            (true, false)
        );
        assert_eq!(tracks(None), (true, true));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn finish_with_only_video_is_ready() {
        let root = root();
        let mut record = Recording {
            id: "123".into(),
            parts: vec![],
            failure: None,
        };
        let setup = clip(MicChoice::Off, false);
        capture_part(
            &root,
            &mut record,
            0,
            Stop::default(),
            Some(&setup),
            |base, stop| {
                fs::write(format!("{base}.screen.mp4"), b"video").unwrap();
                stop.store(true, Ordering::SeqCst);
                Ok(vec![])
            },
        )
        .unwrap();
        // No WAVs: nothing to transcribe, and the old path would refuse it.
        assert!(record.parts.is_empty());
        assert!(
            finish(&root, &record, vec![], |_| {})
                .unwrap_err()
                .contains("No audio was captured")
        );
        let dir = root.join("123");
        let saving = library::summary(&root, "123").unwrap();
        assert_eq!(saving.status, "Recording");
        assert_eq!(saving.kind, library::MeetingKind::Screen);
        assert!(saving.title.starts_with("Screen recording"));
        fs::write(
            dir.join("screen.json"),
            r#"[{"file":"part-0000.screen.mp4","offset_ms":0,"source":{"kind":"window","name":"Safari"},"while_sharing":false,"width":1280,"height":800}]"#,
        )
        .unwrap();
        save_clip(&root, "123").unwrap();
        let saved = library::summary(&root, "123").unwrap();
        assert_eq!(saved.status, "Ready");
        assert_eq!(saved.kind, library::MeetingKind::Screen);
        assert_eq!(
            saved.facts,
            Some(library::ScreenFacts {
                source: "Safari".into(),
                what: "window".into(),
                mic_off: true,
                app_sounds: false,
                clicks: true,
                fps: 0,
                width: 1280,
                height: 800,
            })
        );
        assert!(!dir.join("transcript.txt").exists() && !dir.join("notes.md").exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn a_clip_is_named_by_its_local_start_time() {
        let title = clip_title(1_793_890_000_000);
        let clock = title.strip_prefix("Screen recording at ").unwrap();
        assert!(clock.len() == 5 && clock.as_bytes()[2] == b':', "{title}");
        assert!(
            clock.chars().filter(char::is_ascii_digit).count() == 4,
            "{title}"
        );
    }
    #[test]
    fn canceled_queued_resume_never_opens_capture() {
        let root = root();
        let mut record = Recording {
            id: "123".into(),
            parts: vec![],
            failure: None,
        };
        capture_part(
            &root,
            &mut record,
            0,
            Arc::new(AtomicBool::new(true)),
            None,
            |_, _| panic!("must not start"),
        )
        .unwrap();
        assert!(record.parts.is_empty());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn failed_capture_cannot_be_resumed_or_summarized() {
        let root = root();
        let mut record = Recording {
            id: "123".into(),
            parts: vec![],
            failure: Some("disk failed".into()),
        };
        assert_eq!(
            capture_part(&root, &mut record, 0, Stop::default(), None, |_, _| panic!(
                "must not start"
            )),
            Err("disk failed".into())
        );
        assert!(
            finish(&root, &record, vec![], |_| panic!("must not process"))
                .unwrap_err()
                .contains("Partial audio is retained")
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "requires synthetic audio and installed local models"]
    fn real_resumed_recording_writes_notes_with_offsets() {
        let source = PathBuf::from(std::env::var("MOTETTO_TEST_RECORDING").unwrap());
        let root = root();
        let dir = create(&root, "123", None).unwrap();
        let mut record = Recording {
            id: "123".into(),
            parts: vec![],
            failure: None,
        };
        for (part, offset) in [(0, 0), (1, 60_000)] {
            let file = dir.join(format!("part-{part:04}.mic.wav"));
            fs::copy(&source, &file).unwrap();
            record
                .parts
                .push((vec![file.to_string_lossy().into_owned()], offset));
        }
        finish(&root, &record, vec!["Zoom".into()], |_| {}).unwrap();
        let meeting = library::read(&root, "123").unwrap();
        assert_eq!(meeting.summary.apps, ["Zoom"]);
        assert_eq!(meeting.summary.status, "Ready");
        assert!(meeting.transcript.contains("[00:"));
        assert!(meeting.transcript.contains("[01:"));
        assert!(meeting.transcript.matches("Me:").count() >= 2);
        assert!(!meeting.notes_md.is_empty());
        assert!(Path::new(&record.parts[0].0[0]).exists());
        fs::remove_dir_all(root).unwrap();
    }
}
