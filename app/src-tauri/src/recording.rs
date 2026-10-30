//! Serial capture and notes worker; closing every WAV precedes Resume, processing and discard.
use crate::{
    controller::{Job, Message},
    library,
};
use motetto_core::{
    config, correct,
    events::{Event, Stage},
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
                        let result = capture_part(&root, record, offset_ms, stop, |base, stop| {
                            // The meeting exists from its first second: list it before capture, and
                            // open it in the library (owner 2026-10-20: Record now from the tray
                            // left the old meeting selected). Resume parts don't pull the view.
                            let _ = app.emit("motetto://library-changed", ());
                            if offset_ms == 0 {
                                let _ = app.emit("motetto://open-meeting", id.clone());
                            }
                            // Live captions off (set by the speed test on a slow Mac): the call
                            // is transcribed after Stop only.
                            let settings = app.state::<crate::settings::Store>().get();
                            if live_captions(settings.as_ref().ok()) {
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
                            crate::follow::start(
                                app.clone(),
                                format!("{base}.mic.wav").into(),
                                stop.clone(),
                                false,
                            );
                            #[cfg(target_os = "macos")]
                            let result = {
                                let mic = crate::microphone::for_capture(&app);
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
                                let result = motetto_core::capture::record_with_levels(
                                    base,
                                    stop.clone(),
                                    Some(app.state::<crate::live::State>().meters.clone()),
                                    mic.as_deref(),
                                    picture.as_ref(),
                                )
                                .map_err(|e| e.to_string());
                                picture.take();
                                if let Some(progress) = progress {
                                    let _ = progress.join();
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
                        });
                        if let Err(mut reason) = result {
                            if let Err(e) = status(&root, &id, "NeedsAttention") {
                                reason.push_str(&format!(" Status could not be saved: {e}"));
                            }
                            record.failure = Some(reason.clone());
                            let _ = results.send(Message::CaptureFailed { id, reason });
                        }
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
fn capture_part(
    root: &Path,
    record: &mut Recording,
    offset_ms: u64,
    stop: motetto_core::events::Stop,
    capture: impl FnOnce(&str, motetto_core::events::Stop) -> Result<Vec<String>, String>,
) -> Result<(), String> {
    if let Some(e) = &record.failure {
        return Err(e.clone());
    }
    let dir = create(root, &record.id)?;
    if stop.load(Ordering::SeqCst) {
        return Ok(());
    }
    let base = dir.join(format!("part-{:04}", record.parts.len()));
    let base = base.to_string_lossy().into_owned();
    // Listed before capture, so a crash or a failed capture still leaves its audio findable by
    // Recover and Write the notes again; the entry is narrowed to the files written afterwards.
    let tracks = ["mic", "system"].map(|side| format!("{base}.{side}.wav"));
    record.parts.push((tracks.to_vec(), offset_ms));
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
fn create(root: &Path, id: &str) -> Result<PathBuf, String> {
    let dir = root.join(id);
    if !dir.exists() {
        fs::create_dir(&dir).map_err(|e| e.to_string())?;
        library::save(
            root,
            &library::MeetingSummary {
                id: id.into(),
                title: "Recorded meeting".into(),
                created_ms: id
                    .parse::<u128>()
                    .map_err(|e| e.to_string())?
                    .checked_div(1_000_000)
                    .unwrap_or(0) as u64,
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
            },
        )?;
    }
    library::folder(root, id)
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
            capture_part(&root, &mut record, offset, Stop::default(), |base, stop| {
                let file = format!("{base}.mic.wav");
                fs::write(&file, b"audio").unwrap();
                stop.store(true, Ordering::SeqCst);
                Ok(vec![file])
            })
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
        capture_part(&root, &mut record, 0, Stop::default(), |base, stop| {
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
        })
        .unwrap();
        // A failed part keeps only the audio it wrote; one that wrote nothing is not listed.
        let error = capture_part(&root, &mut record, 25_000, Stop::default(), |base, _| {
            fs::write(format!("{base}.system.wav"), b"partial").unwrap();
            Err("disk full".into())
        });
        assert_eq!(error, Err("disk full".into()));
        let parts = listed(&root);
        assert_eq!(parts, record.parts);
        assert_eq!(parts.len(), 2);
        assert!(parts[1].0.len() == 1 && parts[1].0[0].ends_with("part-0001.system.wav"));
        assert_eq!(parts[1].1, 25_000);
        let ended = capture_part(&root, &mut record, 50_000, Stop::default(), |_, _| {
            Ok(vec![])
        });
        assert!(ended.unwrap_err().contains("ended unexpectedly"));
        assert_eq!(listed(&root).len(), 2);
        fs::remove_dir_all(root).unwrap();
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
            capture_part(&root, &mut record, 0, Stop::default(), |_, _| panic!(
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
        let dir = create(&root, "123").unwrap();
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
