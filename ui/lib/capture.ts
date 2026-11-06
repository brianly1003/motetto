// The screen setup's choices (mockup "Screen capture picker"): pure helpers shared by the overlay
// page on every display. Points are the overlay page's CSS pixels; the bar shows pixels.
import type { CaptureMemory } from "./bindings/CaptureMemory";
import type { CaptureWindow } from "./bindings/CaptureWindow";
import type { MicChoice } from "./bindings/MicChoice";
import type { RecordSetup } from "./bindings/RecordSetup";
import type { ScreenPick } from "./bindings/ScreenPick";

export type Mode = "audio" | "display" | "window" | "area";
export const MODES: Mode[] = ["audio", "display", "window", "area"];
export const TIMERS = [0, 3, 5, 10];
/** `display` null: the main display (the one holding the bar). */
export type Area = { display: number | null; x: number; y: number; w: number; h: number };
export type Screen = { id: number; name: string; w: number; h: number };
/** A countdown: it ends at `until` (ms), and the overlay of display `by` starts the recording. */
export type Count = { total: number; until: number; by: number };
/** One setup for every display: each overlay sends its changes to the others. */
export type Cap = {
  mode: Mode; mic: MicChoice; sys: boolean; notes: boolean; timer: number; clicks: boolean; remember: boolean;
  area: Area; sel: CaptureWindow | null; screen: Screen | null; count: Count | null;
};

/** The remembered setup; during a recording (`live`) there is no Audio only. */
export function fromMemory(memory: CaptureMemory, mode: string | null, live: boolean, view: { w: number; h: number }): Cap {
  let next = (MODES as string[]).includes(mode ?? "") ? mode as Mode : (MODES as string[]).includes(memory.mode) ? memory.mode as Mode : "window";
  if (live && next === "audio") next = "window";
  const a = memory.area;
  return {
    mode: next, mic: memory.mic, sys: hears(memory.mic, memory.app_sounds, next === "audio" || memory.notes), notes: next === "audio" || memory.notes,
    timer: TIMERS.includes(memory.timer) ? memory.timer : 0, clicks: memory.clicks, remember: memory.remember,
    area: a ? { display: null, x: a.x, y: a.y, w: a.width, h: a.height } : { display: null, x: Math.round(view.w / 2 - 260), y: 150, w: 520, h: 320 },
    sel: null, screen: null, count: null,
  };
}

/** One menu choice ("mic", "sys", "notes:1", "timer:5", ...). Notes need something to hear: asking
 *  for them with no mic and no app sounds turns app sounds back on. */
export function capSet(cap: Cap, key: string, mic?: MicChoice): Cap {
  const [k, val] = key.split(":");
  const next = { ...cap };
  if (k === "mic" && mic) next.mic = mic;
  if (k === "sys") next.sys = !cap.sys;
  if (k === "notes") next.notes = val === "1";
  if (k === "timer") next.timer = Number(val);
  if (k === "clicks") next.clicks = !cap.clicks;
  if (k === "remember") next.remember = !cap.remember;
  if (next.notes && next.mic.kind === "Off" && !next.sys) next.sys = true;
  return next;
}

/** Notes need something to hear: with no mic, app sounds are on whenever notes are. */
const hears = (mic: MicChoice, sys: boolean, notes: boolean) => sys || (notes && mic.kind === "Off");

/** A new mode drops the chosen window; Audio only always writes notes, so it needs sound. */
export const capMode = (cap: Cap, mode: Mode): Cap => {
  const notes = mode === "audio" ? true : cap.notes;
  return { ...cap, mode, sel: null, notes, sys: hears(cap.mic, cap.sys, notes) };
};

/** The front-most window under a point on this display. */
export const windowAt = (windows: CaptureWindow[], display: number, x: number, y: number) =>
  windows.find(w => w.display === display && x >= w.x && x < w.x + w.width && y >= w.y && y < w.y + w.height) ?? null;

export type Drag = { h: string; x: number; y: number; from: Area };
/** The area while dragging: a new one, moved, or resized by one of its 8 handles. */
export function dragArea(d: Drag, px: number, py: number, bounds: { w: number; h: number }, display: number): Area {
  const x = Math.max(0, Math.min(bounds.w, px)), y = Math.max(0, Math.min(bounds.h, py));
  let { x: ax, y: ay, w, h } = d.from;
  if (d.h === "move") { ax = Math.max(0, Math.min(bounds.w - w, ax + x - d.x)); ay = Math.max(0, Math.min(bounds.h - h, ay + y - d.y)); }
  else if (d.h === "new") { ax = Math.min(d.x, x); ay = Math.min(d.y, y); w = Math.abs(x - d.x); h = Math.abs(y - d.y); }
  else {
    if (d.h.includes("w")) { const r = ax + w; ax = Math.min(x, r - 40); w = r - ax; }
    if (d.h.includes("e")) w = Math.max(40, x - ax);
    if (d.h.includes("n")) { const b = ay + h; ay = Math.min(y, b - 30); h = b - ay; }
    if (d.h.includes("s")) h = Math.max(30, y - ay);
  }
  return { display: d.h === "new" ? display : d.from.display, x: Math.round(ax), y: Math.round(ay), w: Math.round(w), h: Math.round(h) };
}

/** Arrow keys nudge the area by a point (10 with Shift), inside the display. */
export function nudge(a: Area, key: string, shift: boolean, bounds: { w: number; h: number }): Area {
  const step = shift ? 10 : 1;
  const dx = key === "ArrowLeft" ? -step : key === "ArrowRight" ? step : 0, dy = key === "ArrowUp" ? -step : key === "ArrowDown" ? step : 0;
  return { ...a, x: Math.max(0, Math.min(bounds.w - a.w, a.x + dx)), y: Math.max(0, Math.min(bounds.h - a.h, a.y + dy)) };
}

const px = (n: number, scale: number) => Math.round(n * scale);
export const sizeOf = (w: number, h: number, scale: number, gap = "×") => `${px(w, scale)}${gap}${px(h, scale)}`;

/** What Record will record, as the pick Rust takes; null for Audio only. */
export function screenPick(cap: Cap, main: number, scale: number, mainScreen: Screen): ScreenPick | null {
  if (cap.mode === "display") { const s = cap.screen ?? mainScreen; return { choice: { kind: "Display", id: s.id }, name: s.name }; }
  if (cap.mode === "window" && cap.sel) return { choice: { kind: "Window", window_id: cap.sel.id, app: cap.sel.app }, name: `${cap.sel.title || cap.sel.app} window` };
  if (cap.mode === "area") { const a = cap.area; return { choice: { kind: "Area", display: a.display ?? main, x: a.x, y: a.y, width: a.w, height: a.h }, name: `Area ${sizeOf(a.w, a.h, scale)}` }; }
  return null;
}

export const recordSetup = (cap: Cap, screen: ScreenPick | null): RecordSetup => ({ mic: cap.mic, app_sounds: cap.sys, notes: cap.notes, clicks: cap.clicks, screen });

export const memoryOf = (cap: Cap): CaptureMemory => ({
  mode: cap.mode, mic: cap.mic, app_sounds: cap.sys, notes: cap.notes, timer: cap.timer, clicks: cap.clicks,
  area: { x: cap.area.x, y: cap.area.y, width: cap.area.w, height: cap.area.h }, remember: cap.remember,
});

/** Seconds left in a countdown at `now`. */
export const countLeft = (count: Count, now: number) => Math.max(0, Math.ceil((count.until - now) / 1000));
