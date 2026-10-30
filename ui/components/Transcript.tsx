import { useEffect, useRef, useState } from "react";
import { transcriptLines } from "@/lib/transcript";
import { elapsedLabel } from "@/lib/session";
import { strings as s } from "@/lib/strings";
import * as ipc from "@/lib/ipc";
import { handBack } from "@/lib/focus";

const esc = (text: string) => text.replace(/[&<>"]/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]!));
const READ_MS = 1500;

// As in the mockup: the time plays from its line; the words are the line's text, edited in place
// (SPEC 7.11). The speaker and time never change; an emptied line is removed but keeps its place,
// and what was heard stays one click away (Revert).
export default function Transcript({id, text, heard, marks, locked = false, onSeek, onReplay, onTogglePlay, onChange, onError, position, running = false}: {id: string; text: string; heard: string; marks: number[]; locked?: boolean; onSeek?: (ms: number) => void; onReplay?: (start: number, end: number) => void; onTogglePlay?: () => void; onChange: (text: string, heard: string) => void; onError: (error: string) => void; position?: number; running?: boolean}) {
  const lines = transcriptLines(text, marks);
  const heardLines = heard ? transcriptLines(heard, []) : null;
  const [focused, setFocused] = useState(-1);
  const playing = position === undefined ? -1 : lines.findLastIndex(line => line.ms !== null && line.ms <= position);
  const box = useRef<HTMLDivElement>(null);
  const history = useRef<{index: number; text: string}[]>([]);
  // Playback never pulls the view or the highlight onto the line being edited. Opening at a cited
  // time, or a playing line leaving the view, puts the line at the top so what plays next shows
  // below it (owner 2026-10-21); a line already in view stays put.
  // While the reader scrolls by hand the highlight keeps moving but the view is theirs; once the
  // scrolling has stopped for READ_MS (momentum included) the view goes back to the playing line
  // (owner 2026-10-25). A seek ends it at once. Wheel and touch are the hand; scroll events while
  // a pointer is down are the scroll bar; scrollIntoView sends neither. With the audio stopped the
  // view never follows (owner 2026-10-25); pressing Play brings the playing line back into view.
  const opened = useRef(false), smooth = useRef(false), last = useRef(position);
  const [reading, setReading] = useState(false);
  useEffect(() => {
    const body = box.current?.closest<HTMLElement>(".docbody");
    if (!body) return;
    let held = false, timer = 0;
    const hand = () => { setReading(true); clearTimeout(timer); timer = window.setTimeout(() => { smooth.current = true; setReading(false); }, READ_MS); };
    const down = () => { held = true; }, up = () => { held = false; };
    const bar = () => { if (held) hand(); };
    body.addEventListener("wheel", hand, {passive: true}); body.addEventListener("touchmove", hand, {passive: true});
    body.addEventListener("pointerdown", down); addEventListener("pointerup", up); body.addEventListener("scroll", bar, {passive: true});
    return () => {
      clearTimeout(timer);
      body.removeEventListener("wheel", hand); body.removeEventListener("touchmove", hand);
      body.removeEventListener("pointerdown", down); removeEventListener("pointerup", up); body.removeEventListener("scroll", bar);
    };
  }, []);
  // A seek (back, or ahead by more than a moment) means the reader wants that place now.
  useEffect(() => {
    if (position !== undefined && last.current !== undefined && (position < last.current || position - last.current > 2000)) setReading(false);
    last.current = position;
  }, [position]);
  useEffect(() => {
    const row = playing >= 0 && focused < 0 ? box.current?.children[playing] : undefined;
    if (!row || (reading || !running) && opened.current) return;
    const view = row.closest(".docbody")?.getBoundingClientRect(), r = row.getBoundingClientRect();
    const under = parseFloat(getComputedStyle(row).scrollMarginBottom) || 0;
    if (!opened.current || !view || r.top < view.top || r.bottom > view.bottom - under)
      row.scrollIntoView({block: "start", behavior: smooth.current && !matchMedia("(prefers-reduced-motion: reduce)").matches ? "smooth" : "auto"});
    smooth.current = false; opened.current = true;
  }, [playing, focused, reading, running]);

  async function save(index: number, value: string, undoable = true) {
    const before = lines[index]?.text ?? "";
    if (value === before) return;
    // What was heard is the transcript before the first edit; once every line matches it again, there is no edit (as transcript_edits.rs).
    try { const next = await ipc.saveTranscriptLine(id, index, value), was = heard || text; onChange(next, next === was ? "" : was); if (undoable) history.current.push({index, text: before}); }
    catch (error) {
      onError(String(error));
      const words = box.current?.querySelector<HTMLElement>(`[data-line="${index}"] .words`); if (words) words.textContent = before;
    }
  }
  const words = (index: number) => box.current?.querySelector<HTMLElement>(`[data-line="${index}"] .words`);
  function caretAt(el: HTMLElement, start: boolean) {
    el.focus(); const r = document.createRange(); r.selectNodeContents(el); r.collapse(start);
    const sel = getSelection(); sel?.removeAllRanges(); sel?.addRange(r);
  }
  // The keyboard (owner 2026-10-20): ↑↓ move between lines, Space plays or pauses, ↩ edits the
  // line, ⌥↩ plays it. A line is a focusable row, so a line with no time (disabled button) counts.
  const row = (index: number) => box.current?.querySelector<HTMLElement>(`[data-line="${index}"]`);
  function go(index: number) {
    const target = row(Math.max(0, Math.min(lines.length - 1, index)));
    target?.focus({preventScroll: true}); target?.scrollIntoView({block: "nearest"});
  }
  // While editing: ↑ on the first visual line goes to the line above (caret at its end), ↓ on the
  // last to the line below, as the notes do; elsewhere the caret moves as usual.
  function atEdge(el: HTMLElement, up: boolean) {
    const sel = getSelection(); if (!sel?.rangeCount) return true;
    const caret = sel.getRangeAt(0).cloneRange(); caret.collapse(up);
    const at = caret.getClientRects()[0]; if (!at) return true;
    const edge = el.getBoundingClientRect(), line = parseFloat(getComputedStyle(el).lineHeight) || 20;
    return up ? at.top - edge.top < line * 0.75 : edge.bottom - at.bottom < line * 0.75;
  }
  function editNext(from: number, step: 1 | -1) {
    for (let i = from + step; i >= 0 && i < lines.length; i += step) { const w = words(i); if (w) { caretAt(w, step === 1); return true; } }
    return false;
  }
  // Space and the arrows also work before anything in the list has focus.
  useEffect(() => {
    const keys = (e: KeyboardEvent) => {
      const arrow = e.key === "ArrowDown" || e.key === "ArrowUp";
      const tab = e.target instanceof Element && e.target.matches('#tab-Transcript[aria-selected="true"]');
      if ((e.target !== document.body && !(tab && arrow)) || e.metaKey || e.ctrlKey || e.altKey) return;
      if (e.key === " ") { e.preventDefault(); onTogglePlay?.(); }
      else if (e.key === "ArrowDown" || e.key === "ArrowUp") { e.preventDefault(); go(playing >= 0 ? playing : 0); }
    };
    document.addEventListener("keydown", keys);
    return () => document.removeEventListener("keydown", keys);
  });
  // ⌘Z / Ctrl+Z with no caret in a line undoes the last saved line edit.
  useEffect(() => {
    const undo = (e: KeyboardEvent) => {
      if (e.key.toLowerCase() !== "z" || !(e.metaKey || e.ctrlKey) || e.shiftKey || e.altKey || !history.current.length) return;
      const active = document.activeElement as HTMLElement | null;
      if (active?.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(active?.tagName ?? "")) return;
      e.preventDefault(); const last = history.current.pop()!; void save(last.index, last.text, false);
    };
    document.addEventListener("keydown", undo);
    return () => document.removeEventListener("keydown", undo);
  });

  const editable = !locked;
  return <div className="transcript-edit">
    <div ref={box} className="tlist" tabIndex={0} role="document" aria-label={locked ? s.transcriptLockedLabel : s.transcriptLabel}
      onKeyDown={event => {
        const target = event.target as HTMLElement;
        if (target.isContentEditable || target.closest(".epop, .rmv") || event.metaKey || event.ctrlKey) return;
        const at = target.closest<HTMLElement>(".tline") ? Number(target.closest<HTMLElement>(".tline")!.dataset.line) : -1;
        const here = at >= 0 ? at : playing >= 0 ? playing : 0;
        if (event.key === "ArrowDown" || event.key === "ArrowUp") { event.preventDefault(); go(at < 0 ? here : here + (event.key === "ArrowDown" ? 1 : -1)); }
        else if (event.key === " ") { event.preventDefault(); onTogglePlay?.(); }
        else if (event.key === "Enter" && event.altKey) { event.preventDefault(); const line = lines[here], next = lines.slice(here + 1).find(l => l.ms !== null)?.ms; if (line?.ms != null) onReplay?.(line.ms, next ?? line.ms + 8000); }
        else if (event.key === "Enter" && (at >= 0 || target === box.current)) { event.preventDefault(); const w = words(here); if (w) caretAt(w, false); }
      }}>
      {lines.map((line, index) => {
        const side = line.speaker === "You" ? "me" : "them", original = heardLines?.[index]?.text ?? line.text;
        const removed = !line.text.trim() && !!original.trim(), edited = !!heardLines && line.text !== original;
        const next = lines.slice(index + 1).find(l => l.ms !== null)?.ms;
        const seek = onSeek && line.ms !== null ? () => onSeek(line.ms!) : undefined;
        return <div className={`tline ${side}${index === playing && index !== focused ? " playing" : ""}${edited ? " edited" : ""}${removed ? " removed" : ""}`} key={index} data-line={index} tabIndex={-1}>
          <button className="ts" disabled={!seek} onClick={seek} aria-label={`Play from ${line.time}`}>{line.time}</button><span className={`who ${side}`}>{line.speaker}</span>
          <span className="tx">
            {removed
              ? <span className="rmv"><s>{original}</s><span>{s.lineRemoved}</span>{editable && <button className="lnk" onClick={() => void save(index, original)}>{s.revert}</button>}</span>
              : <span className={`words${editable ? "" : " ro"}`} contentEditable={editable ? "plaintext-only" : undefined} suppressContentEditableWarning spellCheck={false}
                  role={editable ? "textbox" : undefined} aria-label={editable ? `${s.lineAt} ${line.time}, ${line.speaker}` : undefined}
                  dangerouslySetInnerHTML={{__html: esc(line.text)}}
                  onFocus={() => setFocused(index)}
                  onBlur={event => { setFocused(-1); void save(index, (event.currentTarget.textContent ?? "").replace(/\s+/g, " ").trim()); }}
                  onPaste={event => { event.preventDefault(); document.execCommand("insertText", false, event.clipboardData.getData("text/plain").replace(/\s+/g, " ")); }}
                  onDrop={event => event.preventDefault()}
                  onKeyDown={event => {
                    const el = event.currentTarget;
                    if (event.key === "Enter" && event.altKey) { event.preventDefault(); if (line.ms !== null) onReplay?.(line.ms, next ?? line.ms + 8000); return; }
                    if (event.key === "Enter") { event.preventDefault(); el.blur(); const w = words(index + 1); if (w) caretAt(w, true); return; }
                    if ((event.key === "ArrowUp" || event.key === "ArrowDown") && !event.shiftKey && !event.altKey && !event.metaKey && !event.ctrlKey) {
                      const up = event.key === "ArrowUp";
                      if (atEdge(el, up)) { event.preventDefault(); el.blur(); if (!editNext(index, up ? -1 : 1)) caretAt(el, !up); }
                      return;
                    }
                    // Esc leaves the line selected, so ↑↓ carry on from it.
                    if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); const back = handBack(el.closest<HTMLElement>(".tline")); el.blur(); back(); }
                  }}/>}
            <span className="tx-end">{line.marks.map(mark => <span className="flag" key={mark} title={`${s.marked} ${elapsedLabel(mark)}`}>⚑ {s.mark}</span>)}
              {edited && !removed && <span className="edw"><button className="edot" aria-label={`${s.lineEdited} ${original}`}/><span className="epop" role="tooltip"><span>{s.lineEdited} <q>{original}</q></span>{editable && <button className="doc-action" onMouseDown={event => event.preventDefault()} onClick={() => void save(index, original)}>{s.revert}</button>}</span></span>}
            </span>
          </span>
        </div>;
      })}
    </div>
  </div>;
}
