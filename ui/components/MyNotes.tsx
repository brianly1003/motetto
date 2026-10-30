"use client";
import { useEffect, useRef, useState } from "react";
import * as ipc from "@/lib/ipc";
import { elapsedLabel } from "@/lib/session";
import { matchPeople, mentionWord } from "@/lib/people";
import PeoplePicker from "./PeoplePicker";
import { Kbd } from "@/components/ui/Kbd";

export function MyNotes({ meetingId, elapsed, registerFlush, hints = true }: { meetingId: string; elapsed: number; registerFlush: (flush: () => Promise<void>) => void; hints?: boolean }) {
  const editor = useRef<HTMLDivElement>(null);
  const [status, setStatus] = useState("Loading…");
  const [ready, setReady] = useState(false);
  const version = useRef(0);
  const saved = useRef("");
  const draft = useRef("");
  const loaded = useRef(false);
  const finalFlush = useRef<() => Promise<void>>(async () => {});
  const queue = useRef(Promise.resolve());
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  // The @ picker (SPEC 7.11), as in the notes: You, then everyone already @-named in these notes.
  // A picked name is written as one word ("@Linh_Tran"), which the summary turns into the owner.
  const [mention, setMention] = useState<{node: Text; start: number; query: string; x: number; y: number; index: number} | null>(null);
  // Names already @-named here, minus the one being typed now (it would offer itself).
  const known = (typing?: string) => {
    const names = [...(editor.current?.textContent ?? "").matchAll(/@([\p{L}\p{N}_]+)/gu)].map(m => m[1].replace(/_/g, " "));
    const own = typing ? names.indexOf(typing) : -1; if (own >= 0) names.splice(own, 1);
    return [...new Set(["You", ...names])];
  };
  function watchMention() {
    const sel = getSelection(), node = sel?.focusNode;
    const at = node?.nodeType === Node.TEXT_NODE && /(?:^|\s)@([^@\n[\]:*`<_]{0,40})$/.exec(node.nodeValue!.slice(0, sel!.focusOffset)); // "_" means already picked
    // A picked name stays as text here, so a space ends the mention unless it continues a known
    // name ("@Linh T" → Linh Tran); "@Kevin now" is Kevin plus words, not a new person.
    if (!at || (/\s/.test(at[1]) && !known(at[1]).some(name => name.toLowerCase().startsWith(at[1].toLowerCase())))) { setMention(null); return; }
    const start = sel!.focusOffset - at[1].length - 1, r = document.createRange(); r.setStart(node!, start); r.setEnd(node!, start + 1);
    const box = r.getBoundingClientRect();
    setMention(m => ({node: node as Text, start, query: at[1], x: box.left, y: box.bottom + 4, index: m?.node === node && m.start === start ? m.index : 0}));
  }
  function pick(name: string) {
    const m = mention; setMention(null); if (!m) return;
    const r = document.createRange(); r.setStart(m.node, m.start); r.setEnd(m.node, Math.min(m.node.length, m.start + 1 + m.query.length));
    const sel = getSelection(); sel?.removeAllRanges(); sel?.addRange(r);
    document.execCommand("insertText", false, `${mentionWord(name)} `);
    changed();
  }
  function content() {
    // Editable text keeps a trailing space as U+00A0; my-notes.md gets a plain space.
    return Array.from(editor.current?.children ?? []).map(row => `[${(row as HTMLElement).dataset.t || elapsedLabel(elapsed)}] ${(row.textContent ?? "").replace(/\u00a0/g, " ")}`).join("\n");
  }
  function flush() {
    clearTimeout(timer.current);
    queue.current = queue.current.catch(() => {}).then(async () => {
      if (!loaded.current) return;
      const value = draft.current;
      if (value === saved.current) return;
      setStatus("Saving…");
      try {
        const result = await ipc.saveMyNotes(meetingId, value, version.current);
        version.current = result.version;
        saved.current = value;
        setStatus("Saved");
      } catch (error) { setStatus(String(error)); throw error; }
    });
    return queue.current;
  }
  useEffect(() => { finalFlush.current = flush; registerFlush(flush); });
  useEffect(() => {
    let active = true;
    ipc.getMyNotes(meetingId).then(notes => {
      if (!active || !editor.current) return;
      version.current = notes.version;
      editor.current.replaceChildren(...(notes.text.split("\n").filter(Boolean).length ? notes.text.split("\n").filter(Boolean) : [""]).map(line => {
        const row = document.createElement("div");
        const match = /^\[(\d+:\d+)\] ?(.*)$/.exec(line);
        row.dataset.t = match?.[1] ?? elapsedLabel(elapsed);
        row.textContent = match?.[2] ?? line;
        if (!row.textContent) row.append(document.createElement("br"));
        decorate(row);
        return row;
      }));
      saved.current = content(); draft.current = saved.current; loaded.current = true;
      setReady(true); setStatus("Saved");
    }).catch(error => { if (active) setStatus(String(error)); });
    return () => { active = false; clearTimeout(timer.current); void finalFlush.current().catch(() => {}); };
    // A meeting owns the editor. Clock ticks must not replace an unsaved draft.
  }, [meetingId]);
  function decorate(row: HTMLElement) {
    const text = row.textContent ?? "";
    row.className = text.startsWith("[]") ? "act" : text.startsWith("!") ? "dec" : text.startsWith("?") ? "q" : text.startsWith("#") ? "h" : "";
  }
  function newLine() {
    document.execCommand("insertParagraph");
    const selection = window.getSelection();
    const element = selection?.anchorNode?.nodeType === Node.ELEMENT_NODE ? selection.anchorNode as HTMLElement : selection?.anchorNode?.parentElement;
    const row = element?.closest(".editor > div") as HTMLElement | null;
    if (row) row.dataset.t = elapsedLabel(elapsed);
    changed();
  }
  function changed() {
    for (const child of Array.from(editor.current?.children ?? [])) {
      const row = child as HTMLElement;
      if (!row.dataset.t) row.dataset.t = elapsedLabel(elapsed);
      decorate(row);
    }
    draft.current = content();
    setStatus("Saving…"); clearTimeout(timer.current);
    timer.current = setTimeout(() => { void flush().catch(() => {}); }, 300);
  }
  // On the recording page the tab already names it (mockup); the status stays for screen readers.
  return <>{hints ? <div className="panehead mine"><span>My notes</span><span className="tip" role="status">{status}</span></div> : <span className="sr-only" role="status">{status}</span>}<div ref={editor} className="editor" contentEditable={ready} suppressContentEditableWarning spellCheck={false} role="textbox" aria-multiline="true" aria-label="My notes. Each line keeps the time you wrote it." onInput={() => { changed(); watchMention(); }} onKeyDown={event => {
    if (mention) {
      const options = matchPeople(known(mention.query), mention.query);
      if (event.key === "ArrowDown" || event.key === "ArrowUp") { event.preventDefault(); setMention({...mention, index: (mention.index + (event.key === "ArrowDown" ? 1 : -1) + options.length) % Math.max(1, options.length)}); return; }
      if ((event.key === "Enter" || event.key === "Tab") && options.length) { event.preventDefault(); pick(options[Math.min(mention.index, options.length - 1)]); return; }
      if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); setMention(null); return; }
    }
    if (event.key === "Enter") { event.preventDefault(); newLine(); }
  }} onBlur={() => { setMention(null); void flush().catch(() => {}); }} onPaste={event => {
    event.preventDefault();
    event.clipboardData.getData("text/plain").split(/\r?\n/).forEach((line, index) => { if (index) newLine(); document.execCommand("insertText", false, line); });
    changed();
  }} />{mention && <PeoplePicker options={matchPeople(known(mention.query), mention.query)} known={known(mention.query)} index={mention.index} x={mention.x} y={mention.y} onPick={pick}/>}{hints && <div className="hints"><code>[]</code> action <code>!</code> decision <code>?</code> question <code>#</code> topic <code>@</code> owner <span>· <Kbd>Esc</Kbd> back to the call</span></div>}</>;
}
