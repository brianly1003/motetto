"use client";
// What a rehearsal measured (SPEC 14.6, mockup Script screen › report): counted from the whole
// rehearsal's transcript; only the shorter lines come from the notes model, as suggestions.
import { useEffect, useRef, useState, type CSSProperties } from "react";
import * as ipc from "@/lib/ipc";
import { strings as s } from "@/lib/strings";
import { follow, parseScript } from "@/lib/script";
import { report as measure, type Report } from "@/lib/rehearsal";
import type { Rehearsal } from "@/lib/bindings/Rehearsal";

const fmt = (sec: number) => `${Math.floor(sec / 60)}:${String(Math.round(sec % 60)).padStart(2, "0")}`;
const clock = (ms: number) => `${String(Math.floor(ms / 60000)).padStart(2, "0")}:${String(Math.floor(ms / 1000) % 60).padStart(2, "0")}`;
// Earlier totals of each script in this session, for "vs rehearsal N".
const history = new Map<string, number[]>();

// Listen back: native on macOS (the key `rehearsal:<id>`), an audio element elsewhere.
function Player({ id, durationMs }: { id: string; durationMs: number }) {
  const key = `rehearsal:${id}`;
  const [src, setSrc] = useState<string | null | undefined>(undefined);
  const [playing, setPlaying] = useState(false);
  const [pos, setPos] = useState(0);
  const at = useRef(0);
  const seek = useRef(true);
  const audio = useRef<HTMLAudioElement>(null);
  useEffect(() => {
    let active = true;
    ipc.rehearsalAudio(id).then(value => { if (active) setSrc(value); }).catch(() => {});
    return () => { active = false; void ipc.playbackClose(key).catch(() => {}); };
  }, [id]);
  useEffect(() => {
    if (!playing || src !== null) return;
    let active = true, asking = false;
    seek.current = true;
    const tick = () => {
      if (asking) return; asking = true;
      const jump = seek.current; seek.current = false;
      (jump ? ipc.playbackSet(key, true, at.current, 1) : ipc.playbackPosition(key)).then(ms => {
        if (!active || seek.current) return;
        const t = Math.min(durationMs, ms); at.current = t; setPos(t);
        if (t >= durationMs) { at.current = 0; setPlaying(false); }
      }).catch(() => active && setPlaying(false)).finally(() => { asking = false; });
    };
    tick(); const timer = setInterval(tick, 100);
    return () => { active = false; clearInterval(timer); void ipc.playbackSet(key, false, null, 1).catch(() => {}); };
  }, [playing, src]);
  if (src === undefined || durationMs <= 0) return null;
  function toggle() {
    const el = audio.current;
    if (el) { if (playing) el.pause(); else void el.play().catch(() => setPlaying(false)); }
    setPlaying(!playing);
  }
  function jump(t: number) {
    at.current = t; setPos(t); seek.current = true;
    if (audio.current) audio.current.currentTime = t / 1000;
  }
  return <div className="player">
    <button type="button" className="btn sm" onClick={toggle}>{playing ? s.rehearsalPause : s.rehearsalPlay}</button>
    <span className="mono">{clock(pos)} / {clock(durationMs)}</span>
    <input type="range" className="bar" aria-label={s.rehearsalSeek} min={0} max={durationMs} step={100} value={pos} style={{ "--p": `${pos / durationMs * 100}%` } as CSSProperties} onChange={e => jump(+e.currentTarget.value)} />
    <span className="tip">{s.rehearsalYourMic}</span>
    {src && <audio ref={audio} src={src} preload="metadata" onTimeUpdate={e => setPos(e.currentTarget.currentTime * 1000)} onEnded={() => { setPlaying(false); setPos(0); }} />}
  </div>;
}

export default function RehearsalReport({ rehearsal, text, onUse, onAgain, onEdit, onError, device }: {
  rehearsal: Rehearsal; text: string; device: string;
  onUse: (line: number, to: string) => Promise<() => Promise<void>>; onAgain: () => void; onEdit: () => void; onError: (message: string) => void;
}) {
  const [result, setResult] = useState<Report | null>(null);
  const [failed, setFailed] = useState("");
  const [shorter, setShorter] = useState<Record<number, string | null>>({});
  const [hidden, setHidden] = useState<Set<number>>(new Set());
  // Used lines, each with its Undo.
  const [used, setUsed] = useState<Map<number, () => Promise<void>>>(new Map());
  const [kept, setKept] = useState(false);
  const keptRef = useRef(false);
  const script = parseScript(text);
  const earlier = history.get(rehearsal.script) ?? [];
  useEffect(() => {
    let active = true;
    ipc.rehearsalTranscript(rehearsal.id).then(transcript => {
      if (!active) return;
      const r = measure(script, transcript, rehearsal.duration_ms ?? 0, follow);
      setResult(r);
      history.set(rehearsal.script, [...earlier, r.totalS]);
      // One line at a time, so the notes model never runs five questions at once.
      void (async () => {
        for (const k of r.longLines) {
          const answer = await ipc.rehearsalShorten(script.lines[k].text.replace(/\[[^\]]*\]/g, "").trim()).catch(() => null);
          if (!active) return;
          setShorter(v => ({ ...v, [k]: answer }));
        }
      })();
    }).catch(e => active && setFailed(String(e)));
    // Leaving the report deletes the audio unless it was kept.
    return () => { active = false; if (!keptRef.current) void ipc.rehearsalDiscard(rehearsal.id).catch(() => {}); };
  }, [rehearsal.id]);
  async function keep() {
    // Set first: leaving while Keep runs must not delete what is being kept.
    keptRef.current = true;
    try { await ipc.rehearsalKeep(rehearsal.id, rehearsal.script); setKept(true); } catch (e) { keptRef.current = false; onError(String(e)); }
  }
  const title = s.rehearsalTitle.replace("{n}", String(earlier.length + (result ? 0 : 1))).replace("{script}", rehearsal.script);
  if (failed) return <div className="scripts-pane"><div className="scripts-bar"><h2 className="scripts-heading">{title}</h2><button type="button" className="btn" onClick={onEdit}>{s.rehearsalEdit}</button></div><p className="scripts-hint" role="alert">{s.rehearsalFailed.replace("{error}", failed)}</p></div>;
  if (!result) return <div className="scripts-pane"><div className="scripts-bar"><h2 className="scripts-heading">{title}</h2></div><p className="scripts-hint" role="status" aria-busy="true">{s.rehearsalWriting}</p></div>;
  const planned = script.sections.reduce((sum, x) => sum + x.minutes * 60, 0);
  const previous = earlier.at(-2);
  const fillers = Object.entries(result.fillers);
  const fillerTotal = fillers.reduce((sum, [, n]) => sum + n, 0);
  const scale = Math.max(1, ...result.perSection.flatMap(x => [x.plannedS, x.actualS]));
  const sectionOf = (line: number) => script.sections[script.lines[line]?.section ?? 0];
  const items = [
    ...result.longLines.filter(k => !hidden.has(k)).map(k => <li key={`l${k}`}><span className="pttag">{s.rehearsalRanLong}</span><div><q>{script.lines[k].text}</q>
      {used.has(k) ? <div className="sugg">{s.rehearsalUsed} <button type="button" className="btn sm ghost" onClick={() => void used.get(k)!().then(() => setUsed(v => { const next = new Map(v); next.delete(k); return next; })).catch(() => {})}>{s.undo}</button></div> : shorter[k] ? <><div className="sugg">{s.rehearsalShorter} <b>{shorter[k]}</b></div>
        <div className="acts2"><button type="button" className="btn sm" onClick={() => void onUse(k, shorter[k]!).then(undo => setUsed(v => new Map(v).set(k, undo))).catch(() => {})}>{s.rehearsalUse}</button><button type="button" className="btn sm ghost" onClick={() => setHidden(v => new Set(v).add(k))}>{s.rehearsalKeepMine}</button></div></> : null}</div></li>),
    ...result.skipped.map(k => <li key={`s${k}`}><span className="pttag">{s.rehearsalSkipped}</span><div><q>{script.lines[k].text}</q> {s.rehearsalSkippedAt.replace("{section}", String(script.lines[k].section + 1)).replace("{line}", String(k - (sectionOf(k)?.first ?? 0) + 1))}</div></li>),
    ...result.offScript.map((o, n) => <li key={`o${n}`}><span className="pttag">{s.rehearsalOffScript}</span><div>{s.rehearsalOffAt.replace("{secs}", String(Math.round((o.endMs - o.startMs) / 1000))).replace("{at}", clock(o.startMs)).replace("{section}", sectionOf(o.before)?.title ?? "")} {script.lines[o.before] && <q>{script.lines[o.before].text}</q>}</div></li>),
    ...(fillerTotal ? [<li key="f"><span className="pttag">{s.rehearsalFillers}</span><div>{fillers.map(([w, n]) => `"${w}" ${n}`).join(", ")}.</div></li>] : []),
  ];
  return <div className="scripts-pane">
    <div className="scripts-bar"><h2 className="scripts-heading">{title}</h2><button type="button" className="btn" onClick={onEdit}>{s.rehearsalEdit}</button><button type="button" className="btn primary" onClick={onAgain}>{s.rehearsalAgain}</button></div>
    <div className="ptrep">
      <div className="rh"><div className="big">{fmt(result.totalS)} <span>{s.rehearsalPlanned.replace("{planned}", fmt(planned))}</span></div>
        <div className="kp"><div><b>{result.wpm}</b><span>{s.rehearsalWpm}</span></div><div><b>{result.skipped.length}</b><span>{s.rehearsalSkippedN}</span></div><div><b>{fillerTotal}</b><span>{s.rehearsalFillersN}</span></div>
          {previous !== undefined && <div className={result.totalS <= previous ? "good" : ""}><b>{result.totalS <= previous ? "-" : "+"}{fmt(Math.abs(result.totalS - previous))}</b><span>{s.rehearsalVsLast.replace("{n}", String(earlier.length - 1))}</span></div>}</div></div>
      <Player id={rehearsal.id} durationMs={rehearsal.duration_ms ?? 0} />
      <div className="lab">{s.rehearsalBySection}</div>
      {result.perSection.map((x, n) => <div className="sec" key={n}><span>{`${n + 1} ${x.title}`}</span><div className="trk"><i style={{ width: `${x.plannedS / scale * 100}%` }} /><em className={x.actualS > x.plannedS ? "over" : ""} style={{ width: `${x.actualS / scale * 100}%` }} /></div><span className={`t${x.actualS > x.plannedS ? " over" : ""}`}>{fmt(x.actualS)} / {fmt(x.plannedS)}</span></div>)}
      <div className="lab">{s.rehearsalWorth}</div>
      {items.length ? <ul>{items}</ul> : <p className="scripts-hint">{s.rehearsalNothing}</p>}
      <div className="scripts-bar"><button type="button" className="btn sm" disabled={kept} onClick={() => void keep()}>{kept ? s.rehearsalKept : s.rehearsalKeep}</button><span className="scripts-hint">{s.rehearsalFoot.replace("{device}", device)}</span></div>
    </div>
  </div>;
}
