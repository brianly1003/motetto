"use client";
// Trim (mockup trimHtml, owner 2026-11-05): an amber window over a filmstrip of the recording;
// drag either edge or nudge it with the arrow keys, or take the chip that cuts the reach for
// Stop. The kept part is at least 1 s. Return trims, Esc cancels.
import { useEffect, useRef, useState, type CSSProperties } from "react";
import type { MeetingMedia } from "@/lib/bindings/MeetingMedia";
import { partAt } from "@/lib/screen";
import { Thumbnail } from "./VideoTab";
import { Kbd } from "@/components/ui/Kbd";

const svg = (d: React.ReactNode) => <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{d}</svg>;
export const CutIcon = () => svg(<><circle cx="4.5" cy="4.5" r="2"/><circle cx="4.5" cy="11.5" r="2"/><path d="M6.2 5.6L13.5 12M6.2 10.4L13.5 4"/></>);

/** "mm:ss.t", as the handles and the status row show times. */
export const mmss = (s: number) => `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(Math.floor(s % 60)).padStart(2, "0")}`;
export const tfmt = (s: number) => `${mmss(s)}.${Math.floor((s % 1) * 10 + 1e-6)}`;
const TILES = 12;

export default function TrimStrip({ media, reachMs, onCancel, onSave }: { media: MeetingMedia; reachMs: number | null; onCancel: () => void; onSave: (fromMs: number, toMs: number) => void }) {
  const T = media.duration_ms / 1000;
  // The reach for Stop, in seconds, only when it leaves at least 1 s to keep.
  const reach = reachMs && reachMs > 0 && reachMs < media.duration_ms - 1000 ? +(reachMs / 1000).toFixed(1) : 0;
  const [range, setRange] = useState({ a: 0, b: T });
  const [drag, setDrag] = useState<"a" | "b" | null>(null);
  const strip = useRef<HTMLDivElement>(null);
  const handleB = useRef<HTMLSpanElement>(null);
  const bar = useRef<HTMLDivElement>(null);
  const latest = useRef(range); latest.current = range;
  useEffect(() => { bar.current?.scrollIntoView({ block: "nearest" }); handleB.current?.focus({ preventScroll: true }); }, []);
  const set = (side: "a" | "b", v: number) => setRange(r => side === "a" ? { ...r, a: Math.max(0, Math.min(v, r.b - 1)) } : { ...r, b: Math.min(T, Math.max(v, r.a + 1)) });
  const changed = range.a > 0 || range.b < T;
  const save = () => { const r = latest.current; if (r.a > 0 || r.b < T) onSave(Math.round(r.a * 1000), Math.round(r.b * 1000)); };
  // Keys work wherever focus is on the page, as in the mockup: Esc cancels, Return trims from a
  // handle or from nowhere in particular.
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      const side = (document.activeElement as HTMLElement | null)?.dataset?.th as "a" | "b" | undefined;
      if (side && (e.key === "ArrowLeft" || e.key === "ArrowRight")) { e.preventDefault(); const r = latest.current; set(side, +(r[side] + (e.key === "ArrowLeft" ? -1 : 1) * (e.shiftKey ? 1 : 0.1)).toFixed(1)); }
      else if (e.key === "Escape") {
        // Another menu or popover that is open closes on its own Esc first.
        const other = document.querySelector('[role="menu"], [role="listbox"], [role="alertdialog"]');
        if (other && !bar.current?.contains(document.activeElement)) return;
        e.preventDefault(); e.stopPropagation(); onCancel();
      }
      else if (e.key === "Enter" && (side || document.activeElement === document.body)) { e.preventDefault(); save(); }
    };
    document.addEventListener("keydown", key, true);
    return () => document.removeEventListener("keydown", key, true);
  });
  function grab(e: React.PointerEvent<HTMLSpanElement>, side: "a" | "b") {
    e.preventDefault(); e.currentTarget.focus(); setDrag(side);
    const move = (ev: PointerEvent) => { const r = strip.current?.getBoundingClientRect(); if (!r) return; set(side, +(Math.min(1, Math.max(0, (ev.clientX - r.left) / r.width)) * T).toFixed(1)); };
    const up = () => { removeEventListener("pointermove", move); removeEventListener("pointerup", up); setDrag(null); };
    addEventListener("pointermove", move); addEventListener("pointerup", up);
  }
  const a = (range.a / T) * 100, b = (range.b / T) * 100, tip = reach > 0 && range.b > T - reach;
  const th = (side: "a" | "b", v: number) => <span ref={side === "b" ? handleB : undefined} className={`th ${side}${drag === side ? " drag" : ""}`} role="slider" tabIndex={0} data-th={side} aria-label={`${side === "a" ? "Start" : "End"} of the video`} aria-valuemin={0} aria-valuemax={T} aria-valuenow={+v.toFixed(1)} aria-valuetext={tfmt(v)} onPointerDown={e => grab(e, side)}><b>{tfmt(v)}</b></span>;
  // Real frames from the recording, one per twelfth.
  const tiles = Array.from({ length: TILES }, (_, i) => { const at = partAt(media.videos, ((i + 0.5) / TILES) * media.duration_ms); return <i key={i} style={{ "--k": i } as CSSProperties}>{at && <Thumbnail part={at.part} ms={at.fileMs}/>}</i>; });
  return <div ref={bar} className="trimbar" role="group" aria-label="Trim the video">
    <div className="tstrip" id="tstrip" ref={strip}><div className="tfilm">{tiles}</div>
      <span className="tdim" style={{ left: 0, width: `${a}%` }}/><span className="tdim" style={{ left: `${b}%`, right: 0 }}/>
      {reach > 0 && <span className="treach" style={{ left: `${((T - reach) / T) * 100}%` }} title="The pointer on its way to Stop"/>}
      <div className="twin" style={{ left: `${a}%`, width: `${b - a}%` }}>{th("a", range.a)}{th("b", range.b)}</div></div>
    <div className="trow"><span className="mono">Keeps <b>{tfmt(range.b - range.a)}</b> of {tfmt(T)}</span>
      {tip && <button type="button" className="tsug" onClick={() => set("b", +(T - reach).toFixed(1))}><CutIcon/>Cut the last {reach} s · reaching for Stop</button>}
      <span className="sp"/><span className="tkeys"><Kbd>←</Kbd><Kbd>→</Kbd> nudge</span>
      <button type="button" className="btn sm ghost" onClick={onCancel}>Cancel</button><button type="button" className="btn sm primary" disabled={!changed} onClick={save}>Trim</button></div>
  </div>;
}
