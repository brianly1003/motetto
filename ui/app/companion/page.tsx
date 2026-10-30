"use client";
import {ScreenControl, ShareVideoOffer} from "@/components/ScreenMenu";
import { useEffect, useRef, useState } from "react";
import * as ipc from "@/lib/ipc";
import { useSession } from "@/lib/useSession";
import { elapsedLabel } from "@/lib/session";
import { strings as s } from "@/lib/strings";
import MotettoMark from "@/components/MotettoMark";
import { Duet } from "@/components/Duet";
import { MicMenu, MicNote, useMicReport } from "@/components/Microphone";
import { AskCard } from "@/components/AskCard";
import { MyNotes } from "@/components/MyNotes";
import { JumpToNow, useFollow } from "@/components/JumpToNow";
import { gapLabel, transcriptLines } from "@/lib/transcript";
import "../../styles/companion.css";
import { KbdGroup } from "@/components/ui/Kbd";

export default function Companion() {
  const { state, notice, setNotice, pending, act, gaps } = useSession();
  const [captions, setCaptions] = useState(true);
  const [notesShortcut,setNotesShortcut]=useState('⌃⌥N');
  useEffect(()=>{
    let active=true;let off:(()=>void)|undefined;
    const refresh=()=>ipc.getSettings().then(s=>{if(active){setCaptions(s.companion.captions);setNotesShortcut(/Mac/.test(navigator.platform)?s.shortcuts.N.replace('Ctrl+','⌃').replace('Alt+','⌥').replace('Shift+','⇧').replace('Super+','⌘'):s.shortcuts.N);}}).catch(()=>{});
    void refresh();void ipc.subscribe('motetto://settings-changed',()=>void refresh()).then(fn=>{if(active)off=fn;else fn();});
    return()=>{active=false;off?.();};
  },[]);
  const [remaining, setRemaining] = useState<number | null>(null);
  const [markedAt, setMarkedAt] = useState<number | null>(null);
  useEffect(() => {
    if (markedAt === null) return;
    const timeout = setTimeout(() => setMarkedAt(null), 600);
    return () => clearTimeout(timeout);
  }, [markedAt]);
  async function mark() { await act(async () => { setMarkedAt(await ipc.markMoment()); }); }
  const flushNotes = useRef<() => Promise<void>>(async () => {});
  // Tucked: press and move 4 px to drag the capsule; a click without movement opens the Sidecar.
  const dragFrom = useRef<{ x: number; y: number; moved: boolean } | null>(null);
  const [liveError, setLiveError] = useState("");
  const [live, setLive] = useState("");
  const [showLive, setShowLive] = useState(true);
  const liveLines = useRef<HTMLDivElement>(null);
  const splitPanes = useRef<HTMLDivElement>(null);
  const [liveShare, setLiveShare] = useState(38);
  const resizeStart = useRef<{ y: number; share: number } | null>(null);
  const clampShare = (share: number) => Math.max(20, Math.min(80, share));
  const follow = useFollow(liveLines, showLive);
  const pinned = state?.companion.pinned ?? false;
  const [jot, setJot] = useState("");
  const [jotOpen, setJotOpen] = useState(false);
  const [notesKey, setNotesKey] = useState(0);
  const meetingId = state?.meeting_id;
  // No size menu (owner 2026-10-20): ✎ or the caption opens the Sidecar; ⇥, Esc or ⌃⌥Space tucks it.
  // A click outside (focus leaving) slides an unpinned Sidecar out, then tucks it (owner, 2026-10-20).
  const [leaving, setLeaving] = useState(false);
  const recordingOrPaused = state?.state.type === "Recording" || state?.state.type === "Paused";
  // The mic (DESIGN 4.15): "You" in the Duet opens Record from; clipping (the one problem Motetto
  // names) takes the caption's line (Rust keeps the Pill's size) until ✕, the Dot gets an amber ring.
  const mic = useMicReport(recordingOrPaused);
  const [micMenu, setMicMenu] = useState<{ left: number; top: number } | null>(null);
  const [micHidden, setMicHidden] = useState(false);
  const micMenuBox = useRef<HTMLDivElement>(null);
  useEffect(() => { setMicHidden(false); }, [meetingId]);
  useEffect(() => {
    if (!micMenu) return;
    // The window grows just enough for the menu, and shrinks back when it closes.
    const height = micMenuBox.current?.offsetHeight ?? 340;
    void ipc.companionMenu(true, micMenu.top + height + 8 - 72).catch(() => {});
    return () => { void ipc.companionMenu(false).catch(() => {}); };
  }, [micMenu]);
  // A click anywhere else closes it: in this window (the Pill, or the space the window grew for
  // the menu) or in another app, which takes focus from the companion.
  useEffect(() => {
    if (!micMenu) return;
    let active = true;
    let off: (() => void) | undefined;
    const close = (event: PointerEvent) => { if (!(event.target as Element | null)?.closest?.(".micmenu-host, .micbtn")) setMicMenu(null); };
    document.addEventListener("pointerdown", close);
    ipc.onCompanionFocus(focused => { if (active && !focused) setMicMenu(null); })
      .then(cleanup => { if (active) off = cleanup; else cleanup(); }).catch(() => {});
    return () => { active = false; document.removeEventListener("pointerdown", close); off?.(); };
  }, [micMenu]);
  const micDone = (text: string) => { setNotice(text); setTimeout(() => setNotice(""), 4000); void mic.refresh(); };
  const micWarn = !!mic.report?.issue && !micHidden;
  const openMicMenu = (anchor: HTMLElement) => { const box = anchor.getBoundingClientRect(); setMicMenu({ left: Math.max(6, Math.min(box.left - 12, innerWidth - 310)), top: box.bottom + 6 }); };
  useEffect(() => {
    if (state?.companion.size !== "Sidecar" || pinned || !recordingOrPaused) return;
    let active = true;
    let off: (() => void) | undefined;
    let timer: ReturnType<typeof setTimeout> | undefined;
    ipc.onCompanionFocus(focused => {
      if (!active) return;
      clearTimeout(timer);
      setLeaving(!focused);
      if (!focused) timer = setTimeout(() => { void act(() => size("Tucked")).finally(() => setLeaving(false)); }, 180);
    }).then(cleanup => { if (active) off = cleanup; else cleanup(); }).catch(() => {});
    return () => { active = false; clearTimeout(timer); setLeaving(false); off?.(); };
  }, [state?.companion.size, pinned, recordingOrPaused]);
  useEffect(() => {
    let active = true;
    let off: (() => void) | undefined;
    ipc.onOpenNotes(mode => { if (mode === "jot") { setJotOpen(true); void ipc.companionMenu(true); } })
      .then(cleanup => { if (active) off = cleanup; else cleanup(); }).catch(() => {});
    return () => { active = false; off?.(); };
  }, []);

  useEffect(() => {
    let active = true;
    const timer = setInterval(() => { ipc.liveTranscript().then(([id, text, error]) => { if (active && id === meetingId) { setLive(text); setLiveError(error || ""); } }).catch(() => {}); }, 1000);
    setLive("");
    setLiveError("");
    return () => { active = false; clearInterval(timer); };
  }, [meetingId]);
  async function size(value: "Dot" | "Pill" | "Sidecar" | "Tucked") {
    await flushNotes.current(); await ipc.companionMenu(false); setJotOpen(false); await ipc.setCompanionSize(value);
  }
  async function stop() { await flushNotes.current(); await ipc.stopRecording(); }
  const stopped = state?.state.type === "Stopped";
  // Paused keeps an open Sidecar for notes; Dot, Tucked and Pill show the paused bar (Rust sizes it).
  const paused = state?.state.type === "Paused";
  // The call's app let go of the mic: the idle stop counts down on the recording clock.
  const endingIn = state?.state.type === "Recording" && state.stops_at_ms != null ? Math.max(0, Math.ceil((state.stops_at_ms - state.elapsed_ms) / 1000)) : null;
  useEffect(() => {
    if (!state) return;
    // Report readiness after controls have been painted, never merely after the webview loaded.
    const frame = requestAnimationFrame(() => { ipc.companionReady().catch(() => {}); });
    return () => cancelAnimationFrame(frame);
  }, [state]);
  useEffect(() => {
    if (!stopped) return;
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    async function tick() {
      try { const ms = await ipc.resumeRemainingMs(); if (active) setRemaining(Math.ceil(ms / 1000)); }
      catch { if (active) setRemaining(null); }
      if (active) timer = setTimeout(tick, 250);
    }
    setRemaining(null);
    tick();
    return () => { active = false; clearTimeout(timer); };
  }, [stopped]);
  if (!state) return <div className="companion-loading">{s.loading}</div>;
  const asking = state.state.type === "Asking" ? state.state : null;
  const offPending = !asking && state.state.type === "Idle" ? state.ask_off_pending : null;
  const discarded = !asking && state.state.type === "Idle" ? state.discarded : null;
  // Asking and the Undo prompts are Pill-only: Rust sizes them (Ask card, then Pill), never Dot or Tucked.
  const prompting = !!(asking || offPending || discarded);
  if (asking && state.companion.size === "Ask") return <AskCard key={asking.call_id} app={asking.app} callId={asking.call_id}/>;
  const dot = state.companion.size === "Dot" && !state.quit_pending && !stopped && !paused && !prompting;
  const sidecar = state.companion.size === "Sidecar" && !stopped && !state.quit_pending && !prompting;
  const tucked = state.companion.size === "Tucked" && !stopped && !state.quit_pending && !prompting;
  const lines = transcriptLines(live, []);
  // Each pause shows as one muted line before the first line spoken after it.
  const gapsBefore = (index: number) => gaps.filter(gap => lines.findIndex(line => line.ms !== null && line.ms >= gap.at_ms) === index);
  const gapRows = (index: number) => gapsBefore(index).map(gap => <div className="line gap" key={`gap-${gap.at_ms}`}><span className="ts">{elapsedLabel(gap.at_ms)}</span><span className="who"/><span className="tx">{s.pausedFor} {gapLabel(gap.ms)}</span></div>);
  const latest = lines.at(-1);
  return <><main className="pill" data-screen={state.screen&&!state.screen.waiting&&state.state.type==="Recording"?"on":undefined} data-size={dot ? "dot" : tucked ? "tucked" : sidecar ? "sidecar" : "pill"} data-peek={state.companion.peeking} data-leaving={leaving || undefined} data-asking={prompting || undefined} role="toolbar" tabIndex={0} aria-label={s.companion}
    onKeyDown={event => {
      if (event.code === "Space" && event.target === event.currentTarget && state.state.type === "Recording") { event.preventDefault(); void mark(); }
      if (event.key === "Escape" && (state.state.type === "Recording" || paused)) {
        event.preventDefault();
        if (jotOpen) { setJotOpen(false); void act(() => ipc.companionMenu(false)); }
        else if (!(sidecar && pinned)) void act(() => size(sidecar ? "Tucked" : "Dot"));
      }
    }}>
    {tucked ? <><button className="handle" aria-label="Recording. Open notes and live transcript" title="Drag to move"
      onPointerDown={event => { if (event.button === 0) { event.currentTarget.setPointerCapture(event.pointerId); dragFrom.current = { x: event.screenX, y: event.screenY, moved: false }; } }}
      onPointerUp={event => { if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId); }}
      onPointerMove={event => {
        const from = dragFrom.current;
        if (!from || from.moved || !(event.buttons & 1) || Math.hypot(event.screenX - from.x, event.screenY - from.y) < 4) return;
        from.moved = true;
        void act(() => ipc.dragCompanion());
      }}
      onClick={() => { const moved = dragFrom.current?.moved; dragFrom.current = null; if (!moved) void act(() => size("Sidecar")); }}><span className={`dot ${paused ? "stopped" : "pulse"}${micWarn ? " micwarn" : ""}`} /><MotettoMark className="mark"/></button>
      {/* Pause and Stop without opening anything (owner 2026-10-20); Stop then shows the Pill's 10 s Resume. */}
      <span className="tuckctl">
        {paused
          ? <button className="iconbtn" disabled={pending} onClick={() => act(ipc.resumeRecording)} aria-label={s.resume} title={s.resume}><svg viewBox="0 0 16 16" fill="currentColor" stroke="none" aria-hidden="true"><path d="M5 3.2v9.6a.6.6 0 0 0 .9.5l7.4-4.8a.6.6 0 0 0 0-1L5.9 2.7a.6.6 0 0 0-.9.5z"/></svg></button>
          : <button className="iconbtn" disabled={pending} onClick={() => act(async () => { await flushNotes.current(); await ipc.pauseRecording(); })} aria-label={s.pauseRecording} title={s.pauseTitle}><svg viewBox="0 0 16 16" fill="currentColor" stroke="none" aria-hidden="true"><rect x="4" y="3" width="2.6" height="10" rx="1"/><rect x="9.4" y="3" width="2.6" height="10" rx="1"/></svg></button>}
        <button className="iconbtn tstop" disabled={pending} onClick={() => act(stop)} aria-label={s.pillStop} title={s.pillStop}><svg viewBox="0 0 16 16" aria-hidden="true"><rect x="4" y="4" width="8" height="8" rx="2"/></svg></button>
      </span></> : dot ? <button className="dotbtn" aria-label={s.showControls} title={`${s.recording} · ${elapsedLabel(state.elapsed_ms)}`} onClick={() => act(() => ipc.setCompanionSize("Pill"))}><span className={`dot pulse${micWarn ? " micwarn" : ""}`} /></button> :
      <div className="pillbar" data-tauri-drag-region>
        {state.quit_pending ? <>
          <span className="pillmsg">{notice || (state.state.type === "Finishing" ? s.quitPending : s.pillQuitQuestion)}</span>
          <button className="quiet" disabled={pending} onClick={() => act(() => ipc.quitRecording(false))}>{state.state.type === "Recording" ? s.keepRecording : s.stayOpen}</button>
          {state.state.type !== "Finishing" && <button className="primary" disabled={pending} onClick={() => act(async () => { await flushNotes.current(); await ipc.quitRecording(true); })}>{s.stopAndSave}</button>}
        </> : asking ? <>
          <span className="dot amber" aria-hidden="true" />
          <span className="pillmsg" role="status">{notice || s.askPill.replace("{App}", asking.app)}</span>
          <button className="primary" disabled={pending} onClick={() => act(() => ipc.answerAsk(asking.call_id, true))}>{s.askRecord}</button>
          <button className="iconbtn" disabled={pending} aria-label={s.askNotThisCall} title={s.askNotThisCall} onClick={() => act(() => ipc.answerAsk(asking.call_id, false))}>✕</button>
        </> : offPending ? <>
          <span className="dot stopped" aria-hidden="true" />
          <span className="pillmsg" role="status">{notice || s.askOffDone.replace("{App}", offPending.app)}</span>
          <button className="quiet" disabled={pending} onClick={() => act(() => ipc.undoDontAsk(offPending.app_id))}>{s.undo}</button>
        </> : discarded ? <>
          <span className="dot stopped" aria-hidden="true" />
          <span className="pillmsg" role="status">{notice || s.pillDiscarded}</span>
          <button className="quiet" disabled={pending} onClick={() => act(() => ipc.undoDiscard(discarded.meeting_id))}>{s.undo}</button>
        </> : paused ? <>
          <span className="dot stopped" aria-hidden="true" />
          <time className="t">{elapsedLabel(state.elapsed_ms)}</time>
          <span className="pillmsg" role="status">{notice || s.pillPaused}</span>
          <button className="primary" disabled={pending} onClick={() => act(ipc.resumeRecording)}>{s.resume}</button>
          <button className="stop" disabled={pending} onClick={() => act(stop)}>{s.pillStop}</button>
        </> : stopped ? <>
          <span className="dot stopped" /><span className="pillmsg" role="status">{notice || <><span className="lbl"><span className="long">{s.pillStopped}</span><span className="short">{s.pillStoppedShort}</span></span><span className="num" aria-live="off">{remaining ?? "…"} {s.seconds}</span></>}</span>
          <button className="primary" disabled={pending || remaining === 0} onClick={() => act(ipc.resumeRecording)}>{s.resume}</button>
          <button className="quiet" disabled={pending || remaining === 0} onClick={() => act(ipc.discardRecording)} title={s.pillDiscardTitle}>{s.pillDiscard}</button>
        </> : endingIn !== null && !sidecar ? <>
          <span className="dot pulse" aria-hidden="true" />
          <time className="t">{elapsedLabel(state.elapsed_ms)}</time>
          <span className="pillmsg" role="status" title={state.app ? s.pillEndingTitle.replace("{App}", state.app) : s.pillEndingQuiet}>{notice || <>{s.pillEnding} <span aria-live="off">{endingIn}</span> {s.seconds}</>}</span>
          <button className="quiet" disabled={pending} onClick={() => act(ipc.keepRecording)}>{s.keepRecording}</button>
          <button className="stop" disabled={pending} onClick={() => act(stop)}>{s.pillStop}</button>
        </> : <>
          <span className="dot pulse" aria-hidden="true" />
          <time className="t">{elapsedLabel(state.elapsed_ms)}</time>
          <Duet you={<button type="button" className={`lm micbtn${mic.report?.issue ? " warn" : ""}`} aria-haspopup="menu" aria-expanded={!!micMenu}
            aria-label={`Your microphone: ${mic.report?.active?.name ?? "system default"}${mic.report?.issue ? ", needs attention" : ""}. Change`}
            onClick={event => { if (micMenu || !mic.report) setMicMenu(null); else openMicMenu(event.currentTarget); }}>You</button>} />
          <ScreenControl state={state} companion/>
          <button className="iconbtn markbtn" disabled={pending} onClick={mark} aria-label={s.markMoment} title={markedAt === null ? s.markMoment : `${s.marked} ${elapsedLabel(markedAt)}`} aria-pressed={markedAt !== null}>⚑</button>
          <button className="iconbtn" disabled={pending} onClick={() => act(async () => { await flushNotes.current(); await ipc.pauseRecording(); })} aria-label={s.pauseRecording} title={s.pauseTitle}><svg viewBox="0 0 16 16" fill="currentColor" stroke="none" aria-hidden="true"><rect x="4" y="3" width="2.6" height="10" rx="1"/><rect x="9.4" y="3" width="2.6" height="10" rx="1"/></svg></button>
          <button className="stop" disabled={pending} onClick={() => act(stop)}>{s.pillStop}</button>
          {sidecar ? <><button className="iconbtn" aria-label="Keep open" title="Keep open while I work elsewhere" aria-pressed={pinned} onClick={() => act(() => ipc.pinCompanion(!pinned))}><svg viewBox="0 0 16 16" fill="none" stroke="currentColor"><path d="M6 2h4l-.5 4 2.5 2.5H4L6.5 6zM8 8.5V14"/></svg></button><button className="iconbtn" aria-label="Tuck to the screen edge" onClick={() => act(() => size("Tucked"))}><svg viewBox="0 0 16 16" fill="none" stroke="currentColor"><path d="M3 8h8M8 5l3 3-3 3M13.5 3v10"/></svg></button></> : <button className="iconbtn" aria-label="Open my notes" title={`Take a note (${notesShortcut})`} onClick={() => act(async () => { await size("Sidecar"); await ipc.takeNote(); })}><svg viewBox="0 0 16 16" fill="none" stroke="currentColor"><path d="M10.5 2.5l3 3L6 13H3v-3z"/></svg></button>}
        </>}
      </div>}
    {!dot && !tucked && !sidecar && !prompting && recordingOrPaused && micWarn
      ? <MicNote report={mic.report} done={micDone} onHide={() => setMicHidden(true)}/>
      : captions && !dot && !tucked && !sidecar && !prompting && <button className="caption" aria-label="Latest line. Click to open the live transcript" onClick={() => act(() => size("Sidecar"))}><span className={`who ${latest?.speaker === "You" ? "me" : "them"}`}>{latest?.speaker || "…"}</span><span className="ctext">{notice || liveError || latest?.text || "Listening… lines appear a few seconds after people speak."}</span></button>}
    {jotOpen && <form className="jotbar" onSubmit={event => { event.preventDefault(); void act(async () => { await flushNotes.current(); await ipc.addJot(jot); setJot(""); setNotesKey(key => key + 1); setJotOpen(false); await ipc.companionMenu(false); }); }}><input type={state.sharing ? "password" : "text"} autoFocus value={jot} onChange={event => setJot(event.target.value)} placeholder="Jot a line into My notes…" aria-label="Jot a line into My notes"/><KbdGroup keys="⌃⌥J"/><button type="submit" className="quiet">Add</button></form>}
    {sidecar && meetingId && <div className="pilldrawer">{micWarn && <MicNote report={mic.report} done={micDone} onHide={() => setMicHidden(true)}/>}{state.companion.peeking && <div className="sharebadge">Visible to your share</div>}<div className="why"><span>{notice || (state.started_by?.type === "App" ? s.whyStarted.replace("{App}", state.started_by.app) : s.startedManually)}</span><button className="quiet" onClick={() => act(() => navigator.clipboard.writeText("I'm recording this call with Motetto to take notes. Audio and notes stay on my computer."))}>Copy a recording notice</button></div><div className="splitpanes" ref={splitPanes} data-live={showLive} style={{ gridTemplateRows: showLive ? `minmax(70px, ${liveShare}fr) 9px minmax(120px, ${100 - liveShare}fr)` : "auto 0px minmax(0, 1fr)" }}><section className="live-pane" id="live-pane"><button className="panehead" aria-expanded={showLive} onClick={() => setShowLive(!showLive)}><span>Live</span><span className="tip">click a line to quote it</span></button><div ref={liveLines} className="lines" hidden={!showLive} onScroll={follow.onScroll}>{liveError && <p className="tip" role="status">{liveError}</p>}{lines.length ? lines.flatMap((line, i) => [...gapRows(i), <button className="line" key={i} onClick={() => act(async () => { await flushNotes.current(); await ipc.addJot(`> ${line.text} [${line.time}]`); setNotesKey(key => key + 1); })}><span className="ts">{line.time}</span><span className={`who ${line.speaker === "You" ? "me" : "them"}`}>{line.speaker}</span><span className="tx">{line.text}</span></button>]).concat(gapRows(-1), state.state.type === "Recording" && !liveError ? [<div className="line pending" key="pending" aria-hidden="true"><span className="ts"><span>·</span><span>·</span><span>:</span><span>·</span><span>·</span></span><span className="who them dots"><i>.</i><i>.</i><i>.</i></span><span className="tx">listening</span></div>] : []) : !liveError && <span className="tip">Listening… lines appear a few seconds after people speak.</span>}</div><JumpToNow show={follow.behind} onJump={follow.jump}/></section><div className="pane-resizer" role="separator" aria-label="Resize Live and My notes" aria-orientation="horizontal" aria-controls="live-pane notes-pane" aria-valuemin={20} aria-valuemax={80} aria-valuenow={Math.round(liveShare)} tabIndex={showLive ? 0 : -1} hidden={!showLive}
      onPointerDown={e => { if (e.button !== 0) return; e.preventDefault(); e.currentTarget.setPointerCapture(e.pointerId); resizeStart.current = { y: e.clientY, share: liveShare }; }}
      onPointerMove={e => { const start = resizeStart.current, box = splitPanes.current; if (start && box) setLiveShare(clampShare(start.share + (e.clientY - start.y) / (box.clientHeight - 9) * 100)); }}
      onPointerUp={e => { resizeStart.current = null; if (e.currentTarget.hasPointerCapture(e.pointerId)) e.currentTarget.releasePointerCapture(e.pointerId); }}
      onLostPointerCapture={() => { resizeStart.current = null; }}
      onKeyDown={e => { const value = e.key === "Home" ? 20 : e.key === "End" ? 80 : e.key === "ArrowUp" ? liveShare - 5 : e.key === "ArrowDown" ? liveShare + 5 : null; if (value !== null) { e.preventDefault(); e.stopPropagation(); setLiveShare(clampShare(value)); } }}
    /><section className="notes-pane" id="notes-pane"><MyNotes key={`${meetingId}-${notesKey}`} meetingId={meetingId} elapsed={state.elapsed_ms} registerFlush={flush => { flushNotes.current = flush; }}/></section></div></div>}
    {markedAt !== null && <span className="sr-only" role="status">{s.marked} {elapsedLabel(markedAt)}</span>}
    {notice && <span className="sr-only" role="alert">{notice}</span>}
  </main>
  {recordingOrPaused&&<ShareVideoOffer state={state}/>}
  {/* Outside the Pill: its rounded clip would cut the menu off. */}
  {micMenu && mic.report && <div ref={micMenuBox} className="micmenu-host" style={{ position: "fixed", left: micMenu.left, top: micMenu.top, zIndex: 60 }}>
    <MicMenu report={mic.report} recording={state.state.type === "Recording"} done={micDone} onClose={() => setMicMenu(null)} style={{ position: "static" }}/></div>}
  </>;
}
