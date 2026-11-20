// Speaker names (SPEC 7.11): Find speakers labels the call side `Speaker N`; the user can name a
// voice once and every line by it follows. Names live in meeting.json (`speaker_names`), the
// transcript files keep `Speaker N`.
export type SpeakerNames = Record<number, string>;

/** The voice number of a `Speaker N` label; 0 for You, Them or an unlabelled line. */
export const voiceOf = (label: string) => Number(/^Speaker (\d+)$/.exec(label)?.[1] ?? 0);

/** Voices given the same name are one person: the lowest such voice stands for them all. */
export function personOf(voice: number, names: SpeakerNames): number {
  const name = names[voice];
  if (!name) return voice;
  return Math.min(...Object.entries(names).filter(([, v]) => v === name).map(([k]) => Number(k)));
}

/** A voice's colour: Speaker 1 and 2 keep the call side's, then four more that repeat. */
export const voiceColor = (voice: number) => voice <= 2 ? "var(--voice-them)" : `var(--voice-${3 + (voice - 3) % 4})`;

/** What a line's speaker reads as on screen. */
export const speakerLabel = (label: string, names: SpeakerNames) => names[voiceOf(label)] ?? label;

/** The transcript text with named voices' labels replaced, for Copy (files keep `Speaker N`). */
export const namedTranscript = (text: string, names: SpeakerNames) =>
  text.replace(/^(\[[0-9:]+\] )Speaker (\d+):/gm, (line, head: string, n: string) => names[Number(n)] ? `${head}${names[Number(n)]}:` : line);

const mention = (voice: number) => new RegExp(`\\bSpeaker ${voice}\\b(?!\\d)`, "g");
const COPY = "<details><summary>Transcript</summary>";
/** The notes split around the transcript copy the summary step wrote, which a notes edit may not
 * change (the files keep `Speaker N`; names apply on screen). */
const outsideCopy = (markdown: string): [string, string] => {
  const at = markdown.indexOf(COPY);
  if (at < 0) return [markdown, ""];
  const close = markdown.indexOf("</details>", at);
  return [markdown.slice(0, at), close < 0 ? markdown.slice(at) : markdown.slice(at, close)];
};
/** How often the notes say `Speaker N` (an action item's owner, a sentence), not counting the transcript copy. */
export const notesMentions = (markdown: string, voice: number) => {
  const [head, copy] = outsideCopy(markdown), tail = markdown.slice(head.length + copy.length);
  return (head + tail).match(mention(voice))?.length ?? 0;
};
/** The notes with `Speaker N` written as the new name (or back from an old one); the transcript copy stays. */
export const renameInNotes = (markdown: string, voice: number, name: string) => {
  const [head, copy] = outsideCopy(markdown), tail = markdown.slice(head.length + copy.length);
  return head.replace(mention(voice), name) + copy + tail.replace(mention(voice), name);
};

const ADDRESS = /(?:\b(?:[Aa]sk|[Ww]ith|[Hh]i|[Hh]ey|[Tt]hanks,?|[Aa]nh|[Cc]hị|[Ee]m|[Bb]ạn|[Cc]ô|[Cc]hú)\s+(\p{Lu}[\p{L}'-]+))|(?:(\p{Lu}[\p{L}'-]+)\s+ơi\b)/gu;
const NOT_NAMES = new Set(["Ok", "OK", "The", "This", "That", "You", "Them", "Me", "Speaker"]);
/** Names said in the call ("ask Linh", "anh Met", "Kate ơi"), most heard first: name suggestions. */
export function saidNames(texts: string[], max = 4): string[] {
  const count = new Map<string, number>();
  for (const text of texts) for (const m of text.matchAll(ADDRESS)) {
    const name = m[1] ?? m[2];
    if (name && !NOT_NAMES.has(name)) count.set(name, (count.get(name) ?? 0) + 1);
  }
  return [...count].sort((a, b) => b[1] - a[1]).slice(0, max).map(([name]) => name);
}
