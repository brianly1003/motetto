"use client";
import { Alert } from "@/components/Alert";
import { useEffect, useRef, useState } from "react";
import * as ipc from "@/lib/ipc";
import { useSession } from "@/lib/useSession";
import type { MeetingSummary } from "@/lib/bindings/MeetingSummary";
import { meetingTitle } from "@/lib/document";

export default function CommandPalette({onClose, onSelect, onImport, onHome, canRecord = true}: {canRecord?: boolean; onClose: () => void; onSelect: (id: string) => void; onImport?: () => void; onHome?: () => void}) {
  const [query, setQuery] = useState("");
  const [meetings, setMeetings] = useState<MeetingSummary[]>([]);
  const [selected, setSelected] = useState(0);
  const [error, setError] = useState("");
  const dialog = useRef<HTMLDivElement>(null);
  const {state, act, notice} = useSession();
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    dialog.current?.querySelector("input")?.focus();
    return () => previous?.focus();
  }, []);
  useEffect(() => {
    let active = true;
    const timer = setTimeout(() => { ipc.searchMeetings(query).then(rows => { if (active) setMeetings(rows.slice(0, 5).map(hit => hit.meeting)); }).catch(error => { if (active) setError(String(error)); }); }, 100);
    return () => { active = false; clearTimeout(timer); };
  }, [query]);
  const actions = [
    ...(state?.state.type === "Idle" && canRecord ? [{label: "Record now", run: ipc.recordNow}] : []),
    ...(state?.state.type === "Recording" ? [{label: "Stop recording", run: ipc.stopRecording}, {label: "Pause recording", run: ipc.pauseRecording}, {label: "Mark this moment", run: async () => { await ipc.markMoment(); }}, {label: "Open My notes", run: () => ipc.setCompanionSize("Sidecar")}] : []),
    ...(state?.state.type === "Paused" ? [{label: "Resume recording", run: ipc.resumeRecording}, {label: "Stop recording", run: ipc.stopRecording}] : []),
    ...(onHome ? [{label: "Go to Action items", run: async () => { onHome(); }}] : []),
    {label: "Open settings", run: () => ipc.openSettings()},
    ...(onImport ? [{label: "Import a recording", run: async () => { onImport(); }}] : []),
  ].filter(action => action.label.toLowerCase().includes(query.toLowerCase()));
  const rows = [...meetings.map(meeting => ({label: meetingTitle(meeting), run: async () => onSelect(meeting.id)})), ...actions];
  async function choose(index: number) { const row = rows[index]; if (row) await act(async () => { await row.run(); onClose(); }); }
  return <><div className="scrim" onClick={onClose}/><div ref={dialog} className="palette" role="dialog" aria-modal="true" aria-label="Command palette" onKeyDown={event => {
    if (event.key === "Escape") { event.preventDefault(); onClose(); }
    if (event.key === "ArrowDown" || event.key === "ArrowUp") { event.preventDefault(); setSelected(index => rows.length ? (index + (event.key === "ArrowDown" ? 1 : -1) + rows.length) % rows.length : 0); }
    if (event.key === "Enter" && event.target instanceof HTMLInputElement) { event.preventDefault(); void choose(Math.min(selected, rows.length - 1)); }
    if (event.key === "Tab") { const focusable = Array.from(dialog.current?.querySelectorAll<HTMLElement>("input,button") ?? []); const first = focusable[0], last = focusable.at(-1); if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); } else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); } }
  }}><input aria-label="Search meetings or commands" placeholder="Search meetings or type a command" value={query} onChange={event => { setQuery(event.target.value); setSelected(0); }}/><div className="res">{meetings.length > 0 && <div className="k">Meetings</div>}{meetings.map((meeting, i) => <button key={meeting.id} aria-selected={selected === i} onClick={() => choose(i)}><span>{meeting.title}</span><span className="tip">{new Date(meeting.created_ms).toLocaleDateString()}</span></button>)}{actions.length > 0 && <div className="k">Actions</div>}{actions.map((action, i) => <button key={action.label} aria-selected={selected === meetings.length + i} onClick={() => choose(meetings.length + i)}><span>{action.label}</span></button>)}{!rows.length && <p className="tip">Nothing matches “{query}”.</p>}{(error || notice) && <Alert variant={error ? "danger" : "info"} role="alert" title={error ? "Couldn’t run this command" : "Motetto"}>{error || notice}</Alert>}</div></div></>;
}
