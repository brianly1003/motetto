"use client";
import { useRef, useState } from "react";
import type { MeetingSummary } from "@/lib/bindings/MeetingSummary";
import { meetingAction } from "@/lib/ipc";
import { meetingDate, durationLabel } from "@/lib/document";
import { strings as s } from "@/lib/strings";

// One row of the Recently deleted page. Delete permanently asks once, in the row, naming the meeting.
// no "Kept for {n} more days" yet; the deletion time isn't stored (SPEC deleted_kept).
export default function DeletedMeeting({meeting, onChanged, onError}: {meeting: MeetingSummary; onChanged: (action: "restore" | "purge") => Promise<void>; onError: (message: string) => void}) {
  const [confirm, setConfirm] = useState(false);
  const [busy, setBusy] = useState(false);
  const deleteButton = useRef<HTMLButtonElement>(null);
  function cancel() { setConfirm(false); requestAnimationFrame(() => deleteButton.current?.focus()); }
  async function run(action: "restore" | "purge") {
    setBusy(true);
    try { await meetingAction(meeting.id, action); await onChanged(action); }
    catch (error) { onError(String(error)); }
    finally { setBusy(false); }
  }
  // A trim's original (owner 2026-11-05): it goes back into its meeting.
  const title = meeting.deleted_kind === "trimmed_original" ? s.fullVideoOf.replace("{title}", meeting.title) : meeting.title;
  const meta = `${meetingDate(meeting.created_ms)}${meeting.duration_ms != null ? ` · ${durationLabel(meeting.duration_ms)}` : ""}`;
  return confirm
    ? <div className="trow confirming" role="alertdialog" aria-label={`Delete “${title}” permanently?`} onKeyDown={event => { if (event.key === "Escape" && !busy) cancel(); }}>
      <div className="tt"><b>Delete “{title}” permanently?</b><span>{s.permanentDeleteWarning}</span></div>
      <div className="acts"><button className="doc-action danger" disabled={busy} onClick={() => run("purge")}>{s.deletePermanently}</button><button className="doc-action ghost" autoFocus disabled={busy} onClick={cancel}>{s.cancel}</button></div>
    </div>
    : <div className="trow">
      <div className="tt"><b>{title}</b><span>{meta}</span></div>
      <div className="acts"><button className="doc-action" disabled={busy} onClick={() => run("restore")}>{s.restoreMeeting}</button><button ref={deleteButton} className="doc-action ghost" disabled={busy} onClick={() => setConfirm(true)}>{s.deletePermanently}</button></div>
    </div>;
}
