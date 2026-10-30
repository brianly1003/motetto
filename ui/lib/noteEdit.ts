// In-place note edits (SPEC 7.11): a block is a line range of notes.md; an edit rewrites only
// its typed text and keeps the parts the user doesn't type (bullet, checkbox and owner, heading
// marks, a jot's time) exactly as they were.

export type Kind = "task" | "jot" | "heading" | "li" | "p";

// The fixed start of a block's first line.
export function lead(kind: Kind, first: string) {
  // Task: the checkbox and the owner, as document.ts taskLine reads them (kept import-free for the node tests).
  const pattern = { task: /^\s*[-*] \[[ xX]\] (?:(?:\*\*)?[^:*[\]\n]{1,40}(?:\*\*)?:\s+)?/, jot: /^\s*(?:\[\d+:\d{2}\]\s*)?/, heading: /^\s*#{1,6}\s+/, li: /^\s*(?:[-*+]|\d+[.)])\s+/, p: /^/ }[kind];
  return pattern.exec(first)?.[0] ?? "";
}

// Typed at the start of a paragraph: "[] " (or "[ ] ") task, "# " section heading; "- " is
// already a bullet. At the start of a bullet, "[] " turns it into a task.
const TASK = /^\[ ?\]\s+/;
function shorthand(text: string) {
  return text.replace(TASK, "- [ ] ").replace(/^#\s+/, "### ");
}

function clean(body: string) {
  return body.replace(/​/g, "").replace(/[ \t ]+/g, " ").replace(/ *\n */g, "\n").trim();
}

function lines(kind: Kind, first: string, body: string) {
  const text = clean(body);
  if (!text) return [];
  if (kind === "p") return shorthand(text).split("\n");
  const one = text.replace(/\n/g, " ");
  if (kind === "li" && TASK.test(one)) return [`${/^\s*/.exec(first)![0]}- [ ] ${one.replace(TASK, "")}`];
  return [lead(kind, first) + one];
}

// Remove in Quick check (SPEC 7.13): the line goes. An Action items section left empty says
// "- None", as the notes writer does; any other section left empty goes with its heading.
export function removeLine(markdown: string, line: number) {
  const all = markdown.split("\n"), heading = (text: string) => /^#{1,6}\s/.test(text);
  let head = line; while (head >= 0 && !heading(all[head])) head--;
  all.splice(line, 1);
  if (head < 0) return all.join("\n");
  let end = head + 1; while (end < all.length && !heading(all[end]) && !all[end].includes("<details>")) end++;
  if (all.slice(head + 1, end).some(text => text.trim())) return all.join("\n");
  if (/^#{1,6}\s+action items\s*$/i.test(all[head])) all.splice(head + 1, 0, "- None");
  else all.splice(head, end - head);
  return all.join("\n");
}

// Blank lines that end the range (markdown-it counts them in a list item) stay where they are.
function splice(markdown: string, start: number, end: number, replacement: (old: string[]) => string[]) {
  const all = markdown.split("\n"), old = all.slice(start, end);
  let trailing = 0;
  while (trailing < old.length - 1 && !old[old.length - 1 - trailing].trim()) trailing++;
  return [...all.slice(0, start), ...replacement(old), ...Array(trailing).fill(""), ...all.slice(end)].join("\n");
}

// Rewrite one block with its new typed text; an emptied block is removed.
export function replaceBlock(markdown: string, start: number, end: number, kind: Kind, body: string) {
  return splice(markdown, start, end, old => lines(kind, old[0], body));
}

// Enter: the text after the caret becomes the next block of the same kind (a task gives a new
// unticked task). Returns null when the new block would be an empty paragraph, which Markdown
// can't hold: the caller just moves on to the next block.
export function splitBlock(markdown: string, start: number, end: number, kind: Kind, left: string, right: string) {
  const tail = clean(right);
  if (!tail && (kind === "p" || kind === "heading" || kind === "jot")) return null;
  return splice(markdown, start, end, old => {
    const next = kind === "task" ? `${/^\s*/.exec(old[0])![0]}- [ ] ${tail}` : kind === "li" ? lead("li", old[0]) + tail : kind === "heading" ? tail : `\n${tail}`;
    return [...lines(kind, old[0], left), ...next.split("\n")];
  });
}

// Owners are picked, never typed (SPEC 7.11): the @ picker gives a clean name; the file keeps the
// "- [ ] Name: task" form. 1-40 characters without the ones that would break the line.
export const cleanName = (name: string) => name.replace(/[:[\]*`<\n]/g, "").replace(/\s+/g, " ").trim().slice(0, 40);

// Assign: the task's owner becomes `name` (replacing any owner); a bullet or paragraph becomes a
// task, since giving it an owner makes it an action item. The checkbox state is kept.
export function assignBlock(markdown: string, start: number, end: number, kind: Kind, body: string, name: string) {
  return splice(markdown, start, end, old => {
    const indent = /^\s*/.exec(old[0])![0], box = kind === "task" ? /^\s*[-*] (\[[ xX]\])/.exec(old[0])?.[1] ?? "[ ]" : "[ ]";
    return [`${indent}- ${box} ${cleanName(name)}: ${clean(body).replace(/\n/g, " ").replace(TASK, "")}`];
  });
}

// Fix this name / reassign from the owner pill: one task (`line`), or every task owned by `old`
// (a misheard name is wrong everywhere in the meeting). "Me" and "You" are the same person.
export function renameOwner(markdown: string, old: string, name: string, line: number | null) {
  const same = (a: string) => (a === "Me" ? "You" : a).toLowerCase() === (old === "Me" ? "You" : old).toLowerCase();
  return markdown.split("\n").map((text, index) => text.replace(/^(\s*[-*] \[[ xX]\] )(?:(?:\*\*)?([^:*[\]\n]{1,40}?)(?:\*\*)?:\s+)?/, (head, box, owner) =>
    (line === null ? !!owner && same(owner) : index === line) ? `${box}${cleanName(name)}: ` : head)).join("\n");
}
