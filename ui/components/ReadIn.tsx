"use client";
import { useEffect, useMemo, useRef, useState } from "react";
import type { Meeting } from "@/lib/bindings/Meeting";
import * as ipc from "@/lib/ipc";
import { englishName, matches, nativeName, NOTE_LANGUAGES, systemLanguages, withTicks } from "@/lib/languages";
import { strings as s } from "@/lib/strings";
import { Alert } from "./Alert";
import { MenuIcon } from "./MenuIcon";

// Read in (owner 2026-10-26): notes.md stays the one you edit; each language is a saved
// translation beside it, made on request by the local notes model. The language you pick is
// remembered for every meeting: one saved in it opens in it, one not yet asks once.
const PREFERRED = "motetto.readIn", RECENT = "motetto.readIn.recent";
function stored(key: string) { try { return localStorage.getItem(key); } catch { return null; } }
function store(key: string, value: string | null) { try { if (value === null) localStorage.removeItem(key); else localStorage.setItem(key, value); } catch { /* private window: not remembered */ } }
const recent = (): string[] => { try { return JSON.parse(stored(RECENT) ?? "[]").filter((code: string) => (NOTE_LANGUAGES as readonly string[]).includes(code)); } catch { return []; } };

export function TranslateIcon() {
  return <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M2 3.5h6.5M5.25 2v1.5M7.2 3.5C6.6 6 4.9 7.9 2.5 8.8M3.9 5.6c.8 1.4 2 2.5 3.4 3.1"/><path d="m8.6 14 2.7-6.3L14 14M9.5 12h3.6"/></svg>;
}

export type ReadIn = ReturnType<typeof useReadIn>;

export function useReadIn(meeting: Meeting, onChange: (meeting: Meeting) => void, onError: (error: string) => void, onToast?: (text: string) => void) {
  const id = meeting.summary.id;
  // The notes may already be written in a language (Write in): reading in it is the original.
  const written = meeting.summary.notes_language ?? null;
  const desktop = ipc.desktopAvailable();
  const [saved, setSavedState] = useState<ipc.Translation[]>([]);
  // Kept as it was when nothing changed: a re-render moves Find's place in the page (DocumentFind).
  const setSaved = (found: ipc.Translation[]) => setSavedState(was => JSON.stringify(was) === JSON.stringify(found) ? was : found);
  const [view, setView] = useState<string | null>(null);
  const [shown, setShown] = useState<ipc.Translation | null>(null);
  const [running, setRunning] = useState<ipc.TranslateProgress | null>(null);
  const [failed, setFailed] = useState<{ code: string; message: string } | null>(null);
  const [preferred, setPreferred] = useState<string | null>(() => stored(PREFERRED));
  const [dismissed, setDismissed] = useState(false);
  // The language this page asked for: it opens when it lands (as does the remembered one).
  const wanted = useRef<string | null>(null);
  const live = useRef({ id, preferred }); live.current = { id, preferred };

  // A list or nothing: a missing list (an older fixture, a failed read) is no saved languages.
  async function list() { try { const found = await ipc.notesTranslations(id); return Array.isArray(found) ? found : []; } catch { return []; } }
  useEffect(() => {
    if (!desktop) return;
    let active = true;
    setView(null); setShown(null); setFailed(null); setDismissed(false); wanted.current = null;
    void list().then(found => { if (!active) return; setSaved(found); const p = stored(PREFERRED); if (p && p !== written && found.some(t => t.code === p)) setView(p); });
    void ipc.translating().then(now => { if (active) setRunning(now || null); }).catch(() => {});
    return () => { active = false; };
  }, [id]);
  // Edits to the notes can make the translation stale; ticks don't (translations.rs `source`).
  useEffect(() => {
    if (!desktop) return;
    let active = true;
    void list().then(found => { if (active) setSaved(found); });
    if (view) void ipc.getTranslation(id, view).then(t => { if (active) setShown(t); }).catch(error => { if (active) onError(String(error)); });
    return () => { active = false; };
  }, [id, view, meeting.notes_md]);
  useEffect(() => {
    if (!desktop) return;
    let off: (() => void) | undefined, active = true;
    void ipc.onTranslate(progress => {
      if (!active) return;
      setRunning(progress.done ? null : progress);
      if (!progress.done || progress.id !== live.current.id) return;
      void list().then(found => { if (active) setSaved(found); });
      if (progress.error === ipc.STOPPED) onToast?.(s.stoppedTranslation);
      else if (progress.error) setFailed({ code: progress.code, message: progress.error });
      else if (wanted.current === progress.code || live.current.preferred === progress.code) { setFailed(null); setView(progress.code); }
      if (wanted.current === progress.code) wanted.current = null;
    }).then(stop => { if (active) off = stop; else stop(); });
    return () => { active = false; off?.(); };
  }, []);

  function remember(code: string | null) {
    setPreferred(code); store(PREFERRED, code);
    if (code) store(RECENT, JSON.stringify([code, ...recent().filter(c => c !== code)].slice(0, 3)));
  }
  function translate(code: string) {
    wanted.current = code; setFailed(null); setDismissed(true);
    // A run's own failure arrives as its last event too; this catches the ones refused before it starts.
    ipc.translateNotes(id, code).catch(error => { if (String(error) !== ipc.STOPPED && live.current.id === id) setFailed({ code, message: String(error) }); });
  }
  /** Pick from Read in: null is the original, and you stop reading meetings in a language. */
  function choose(code: string | null) {
    remember(code);
    if (!code || code === written) { setView(null); return; }
    if (saved.some(t => t.code === code)) { setFailed(null); setView(code); } else translate(code);
  }
  const busyHere = running && running.id === id ? running : null;
  const viewing = view && view !== written ? view : null;
  const translation = viewing && shown?.code === viewing ? shown : null;
  async function toggle(line: number, done: boolean) {
    try { onChange({ ...meeting, notes_md: await ipc.setActionItemDone(id, line, done, meeting.notes_md) }); }
    catch (error) { onError(String(error)); }
  }
  return {
    desktop, saved, view: viewing, written, preferred, running, busyHere, failed, dismissed,
    /** What the Notes tab shows instead of notes.md, with the notes' own ticks while it matches them. */
    markdown: translation ? (translation.stale ? translation.markdown : withTicks(translation.markdown, meeting.notes_md)) : null,
    stale: !!translation?.stale,
    toggle: translation && !translation.stale ? toggle : undefined,
    choose, translate, show: setView, dismiss: () => setDismissed(true), clearFailed: () => setFailed(null),
    stop: () => void ipc.stopTranslation(id).catch(error => onError(String(error))),
  };
}

/** Call languages as the meeting panel saves them ("en,vi"), by name. */
function callLanguages(code?: string | null) {
  return code ? code.split(",").map(c => englishName(c.trim())).join(", ") : "";
}

/** Write the notes again's language: the call's own, then your languages, then all of them. */
export function WriteInSelect({ value, onChange, call, disabled }: { value: string; onChange: (code: string) => void; call?: string | null; disabled?: boolean }) {
  const calls = callLanguages(call);
  const preferred = stored(PREFERRED);
  const suggested = [...new Set([...(preferred ? [preferred] : []), ...recent(), ...systemLanguages()])];
  const option = (code: string) => <option key={code} value={code} lang={code}>{nativeName(code) === englishName(code) ? nativeName(code) : `${nativeName(code)} · ${englishName(code)}`}</option>;
  return <label className="writein">
    <TranslateIcon/><span>{s.writeIn}</span>
    <select value={value} disabled={disabled} onChange={event => onChange(event.target.value)}>
      <option value="same_as_call">{calls ? s.writeInCallLangs.replace("{langs}", calls) : s.writeInCall}</option>
      {suggested.length > 0 && <optgroup label={s.readInSuggested}>{suggested.map(option)}</optgroup>}
      <optgroup label={s.readInAll}>{[...NOTE_LANGUAGES].sort((a, b) => nativeName(a).localeCompare(nativeName(b))).map(option)}</optgroup>
    </select>
  </label>;
}

type Row = { code: string | null; group?: string };

export function ReadInPicker({ readIn, meeting }: { readIn: ReadIn; meeting: Meeting }) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const root = useRef<HTMLDivElement>(null), button = useRef<HTMLButtonElement>(null), list = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (event: PointerEvent) => { if (!root.current?.contains(event.target as Node)) setOpen(false); };
    document.addEventListener("pointerdown", close);
    return () => document.removeEventListener("pointerdown", close);
  }, [open]);
  // Original, then saved translations, then suggestions (recent picks, this computer's languages),
  // then every language: each language appears once, in the first group it belongs to.
  const rows = useMemo(() => {
    const seen = new Set<string>(), out: Row[] = [];
    const add = (group: string, codes: readonly string[]) => {
      const fresh = codes.filter(code => !seen.has(code) && matches(code, query));
      fresh.forEach((code, i) => { seen.add(code); out.push({ code, group: i === 0 ? group : undefined }); });
    };
    if (readIn.written) seen.add(readIn.written);
    if (!query.trim()) out.push({ code: null });
    add(s.readInSaved, readIn.saved.map(t => t.code));
    add(s.readInSuggested, [...(readIn.preferred ? [readIn.preferred] : []), ...recent(), ...systemLanguages()]);
    add(s.readInAll, [...NOTE_LANGUAGES].sort((a, b) => englishName(a).localeCompare(englishName(b))));
    return out;
  }, [query, readIn.saved, readIn.preferred, readIn.written, open]);
  useEffect(() => { setActive(0); }, [query]);
  useEffect(() => { list.current?.querySelector(`[data-index="${active}"]`)?.scrollIntoView({ block: "nearest" }); }, [active]);
  function pick(code: string | null) { setOpen(false); setQuery(""); readIn.choose(code); button.current?.focus(); }
  const label = readIn.view ? nativeName(readIn.view) : readIn.written ? nativeName(readIn.written) : s.readInOriginal;
  const asWritten = readIn.written ? s.writtenIn.replace("{lang}", nativeName(readIn.written)) : s.readInAsWritten;
  const calls = callLanguages(meeting.summary.language);
  return <div className="readin" ref={root}>
    <button ref={button} type="button" className={`readin-button${readIn.view ? " on" : ""}`} aria-haspopup="dialog" aria-expanded={open} title={s.readIn} onClick={() => { setOpen(!open); setQuery(""); }}>
      <TranslateIcon/><span className="readin-label"><span className="sr-only">{s.readIn}: </span>{label}</span>{readIn.busyHere && <i className="readin-spin" aria-hidden="true"/>}<svg className="chev" viewBox="0 0 16 16" aria-hidden="true"><path d="m4.5 6.5 3.5 3.5 3.5-3.5" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round"/></svg>
    </button>
    {open && <div className="readin-pop" role="dialog" aria-label={s.readIn} onKeyDown={event => {
      if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); setOpen(false); button.current?.focus(); }
      else if (event.key === "ArrowDown" || event.key === "ArrowUp") { event.preventDefault(); setActive(i => rows.length ? (i + (event.key === "ArrowDown" ? 1 : -1) + rows.length) % rows.length : 0); }
      else if (event.key === "Enter" && rows[active]) { event.preventDefault(); pick(rows[active].code); }
    }}>
      <div className="readin-search">
        <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" aria-hidden="true"><circle cx="7" cy="7" r="4.5"/><path d="m10.5 10.5 3 3"/></svg>
        <input autoFocus role="combobox" aria-expanded="true" aria-controls="readin-list" aria-activedescendant={rows[active] ? `readin-${rows[active].code ?? "original"}` : undefined} aria-label={s.readInSearch} placeholder={s.readInSearch} value={query} onChange={event => setQuery(event.target.value)} autoComplete="off" spellCheck={false}/>
      </div>
      <div className="readin-list" id="readin-list" role="listbox" aria-label={s.readIn} ref={list}>
        {!rows.length && <p className="readin-empty">{s.readInNone}</p>}
        {rows.map((row, index) => {
          const t = row.code ? readIn.saved.find(x => x.code === row.code) : undefined;
          const current = row.code === readIn.view;
          const translating = row.code && readIn.busyHere?.code === row.code;
          return <div key={row.code ?? "original"} className="readin-row-wrap">
            {row.group && <div className="readin-group" role="presentation">{row.group}</div>}
            <div id={`readin-${row.code ?? "original"}`} data-index={index} role="option" aria-selected={current} className={`readin-row${index === active ? " active" : ""}`} onPointerMove={() => setActive(index)} onClick={() => pick(row.code)}>
              <span className="readin-check" aria-hidden="true">{current && <svg viewBox="0 0 16 16"><path d="m3.5 8.5 3 3 6-7" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round"/></svg>}</span>
              {row.code ? <span className="readin-name"><span lang={row.code}>{nativeName(row.code)}</span>{nativeName(row.code) !== englishName(row.code) && <small>{englishName(row.code)}</small>}</span>
                : <span className="readin-name"><span>{s.readInOriginal}</span><small>{calls ? `${asWritten} · ${s.readInCall.replace("{langs}", calls)}` : asWritten}</small></span>}
              {translating ? <span className="readin-tag busy">{s.readInRunning}</span> : t && <span className={`readin-tag${t.stale ? " stale" : ""}`}>{t.stale ? s.readInStale : s.readInFresh}</span>}
            </div>
          </div>;
        })}
      </div>
      <p className="readin-foot"><MenuIcon name="lock"/>{s.readInFoot}</p>
    </div>}
  </div>;
}

/** The line over the notes: progress, a failure, what you are reading, or the one-time offer. */
/** `onWriteIn`: write the notes again in the language being read (none while locked). */
export function ReadInBar({ readIn, onWriteIn }: { readIn: ReadIn; onWriteIn?: (code: string) => void }) {
  const run = readIn.busyHere;
  if (run) {
    const step = run.of ? s.translatingPart.replace("{n}", String(run.part)).replace("{m}", String(run.of)) : s.translatingStart;
    return <div className="readin-bar running" role="status">
      <TranslateIcon/><div className="readin-text"><b>{s.translatingTo.replace("{lang}", nativeName(run.code))}</b><span> · {step}</span><span className="readin-sub">{s.translatingKeep}</span></div>
      <button type="button" className="doc-action" onClick={readIn.stop}>{s.stopTranslating}</button>
      <i className="readin-progress" style={run.of ? { ["--done" as string]: `${(run.part - 1) / run.of * 100}%`, ["--next" as string]: `${run.part / run.of * 100}%` } : undefined} aria-hidden="true"/>
    </div>;
  }
  if (readIn.failed) return <Alert variant="danger" role="alert" title={s.translateFailed} actions={<><button className="doc-action" onClick={() => readIn.translate(readIn.failed!.code)}>{s.translateRetry}</button><button className="doc-action ghost" onClick={readIn.clearFailed}>{s.cancel}</button></>}>{readIn.failed.message}</Alert>;
  if (readIn.view && readIn.markdown !== null) {
    const name = nativeName(readIn.view);
    if (readIn.stale) return <Alert variant="warning" title={s.staleTitle} actions={<><button className="doc-action" onClick={() => readIn.translate(readIn.view!)}>{s.updateTranslation}</button><button className="doc-action ghost" onClick={() => readIn.show(null)}>{s.showOriginal}</button></>}>{s.staleBody}</Alert>;
    return <div className="readin-bar" role="status">
      <TranslateIcon/><div className="readin-text"><b lang={readIn.view}>{name}</b><span className="readin-sub">{s.translatedFrom}</span></div>
      {onWriteIn && <button type="button" className="doc-action ghost" title={s.writeInOfferTip.replace("{lang}", name)} onClick={() => onWriteIn(readIn.view!)}>{s.writeInOffer.replace("{lang}", name)}</button>}
      <button type="button" className="doc-action ghost" onClick={() => readIn.show(null)}>{s.showOriginal}</button>
      <button type="button" className="doc-action ghost" title={s.translateAgain} aria-label={s.translateAgain} onClick={() => readIn.translate(readIn.view!)}><MenuIcon name="again"/></button>
    </div>;
  }
  const p = readIn.preferred;
  if (!p || p === readIn.written) return null;
  // You were reading in your language and looked at the original: one click back.
  if (readIn.saved.some(t => t.code === p)) return <div className="readin-bar quiet" role="status">
    <TranslateIcon/><div className="readin-text"><span>{s.showingOriginal}</span></div>
    <button type="button" className="doc-action ghost" onClick={() => readIn.show(p)}>{s.showLanguage.replace("{lang}", nativeName(p))}</button>
  </div>;
  if (readIn.dismissed) return null;
  return <div className="readin-bar offer" role="status">
    <TranslateIcon/><div className="readin-text"><b>{s.suggestTitle.replace("{lang}", nativeName(p))}</b><span className="readin-sub">{s.suggestBody}</span></div>
    <button type="button" className="doc-action" onClick={() => readIn.translate(p)}>{s.translate}</button>
    <button type="button" className="doc-action ghost" onClick={readIn.dismiss}>{s.notNow}</button>
  </div>;
}
