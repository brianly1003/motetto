import type { Meeting } from "./bindings/Meeting";
import type { MeetingSummary } from "./bindings/MeetingSummary";
import type { SearchHit } from "./bindings/SearchHit";

export function desktopAvailable() { return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window; }
async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(command, args);
}
export const listMeetings = () => invoke<MeetingSummary[]>("list_meetings");
export const libraryBytes = () => invoke<number>("library_bytes");
export const getMeeting = (id: string) => invoke<Meeting>("get_meeting", { id });
export const importAudio = () => invoke<MeetingSummary | null>("import_audio");
// The meeting an import or Write the notes again is writing (id null while the file picker is open).
export type Job = { id: string | null; import: boolean; transcribe: boolean };
export const importing = () => invoke<Job | null>("importing");
export const cancelImport = () => invoke<void>("cancel_import");
/** Stop on a meeting's steps page; Write the notes again and Transcribe again then fail with STOPPED. */
export const stopProcessing = (id: string) => invoke<void>("stop_processing", { id });
export const STOPPED = "stopped";
export async function subscribe(name: string, callback: () => void) {
  const { listen } = await import("@tauri-apps/api/event");
  return listen(name, callback);
}

import type { AppState } from "./bindings/AppState";
import type { Event as SessionEvent } from "./bindings/Event";
export const getState = () => invoke<AppState>("get_state");
export const startRecording = () => invoke<void>("start_recording");
export const stopRecording = () => invoke<void>("stop_recording");
export const pauseRecording = () => invoke<void>("pause_recording");
export const keepRecording = () => invoke<void>("keep_recording");
export const discardRecording = () => invoke<void>("discard_recording");
export const undoDiscard = (id: string) => invoke<void>("undo_discard", {id});
export const resumeRecording = () => invoke<void>("resume_recording");
export const quitRecording = (stopAndSave: boolean) => invoke<void>("quit", { stopAndSave });
export const dismissProblem = (id: number) => invoke<void>("dismiss_problem", { id });
export async function onSessionEvent(callback: (event: SessionEvent) => void) {
  const { listen } = await import("@tauri-apps/api/event");
  return listen<SessionEvent>("motetto://event", event => callback(event.payload));
}
export async function onNotice(callback: (notice: string) => void) {
  const { listen } = await import("@tauri-apps/api/event");
  return listen<string>("motetto://notice", event => callback(event.payload));
}

export const setCompanionSize = (size: "Pill" | "Dot" | "Sidecar" | "Tucked") => invoke<void>("set_companion_size", { size });
export const companionReady = () => invoke<void>("companion_ready");
export const takeNote = () => invoke<void>("take_note");
export const resumeRemainingMs = () => invoke<number>("resume_remaining_ms");
export const notificationPermission = (request = false) => invoke<boolean>("notification_permission", { request });

/** Files dragged over this window: true while over it, false on leave or drop (Rust imports a drop). */
export async function onFileDrag(callback: (over: boolean) => void) {
  const { getCurrentWebview } = await import("@tauri-apps/api/webview");
  return getCurrentWebview().onDragDropEvent(event => callback(event.payload.type === "enter" || event.payload.type === "over"));
}

export async function onOpenMeeting(callback: (id: string) => void) {
  const { listen } = await import("@tauri-apps/api/event");
  return listen<string>("motetto://open-meeting", event => callback(event.payload));
}

export const answerAsk = (callId: string, record: boolean) => invoke<void>("answer_ask", { callId, record });
export const dontAskForApp = (callId: string) => invoke<void>("dont_ask_for_app", { callId });
export const undoDontAsk = (appId: string) => invoke<void>("undo_dont_ask", { appId });
export const holdAsk = (hold: boolean) => invoke<void>("hold_ask", { hold });

export const markMoment = () => invoke<number>("mark_moment");

import type { MyNotes } from "./bindings/MyNotes";
export const getMyNotes = (meetingId: string) => invoke<MyNotes>("get_my_notes", { meetingId });
export const saveMyNotes = (meetingId: string, text: string, baseVersion: number) => invoke<MyNotes>("save_my_notes", { meetingId, text, baseVersion });
export const addJot = (text: string) => invoke<MyNotes>("add_jot", { text });
// `extra`: px the page needs under its controls (a menu, the Ask card's mic line); Rust sizes the window.
export const companionMenu = (open: boolean, extra?: number) => invoke<void>("companion_menu", { open, extra: extra ?? null });

// The microphone picker (DESIGN 4.15): what Motetto records from, and its one problem.
import type { MicReport } from "./bindings/MicReport";
import type { PermissionKind } from "./bindings/PermissionKind";
export const microphones = () => invoke<MicReport>("microphones");
export const setMicrophone = (id: string | null) => invoke<MicReport>("set_microphone", { id });
export const setMicVolume = (id: string, volume: number) => invoke<MicReport>("set_mic_volume", { id, volume });
export const openSoundSettings = () => invoke<void>("open_sound_settings");
export const openPermissionSettings = (kind: PermissionKind) => invoke<void>("open_permission_settings", { kind });
export const getLevels = () => invoke<[number, number]>("get_levels");

export const liveTranscript = () => invoke<[string, string, string]>("live_transcript");

// Native movement events keep Tucked on-screen; this final snap chooses its nearer edge.
export async function dragCompanion() {
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  await getCurrentWindow().startDragging();
  await invoke<void>("snap_companion");
}
export async function onCompanionFocus(callback: (focused: boolean) => void) {
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  return getCurrentWindow().onFocusChanged(event => callback(event.payload));
}
export async function onOpenNotes(callback: (mode: string) => void) {
  const { listen } = await import("@tauri-apps/api/event");
  return listen<string>("motetto://open-notes", event => callback(event.payload));
}

export const pinCompanion = (pinned: boolean) => invoke<void>("pin_companion", { pinned });
export const sharingMode = (active: boolean) => invoke<void>("sharing_mode", { active });

export const openActionItems = () => invoke<[MeetingSummary, string][]>("open_action_items");
export const searchMeetings = (query: string) => invoke<SearchHit[]>("search_meetings", { query });

/** `language`: Write the notes again in this code (or "same_as_call") instead of the Settings one. */
export const meetingAction = (id: string, action: "reveal" | "regenerate" | "retranscribe" | "delete" | "restore" | "purge", language?: string) => invoke<void>("meeting_action", { id, action, language: language ?? null });
export const deletedMeetings = () => invoke<MeetingSummary[]>("deleted_meetings");
export const renameMeeting = (id: string, title: string) => invoke<MeetingSummary>("rename_meeting", { id, title });
export const setLocked = (id: string, locked: boolean) => invoke<MeetingSummary>("set_locked", { id, locked });

import type { MeetingMedia } from './bindings/MeetingMedia';
export async function meetingMedia(id: string): Promise<MeetingMedia> {
  const media = await invoke<MeetingMedia>('meeting_media', {id});
  const {convertFileSrc} = await import('@tauri-apps/api/core');
  return {...media, tracks:media.tracks.map(track=>({...track,path:convertFileSrc(track.path)}))};
}
export const setActionItemDone = (id:string, line:number, done:boolean, expected:string) => invoke<string>('set_action_item_done',{id,line,done,expected});
export const saveNotes = (id:string, markdown:string, expected:string) => invoke<string>('save_notes',{id,markdown,expected});
export const saveTranscriptLine = (id:string, index:number, text:string) => invoke<string>('save_transcript_line',{id,index,text});

import type { Settings } from './bindings/Settings';
export type CallApp=Settings['apps'][number];
export const getSettings=()=>invoke<Settings>('get_settings');
export const saveSettings=(settings:Settings)=>invoke<Settings>('save_settings',{settings});
export const openSettings=(tab='general')=>invoke<void>('open_settings',{tab});
export const setAskOnCall=(on:boolean)=>invoke<void>('set_ask_on_call',{on});
export const setAppMode=(appId:string,mode:'ask'|'off')=>invoke<void>('set_app_mode',{appId,mode});
export const callApps=()=>invoke<CallApp[]>('call_apps');
export const recentMicApps=()=>invoke<string[]>('recent_mic_apps');
export const addApp=(appId:string)=>invoke<Settings>('add_app',{appId});
export const installedModels=()=>invoke<{speech:string[];notes:string[];warning:string|null;suggested:string|null;memory_gb:number|null}>('installed_models');
export const pickSpeechModel=()=>invoke<string|null>('pick_speech_model');
export const runSpeedTest=()=>invoke<number>('run_speed_test');
import type {StorageSummary} from "./bindings/StorageSummary";
export type {StorageSummary};
export const storageSummary=()=>invoke<StorageSummary>('storage_summary');
export const deleteAllMeetings=()=>invoke<void>('delete_all_meetings',{confirm:true});
export const moveNotesFolder=()=>invoke<string|null>('move_notes_folder');
export const about=()=>invoke<{version:string;licences:string;platform:string}>('about');
export const openLogsFolder=()=>invoke<void>('open_logs_folder');
export async function onSettingsTab(callback:(tab:string)=>void){
  const {listen}=await import('@tauri-apps/api/event');return listen<string>('motetto://settings-tab',event=>callback(event.payload));
}

// Read in (Notes tab): the notes translated on this computer, one saved file per language.
export type Translation = { code: string; markdown: string; stale: boolean };
export type TranslateProgress = { id: string; code: string; part: number; of: number; done: boolean; error: string | null };
export const notesTranslations = (id: string) => invoke<Translation[]>('notes_translations', { id });
export const getTranslation = (id: string, code: string) => invoke<Translation | null>('get_translation', { id, code });
export const translateNotes = (id: string, code: string) => invoke<Translation>('translate_notes', { id, code });
export const translating = () => invoke<TranslateProgress | null>('translating');
export const stopTranslation = (id: string) => invoke<void>('stop_translation', { id });
export async function onTranslate(callback: (progress: TranslateProgress) => void) {
  const { listen } = await import('@tauri-apps/api/event');
  return listen<TranslateProgress>('motetto://translate', event => callback(event.payload));
}
