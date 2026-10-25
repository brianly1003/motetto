"use client";
import { useEffect, useRef, useState } from "react";
import * as ipc from "@/lib/ipc";
import { elapsedLabel } from "@/lib/session";
import { strings as s } from "@/lib/strings";
import { gapLabel, transcriptLines } from "@/lib/transcript";
import { stageTimes } from "@/lib/stageClock";
import { MyNotes } from "./MyNotes";
import { JumpToNow, useFollow } from "./JumpToNow";
import { MicChip, MicMenu, MicNote, useMicReport } from "./Microphone";
import type { AppState } from "@/lib/bindings/AppState";
import type { Stage } from "@/lib/bindings/Stage";
import type { Gap } from "@/lib/useSession";

type Session = { state: AppState; gaps: Gap[]; pending: boolean; act: (command: () => Promise<void>) => Promise<void> };

const PauseIcon = () => <svg viewBox="0 0 16 16" fill="currentColor" aria-hidden="true"><rect x="4" y="3" width="2.6" height="10" rx="1"/><rect x="9.4" y="3" width="2.6" height="10" rx="1"/></svg>;

// The meeting page while it records (DESIGN 4.8): a place to work, not a status screen. My notes is
// the companion's my-notes.md; the live transcript is the same preview the companion polls.
export function RecordingPage({ session: { state, gaps, pending, act }, onToast }: { session: Session; onToast?: (text: string, undo?: () => Promise<void>) => void }) {
  const id = state.meeting_id!;
  const paused = state.state.type === "Paused", stopped = state.state.type === "Stopped";
  // The mic (DESIGN 4.15): the chip opens Record from; a problem shows under the bar until ✕.
  const mic = useMicReport(!stopped);
  const [micMenu, setMicMenu] = useState<{ left: number; top: number } | null>(null);
  const [micHidden, setMicHidden] = useState(false);
  const toast = (text: string, undo?: () => Promise<void>) => { onToast?.(text, undo); void mic.refresh(); };
  const openMicMenu = (anchor: HTMLElement) => { const box = anchor.getBoundingClientRect(); setMicMenu({ left: Math.max(8, Math.min(box.left, innerWidth - 312)), top: box.bottom + 6 }); };
  useEffect(() => {
    if (!micMenu) return;
    const close = (event: PointerEvent) => { if (!(event.target as Element | null)?.closest?.(".micmenu, .micchip")) setMicMenu(null); };
    document.addEventListener("pointerdown", close); return () => document.removeEventListener("pointerdown", close);
  }, [micMenu]);
  const [tab, setTab] = useState<"notes" | "live">("notes");
  const [live, setLive] = useState("");
  const [liveError, setLiveError] = useState("");
  const [notesKey, setNotesKey] = useState(0);
  const [marked, setMarked] = useState<number | null>(null);
  const [remaining, setRemaining] = useState<number | null>(null);
  const flush = useRef<() => Promise<void>>(async () => {});
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    let active = true;
    const poll = () => ipc.liveTranscript().then(([meeting, text, error]) => { if (active && meeting === id) { setLive(text); setLiveError(error || ""); } }).catch(() => {});
    poll(); const timer = setInterval(poll, 1000);
    return () => { active = false; clearInterval(timer); };
  }, [id]);
  useEffect(() => {
    if (!stopped) return;
    let active = true;
    const tick = () => ipc.resumeRemainingMs().then(ms => { if (active) setRemaining(Math.ceil(ms / 1000)); }).catch(() => {});
    tick(); const timer = setInterval(tick, 250);
    return () => { active = false; clearInterval(timer); };
  }, [stopped]);
  useEffect(() => { if (marked === null) return; const t = setTimeout(() => setMarked(null), 2000); return () => clearTimeout(t); }, [marked]);
  const lines = transcriptLines(live, []);
  const follow = useFollow(box, tab === "live");
  const gapRows = (index: number) => gaps.filter(gap => lines.findIndex(line => line.ms !== null && line.ms >= gap.at_ms) === index)
    .map(gap => <div className="line gap" key={`gap-${gap.at_ms}`}><span className="ts">{elapsedLabel(gap.at_ms)}</span><span className="who"/><span className="tx">{s.pausedFor} {gapLabel(gap.ms)}</span></div>);
  const count = lines.length;
  return <>
    <div className="recbar" role="status">
      <span className={`dot${paused || stopped ? " off" : " pulse"}`} aria-hidden="true"/>
      <b>{stopped ? s.chipStopped : paused ? s.paused : s.chipRecording}</b>
      <time className="t">{elapsedLabel(state.elapsed_ms)}</time>
      <span className="tip">{stopped ? <>The notes start in {remaining ?? "…"} s</> : paused ? "Not recording. Resume when you're ready." : marked !== null ? `${s.marked} ${elapsedLabel(marked)}.` : `Both sides · ${count} line${count === 1 ? "" : "s"}`}</span>
      <span className="sp"/>
      {!stopped && <MicChip report={mic.report} open={!!micMenu} onToggle={button => { if (micMenu) setMicMenu(null); else openMicMenu(button); }}/>}
      {stopped || paused
        ? <><button className="doc-action primary" disabled={pending || (stopped && remaining === 0)} onClick={() => act(ipc.resumeRecording)}>{s.resume}</button>
          {stopped && <button className="doc-action ghost" disabled={pending || remaining === 0} title={s.pillDiscardTitle} onClick={() => act(ipc.discardRecording)}>{s.pillDiscard}</button>}
          {paused && <button className="doc-action danger" disabled={pending} onClick={() => act(async () => { await flush.current(); await ipc.stopRecording(); })}>{s.pillStop}</button>}</>
        : <><button className="doc-action ghost" disabled={pending} title={`${s.markMoment} (⌃⌥M)`} onClick={() => act(async () => setMarked(await ipc.markMoment()))}>⚑ Mark</button>
          <button className="doc-action ghost" disabled={pending} title={s.pauseTitle} onClick={() => act(async () => { await flush.current(); await ipc.pauseRecording(); })}><PauseIcon/> {s.pause}</button>
          <button className="doc-action danger" disabled={pending} onClick={() => act(async () => { await flush.current(); await ipc.stopRecording(); })}>{s.pillStop}</button></>}
    </div>
    {!stopped && !micHidden && <MicNote report={mic.report} done={toast} onHide={() => setMicHidden(true)} className="micnote recnote"/>}
    {micMenu && mic.report && <MicMenu report={mic.report} recording done={toast} onClose={() => setMicMenu(null)} style={{ position: "fixed", left: micMenu.left, top: micMenu.top, right: "auto" }}/>}
    <div className="tabs" role="tablist" aria-label={s.meetingViews}>
      {(["notes", "live"] as const).map(key => <button key={key} type="button" role="tab" aria-selected={tab === key} onClick={() => { void flush.current(); setTab(key); }}>{key === "notes" ? "My notes" : "Live transcript"}</button>)}
    </div>
    <div ref={box} className="recbody" role="tabpanel" onScroll={follow.onScroll}>
      {tab === "notes" ? <>
        <p className="tip">Each line keeps the time you wrote it. Start a line with <code>[]</code> action, <code>!</code> decision, <code>?</code> question or <code>#</code> topic. The companion shows the same notes.</p>
        <MyNotes key={`${id}-${notesKey}`} meetingId={id} elapsed={state.elapsed_ms} registerFlush={f => { flush.current = f; }} hints={false}/>
      </> : <>
        <div className="lines reclines" aria-live="polite">
          {liveError && <p className="tip" role="status">{liveError}</p>}
          {lines.length ? lines.flatMap((line, i) => [...gapRows(i), <button className="line" key={i} onClick={() => act(async () => { await flush.current(); await ipc.addJot(`> ${line.text} [${line.time}]`); setNotesKey(n => n + 1); setTab("notes"); })}>
            <span className="ts">{line.time}</span><span className={`who ${line.speaker === "You" ? "me" : "them"}`}>{line.speaker}</span><span className="tx">{line.text}</span></button>])
            .concat(gapRows(-1), state.state.type === "Recording" && !liveError ? [<div className="line pending" key="pending" aria-hidden="true"><span className="ts"><span>·</span><span>·</span><span>:</span><span>·</span><span>·</span></span><span className="who them dots"><i>.</i><i>.</i><i>.</i></span><span className="tx">listening</span></div>] : [])
            : !liveError && <span className="tip">Listening… lines appear a few seconds after people speak.</span>}
        </div>
        <p className="tip">Click a line to quote it into My notes.</p>
        <JumpToNow show={follow.behind} onJump={follow.jump}/>
      </>}
    </div>
    <p className="tip recfoot">{stopped ? "Resume keeps it one meeting." : paused ? "Paused time isn't recorded or transcribed. Motetto reminds you if a pause runs past 15 minutes." : "After you stop, the transcript and notes are written on this computer. You can close this window; recording continues."}</p>
  </>;
}

// The steps page (mockup procHtml): each step is [label, detail]; `at` is the one being done,
// `bar` the progress width (%) for each step. `began` reads when each step started (stageClock), on
// every tick since the run's start is recorded just after the page first draws: a
// step without a detail shows how long it took, and the one being done counts up (owner 2026-10-25).
// `stop` is the meeting being processed and what Stop keeps (owner 2026-10-25, mockup stopJob): the
// model stops at once and nothing is lost, so there is no question; the page greys while it stops.
function StepsPage({ title, steps, at, bar, tip, began, stop }: { title: string; steps: [string, string][]; at: number; bar: number[]; tip: string; began: () => (number | undefined)[]; stop: { id: string; hint: string } }) {
  const [now, setNow] = useState(Date.now);
  const [stopping, setStopping] = useState(false);
  useEffect(() => { const tick = setInterval(() => setNow(Date.now()), 1000); return () => clearInterval(tick); }, []);
  const times = began();
  const took = (i: number) => {
    const from = times[i], to = i < at ? times[i + 1] : i === at ? Math.max(now, from ?? 0) : undefined;
    return from !== undefined && to !== undefined ? elapsedLabel(to - from) : "";
  };
  return <div className={`writing${stopping ? " stopping" : ""}`} role="status">
    <h4>{title}</h4>
    <div className="steps">{steps.map(([label, detail], i) => <div key={i} className={`step${i < at ? " done" : i === at ? " now" : ""}`}><span className="b"/><span>{label}</span><small>{detail || took(i)}</small></div>)}</div>
    <div className="procbar"><i style={{ width: `${bar[at]}%` }}/></div>
    <p className="tip">{tip}</p>
    <div className="proc-cancel"><button className="btn" disabled={stopping || !ipc.desktopAvailable()} onClick={() => { setStopping(true); ipc.stopProcessing(stop.id).catch(() => setStopping(false)); }}>{stopping ? s.stoppingJob : s.stopJob}</button><span className="hint">{stop.hint}</span></div>
  </div>;
}

// A long call is summarized in parts, then merged: say which, since each part takes minutes.
const summarizingLabel = (stage: Stage | null) =>
  stage?.type === "Summarizing" && stage.of > 1 ? `Summarizing part ${stage.part} of ${stage.of}` : stage?.type === "Merging" ? "Summarizing: merging the parts" : "Summarizing";

// After Stop: honest steps while the local models write the notes.
export function ProcessingPage({ id, stage, durationMs }: { id: string; stage: Stage | null; durationMs: number }) {
  const at = stage?.type === "Saving" ? 3 : stage?.type === "Summarizing" || stage?.type === "Merging" ? 2 : 1;
  return <StepsPage title="Writing the notes" at={at} bar={[0, 35, 65, 90]} began={() => { const t = stageTimes(id); return [undefined, t.start, t.Summarizing ?? t.Merging, t.Saving]; }} tip="You can close this window. A notification arrives when the notes are ready." stop={{ id, hint: s.stopAfterCallHint }}
    steps={[["Recorded both sides", durationMs ? elapsedLabel(durationMs) : ""], ["Transcribed", ""], [summarizingLabel(stage), ""], ["Saving", ""]]}/>;
}

// Write the notes again (owner 2026-10-21): only the steps a rewrite does. A meeting with a
// transcript starts at Summarizing; one without (Recover) transcribes first.
export function RewritingPage({ id, stage, lines }: { id: string; stage: Stage | null; lines: number }) {
  const at = stage?.type === "Saving" ? 2 : stage?.type === "Summarizing" || stage?.type === "Merging" || (lines && !stage) ? 1 : 0;
  const first = lines ? "Transcript ready" : stage?.type === "Transcribing" ? "Transcribing the saved audio" : "Transcribed";
  // With a transcript, summarizing starts with the run; without, it waits for the transcription.
  const began = () => { const t = stageTimes(id); return [lines ? undefined : t.start, lines ? t.start : t.Summarizing ?? t.Merging, t.Saving]; };
  return <StepsPage title={s.rewriting} at={at} bar={[20, 55, 90]} tip={s.rewritingTip} began={began} stop={{ id, hint: s.stopNotesHint }}
    steps={[[first, lines ? `${lines} ${lines === 1 ? "line" : "lines"}` : ""], [summarizingLabel(stage), ""], ["Saving", ""]]}/>;
}

// Transcribe again (owner 2026-10-21): whisper reads the saved audio, then the transcript is saved.
export function RetranscribingPage({ id, stage }: { id: string; stage: Stage | null }) {
  return <StepsPage title={s.retranscribing} at={stage?.type === "Saving" ? 1 : 0} bar={[35, 90]} tip={s.retranscribingTip} began={() => { const t = stageTimes(id); return [t.start, t.Saving]; }} stop={{ id, hint: s.stopTranscriptHint }}
    steps={[["Transcribing the saved audio", ""], ["Saving", ""]]}/>;
}
