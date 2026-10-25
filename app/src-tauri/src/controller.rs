//! Desktop session actor: one ordered command stream, independent of capture and model work.
use motetto_core::{
    events::*,
    session::{self, Effect, Input, Session},
    watch::WatchEvent,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub enum Action {
    Start,
    Stop,
    StopMeeting(String),
    Pause,
    /// Another mic was chosen: a new part when recording, nothing otherwise.
    SwitchMic,
    Keep,
    Discard,
    UndoDiscard,
    Resume,
    Size(CompanionSize),
    Pin(bool),
    Sharing(bool),
    RequestQuit,
    Quit(bool),
    Dismiss(u64),
    /// Setup verified the notes model: a pending "notes model missing" problem is resolved.
    NotesModelReady,
    AnswerAsk {
        call_id: String,
        record: bool,
    },
    DontAsk {
        call_id: String,
    },
    UndoDontAsk {
        app_id: String,
    },
    HoldAsk(bool),
    AskOnCall(bool),
    /// Sets an app's mode at any time (the tray's "Ask again about {App}"); no session command.
    AppMode {
        app_id: String,
        mode: Mode,
    },
}
pub enum Message {
    Mark(mpsc::Sender<Result<u64, String>>),
    Action(Action, mpsc::Sender<Result<(), String>>),
    Watch(WatchEvent),
    CaptureFailed {
        id: String,
        reason: String,
    },
    Finished {
        id: String,
        result: Result<String, String>,
    },
}
pub enum Job {
    Exit,
    Capture {
        id: String,
        offset_ms: u64,
        stop: Stop,
    },
    Finish {
        id: String,
    },
    Discard {
        id: String,
    },
    /// Discard from the Stopped Pill: into Recently deleted, no notes.
    Trash {
        id: String,
    },
}

/// Native effects are injected so controller ordering can be tested without opening a microphone.
pub trait Host: Send + 'static {
    fn preferences(&self) -> (u32, u32, bool) {
        (30, 20, false)
    }
    fn mark(&self, id: &str, at_ms: u64) -> Result<(), String>;
    /// `request` may show the OS permission prompts; `false` only queries (a detected call).
    fn readiness(&self, request: bool) -> Result<Readiness, String>;
    fn event(&self, event: Event) -> Result<(), String>;
    fn announce(&self) -> Result<(), String>;
    fn show_main(&self);
    fn job(&self, job: Job) -> Result<(), String>;
    fn notice(&self, message: &str);
    /// SPEC 7.5 `cant_record` for a detected call: an OS notification only, no window comes forward.
    fn cant_record(&self, body: &str);
    fn exit(&self);
    /// The saved Ask/Off choice for a call app, or its bundled default.
    fn app_mode(&self, app_id: &str) -> Mode;
    fn set_app_mode(&self, app_id: &str, mode: Mode) -> Result<(), String>;
    fn ask_on_call(&self) -> bool;
    fn set_ask_on_call(&self, on: bool) -> Result<(), String>;
}

pub struct Controller {
    origin: Instant,
    sender: mpsc::Sender<Message>,
    state: Arc<Mutex<AppState>>,
}
impl Controller {
    pub fn spawn(
        host: impl Host,
        sender: mpsc::Sender<Message>,
        receiver: mpsc::Receiver<Message>,
        busy: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        let origin = Instant::now();
        let state = Arc::new(Mutex::new(AppState::default()));
        let shared = state.clone();
        std::thread::Builder::new()
            .name("motetto-session".into())
            .spawn(move || {
                let mut engine = Engine::new(host, busy, shared);
                engine.clock = Some(origin);
                loop {
                    let message = receiver.recv_timeout(Duration::from_millis(100));
                    let now = origin.elapsed().as_millis() as u64;
                    // Advance deadlines even under a continuous stream of UI commands.
                    engine.apply(Command::Tick, now);
                    match message {
                        Ok(Message::Mark(reply)) => {
                            let _ = reply.send(engine.mark());
                        }
                        Ok(Message::Action(action, reply)) => {
                            let _ = reply.send(engine.action(action, now));
                        }
                        Ok(Message::Watch(event)) => engine.watch(event, now),
                        Ok(Message::CaptureFailed { id, reason })
                            if engine.session.app.meeting_id.as_ref() == Some(&id) =>
                        {
                            engine.capture_failed = true;
                            engine.apply(
                                Command::ProblemRaised(Problem::ToolFailed {
                                    tool: "Audio capture".into(),
                                    stderr: reason,
                                }),
                                now,
                            );
                        }
                        Ok(Message::Finished { id, result }) => engine.apply(
                            match result {
                                Ok(title) => Command::Finished {
                                    meeting_id: id,
                                    title,
                                    action_items: vec![],
                                },
                                // Stop on the steps page: the worker failed because it was told to.
                                Err(_) if motetto_core::tools::cancelled() => {
                                    Command::FinishStopped { meeting_id: id }
                                }
                                Err(reason) => Command::FinishFailed {
                                    meeting_id: id,
                                    reason,
                                },
                            },
                            now,
                        ),
                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                            engine.stop();
                            break;
                        }
                        _ => {}
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            sender,
            state,
            origin,
        })
    }
    pub fn mark(&self) -> Result<u64, String> {
        let (tx, rx) = mpsc::channel();
        self.sender
            .send(Message::Mark(tx))
            .map_err(|_| "Session controller stopped")?;
        rx.recv()
            .map_err(|_| "Session controller stopped".to_string())?
    }
    pub fn resume_remaining_ms(&self) -> Result<u64, String> {
        Ok(match self.state()?.state {
            SessionState::Stopped { resume_until_ms } => {
                resume_until_ms.saturating_sub(self.origin.elapsed().as_millis() as u64)
            }
            _ => 0,
        })
    }
    pub fn state(&self) -> Result<AppState, String> {
        self.state
            .lock()
            .map(|s| s.clone())
            .map_err(|_| "Session state is unavailable".into())
    }
    pub fn action(&self, action: Action) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.sender
            .send(Message::Action(action, tx))
            .map_err(|_| "Session controller stopped")?;
        rx.recv()
            .map_err(|_| "Session controller stopped".to_string())?
    }
}

struct Engine<H> {
    host: H,
    session: Session,
    busy: Arc<AtomicBool>,
    shared: Arc<Mutex<AppState>>,
    stop: Option<Stop>,
    owns_busy: bool,
    capture_failed: bool,
    exiting: bool,
    clock: Option<Instant>,
}
impl<H: Host> Engine<H> {
    fn new(host: H, busy: Arc<AtomicBool>, shared: Arc<Mutex<AppState>>) -> Self {
        let mut session = Session::new(Readiness::default());
        session.app.dnd_active = true; // Focus fallback: the companion always announces recording.
        // The saved switch is already on disk and no window exists yet: keep the state only.
        let (session, _) = session::next(
            session,
            Input {
                now_ms: 0,
                since: timestamp(),
                command: Command::SetAskOnCall(host.ask_on_call()),
            },
        );
        if let Ok(mut state) = shared.lock() {
            *state = session.app.clone();
        }
        Self {
            host,
            session,
            busy,
            shared,
            stop: None,
            owns_busy: false,
            capture_failed: false,
            exiting: false,
            clock: None,
        }
    }
    fn mark(&self) -> Result<u64, String> {
        if self.session.app.state != SessionState::Recording || self.session.app.quit_pending {
            return Err("Mark a moment while recording".into());
        }
        let id = self
            .session
            .app
            .meeting_id
            .as_deref()
            .ok_or("No active meeting")?;
        let at_ms = self.session.app.elapsed_ms;
        self.host.mark(id, at_ms)?;
        Ok(at_ms)
    }
    fn action(&mut self, action: Action, now: u64) -> Result<(), String> {
        if self.exiting {
            return Err("Motetto is shutting down".into());
        }
        // Record now while asking is the answer to the question (guardrail 2): record that call.
        let action = match (action, &self.session.app.state) {
            (Action::Start, SessionState::Asking { call_id, .. }) => Action::AnswerAsk {
                call_id: call_id.clone(),
                record: true,
            },
            (action, _) => action,
        };
        let command = match action {
            Action::Start => {
                self.reserve(now)?;
                self.apply(
                    Command::StartManual {
                        meeting_id: timestamp(),
                        notes_only: false,
                    },
                    now,
                );
                // A blocked or ignored start leaves no recording to hold the gate for.
                if self.session.app.state != SessionState::Recording {
                    self.release();
                }
                return Ok(());
            }
            Action::AnswerAsk {
                call_id,
                record: true,
            } => {
                self.reserve(now)?;
                self.apply(
                    Command::AnswerAsk {
                        call_id,
                        record: true,
                    },
                    now,
                );
                // A stale call or a blocked start leaves no recording to hold the gate for.
                if self.session.app.state != SessionState::Recording {
                    self.release();
                }
                return Ok(());
            }
            Action::AnswerAsk { call_id, record } => Command::AnswerAsk { call_id, record },
            Action::DontAsk { call_id } => Command::DontAskForApp { call_id },
            Action::UndoDontAsk { app_id } => Command::UndoDontAsk { app_id },
            Action::AppMode { app_id, mode } => return self.host.set_app_mode(&app_id, mode),
            Action::HoldAsk(hold) => Command::HoldAsk(hold),
            Action::AskOnCall(on) => {
                self.host.set_ask_on_call(on)?;
                Command::SetAskOnCall(on)
            }
            Action::Resume => {
                if self.capture_failed {
                    return Err(
                        "Capture failed. Finish this meeting before starting another recording."
                            .into(),
                    );
                }
                if !matches!(
                    self.session.app.state,
                    SessionState::Stopped { .. } | SessionState::Paused
                ) {
                    return Err("There is no paused or stopped recording to resume".into());
                }
                let readiness = self.host.readiness(true)?;
                self.apply(Command::ReadinessChanged(readiness), now);
                Command::ResumeRecording
            }
            Action::Size(size) => Command::SetCompanionSize(size),
            Action::Pin(pinned) => Command::SetCompanionPinned(pinned),
            Action::Sharing(active) => Command::SetSharing(active),
            Action::StopMeeting(id) => {
                if self.session.app.meeting_id.as_ref() != Some(&id) {
                    return Ok(());
                }
                Command::StopRecordingInMain
            }
            Action::Stop => Command::StopRecordingInMain,
            Action::Keep => Command::KeepRecording,
            Action::SwitchMic => {
                if self.session.app.state != SessionState::Recording {
                    return Ok(());
                }
                Command::SwitchMicrophone
            }
            Action::Discard => Command::DiscardRecording,
            Action::UndoDiscard => Command::UndoDiscard,
            Action::Pause => {
                if self.session.app.state != SessionState::Recording {
                    return Err("There is no recording to pause".into());
                }
                Command::PauseRecording
            }
            Action::RequestQuit => {
                if self.busy.load(Ordering::SeqCst) && !self.owns_busy {
                    return Err(
                        "An import is running. Motetto must stay open until it finishes.".into(),
                    );
                }
                Command::RequestQuit
            }
            Action::Quit(stop_and_save) => Command::Quit { stop_and_save },
            Action::Dismiss(id) => Command::DismissProblem { id },
            Action::NotesModelReady => {
                let missing: Vec<u64> = self
                    .session
                    .problems()
                    .iter()
                    .filter(|p| {
                        p.problem
                            == Problem::ModelMissing {
                                kind: ModelKind::Notes,
                            }
                    })
                    .map(|p| p.id)
                    .collect();
                for id in missing {
                    self.apply(Command::ProblemResolved { id }, now);
                }
                return Ok(());
            }
        };
        let manual_stop = matches!(command, Command::StopRecordingInMain)
            && matches!(
                self.session.app.state,
                SessionState::Recording | SessionState::Paused
            );
        self.apply(command, now);
        if manual_stop && matches!(self.session.app.state, SessionState::Stopped { .. }) {
            self.host.show_main();
        }
        Ok(())
    }
    /// Take the one-meeting reservation and fresh readiness before a recording can start.
    fn reserve(&mut self, now: u64) -> Result<(), String> {
        if self
            .busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err("A meeting is already active or being imported".into());
        }
        self.owns_busy = true;
        self.capture_failed = false;
        let readiness = match self.host.readiness(true) {
            Ok(r) => r,
            Err(e) => {
                self.release();
                return Err(e);
            }
        };
        // Apply readiness without releasing the reservation before the start command.
        let (session, effects) = session::next(
            self.session.clone(),
            Input {
                now_ms: now,
                since: timestamp(),
                command: Command::ReadinessChanged(readiness),
            },
        );
        self.session = session;
        self.effects(effects, now);
        Ok(())
    }
    /// Detector events. A detected call only ever asks; nothing records until AnswerAsk.
    pub(crate) fn watch(&mut self, event: WatchEvent, now: u64) {
        if self.exiting {
            return;
        }
        let command = match event {
            WatchEvent::Started {
                call_id,
                app_id,
                app,
            } => {
                let mode = self.host.app_mode(&app_id);
                if matches!(
                    self.session.app.state,
                    SessionState::Idle | SessionState::Asking { .. }
                ) && self.session.app.ask_on_call
                    && mode == Mode::Ask
                {
                    // The question is only asked if a recording could start; check that freshly,
                    // without permission prompts in the middle of someone's call (SPEC 6.1).
                    match self.host.readiness(false) {
                        Ok(readiness) => self.apply(Command::ReadinessChanged(readiness), now),
                        Err(e) => {
                            eprintln!("Motetto: not asking about the {app} call: {e}");
                            return;
                        }
                    }
                }
                Command::CallDetected {
                    call: Call {
                        call_id,
                        app_id,
                        app,
                        mode,
                    },
                    meeting_id: timestamp(),
                }
            }
            WatchEvent::Ended { call_id } => Command::CallEnded { call_id },
            WatchEvent::Mic { app_id, active } => Command::MicActivity { app_id, active },
        };
        self.apply(command, now);
    }
    fn stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop.store(true, Ordering::SeqCst);
        }
    }
    fn release(&mut self) {
        if self.owns_busy && !self.exiting {
            self.busy.store(false, Ordering::SeqCst);
            self.owns_busy = false;
        }
    }
    fn apply(&mut self, command: Command, now: u64) {
        // A native permission check may have waited for a dialog; it cannot extend Resume.
        let now = self
            .clock
            .map_or(now, |origin| (origin.elapsed().as_millis() as u64).max(now));
        let (idle, discard, _) = self.host.preferences();
        self.session.idle_stop_s = idle;
        self.session.discard_short_s = discard;
        let starting = matches!(
            self.session.app.state,
            SessionState::Idle | SessionState::Asking { .. }
        );
        let (session, effects) = session::next(
            self.session.clone(),
            Input {
                now_ms: now,
                since: timestamp(),
                command,
            },
        );
        self.session = session;
        if let Ok(mut state) = self.shared.lock() {
            *state = self.session.app.clone();
        }
        self.effects(effects, now);
        if starting
            && self.session.app.state == SessionState::Recording
            && self.host.preferences().2
        {
            self.apply(Command::SetCompanionSize(CompanionSize::Dot), now);
        }
        if let Ok(mut state) = self.shared.lock() {
            *state = self.session.app.clone();
        }
        if self.session.app.state == SessionState::Idle {
            self.release();
        }
    }
    fn effects(&mut self, effects: Vec<Effect>, now: u64) {
        for effect in effects {
            let result = match effect {
                Effect::Emit(event) => self.host.event(event),
                Effect::NotifyRecording { .. } => self.host.announce(),
                Effect::StartCapture { meeting_id, .. } => {
                    let stop = Stop::default();
                    self.stop = Some(stop.clone());
                    self.host.job(Job::Capture {
                        id: meeting_id,
                        offset_ms: self.session.app.elapsed_ms,
                        stop,
                    })
                }
                Effect::StopCapture => {
                    self.stop();
                    Ok(())
                }
                Effect::Finish { meeting_id, .. } => self.host.job(Job::Finish { id: meeting_id }),
                Effect::Discard { meeting_id } => self.host.job(Job::Discard { id: meeting_id }),
                Effect::Trash { meeting_id } => self.host.job(Job::Trash { id: meeting_id }),
                Effect::NotifyShort { seconds } => {
                    self.host.notice(&format!(
                        "Recording under {seconds} seconds discarded unless capture failed."
                    ));
                    Ok(())
                }
                Effect::NotifyStillPaused => {
                    self.host.notice(STILL_PAUSED);
                    Ok(())
                }
                Effect::NotifyProblem { .. } => {
                    self.host
                        .notice("Recording could not start. Check the problem shown in Motetto.");
                    Ok(())
                }
                Effect::CantRecord { problem } => {
                    self.host.cant_record(&cant_record(&problem));
                    Ok(())
                }
                Effect::Exit => {
                    self.exiting = true;
                    self.busy.store(true, Ordering::SeqCst);
                    self.host.exit();
                    Ok(())
                }
                // Nothing to pause here: setup's download keeps its `.part` on quit and the next
                // launch resumes it (setup::resume).
                Effect::PauseDownload => Ok(()),
                // A settings write that fails must not stop a recording; say so and go on.
                Effect::SetAppMode { app_id, mode } => {
                    if let Err(e) = self.host.set_app_mode(&app_id, mode) {
                        self.host
                            .notice(&format!("Couldn't save the Calls setting: {e}"));
                    }
                    Ok(())
                }
                Effect::SetAskOnCall(on) => {
                    if let Err(e) = self.host.set_ask_on_call(on) {
                        self.host
                            .notice(&format!("Couldn't save the Calls setting: {e}"));
                    }
                    Ok(())
                }
                // meeting.json `apps` is written when the full metadata lands.
                Effect::AddApp { .. } => Ok(()),
                _ => Err(
                    "This session effect is not available in the manual recording controller"
                        .into(),
                ),
            };
            if let Err(reason) = result {
                self.stop();
                self.capture_failed = true;
                let command =
                    if let SessionState::Finishing { meeting_id } = &self.session.app.state {
                        Command::FinishFailed {
                            meeting_id: meeting_id.clone(),
                            reason,
                        }
                    } else {
                        Command::ProblemRaised(Problem::ToolFailed {
                            tool: "Desktop controller".into(),
                            stderr: reason,
                        })
                    };
                // Do not execute the rest of a failed start sequence (especially capture).
                let (session, effects) = session::next(
                    self.session.clone(),
                    Input {
                        now_ms: now,
                        since: timestamp(),
                        command,
                    },
                );
                self.session = session;
                for effect in effects {
                    if let Effect::Emit(event) = effect {
                        let _ = self.host.event(event);
                    }
                }
                break;
            }
        }
    }
}
pub(crate) const STILL_PAUSED: &str =
    "Still paused. Nothing has been recorded for 15 minutes. Resume or stop the recording.";
/// SPEC 7.5 `cant_record` body: "{Permission} is off. Turn it on in {Settings name}.", or a
/// plain one-line reason for a blocker that is not a permission.
fn cant_record(problem: &Problem) -> String {
    let settings = if cfg!(windows) {
        "Windows Settings"
    } else {
        "System Settings"
    };
    match problem {
        Problem::PermissionMissing { kind, .. } => {
            let permission = match kind {
                PermissionKind::Microphone => "Microphone",
                PermissionKind::SystemAudio => "Screen & System Audio Recording",
                PermissionKind::Notifications => "Notifications",
            };
            format!("{permission} is off. Turn it on in {settings}.")
        }
        Problem::ModelMissing {
            kind: ModelKind::Notes,
        } => "The notes model is missing.".into(),
        Problem::ModelMissing { .. } => "The speech model is missing.".into(),
        Problem::DiskFull | Problem::DiskLow { .. } => "The disk is full.".into(),
        _ => "Check the problem shown in Motetto.".into(),
    }
}
fn timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .to_string()
}

#[cfg(test)]
mod tests;
