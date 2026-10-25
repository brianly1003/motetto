//! Desktop session contracts; effects are executed by the future desktop adapter.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use std::sync::{Arc, atomic::AtomicBool};

/// Shared cancellation flag for capture and live workers. Each recording gets a fresh flag.
pub type Stop = Arc<AtomicBool>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type")]
pub enum SessionState {
    Idle,
    Asking {
        call_id: String,
        app: String,
    },
    Recording,
    /// Capture stopped by the user without ending the meeting; Resume starts a new part.
    Paused,
    Stopped {
        #[ts(type = "number")]
        resume_until_ms: u64,
    },
    Finishing {
        meeting_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AppState {
    pub state: SessionState,
    pub meeting_id: Option<String>,
    pub app: Option<String>,
    #[ts(type = "number")]
    pub elapsed_ms: u64,
    pub started_by: Option<StartReason>,
    /// Master switch for the Ask card on detected calls (Settings > Calls); default on.
    pub ask_on_call: bool,
    pub problem: Option<ProblemInfo>,
    pub more_problems: u32,
    pub quit_pending: bool,
    pub dnd_active: bool,
    pub onboarding_done: bool,
    pub sharing: Option<Sharing>,
    pub companion: CompanionState,
    pub ask_off_pending: Option<AskOffPending>,
    /// While the call's app has let go of the mic: the recording time (`elapsed_ms`) at which it
    /// stops by itself, so the Pill can count down. None while someone is talking or after Keep
    /// recording (owner, 2026-10-20: "the call ended but Motetto still records").
    #[ts(type = "number | null")]
    pub stops_at_ms: Option<u64>,
    /// After Discard in the Stopped Pill: the meeting went to Recently deleted; the Pill offers
    /// Undo until `until_ms` (owner, 2026-10-20).
    pub discarded: Option<Discarded>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Discarded {
    pub meeting_id: String,
    #[ts(type = "number")]
    pub until_ms: u64,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            state: SessionState::Idle,
            meeting_id: None,
            app: None,
            elapsed_ms: 0,
            started_by: None,
            ask_on_call: true,
            problem: None,
            more_problems: 0,
            quit_pending: false,
            dnd_active: false,
            onboarding_done: false,
            sharing: None,
            companion: CompanionState {
                visible: false,
                size: CompanionSize::Pill,
                pinned: false,
                peeking: false,
                display_id: String::new(),
            },
            ask_off_pending: None,
            stops_at_ms: None,
            discarded: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CompanionState {
    pub visible: bool,
    pub size: CompanionSize,
    pub pinned: bool,
    pub peeking: bool,
    pub display_id: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub enum CompanionSize {
    /// The 460 px Ask card; fades to the Pill after 20 s.
    Ask,
    Dot,
    Pill,
    Sidecar,
    Tucked,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Ask,
    Off,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
/// "Don't ask for {App}" with its ten-second Undo.
pub struct AskOffPending {
    pub app_id: String,
    pub app: String,
    pub previous: Mode,
    #[ts(type = "number")]
    pub until_ms: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type")]
pub enum StartReason {
    App { app: String },
    Manual,
    NotesOnly,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProblemInfo {
    #[ts(type = "number")]
    pub id: u64,
    pub problem: Problem,
    pub since: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type")]
pub enum Problem {
    PermissionMissing {
        kind: PermissionKind,
        side: Side,
    },
    ModelMissing {
        kind: ModelKind,
    },
    DiskLow {
        #[ts(type = "number")]
        free_bytes: u64,
    },
    DiskFull,
    Slow,
    ToolFailed {
        tool: String,
        stderr: String,
    },
    NotesFailed {
        meeting_id: String,
        reason: String,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub enum PermissionKind {
    Microphone,
    SystemAudio,
    Notifications,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub enum Side {
    TheCall,
    YourMicrophone,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub enum ModelKind {
    Speech,
    Notes,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Sharing {
    pub hidden_from_capture: Option<bool>,
    pub shared_display: Option<String>,
    pub other_displays: Vec<Display>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Display {
    pub id: String,
    pub name: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type")]
pub enum Stage {
    Recording,
    Transcribing,
    Summarizing { part: u32, of: u32 },
    Merging,
    Saving,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ActionItem {
    pub index: u32,
    pub owner: Option<String>,
    pub task: String,
    pub due: Option<String>,
    pub done: bool,
    #[ts(type = "number | null")]
    pub source_ms: Option<u64>,
}

/// The desktop's model tiers (SPEC 5.1); the files behind each live in the app's catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub enum TierId {
    Best,
    Balanced,
    Light,
}
impl TierId {
    /// The tier a too-slow speed test offers next; Light is the floor.
    pub fn down(self) -> Self {
        match self {
            Self::Best => Self::Balanced,
            Self::Balanced | Self::Light => Self::Light,
        }
    }
}

/// One speed test: `rtf` is processing time over audio time, so below 1.0 keeps up with a call.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct SpeedResult {
    pub tier: TierId,
    pub rtf: f32,
    pub keeps_up: bool,
    pub measured_at: String,
}

/// Where first-run setup is (SPEC 5.2): speech, voice detection, speed test, then notes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type")]
pub enum Phase {
    Idle,
    Downloading { id: String },
    SpeedTest,
    AwaitingAnswer { offer: TierId },
    Paused,
    Failed { id: String, reason: String },
    Done,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct SetupState {
    pub tier: TierId,
    pub phase: Phase,
    /// (catalog id, bytes on disk, total) for the tier's files, read from disk.
    #[ts(type = "[string, number, number][]")]
    pub files: Vec<(String, u64, u64)>,
    pub speed: Option<SpeedResult>,
    /// The user answered "Keep {tier}" to a slow result, so it isn't asked again.
    pub keep_slow: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type")]
pub enum Event {
    StateChanged(Box<AppState>),
    CallDetected {
        call_id: String,
        app: String,
        app_id: String,
        mode: Mode,
    },
    Stage {
        meeting_id: String,
        stage: Stage,
        percent: u8,
        eta_s: Option<u32>,
    },
    NotesReady {
        meeting_id: String,
        title: String,
        action_items: Vec<ActionItem>,
    },
    DownloadProgress {
        id: String,
        #[ts(type = "number")]
        bytes: u64,
        #[ts(type = "number")]
        total: u64,
    },
    DownloadDone {
        id: String,
    },
    DownloadFailed {
        id: String,
        reason: String,
        checksum: bool,
    },
    SpeedTest(SpeedResult),
    SetupChanged(SetupState),
}

/// A detector-confirmed call; unknown apps are filtered by the detector before this boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub call_id: String,
    pub app_id: String,
    pub app: String,
    pub mode: Mode,
}

/// Fresh platform/model checks, supplied by the adapter; M1 performs no platform queries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Readiness {
    pub microphone: bool,
    pub system_audio: bool,
    pub notifications: bool,
    pub speech_model: bool,
    pub free_bytes: u64,
}

/// Inputs handled by the M1 reducer. Meeting ids are allocated by its caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    StartManual {
        meeting_id: String,
        notes_only: bool,
    },
    CallDetected {
        call: Call,
        meeting_id: String,
    },
    AnswerAsk {
        call_id: String,
        record: bool,
    },
    StopRecording,
    /// Explicit Stop restores the library, which owns Resume/Discard during the grace period.
    StopRecordingInMain,
    /// Turns off the idle stop until the call's app uses the mic again.
    KeepRecording,
    /// The Stopped Pill's Discard: no notes; the meeting goes to Recently deleted.
    DiscardRecording,
    /// The Pill's Undo after Discard ends the offer; the app restores and writes the notes.
    UndoDiscard,
    PauseRecording,
    ResumeRecording,
    /// The user picked another microphone: the current part ends and the next one records from it.
    SwitchMicrophone,
    CallEnded {
        call_id: String,
    },
    MicActivity {
        app_id: String,
        active: bool,
    },
    AppQuit {
        app_id: String,
    },
    DontAskForApp {
        call_id: String,
    },
    UndoDontAsk {
        app_id: String,
    },
    /// Pointer over the Ask card holds it; leaving restarts the 20 s.
    HoldAsk(bool),
    SetAskOnCall(bool),
    SetCompanionSize(CompanionSize),
    SetCompanionPinned(bool),
    SetSharing(bool),
    RequestQuit,
    Quit {
        stop_and_save: bool,
    },
    QuitNow,
    Tick,
    ReadinessChanged(Readiness),
    ProblemRaised(Problem),
    ProblemResolved {
        id: u64,
    },
    RetryProblem {
        id: u64,
    },
    DismissProblem {
        id: u64,
    },
    Finished {
        meeting_id: String,
        title: String,
        action_items: Vec<ActionItem>,
    },
    FinishFailed {
        meeting_id: String,
        reason: String,
    },
    /// The user stopped the notes being written (Stop on the steps page): nothing failed, so no
    /// problem is raised; the meeting waits with its recording and transcript.
    FinishStopped {
        meeting_id: String,
    },
    AudioInterrupted,
    Recoverable {
        meeting_ids: Vec<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_event_round_trips_with_tagged_state_and_numeric_milliseconds() {
        let state = AppState {
            state: SessionState::Stopped {
                resume_until_ms: 10_000,
            },
            meeting_id: Some("m1".into()),
            ..AppState::default()
        };
        let event = Event::StateChanged(Box::new(state));
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "StateChanged");
        assert_eq!(json["state"]["type"], "Stopped");
        assert_eq!(json["state"]["resume_until_ms"], 10_000);
        assert_eq!(serde_json::from_value::<Event>(json).unwrap(), event);
        assert!(AppState::decl(&ts_rs::Config::default()).contains("elapsed_ms: number"));
        assert!(SessionState::decl(&ts_rs::Config::default()).contains("resume_until_ms: number"));
        assert!(Problem::decl(&ts_rs::Config::default()).contains("free_bytes: number"));
        assert_eq!(serde_json::to_string(&Mode::Ask).unwrap(), "\"ask\"");
    }

    #[test]
    fn stage_and_problem_payloads_keep_their_fields() {
        let event = Event::Stage {
            meeting_id: "m1".into(),
            stage: Stage::Summarizing { part: 2, of: 3 },
            percent: 40,
            eta_s: Some(10),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["stage"]["part"], 2);
        assert_eq!(serde_json::from_value::<Event>(json).unwrap(), event);
        let problem = Problem::NotesFailed {
            meeting_id: "m1".into(),
            reason: "disk full".into(),
        };
        let json = serde_json::to_string(&problem).unwrap();
        assert_eq!(serde_json::from_str::<Problem>(&json).unwrap(), problem);
    }
}
