"use client";
// Scripts (SPEC 14.4, mockup Script screen): your talk tracks as Markdown files in the library's
// Scripts folder, listed in the sidebar under Scripts. Present opens the prompter strip under the
// camera with the chosen script; the one it shows is also the one ⌃⌥T opens.
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import * as ipc from "@/lib/ipc";
import { strings as s } from "@/lib/strings";
import { keyLabel, nameFrom, parseScript, withLine } from "@/lib/script";
import { meetingDate } from "@/lib/document";
import type { ScriptInfo } from "@/lib/bindings/ScriptInfo";
import type { Rehearsal } from "@/lib/bindings/Rehearsal";
import RehearsalReport, { lastRehearsal } from "./RehearsalReport";
import { MenuItem } from "./MenuIcon";

// "today 16:48", "yesterday 09:14", "Thu 09:14": the library's own date wording.
function edited(ms: number) {
  const [day, clock] = meetingDate(ms).split(" · ");
  return `${day === "Today" || day === "Yesterday" ? day.toLowerCase() : day} ${clock}`;
}
const mmss = (sec: number) => `${Math.floor(sec / 60)}:${String(Math.round(sec % 60)).padStart(2, "0")}`;
// Time to say a section at an even 150 words a minute, until a rehearsal measures yours.
const WPM = 150;

const MIC_ICON = <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" aria-hidden="true"><rect x="6" y="2" width="4" height="8" rx="2" /><path d="M3.5 8a4.5 4.5 0 0 0 9 0M8 12.5V14" /></svg>;
const STRIP_ICON = <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" aria-hidden="true"><rect x="2" y="2.5" width="12" height="5" rx="1.5" /><path d="M4.5 5h5M2 11h12M2 13.5h8" /></svg>;
const LOCK_ICON = <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden="true"><rect x="3" y="7" width="10" height="7" rx="1.5" /><path d="M5.5 7V5a2.5 2.5 0 0 1 5 0v2" /></svg>;

export default function ScriptsPage({ onError, device, pending, onTaken, onCount }: { onError: (message: string) => void; device: string; pending?: Rehearsal | null; onTaken?: () => void; onCount?: (n: number) => void }) {
  const [list, setList] = useState<ScriptInfo[]>([]);
  const [open, setOpen] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [title, setTitle] = useState("");
  const [saving, setSaving] = useState(false);
  const base = useRef<number | null>(null);
  const dirty = useRef(false);
  // The script the strip shows and ⌃⌥T opens, the library folder and the shortcut, for the
  // sidebar key and the footer.
  const [shown, setShown] = useState<string | null>(null);
  const [folder, setFolder] = useState("");
  const [showKey, setShowKey] = useState("");
  useEffect(() => {
    void ipc.getSettings().then(settings => { setShown(settings.prompter.last_script); setFolder(settings.notes_folder); setShowKey(keyLabel(settings.shortcuts.T)); }).catch(() => {});
    const off = ipc.listen<string | null>("motetto://prompter-script", name => { if (name) setShown(name); });
    return () => { void off.then(fn => fn()); };
  }, []);
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
  const [rehearsing, setRehearsing] = useState<number | null>(null);
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const off = ipc.listen<Rehearsal>("motetto://rehearsal", r => setRehearsing(r.duration_ms === null ? Date.now() : null));
    return () => { void off.then(fn => fn()); };
  }, []);
  useEffect(() => { if (rehearsing === null) return; const t = setInterval(() => setNow(Date.now()), 1000); return () => clearInterval(t); }, [rehearsing]);
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
    setList(scripts); onCount?.(scripts.length);
    const pick = select ?? open ?? scripts[0]?.name ?? null;
    if (pick && pick !== open) await choose(pick, scripts);
  }
  async function choose(name: string, scripts = list) {
    if (done && done.script !== name) setDone(null);
    dirty.current = false; setSaving(false); setMenu(false);
    base.current = scripts.find(info => info.name === name)?.modified_ms ?? null;
    setText(await ipc.scriptRead(name));
    setOpen(name); setTitle(name);
  }
  useEffect(() => { refresh().catch(e => onError(String(e))); }, []);
  // Saved 300 ms after you stop typing, like My notes.
  useEffect(() => {
    if (!open || !dirty.current) return;
    const save = setTimeout(() => ipc.scriptSave(open, text, base.current).then(time => { base.current = time; dirty.current = false; setSaving(false); }).catch(e => onError(String(e))), 300);
    return () => clearTimeout(save);
  }, [text, open]);
  function unique(name: string) {
    const taken = new Set(list.map(info => info.name));
    let next = name;
    for (let n = 2; taken.has(next); n++) next = `${name} ${n}`;
    return next;
  }
  async function create() {
    const name = unique(s.untitledScript);
    try { await ipc.scriptSave(name, "", null); await refresh(name); } catch (e) { onError(String(e)); }
  }
  async function duplicate() {
    if (!open) return;
    const name = unique(s.spCopy.replace("{name}", open));
    try { await ipc.scriptSave(name, text, null); await refresh(name); } catch (e) { onError(String(e)); }
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
  const nameField = useRef<HTMLInputElement>(null);
  async function rename() {
    const next = title.trim();
    if (!open || !next || next === open) { setTitle(open ?? ""); return; }
    try { await ipc.scriptRename(open, next); setOpen(next); await refresh(next); } catch (e) { onError(String(e)); setTitle(open); }
  }
  async function remove() {
    if (!open) return;
    try { await ipc.scriptDelete(open); setOpen(null); setText(""); await refresh(); } catch (e) { onError(String(e)); }
  }
  // The ••• menu: Rename, Duplicate, Show in Finder, Delete script.
  const [menu, setMenu] = useState(false);
  const more = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!menu) return;
    const away = (e: PointerEvent) => { if (!more.current?.contains(e.target as Node)) setMenu(false); };
    const key = (e: KeyboardEvent) => { if (e.key === "Escape") setMenu(false); };
    document.addEventListener("pointerdown", away); document.addEventListener("keydown", key);
    return () => { document.removeEventListener("pointerdown", away); document.removeEventListener("keydown", key); };
  }, [menu]);
  // The scripts sit in the library sidebar under Scripts, so the editor keeps the width.
  const [side, setSide] = useState<HTMLElement | null>(null);
  // The sidebar list appears once there is a script, so look again when the count changes.
  useEffect(() => setSide(document.getElementById("scripts-sub")), [list.length]);
  // Many scripts scroll in their own short list (about seven rows), so the meetings stay in
  // reach; the open one is kept in view.
  useEffect(() => { side?.querySelector('[aria-current="true"]')?.scrollIntoView({ block: "nearest" }); }, [side, open]);
  const sidebar = side && list.length ? createPortal(<>
    <div className="sp-subscroll">{list.map(info => <button key={info.name} type="button" className="sp-subitem" aria-current={info.name === open ? "true" : undefined} data-shown={info.name === shown ? "" : undefined} title={info.name === shown && showKey ? s.spShownKey.replace("{key}", showKey) : s.scriptEditedOn.replace("{when}", edited(info.modified_ms))} onClick={() => void choose(info.name).catch(e => onError(String(e)))}><span>{info.name}</span>{showKey && <kbd>{showKey}</kbd>}</button>)}</div>
    <button type="button" className="sp-subitem sp-new" onClick={() => void create()}>+ {s.newScript}</button>
  </>, side) : null;
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
  if (done && open === done.script) return <article className="document scripts sp-main">{sidebar}<RehearsalReport key={done.id} rehearsal={done} text={text} device={device} onUse={use} onAgain={() => void rehearse(done.script)} onEdit={() => setDone(null)} onError={onError} /></article>;
  if (!open) return <article className="document scripts sp-main">{sidebar}</article>;
  const script = parseScript(text);
  const outline = script.sections.map((section, n) => ({
    title: section.title,
    planned: section.minutes * 60,
    say: Math.round(script.lines.filter(line => line.section === n).reduce((sum, line) => sum + line.words.length, 0) / WPM * 60),
  }));
  const say = outline.reduce((sum, x) => sum + x.say, 0), planned = outline.reduce((sum, x) => sum + x.planned, 0);
  const scale = Math.max(1, ...outline.flatMap(x => [x.say, x.planned]));
  const last = lastRehearsal.get(open);
  const path = `${folder.replace(/^\/(Users|home)\/[^/]+/, "~") || "~/Motetto"}/Scripts/${open}.md`;
  return <article className="document scripts sp-main">
    {sidebar}
    <header className="sp-head">
      <div className="sp-titles">
        <input ref={nameField} className="sp-name" aria-label={s.scriptName} value={title} onChange={e => setTitle(e.target.value)} onBlur={() => void rename()} onKeyDown={e => { if (e.key === "Enter") e.currentTarget.blur(); }} />
        <div className="sp-meta">{(script.sections.length === 1 ? s.spMetaOne : s.spMeta).replace("{n}", String(script.sections.length)).replace("{say}", mmss(say)).replace("{planned}", mmss(planned))} · <span className={saving ? undefined : "sp-saved"}>{saving ? s.spSaving : s.spSaved}</span></div>
      </div>
      <div className="sp-actions">
        {rehearsing !== null ? <>
          <span className="sp-live" role="status"><i />{s.spRehearsingFor.replace("{t}", mmss(Math.max(0, (now - rehearsing) / 1000)))}</span>
          <button type="button" className="btn" onClick={() => void ipc.rehearseStop().then(() => {}).catch(e => onError(String(e)))}>{s.rehearsalStopRehearsing}</button>
        </> : <>
          <div className="sp-morewrap" ref={more}>
            <button type="button" className="sp-more" aria-label={s.spMore} title={s.spMore} aria-haspopup="menu" aria-expanded={menu} onClick={() => setMenu(!menu)}>•••</button>
            {menu && <div className="doc-dropdown gmenu" role="menu" aria-label={s.spMore}>
              <MenuItem icon="rename" label={s.spRename} onClick={() => { setMenu(false); nameField.current?.focus(); nameField.current?.select(); }} />
              <MenuItem icon="copy" label={s.spDuplicate} onClick={() => { setMenu(false); void duplicate(); }} />
              <MenuItem icon="folder" label={device === "Mac" ? s.spShowFolder : s.spShowFolderWin} onClick={() => { setMenu(false); void ipc.scriptReveal().catch(e => onError(String(e))); }} />
              <div className="sep" />
              <MenuItem icon="trash" label={s.deleteScript} danger onClick={() => { setMenu(false); void remove(); }} />
            </div>}
          </div>
          <button type="button" className="btn" onClick={() => void rehearse(open)}>{MIC_ICON}{s.rehearse}</button>
          <button type="button" className="btn primary" onClick={() => void ipc.prompterShow(open).catch(e => onError(String(e)))}>{STRIP_ICON}{s.present}</button>
        </>}
      </div>
    </header>
    {rehearsing !== null && <div className="sp-note">{MIC_ICON}<span>{s.spRehearsingNote.replace("{device}", device)}</span></div>}
    <div className="sp-body">
      <div className="sp-editor">
        <textarea ref={editor} aria-label={open} spellCheck value={text} onChange={e => { dirty.current = true; setSaving(true); setText(e.target.value); }} />
        <div className="sp-legend"><span><code># Opening (2m)</code>{s.spLegendSection}</span><span><code>[click New]</code>{s.spLegendCue}</span><span><code>{s.spLegendBlankCode}</code>{s.spLegendBlank}</span></div>
      </div>
      <aside className="sp-outline" aria-label={s.spOutline}>
        <div className="sp-lab">{s.spOutline} <span>{s.spOutlineKey}</span></div>
        {outline.map((x, n) => { const over = x.planned > 0 && x.say > x.planned; return <div className="sp-sec" key={n}>
          <span className="sp-n">{n + 1}</span><span className="sp-t">{x.title}</span><span className={`sp-d${over ? " over" : ""}`}>{mmss(x.say)} / {mmss(x.planned)}</span>
          <div className="sp-bar"><i style={{ width: `${x.planned / scale * 100}%` }} /><em className={over ? "over" : undefined} style={{ width: `${x.say / scale * 100}%` }} /></div>
        </div>; })}
        <p className="sp-est">{s.spEstimate}</p>
        {last && rehearsing === null && <div className="sp-card"><div className="sp-lab">{s.spLast}</div><div className="sp-cardrow"><b>{mmss(last.totalS)}</b><span>{s.spLastOf.replace("{planned}", mmss(planned)).replace("{when}", edited(last.at))}</span></div></div>}
      </aside>
    </div>
    <footer className="sp-foot">{LOCK_ICON}{s.spFoot.replace("{path}", path).replace("{device}", device)}</footer>
  </article>;
}
