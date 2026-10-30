import { useEffect, useRef, useState } from 'react';
import { noteSections, taskLine, timeLabel } from '@/lib/document';
import { transcriptLines } from '@/lib/transcript';
import { renderInline, renderNotes, timeChip, timeMs } from '@/lib/md';
import { assignBlock, cleanName, renameOwner, replaceBlock, splitBlock, type Kind } from '@/lib/noteEdit';
import { alike, matchPeople } from '@/lib/people';
import { handBack } from '@/lib/focus';
import PeoplePicker from './PeoplePicker';
import { strings as s } from '@/lib/strings';
import * as ipc from '@/lib/ipc';

// Mockup: these sections take the small label heading wherever they appear; topics take the serif one.
const LABELS = /^(action items|decisions|overview|open questions|my notes|your jots)$/i;
const DECISIONS = /^decisions$/i, CHECKED = /^(action items|decisions)$/i;
const MY_NOTES = /^my notes$/i;
// Atoms keep their look and their own controls while a block is edited (SPEC 7.11).
const ATOMS = 'input, button, .owner, .jotmark, .tip, .aq, .unsure';
// Blocks with links (an autolink is "<scheme:...>" or "<a@b>") or code stay read-only until the
// serializer round-trips them. Other "<" is text (no HTML in notes), so "<You>" stays editable.
const UNSAFE = /\]\(|<[^<>\s]*[:@][^<>\s]*>|`/;

// Typed text of an edited element, written back as note syntax; atoms add nothing except a time chip.
function rawOf(node: Node): string {
  return [...node.childNodes].map(n => {
    if (n.nodeType === Node.TEXT_NODE) return n.nodeValue ?? '';
    if (!(n instanceof HTMLElement)) return '';
    if (n.matches('button.src')) return `[${n.textContent}]`;
    if (n.matches('input, .owner, .jotmark, .tip, .aq, .unsure')) return '';
    if (n.matches('b, strong')) return `**${rawOf(n)}**`;
    if (n.matches('em, i')) return `*${rawOf(n)}*`;
    if (n.matches('br')) return '\n';
    return rawOf(n);
  }).join('');
}
const texts = (el: Element) => { const w = document.createTreeWalker(el, NodeFilter.SHOW_TEXT), out: Text[] = []; while (w.nextNode()) if (!w.currentNode.parentElement?.closest(ATOMS)) out.push(w.currentNode as Text); return out; };
type At = {x: number; y: number} | 0 | 'end';
function placeCaret(el: HTMLElement, at: At) {
  el.focus({preventScroll: true});
  let node: Node | null = null, offset = 0;
  if (typeof at === 'object') {
    const range = document.caretRangeFromPoint?.(at.x, at.y);
    if (range && el.contains(range.startContainer) && !(range.startContainer.nodeType === 3 ? range.startContainer.parentElement : range.startContainer as Element)?.closest(ATOMS)) [node, offset] = [range.startContainer, range.startOffset];
  }
  if (!node) { const all = texts(el), t = at === 0 ? all[0] : all[all.length-1]; [node, offset] = t ? [t, at === 0 ? 0 : t.length] : [el, el.childNodes.length]; }
  const r = document.createRange(); r.setStart(node, offset); r.collapse(true);
  const sel = getSelection()!; sel.removeAllRanges(); sel.addRange(r);
}
// The caret is at the first / last typed character, ignoring atoms (a chip at the end, the checkbox).
function atEdge(el: HTMLElement, last: boolean) {
  const sel = getSelection(); if (!sel?.rangeCount || !sel.focusNode) return true;
  const r = document.createRange();
  if (last) { r.setStart(sel.focusNode, sel.focusOffset); r.setEnd(el, el.childNodes.length); } else { r.setStart(el, 0); r.setEnd(sel.focusNode, sel.focusOffset); }
  const f = r.cloneContents(); f.querySelectorAll(ATOMS).forEach(x => x.remove()); return !f.textContent?.replace(/​/g, '').trim();
}
const esc = (text: string) => text.replace(/[&<>"]/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]!));
// A note line as the notes writer lists an unconfirmed one (verify::claim): no list mark or checkbox.
export const claimOf = (text: string) => text.trim().replace(/^[-*] (?:\[[ xX]\] )?/, '').trim();
// The time a line cites: its last [m:ss], in ms; -1 for none.
const citedMs = (text: string) => { const all = [...text.matchAll(/\[(\d+):(\d{2})\]/g)], t = all[all.length - 1]; return t && Number(t[2]) < 60 ? (Number(t[1])*60+Number(t[2]))*1000 : -1; };

/** Verified notes (SPEC 7.13): `lines` are the unconfirmed claims; Keep and Remove in Quick check. */
export type Check = {lines: string[]; keep: (claim: string) => void; remove: (claim: string, line: number, decision: boolean) => void};

const editableOf = (block: HTMLElement) => block.querySelector<HTMLElement>(':scope .task-text, :scope .jot-text') ?? block;
const kindOf = (block: HTMLElement): Kind => (block.dataset.kind as Kind) ?? (block.tagName === 'LI' ? 'li' : 'p');

/** `translated`: a Read in translation, read-only. Its lines are the notes' lines, so a heading
 *  keeps its original's look (`labelOf`) and a tick goes to notes.md (`onToggle`, none when stale). */
export type Translated = {hint:string; labelOf:(line:number)=>string|undefined; onToggle?:(line:number, done:boolean)=>Promise<void>};

export default function NotesView({id, markdown, locked=false, translated, onChange, onError, durationMs, onSeek, transcript='', check}: {id:string; markdown:string; locked?:boolean; translated?:Translated; onChange:(text:string)=>void; onError:(error:string)=>void; durationMs:number; onSeek?:(ms:number)=>void; transcript?:string; check?:Check}) {
  const [saving, setSaving] = useState(false);
  // The @ picker (SPEC 7.11): the one way to assign. `at` is the "@" being typed: its text node,
  // offset and the query after it; the menu lists You, then owners already in these notes.
  type Mention = {node: Text; start: number; query: string; x: number; y: number; index: number};
  const [mention, setMentionState] = useState<Mention | null>(null);
  const mentionRef = useRef<Mention | null>(null);
  const setMention = (m: Mention | null) => { mentionRef.current = m; setMentionState(m); };
  const box = useRef<HTMLDivElement>(null);
  // `latest` is what notes.md holds now (autosave writes it while typing); `base` is the text the
  // edited block's line range refers to. The page re-renders only when a block is left, so the
  // caret never jumps and nothing reflows (SPEC 7.11).
  const latest = useRef(markdown), base = useRef(markdown), editing = useRef<HTMLElement|null>(null);
  const history = useRef<string[]>([]), pending = useRef<{index:number; at:At}|null>(null), timer = useRef(0);
  const durationRef = useRef(durationMs); durationRef.current = durationMs;
  useEffect(() => { if (!editing.current) latest.current = base.current = markdown; }, [markdown]);

  async function save(next: string) {
    if (next === latest.current) return true;
    try { latest.current = await ipc.saveNotes(id, next, latest.current); return true; }
    catch (error) { onError(String(error)); return false; }
  }
  const blocks = () => [...(box.current?.querySelectorAll<HTMLElement>('[data-lines]') ?? [])].filter(b => b.dataset.editable === '1');
  const range = (block: HTMLElement) => block.dataset.lines!.split(',').map(Number) as [number, number];
  const draft = (block: HTMLElement) => { const [a, b] = range(block); return replaceBlock(base.current, a, b, kindOf(block), rawOf(editableOf(block))); };

  function start(block: HTMLElement, at: At) {
    const el = editableOf(block);
    el.querySelectorAll<HTMLElement>(ATOMS).forEach(x => { x.contentEditable = 'false'; });
    block.classList.add('editing'); el.contentEditable = 'true'; editing.current = block; base.current = latest.current;
    placeCaret(el, at);
  }
  function stop(block: HTMLElement) {
    if (mentionRef.current) setMention(null);
    const el = editableOf(block);
    block.classList.remove('editing'); el.removeAttribute('contenteditable'); el.querySelectorAll('[contenteditable]').forEach(x => x.removeAttribute('contenteditable'));
    if (editing.current === block) editing.current = null;
  }
  // Leave a block: save it, then re-render once; `then` says where the caret goes next.
  async function commit(block: HTMLElement, then?: {index:number; at:At}, next = draft(block)) {
    clearTimeout(timer.current);
    const before = base.current; stop(block);
    if (next !== before && await save(next)) { history.current.push(before); base.current = next; pending.current = then ?? null; onChange(next); return; }
    if (then) focusBlock(then);
  }
  // People to offer: You first, then everyone already owning a task here (lib/people.ts).
  const known = () => [...new Set(['You', ...latest.current.split('\n').map(line => taskLine(line)?.owner).filter((n): n is string => !!n).map(n => n === 'Me' ? 'You' : n.replace(/^\*\*|\*\*$/g, ''))])];
  const people = (query: string) => matchPeople(known(), query);
  // The owner pill's menu: filter Action items by person, or fix a misheard name.
  const [ownerMenu, setOwnerMenu] = useState<{name: string; line: number; x: number; y: number} | null>(null);
  const [ownerFilter, setOwnerFilter] = useState<string | null>(null);
  const [renaming, setRenaming] = useState<{name: string; line: number} | null>(null);
  useEffect(() => {
    if (!ownerMenu) return;
    const close = (e: PointerEvent) => { if (!(e.target as HTMLElement).closest('.mention, .owner')) setOwnerMenu(null); };
    const esc = (e: KeyboardEvent) => { if (e.key === 'Escape') setOwnerMenu(null); };
    document.addEventListener('pointerdown', close); document.addEventListener('keydown', esc);
    return () => { document.removeEventListener('pointerdown', close); document.removeEventListener('keydown', esc); };
  }, [ownerMenu]);
  // A correction (Kenvin → Kevin) renames that person on every task; a different name reassigns this one.
  async function fixName(old: string, line: number, value: string) {
    setRenaming(null);
    const name = cleanName(value); if (!name || name === old) return;
    const before = latest.current, next = renameOwner(before, old, name, alike(old, name) ? null : line);
    if (next !== before && await save(next)) { history.current.push(before); base.current = next; onChange(next); }
  }
  function pick(name: string) {
    const block = editing.current, m = mentionRef.current; setMention(null);
    if (!block || !m) return;
    const r = document.createRange(); r.setStart(m.node, m.start); r.setEnd(m.node, Math.min(m.node.length, m.start + 1 + m.query.length)); r.deleteContents();
    const [a, b] = range(block), task = kindOf(block) === 'task', index = blocks().indexOf(block);
    void commit(block, task ? {index, at: 'end'} : undefined, assignBlock(base.current, a, b, kindOf(block), rawOf(editableOf(block)), name));
  }
  function focusBlock({index, at}: {index:number; at:At}) { const b = blocks()[Math.max(0, index)]; if (b) start(b, at); }
  useEffect(() => { const p = pending.current; pending.current = null; if (p) focusBlock(p); });

  // Pointer, keys, typing and paste go through the container: blocks are rendered HTML.
  useEffect(() => {
    const root = box.current; if (!root) return;
    let pointer = false;
    const down = (e: PointerEvent) => { pointer = true; };
    const up = () => { pointer = false; };
    const mouse = (e: MouseEvent) => {
      const block = (e.target as HTMLElement).closest<HTMLElement>('[data-lines]');
      if (!block || block.dataset.editable !== '1' || block === editing.current || (e.target as HTMLElement).closest(ATOMS)) return;
      e.preventDefault();
      const at = {x: e.clientX, y: e.clientY}, index = blocks().indexOf(block);
      if (editing.current) void commit(editing.current, {index, at}); else start(block, at);
    };
    const leave = (e: FocusEvent) => {
      const block = editing.current; if (!block || !editableOf(block).contains(e.target as Node)) return;
      if (editableOf(block).contains(e.relatedTarget as Node)) return;
      // A click elsewhere lands first, then the block saves and re-renders.
      if (pointer) document.addEventListener('pointerup', () => setTimeout(() => { if (editing.current === block) void commit(block); }), {once: true, capture: true});
      else void commit(block);
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === 'Enter' && e.target === root) { e.preventDefault(); focusBlock({index: 0, at: 'end'}); return; }
      const block = editing.current; if (!block) return;
      const m = mentionRef.current;
      if (m) {
        const options = people(m.query);
        if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { e.preventDefault(); setMention({...m, index: (m.index + (e.key === 'ArrowDown' ? 1 : -1) + options.length) % Math.max(1, options.length)}); return; }
        if ((e.key === 'Enter' || e.key === 'Tab') && options.length) { e.preventDefault(); pick(options[Math.min(m.index, options.length - 1)]); return; }
        if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); setMention(null); return; }
      }
      const el = editableOf(block), all = blocks(), index = all.indexOf(block), empty = !rawOf(el).replace(/​/g, '').trim();
      if ((e.metaKey || e.ctrlKey) && /^[iu]$/i.test(e.key)) { e.preventDefault(); return; } // notes keep bold only
      // Enter saves and leaves the block, as Esc does; ⌘Enter / Ctrl+Enter adds a block after it;
      // ⇧Enter is a line break inside it (owner 2026-10-26).
      if (e.key === 'Escape' || e.key === 'Enter' && !e.shiftKey && !e.metaKey && !e.ctrlKey) { e.preventDefault(); e.stopPropagation(); const back = handBack(root); void commit(block).then(back); }
      else if (e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault();
        const sel = getSelection(); if (!sel?.rangeCount || empty) { const back = handBack(root); void commit(block).then(back); return; }
        // At the end of the typed text, a trailing chip (the task's source time) stays with its block.
        const end = atEdge(el, true);
        const tail = document.createRange(); tail.setStart(sel.focusNode!, sel.focusOffset); tail.setEnd(el, el.childNodes.length);
        const after = document.createElement('div'); if (!end) after.append(tail.cloneContents());
        const head = document.createRange(); head.setStart(el, 0); head.setEnd(end ? el : sel.focusNode!, end ? el.childNodes.length : sel.focusOffset);
        const before = document.createElement('div'); before.append(head.cloneContents());
        const [a, b] = range(block), split = splitBlock(base.current, a, b, kindOf(block), rawOf(before), rawOf(after));
        void commit(block, {index: index + 1, at: split ? 0 : 'end'}, split ?? draft(block));
      }
      else if (e.key === 'Backspace' && empty) { e.preventDefault(); void commit(block, {index: index - 1, at: 'end'}); }
      else if (e.key === 'ArrowUp' && index > 0 && atEdge(el, false)) { e.preventDefault(); void commit(block, {index: index - 1, at: 'end'}); }
      else if (e.key === 'ArrowDown' && index < all.length - 1 && atEdge(el, true)) { e.preventDefault(); void commit(block, {index: index + 1, at: 0}); }
    };
    // Note syntax turns into its element as soon as it is complete; autosave 1 s after typing stops.
    const input = () => {
      const block = editing.current; if (!block) return;
      const sel = getSelection(), n = sel?.focusNode;
      if (sel?.isCollapsed && n?.nodeType === Node.TEXT_NODE) {
        const before = n.nodeValue!.slice(0, sel.focusOffset);
        const t = /\[(\d+):(\d{2})\]$/.exec(before), ms = t ? timeMs(t[1], t[2], durationRef.current) : -1, bold = t ? null : /\*\*([^*\n]+)\*\*$/.exec(before);
        if (ms >= 0 || bold) {
          const m = (t ?? bold)!, r = document.createRange(); r.setStart(n, sel.focusOffset - m[0].length); r.setEnd(n, sel.focusOffset); r.deleteContents();
          const holder = document.createElement('span'); holder.innerHTML = t ? timeChip(t[1], t[2], ms) : '<b></b>';
          const el = holder.firstElementChild as HTMLElement; if (t) el.contentEditable = 'false'; else el.textContent = bold![1];
          const caret = document.createTextNode('​'); r.insertNode(caret); r.insertNode(el);
          const c = document.createRange(); c.setStart(caret, 1); c.collapse(true); sel.removeAllRanges(); sel.addRange(c);
        }
      }
      const node = getSelection()?.focusNode;
      if (node?.nodeType === Node.TEXT_NODE) {
        const offset = getSelection()!.focusOffset, at = /(?:^|\s)@([^@\n[\]:*`<]{0,40})$/.exec(node.nodeValue!.slice(0, offset).replace(/\u200b/g, ''));
        if (at) {
          const start = offset - at[1].length - 1, r = document.createRange(); r.setStart(node, start); r.setEnd(node, start + 1);
          const box = r.getBoundingClientRect(), was = mentionRef.current;
          setMention({node: node as Text, start, query: at[1], x: box.left, y: box.bottom + 4, index: was?.node === node && was.start === start ? Math.min(was.index, people(at[1]).length - 1) : 0});
        } else if (mentionRef.current) setMention(null);
      } else if (mentionRef.current) setMention(null);
      clearTimeout(timer.current);
      timer.current = window.setTimeout(() => { if (editing.current === block) void save(draft(block)); }, 1000);
    };
    const paste = (e: ClipboardEvent) => {
      if (!editing.current?.contains(e.target as Node)) return;
      e.preventDefault(); document.execCommand('insertText', false, (e.clipboardData?.getData('text/plain') ?? '').replace(/\s*\n\s*/g, ' '));
    };
    const drop = (e: DragEvent) => { if (editing.current?.contains(e.target as Node)) e.preventDefault(); };
    // ⌘Z / Ctrl+Z with no caret in a block undoes the last saved change to these notes.
    const undo = (e: KeyboardEvent) => {
      if (e.key.toLowerCase() !== 'z' || !(e.metaKey || e.ctrlKey) || e.shiftKey || e.altKey || editing.current || !history.current.length) return;
      const active = document.activeElement as HTMLElement | null;
      if (active?.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(active?.tagName ?? '')) return;
      e.preventDefault(); const prev = history.current.pop()!;
      void save(prev).then(ok => { if (ok) { base.current = prev; onChange(prev); } else history.current.push(prev); });
    };
    root.addEventListener('pointerdown', down, true); document.addEventListener('pointerup', up, true);
    root.addEventListener('mousedown', mouse); root.addEventListener('focusout', leave); root.addEventListener('keydown', key);
    root.addEventListener('input', input); root.addEventListener('paste', paste); root.addEventListener('drop', drop); document.addEventListener('keydown', undo);
    return () => {
      root.removeEventListener('pointerdown', down, true); document.removeEventListener('pointerup', up, true);
      root.removeEventListener('mousedown', mouse); root.removeEventListener('focusout', leave); root.removeEventListener('keydown', key);
      root.removeEventListener('input', input); root.removeEventListener('paste', paste); root.removeEventListener('drop', drop); document.removeEventListener('keydown', undo);
    };
  });

  async function toggle(line:number, done:boolean) {
    setSaving(true);
    if (translated) { try { await translated.onToggle?.(line, done); } finally { setSaving(false); } return; }
    try { const next = await ipc.setActionItemDone(id,line,done,latest.current); latest.current = base.current = next; onChange(next); }
    catch(error) { onError(String(error)); }
    finally { setSaving(false); }
  }
  // The line each task or decision rests on, looked up by its time (mockup: shown on hover or focus).
  const said = new Map<number, {speaker: string; text: string}>();
  for (const line of transcriptLines(transcript, [])) if (line.ms !== null && !said.has(line.ms)) said.set(line.ms, line);
  const quoteOf = (text: string) => said.get(citedMs(text));
  const saidLabel = (speaker: string) => !speaker ? '' : speaker === 'You' ? `<span class="said me">${s.youSaid}</span>` : `<span class="said them">${s.theySaid}</span>`;
  const quoteInner = (text: string) => { const q = quoteOf(text); return q ? `${saidLabel(q.speaker)}<q>${esc(q.text)}</q>` : ''; };
  const quoteHtml = (text: string) => quoteOf(text) ? `<span class="aq rcpt" contenteditable="false">${quoteInner(text)}</span>` : '';
  const marked = (text: string) => !translated && !!check?.lines.includes(claimOf(text));
  const unsureHtml = `<span class="unsure" contenteditable="false" title="${esc(s.unsureTitle)}">${esc(s.unsure)}</span>`;
  const all = markdown.split('\n');
  const editable = (a:number, b:number) => locked || translated || all.slice(a, b).some(line => UNSAFE.test(line)) ? '0' : '1';
  // A section's non-task text, rendered with its real line numbers so each block knows its range.
  const body = (lines:{text:string; line:number}[], skip:(text:string)=>boolean, decisions = false) => {
    if (!lines.length) return '';
    const first = lines[0].line, text = Array(lines[lines.length-1].line - first + 1).fill('');
    lines.forEach(line => { if (!skip(line.text)) text[line.line - first] = line.text; });
    const html = renderNotes(text.join('\n'), onSeek ? durationMs : 0, first);
    // A block's range stops at the first line it doesn't really own: Markdown counts the blank
    // lines after a list item as part of it, and here those can stand for a skipped task or the
    // hidden transcript copy, which an edit must never rewrite.
    const own = new Set(lines.filter(line => !skip(line.text)).map(line => line.line));
    const blocks = html.replace(/<(li|p) data-lines="(\d+),(\d+)">/g, (tag, name, a, b) => {
      let end = +a + 1; while (end < +b && own.has(end)) end++;
      // Blocks with nested lists, links, HTML or code stay read-only (see UNSAFE).
      return `<${name} data-lines="${a},${end}" data-editable="${editable(+a, end)}">`;
    });
    // A decision is one flat bullet: its mark and quote go at the end of its <li>.
    return decisions ? blocks.replace(/(<li data-lines="(\d+),\d+"[^>]*>)([\s\S]*?)<\/li>/g, (whole, open, a, inner) => `${open}${inner}${marked(all[+a]) ? unsureHtml : ''}${quoteHtml(all[+a])}</li>`) : blocks;
  };
  const inline = (text:string) => renderNotes(text, onSeek ? durationMs : 0);
  // "Show all" sits once, on the first section with tasks (Action items), as in the mockup.
  const sections = noteSections(markdown), firstTasks = sections.findIndex(section => section.lines.some(line => taskLine(line.text)));
  // Quick check (mockup): the unconfirmed lines, at most 5, after Action items; editable notes only.
  const rows = check && !locked && !translated ? sections.flatMap(section => CHECKED.test(section.title) ? section.lines.filter(line => /^\s*[-*] /.test(line.text) && check.lines.includes(claimOf(line.text))).map(line => ({line: line.line, claim: claimOf(line.text), decision: DECISIONS.test(section.title)})) : []).slice(0, 5) : [];
  const checkAt = Math.max(0, sections.findIndex(section => /^action items$/i.test(section.title)) + 1);
  const quickCheck = rows.length > 0 && check && <section key="check" className="suggest" aria-label={s.quickCheck}><h4>{s.quickCheck}</h4><div className="cgroup"><b>{s.unsure}</b><span className="tip">{s.unsureTip}</span>{rows.map(row => {
    const ms = citedMs(row.claim), q = said.get(ms), kind = row.decision ? s.unsureDecision : s.unsureTask, time = ms >= 0 ? timeLabel(ms) : '';
    return <div className="sg" key={row.line}><span className="ctask">{row.claim.replace(/\s*\[\d+:\d{2}\]\s*$/, '').replace(/\*\*/g, '')}</span><span className="rcpt"><span className="tip">{time ? s.unsureFrom.replace('{kind}', kind).replace('{time}', time) : s.unsureNoTime.replace('{kind}', kind)}</span>{q && <q>{q.text}</q>}{time && onSeek && ms <= durationMs && <button type="button" className="src" data-ms={ms} title={`Play from ${time}`}>{time}</button>}</span>
      <span className="field"><button type="button" className="btn sm" onClick={() => check.keep(row.claim)}>{s.keep}</button><button type="button" className="btn sm ghost" onClick={() => check.remove(row.claim, row.line, row.decision)}>{s.remove}</button></span></div>;
  })}</div></section>;
  const withCheck = (out: React.ReactNode[]) => { if (quickCheck) out.splice(Math.min(checkAt, out.length), 0, quickCheck); return out; };
  return <div ref={box} className="notes" tabIndex={0} role="document" aria-label={translated ? s.notesTranslatedLabel : locked ? s.notesLockedLabel : s.notesLabel} onClick={event=>{const chip=(event.target as HTMLElement).closest<HTMLButtonElement>('button.src');if(chip){event.preventDefault();onSeek?.(Number(chip.dataset.ms));}}}>{withCheck(sections.map((section,index)=>{
    const tasks = section.lines.filter(line=>{const t=taskLine(line.text);return t&&(!ownerFilter||(t.owner==='Me'?'You':t.owner)===ownerFilter);});
    const kind = translated?.labelOf(section.line) ?? section.title;
    const Heading = LABELS.test(kind) ? 'h3' : 'h4';
    const filtered = ownerFilter && index === firstTasks;
    const head = section.title && (filtered ? <Heading>{section.title} <button type="button" className="doc-action ghost show-all" onClick={()=>setOwnerFilter(null)}>{s.showAll}</button></Heading> : Heading === 'h4' && section.line >= 0
      ? <h4 data-lines={`${section.line},${section.line+1}`} data-kind="heading" data-editable={editable(section.line, section.line+1)} dangerouslySetInnerHTML={{__html:renderInline(section.title)}}/>
      : <Heading>{section.title}</Heading>);
    // The user's own lines, as the mockup's jots: ✎, the line, then its time.
    if (MY_NOTES.test(kind)) return <section key={index}>{head}<ul>{section.lines.filter(line=>line.text.trim()).map(line=>{
      const [, time, text] = /^\s*(?:\[(\d+:\d{2})\]\s*)?(.*)$/.exec(line.text)!;
      return <li key={line.line} data-lines={`${line.line},${line.line+1}`} data-kind="jot" data-editable={editable(line.line, line.line+1)}><span className="jotmark" aria-label="Your note">✎</span><span className="jot-text" dangerouslySetInnerHTML={{__html:renderInline(text, onSeek ? durationMs : 0)}}/>{time&&<> <span className="tip">{time}</span></>}</li>;
    })}</ul></section>;
    return <section key={index}>{head}
      {tasks.length ? <><ul className="todo">{tasks.map(line=>{
        const task = taskLine(line.text)!;
        return <li key={line.line} className={task.done?'done':''} data-lines={`${line.line},${line.line+1}`} data-kind="task" data-editable={editable(line.line, line.line+1)}><input id={`task-${line.line}`} type="checkbox" checked={task.done} disabled={saving || (!!translated && !translated.onToggle)} aria-labelledby={`task-label-${line.line}`} onChange={event=>void toggle(line.line,event.target.checked)}/><span className="tt" id={`task-label-${line.line}`}>{task.owner&&(renaming?.line===line.line
          ? <input className={`owner ownerin ${task.owner==='You'||task.owner==='Me'?'me':''}`} aria-label={s.ownerName} defaultValue={renaming.name} size={Math.max(3, renaming.name.length)} maxLength={40} autoFocus onFocus={event=>event.currentTarget.select()}
              onInput={event=>{event.currentTarget.size=Math.max(3,event.currentTarget.value.length);}}
              onKeyDown={event=>{if(event.key==='Enter'){event.preventDefault();void fixName(renaming.name,line.line,event.currentTarget.value);}if(event.key==='Escape'){event.preventDefault();event.stopPropagation();setRenaming(null);}}}
              onBlur={event=>void fixName(renaming.name,line.line,event.currentTarget.value)}/>
          : <button type="button" className={`owner ${task.owner==='You'||task.owner==='Me'?'me':''}`} aria-haspopup="menu" aria-pressed={ownerFilter===(task.owner==='Me'?'You':task.owner)} onClick={event=>{const r=event.currentTarget.getBoundingClientRect();const name=task.owner==='Me'?'You':task.owner;setOwnerMenu(ownerMenu?.line===line.line?null:{name,line:line.line,x:r.left,y:r.bottom+4});}}>{task.owner==='Me'?'You':task.owner}</button>)}<span className="task-text" dangerouslySetInnerHTML={{__html:inline(task.text)}}/>{marked(line.text)&&<span className="unsure" contentEditable={false} title={s.unsureTitle}>{s.unsure}</span>}</span>{quoteOf(task.text)&&<span className="aq rcpt" contentEditable={false} dangerouslySetInnerHTML={{__html:quoteInner(task.text)}}/>}</li>;
      })}</ul><div dangerouslySetInnerHTML={{__html:body(section.lines, text=>!!taskLine(text))}}/></> : <div dangerouslySetInnerHTML={{__html:body(section.lines, text=>!!ownerFilter&&!!taskLine(text), DECISIONS.test(kind))}}/>}
    </section>;
  }))}<p className="tip notes-hint">{translated ? translated.hint : locked ? s.lockedHint : s.editHint}</p>
    {mention && <PeoplePicker options={people(mention.query)} known={known()} index={mention.index} x={mention.x} y={mention.y} onPick={pick}/>}
    {ownerMenu && <div className="mention gmenu" role="menu" aria-label={ownerMenu.name} style={{left: ownerMenu.x, top: ownerMenu.y}}>
      <button type="button" role="menuitem" onClick={() => { setOwnerFilter(ownerFilter === ownerMenu.name ? null : ownerMenu.name); setOwnerMenu(null); }}>{ownerFilter === ownerMenu.name ? s.showEveryone : s.showOnly.replace('{name}', ownerMenu.name)}</button>
      {!locked && !translated && ownerMenu.name !== 'You' && <button type="button" role="menuitem" onClick={() => { setRenaming({name: ownerMenu.name, line: ownerMenu.line}); setOwnerMenu(null); }}>{s.fixName}</button>}
    </div>}</div>;
}
