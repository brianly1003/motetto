"use client";
import { useEffect, useRef, useState } from "react";
import type { MeetingSummary } from "@/lib/bindings/MeetingSummary";
import { meetingAction } from "@/lib/ipc";
import { strings as s } from "@/lib/strings";
import DeletedMeeting from "./DeletedMeeting";

const meetingsN = (n: number) => `${n} meeting${n === 1 ? "" : "s"}`;

// The Recently deleted page (DESIGN 4.7): Restore all, and Delete all behind one inline question.
export default function RecentlyDeleted({deleted, refresh, notify, onError}: {deleted: MeetingSummary[]; refresh: () => Promise<void>; notify: (text: string, undo?: () => Promise<void>) => void; onError: (message: string) => void}) {
  const [confirmAll, setConfirmAll] = useState(false);
  const [busy, setBusy] = useState(false);
  const cancel = useRef<HTMLButtonElement>(null);
  const deleteAll = useRef<HTMLButtonElement>(null);
  // Cancel has focus, so an accidental Enter never deletes everything; Esc cancels.
  useEffect(() => { if (confirmAll) cancel.current?.focus(); }, [confirmAll]);
  function close() { setConfirmAll(false); requestAnimationFrame(() => deleteAll.current?.focus()); }
  // Esc cancels wherever focus went (a click on the inert list moves it to the page).
  useEffect(() => {
    if (!confirmAll) return;
    const key = (event: KeyboardEvent) => { if (event.key === "Escape") { event.preventDefault(); close(); } };
    document.addEventListener("keydown", key); return () => document.removeEventListener("keydown", key);
  }, [confirmAll]);
  // One meeting at a time, so a failure says which meeting is left and nothing is half-removed.
  async function each(ids: string[], action: "restore" | "purge" | "delete") {
    setBusy(true);
    try { for (const id of ids) await meetingAction(id, action); }
    finally { setBusy(false); await refresh(); }
  }
  // Meetings first: a trimmed original goes back into its meeting, so that meeting must be back
  // already. Undo deletes the meetings again; a full video put back stays (trim it again).
  async function restoreAll() {
    const meetings = deleted.filter(m => m.deleted_kind !== "trimmed_original").map(m => m.id);
    const full = deleted.filter(m => m.deleted_kind === "trimmed_original" && m.kind === "screen").map(m => m.id);
    try {
      await each([...meetings, ...full], "restore");
      notify(s.restoredAll.replace("{n}", meetingsN(meetings.length + full.length)), meetings.length ? () => each(meetings, "delete") : undefined);
    }
    catch (error) { onError(String(error)); }
  }
  async function purgeAll() {
    const n = deleted.length;
    try { await each(deleted.map(meeting => meeting.id), "purge"); setConfirmAll(false); notify(s.purgedAll.replace("{n}", meetingsN(n))); }
    catch (error) { setConfirmAll(false); onError(String(error)); }
  }
  const n = deleted.length;
  return <div className={`trashview${confirmAll ? " confirmingall" : ""}`}>
    <div className="titlerow"><h1>{s.deletedMeetings}</h1><div className="headacts">
      <button className="doc-action" disabled={busy || confirmAll} onClick={() => void restoreAll()}>{s.restoreAll}</button>
      <button ref={deleteAll} className="doc-action ghost danger-text" disabled={busy || confirmAll} onClick={() => setConfirmAll(true)}>{s.deleteAll}</button>
    </div></div>
    <p className="lead">{s.deletedLead}</p>
    {confirmAll && <div className="trashconfirm" role="alertdialog" aria-label={s.deleteAll}>
      <div className="tt"><b>{s.confirmPurgeAll.replace("{n}", meetingsN(n))}</b><span>{s.purgeAllBody}</span></div>
      <div className="acts"><button className="doc-action danger" disabled={busy} onClick={() => void purgeAll()}>Delete {meetingsN(n)}</button><button ref={cancel} className="doc-action ghost" disabled={busy} onClick={close}>{s.cancel}</button></div>
    </div>}
    {/* While Delete all asks, the rows answer nothing (inert: no clicks, hover or focus). */}
    <div className="trashlist" inert={confirmAll}>{deleted.map(meeting => <DeletedMeeting key={meeting.id} meeting={meeting} onError={onError} onChanged={async action => { await refresh(); notify(action === "restore" ? s.restored : s.purged); }}/>)}</div>
  </div>;
}
