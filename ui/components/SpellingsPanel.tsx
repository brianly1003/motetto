import { useEffect, useRef, useState } from "react";
import type { Meeting } from "@/lib/bindings/Meeting";
import type { Spelling } from "@/lib/bindings/Spelling";
import { strings as s } from "@/lib/strings";
import * as ipc from "@/lib/ipc";

const X = <svg viewBox="0 0 8 8" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" aria-hidden="true"><path d="M1.5 1.5l5 5M6.5 1.5l-5 5"/></svg>;
const CHEVRON = <svg viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M2.5 4l2.5 2.5L7.5 4"/></svg>;
const PLUS = <svg viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" aria-hidden="true"><path d="M6 2v8M2 6h8"/></svg>;
const CHECK = <svg viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M2.5 6.2l2.3 2.3 4.7-5"/></svg>;

/** Heard forms as shown and saved: one `; ` between them, whatever separated them when typed. */
export const forms = (heard: string) => heard.split(/\s*[,;/]\s*/).filter(Boolean).join("; ");

// Whether the panel and its global list are open: a per-viewer convenience kept in localStorage.
function useFold(key: string, start: boolean) {
  const [open, setOpen] = useState(start);
  useEffect(() => { try { const saved = localStorage.getItem(key); if (saved !== null) setOpen(saved === "1"); } catch {} }, [key]);
  const toggle = () => setOpen(was => { try { localStorage.setItem(key, was ? "0" : "1"); } catch {} return !was; });
  return [open, toggle] as const;
}

type Scope = "meeting" | "global";

// One list (mockup .spell-chips + .spell-add): chips to click (the pair goes into the field to edit)
// or remove, and the field that adds a pair, or updates the one being edited. A spelling already in
// the list takes the new heard forms instead.
function SpellList({ scope, rows, most, readOnly, onSave, onToast }: { scope: Scope; rows: Spelling[]; most: number; readOnly: boolean; onSave: (rows: Spelling[]) => Promise<boolean>; onToast?: (text: string) => void }) {
  const [spelling, setSpelling] = useState("");
  const [heard, setHeard] = useState("");
  const [editing, setEditing] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const field = useRef<HTMLInputElement>(null);
  const where = scope === "global" ? s.spellWhereAll : s.spellWhereMeeting;
  const reset = () => { setSpelling(""); setHeard(""); setEditing(null); field.current?.focus(); };
  async function save(next: Spelling[]) { setBusy(true); try { return await onSave(next); } finally { setBusy(false); } }
  async function submit() {
    const row = { spelling: spelling.trim(), heard: forms(heard.trim()) };
    if (!row.spelling || busy) return;
    const same = (x: Spelling) => x.spelling.toLowerCase() === row.spelling.toLowerCase();
    if (editing !== null) {
      if (rows.some((x, i) => i !== editing && same(x))) { onToast?.(s.spellAlready.replace("{spelling}", row.spelling).replace("{where}", where)); return; }
      if (await save(rows.map((x, i) => i === editing ? row : x))) { reset(); onToast?.(s.spellUpdated.replace("{spelling}", row.spelling)); }
      return;
    }
    const known = rows.findIndex(same);
    if (known >= 0) {
      if (!row.heard) { onToast?.(s.spellAlready.replace("{spelling}", rows[known].spelling).replace("{where}", where)); return; }
      const merged = rows.map((x, i) => i === known ? { ...x, heard: forms([x.heard, row.heard].filter(Boolean).join("; ")) } : x);
      if (await save(merged)) { reset(); onToast?.(s.spellMoreHeard.replace("{spelling}", rows[known].spelling)); }
      return;
    }
    if (rows.length >= most) { onToast?.((scope === "global" ? s.spellMostAll : s.spellMost).replace("{n}", String(most))); return; }
    if (await save([...rows, row])) { reset(); if (scope === "global") onToast?.(s.spellAddedAll.replace("{spelling}", row.spelling)); }
  }
  const keys = (event: React.KeyboardEvent) => {
    if (event.key === "Enter") { event.preventDefault(); void submit(); }
    if (event.key === "Escape" && editing !== null) { event.preventDefault(); reset(); }
  };
  const pick = (i: number) => {
    if (editing === i) { reset(); return; }
    setEditing(i); setSpelling(rows[i].spelling); setHeard(forms(rows[i].heard)); field.current?.focus();
  };
  const words = (row: Spelling) => <><b>{row.heard ? `${row.spelling}:` : row.spelling}</b>{row.heard && <small>“{forms(row.heard)}”</small>}</>;
  return <>
    {rows.length > 0 ? <div className="spell-chips">{rows.map((row, i) =>
      <span key={row.spelling} className={`spell-chip${scope === "global" ? " global" : ""}${editing === i ? " editing" : ""}`} title={row.heard ? s.spellHeardAs.replace("{heard}", forms(row.heard)) : undefined}>
        {readOnly ? words(row) : <>
          <button type="button" className="spell-pick" aria-label={s.spellEdit.replace("{spelling}", row.spelling)} onClick={() => pick(i)}>{words(row)}</button>
          <button type="button" className="spell-x" aria-label={s.spellRemove.replace("{spelling}", row.spelling)} title={s.spellRemoveShort} onClick={() => void save(rows.filter((_, k) => k !== i)).then(() => reset())}>{X}</button>
        </>}
      </span>)}</div> : readOnly && <p className="tip">{scope === "global" ? s.spellNoneYet : s.spellNone}</p>}
    {!readOnly && <div className="spell-add" data-scope={scope}>
      <input ref={field} className="spell-in" type="text" maxLength={40} placeholder={s.spellAddPlaceholder} aria-label={scope === "global" ? s.spellAddLabelAll : s.spellAddLabel} value={spelling} disabled={busy} onChange={e => setSpelling(e.target.value)} onKeyDown={keys}/>
      <input className="spell-heard" type="text" maxLength={80} placeholder={s.spellHeardPlaceholder} aria-label={s.spellHeardLabel} title={s.spellHeardHint} value={heard} disabled={busy} onChange={e => setHeard(e.target.value)} onKeyDown={keys}/>
      <button type="button" className="spell-clear" tabIndex={-1} aria-label={s.spellClear} title={s.spellClear} onMouseDown={event => event.preventDefault()} onClick={reset}>{X}</button>
      {editing !== null
        ? <button type="button" className="spell-go" aria-label={s.spellUpdate} title={s.spellUpdateShort} disabled={busy || !spelling.trim()} onClick={() => void submit()}>{CHECK}</button>
        : <button type="button" className="spell-go" aria-label={s.spellAdd} title={s.spellAddShort} disabled={busy || !spelling.trim()} onClick={() => void submit()}>{PLUS}</button>}
    </div>}
  </>;
}

// Spellings used (SPEC 7.11, mockup .spell): this meeting's own list and the one for all meetings
// (Settings › Spellings), both edited here. Saving either respells the transcript's heard forms.
export default function SpellingsPanel({ meeting, onSaved, onError, onToast }: { meeting: Meeting; onSaved: (meeting: Meeting) => void; onError: (message: string) => void; onToast?: (text: string) => void }) {
  const [global, setGlobal] = useState<Spelling[]>([]);
  const [open, toggleOpen] = useFold("motetto.spellOpen", true);
  const [globalOpen, toggleGlobal] = useFold("motetto.spellGlobal", false);
  const summary = meeting.summary;
  const mine = summary.meeting_spellings ?? [];
  const locked = summary.locked;
  useEffect(() => { ipc.getSettings().then(settings => setGlobal(settings.spellings), () => setGlobal([])); }, [summary.id]);

  // Shows the meeting the backend respelled after a save, and says how many lines changed.
  function respelled(saved: Meeting) {
    const before = meeting.transcript.split("\n"), after = saved.transcript.split("\n");
    const fixed = after.length === before.length ? after.filter((line, i) => line !== before[i]).length : 0;
    onSaved(saved);
    if (fixed) onToast?.((fixed === 1 ? s.spellFixedOne : s.spellFixedMany).replace("{n}", String(fixed)));
  }
  // Each save respells this meeting from the lists before and after, so a removed pair goes back.
  const saveMine = async (next: Spelling[]) => { try { respelled(await ipc.setMeetingSpellings(summary.id, next)); return true; } catch (e) { onError(String(e)); return false; } };
  // The list for all meetings lives in Settings › Spellings.
  const saveGlobal = async (next: Spelling[]) => {
    try {
      respelled(await ipc.setGlobalSpellings(summary.id, next));
      setGlobal((await ipc.getSettings()).spellings);
      return true;
    } catch (e) { onError(String(e)); return false; }
  };

  const head = <h5><button type="button" className="spell-head" aria-expanded={open} onClick={toggleOpen}><span>{s.spellingsUsed}</span><em>{mine.length + global.length}</em>{CHEVRON}</button></h5>;
  if (!open) return <div className="panel spell">{head}</div>;
  return <div className="panel spell">
    {head}
    <div className="spell-sub"><span>{s.spellThisMeeting}</span></div>
    <SpellList scope="meeting" rows={mine} most={100} readOnly={locked} onSave={saveMine} onToast={onToast}/>
    <div className="spell-sub"><button type="button" className="spell-fold" aria-expanded={globalOpen} onClick={toggleGlobal}>{CHEVRON}<span>{s.spellAllMeetings}</span><em>{global.length}</em></button><button type="button" className="linkbtn" onClick={() => void ipc.openSettings("spellings").catch(e => onError(String(e)))}>{s.spellSettings}</button></div>
    {globalOpen && <SpellList scope="global" rows={global} most={500} readOnly={locked} onSave={saveGlobal} onToast={onToast}/>}
  </div>;
}
