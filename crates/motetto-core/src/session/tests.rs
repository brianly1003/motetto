use super::*;

fn ready() -> Readiness {
    Readiness {
        microphone: true,
        system_audio: true,
        notifications: true,
        speech_model: true,
        free_bytes: 10 * GB,
    }
}
fn fresh() -> Session {
    Session::new(ready())
}
fn step(s: Session, ms: u64, command: Command) -> (Session, Vec<Effect>) {
    next(
        s,
        Input {
            now_ms: ms,
            since: "2026-10-18T00:00:00Z".into(),
            command,
        },
    )
}
fn start(s: Session, notes_only: bool) -> Session {
    step(
        s,
        0,
        Command::StartManual {
            meeting_id: "m1".into(),
            notes_only,
        },
    )
    .0
}
fn call(mode: Mode) -> Call {
    Call {
        call_id: "c1".into(),
        app_id: "zoom".into(),
        app: "Zoom".into(),
        mode,
    }
}
fn detect(s: Session, mode: Mode) -> (Session, Vec<Effect>) {
    step(
        s,
        0,
        Command::CallDetected {
            call: call(mode),
            meeting_id: "m1".into(),
        },
    )
}
/// A call recorded through the Ask card, as the only way a call starts now.
fn recording() -> Session {
    let (s, _) = detect(fresh(), Mode::Ask);
    step(
        s,
        1,
        Command::AnswerAsk {
            call_id: "c1".into(),
            record: true,
        },
    )
    .0
}
fn mic(active: bool) -> Command {
    Command::MicActivity {
        app_id: "zoom".into(),
        active,
    }
}
fn work(effects: Vec<Effect>) -> Vec<Effect> {
    effects
        .into_iter()
        .filter(|e| !matches!(e, Effect::Emit(Event::StateChanged(_))))
        .collect()
}
fn finished() -> Command {
    Command::Finished {
        meeting_id: "m1".into(),
        title: "Notes".into(),
        action_items: vec![],
    }
}

#[test]
fn manual_start_publishes_visible_state_and_notice_before_capture() {
    let (s, effects) = step(
        fresh(),
        0,
        Command::StartManual {
            meeting_id: "m1".into(),
            notes_only: false,
        },
    );
    assert_eq!(s.app.state, SessionState::Recording);
    assert!(s.app.companion.visible);
    assert!(matches!(&effects[0], Effect::Emit(Event::StateChanged(a)) if a.companion.visible));
    assert_eq!(
        work(effects),
        vec![
            Effect::NotifyRecording {
                reason: StartReason::Manual
            },
            Effect::StartCapture {
                meeting_id: "m1".into(),
                resume: false
            }
        ]
    );
}

#[test]
fn detected_call_asks_and_never_captures_until_record() {
    let (s, fx) = detect(fresh(), Mode::Ask);
    assert!(matches!(s.app.state, SessionState::Asking { .. }));
    assert_eq!(
        (s.app.companion.visible, s.app.companion.size),
        (true, CompanionSize::Ask)
    );
    assert!(!fx.iter().any(|e| matches!(e, Effect::StartCapture { .. })));
    let (s, _) = step(s, 20_000, Command::Tick);
    assert_eq!(s.app.companion.size, CompanionSize::Pill); // card fades to the Pill
    let (s, _) = step(s, 600_000, Command::Tick);
    assert!(matches!(s.app.state, SessionState::Asking { .. })); // no timeout to Idle
    assert!(s.app.companion.visible);
    let (s, fx) = step(
        s,
        600_001,
        Command::AnswerAsk {
            call_id: "c1".into(),
            record: true,
        },
    );
    assert_eq!(s.app.state, SessionState::Recording);
    assert!(fx.iter().any(|e| matches!(e, Effect::StartCapture { .. })));
}

#[test]
fn hold_keeps_the_card_and_release_restarts_twenty_seconds() {
    let (s, _) = detect(fresh(), Mode::Ask);
    let (s, _) = step(s, 5_000, Command::HoldAsk(true));
    let (s, _) = step(s, 40_000, Command::Tick);
    assert_eq!(s.app.companion.size, CompanionSize::Ask);
    let (s, _) = step(s, 40_000, Command::HoldAsk(false));
    let (s, _) = step(s, 59_999, Command::Tick);
    assert_eq!(s.app.companion.size, CompanionSize::Ask);
    let (s, _) = step(s, 60_000, Command::Tick);
    assert_eq!(s.app.companion.size, CompanionSize::Pill);
}

#[test]
fn not_for_this_call_and_call_end_return_idle_without_work() {
    let (s, _) = detect(fresh(), Mode::Ask);
    let (no, fx) = step(
        s.clone(),
        10,
        Command::AnswerAsk {
            call_id: "c1".into(),
            record: false,
        },
    );
    assert_eq!(no.app.state, SessionState::Idle);
    assert!(!no.app.companion.visible);
    assert!(work(fx).is_empty());
    let (same, fx) = step(
        s.clone(),
        10,
        Command::CallEnded {
            call_id: "other".into(),
        },
    );
    assert!(matches!(same.app.state, SessionState::Asking { .. }));
    assert!(fx.is_empty());
    let (ended, fx) = step(
        s,
        10,
        Command::CallEnded {
            call_id: "c1".into(),
        },
    );
    assert_eq!(ended.app.state, SessionState::Idle);
    assert!(work(fx).is_empty());
}

#[test]
fn dont_ask_sets_off_with_undo_for_ten_seconds() {
    let (s, _) = detect(fresh(), Mode::Ask);
    let (s, fx) = step(
        s,
        1_000,
        Command::DontAskForApp {
            call_id: "c1".into(),
        },
    );
    assert_eq!(s.app.state, SessionState::Idle);
    assert_eq!(
        work(fx),
        vec![Effect::SetAppMode {
            app_id: "zoom".into(),
            mode: Mode::Off
        }]
    );
    assert!(s.app.ask_off_pending.is_some());
    assert!(s.app.companion.visible);
    assert_eq!(s.app.companion.size, CompanionSize::Pill);
    let (undone, fx) = step(
        s.clone(),
        10_999,
        Command::UndoDontAsk {
            app_id: "zoom".into(),
        },
    );
    assert_eq!(
        work(fx),
        vec![Effect::SetAppMode {
            app_id: "zoom".into(),
            mode: Mode::Ask
        }]
    );
    assert!(undone.app.ask_off_pending.is_none());
    let (late, fx) = step(
        s,
        11_000,
        Command::UndoDontAsk {
            app_id: "zoom".into(),
        },
    );
    assert!(late.app.ask_off_pending.is_none() && !late.app.companion.visible);
    assert!(work(fx).is_empty());
}

#[test]
fn off_mode_and_ask_on_call_false_never_ask() {
    let (s, fx) = detect(fresh(), Mode::Off);
    assert_eq!(s.app.state, SessionState::Idle);
    assert!(fx.is_empty());
    let (s, fx) = step(fresh(), 0, Command::SetAskOnCall(false));
    assert!(!s.app.ask_on_call);
    assert!(fx.contains(&Effect::SetAskOnCall(false)));
    let (s, fx) = detect(s, Mode::Ask);
    assert_eq!(s.app.state, SessionState::Idle);
    assert!(fx.is_empty());
}

#[test]
fn stale_record_after_call_ended_starts_nothing() {
    let (s, _) = detect(fresh(), Mode::Ask);
    let (s, _) = step(
        s,
        5,
        Command::CallEnded {
            call_id: "c1".into(),
        },
    );
    let (s, fx) = step(
        s,
        6,
        Command::AnswerAsk {
            call_id: "c1".into(),
            record: true,
        },
    );
    assert_eq!(s.app.state, SessionState::Idle);
    assert!(!fx.iter().any(|e| matches!(e, Effect::StartCapture { .. })));
}

#[test]
fn ask_accept_decline_and_stale_answers() {
    let (s, _) = detect(fresh(), Mode::Ask);
    let (unchanged, effects) = step(
        s.clone(),
        1,
        Command::AnswerAsk {
            call_id: "old".into(),
            record: true,
        },
    );
    assert_eq!(unchanged.app, s.app);
    assert!(effects.is_empty());
    let (yes, _) = step(
        s,
        10,
        Command::AnswerAsk {
            call_id: "c1".into(),
            record: true,
        },
    );
    assert_eq!(yes.app.state, SessionState::Recording);
    assert_eq!(yes.app.companion.size, CompanionSize::Pill);
}

#[test]
fn stop_closes_capture_and_resume_preserves_meeting_and_active_duration() {
    let (s, effects) = step(start(fresh(), false), 20_000, Command::StopRecording);
    assert_eq!(
        s.app.state,
        SessionState::Stopped {
            resume_until_ms: 30_000
        }
    );
    assert_eq!(work(effects), vec![Effect::StopCapture]);
    let (s, effects) = step(s, 29_999, Command::ResumeRecording);
    assert_eq!(s.app.state, SessionState::Recording);
    assert_eq!(s.app.elapsed_ms, 20_000);
    assert_eq!(s.app.meeting_id.as_deref(), Some("m1"));
    assert!(effects.contains(&Effect::StartCapture {
        meeting_id: "m1".into(),
        resume: true
    }));
    assert_eq!(step(s, 30_999, Command::Tick).0.app.elapsed_ms, 21_000);
}

#[test]
fn grace_expiry_finishes_at_threshold_and_rejects_late_resume() {
    let (s, _) = step(start(fresh(), false), 20_000, Command::StopRecording);
    let (s, effects) = step(s, 30_000, Command::ResumeRecording);
    assert_eq!(
        s.app.state,
        SessionState::Finishing {
            meeting_id: "m1".into()
        }
    );
    assert_eq!(
        work(effects),
        vec![Effect::Finish {
            meeting_id: "m1".into(),
            notes_only: false
        }]
    );
}

#[test]
fn short_recording_is_kept_through_grace_then_discarded_once() {
    let (s, _) = step(start(fresh(), false), 19_999, Command::StopRecording);
    assert!(work(step(s.clone(), 29_998, Command::Tick).1).is_empty());
    let (s, effects) = step(s, 29_999, Command::Tick);
    assert_eq!(s.app.state, SessionState::Idle);
    assert_eq!(
        work(effects),
        vec![
            Effect::Discard {
                meeting_id: "m1".into()
            },
            Effect::NotifyShort { seconds: 20 }
        ]
    );
    assert!(step(s, 30_000, Command::Tick).1.is_empty());
}

#[test]
fn zero_discard_setting_keeps_even_zero_length_recordings() {
    let mut s = fresh();
    s.discard_short_s = 0;
    let (s, _) = step(start(s, false), 0, Command::StopRecording);
    assert!(matches!(
        step(s, 10_000, Command::Tick).0.app.state,
        SessionState::Finishing { .. }
    ));
}

#[test]
fn idle_mic_timeout_resets_on_activity_and_ignores_repeat_idle_signals() {
    let s = recording();
    let s = step(s, 1_000, mic(false)).0;
    let s = step(s, 20_000, mic(false)).0;
    let s = step(s, 30_000, mic(true)).0;
    assert_eq!(
        step(s.clone(), 31_000, Command::Tick).0.app.state,
        SessionState::Recording
    );
    let s = step(s, 40_000, mic(false)).0;
    // Another app's mic never counts for this call.
    let s = step(
        s,
        50_000,
        Command::MicActivity {
            app_id: "teams".into(),
            active: true,
        },
    )
    .0;
    let (s, effects) = step(s, 70_000, Command::Tick);
    assert!(matches!(s.app.state, SessionState::Stopped { .. }));
    assert!(effects.contains(&Effect::StopCapture));
}

#[test]
fn discard_in_the_stopped_pill_trashes_the_meeting_and_offers_undo_for_five_seconds() {
    let s = recording();
    // Discard is only for a stopped recording: while recording or paused it does nothing.
    let (same, effects) = step(s.clone(), 30_000, Command::DiscardRecording);
    assert_eq!(same.app.state, SessionState::Recording);
    assert!(work(effects).is_empty());
    let s = step(s, 30_000, Command::StopRecording).0;
    assert!(matches!(s.app.state, SessionState::Stopped { .. }));
    let (s, effects) = step(s, 32_000, Command::DiscardRecording);
    assert_eq!(s.app.state, SessionState::Idle);
    assert_eq!(
        work(effects),
        vec![Effect::Trash {
            meeting_id: "m1".into()
        }]
    );
    assert_eq!(
        s.app.discarded,
        Some(Discarded {
            meeting_id: "m1".into(),
            until_ms: 37_000
        })
    );
    assert!(s.app.companion.visible, "the Pill stays for the Undo");
    // The grace deadline has gone with the meeting: no notes are written later.
    let (s, effects) = step(s, 45_000, Command::Tick);
    assert!(
        !work(effects)
            .iter()
            .any(|e| matches!(e, Effect::Finish { .. }))
    );
    assert_eq!(s.app.discarded, None);
    assert!(!s.app.companion.visible);
    // Undo ends the offer at once.
    let s = step(recording(), 30_000, Command::StopRecording).0;
    let s = step(s, 31_000, Command::DiscardRecording).0;
    let s = step(s, 32_000, Command::UndoDiscard).0;
    assert_eq!(s.app.discarded, None);
}

#[test]
fn the_idle_stop_is_published_and_keep_recording_turns_it_off_until_the_mic_returns() {
    let s = recording();
    assert_eq!(s.app.stops_at_ms, None, "someone is talking");
    // The call's app lets go of the mic at 10 s of recording: it stops at 40 s on the clock.
    let s = step(s, 10_001, mic(false)).0;
    assert_eq!(s.app.stops_at_ms, Some(s.app.elapsed_ms + 30_000));
    let s = step(s, 25_001, Command::Tick).0;
    assert_eq!(
        s.app.stops_at_ms,
        Some(40_000),
        "the deadline holds still between ticks"
    );
    let (s, effects) = step(s, 26_000, Command::KeepRecording);
    assert_eq!(s.app.stops_at_ms, None);
    assert!(work(effects).is_empty());
    // Quiet and the call's end no longer stop it...
    let s = step(s, 27_000, mic(false)).0;
    let s = step(
        s,
        28_000,
        Command::CallEnded {
            call_id: "c1".into(),
        },
    )
    .0;
    let s = step(s, 200_000, Command::Tick).0;
    assert_eq!(s.app.state, SessionState::Recording);
    assert_eq!(s.app.stops_at_ms, None);
    // ...until the mic comes back; the next quiet counts down again.
    let s = step(s, 201_000, mic(true)).0;
    let s = step(s, 202_000, mic(false)).0;
    assert!(s.app.stops_at_ms.is_some());
    let s = step(s, 232_000, Command::Tick).0;
    assert!(matches!(s.app.state, SessionState::Stopped { .. }));
    assert_eq!(s.app.stops_at_ms, None);
}

#[test]
fn manual_recording_idle_stops_on_any_app_but_a_call_ignores_other_apps() {
    let idle_zoom = |s: Session| {
        let s = step(s, 1_000, mic(false)).0;
        step(s, 31_000, Command::Tick).0
    };
    let s = idle_zoom(start(fresh(), false));
    assert!(matches!(s.app.state, SessionState::Stopped { .. }));
    let other = Command::MicActivity {
        app_id: "teams".into(),
        active: false,
    };
    let s = step(recording(), 1_000, other).0;
    let s = step(s, 31_000, Command::Tick).0;
    assert_eq!(s.app.state, SessionState::Recording);
}

#[test]
fn app_quit_stops_only_the_matching_call() {
    let s = recording();
    assert_eq!(
        step(
            s.clone(),
            1,
            Command::AppQuit {
                app_id: "other".into()
            }
        )
        .0
        .app
        .state,
        SessionState::Recording
    );
    assert!(matches!(
        step(
            s,
            2,
            Command::AppQuit {
                app_id: "zoom".into()
            }
        )
        .0
        .app
        .state,
        SessionState::Stopped { .. }
    ));
}

#[test]
fn notes_only_needs_no_audio_permissions_ignores_idle_and_skips_grace() {
    let s = Session::new(Readiness {
        free_bytes: 10 * GB,
        ..Readiness::default()
    });
    let (s, effects) = step(
        s,
        0,
        Command::StartManual {
            meeting_id: "m1".into(),
            notes_only: true,
        },
    );
    assert_eq!(s.app.started_by, Some(StartReason::NotesOnly));
    assert!(work(effects).is_empty());
    let s = step(s, 1, mic(false)).0;
    assert_eq!(
        step(s.clone(), 40_000, Command::Tick).0.app.state,
        SessionState::Recording
    );
    let (s, effects) = step(s, 2, Command::StopRecording);
    assert!(matches!(s.app.state, SessionState::Finishing { .. }));
    assert_eq!(
        work(effects),
        vec![Effect::Finish {
            meeting_id: "m1".into(),
            notes_only: true
        }]
    );
}

#[test]
fn quit_question_cancel_and_stop_and_save_bypass_short_discard() {
    for stopped in [false, true] {
        let mut s = start(fresh(), false);
        if stopped {
            s = step(s, 100, Command::StopRecording).0;
        }
        let (s, effects) = step(s, 200, Command::RequestQuit);
        assert!(s.app.quit_pending);
        assert!(work(effects).is_empty());
        assert!(
            !step(
                s.clone(),
                300,
                Command::Quit {
                    stop_and_save: false
                }
            )
            .0
            .app
            .quit_pending
        );
        let (s, effects) = step(
            s,
            300,
            Command::Quit {
                stop_and_save: true,
            },
        );
        assert!(matches!(s.app.state, SessionState::Finishing { .. }));
        assert_eq!(effects.contains(&Effect::StopCapture), !stopped);
        assert!(effects.contains(&Effect::Finish {
            meeting_id: "m1".into(),
            notes_only: false
        }));
        let (s, effects) = step(s, 400, finished());
        assert_eq!(s.app.state, SessionState::Idle);
        assert!(effects.contains(&Effect::Exit));
    }
}

#[test]
fn quit_in_finishing_waits_or_marks_recovery_on_quit_now() {
    let s = start(fresh(), true);
    let s = step(s, 1, Command::StopRecording).0;
    let (s, effects) = step(s, 2, Command::RequestQuit);
    assert!(s.app.quit_pending && s.app.companion.visible);
    assert!(!effects.contains(&Effect::Exit));
    let (_, effects) = step(s, 3, Command::QuitNow);
    assert_eq!(
        work(effects),
        vec![
            Effect::MarkRecoverable {
                meeting_id: "m1".into()
            },
            Effect::PauseDownload,
            Effect::Exit
        ]
    );
    assert_eq!(
        work(step(fresh(), 0, Command::RequestQuit).1),
        vec![Effect::PauseDownload, Effect::Exit]
    );
}

#[test]
fn completion_and_failure_return_idle_and_ignore_stale_workers() {
    let s = step(start(fresh(), true), 1, Command::StopRecording).0;
    let (unchanged, effects) = step(
        s.clone(),
        2,
        Command::FinishFailed {
            meeting_id: "old".into(),
            reason: "bad".into(),
        },
    );
    assert_eq!(unchanged.app, s.app);
    assert!(effects.is_empty());
    let (done, effects) = step(s.clone(), 2, finished());
    assert_eq!(done.app.state, SessionState::Idle);
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::Emit(Event::NotesReady { .. })))
    );
    let (failed, effects) = step(
        s,
        2,
        Command::FinishFailed {
            meeting_id: "m1".into(),
            reason: "disk error".into(),
        },
    );
    assert_eq!(failed.app.state, SessionState::Idle);
    assert!(matches!(
        failed.app.problem.unwrap().problem,
        Problem::NotesFailed { .. }
    ));
    assert!(!effects.contains(&Effect::Exit));
}

#[test]
fn stopping_the_notes_returns_idle_without_a_problem() {
    let s = step(start(fresh(), true), 1, Command::StopRecording).0;
    let stop = |id: &str| Command::FinishStopped {
        meeting_id: id.into(),
    };
    let (unchanged, effects) = step(s.clone(), 2, stop("old"));
    assert_eq!(unchanged.app, s.app);
    assert!(effects.is_empty());
    let (stopped, effects) = step(s, 2, stop("m1"));
    assert_eq!(stopped.app.state, SessionState::Idle);
    assert!(stopped.app.problem.is_none());
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::Emit(Event::NotesReady { .. })))
    );
}

#[test]
fn extra_calls_are_added_once_without_starting_another_recording() {
    let s = recording();
    let mut other = call(Mode::Ask);
    other.app_id = "teams".into();
    other.call_id = "c2".into();
    let command = Command::CallDetected {
        call: other,
        meeting_id: "m2".into(),
    };
    let paused = step(s.clone(), 2, Command::PauseRecording).0;
    let stopped = step(s.clone(), 2, Command::StopRecording).0;
    for s in [s, paused, stopped] {
        let before = s.app.state.clone();
        let (s, effects) = step(s, 3, command.clone());
        assert_eq!(s.app.state, before);
        assert_eq!(s.app.meeting_id.as_deref(), Some("m1"));
        assert_eq!(
            work(effects),
            vec![Effect::AddApp {
                meeting_id: "m1".into(),
                app_id: "teams".into()
            }]
        );
        assert!(work(step(s, 4, command.clone()).1).is_empty());
    }
}

#[test]
fn missing_permissions_models_and_disk_block_start_with_prioritized_problems() {
    let (s, effects) = detect(Session::new(Readiness::default()), Mode::Ask);
    assert_eq!(s.app.state, SessionState::Idle);
    assert_eq!(s.app.problem.unwrap().problem, Problem::DiskFull);
    assert_eq!(s.app.more_problems, 3);
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::StartCapture { .. }))
    );
    // A detected call only gets one quiet notice for the top blocker, never the user-start notice.
    assert_eq!(
        work(effects),
        vec![Effect::CantRecord {
            problem: Problem::DiskFull
        }]
    );
    let (_, effects) = step(
        Session::new(Readiness::default()),
        0,
        Command::StartManual {
            meeting_id: "m1".into(),
            notes_only: false,
        },
    );
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::NotifyProblem { .. }))
    );
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::CantRecord { .. }))
    );
}

#[test]
fn disk_boundaries_warn_then_clear_and_stop_on_full() {
    let mut checks = ready();
    checks.free_bytes = GB;
    let s = start(Session::new(checks), false);
    assert_eq!(s.app.state, SessionState::Recording);
    assert!(matches!(
        s.app.problem.as_ref().unwrap().problem,
        Problem::DiskLow { .. }
    ));
    let s = step(s, 1, Command::ReadinessChanged(ready())).0;
    assert!(s.app.problem.is_none());
    let mut checks = ready();
    checks.free_bytes = GB - 1;
    let (s, effects) = step(s, 2, Command::ReadinessChanged(checks));
    assert!(matches!(s.app.state, SessionState::Stopped { .. }));
    assert_eq!(s.app.problem.unwrap().problem, Problem::DiskFull);
    assert!(effects.contains(&Effect::StopCapture));
}

#[test]
fn problems_are_deduplicated_prioritized_and_clear_without_changing_session() {
    let mut s = start(fresh(), true);
    for problem in [
        Problem::Slow,
        Problem::DiskLow { free_bytes: 2 * GB },
        Problem::NotesFailed {
            meeting_id: "old".into(),
            reason: "failed".into(),
        },
        Problem::ToolFailed {
            tool: "notes".into(),
            stderr: "bad".into(),
        },
        Problem::ModelMissing {
            kind: ModelKind::Notes,
        },
        Problem::PermissionMissing {
            kind: PermissionKind::Notifications,
            side: Side::TheCall,
        },
        Problem::DiskFull,
        Problem::DiskFull,
    ] {
        s = step(s, 0, Command::ProblemRaised(problem)).0;
    }
    assert_eq!(s.app.state, SessionState::Recording);
    assert_eq!(s.app.more_problems, 6);
    for (rank, command_kind) in (0..7).zip([0, 1, 2, 0, 1, 2, 0]) {
        let problem = s.app.problem.clone().unwrap();
        assert_eq!(priority(&problem.problem), rank);
        let command = match command_kind {
            0 => Command::DismissProblem { id: problem.id },
            1 => Command::ProblemResolved { id: problem.id },
            _ => Command::RetryProblem { id: problem.id },
        };
        let (next_s, effects) = step(s, 0, command);
        if command_kind == 2 {
            assert!(effects.contains(&Effect::Retry { problem }));
        }
        s = next_s;
    }
    assert!(s.app.problem.is_none());
    assert_eq!(s.app.more_problems, 0);
}

#[test]
fn readiness_changes_never_touch_ask_on_call() {
    let mut denied = ready();
    denied.notifications = false;
    let s = step(fresh(), 0, Command::ReadinessChanged(denied)).0;
    assert!(s.app.ask_on_call);
    let s = step(
        s,
        1,
        Command::ProblemRaised(Problem::PermissionMissing {
            kind: PermissionKind::Notifications,
            side: Side::TheCall,
        }),
    )
    .0;
    assert!(s.app.ask_on_call);
    assert_eq!(
        detect(s, Mode::Ask).0.app.state.clone(),
        SessionState::Asking {
            call_id: "c1".into(),
            app: "Zoom".into()
        }
    );
}

#[test]
fn interrupted_audio_keeps_meeting_and_inserts_pause_marker() {
    let (s, effects) = step(start(fresh(), false), 1_000, Command::AudioInterrupted);
    assert_eq!(s.app.state, SessionState::Recording);
    assert_eq!(
        work(effects),
        vec![
            Effect::StopCapture,
            Effect::AudioPaused {
                meeting_id: "m1".into(),
                at_ms: 1000
            },
            Effect::StartCapture {
                meeting_id: "m1".into(),
                resume: true
            }
        ]
    );
}

#[test]
fn recoverable_meetings_are_offered_without_starting_capture() {
    let (s, effects) = step(
        fresh(),
        0,
        Command::Recoverable {
            meeting_ids: vec!["m1".into()],
        },
    );
    assert_eq!(s.app.state, SessionState::Idle);
    assert_eq!(
        effects,
        vec![Effect::OfferRecovery {
            meeting_ids: vec!["m1".into()]
        }]
    );
}

#[test]
fn elapsed_only_updates_are_throttled_and_backwards_time_cannot_underflow() {
    let s = start(fresh(), false);
    let (s, effects) = step(s, 999, Command::Tick);
    assert!(effects.is_empty());
    let (s, effects) = step(s, 1000, Command::Tick);
    assert!(matches!(
        &effects[..],
        [Effect::Emit(Event::StateChanged(_))]
    ));
    let (s, effects) = step(s, 0, Command::Tick);
    assert_eq!(s.app.elapsed_ms, 1000);
    assert!(effects.is_empty());
}

#[test]
fn invalid_commands_do_not_start_or_stop_work() {
    for command in [
        Command::StopRecording,
        Command::ResumeRecording,
        Command::QuitNow,
        Command::UndoDontAsk {
            app_id: "zoom".into(),
        },
        Command::DontAskForApp {
            call_id: "old".into(),
        },
        Command::CallEnded {
            call_id: "old".into(),
        },
        Command::Quit {
            stop_and_save: true,
        },
        finished(),
    ] {
        let (s, effects) = step(fresh(), 0, command);
        assert_eq!(s.app.state, SessionState::Idle);
        assert!(effects.is_empty());
    }
}

#[test]
fn raised_disk_and_permission_failures_cannot_be_bypassed_by_dismiss_then_resume() {
    for problem in [
        Problem::DiskFull,
        Problem::PermissionMissing {
            kind: PermissionKind::Microphone,
            side: Side::YourMicrophone,
        },
    ] {
        let s = start(fresh(), false);
        let s = step(s, 100, Command::ProblemRaised(problem)).0;
        let id = s.app.problem.as_ref().unwrap().id;
        let s = step(s, 101, Command::DismissProblem { id }).0;
        let (s, effects) = step(s, 102, Command::ResumeRecording);
        assert!(matches!(s.app.state, SessionState::Stopped { .. }));
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::StartCapture { .. }))
        );
        let s = step(s, 103, Command::ReadinessChanged(ready())).0;
        assert_eq!(
            step(s, 104, Command::ResumeRecording).0.app.state,
            SessionState::Recording
        );
    }
}

#[test]
fn readiness_revocation_stops_audio_and_low_disk_warns_during_capture() {
    let mut checks = ready();
    checks.free_bytes = 4 * GB;
    let s = step(
        start(fresh(), false),
        100,
        Command::ReadinessChanged(checks.clone()),
    )
    .0;
    assert!(matches!(
        s.app.problem.unwrap().problem,
        Problem::DiskLow { .. }
    ));
    checks.microphone = false;
    let (s, effects) = step(
        start(fresh(), false),
        100,
        Command::ReadinessChanged(checks),
    );
    assert!(matches!(s.app.state, SessionState::Stopped { .. }));
    assert!(effects.contains(&Effect::StopCapture));
}

#[test]
fn cancelling_quit_during_finishing_hides_companion_again() {
    let s = step(start(fresh(), true), 1, Command::StopRecording).0;
    let s = step(s, 2, Command::RequestQuit).0;
    let s = step(
        s,
        3,
        Command::Quit {
            stop_and_save: false,
        },
    )
    .0;
    assert!(!s.app.quit_pending && !s.app.companion.visible);
}

#[test]
fn quit_pending_short_expiry_discards_and_exits() {
    let s = step(start(fresh(), false), 1, Command::StopRecording).0;
    let s = step(s, 2, Command::RequestQuit).0;
    let (s, effects) = step(s, 10_001, Command::Tick);
    assert_eq!(s.app.state, SessionState::Idle);
    assert!(effects.contains(&Effect::Discard {
        meeting_id: "m1".into()
    }));
    assert!(effects.contains(&Effect::Exit));
}

#[test]
fn finish_failure_cancels_pending_quit_and_clears_slow_warning() {
    let s = step(
        start(fresh(), true),
        0,
        Command::ProblemRaised(Problem::Slow),
    )
    .0;
    let s = step(s, 1, Command::StopRecording).0;
    let s = step(s, 2, Command::RequestQuit).0;
    let (s, effects) = step(
        s,
        3,
        Command::FinishFailed {
            meeting_id: "m1".into(),
            reason: "failed".into(),
        },
    );
    assert!(!s.app.quit_pending);
    assert_eq!(s.app.more_problems, 0);
    assert!(matches!(
        s.app.problem.unwrap().problem,
        Problem::NotesFailed { .. }
    ));
    assert!(!effects.contains(&Effect::Exit));
}

#[test]
fn expiry_that_exits_cannot_start_another_recording() {
    let s = step(start(fresh(), false), 1, Command::StopRecording).0;
    let s = step(s, 2, Command::RequestQuit).0;
    let (s, effects) = step(
        s,
        10_001,
        Command::StartManual {
            meeting_id: "m2".into(),
            notes_only: false,
        },
    );
    assert_eq!(s.app.state, SessionState::Idle);
    assert!(effects.contains(&Effect::Exit));
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::StartCapture { .. }))
    );
}

#[test]
fn companion_sizes_follow_recording_stop_quit_and_deadline() {
    let s = start(fresh(), false);
    let (s, effects) = step(s, 25_000, Command::SetCompanionSize(CompanionSize::Dot));
    assert_eq!(s.app.companion.size, CompanionSize::Dot);
    assert!(s.app.companion.visible);
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::Emit(Event::StateChanged(_))))
    );
    let (s, _) = step(s, 26_000, Command::StopRecording);
    assert_eq!(s.app.companion.size, CompanionSize::Pill);
    let (s, _) = step(s, 27_000, Command::SetCompanionSize(CompanionSize::Dot));
    assert_eq!(s.app.companion.size, CompanionSize::Pill);
    let (s, _) = step(s, 36_000, Command::Tick);
    assert!(!s.app.companion.visible);
    let (s, _) = step(s, 36_001, Command::RequestQuit);
    assert!(s.app.companion.visible);
    assert_eq!(s.app.companion.size, CompanionSize::Pill);

    let s = start(fresh(), false);
    let (s, _) = step(s, 1, Command::SetCompanionSize(CompanionSize::Sidecar));
    assert_eq!(s.app.companion.size, CompanionSize::Sidecar);
    let (s, _) = step(s, 2, Command::SetCompanionSize(CompanionSize::Dot));
    let (s, _) = step(s, 2, Command::SetCompanionSize(CompanionSize::Ask));
    assert_eq!(s.app.companion.size, CompanionSize::Dot);
    let (s, _) = step(s, 3, Command::RequestQuit);
    assert_eq!(s.app.companion.size, CompanionSize::Pill);
}

#[test]
fn unverified_sharing_hides_text_and_peek_expires_even_when_pinned() {
    let s = start(fresh(), false);
    let (s, _) = step(s, 1, Command::SetCompanionSize(CompanionSize::Sidecar));
    let (s, _) = step(s, 2, Command::SetSharing(true));
    assert_eq!(s.app.companion.size, CompanionSize::Dot);
    assert_eq!(s.app.sharing.as_ref().unwrap().hidden_from_capture, None);
    let (s, _) = step(s, 3, Command::SetCompanionSize(CompanionSize::Pill));
    assert_eq!(s.app.companion.size, CompanionSize::Dot);
    let (s, _) = step(s, 4, Command::SetCompanionPinned(true));
    let (s, _) = step(s, 5, Command::SetCompanionSize(CompanionSize::Sidecar));
    assert!(s.app.companion.peeking);
    let (s, _) = step(s, 15_004, Command::Tick);
    assert_eq!(s.app.companion.size, CompanionSize::Sidecar);
    let (s, _) = step(s, 15_005, Command::Tick);
    assert_eq!(s.app.companion.size, CompanionSize::Dot);
    assert!(!s.app.companion.peeking);
    let (s, _) = step(s, 15_006, Command::SetSharing(false));
    assert_eq!(s.app.companion.size, CompanionSize::Sidecar);
    assert!(s.app.sharing.is_none());
}

#[test]
fn pause_stops_capture_keeps_the_meeting_and_freezes_the_clock() {
    let s = step(
        start(fresh(), false),
        0,
        Command::SetCompanionSize(CompanionSize::Sidecar),
    )
    .0;
    let (s, effects) = step(s, 20_000, Command::PauseRecording);
    assert_eq!(s.app.state, SessionState::Paused);
    assert_eq!(work(effects), vec![Effect::StopCapture]);
    // Still visible and still a Sidecar: Pause is not the end of the call.
    assert!(s.app.companion.visible);
    assert_eq!(s.app.companion.size, CompanionSize::Sidecar);
    // No Resume deadline: an hour later it is still paused, and paused time is not counted.
    let s = step(s, 3_620_000, Command::Tick).0;
    assert_eq!(s.app.state, SessionState::Paused);
    assert_eq!(s.app.elapsed_ms, 20_000);
    let (s, effects) = step(s, 3_620_000, Command::ResumeRecording);
    assert_eq!(s.app.state, SessionState::Recording);
    assert_eq!(s.app.meeting_id.as_deref(), Some("m1"));
    assert!(effects.contains(&Effect::StartCapture {
        meeting_id: "m1".into(),
        resume: true
    }));
    assert_eq!(step(s, 3_625_000, Command::Tick).0.app.elapsed_ms, 25_000);
}

#[test]
fn switching_the_mic_starts_a_new_part_only_while_recording() {
    let s = start(fresh(), false);
    let (s, effects) = step(s, 20_000, Command::SwitchMicrophone);
    assert_eq!(s.app.state, SessionState::Recording);
    assert_eq!(
        work(effects.clone()),
        vec![
            Effect::StopCapture,
            Effect::StartCapture {
                meeting_id: "m1".into(),
                resume: true
            }
        ]
    );
    // Already announced: no second "Recording" notice.
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::NotifyRecording { .. }))
    );
    assert_eq!(
        step(s.clone(), 25_000, Command::Tick).0.app.elapsed_ms,
        25_000
    );
    // Paused, the next Resume picks the new mic up; notes-only has no mic to switch.
    let paused = step(s, 30_000, Command::PauseRecording).0;
    assert!(work(step(paused, 31_000, Command::SwitchMicrophone).1).is_empty());
    let notes = start(fresh(), true);
    assert!(work(step(notes, 1_000, Command::SwitchMicrophone).1).is_empty());
}

#[test]
fn stop_while_paused_starts_the_resume_window_without_stopping_capture_twice() {
    let s = step(start(fresh(), false), 30_000, Command::PauseRecording).0;
    let (s, effects) = step(s, 40_000, Command::StopRecording);
    assert_eq!(
        s.app.state,
        SessionState::Stopped {
            resume_until_ms: 50_000
        }
    );
    assert!(work(effects).is_empty());
    let (s, effects) = step(s, 50_000, Command::Tick);
    assert!(matches!(s.app.state, SessionState::Finishing { .. }));
    assert!(effects.contains(&Effect::Finish {
        meeting_id: "m1".into(),
        notes_only: false
    }));
}

#[test]
fn a_long_pause_reminds_once_at_fifteen_minutes() {
    let s = step(start(fresh(), false), 0, Command::PauseRecording).0;
    let (s, effects) = step(s, 899_999, Command::Tick);
    assert!(!effects.contains(&Effect::NotifyStillPaused));
    let (s, effects) = step(s, 900_000, Command::Tick);
    assert!(effects.contains(&Effect::NotifyStillPaused));
    let (s, effects) = step(s, 1_900_000, Command::Tick);
    assert!(!effects.contains(&Effect::NotifyStillPaused));
    // A new pause gets a new reminder.
    let s = step(s, 1_900_000, Command::ResumeRecording).0;
    let s = step(s, 1_910_000, Command::PauseRecording).0;
    assert!(
        step(s, 2_810_000, Command::Tick)
            .1
            .contains(&Effect::NotifyStillPaused)
    );
}

#[test]
fn pause_is_ignored_outside_recording_and_calls_do_not_start_while_paused() {
    let (s, effects) = step(fresh(), 0, Command::PauseRecording);
    assert_eq!(s.app.state, SessionState::Idle);
    assert!(work(effects).is_empty());
    let notes_only = start(fresh(), true);
    assert_eq!(
        step(notes_only, 1_000, Command::PauseRecording).0.app.state,
        SessionState::Recording
    );
    let s = step(start(fresh(), false), 1_000, Command::PauseRecording).0;
    let (s, effects) = detect(s, Mode::Ask);
    assert_eq!(s.app.state, SessionState::Paused);
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::StartCapture { .. }))
    );
    // Quitting while paused asks first, and Stop and save finishes without stopping capture again.
    let s = step(s, 2_000, Command::RequestQuit).0;
    assert!(s.app.quit_pending);
    let (s, effects) = step(
        s,
        2_000,
        Command::Quit {
            stop_and_save: true,
        },
    );
    assert!(matches!(s.app.state, SessionState::Finishing { .. }));
    assert!(!effects.contains(&Effect::StopCapture));
}

fn teams(meeting_id: &str) -> Command {
    Command::CallDetected {
        call: Call {
            call_id: "c2".into(),
            app_id: "teams".into(),
            app: "Teams".into(),
            mode: Mode::Ask,
        },
        meeting_id: meeting_id.into(),
    }
}
fn asks(effects: &[Effect], call_id: &str) -> bool {
    effects.iter().any(
        |e| matches!(e, Effect::Emit(Event::CallDetected { call_id: id, .. }) if id == call_id),
    )
}

#[test]
fn a_second_call_while_asking_replaces_the_question_and_restarts_the_card() {
    let (s, _) = detect(fresh(), Mode::Ask);
    let (s, _) = step(s, 25_000, Command::Tick);
    assert_eq!(s.app.companion.size, CompanionSize::Pill);
    let (s, fx) = step(s, 25_000, teams("m2"));
    assert!(asks(&fx, "c2"));
    assert_eq!(
        s.app.state,
        SessionState::Asking {
            call_id: "c2".into(),
            app: "Teams".into()
        }
    );
    assert_eq!(s.app.companion.size, CompanionSize::Ask);
    assert_eq!(
        step(s.clone(), 44_999, Command::Tick).0.app.companion.size,
        CompanionSize::Ask
    );
    assert_eq!(
        step(s.clone(), 45_000, Command::Tick).0.app.companion.size,
        CompanionSize::Pill
    );
    // The replaced question can no longer be answered; the new one records its own meeting.
    let answer = |call_id: &str| Command::AnswerAsk {
        call_id: call_id.into(),
        record: true,
    };
    let (s, fx) = step(s, 26_000, answer("c1"));
    assert!(matches!(s.app.state, SessionState::Asking { .. }));
    assert!(work(fx).is_empty());
    let (s, _) = step(s, 27_000, answer("c2"));
    assert_eq!(s.app.state, SessionState::Recording);
    assert_eq!(s.app.meeting_id.as_deref(), Some("m2"));
    assert_eq!(s.app.app.as_deref(), Some("Teams"));
}

fn finishing() -> Session {
    let s = step(recording(), 30_000, Command::Tick).0;
    let s = step(s, 30_000, Command::StopRecording).0;
    let s = step(s, 40_000, Command::Tick).0;
    assert!(matches!(s.app.state, SessionState::Finishing { .. }));
    s
}

#[test]
fn a_call_detected_while_finishing_asks_once_the_notes_are_done() {
    let (s, fx) = step(finishing(), 41_000, teams("m2"));
    assert!(!asks(&fx, "c2"));
    assert!(matches!(s.app.state, SessionState::Finishing { .. }));
    let (s, fx) = step(s, 50_000, finished());
    assert!(asks(&fx, "c2"));
    assert_eq!(
        s.app.state,
        SessionState::Asking {
            call_id: "c2".into(),
            app: "Teams".into()
        }
    );
    assert_eq!(s.app.companion.size, CompanionSize::Ask);
    let (s, _) = step(
        s,
        51_000,
        Command::AnswerAsk {
            call_id: "c2".into(),
            record: true,
        },
    );
    assert_eq!(s.app.meeting_id.as_deref(), Some("m2"));

    // A failed finish asks too.
    let s = step(finishing(), 41_000, teams("m2")).0;
    let (s, fx) = step(
        s,
        50_000,
        Command::FinishFailed {
            meeting_id: "m1".into(),
            reason: "model".into(),
        },
    );
    assert!(asks(&fx, "c2"));
    assert!(matches!(s.app.state, SessionState::Asking { .. }));

    // A call that ended meanwhile is not asked about.
    let s = step(finishing(), 41_000, teams("m2")).0;
    let s = step(
        s,
        45_000,
        Command::CallEnded {
            call_id: "c2".into(),
        },
    )
    .0;
    let (s, fx) = step(s, 50_000, finished());
    assert!(!asks(&fx, "c2"));
    assert_eq!(s.app.state, SessionState::Idle);
    // Nor once the user turned asking off.
    let s = step(finishing(), 41_000, teams("m2")).0;
    let s = step(s, 42_000, Command::SetAskOnCall(false)).0;
    let (s, fx) = step(s, 50_000, finished());
    assert!(!asks(&fx, "c2"));
    assert_eq!(s.app.state, SessionState::Idle);
}

#[test]
fn a_call_that_ends_while_recording_starts_the_idle_stop() {
    // Record was pressed after the mic went quiet, so no Mic{false} reached the recording.
    let s = recording();
    let s = step(
        s,
        5_000,
        Command::CallEnded {
            call_id: "c1".into(),
        },
    )
    .0;
    assert_eq!(
        step(s.clone(), 34_999, Command::Tick).0.app.state,
        SessionState::Recording
    );
    let (s, fx) = step(s, 35_000, Command::Tick);
    assert!(matches!(s.app.state, SessionState::Stopped { .. }));
    assert!(fx.contains(&Effect::StopCapture));
    // Another call's end never stops this recording.
    let s = step(
        recording(),
        5_000,
        Command::CallEnded {
            call_id: "other".into(),
        },
    )
    .0;
    assert_eq!(
        step(s, 60_000, Command::Tick).0.app.state,
        SessionState::Recording
    );
}
