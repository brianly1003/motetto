//! Pure desktop recording rules. Time, readiness and worker outcomes are explicit inputs.

use crate::events::*;

const GRACE_MS: u64 = 10_000;
/// The Ask card fades to the Pill after this long (DESIGN 4.2); the question itself never times out.
const ASK_CARD_MS: u64 = 20_000;
/// How long the Pill offers Undo after Discard (SPEC 7.7: the same 5 s as Delete).
const DISCARD_UNDO_MS: u64 = 5_000;
/// One reminder when a pause runs this long, so a forgotten pause doesn't lose the meeting.
const PAUSE_REMIND_MS: u64 = 15 * 60_000;
const GB: u64 = 1_000_000_000;

/// No clock or I/O is read by the reducer. `since` is a caller-supplied wall-clock timestamp.
#[derive(Debug, Clone)]
pub struct Input {
    pub now_ms: u64,
    pub since: String,
    pub command: Command,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Emit(Event),
    NotifyRecording {
        reason: StartReason,
    },
    NotifyProblem {
        id: u64,
    },
    /// A detected call can't be asked about: an OS notice only, nothing pulled forward (SPEC 7.5).
    CantRecord {
        problem: Problem,
    },
    NotifyShort {
        seconds: u32,
    },
    NotifyStillPaused,
    StartCapture {
        meeting_id: String,
        resume: bool,
    },
    StopCapture,
    Finish {
        meeting_id: String,
        notes_only: bool,
    },
    Discard {
        meeting_id: String,
    },
    /// Discard chosen in the Pill: move the meeting to Recently deleted (recoverable), no notes.
    Trash {
        meeting_id: String,
    },
    AddApp {
        meeting_id: String,
        app_id: String,
    },
    SetAppMode {
        app_id: String,
        mode: Mode,
    },
    SetAskOnCall(bool),
    AudioPaused {
        meeting_id: String,
        at_ms: u64,
    },
    Retry {
        problem: ProblemInfo,
    },
    OfferRecovery {
        meeting_ids: Vec<String>,
    },
    MarkRecoverable {
        meeting_id: String,
    },
    PauseDownload,
    Exit,
}

#[derive(Debug, Clone)]
struct PendingAsk {
    call: Call,
    meeting_id: String,
    /// None while the pointer holds the card.
    card_until_ms: Option<u64>,
}

/// Reducer state, including private timers and the full problem queue behind `AppState`.
#[derive(Debug, Clone)]
pub struct Session {
    pub app: AppState,
    pub discard_short_s: u32,
    pub idle_stop_s: u32,
    readiness: Readiness,
    ask: Option<PendingAsk>,
    /// A call detected while the last meeting's notes were written; asked once they are done.
    queued: Option<(Call, String)>,
    call: Option<Call>,
    apps: Vec<String>,
    problems: Vec<ProblemInfo>,
    next_problem_id: u64,
    last_ms: u64,
    last_emit_ms: u64,
    idle_since: Option<u64>,
    /// Keep recording was chosen during the idle countdown; the mic coming back clears it.
    keep: bool,
    peek_until: Option<u64>,
    before_share: CompanionSize,
    pause_remind_at: Option<u64>,
}

impl Session {
    pub fn new(readiness: Readiness) -> Self {
        Self {
            app: AppState::default(),
            discard_short_s: 20,
            idle_stop_s: 30,
            readiness,
            ask: None,
            queued: None,
            call: None,
            apps: Vec::new(),
            problems: Vec::new(),
            next_problem_id: 1,
            last_ms: 0,
            last_emit_ms: 0,
            idle_since: None,
            keep: false,
            peek_until: None,
            before_share: CompanionSize::Pill,
            pause_remind_at: None,
        }
    }

    fn audio(&self) -> bool {
        self.app.started_by != Some(StartReason::NotesOnly)
    }

    fn idle(&mut self) {
        self.app.state = SessionState::Idle;
        self.app.meeting_id = None;
        self.app.app = None;
        self.app.elapsed_ms = 0;
        self.app.started_by = None;
        self.ask = None;
        self.call = None;
        self.apps.clear();
        self.idle_since = None;
        self.keep = false;
        self.pause_remind_at = None;
    }

    fn problem(&mut self, problem: Problem, since: &str) -> u64 {
        if let Some(existing) = self
            .problems
            .iter_mut()
            .find(|p| same_cause(&p.problem, &problem))
        {
            existing.problem = problem;
            return existing.id;
        }
        let id = self.next_problem_id;
        self.next_problem_id = self.next_problem_id.saturating_add(1);
        self.problems.push(ProblemInfo {
            id,
            problem,
            since: since.into(),
        });
        id
    }

    fn can_start(&mut self, notes_only: bool, since: &str, effects: &mut Vec<Effect>) -> bool {
        let mut blocked = Vec::new();
        if self.readiness.free_bytes < GB {
            blocked.push(Problem::DiskFull);
        }
        if !notes_only {
            for (allowed, kind, side) in [
                (
                    self.readiness.microphone,
                    PermissionKind::Microphone,
                    Side::YourMicrophone,
                ),
                (
                    self.readiness.system_audio,
                    PermissionKind::SystemAudio,
                    Side::TheCall,
                ),
            ] {
                if !allowed {
                    blocked.push(Problem::PermissionMissing { kind, side });
                }
            }
            if !self.readiness.speech_model {
                blocked.push(Problem::ModelMissing {
                    kind: ModelKind::Speech,
                });
            }
        }
        if !blocked.is_empty() {
            for problem in blocked {
                let id = self.problem(problem, since);
                effects.push(Effect::NotifyProblem { id });
            }
            return false;
        }
        if self.readiness.free_bytes < 5 * GB {
            self.problem(
                Problem::DiskLow {
                    free_bytes: self.readiness.free_bytes,
                },
                since,
            );
        }
        true
    }

    /// `can_start` for a detected call: nobody pressed anything, so only the top blocker is told.
    fn can_ask(&mut self, since: &str, effects: &mut Vec<Effect>) -> bool {
        let mut blocked = Vec::new();
        if self.can_start(false, since, &mut blocked) {
            return true;
        }
        let top = blocked.iter().find_map(|e| match e {
            Effect::NotifyProblem { id } => self.problems.iter().find(|p| p.id == *id),
            _ => None,
        });
        if let Some(top) = top {
            effects.push(Effect::CantRecord {
                problem: top.problem.clone(),
            });
        }
        false
    }

    fn ask(&mut self, call: Call, meeting_id: String, now: u64, effects: &mut Vec<Effect>) {
        effects.push(Effect::Emit(Event::CallDetected {
            call_id: call.call_id.clone(),
            app: call.app.clone(),
            app_id: call.app_id.clone(),
            mode: call.mode,
        }));
        self.app.state = SessionState::Asking {
            call_id: call.call_id.clone(),
            app: call.app.clone(),
        };
        self.app.companion.size = CompanionSize::Ask;
        self.ask = Some(PendingAsk {
            call,
            meeting_id,
            card_until_ms: Some(now.saturating_add(ASK_CARD_MS)),
        });
    }

    /// After Finishing returns to Idle: ask about a call that started meanwhile and is still on.
    fn ask_queued(&mut self, now: u64, since: &str, effects: &mut Vec<Effect>) {
        if let Some((call, meeting_id)) = self.queued.take()
            && !self.app.quit_pending
            && self.app.ask_on_call
            && call.mode == Mode::Ask
            && self.can_ask(since, effects)
        {
            self.ask(call, meeting_id, now, effects);
        }
    }

    fn start(&mut self, meeting_id: String, reason: StartReason, effects: &mut Vec<Effect>) {
        self.app.state = SessionState::Recording;
        self.app.meeting_id = Some(meeting_id.clone());
        self.app.elapsed_ms = 0;
        self.app.app = self.call.as_ref().map(|c| c.app.clone());
        self.app.started_by = Some(reason.clone());
        self.app.companion.size = CompanionSize::Pill;
        self.app.sharing = None;
        self.app.companion.peeking = false;
        self.peek_until = None;
        self.idle_since = None;
        self.keep = false;
        self.app.discarded = None;
        if self.audio() {
            effects.push(Effect::NotifyRecording { reason });
            effects.push(Effect::StartCapture {
                meeting_id,
                resume: false,
            });
        }
    }

    fn finish(&mut self, effects: &mut Vec<Effect>) {
        if let Some(meeting_id) = self.app.meeting_id.clone() {
            self.app.state = SessionState::Finishing {
                meeting_id: meeting_id.clone(),
            };
            effects.push(Effect::Finish {
                meeting_id,
                notes_only: !self.audio(),
            });
        }
    }

    fn stop(&mut self, now: u64, effects: &mut Vec<Effect>) {
        if self.app.state == SessionState::Paused {
            // Capture already stopped at Pause; Stop only starts the usual Resume window.
            self.pause_remind_at = None;
            self.app.state = SessionState::Stopped {
                resume_until_ms: now.saturating_add(GRACE_MS),
            };
            return;
        }
        if self.app.state != SessionState::Recording {
            return;
        }
        self.idle_since = None;
        if self.audio() {
            effects.push(Effect::StopCapture);
            self.app.state = SessionState::Stopped {
                resume_until_ms: now.saturating_add(GRACE_MS),
            };
        } else {
            self.finish(effects);
        }
    }

    fn resume(&mut self, since: &str, effects: &mut Vec<Effect>) {
        if self.can_start(false, since, effects) {
            self.app.state = SessionState::Recording;
            self.idle_since = None;
            self.pause_remind_at = None;
            if let (Some(meeting_id), Some(reason)) =
                (self.app.meeting_id.clone(), self.app.started_by.clone())
            {
                effects.push(Effect::NotifyRecording { reason });
                effects.push(Effect::StartCapture {
                    meeting_id,
                    resume: true,
                });
            }
        }
    }

    fn advance(&mut self, now: u64, effects: &mut Vec<Effect>) {
        if self.app.state == SessionState::Recording {
            self.app.elapsed_ms = self.app.elapsed_ms.saturating_add(now - self.last_ms);
            if self.audio()
                && self
                    .idle_since
                    .is_some_and(|t| now.saturating_sub(t) >= u64::from(self.idle_stop_s) * 1000)
            {
                self.stop(now, effects);
            }
        }
        self.last_ms = now;
        if self.app.state == SessionState::Paused
            && self.pause_remind_at.is_some_and(|at| now >= at)
        {
            self.pause_remind_at = None;
            effects.push(Effect::NotifyStillPaused);
        }
        if self.app.companion.size == CompanionSize::Ask
            && let Some(ask) = self.ask.as_mut()
            && ask.card_until_ms.is_some_and(|at| now >= at)
        {
            ask.card_until_ms = None;
            self.app.companion.size = CompanionSize::Pill;
        }
        if self
            .app
            .ask_off_pending
            .as_ref()
            .is_some_and(|p| now >= p.until_ms)
        {
            self.app.ask_off_pending = None;
        }
        if self
            .app
            .discarded
            .as_ref()
            .is_some_and(|d| now >= d.until_ms)
        {
            self.app.discarded = None;
        }
        if let SessionState::Stopped { resume_until_ms } = self.app.state
            && now >= resume_until_ms
        {
            if self.app.elapsed_ms < u64::from(self.discard_short_s) * 1000 {
                if let Some(meeting_id) = self.app.meeting_id.clone() {
                    effects.push(Effect::Discard { meeting_id });
                }
                effects.push(Effect::NotifyShort {
                    seconds: self.discard_short_s,
                });
                self.idle();
                if self.app.quit_pending {
                    effects.push(Effect::PauseDownload);
                    effects.push(Effect::Exit);
                }
            } else {
                self.finish(effects);
            }
        }
    }
}

/// Apply one input and return work for the adapter. Late Resume/Undo cannot beat expired timers.
pub fn next(mut session: Session, input: Input) -> (Session, Vec<Effect>) {
    let before = session.app.clone();
    let now = input.now_ms.max(session.last_ms);
    let mut effects = Vec::new();
    session.advance(now, &mut effects);
    let command = if effects.contains(&Effect::Exit) {
        Command::Tick
    } else {
        input.command
    };
    match command {
        Command::StartManual {
            meeting_id,
            notes_only,
        } if session.app.state == SessionState::Idle => {
            if session.can_start(notes_only, &input.since, &mut effects) {
                session.start(
                    meeting_id,
                    if notes_only {
                        StartReason::NotesOnly
                    } else {
                        StartReason::Manual
                    },
                    &mut effects,
                );
            }
        }
        Command::CallDetected { call, meeting_id } => {
            if matches!(
                session.app.state,
                SessionState::Recording | SessionState::Paused | SessionState::Stopped { .. }
            ) {
                // A second call app joins the running meeting; nothing is asked.
                if !session.apps.contains(&call.app_id) {
                    session.apps.push(call.app_id.clone());
                    if let Some(id) = session.app.meeting_id.clone() {
                        effects.push(Effect::AddApp {
                            meeting_id: id,
                            app_id: call.app_id,
                        });
                    }
                }
            } else if matches!(session.app.state, SessionState::Finishing { .. }) {
                // The watcher reports a call once; keep it until the notes are done.
                session.queued = Some((call, meeting_id));
            } else if session.app.ask_on_call
                && call.mode == Mode::Ask
                && (session.app.state == SessionState::Idle
                    || session
                        .ask
                        .as_ref()
                        .is_some_and(|a| a.call.call_id != call.call_id))
                && session.can_ask(&input.since, &mut effects)
            {
                // A newer call replaces an unanswered question; the card shows for 20 s again.
                session.ask(call, meeting_id, now, &mut effects);
            }
        }
        Command::AnswerAsk { call_id, record } => {
            if session
                .ask
                .as_ref()
                .is_some_and(|a| a.call.call_id == call_id)
            {
                if record && session.can_start(false, &input.since, &mut effects) {
                    if let Some(ask) = session.ask.take() {
                        session.apps.push(ask.call.app_id.clone());
                        session.call = Some(ask.call.clone());
                        session.start(
                            ask.meeting_id,
                            StartReason::App { app: ask.call.app },
                            &mut effects,
                        );
                    }
                } else if !record {
                    session.idle();
                }
            }
        }
        Command::CallEnded { call_id } => {
            if session
                .ask
                .as_ref()
                .is_some_and(|a| a.call.call_id == call_id)
            {
                session.idle();
            } else if session
                .queued
                .as_ref()
                .is_some_and(|(c, _)| c.call_id == call_id)
            {
                session.queued = None;
            } else if session.app.state == SessionState::Recording
                && session.audio()
                && session.call.as_ref().is_some_and(|c| c.call_id == call_id)
            {
                // Record may come after the mic went quiet, so no Mic signal starts the idle stop.
                if !session.keep {
                    session.idle_since.get_or_insert(now);
                }
            }
        }
        Command::HoldAsk(hold) if session.app.companion.size == CompanionSize::Ask => {
            if let Some(ask) = session.ask.as_mut() {
                ask.card_until_ms = (!hold).then(|| now.saturating_add(ASK_CARD_MS));
            }
        }
        Command::DontAskForApp { call_id }
            if session
                .ask
                .as_ref()
                .is_some_and(|a| a.call.call_id == call_id) =>
        {
            if let Some(ask) = session.ask.clone() {
                session.idle();
                session.app.ask_off_pending = Some(AskOffPending {
                    app_id: ask.call.app_id.clone(),
                    app: ask.call.app,
                    previous: ask.call.mode,
                    until_ms: now.saturating_add(GRACE_MS),
                });
                effects.push(Effect::SetAppMode {
                    app_id: ask.call.app_id,
                    mode: Mode::Off,
                });
            }
        }
        Command::UndoDontAsk { app_id } => {
            if session
                .app
                .ask_off_pending
                .as_ref()
                .is_some_and(|p| p.app_id == app_id)
                && let Some(pending) = session.app.ask_off_pending.take()
            {
                effects.push(Effect::SetAppMode {
                    app_id,
                    mode: pending.previous,
                });
            }
        }
        Command::SetAskOnCall(on) => {
            session.app.ask_on_call = on;
            effects.push(Effect::SetAskOnCall(on));
        }
        command @ (Command::StopRecording | Command::StopRecordingInMain) => {
            let was_recording = matches!(
                session.app.state,
                SessionState::Recording | SessionState::Paused
            );
            session.stop(now, &mut effects);
            if was_recording
                && matches!(command, Command::StopRecordingInMain)
                && matches!(session.app.state, SessionState::Stopped { .. })
            {
                session.app.companion.visible = false;
            }
        }
        Command::DiscardRecording if matches!(session.app.state, SessionState::Stopped { .. }) => {
            if let Some(meeting_id) = session.app.meeting_id.clone() {
                effects.push(Effect::Trash {
                    meeting_id: meeting_id.clone(),
                });
                session.idle();
                session.app.discarded = Some(Discarded {
                    meeting_id,
                    until_ms: now.saturating_add(DISCARD_UNDO_MS),
                });
                if session.app.quit_pending {
                    effects.push(Effect::PauseDownload);
                    effects.push(Effect::Exit);
                }
            }
        }
        Command::UndoDiscard => session.app.discarded = None,
        Command::KeepRecording if session.app.state == SessionState::Recording => {
            session.idle_since = None;
            session.keep = true;
        }
        Command::PauseRecording
            if session.app.state == SessionState::Recording && session.audio() =>
        {
            effects.push(Effect::StopCapture);
            session.app.state = SessionState::Paused;
            session.idle_since = None;
            session.pause_remind_at = Some(now.saturating_add(PAUSE_REMIND_MS));
        }
        // A new part on the new mic, like Resume without the pause; already announced.
        Command::SwitchMicrophone
            if session.app.state == SessionState::Recording && session.audio() =>
        {
            if let Some(meeting_id) = session.app.meeting_id.clone() {
                effects.push(Effect::StopCapture);
                effects.push(Effect::StartCapture {
                    meeting_id,
                    resume: true,
                });
            }
        }
        Command::MicActivity { app_id, active }
            if session.app.state == SessionState::Recording
                && session.audio()
                && session.call.as_ref().is_none_or(|c| c.app_id == app_id) =>
        {
            // A manual recording has no call of its own, so any app's mic signal counts.
            if active {
                session.idle_since = None;
                session.keep = false;
            } else if session.idle_since.is_none() && !session.keep {
                session.idle_since = Some(now);
            }
        }
        Command::AppQuit { app_id }
            if session.call.as_ref().is_some_and(|c| c.app_id == app_id) =>
        {
            session.stop(now, &mut effects)
        }
        Command::ResumeRecording
            if matches!(
                session.app.state,
                SessionState::Stopped { .. } | SessionState::Paused
            ) =>
        {
            session.resume(&input.since, &mut effects)
        }
        Command::SetCompanionSize(size)
            if matches!(
                session.app.state,
                SessionState::Recording | SessionState::Paused
            ) && size != CompanionSize::Ask =>
        {
            if session.app.sharing.is_some() {
                if size == CompanionSize::Sidecar {
                    session.app.companion.peeking = true;
                    session.peek_until = Some(now.saturating_add(15_000));
                    session.app.companion.size = size;
                } else {
                    session.app.companion.peeking = false;
                    session.peek_until = None;
                    session.app.companion.size = CompanionSize::Dot;
                }
            } else {
                session.app.companion.size = size;
            }
        }
        Command::SetCompanionPinned(pinned) => session.app.companion.pinned = pinned,
        Command::SetSharing(active)
            if matches!(
                session.app.state,
                SessionState::Recording | SessionState::Paused
            ) =>
        {
            if active && session.app.sharing.is_none() {
                session.before_share = session.app.companion.size;
                session.app.sharing = Some(Sharing {
                    hidden_from_capture: None,
                    shared_display: None,
                    other_displays: vec![],
                });
                session.app.companion.size = CompanionSize::Dot;
            } else if !active && session.app.sharing.is_some() {
                session.app.sharing = None;
                session.app.companion.size = session.before_share;
            }
            session.app.companion.peeking = false;
            session.peek_until = None;
        }
        Command::RequestQuit => match session.app.state {
            SessionState::Recording
            | SessionState::Paused
            | SessionState::Stopped { .. }
            | SessionState::Finishing { .. } => {
                session.app.quit_pending = true;
            }
            _ => {
                effects.push(Effect::PauseDownload);
                effects.push(Effect::Exit);
            }
        },
        Command::Quit { stop_and_save } if session.app.quit_pending => {
            if !stop_and_save {
                session.app.quit_pending = false;
            } else if matches!(
                session.app.state,
                SessionState::Recording | SessionState::Paused | SessionState::Stopped { .. }
            ) {
                if session.app.state == SessionState::Recording && session.audio() {
                    effects.push(Effect::StopCapture);
                }
                session.finish(&mut effects);
            }
        }
        Command::QuitNow
            if matches!(session.app.state, SessionState::Finishing { .. })
                && session.app.quit_pending =>
        {
            if let Some(meeting_id) = session.app.meeting_id.clone() {
                effects.push(Effect::MarkRecoverable { meeting_id });
            }
            effects.push(Effect::PauseDownload);
            effects.push(Effect::Exit);
        }
        Command::ReadinessChanged(readiness) => {
            session
                .problems
                .retain(|p| !resolved(&p.problem, &readiness));
            session.readiness = readiness;
            if session.app.state == SessionState::Recording
                && !session.can_start(!session.audio(), &input.since, &mut effects)
            {
                session.stop(now, &mut effects);
            }
        }
        Command::ProblemRaised(problem) => {
            match &problem {
                Problem::DiskFull => session.readiness.free_bytes = 0,
                Problem::DiskLow { free_bytes } => session.readiness.free_bytes = *free_bytes,
                Problem::ModelMissing {
                    kind: ModelKind::Speech,
                } => session.readiness.speech_model = false,
                // No readiness flag yet: it clears when notes are written again.
                Problem::ModelMissing {
                    kind: ModelKind::Notes,
                } => {}
                Problem::PermissionMissing { kind, .. } => match kind {
                    PermissionKind::Microphone => session.readiness.microphone = false,
                    PermissionKind::SystemAudio => session.readiness.system_audio = false,
                    PermissionKind::Notifications => session.readiness.notifications = false,
                },
                _ => {}
            }
            let fatal = matches!(
                problem,
                Problem::DiskFull
                    | Problem::PermissionMissing {
                        kind: PermissionKind::Microphone | PermissionKind::SystemAudio,
                        ..
                    }
                    | Problem::ToolFailed { .. }
            );
            session.problem(problem, &input.since);
            if fatal && session.app.state == SessionState::Recording && session.audio() {
                session.stop(now, &mut effects);
            }
        }
        Command::ProblemResolved { id } | Command::DismissProblem { id } => {
            session.problems.retain(|p| p.id != id)
        }
        Command::RetryProblem { id } => {
            if let Some(index) = session.problems.iter().position(|p| p.id == id) {
                effects.push(Effect::Retry {
                    problem: session.problems.remove(index),
                });
            }
        }
        Command::Finished {
            meeting_id,
            title,
            action_items,
        } if session.app.state
            == (SessionState::Finishing {
                meeting_id: meeting_id.clone(),
            }) =>
        {
            session.problems.retain(|p| !matches!(
                    p.problem,
                    Problem::Slow
                        | Problem::ModelMissing {
                            kind: ModelKind::Notes
                        }
                ) && !matches!(&p.problem, Problem::NotesFailed { meeting_id: id, .. } if id == &meeting_id));
            effects.push(Effect::Emit(Event::NotesReady {
                meeting_id,
                title,
                action_items,
            }));
            session.idle();
            if session.app.quit_pending {
                effects.push(Effect::PauseDownload);
                effects.push(Effect::Exit);
            }
            session.ask_queued(now, &input.since, &mut effects);
        }
        Command::FinishFailed { meeting_id, reason }
            if session.app.state
                == (SessionState::Finishing {
                    meeting_id: meeting_id.clone(),
                }) =>
        {
            // No notes model: the transcript is kept and the notes can be written later.
            let problem = if reason == crate::summarize::NOTES_NOT_READY {
                Problem::ModelMissing {
                    kind: ModelKind::Notes,
                }
            } else {
                Problem::NotesFailed { meeting_id, reason }
            };
            session.problem(problem, &input.since);
            session.idle();
            session
                .problems
                .retain(|p| !matches!(p.problem, Problem::Slow));
            // Keep the failure visible instead of silently exiting with unfinished notes.
            session.app.quit_pending = false;
            session.ask_queued(now, &input.since, &mut effects);
        }
        Command::FinishStopped { meeting_id }
            if session.app.state
                == (SessionState::Finishing {
                    meeting_id: meeting_id.clone(),
                }) =>
        {
            session
                .problems
                .retain(|p| !matches!(p.problem, Problem::Slow));
            session.idle();
            if session.app.quit_pending {
                effects.push(Effect::PauseDownload);
                effects.push(Effect::Exit);
            }
            session.ask_queued(now, &input.since, &mut effects);
        }
        Command::AudioInterrupted
            if session.app.state == SessionState::Recording && session.audio() =>
        {
            if let Some(meeting_id) = session.app.meeting_id.clone() {
                effects.push(Effect::StopCapture);
                effects.push(Effect::AudioPaused {
                    meeting_id: meeting_id.clone(),
                    at_ms: session.app.elapsed_ms,
                });
                effects.push(Effect::StartCapture {
                    meeting_id,
                    resume: true,
                });
            }
        }
        Command::Recoverable { meeting_ids } => effects.push(Effect::OfferRecovery { meeting_ids }),
        _ => {}
    }
    if session.peek_until.is_some_and(|until| now >= until) {
        session.peek_until = None;
        session.app.companion.peeking = false;
        session.app.companion.size = CompanionSize::Dot;
    }
    session.app.companion.visible = match session.app.state {
        SessionState::Idle => {
            session.app.ask_off_pending.is_some() || session.app.discarded.is_some()
        }
        SessionState::Asking { .. } => true,
        SessionState::Recording | SessionState::Paused => true,
        SessionState::Stopped { .. } => session.app.companion.visible || session.app.quit_pending,
        SessionState::Finishing { .. } => session.app.quit_pending,
    };
    if !matches!(
        session.app.state,
        SessionState::Recording | SessionState::Paused | SessionState::Asking { .. }
    ) || session.app.quit_pending
    {
        session.app.companion.size = CompanionSize::Pill;
    }
    // The idle stop as a point on the recording clock; elapsed and the deadline move together,
    // so this stays put between ticks and adds no extra state events.
    session.app.stops_at_ms = session
        .idle_since
        .filter(|_| session.app.state == SessionState::Recording && session.audio())
        .map(|since| {
            let deadline = since.saturating_add(u64::from(session.idle_stop_s) * 1000);
            session.app.elapsed_ms + deadline.saturating_sub(now)
        });
    session.problems.sort_by_key(|p| priority(&p.problem));
    session.app.problem = session.problems.first().cloned();
    session.app.more_problems = session.problems.len().saturating_sub(1) as u32;
    let mut without_elapsed = session.app.clone();
    without_elapsed.elapsed_ms = before.elapsed_ms;
    if session.app != before
        && (without_elapsed != before || now.saturating_sub(session.last_emit_ms) >= 1000)
    {
        // Publish the visible state before any effect can start audio capture.
        effects.insert(
            0,
            Effect::Emit(Event::StateChanged(Box::new(session.app.clone()))),
        );
        session.last_emit_ms = now;
    }
    (session, effects)
}

fn priority(problem: &Problem) -> u8 {
    match problem {
        Problem::DiskFull => 0,
        Problem::PermissionMissing { .. } => 1,
        Problem::ModelMissing { .. } => 2,
        Problem::ToolFailed { .. } => 3,
        Problem::NotesFailed { .. } => 4,
        Problem::DiskLow { .. } => 5,
        Problem::Slow => 6,
    }
}

fn same_cause(a: &Problem, b: &Problem) -> bool {
    match (a, b) {
        (
            Problem::PermissionMissing { kind: a, .. },
            Problem::PermissionMissing { kind: b, .. },
        ) => a == b,
        (Problem::ModelMissing { kind: a }, Problem::ModelMissing { kind: b }) => a == b,
        (Problem::ToolFailed { tool: a, .. }, Problem::ToolFailed { tool: b, .. }) => a == b,
        (
            Problem::NotesFailed { meeting_id: a, .. },
            Problem::NotesFailed { meeting_id: b, .. },
        ) => a == b,
        _ => std::mem::discriminant(a) == std::mem::discriminant(b),
    }
}

fn resolved(problem: &Problem, ready: &Readiness) -> bool {
    match problem {
        Problem::DiskFull => ready.free_bytes >= GB,
        Problem::DiskLow { .. } => ready.free_bytes >= 5 * GB,
        Problem::PermissionMissing { kind, .. } => match kind {
            PermissionKind::Microphone => ready.microphone,
            PermissionKind::SystemAudio => ready.system_audio,
            PermissionKind::Notifications => ready.notifications,
        },
        Problem::ModelMissing {
            kind: ModelKind::Speech,
        } => ready.speech_model,
        _ => false,
    }
}

#[cfg(test)]
mod tests;
