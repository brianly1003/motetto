// Save a clip (SPEC 15): the pure rules behind the panel, kept apart so they're tested without a page.

export type ClipTrack = "Me" | "Them" | "Both";
export type Tone = "ok" | "amber" | "rec";
export type Line = { ms: number | null; speaker: string; text: string };

/** The transcript says You for the mic and Them or Speaker N for the system audio. */
export const lineTrack = (speaker: string): ClipTrack => speaker === "You" ? "Me" : "Them";
const onTrack = (line: Line, track: ClipTrack) => track === "Both" || lineTrack(line.speaker) === track;

/** Length against the 5-10 s most voice-cloning tools want; they cut longer samples short. */
export function lengthChip(seconds: number): { tone: Tone; label: string } {
  if (seconds < 3) return { tone: "rec", label: "Under 3 s" };
  if (seconds < 5) return { tone: "amber", label: "Under 5 s" };
  if (seconds <= 10) return { tone: "ok", label: "5-10 s · voice sample" };
  if (seconds <= 12) return { tone: "amber", label: "Over 10 s" };
  return { tone: "rec", label: "Over 12 s" };
}

/** Rate and format; under 24 kHz is a headset in call mode and sounds like a phone line. */
export function rateChip(rate: number, bits: number, float: boolean): { tone: Tone; label: string } {
  const khz = `${+(rate / 1000).toFixed(1)} kHz`;
  return rate < 24_000 ? { tone: "amber", label: `${khz} · sounds like a phone line` }
    : { tone: "ok", label: `${khz} · ${bits}-bit ${float ? "float" : "integer"}` };
}

export const clippedChip = (n: number): { tone: Tone; label: string } =>
  n ? { tone: "amber", label: `Clipped ${n} ${n === 1 ? "time" : "times"}` } : { tone: "ok", label: "No clipping" };

export const peakLabel = (db: number | null) => db === null ? "Silent" : `Peak ${db < 0 ? "−" : ""}${Math.abs(db).toFixed(1)} dB`;

/**
 * Where a dropped handle lands: the quietest 20 ms within `radiusMs` of `atMs`, read from the
 * close-up's levels (`levels` spread evenly over fromMs..toMs), when it is a real pause. Outside the
 * close-up, as is.
 */
export function snap(levels: number[], fromMs: number, toMs: number, atMs: number, radiusMs = 400): number {
  if (!levels.length || atMs < fromMs || atMs > toMs || toMs <= fromMs) return atMs;
  const per = (toMs - fromMs) / levels.length;
  const step = Math.max(1, Math.round(20 / per));
  let best = atMs, quiet = Infinity;
  for (let i = 0; i < levels.length; i += step) {
    const mid = fromMs + (i + step / 2) * per;
    if (Math.abs(mid - atMs) > radiusMs) continue;
    const level = Math.max(...levels.slice(i, i + step));
    if (level < quiet || (level === quiet && Math.abs(mid - atMs) < Math.abs(best - atMs))) { quiet = level; best = mid; }
  }
  // Only a real pause pulls: half as loud as where it was dropped, else the handle stays.
  const here = levels[Math.min(levels.length - 1, Math.floor((atMs - fromMs) / per))];
  return quiet <= here * 0.5 ? Math.round(best) : atMs;
}

/** A timed line runs to the next timed line (or the end of the recording). */
export function lineRange(lines: Line[], index: number, totalMs: number): { from: number; to: number } | null {
  const start = lines[index]?.ms;
  if (start === null || start === undefined) return null;
  const next = lines.slice(index + 1).find(line => line.ms !== null && line.ms > start)?.ms ?? totalMs;
  return { from: start, to: Math.min(next, totalMs) };
}

/** The words of the lines on `track` whose time falls in the range, joined by spaces. */
export function wordsInRange(lines: Line[], fromMs: number, toMs: number, track: ClipTrack): string {
  return lines.filter(line => line.ms !== null && line.ms >= fromMs && line.ms < toMs && onTrack(line, track))
    .map(line => line.text.trim()).filter(Boolean).join(" ");
}

/** True when a line overlaps the range and belongs to the track (it keeps full strength). */
export function lineInClip(lines: Line[], index: number, totalMs: number, fromMs: number, toMs: number, track: ClipTrack) {
  const r = lineRange(lines, index, totalMs);
  return !!r && onTrack(lines[index], track) && r.from < toMs && r.to > fromMs;
}

/** The panel opens on the first line of the track (the whole first 8 s when there is none). */
export function defaultRange(lines: Line[], track: ClipTrack, totalMs: number): { from: number; to: number } {
  const index = lines.findIndex(line => line.ms !== null && onTrack(line, track));
  const r = index < 0 ? null : lineRange(lines, index, totalMs);
  return r && r.to - r.from >= 500 ? r : { from: 0, to: Math.min(8000, totalMs) };
}

/** `<title> mm-ss.wav`, as the save panel proposes it (the app cleans the title the same way). */
export function clipFileName(title: string, fromMs: number): string {
  const s = Math.floor(fromMs / 1000), clean = title.replace(/[/\\:\p{Cc}]/gu, "-").trim().replace(/^\.+/, "") || "Clip";
  return `${clean} ${String(Math.floor(s / 60)).padStart(2, "0")}-${String(s % 60).padStart(2, "0")}.wav`;
}
