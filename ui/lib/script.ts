// A presenter's script (SPEC 14.4): `# Title (Nm)` sections, `[cue]` notes that are never
// spoken, and blank lines between strip lines. Following (14.5) matches heard words to it.
export type Line = {text: string; words: string[]; cues: string[]; section: number};
export type Section = {title: string; minutes: number; first: number};
export type At = {line: number; word: number};
/** "Ctrl+Alt+T" as macOS shows it, "⌃⌥T"; as typed elsewhere. */
export const keyLabel = (accel: string | undefined) => !accel ? '' : /Mac/.test(navigator.platform) ? accel.replace('Ctrl+', '⌃').replace('Alt+', '⌥').replace('Shift+', '⇧').replace('Down', '↓').replace('Up', '↑').replace('Backslash', '\\') : accel;

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
  // Up to 2 misheard words at the end are skipped when at least 2 words before them line up.
  for (let d = 0; d <= Math.min(2, h.length - 1); d++) {
    const n = h.length - d;
    for (let p = 0; p < flat.length; p++) {
      // Back from p: each heard word matches one script word, or two said as one
      // ("unemployed" for "an employee"); k counts heard words, j the script word before them.
      let k = 0, j = p;
      while (k < n && j >= 0) {
        const heard = h[n - 1 - k];
        if (same(flat[j].w, heard, d === 0 && k === 0)) j--;
        else if (j > 0 && flat[j - 1].line === flat[j].line && merged(flat[j - 1].w, flat[j].w, heard)) j -= 2;
        else break;
        k++;
      }
      if (!k || (d && k < 2)) continue;
      if (p === c - 1 && !d) inStep = true;
      // On from where you are, a jump on 3 words, or past up to 5 words you skipped or were
      // misheard when the 2 words after them line up (you need not read every word).
      if (p >= c && (k >= 3 || j + 1 <= c || (k >= 2 && (j + 1 === c + 1 || (!d && j + 1 <= c + 5))))) best = Math.max(best, p);
    }
  }
  // Nothing lines up: pronunciation is not this strip's job (a report will help with it), so 2
  // or more words that sound like the next ones still carry you on from where you are (3 or
  // more also past the 1 or 2 words you are on).
  if (best < 0) for (let p = c; p < flat.length; p++) {
    let k = 0, j = p;
    while (k < h.length && j >= 0 && (same(flat[j].w, h[h.length - 1 - k], k === 0) || sounds(flat[j].w, h[h.length - 1 - k]))) { j--; k++; }
    if ((k >= 2 && j + 1 <= c) || (k >= 3 && j + 1 <= c + 2)) best = Math.max(best, p);
  }
  // Still nothing: a word read wrong, even nothing like it, never holds you (a rehearsal, not a
  // lesson). Saying one of the next 2 script words passes the 1 or 2 before it, when up to one
  // more heard word than were passed sits between it and the script word before them.
  const like = (script: string, heard: string, saying: boolean) => same(script, heard, saying) || sounds(script, heard);
  if (best < 0 && c > 0) for (let p = c + 1; best < 0 && p <= Math.min(c + 2, flat.length - 1); p++) {
    if (!like(flat[p].w, h[h.length - 1], true)) continue;
    for (let m = 0; m <= p - c + 1 && m + 2 <= h.length; m++)
      if (like(flat[c - 1].w, h[h.length - 2 - m], false)) best = p;
  }
  // A garbled stretch ("address and they sabotage" for "insider threats and data sabotage"):
  // 3 or more heard words that line up in order, starting near where you are, each as far from
  // the last as in the script (1 apart either way, at most 3 wrong between), end on the word said.
  if (best < 0) {
    const chain = new Map<string, number>(), from = Math.max(0, c - 1);
    const longest = (i: number, j: number): number => {
      const keyed = `${i},${j}`;
      if (chain.has(keyed)) return chain.get(keyed)!;
      let n = j >= from && j <= c + 2 ? 1 : -Infinity;
      for (let di = 1; di <= 4 && i - di >= 0; di++)
        for (let dj = Math.max(1, di - 1); dj <= di + 1 && j - dj >= from; dj++)
          if (like(flat[j - dj].w, h[i - di], false)) n = Math.max(n, longest(i - di, j - dj) + 1);
      chain.set(keyed, n);
      return n;
    };
    for (let p = c; p < flat.length; p++)
      if (like(flat[p].w, h[h.length - 1], true) && longest(h.length - 1, p) >= 3) best = Math.max(best, p);
  }
  if (best < 0) return {...at, waiting: !inStep};
  const {line, word} = flat[best];
  return word + 1 >= lines[line].words.length && line + 1 < lines.length
    ? {line: line + 1, word: 0, waiting: false}
    : {line, word: word + 1, waiting: false};
}

/**
 * A heard word against a script word: equal, a near miss ("handle" for "handles", 4+ letters),
 * a long word heard a little wrong (6+ letters, at most 1 letter in 4 differs: "repairing" for
 * "preparing"), or, for the word still being said, its first 3+ letters ("infra").
 */
function same(script: string, heard: string, saying: boolean) {
  if (script === heard) return true;
  const [short, long] = script.length < heard.length ? [script, heard] : [heard, script];
  if (short.length >= 4 && long.length - short.length <= 2 && long.startsWith(short)) return true;
  if (close(script, heard)) return true;
  return saying && heard.length >= 3 && script.startsWith(heard);
}

/**
 * Sounds alike: the same consonants once vowels go and close ones count as one (c/k/g/q, d/t,
 * b/p, s/z/x, f/v): "lock" for "log", "did" for "data", "Malaysia" for "malicious" (one apart).
 */
function sounds(script: string, heard: string) {
  const a = key(script), b = key(heard);
  // One consonant says little ("the", "today"): then the words must also start alike and be
  // near the same length.
  if (a.length < 2) return a === b && a.length > 0 && script[0] === heard[0] && Math.abs(script.length - heard.length) <= 1;
  return a === b || (Math.min(a.length, b.length) >= 3 && distance(a, b, 1) <= 1);
}
const SOUND: Record<string, string> = {c: 'k', g: 'k', q: 'k', d: 't', b: 'p', z: 's', x: 's', v: 'f'};
const key = (w: string) => w.replace(/[aeiouyhw]/g, '').replace(/./g, ch => SOUND[ch] ?? ch).replace(/(.)\1+/g, '$1');

/** Equal, or a long word with at most 1 letter in 4 different. */
function close(a: string, b: string) {
  const cap = Math.min(a.length, b.length) >> 2;
  return a === b || (cap >= 1 && Math.min(a.length, b.length) >= 6 && distance(a, b, cap) <= cap);
}

/** Two script words said as one: close to both together, closer than to either alone. */
function merged(first: string, second: string, heard: string) {
  const both = first + second, d = distance(both, heard, both.length);
  return close(both, heard) && d < distance(first, heard, both.length) && d < distance(second, heard, both.length);
}

/** Letters to add, drop or change to turn `a` into `b`; stops counting past `cap`. */
function distance(a: string, b: string, cap: number) {
  if (Math.abs(a.length - b.length) > cap) return cap + 1;
  let row = Array.from({length: b.length + 1}, (_, i) => i);
  for (let i = 1; i <= a.length; i++) {
    const next = [i];
    for (let k = 1; k <= b.length; k++)
      next[k] = Math.min(row[k] + 1, next[k - 1] + 1, row[k - 1] + (a[i - 1] === b[k - 1] ? 0 : 1));
    if (Math.min(...next) > cap) return cap + 1;
    row = next;
  }
  return row[b.length];
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
