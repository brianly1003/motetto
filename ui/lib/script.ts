// A presenter's script (SPEC 14.4): `# Title (Nm)` sections, `[cue]` notes that are never
// spoken, and blank lines between strip lines. Following (14.5) matches heard words to it.
export type Line = {text: string; words: string[]; cues: string[]; section: number};
export type Section = {title: string; minutes: number; first: number};
export type At = {line: number; word: number};

const NUMBERS = ['zero', 'one', 'two', 'three', 'four', 'five', 'six', 'seven', 'eight', 'nine', 'ten', 'eleven', 'twelve', 'thirteen', 'fourteen', 'fifteen', 'sixteen', 'seventeen', 'eighteen', 'nineteen', 'twenty'];
const CJK = /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]/u;

/** Comparable words: no case, accents or punctuation; "3" reads as "three"; CJK per character. */
export function tokens(text: string) {
  const out: string[] = [];
  for (const raw of text.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase().split(/[^\p{L}\p{N}]+/u)) {
    if (!raw) continue;
    if (CJK.test(raw)) out.push(...[...raw]);
    else out.push(/^\d+$/.test(raw) && +raw <= 20 ? NUMBERS[+raw] : raw);
  }
  return out;
}

/** A pasted Markdown line as it is said: no bullet, number or quote mark, no emphasis marks. */
function plain(line: string) {
  return line.trim().replace(/^(?:[-*+>]|\d+[.)])\s+/, '').replace(/\*\*|__|`/g, '').trim();
}

export function parseScript(md: string): {sections: Section[]; lines: Line[]} {
  const sections: Section[] = [], lines: Line[] = [];
  for (const raw of md.split(/\n\s*\n|\n(?=#)/)) {
    const block = raw.trim();
    if (!block) continue;
    let body = block;
    const heading = block.startsWith('#') ? block.match(/^#+\s+(.+?)(?:\s*\((\d+)\s*m\))?\s*$/m) : null;
    if (heading) {
      sections.push({title: heading[1], minutes: Number(heading[2] ?? 0), first: lines.length});
      const end = block.indexOf('\n');
      body = end < 0 ? '' : block.slice(end + 1);
    }
    if (!sections.length) sections.push({title: 'Script', minutes: 0, first: 0});
    for (const text of body.split('\n').map(plain).filter(Boolean)) {
      const cues = [...text.matchAll(/\[([^\]]*)\]/g)].map(m => m[1].trim()).filter(Boolean);
      lines.push({text, words: tokens(text.replace(/\[[^\]]*\]/g, ' ')), cues, section: sections.length - 1});
    }
  }
  return {sections, lines};
}

/**
 * Where you are after hearing `heard` (the last few seconds of your voice). The position moves on
 * with words that continue from it, jumps ahead only on 3 matching words within the next 3 lines,
 * and never goes back by itself; heard words that match nothing ahead mean you went off-script.
 */
export function follow(lines: Line[], at: At, heard: string): At & {waiting: boolean} {
  const h = tokens(heard).slice(-12);
  if (!h.length || !lines.length) return {...at, waiting: false};
  // The previous line too, so words already followed still count as "in step".
  const flat: {line: number; word: number; w: string}[] = [];
  for (let l = Math.max(0, at.line - 1); l <= Math.min(lines.length - 1, at.line + 3); l++)
    lines[l].words.forEach((w, word) => flat.push({line: l, word, w}));
  let c = flat.findIndex(f => f.line === at.line && f.word >= at.word);
  if (c < 0) c = flat.length;
  let best = -1, inStep = false;
  for (let p = 0; p < flat.length; p++) {
    let k = 0;
    while (k < h.length && k <= p && flat[p - k].w === h[h.length - 1 - k]) k++;
    if (!k) continue;
    if (p === c - 1) inStep = true;
    if (p >= c && (k >= 3 || p - k + 1 <= c)) best = p;
  }
  if (best < 0) return {...at, waiting: !inStep};
  const {line, word} = flat[best];
  return word + 1 >= lines[line].words.length && line + 1 < lines.length
    ? {line: line + 1, word: 0, waiting: false}
    : {line, word: word + 1, waiting: false};
}

/** Planned seconds in all, planned seconds up to the end of the current section, and whether you're over. */
export function pace(sections: Section[], line: number, elapsedS: number) {
  const totalS = sections.reduce((sum, s) => sum + s.minutes * 60, 0);
  const current = sections.findLastIndex(s => s.first <= line);
  const budgetS = sections.slice(0, current + 1).reduce((sum, s) => sum + s.minutes * 60, 0);
  return {totalS, budgetS, behind: elapsedS > budgetS};
}

/** A name for a new script from its first line: "# Opening (1m)" names it "Opening". */
export function nameFrom(text: string, taken: Set<string>, fallback: string) {
  const first = text.split("\n").map(l => l.trim()).find(Boolean) ?? "";
  const words = first.replace(/^#+\s*/, "").replace(/\(\d+\s*m\)/, "").replace(/\[[^\]]*\]/g, "")
    .replace(/[^\p{L}\p{N} \-_.()]/gu, " ").trim().split(/\s+/).filter(Boolean);
  let base = "";
  for (const w of words) { if ((base + " " + w).trim().length > 40) break; base = (base + " " + w).trim(); }
  base = base.replace(/^\.+/, "") || fallback;
  let name = base;
  for (let n = 2; taken.has(name); n++) name = `${base} ${n}`;
  return name;
}

/** The raw text with script line `k` (as parseScript counts them) replaced; cues on it stay. */
export function withLine(text: string, k: number, to: string) {
  const rows = text.split("\n");
  let n = -1;
  for (let i = 0; i < rows.length; i++) {
    const row = rows[i].trim();
    if (!row || row.startsWith("#")) continue;
    if (++n !== k) continue;
    const cues = row.match(/\[[^\]]*\]/g)?.join(" ");
    rows[i] = rows[i].slice(0, rows[i].length - rows[i].trimStart().length) + (cues ? `${cues} ${to}` : to);
    return rows.join("\n");
  }
  return text;
}
