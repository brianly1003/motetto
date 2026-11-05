"use client";
// A screen recording kept as just the video (mockup clipHtml, owner 2026-11-05): the same library
// item as a meeting, whose page is the player. While it records, the page says what is being
// recorded; once saved, it plays, shows its facts and offers notes when it has sound.
import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import type { Meeting } from "@/lib/bindings/Meeting";
import type { MeetingMedia } from "@/lib/bindings/MeetingMedia";
import type { ScreenFacts } from "@/lib/bindings/ScreenFacts";
import type { AppState } from "@/lib/bindings/AppState";
import { elapsedLabel } from "@/lib/session";
import { strings as s } from "@/lib/strings";
import * as ipc from "@/lib/ipc";
import { MenuIcon } from "./MenuIcon";
import { MeetingVideo, Play, useMeetingVideo } from "./VideoTab";
import TrimStrip, { CutIcon, mmss } from "./TrimStrip";

const svg = (d: ReactNode) => <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{d}</svg>;
export const FilmIcon = () => svg(<><rect x="2" y="3" width="12" height="10" rx="1.5"/><path d="M5 3v10M11 3v10M2 6h3M2 10h3M11 6h3M11 10h3"/></>);
const MicIcon = () => svg(<><rect x="6" y="1.8" width="4" height="8" rx="2"/><path d="M3.6 7.6a4.4 4.4 0 0 0 8.8 0M8 12v2.2"/></>);
export const MicOffIcon = () => svg(<><rect x="6" y="1.8" width="4" height="8" rx="2"/><path d="M3.6 7.6a4.4 4.4 0 0 0 8.8 0M8 12v2.2M2.4 2.4l11.2 11.2"/></>);
const SpeakerIcon = () => svg(<><path d="M2.5 6h2.6L8.5 3.2v9.6L5.1 10H2.5z"/><path d="M10.8 5.6a3.4 3.4 0 0 1 0 4.8M12.6 3.8a6 6 0 0 1 0 8.4"/></>);
const NotesIcon = () => svg(<><path d="M4 2.2h5.6L12.5 5v8.8H4z"/><path d="M9.4 2.2V5h3M6 8h4.4M6 10.4h4.4"/></>);
const ClickIcon = () => svg(<><path d="M4 3.5l7.4 3.2-3.2 1-1 3.2z"/><path d="M8.4 8.4l3.6 3.6"/></>);
const PauseIcon = () => <svg viewBox="0 0 16 16" fill="currentColor" aria-hidden="true"><rect x="4" y="3" width="2.6" height="10" rx="1"/><rect x="9.4" y="3" width="2.6" height="10" rx="1"/></svg>;

/** "22 s", "1 min 27 s": how long a trimmed video is, as the toast says it. */
export const clipLength = (secs: number) => secs < 60 ? `${Math.max(1, Math.round(secs))} s` : `${Math.floor(secs / 60)} min ${Math.round(secs % 60)} s`;
/** Trim only shows on a saved screen recording; ⌘T and the head's button share this. */
export const trimmable = (meeting: Meeting, live: boolean) => meeting.summary.kind === "screen" && meeting.summary.status === "Ready" && !live;

function chips(f: ScreenFacts) {
  const what = f.what === "area" ? <MenuIcon name="area"/> : f.what === "window" ? <MenuIcon name="window"/> : <MenuIcon name="screen"/>;
  const list: [ReactNode, string][] = [
    [what, f.source],
    [<FilmIcon key="f"/>, `${f.what === "display" && f.width > 0 ? `${f.width}×${f.height} · ` : ""}${f.fps > 0 ? `${f.fps} fps · ` : ""}MP4`],
    [f.mic_off ? <MicOffIcon key="m"/> : <MicIcon key="m"/>, f.mic_off ? "No mic" : "Your mic"],
    [<SpeakerIcon key="s"/>, f.app_sounds ? "App sounds" : "No app sounds"],
    ...(f.clicks ? [[<ClickIcon key="c"/>, "Clicks shown"] as [ReactNode, string]] : []),
  ];
  return list.map(([icon, text]) => <span key={text} className="cfact">{icon}{text}</span>);
}

const frameStyle = (f: ScreenFacts | null) => {
  const w = f?.width || 16, h = f?.height || 10;
  return { aspectRatio: `${w} / ${h}`, "--ar": (w / h).toFixed(3) } as CSSProperties;
};

/** The head's actions on a saved screen recording: Trim, Show in Finder, Export MP4…. */
export function ClipActions({ id, trimming, onTrim, onToast, onError }: { id: string; trimming: boolean; onTrim: () => void; onToast?: (text: string) => void; onError: (error: string) => void }) {
  const [exporting, setExporting] = useState(false);
  const mac = typeof navigator !== "undefined" && /Mac/.test(navigator.platform);
  async function exportMp4() {
    setExporting(true);
    try { if (await ipc.exportVideo(id)) onToast?.("Exported the video."); } catch (e) { onError(String(e)); } finally { setExporting(false); }
  }
  return <div className="headacts">
    <button type="button" className="btn sm" aria-pressed={trimming} title={`Trim (${mac ? "⌘" : "Ctrl+"}T)`} onClick={onTrim}><CutIcon/>Trim</button>
    <button type="button" className="btn sm" onClick={() => void ipc.meetingAction(id, "reveal").catch(e => onError(String(e)))}><MenuIcon name="folder"/>{mac ? "Show in Finder" : "Show in folder"}</button>
    <button type="button" className="btn sm" disabled={exporting} onClick={() => void exportMp4()}>Export MP4…</button>
  </div>;
}

type Player = { media: MeetingMedia; position: number; playing: boolean; rate: number; setRate: (rate: number) => void; seek: (ms: number) => void; scrub: (ms: number) => void; togglePlay: () => void };

/** While it records: the recbar, the source over the dark stage with ● REC. The part being
 *  written can't be read yet, so there is no picture here; the frame on screen shows what records. */
export function ClipRecording({ meeting, state, pending, act }: { meeting: Meeting; state: AppState; pending: boolean; act: (command: () => Promise<void>) => Promise<void> }) {
  const f = meeting.summary.facts;
  const paused = state.state.type === "Paused", stopped = state.state.type === "Stopped";
  const [remaining, setRemaining] = useState<number | null>(null);
  useEffect(() => {
    if (!stopped) return;
    let active = true;
    const tick = () => ipc.resumeRemainingMs().then(ms => { if (active) setRemaining(Math.ceil(ms / 1000)); }).catch(() => {});
    tick(); const timer = setInterval(tick, 250);
    return () => { active = false; clearInterval(timer); };
  }, [stopped]);
  const mac = typeof navigator !== "undefined" && /Mac/.test(navigator.platform);
  return <>
    <div className="recbar" role="status"><span className={`dot${stopped || paused ? " off" : " pulse"}`}/><b>{stopped ? s.chipStopped : paused ? s.paused : s.chipRecording}</b><span className="t">{elapsedLabel(state.elapsed_ms)}</span>
      <span className="tip">{stopped ? <>Saving to your library in <span>{remaining ?? "…"}</span> s</> : paused ? "Not recording. Resume when you're ready." : `The screen · ${f?.source ?? ""}${f?.mic_off ? " · no mic" : ""}`}</span><span className="sp"/>
      {stopped ? <><button type="button" className="btn sm primary" disabled={pending || remaining === 0} onClick={() => act(ipc.resumeRecording)}>{s.resume}</button><button type="button" className="btn sm ghost" disabled={pending || remaining === 0} title="The video goes to Recently deleted for 30 days." onClick={() => act(ipc.discardRecording)}>{s.pillDiscard}</button></>
        : paused ? <><button type="button" className="btn sm primary" disabled={pending} onClick={() => act(ipc.resumeRecording)}>{s.resume}</button><button type="button" className="btn sm rec" disabled={pending} onClick={() => act(ipc.stopRecording)}>{s.pillStop}</button></>
        : <><button type="button" className="btn sm ghost" disabled={pending} title={s.pauseTitle} onClick={() => act(ipc.pauseRecording)}>{s.pause}</button><button type="button" className="btn sm rec" disabled={pending} onClick={() => act(ipc.stopRecording)}>{s.pillStop}</button></>}</div>
    <div className="clipbody"><div className={`clipstage live${paused || stopped ? " held" : ""}`}><div className="clipframe" style={frameStyle(f)}><div className="cliplive">{f?.source}</div><span className="cliprec">{paused || stopped ? "Paused" : "● REC"}</span></div></div>
      <p className="tip clipnote"><MenuIcon name="lock"/> Motetto&apos;s own windows are never in the video. It stays on this {mac ? "Mac" : "PC"}.</p></div>
  </>;
}

/** Saved: the player (the meeting's one video element), click dots, the facts and the notes card. */
export default function ScreenRecordingPage({ meeting, player, trimming, onTrim, onTrimCancel, onTrimSave, onWriteNotes, writingNotes }: { meeting: Meeting; player: Player; trimming: boolean; onTrim: () => void; onTrimCancel: () => void; onTrimSave: (fromMs: number, toMs: number) => void; onWriteNotes: () => void; writingNotes: boolean }) {
  const { media, position, playing, rate, setRate, scrub, togglePlay } = player;
  const f = meeting.summary.facts;
  const { video, current, error, setError } = useMeetingVideo(media, position, playing, rate);
  const box = useRef<HTMLDivElement>(null);
  const [fullscreen, setFullscreen] = useState(false);
  useEffect(() => { const changed = () => setFullscreen(document.fullscreenElement === box.current); document.addEventListener("fullscreenchange", changed); return () => document.removeEventListener("fullscreenchange", changed); }, []);
  // ⌘T opens and closes the trim strip on this page.
  useEffect(() => {
    const key = (e: KeyboardEvent) => { if ((e.metaKey || e.ctrlKey) && !e.altKey && e.key.toLowerCase() === "t") { e.preventDefault(); onTrim(); } };
    document.addEventListener("keydown", key); return () => document.removeEventListener("keydown", key);
  }, [onTrim]);
  const total = media.duration_ms || 1;
  const quiet = !!f?.mic_off && !f.app_sounds;
  const mac = typeof navigator !== "undefined" && /Mac/.test(navigator.platform);
  return <div className="clipbody">
    <div ref={box} className="clipplayer">
      <div className="clipstage"><div className="clipframe" style={frameStyle(f)}><MeetingVideo video={video} setError={setError}/>{(!current || error) && <div className="vgap" role="status">{error || s.video_gap}</div>}
        <button type="button" className="vtoggle" aria-label={playing ? "Pause video" : "Play video"} onClick={togglePlay}>{!playing && <span className="vbig"><Play/></span>}</button></div></div>
      {trimming ? <TrimStrip media={media} reachMs={meeting.summary.reach_ms} onCancel={onTrimCancel} onSave={onTrimSave}/>
        : <div className="vctl clipctl"><button type="button" className="iconbtn" aria-label={playing ? "Pause" : "Play"} title="Play or pause (Space)" onClick={togglePlay}>{playing ? <PauseIcon/> : <Play/>}</button><span className="mono">{mmss(position / 1000)} / {mmss(media.duration_ms / 1000)}</span>
          <div className="vscrub"><i style={{ width: `${position / total * 100}%` }}/>{f?.clicks !== false && media.clicks.map(ms => <span key={ms} className="cdot" style={{ left: `${ms / total * 100}%` }} title="Click"/>)}<input type="range" aria-label={media.clicks.length ? "Seek. Dots mark your mouse clicks" : "Seek video"} min={0} max={media.duration_ms} value={position} onChange={e => scrub(Number(e.target.value))}/></div>
          <select aria-label="Speed" value={rate} onChange={e => setRate(Number(e.target.value))}>{[1, 1.5, 2].map(r => <option key={r} value={r}>{r}×</option>)}</select>
          <button type="button" className="iconbtn" aria-label={fullscreen ? "Exit full screen" : "Full screen"} title={fullscreen ? "Exit full screen (Esc)" : "Full screen"} onClick={() => void (document.fullscreenElement ? document.exitFullscreen() : box.current?.requestFullscreen())?.catch(() => {})}>{svg(<path d="M2.5 6V2.5H6M10 2.5h3.5V6M13.5 10v3.5H10M6 13.5H2.5V10"/>)}</button></div>}
    </div>
    {f && <div className="cfacts">{chips(f)}</div>}
    {quiet ? <div className="clipcard quiet"><span className="cic"><SpeakerIcon/></span><div><b>No sound in this video</b><p>You recorded it with the microphone and app sounds off, so there&apos;s nothing to transcribe.</p></div></div>
      : <div className="clipcard"><span className="cic"><NotesIcon/></span><div><b>Just the video</b><p>Motetto can transcribe {f?.mic_off ? "its app sounds" : "what was said"} and write notes, on this {mac ? "Mac" : "PC"}. The video stays as it is.</p></div><button type="button" className="btn sm" disabled={writingNotes} onClick={onWriteNotes}>Write notes</button></div>}
  </div>;
}
