"use client";
// Save a clip (SPEC 15, DESIGN 4.20, mockup clipEdHtml): a range of one track saved as its own WAV
// with the recorded samples untouched, plus its words. It takes the Duet timeline's place; Trim's
// window and handles over the track's sound, a close-up with the pauses, and Play/Loop on a preview
// the app writes, so what you hear is exactly what Save writes.
import { useEffect, useRef, useState, type CSSProperties } from "react";
import type { MeetingMedia } from "@/lib/bindings/MeetingMedia";
import type { ClipPreview } from "@/lib/bindings/ClipPreview";
import * as ipc from "@/lib/ipc";
import { strings as s } from "@/lib/strings";
import { clipFileName, clippedChip, lengthChip, peakLabel, rateChip, snap, wordsInRange, type ClipTrack, type Line } from "@/lib/clip";
import { CutIcon, tfmt } from "./TrimStrip";
import { MenuIcon } from "./MenuIcon";

export type ClipRange = { from: number; to: number; track: ClipTrack };
const ZOOM_BINS = 140, ZOOM_PAD = 2000;
const PLAY = <svg viewBox="0 0 16 16" fill="currentColor" aria-hidden="true"><path d="M4.5 2.8v10.4a.6.6 0 0 0 .9.5l8-5.2a.6.6 0 0 0 0-1l-8-5.2a.6.6 0 0 0-.9.5z"/></svg>;
const STOP = <svg viewBox="0 0 16 16" fill="currentColor" aria-hidden="true"><rect x="4" y="3" width="2.6" height="10" rx="1"/><rect x="9.4" y="3" width="2.6" height="10" rx="1"/></svg>;

export default function ClipPanel({ id, title, media, lines, clip, onChange, onClose, onToast, onError }: {
  id: string; title: string; media: MeetingMedia; lines: Line[]; clip: ClipRange;
  onChange: (clip: ClipRange) => void; onClose: () => void;
  onToast?: (message: string, action?: () => Promise<void>, label?: string) => void; onError: (message: string) => void;
}) {
  const total = media.duration_ms, { from, to, track } = clip, length = (to - from) / 1000;
  // The close-up's levels with the window they were read for: while a handle moves the window
  // moves too, and snapping must read the levels at their own times, not the new window's.
  const [read, setRead] = useState<{ z0: number; z1: number; levels: number[] }>({ z0: 0, z1: 0, levels: [] }), levels = read.levels;
  // The last measured facts stay on screen while a new preview is made; only Play waits for it.
  const [preview, setPreview] = useState<{ facts: ClipPreview; src: string } | null>(null), [fresh, setFresh] = useState(false);
  const [playing, setPlaying] = useState(false), [loop, setLoop] = useState(false), [words, setWords] = useState(true);
  const [at, setAt] = useState<number | null>(null), [drag, setDrag] = useState<"a" | "b" | null>(null), [saving, setSaving] = useState(false);
  const strip = useRef<HTMLDivElement>(null), audio = useRef<HTMLAudioElement>(null), handleB = useRef<HTMLSpanElement>(null), bar = useRef<HTMLDivElement>(null);
  const latest = useRef(clip); latest.current = clip;
  // The edge the arrow keys move: the one last dragged, clicked or focused; the end at first.
  const [edge, setEdge] = useState<"a" | "b">("b");
  const z0 = Math.max(0, from - ZOOM_PAD), z1 = Math.min(total, to + ZOOM_PAD);
  const zoom = useRef(read); zoom.current = read;

  // The close-up and the preview follow the range once it stops moving.
  useEffect(() => {
    let live = true;
    const t = setTimeout(() => { ipc.clipLevels(id, track, z0, z1, ZOOM_BINS).then(v => { if (live) setRead({ z0, z1, levels: v }); }).catch(() => {}); }, 120);
    return () => { live = false; clearTimeout(t); };
  }, [id, track, z0, z1]);
  useEffect(() => {
    let live = true;
    stop(); setFresh(false);
    const t = setTimeout(async () => {
      try { const facts = await ipc.clipPreview(id, track, from, to); const src = await ipc.clipAudioSrc(facts.path); if (live) { setPreview({ facts, src }); setFresh(true); } }
      catch (error) { if (live) onError(String(error)); }
    }, 250);
    return () => { live = false; clearTimeout(t); };
  }, [id, track, from, to]);
  useEffect(() => { handleB.current?.focus({ preventScroll: true }); bar.current?.scrollIntoView({ block: "nearest" }); return () => { void ipc.clipPreviewClear().catch(() => {}); }; }, []);
  useEffect(() => { if (audio.current) audio.current.loop = loop; }, [loop]);

  function stop() { audio.current?.pause(); setPlaying(false); setAt(null); }
  function toggle() {
    const el = audio.current;
    if (!el || !fresh) return;
    if (playing) { stop(); return; }
    el.currentTime = 0; el.loop = loop; void el.play(); setPlaying(true);
  }
  // The play position, read from the element while it plays.
  useEffect(() => {
    if (!playing) return;
    let frame = 0;
    const tick = () => { const el = audio.current; if (el) setAt(latest.current.from + el.currentTime * 1000); frame = requestAnimationFrame(tick); };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [playing]);

  const set = (side: "a" | "b", ms: number) => {
    const c = latest.current, v = Math.round(Math.max(0, Math.min(total, ms)));
    onChange(side === "a" ? { ...c, from: Math.min(v, c.to - 500) } : { ...c, to: Math.max(v, c.from + 500) });
  };
  function grab(e: React.PointerEvent<HTMLSpanElement>, side: "a" | "b") {
    e.preventDefault(); e.currentTarget.focus(); setDrag(side); setEdge(side);
    let last = latest.current[side === "a" ? "from" : "to"], exact = e.altKey;
    const move = (ev: PointerEvent) => { const r = strip.current?.getBoundingClientRect(); if (!r) return; exact = ev.altKey; last = Math.min(1, Math.max(0, (ev.clientX - r.left) / r.width)) * total; set(side, last); };
    // A dropped handle lands on the nearest pause, unless Alt was held.
    const up = (ev: PointerEvent) => { removeEventListener("pointermove", move); removeEventListener("pointerup", up); setDrag(null); if (!exact && !ev.altKey) { const z = zoom.current; set(side, snap(z.levels, z.z0, z.z1, last)); } };
    addEventListener("pointermove", move); addEventListener("pointerup", up);
  }
  async function save() {
    if (saving) return;
    setSaving(true); stop();
    const c = latest.current, text = words ? wordsInRange(lines, c.from, c.to, c.track) : "";
    try {
      const path = await ipc.saveClip(id, c.track, c.from, c.to, text || null);
      if (path) {
        const file = path.split(/[\\/]/).pop() ?? path;
        onToast?.((text ? s.clipSaved : s.clipSavedAudio).replace("{file}", file), () => ipc.revealClip(), s.reveal);
        onClose();
      }
    } catch (error) { onError(String(error)); }
    finally { setSaving(false); }
  }
  // Keys work wherever focus is on the page, as Trim's do, except while typing in a line.
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      if (target?.isContentEditable || target?.closest?.("input:not([type=checkbox]), textarea, [role=menu], [role=alertdialog]")) return;
      // Arrows move the chosen edge wherever focus is (Play, Loop, a transcript time), not only on a handle.
      const side = ((document.activeElement as HTMLElement | null)?.dataset?.ch as "a" | "b" | undefined) ?? edge;
      if (side && (e.key === "ArrowLeft" || e.key === "ArrowRight")) { e.preventDefault(); e.stopPropagation(); const c = latest.current; set(side, (side === "a" ? c.from : c.to) + (e.key === "ArrowLeft" ? -1 : 1) * (e.shiftKey ? 1000 : 100)); }
      else if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); onClose(); }
      else if (e.key === "Enter" && !e.metaKey && !e.altKey) { e.preventDefault(); e.stopPropagation(); void save(); }
      else if (e.key === " ") { e.preventDefault(); e.stopPropagation(); toggle(); }
      else if (e.key.toLowerCase() === "l" && !e.metaKey && !e.ctrlKey) { e.preventDefault(); setLoop(on => !on); }
    };
    document.addEventListener("keydown", key, true);
    return () => document.removeEventListener("keydown", key, true);
  });

  const A = (from / total) * 100, B = (to / total) * 100;
  // The whole meeting's sound of the chosen track(s), mirrored around the middle.
  const envelope = (speaker: string) => media.tracks.filter(t => t.speaker === speaker).map(t => {
    const x = (ms: number) => (ms / total) * 1000, h = (v: number) => Math.sqrt(Math.min(1, v)) * 16;
    const pts = t.waveform.map((v, i) => [x(t.offset_ms + (i / t.waveform.length) * t.duration_ms), h(v)] as const);
    return `M${x(t.offset_ms)} 20 ` + pts.map(([px, py]) => `L${px.toFixed(1)} ${(20 - py).toFixed(1)}`).join(" ") + ` L${x(t.offset_ms + t.duration_ms)} 20 `
      + [...pts].reverse().map(([px, py]) => `L${px.toFixed(1)} ${(20 + py).toFixed(1)}`).join(" ") + " Z";
  }).join(" ");
  const zx = (ms: number) => (((ms - z0) / Math.max(1, z1 - z0)) * 1000).toFixed(1);
  const loudest = Math.max(0.0001, ...levels), per = (z1 - z0) / Math.max(1, levels.length);
  // Pauses: runs of bins under a fifth of the loudest; a snap point sits in the middle of each.
  const quiet: [number, number][] = [];
  levels.forEach((v, i) => { if (v < loudest * 0.2) { const last = quiet[quiet.length - 1]; if (last && last[1] === i) last[1] = i + 1; else quiet.push([i, i + 1]); } });
  const pauses = quiet.filter(([a, b]) => (b - a) * per >= 60);
  const th = (side: "a" | "b", ms: number) => <span ref={side === "b" ? handleB : undefined} className={`th ${side}${drag === side ? " drag" : ""}${edge === side ? " on" : ""}`} role="slider" tabIndex={0} data-ch={side} onFocus={() => setEdge(side)}
    aria-label={side === "a" ? "Start of the clip" : "End of the clip"} aria-valuemin={0} aria-valuemax={total / 1000} aria-valuenow={+(ms / 1000).toFixed(1)} aria-valuetext={tfmt(ms / 1000)}
    onPointerDown={e => grab(e, side)}><b>{tfmt(ms / 1000)}</b></span>;
  const text = wordsInRange(lines, from, to, track), who = track === "Me" ? "You" : track;
  const facts = preview?.facts, lc = lengthChip(length);
  const chips = facts ? [rateChip(facts.rate, facts.bits, facts.float), clippedChip(facts.clipped), { tone: "ok", label: peakLabel(facts.peak_db) }, lc] : [lc];

  return <div ref={bar} className="trimbar clipbar" role="group" aria-label={s.clipTitle} data-track={track}>
    <div className="chead"><h4><CutIcon/>{s.clipTitle}</h4>
      <div className="segm" role="radiogroup" aria-label="Track">{([["Me", "You"], ["Them", "Them"], ["Both", "Both"]] as const).map(([v, l]) =>
        <button key={v} type="button" role="radio" aria-checked={track === v} data-ctrack={v} onClick={() => onChange({ ...clip, track: v })}>{l}</button>)}</div>
      <span className="sp"/><span className="tkeys">{s.clipHint}</span></div>
    <div className="tstrip" ref={strip}><svg className="cwave" viewBox="0 0 1000 40" preserveAspectRatio="none" aria-hidden="true">
      {(track === "Me" || track === "Both") && <path className="w me" d={envelope("You")}/>}{(track === "Them" || track === "Both") && <path className="w them" d={envelope("Them")}/>}
      <line className="mid" x1="0" x2="1000" y1="20" y2="20"/></svg>
      <span className="tdim" style={{ left: 0, width: `${A}%` }}/><span className="tdim" style={{ left: `${B}%`, right: 0 }}/>
      {at !== null && <span className="cplay" style={{ left: `${(at / total) * 100}%` }}/>}
      <div className="twin" style={{ left: `${A}%`, width: `${B - A}%` } as CSSProperties}>{th("a", from)}{th("b", to)}</div></div>
    <div className="czoom" aria-label="Close-up of the clip: shaded parts are pauses, dashed lines are where a handle snaps"><svg viewBox="0 0 1000 46" preserveAspectRatio="none" aria-hidden="true">
      {pauses.map(([a, b]) => <rect key={`q${a}`} className="quiet" x={zx(z0 + a * per)} y="0" width={((b - a) / levels.length) * 1000} height="46"/>)}
      {levels.map((v, i) => { const t = z0 + (i + 0.5) * per, h = (v / loudest) * 40; return <rect key={i} className={`bar${t >= from && t <= to ? " in" : ""}`} x={((i / levels.length) * 1000).toFixed(1)} y={(23 - h / 2).toFixed(1)} width={(1000 / levels.length * 0.62).toFixed(1)} height={h.toFixed(1)}/>; })}
      {pauses.map(([a, b]) => { const x = zx(z0 + ((a + b) / 2) * per); return <line key={`s${a}`} className="snap" x1={x} x2={x} y1="0" y2="46"/>; })}
      <line className="edge" x1={zx(from)} x2={zx(from)} y1="0" y2="46"/><line className="edge" x1={zx(to)} x2={zx(to)} y1="0" y2="46"/>
      {at !== null && <line className="head" x1={zx(at)} x2={zx(at)} y1="0" y2="46"/>}</svg>
      <span className="zl">{tfmt(z0 / 1000)}</span><span className="zl zr">{tfmt(z1 / 1000)}</span></div>
    {text ? <p className="cwords" title={text}><span className="who">{who}</span><q>{text}</q></p> : <p className="cwords empty"><q>No words in this range</q></p>}
    <div className="cchips">{chips.map(c => <span key={c.label} className={`chip ${c.tone}`}>{c.label}</span>)}</div>
    {track !== "Me" && <p className="cnote"><MenuIcon name="lock"/><span>{track === "Them" ? s.clipOthers : s.clipOthersBoth}</span></p>}
    <div className="trow"><button type="button" className="btn sm" disabled={!fresh} onClick={toggle} title="Play the clip (Space)">{playing ? STOP : PLAY}{playing ? "Stop" : "Play"}</button>
      <button type="button" className="btn sm" aria-pressed={loop} onClick={() => setLoop(on => !on)} title="Play it again and again (L)"><MenuIcon name="again"/>Loop</button>
      <span className="mono"><b>{tfmt(from / 1000)} - {tfmt(to / 1000)}</b> · {length.toFixed(1)} s</span><span className="sp"/>
      <span className="tkeys"><kbd>←</kbd><kbd>→</kbd> {edge === "a" ? "move the start" : "move the end"} · <kbd>⌥</kbd>-drag exact</span></div>
    <div className="trow end"><span className="cfile" title={clipFileName(title, from)}>{clipFileName(title, from)} · {s.clipFormat.replace("{bits}", String(facts?.bits ?? 32)).replace("{kind}", facts && !facts.float ? "integer" : "float")}</span>
      <label className="cbx"><input type="checkbox" checked={words} onChange={e => setWords(e.target.checked)}/> {s.clipWords}</label>
      <button type="button" className="btn sm ghost" onClick={onClose}>{s.cancel}</button>
      <button type="button" className="btn sm primary" disabled={saving} onClick={() => void save()} title="Save clip (Return)">{s.clipSave}</button></div>
    {preview && <audio ref={audio} src={preview.src} preload="auto" onEnded={() => { if (!loop) stop(); }} hidden/>}
  </div>;
}
