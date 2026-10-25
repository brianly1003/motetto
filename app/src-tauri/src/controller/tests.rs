use super::*;
#[derive(Clone)]
struct Fake {
    log: Arc<Mutex<Vec<String>>>,
    stops: Arc<Mutex<Vec<Stop>>>,
    allowed: Arc<AtomicBool>,
    announce_ok: bool,
    preferences: (u32, u32, bool),
    mode: Mode,
    modes: Arc<Mutex<Vec<String>>>,
    requests: Arc<Mutex<Vec<bool>>>,
}
impl Host for Fake {
    fn preferences(&self) -> (u32, u32, bool) {
        self.preferences
    }
    fn mark(&self, id: &str, at_ms: u64) -> Result<(), String> {
        self.log.lock().unwrap().push(format!("mark:{id}:{at_ms}"));
        Ok(())
    }
    fn readiness(&self, request: bool) -> Result<Readiness, String> {
        self.requests.lock().unwrap().push(request);
        Ok(Readiness {
            microphone: self.allowed.load(Ordering::SeqCst),
            system_audio: true,
            notifications: false,
            speech_model: true,
            free_bytes: 10_000_000_000,
        })
    }
    fn event(&self, event: Event) -> Result<(), String> {
        self.log.lock().unwrap().push(match event {
            Event::StateChanged(s) => format!("state:{:?}", s.state),
            Event::NotesReady { .. } => "ready".into(),
            _ => "event".into(),
        });
        Ok(())
    }
    fn announce(&self) -> Result<(), String> {
        self.log.lock().unwrap().push("announce".into());
        if self.announce_ok {
            Ok(())
        } else {
            Err("no visible window".into())
        }
    }
    fn show_main(&self) {
        self.log.lock().unwrap().push("show-main".into());
    }
    fn job(&self, job: Job) -> Result<(), String> {
        let value = match job {
            Job::Capture {
                stop, offset_ms, ..
            } => {
                self.stops.lock().unwrap().push(stop);
                format!("capture:{offset_ms}")
            }
            Job::Finish { .. } => "finish".into(),
            Job::Discard { .. } => "discard".into(),
            Job::Trash { id } => format!("trash:{id}"),
            Job::Exit => "exit-job".into(),
        };
        self.log.lock().unwrap().push(value);
        Ok(())
    }
    fn notice(&self, message: &str) {
        self.log.lock().unwrap().push(format!("notice:{message}"));
    }
    fn cant_record(&self, body: &str) {
        self.log.lock().unwrap().push(format!("cant_record:{body}"));
    }
    fn exit(&self) {
        self.log.lock().unwrap().push("exit".into());
    }
    fn app_mode(&self, _app_id: &str) -> Mode {
        self.mode
    }
    fn set_app_mode(&self, app_id: &str, mode: Mode) -> Result<(), String> {
        self.modes
            .lock()
            .unwrap()
            .push(format!("{app_id}:{mode:?}"));
        Ok(())
    }
    fn ask_on_call(&self) -> bool {
        true
    }
    fn set_ask_on_call(&self, on: bool) -> Result<(), String> {
        self.log.lock().unwrap().push(format!("ask_on_call:{on}"));
        Ok(())
    }
}
fn engine() -> Engine<Fake> {
    Engine::new(
        Fake {
            log: Arc::default(),
            stops: Arc::default(),
            allowed: Arc::new(AtomicBool::new(true)),
            announce_ok: true,
            preferences: (30, 20, false),
            mode: Mode::Ask,
            modes: Arc::default(),
            requests: Arc::default(),
        },
        Arc::new(AtomicBool::new(false)),
        Arc::new(Mutex::new(AppState::default())),
    )
}
#[test]
fn publishes_and_announces_before_capture_and_rejects_overlap() {
    let mut e = engine();
    e.action(Action::Start, 0).unwrap();
    assert_eq!(
        &*e.host.log.lock().unwrap(),
        &["state:Recording", "announce", "capture:0"]
    );
    assert!(e.action(Action::Start, 10).is_err());
    assert!(e.busy.load(Ordering::SeqCst));
}
#[test]
fn resume_has_fresh_stop_and_excludes_pause_from_offset() {
    let mut e = engine();
    e.action(Action::Start, 0).unwrap();
    e.action(Action::Stop, 25_000).unwrap();
    assert!(e.host.stops.lock().unwrap()[0].load(Ordering::SeqCst));
    e.action(Action::Resume, 29_000).unwrap();
    let stops = e.host.stops.lock().unwrap();
    assert_eq!(stops.len(), 2);
    assert!(!stops[1].load(Ordering::SeqCst));
    assert!(!Arc::ptr_eq(&stops[0], &stops[1]));
    assert!(
        e.host
            .log
            .lock()
            .unwrap()
            .iter()
            .any(|s| s == "capture:25000")
    );
}
#[test]
fn deadline_wins_over_resume_and_processing_only_runs_once() {
    let mut e = engine();
    e.action(Action::Start, 0).unwrap();
    e.action(Action::Stop, 25_000).unwrap();
    e.action(Action::Resume, 35_000).unwrap();
    e.apply(Command::Tick, 36_000);
    assert!(matches!(
        e.session.app.state,
        SessionState::Finishing { .. }
    ));
    assert_eq!(
        e.host
            .log
            .lock()
            .unwrap()
            .iter()
            .filter(|s| *s == "finish")
            .count(),
        1
    );
}
#[test]
fn short_recording_discards_after_grace_and_releases_import_gate() {
    let mut e = engine();
    e.action(Action::Start, 0).unwrap();
    e.action(Action::Stop, 2_000).unwrap();
    e.apply(Command::Tick, 11_999);
    assert!(e.busy.load(Ordering::SeqCst));
    e.apply(Command::Tick, 12_000);
    assert!(!e.busy.load(Ordering::SeqCst));
    assert!(e.host.log.lock().unwrap().contains(&"discard".into()));
}
#[test]
fn discard_in_the_pill_trashes_without_notes_and_releases_the_gate() {
    let mut e = engine();
    e.action(Action::Start, 0).unwrap();
    // Not while recording: only the Stopped Pill offers Discard.
    e.action(Action::Discard, 30_000).unwrap();
    assert!(
        !e.host
            .log
            .lock()
            .unwrap()
            .iter()
            .any(|l| l.starts_with("trash:"))
    );
    e.action(Action::Stop, 30_000).unwrap();
    e.action(Action::Discard, 31_000).unwrap();
    let id = e.session.app.discarded.clone().unwrap().meeting_id;
    assert!(e.host.log.lock().unwrap().contains(&format!("trash:{id}")));
    assert!(
        !e.busy.load(Ordering::SeqCst),
        "a new recording or import can start"
    );
    // The grace deadline passes without notes.
    e.apply(Command::Tick, 45_000);
    assert!(!e.host.log.lock().unwrap().contains(&"finish".into()));
}
#[test]
fn blocked_permission_and_failed_announcement_never_capture() {
    let mut e = engine();
    e.host.allowed.store(false, Ordering::SeqCst);
    e.action(Action::Start, 0).unwrap();
    assert!(e.host.stops.lock().unwrap().is_empty());
    assert!(!e.busy.load(Ordering::SeqCst));
    e.host.allowed.store(true, Ordering::SeqCst);
    e.host.announce_ok = false;
    e.action(Action::Start, 1).unwrap();
    assert!(e.host.stops.lock().unwrap().is_empty());
    assert!(e.session.app.problem.is_some());
}
#[test]
fn quitting_waits_for_processing_and_failure_keeps_window_open() {
    let mut e = engine();
    e.action(Action::Start, 0).unwrap();
    let id = e.session.app.meeting_id.clone().unwrap();
    e.action(Action::RequestQuit, 100).unwrap();
    e.action(Action::Quit(false), 101).unwrap();
    assert!(!e.session.app.quit_pending);
    e.action(Action::RequestQuit, 102).unwrap();
    e.action(Action::Quit(true), 103).unwrap();
    assert!(e.host.stops.lock().unwrap()[0].load(Ordering::SeqCst));
    assert!(!e.host.log.lock().unwrap().contains(&"exit".into()));
    e.apply(
        Command::Finished {
            meeting_id: "stale".into(),
            title: "wrong".into(),
            action_items: vec![],
        },
        104,
    );
    assert!(matches!(
        e.session.app.state,
        SessionState::Finishing { .. }
    ));
    e.apply(
        Command::FinishFailed {
            meeting_id: id,
            reason: "disk full".into(),
        },
        105,
    );
    assert_eq!(e.session.app.state, SessionState::Idle);
    assert!(!e.session.app.quit_pending);
    assert!(!e.host.log.lock().unwrap().contains(&"exit".into()));
}
#[test]
fn successful_finish_emits_notes_before_quit() {
    let mut e = engine();
    e.action(Action::Start, 0).unwrap();
    let id = e.session.app.meeting_id.clone().unwrap();
    e.action(Action::RequestQuit, 100).unwrap();
    e.action(Action::Quit(true), 101).unwrap();
    e.apply(
        Command::Finished {
            meeting_id: id,
            title: "Meeting".into(),
            action_items: vec![],
        },
        102,
    );
    let log = e.host.log.lock().unwrap();
    assert!(
        log.iter().position(|s| s == "ready").unwrap()
            < log.iter().position(|s| s == "exit").unwrap()
    );
}

#[test]
fn import_reservation_prevents_recording_and_early_exit() {
    let mut e = engine();
    e.busy.store(true, Ordering::SeqCst);
    assert!(e.action(Action::Start, 0).is_err());
    assert!(e.action(Action::RequestQuit, 1).is_err());
    assert!(!e.host.log.lock().unwrap().contains(&"exit".into()));
}

#[test]
fn notification_stop_is_scoped_to_its_meeting_and_sizes_are_validated() {
    let mut e = engine();
    e.action(Action::Start, 0).unwrap();
    let id = e.session.app.meeting_id.clone().unwrap();
    assert!(e.session.app.dnd_active);
    e.action(Action::StopMeeting("older-meeting".into()), 25_000)
        .unwrap();
    assert_eq!(e.session.app.state, SessionState::Recording);
    assert!(!e.host.stops.lock().unwrap()[0].load(Ordering::SeqCst));
    e.action(Action::Size(CompanionSize::Sidecar), 25_000)
        .unwrap();
    assert_eq!(
        e.shared.lock().unwrap().companion.size,
        CompanionSize::Sidecar
    );
    e.action(Action::Size(CompanionSize::Dot), 25_000).unwrap();
    assert_eq!(e.shared.lock().unwrap().companion.size, CompanionSize::Dot);
    e.action(Action::StopMeeting(id), 26_000).unwrap();
    assert!(e.host.stops.lock().unwrap()[0].load(Ordering::SeqCst));
    assert_eq!(e.shared.lock().unwrap().companion.size, CompanionSize::Pill);
}

#[test]
fn bookmarks_use_recording_time_and_reject_stopped_sessions() {
    let mut e = engine();
    assert!(e.mark().is_err());
    e.action(Action::Start, 0).unwrap();
    e.apply(Command::Tick, 25_000);
    assert_eq!(e.mark().unwrap(), 25_000);
    e.action(Action::Stop, 25_000).unwrap();
    assert!(e.mark().is_err());
    e.action(Action::Resume, 29_000).unwrap();
    e.apply(Command::Tick, 30_000);
    assert_eq!(e.mark().unwrap(), 26_000);
}
#[test]
fn pause_stops_capture_resume_starts_a_fresh_part_and_the_long_pause_reminds() {
    let mut e = engine();
    assert!(e.action(Action::Pause, 0).is_err());
    e.action(Action::Start, 0).unwrap();
    e.action(Action::Pause, 30_000).unwrap();
    assert_eq!(e.session.app.state, SessionState::Paused);
    assert!(e.host.stops.lock().unwrap()[0].load(Ordering::SeqCst));
    // Marks need a recording: paused time is not part of the meeting.
    assert!(e.mark().is_err());
    e.apply(Command::Tick, 30_000 + 15 * 60_000);
    assert!(
        e.host
            .log
            .lock()
            .unwrap()
            .iter()
            .any(|s| s.contains("Still paused"))
    );
    e.action(Action::Resume, 31 * 60_000).unwrap();
    let stops = e.host.stops.lock().unwrap();
    assert_eq!(stops.len(), 2);
    assert!(!stops[1].load(Ordering::SeqCst));
    // The new part starts where the recorded time stopped, not at the wall clock.
    assert!(
        e.host
            .log
            .lock()
            .unwrap()
            .iter()
            .any(|s| s == "capture:30000")
    );
}

fn zoom_call(e: &mut Engine<Fake>, now: u64) -> String {
    e.watch(
        WatchEvent::Started {
            call_id: "zoom-1".into(),
            app_id: "zoom".into(),
            app: "Zoom".into(),
        },
        now,
    );
    "zoom-1".into()
}
#[test]
fn detected_call_asks_and_records_only_after_answer() {
    let mut e = engine();
    let call_id = zoom_call(&mut e, 0);
    {
        let log = e.host.log.lock().unwrap();
        assert!(log.iter().any(|s| s.starts_with("state:Asking")));
        assert!(!log.iter().any(|s| s.starts_with("capture")));
        assert!(!log.iter().any(|s| s == "announce"));
    }
    assert!(!e.busy.load(Ordering::SeqCst));
    e.action(
        Action::AnswerAsk {
            call_id,
            record: true,
        },
        1_000,
    )
    .unwrap();
    // Detection only queries permissions; Record may prompt for them.
    assert_eq!(*e.host.requests.lock().unwrap(), [false, true]);
    let log = e.host.log.lock().unwrap();
    let announce = log.iter().position(|s| s == "announce").unwrap();
    assert_eq!(log[announce + 1], "capture:0");
    assert_eq!(e.session.app.state, SessionState::Recording);
    assert!(e.busy.load(Ordering::SeqCst));
}
#[test]
fn answer_while_importing_is_refused_and_keeps_asking() {
    let mut e = engine();
    let call_id = zoom_call(&mut e, 0);
    e.busy.store(true, Ordering::SeqCst);
    assert!(
        e.action(
            Action::AnswerAsk {
                call_id,
                record: true
            },
            1_000
        )
        .is_err()
    );
    assert!(matches!(e.session.app.state, SessionState::Asking { .. }));
    assert!(e.busy.load(Ordering::SeqCst));
}
#[test]
fn not_this_call_and_stale_answers_take_no_reservation() {
    let mut e = engine();
    let call_id = zoom_call(&mut e, 0);
    e.action(
        Action::AnswerAsk {
            call_id: "zoom-0".into(),
            record: true,
        },
        500,
    )
    .unwrap();
    assert!(matches!(e.session.app.state, SessionState::Asking { .. }));
    assert!(!e.busy.load(Ordering::SeqCst));
    e.action(
        Action::AnswerAsk {
            call_id,
            record: false,
        },
        1_000,
    )
    .unwrap();
    assert_eq!(e.session.app.state, SessionState::Idle);
    assert!(!e.busy.load(Ordering::SeqCst));
    assert!(e.host.stops.lock().unwrap().is_empty());
}
#[test]
fn dont_ask_saves_off_and_undo_restores_ask() {
    let mut e = engine();
    let call_id = zoom_call(&mut e, 0);
    e.action(Action::DontAsk { call_id }, 1_000).unwrap();
    assert_eq!(e.session.app.state, SessionState::Idle);
    e.action(
        Action::UndoDontAsk {
            app_id: "zoom".into(),
        },
        2_000,
    )
    .unwrap();
    assert_eq!(*e.host.modes.lock().unwrap(), ["zoom:Off", "zoom:Ask"]);
    assert!(e.session.app.problem.is_none());
}
#[test]
fn off_app_never_asks() {
    let mut e = engine();
    e.host.mode = Mode::Off;
    zoom_call(&mut e, 0);
    assert_eq!(e.session.app.state, SessionState::Idle);
    assert!(e.host.log.lock().unwrap().is_empty());
}
#[test]
fn second_call_app_while_recording_raises_no_problem() {
    let mut e = engine();
    e.action(Action::Start, 0).unwrap();
    zoom_call(&mut e, 1_000);
    e.watch(
        WatchEvent::Mic {
            app_id: "zoom".into(),
            active: true,
        },
        2_000,
    );
    assert_eq!(e.session.app.state, SessionState::Recording);
    assert!(e.session.app.problem.is_none());
    assert!(!e.host.stops.lock().unwrap()[0].load(Ordering::SeqCst));
}
#[test]
fn hold_and_ask_on_call_reach_the_session_and_the_host() {
    let mut e = engine();
    assert!(e.session.app.ask_on_call);
    e.action(Action::AskOnCall(false), 0).unwrap();
    assert!(!e.session.app.ask_on_call);
    assert!(
        e.host
            .log
            .lock()
            .unwrap()
            .contains(&"ask_on_call:false".into())
    );
    zoom_call(&mut e, 1_000);
    assert_eq!(e.session.app.state, SessionState::Idle);
    e.action(Action::AskOnCall(true), 2_000).unwrap();
    zoom_call(&mut e, 3_000);
    e.action(Action::HoldAsk(true), 4_000).unwrap();
    e.apply(Command::Tick, 60_000);
    assert_eq!(e.session.app.companion.size, CompanionSize::Ask);
}
#[test]
fn app_mode_action_saves_the_mode_through_the_host() {
    let mut e = engine();
    e.action(
        Action::AppMode {
            app_id: "zoom".into(),
            mode: Mode::Ask,
        },
        0,
    )
    .unwrap();
    assert_eq!(*e.host.modes.lock().unwrap(), vec!["zoom:Ask".to_string()]);
    assert_eq!(e.session.app.state, SessionState::Idle);
}
#[test]
fn a_blocked_detected_call_notifies_quietly_and_a_blocked_start_still_shows_motetto() {
    let mut e = engine();
    e.host.allowed.store(false, Ordering::SeqCst);
    zoom_call(&mut e, 0);
    assert_eq!(e.session.app.state, SessionState::Idle);
    {
        let log = e.host.log.lock().unwrap();
        // `notice` is what pulls the library forward (Desktop::notice -> show()).
        assert!(!log.iter().any(|s| s.starts_with("notice:")));
        assert_eq!(
            log.iter()
                .filter(|s| s.starts_with("cant_record:"))
                .collect::<Vec<_>>(),
            ["cant_record:Microphone is off. Turn it on in System Settings."]
        );
    }
    e.action(Action::Start, 1_000).unwrap();
    let log = e.host.log.lock().unwrap();
    assert!(log.iter().any(|s| s.starts_with("notice:")));
    assert_eq!(
        log.iter().filter(|s| s.starts_with("cant_record:")).count(),
        1
    );
}
#[test]
fn record_now_while_asking_records_the_asked_call_or_releases_the_gate() {
    let mut e = engine();
    zoom_call(&mut e, 0);
    e.action(Action::Start, 1_000).unwrap();
    assert_eq!(e.session.app.state, SessionState::Recording);
    assert_eq!(e.session.app.app.as_deref(), Some("Zoom"));
    assert!(e.busy.load(Ordering::SeqCst));
    assert!(e.host.log.lock().unwrap().iter().any(|s| s == "capture:0"));

    // Blocked at Record: no recording, so the reservation is not kept.
    let mut e = engine();
    zoom_call(&mut e, 0);
    e.host.allowed.store(false, Ordering::SeqCst);
    e.action(Action::Start, 1_000).unwrap();
    assert_ne!(e.session.app.state, SessionState::Recording);
    assert!(!e.busy.load(Ordering::SeqCst));
}

#[test]
fn saved_preferences_set_timers_discard_and_initial_size() {
    let mut e = engine();
    e.host.preferences = (10, 0, true);
    e.action(Action::Start, 0).unwrap();
    assert_eq!(e.session.idle_stop_s, 10);
    assert_eq!(e.session.discard_short_s, 0);
    assert_eq!(e.session.app.companion.size, CompanionSize::Dot);
    e.action(Action::Size(CompanionSize::Sidecar), 100).unwrap();
    e.action(Action::Pause, 200).unwrap();
    e.action(Action::Resume, 300).unwrap();
    assert_eq!(e.session.app.companion.size, CompanionSize::Sidecar);
    e.action(Action::Stop, 1000).unwrap();
    e.apply(Command::Tick, 11000);
    assert!(e.host.log.lock().unwrap().contains(&"finish".into()));
    assert!(!e.host.log.lock().unwrap().contains(&"discard".into()));
}

#[test]
fn explicit_stop_restores_main_after_stopping_but_automatic_stop_does_not() {
    for paused in [false, true] {
        let mut e = engine();
        e.action(Action::Stop, 0).unwrap();
        assert!(!e.host.log.lock().unwrap().iter().any(|s| s == "show-main"));
        e.action(Action::Start, 0).unwrap();
        if paused {
            e.action(Action::Pause, 25_000).unwrap();
        }
        e.host.log.lock().unwrap().clear();
        e.action(Action::Stop, 26_000).unwrap();
        assert!(matches!(e.session.app.state, SessionState::Stopped { .. }));
        assert!(e.host.stops.lock().unwrap()[0].load(Ordering::SeqCst));
        assert!(!e.session.app.companion.visible);
        e.apply(Command::Tick, 27_000);
        assert!(
            !e.session.app.companion.visible,
            "ticks must not restore the stopped companion"
        );
        e.action(Action::Resume, 28_000).unwrap();
        assert!(e.session.app.companion.visible);
        let log = e.host.log.lock().unwrap();
        assert!(
            log.iter()
                .position(|s| s.starts_with("state:Stopped"))
                .unwrap()
                < log.iter().position(|s| s == "show-main").unwrap()
        );
    }
    let mut e = engine();
    e.action(Action::Start, 0).unwrap();
    e.host.log.lock().unwrap().clear();
    e.apply(Command::StopRecording, 26_000);
    assert!(
        e.session.app.companion.visible,
        "automatic stops keep Resume accessible without opening the library"
    );
    assert!(matches!(e.session.app.state, SessionState::Stopped { .. }));
    assert!(!e.host.log.lock().unwrap().iter().any(|s| s == "show-main"));
}
#[test]
fn notes_model_ready_resolves_only_the_missing_notes_model() {
    let mut e = engine();
    e.apply(
        Command::ProblemRaised(Problem::ModelMissing {
            kind: ModelKind::Notes,
        }),
        0,
    );
    e.apply(
        Command::ProblemRaised(Problem::DiskLow {
            free_bytes: 2_000_000_000,
        }),
        0,
    );
    assert_eq!(e.session.problems().len(), 2);
    e.action(Action::NotesModelReady, 1_000).unwrap();
    let left: Vec<_> = e.session.problems().iter().map(|p| &p.problem).collect();
    assert_eq!(
        left,
        [&Problem::DiskLow {
            free_bytes: 2_000_000_000
        }]
    );
    // Nothing pending: nothing changes.
    e.action(Action::NotesModelReady, 2_000).unwrap();
    assert_eq!(e.session.problems().len(), 1);
}
