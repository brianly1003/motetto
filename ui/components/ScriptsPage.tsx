"use client";
// Scripts (SPEC 14.4): your talk tracks as Markdown files in the library's Scripts folder.
// Present opens the prompter strip under the camera with the chosen script.
import { useEffect, useRef, useState } from "react";
import * as ipc from "@/lib/ipc";
import { strings as s } from "@/lib/strings";
import { nameFrom, parseScript, withLine } from "@/lib/script";
import { meetingDate } from "@/lib/document";
import type { ScriptInfo } from "@/lib/bindings/ScriptInfo";
import type { Rehearsal } from "@/lib/bindings/Rehearsal";
import RehearsalReport from "./RehearsalReport";

// "today 16:48", "yesterday 09:14", "Thu 09:14": the library's own date wording.
function edited(ms: number) {
  const [day, clock] = meetingDate(ms).split(" · ");
  return `${day === "Today" || day === "Yesterday" ? day.toLowerCase() : day} ${clock}`;
}

export default function ScriptsPage({ onError, device, pending, onTaken }: { onError: (message: string) => void; device: string; pending?: Rehearsal | null; onTaken?: () => void }) {
  const [list, setList] = useState<ScriptInfo[]>([]);
  const [open, setOpen] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [title, setTitle] = useState("");
  const base = useRef<number | null>(null);
  const dirty = useRef(false);
  // A finished rehearsal: the library opens here on its script and shows the report; leaving
  // the report (another script, Edit script) deletes the audio unless it was kept.
  const [done, setDone] = useState<Rehearsal | null>(null);
  useEffect(() => {
    if (!pending) return;
    // Taken once: leaving this page deletes the audio, so coming back must not reopen its report.
    onTaken?.();
    void refresh(pending.script).then(() => setDone(pending)).catch(e => onError(String(e)));
  }, [pending?.id]);
  // Rehearse turns into Stop while one runs, so a second press never starts another.
  const [rehearsing, setRehearsing] = useState(false);
  useEffect(() => {
    const off = ipc.listen<Rehearsal>("motetto://rehearsal", r => setRehearsing(r.duration_ms === null));
    return () => { void off.then(fn => fn()); };
  }, []);
  async function rehearse(name: string) {
    setDone(null);
    try { await ipc.rehearseStart(name); } catch (e) { onError(String(e)); }
  }
  // "Use in script": that line replaced (its cues kept), saved at once against the loaded
  // time; Undo puts the text back the same way.
  async function save(next: string) {
    if (!open) return;
    try { base.current = await ipc.scriptSave(open, next, base.current); setText(next); }
    catch (e) { onError(String(e)); throw e; }
  }
  const use = (k: number, to: string) => { const before = text; return save(withLine(text, k, to)).then(() => async () => save(before)); };
  async function refresh(select?: string) {
    const answer = await ipc.scriptsList();
    // A failed list must not take the library window down with it.
    const scripts = Array.isArray(answer) ? answer : [];
    setList(scripts);
    const pick = select ?? open ?? scripts[0]?.name ?? null;
    if (pick && pick !== open) await choose(pick, scripts);
  }
  async function choose(name: string, scripts = list) {
    if (done && done.script !== name) setDone(null);
    dirty.current = false;
    base.current = scripts.find(info => info.name === name)?.modified_ms ?? null;
    setText(await ipc.scriptRead(name));
    setOpen(name); setTitle(name);
  }
  useEffect(() => { refresh().catch(e => onError(String(e))); }, []);
  // Saved 300 ms after you stop typing, like My notes.
  useEffect(() => {
    if (!open || !dirty.current) return;
    const save = setTimeout(() => ipc.scriptSave(open, text, base.current).then(time => { base.current = time; dirty.current = false; }).catch(e => onError(String(e))), 300);
    return () => clearTimeout(save);
  }, [text, open]);
  async function create() {
    const taken = new Set(list.map(info => info.name));
    let name = s.untitledScript;
    for (let n = 2; taken.has(name); n++) name = `${s.untitledScript} ${n}`;
    try { await ipc.scriptSave(name, "", null); await refresh(name); } catch (e) { onError(String(e)); }
  }
  // First use: the paste box becomes the first script, named from its first line, and typing
  // carries on in the editor with the caret where it was.
  const caret = useRef<number | null>(null);
  async function first(pasted: string, at: number) {
    const name = nameFrom(pasted, new Set(list.map(info => info.name)), s.untitledScript);
    try { caret.current = at; await ipc.scriptSave(name, pasted, null); await refresh(name); } catch (e) { onError(String(e)); }
  }
  const editor = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    if (caret.current === null || !editor.current) return;
    editor.current.focus(); editor.current.setSelectionRange(caret.current, caret.current); caret.current = null;
  }, [open]);
  async function rename() {
    const next = title.trim();
    if (!open || !next || next === open) { setTitle(open ?? ""); return; }
    try { await ipc.scriptRename(open, next); setOpen(next); await refresh(next); } catch (e) { onError(String(e)); setTitle(open); }
  }
  async function remove() {
    if (!open) return;
    try { await ipc.scriptDelete(open); setOpen(null); setText(""); await refresh(); } catch (e) { onError(String(e)); }
  }
  if (!list.length && !open) return <article className="document scripts">
    <div className="scripts-first">
      <h1 className="scripts-first-title">{s.scriptsFirstTitle}</h1>
      <p className="scripts-first-lead">{s.scriptsFirstLead}</p>
      <textarea className="scripts-text scripts-first-box" aria-label={s.scriptsFirstLabel} placeholder={s.scriptsFirstPlaceholder} spellCheck autoFocus onChange={e => { const value = e.target.value; if (value.trim()) void first(value, e.target.selectionStart); }} />
      <ol className="scripts-steps">
        {[[s.scriptsStep1, s.scriptsStep1Body], [s.scriptsStep2, s.scriptsStep2Body], [s.scriptsStep3, s.scriptsStep3Body]].map(([title, body], n) => <li key={title}><span className="scripts-step-n" aria-hidden="true">{n + 1}</span><div><b>{title}</b><span>{body}</span></div></li>)}
      </ol>
    </div>
  </article>;
  return <article className="document scripts">
    <div className="scripts-body">
      <nav className="scripts-list" aria-label={s.scripts}>
        {list.map(info => <button key={info.name} type="button" aria-current={info.name === open ? "true" : undefined} onClick={() => void choose(info.name).catch(e => onError(String(e)))}>{info.name}<small>{info.name === open ? (n => (n === 1 ? s.scriptEditedOne : s.scriptEdited).replace("{when}", edited(info.modified_ms)).replace("{n}", String(n)))(parseScript(text).sections.length) : s.scriptEditedOn.replace("{when}", edited(info.modified_ms))}</small></button>)}
        <button type="button" onClick={() => void create()}>+ {s.newScript}</button>
      </nav>
      {done && open === done.script ? <RehearsalReport key={done.id} rehearsal={done} text={text} device={device} onUse={use} onAgain={() => void rehearse(done.script)} onEdit={() => setDone(null)} onError={onError} />
      : open ? <div className="scripts-pane">
        <div className="scripts-bar">
          <input className="scripts-title" aria-label={s.scriptName} value={title} onChange={e => setTitle(e.target.value)} onBlur={() => void rename()} onKeyDown={e => { if (e.key === "Enter") e.currentTarget.blur(); }} />
          <button type="button" className="btn ghost" onClick={() => void remove()}>{s.deleteScript}</button>
          <button type="button" className="btn" aria-pressed={rehearsing} onClick={() => void (rehearsing ? ipc.rehearseStop().then(() => {}).catch(e => onError(String(e))) : rehearse(open))}>{rehearsing ? s.rehearsalStopRehearsing : s.rehearse}</button>
          <button type="button" className="btn primary" onClick={() => void ipc.prompterShow(open).catch(e => onError(String(e)))}>{s.present}</button>
        </div>
        <p className="scripts-hint">{s.scriptHelp}</p>
        <textarea ref={editor} className="scripts-text" aria-label={open} spellCheck value={text} onChange={e => { dirty.current = true; setText(e.target.value); }} />
        <p className="scripts-hint">{s.scriptSaved.replace("{path}", `Scripts/${open}.md`).replace("{device}", device)}</p>
      </div> : null}
    </div>
  </article>;
}
