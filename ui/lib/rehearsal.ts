// What a rehearsal measured (SPEC 14.6): counted from your words, never generated. The full
// transcript is replayed word by word through the strip's own follower, so "reached",
// "skipped" and "off-script" mean what the strip showed.
import type { At, Line, Section, follow as Follow } from "./script";

export type Word = { ms: number; text: string };
export type Report = {
  totalS: number;
  perSection: { title: string; plannedS: number; actualS: number }[];
  wpm: number;
  skipped: number[];
  offScript: { startMs: number; endMs: number; before: number }[];
  fillers: Record<string, number>;
  longLines: number[];
};

// Fixed lists per script language (SPEC 14.6); other languages show no filler count.
const FILLERS: Record<string, string[]> = { en: ["um", "uh", "you know", "basically", "like"], vi: ["ờ", "à", "kiểu", "thì là"] };
const VIETNAMESE = /[ăâđêôơưạảấầẩẫậắằẳẵặẹẻẽếềểễệỉịọỏốồổỗộớờởỡợụủứừửữựỳỵỷỹ]/i;
const OFF_SCRIPT_MS = 10_000;
// A word takes at most this long; the rest of a gap is silence (a demo click), not off-script.
const WORD_MS = 600;
// Shorter lines are left alone: a pause makes them look long.
const LONG_MIN_WORDS = 15;

/** `[mm:ss] Me: …` lines of a transcript, as timed text (your lines only). */
export function spoken(transcript: string) {
  return transcript.split("\n").flatMap(line => {
    const m = line.match(/^\[(\d+):(\d+)(?::(\d+))?\]\s*Me:\s*(.*)$/);
    if (!m || !m[4].trim()) return [];
    const [h, mm, ss] = m[3] ? [+m[1], +m[2], +m[3]] : [0, +m[1], +m[2]];
    return [{ ms: ((h * 60 + mm) * 60 + ss) * 1000, text: m[4].trim() }];
  });
}

function count(tokens: string[], phrase: string) {
  const want = phrase.split(" ");
  let n = 0;
  for (let i = 0; i + want.length <= tokens.length; i++) if (want.every((w, k) => tokens[i + k] === w)) n++;
  return n;
}

/** `follow` is the strip's own (script.ts), passed in so this module has no runtime imports. */
export function report(script: { sections: Section[]; lines: Line[] }, transcript: string, durationMs: number, follow: typeof Follow): Report {
  const { lines, sections } = script;
  const segments = spoken(transcript);
  // Each word gets a time inside its segment, spread evenly up to the next one.
  const words: Word[] = segments.flatMap((seg, i) => {
    const parts = seg.text.split(/\s+/).filter(Boolean);
    const next = segments[i + 1]?.ms ?? seg.ms + parts.length * 400;
    return parts.map((text, k) => ({ ms: seg.ms + Math.round((next - seg.ms) * k / parts.length), text }));
  });
  const reached = new Map<number, number>();
  const advanced: boolean[] = [];
  const startedAt: number[] = [];
  let at: At = { line: 0, word: 0 };
  words.forEach((word, i) => {
    startedAt.push(at.line);
    const next = follow(lines, at, words.slice(Math.max(0, i - 4), i + 1).map(w => w.text).join(" "));
    const moved = next.line !== at.line || next.word !== at.word;
    advanced.push(moved);
    if (moved) {
      // The line a word landed in; reaching the start of the next line only finished this one.
      const line = next.word > 0 ? next.line : next.line - 1;
      if (!reached.has(line)) reached.set(line, word.ms);
    }
    at = { line: next.line, word: next.word };
  });
  const last = Math.max(-1, ...reached.keys());
  const skipped = lines.map((_, i) => i).filter(i => i < last && !reached.has(i));

  // Off-script: words that moved nothing, in runs; a run with fewer than 3 moving words inside
  // is one stretch (a stray "the" matching mid-ad-lib must not split it).
  const offScript: Report["offScript"] = [];
  let i = 0;
  while (i < words.length) {
    if (advanced[i]) { i++; continue; }
    let end = i;
    for (let j = i; j < words.length; j++) {
      if (!advanced[j]) { end = j; continue; }
      let moving = 0;
      while (j + moving < words.length && advanced[j + moving] && moving < 3) moving++;
      if (moving >= 3 || j + moving >= words.length) break;
      j += moving - 1;
    }
    const endMs = Math.min(words[end + 1]?.ms ?? durationMs, words[end].ms + WORD_MS);
    if (endMs - words[i].ms >= OFF_SCRIPT_MS) offScript.push({ startMs: words[i].ms, endMs, before: startedAt[i] });
    i = end + 1;
  }

  const sectionStart = sections.map((s, n) => {
    const own = lines.map((line, k) => line.section === n ? reached.get(k) : undefined).filter((v): v is number => v !== undefined);
    return own.length ? Math.min(...own) : undefined;
  });
  const perSection = sections.map((s, n) => {
    const start = sectionStart[n];
    const end = sectionStart.slice(n + 1).find(v => v !== undefined) ?? durationMs;
    return { title: s.title, plannedS: s.minutes * 60, actualS: start === undefined ? 0 : Math.round((end - start) / 1000) };
  });

  const longLines = lines.map((line, k) => {
    const start = reached.get(k);
    if (start === undefined || line.words.length < LONG_MIN_WORDS) return -1;
    const end = [...reached.entries()].filter(([l]) => l > k).map(([, ms]) => ms).sort((a, b) => a - b)[0] ?? durationMs;
    return (end - start) / 1000 > 1.4 * line.words.length / 150 * 60 ? k : -1;
  }).filter(k => k >= 0).slice(0, 5);

  const tokens = segments.map(s => s.text).join(" ").normalize("NFC").toLowerCase().split(/[^\p{L}\p{N}']+/u).filter(Boolean);
  const language = VIETNAMESE.test(lines.map(l => l.text).join(" ")) ? "vi" : "en";
  // A filler the script itself says ("I basically…") is the script, not a filler.
  const scripted = lines.map(l => l.text).join(" ").normalize("NFC").toLowerCase().split(/[^\p{L}\p{N}']+/u).filter(Boolean);
  const fillers: Record<string, number> = {};
  for (const phrase of FILLERS[language]) { const n = count(tokens, phrase) - count(scripted, phrase); if (n > 0) fillers[phrase] = n; }

  return {
    totalS: Math.round(durationMs / 1000),
    perSection,
    wpm: durationMs ? Math.round(words.length / (durationMs / 60_000)) : 0,
    skipped,
    offScript,
    fillers,
    longLines,
  };
}
