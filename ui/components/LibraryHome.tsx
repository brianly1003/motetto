import { useEffect, useState } from "react";
import { meetingDate, noteSections, recordedAt, taskLine } from "@/lib/document";
import { Alert } from "@/components/Alert";
import { notesText } from "@/components/MeetingActions";
import { renderInline } from "@/lib/md";
import * as ipc from "@/lib/ipc";
import { strings as s } from "@/lib/strings";
import type { MeetingSummary } from "@/lib/bindings/MeetingSummary";

type Item = { meeting: MeetingSummary; notes: string; line: number; owner: string; text: string; src: string };

// Unchecked action items across meetings; also counted on the sidebar's Action items row.
export function openItems(open: unknown): Item[] {
  // Anything but a list (an older backend without this command) means nothing open.
  return (Array.isArray(open) ? open as [MeetingSummary, string][] : []).flatMap(([meeting, notes]) => noteSections(notes).flatMap(section => section.lines.flatMap(line => {
    const task = taskLine(line.text);
    if (!task || task.done) return [];
    const src = /\[(\d+:\d{2})\]\s*$/.exec(task.text)?.[1] ?? "";
    return [{meeting, notes, line: line.line, owner: task.owner === "Me" ? "You" : task.owner, text: task.text.replace(/\s*\[\d+:\d{2}\]\s*$/, ""), src}];
  })));
}

/** "mm:ss" (minutes may pass 59) in milliseconds. */
export function srcMs(src: string) {
  const [m, sec] = src.split(":").map(Number);
  return (m * 60 + sec) * 1000;
}

// A meeting shows this many rows, then "Show n more"; one more row than this shows whole.
const ROWS = 4;
type Filter = "all" | "you" | "others";

// Library home (mockup renderHome, owner 2026-10-30): one list, a one-line header per meeting with
// open items (newest first; the sidebar already shows its time and length), yours then waiting on
// others under it. Import sits in the header so a long list never hides it; a file dropped
// anywhere in the window still imports (the shell's drop frame).
export default function LibraryHome({version, onOpen, onPlay, onImport, onDone, onCopied, onError, onCount, disabled, disabledReason, interrupted = [], onRecover, onDiscard}: {version: number; disabledReason?: string; onCount: (n: number) => void; onOpen: (id: string) => void; onImport: () => void; onDone: (undo: () => Promise<void>) => void; onError: (error: string) => void; disabled: boolean;
  /** Opens the meeting and plays from `ms`, as a source chip in the notes does. */
  onPlay: (id: string, ms: number) => void; onCopied: () => void;
  /** Recordings cut off by a quit or crash (status Interrupted), offered first (SPEC 6.1, mockup renderHome). */
  interrupted?: MeetingSummary[]; onRecover?: (ids: string[]) => void; onDiscard?: (ids: string[]) => void}) {
  const [items, setItems] = useState<Item[] | null>(null);
  const [reload, setReload] = useState(0);
  const [filter, setFilter] = useState<Filter>("all");
  const [more, setMore] = useState<Set<string>>(new Set());
  useEffect(() => {
    let active = true;
    ipc.openActionItems().then(open => { if (active) setItems(openItems(open)); }).catch(error => { if (active) onError(String(error)); });
    return () => { active = false; };
  }, [version, reload, onError]);
  async function done(item: Item) {
    try {
      const notes = await ipc.setActionItemDone(item.meeting.id, item.line, true, item.notes);
      setReload(n => n + 1);
      onDone(async () => { await ipc.setActionItemDone(item.meeting.id, item.line, false, notes); setReload(n => n + 1); });
    } catch (error) { onError(String(error)); }
  }
  async function copy(notes: string) {
    try { await navigator.clipboard.writeText(notesText(notes, "items")); onCopied(); } catch (error) { onError(String(error)); }
  }
  function toggle(id: string) { setMore(open => { const next = new Set(open); if (!next.delete(id)) next.add(id); return next; }); }
  useEffect(() => { if (items) onCount(items.length); }, [items, onCount]);
  const all = items ?? [], mine = all.filter(item => item.owner === "You"), others = all.filter(item => item.owner !== "You");
  const shown = filter === "all" ? all : filter === "you" ? mine : others;
  // Grouped in the backend's order: meetings newest first, items in note order.
  const groups = [...new Map(shown.map(item => [item.meeting.id, item.meeting])).values()].map(meeting => ({meeting,
    mine: shown.filter(item => item.meeting.id === meeting.id && item.owner === "You"), others: shown.filter(item => item.meeting.id === meeting.id && item.owner !== "You")}));
  const meetings = new Set(all.map(item => item.meeting.id)).size;
  const ids = interrupted.map(meeting => meeting.id);
  const src = (item: Item) => item.src && <button type="button" className="srcchip" title={s.playFrom.replace("{t}", item.src)} onClick={() => onPlay(item.meeting.id, srcMs(item.src))}><svg viewBox="0 0 12 12" aria-hidden="true"><path d="M3.5 2.2v7.6L9.6 6z" fill="currentColor"/></svg>{item.src}</button>;
  const row = (item: Item, owner: boolean) => <div className="oi" key={`${item.meeting.id}-${item.line}`}>
    <input type="checkbox" aria-label={s.markDone} onChange={() => void done(item)}/>
    <span>{owner && (item.owner ? <span className="owner">{item.owner}</span> : <span className="tip">{s.noOwner} · </span>)}<span dangerouslySetInnerHTML={{__html: renderInline(item.text)}}/></span>
    {src(item)}
  </div>;
  const seg = (id: Filter, label: string, n?: number) => <button type="button" role="radio" aria-checked={filter === id} onClick={() => setFilter(id)}>{label}{n !== undefined && <> <span className="n">{n}</span></>}</button>;
  return <div className="home">
    {interrupted.length > 0 && <Alert variant="warning" title={interrupted.length === 1 ? s.recoverTitle : s.recoverTitleMany}
      actions={<><button type="button" className="btn sm" onClick={() => onRecover?.(ids)} disabled={disabled} title={disabledReason || undefined}>{s.recover}</button><button type="button" className="btn sm ghost" onClick={() => onDiscard?.(ids)}>{s.discard}</button></>}>
      {interrupted.length === 1 ? s.recoverBody.replace("{when}", recordedAt(interrupted[0].created_ms)) : s.recoverBodyMany.replace("{n}", String(interrupted.length))}
    </Alert>}
    <div className="homehead"><div><h1>{s.openItemsTitle}</h1>{items && items.length > 0 && <span className="tip">{s.openItemsCount.replace("{n}", String(items.length)).replace("{m}", String(meetings))}</span>}</div>
      <div className="homeacts">{all.length > 0 && <div className="hseg" role="radiogroup" aria-label={s.showItems}>{seg("all", s.filterAll)}{seg("you", s.filterYours, mine.length)}{seg("others", s.filterOthers, others.length)}</div>}
        <button type="button" className="btn sm" onClick={onImport} disabled={disabled} title={disabledReason || undefined}>{s.import}</button></div></div>
    {items && (groups.length ? <div className="olist">{groups.map(({meeting, mine, others}) => {
      const count = mine.length + others.length, open = more.has(meeting.id) || count <= ROWS + 1, cut = open ? count : ROWS;
      const rowsMine = mine.slice(0, cut), rowsOthers = others.slice(0, Math.max(0, cut - mine.length));
      return <section className="mgroup" key={meeting.id} aria-label={meeting.title}>
        <header className="mhead"><button type="button" className="mname" title={s.openMeeting} onClick={() => onOpen(meeting.id)}>{meeting.title}</button><span className="mday">· {meetingDate(meeting.created_ms).split(" · ")[0]}</span>
          <div className="mact"><span className="n" aria-label={s.openCount.replace("{n}", String(count))}>{count}</span>
            <button type="button" className="iconbtn" title={s.copyMeetingItems} aria-label={`${s.copyMeetingItems}: ${meeting.title}`} onClick={() => void copy(all.find(item => item.meeting.id === meeting.id)!.notes)}><svg viewBox="0 0 16 16" aria-hidden="true"><rect x="5.5" y="5.5" width="8" height="8" rx="1.8"/><path d="M10.5 3.2A1.7 1.7 0 0 0 8.8 2H4a2 2 0 0 0-2 2v4.8c0 .8.5 1.5 1.2 1.7"/></svg></button>
            <button type="button" className="iconbtn" title={s.openMeeting} aria-label={`${s.openMeeting}: ${meeting.title}`} onClick={() => onOpen(meeting.id)}><svg viewBox="0 0 16 16" aria-hidden="true"><path d="m6 3.5 4.5 4.5L6 12.5"/></svg></button></div></header>
        {rowsMine.length > 0 && <div className="openitems">{rowsMine.map(item => row(item, false))}</div>}
        {rowsOthers.length > 0 && <>{mine.length > 0 && <div className="mlabel">{s.waitingOnOthers}</div>}<div className="openitems">{rowsOthers.map(item => row(item, true))}</div></>}
        {count > ROWS + 1 && <button type="button" className="mmore" aria-expanded={open} onClick={() => toggle(meeting.id)}>{open ? s.showFewer : s.showMore.replace("{n}", String(count - ROWS))}</button>}
      </section>;
    })}</div> : <p className="tip">{filter === "you" ? s.noneYours : s.noOpenItems}</p>)}
  </div>;
}
