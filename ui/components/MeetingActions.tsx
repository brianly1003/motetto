"use client";
import { Alert } from "@/components/Alert";
import { useEffect, useRef, useState } from "react";
import type { Meeting } from "@/lib/bindings/Meeting";
import { getSettings, meetingAction, STOPPED } from "@/lib/ipc";
import { nativeName } from "@/lib/languages";
import { WriteInSelect } from "@/components/ReadIn";
import { renderNotes } from "@/lib/md";
import { withoutTranscript } from "@/lib/document";
import { strings as s } from "@/lib/strings";
import { MenuItem, MenuSwitch } from "@/components/MenuIcon";
import { KbdGroup } from "@/components/ui/Kbd";
import { ChevronDown } from "@/components/ui/Icon";

/** Text for Copy: the whole notes as Markdown or plain text, or only the action items. A Read in
 *  translation has the `original`'s lines, so its action items are found where the original's are. */
export function notesText(notes_md: string, kind: "items" | "markdown" | "plain", original = notes_md) {
  let text = withoutTranscript(notes_md).trim();
  if (kind === "items") {
    const lines = original.split("\n"), head = lines.findIndex(line => /^#{1,6}\s+Action items\s*$/i.test(line));
    const end = head < 0 ? -1 : lines.findIndex((line, i) => i > head && /^#{1,6}\s/.test(line));
    text = head < 0 ? "" : withoutTranscript(notes_md.split("\n").slice(head + 1, end < 0 ? undefined : end).join("\n")).trim();
  }
  if (kind !== "markdown") {
    const document = new DOMParser().parseFromString(renderNotes(text), "text/html");
    document.querySelectorAll("li, p, h1, h2, h3, h4, br").forEach(node => node.append("\n"));
    text = document.body.textContent?.trim() ?? "";
  }
  return text;
}

/** A meeting still recording or writing its notes can't be deleted yet. */
export const deletable = (status: string) => status !== "Recording" && status !== "Finishing";

/** `shown`: the notes as on screen (a Read in translation), which Copy copies. `reading`: its
 *  language, which Write the notes again offers first; `ask` opens that question in a language. */
export default function MeetingActions({ meeting, shown, reading, ask, hasAudio, onChanged, onError, onToast, onRename, onDelete, onLock }: { meeting: Meeting; shown?: string; reading?: string | null; ask?: { code: string } | null; hasAudio: boolean; onChanged: (deleted: boolean) => Promise<void>; onError: (message: string) => void; onToast?: (text: string) => void; onRename: () => void; onDelete: () => void; onLock: (locked: boolean) => void }) {
  const locked = meeting.summary.locked;
  const [menu, setMenu] = useState<"copy" | "more" | null>(null);
  const [confirm, setConfirm] = useState<"regenerate" | "retranscribe" | null>(null);
  // Corrections Transcribe again would replace: lines that differ from what was heard.
  const heard = meeting.transcript_heard ? meeting.transcript_heard.split("\n") : [];
  const corrections = heard.length ? meeting.transcript.split("\n").filter((line, i) => line !== heard[i]).length : 0;
  const [busy, setBusy] = useState(false);
  // Write the notes again's language: what you read in, else what they're written in, else Settings.
  const [language, setLanguage] = useState("same_as_call");
  function askRegenerate(code?: string) {
    setConfirm("regenerate"); setMenu(null);
    const known = code ?? reading ?? meeting.summary.notes_language;
    if (known) { setLanguage(known); return; }
    setLanguage("same_as_call");
    void getSettings().then(settings => { if (settings?.notes_language) setLanguage(settings.notes_language); }).catch(() => {});
  }
  useEffect(() => { if (ask) askRegenerate(ask.code); }, [ask]);
  const [message, setMessage] = useState("");
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const close = (event: PointerEvent) => { if (!root.current?.contains(event.target as Node)) setMenu(null); };
    document.addEventListener("pointerdown", close);
    return () => document.removeEventListener("pointerdown", close);
  }, []);
  useEffect(() => {
    const key = (event: KeyboardEvent) => { if ((event.metaKey || event.ctrlKey) && event.shiftKey && event.key.toLowerCase() === "c") { event.preventDefault(); void copy("items"); } };
    document.addEventListener("keydown", key);
    return () => document.removeEventListener("keydown", key);
  }, [meeting.notes_md, shown]);
  // The transcript copies as saved, with your corrections: "[mm:ss] Me: …" and "Them:" per line.
  async function copy(kind: "items" | "markdown" | "plain" | "transcript") {
    try { await navigator.clipboard.writeText(kind === "transcript" ? meeting.transcript.trim() : notesText(shown ?? meeting.notes_md, kind, meeting.notes_md)); setMessage(s.copied); setMenu(null); }
    catch (error) { onError(String(error)); }
  }
  async function run(action: "reveal" | "regenerate" | "retranscribe") {
    setBusy(true); setMenu(null);
    // The question is answered either way; a failure shows in the page's alert, not under it.
    // The menu is hidden while the job runs, so the parent shows what finished.
    try { await meetingAction(meeting.summary.id, action, action === "regenerate" ? language : undefined); if (action !== "reveal") await onChanged(false); if (action === "retranscribe") onToast?.(s.retranscribed); }
    catch (error) {
      // Stop on the steps page is not a failure: nothing was replaced (owner 2026-10-25).
      if (String(error) === STOPPED) { await onChanged(false); onToast?.(action === "retranscribe" ? s.stoppedTranscript : s.stoppedNotes); }
      else onError(String(error));
    }
    finally { setBusy(false); setConfirm(null); }
  }
  return <div className="headacts" ref={root} onKeyDown={event => {
    if (event.key === "Escape") { setMenu(null); root.current?.querySelector<HTMLButtonElement>(`[data-menu="${menu}"]`)?.focus(); }
    if (menu && ["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
      event.preventDefault(); const items = Array.from(root.current?.querySelectorAll<HTMLButtonElement>('[role^="menuitem"]:not(:disabled)') ?? []);
      const index = items.indexOf(document.activeElement as HTMLButtonElement);
      items[event.key === "Home" ? 0 : event.key === "End" ? items.length - 1 : (index + (event.key === "ArrowUp" ? -1 : 1) + items.length) % items.length]?.focus();
    }
  }}>
    <button className="doc-action" data-menu="copy" aria-haspopup="menu" aria-expanded={menu === "copy"} disabled={busy} onClick={() => setMenu(menu === "copy" ? null : "copy")}>{s.copyMenu}<ChevronDown/></button>
    <button className="doc-action ghost" data-menu="more" aria-label={s.moreActions} aria-haspopup="menu" aria-expanded={menu === "more"} disabled={busy} onClick={() => setMenu(menu === "more" ? null : "more")}>•••</button>
    {menu === "copy" && <div className="doc-dropdown" role="menu" aria-label="Copy">
      <button role="menuitem" onClick={() => copy("items")}><span>{s.copyItems}</span><KbdGroup keys="⇧⌘C"/></button><button role="menuitem" onClick={() => copy("markdown")}>{s.copyMarkdown}</button><button role="menuitem" onClick={() => copy("plain")}>{s.copyPlain}</button><div className="sep"/><button role="menuitem" disabled={!meeting.transcript.trim()} onClick={() => copy("transcript")}>{s.copyTranscript}</button>
    </div>}
    {menu === "more" && <div className="doc-dropdown gmenu" role="menu" aria-label={s.moreActions}>
      {/* Locked notes are read-only for review (SPEC 7.11): no rename, no new notes over them. */}
      <MenuItem icon="rename" label={s.rename} disabled={locked} title={locked ? s.lockedHint : undefined} onClick={() => { setMenu(null); onRename(); }}/>
      <MenuItem icon="again" label={s.regenerate} disabled={locked} title={locked ? s.lockedHint : undefined} onClick={() => askRegenerate()}/>
      <MenuItem icon="wave" label={s.retranscribe} disabled={locked || !hasAudio} title={locked ? s.lockedHint : !hasAudio ? s.noAudio : undefined} onClick={() => { setConfirm("retranscribe"); setMenu(null); }}/>
      <MenuItem icon="folder" label={s.reveal} onClick={() => run("reveal")}/>
      <div className="sep"/>
      <MenuSwitch icon="lock" label={s.lockNotes} checked={locked} onChange={onLock}/>
      <div className="sep"/>
      <MenuItem icon="trash" label={s.deleteMeeting} danger disabled={!deletable(meeting.summary.status)} title={deletable(meeting.summary.status) ? undefined : s.deleteBusy} onClick={() => { setMenu(null); onDelete(); }}/>
    </div>}
    {confirm && <Alert variant="warning" className="doc-confirm" role="alertdialog" title={confirm === "retranscribe" ? s.retranscribe : s.regenerate} actions={<><button className="doc-action" disabled={busy} onClick={() => run(confirm)}>{busy ? s.processing : confirm === "regenerate" && language !== "same_as_call" ? s.writeInGo.replace("{lang}", nativeName(language)) : s.confirm}</button><button className="doc-action ghost" disabled={busy} onClick={() => setConfirm(null)}>{s.cancel}</button></>}>
      {confirm === "retranscribe" ? corrections ? s.retranscribeEdited.replace("{n}", String(corrections)) : s.retranscribeConfirm
        : <>{s.regenerateConfirm}<WriteInSelect value={language} onChange={setLanguage} call={meeting.summary.language} disabled={busy}/><span className="writein-help">{s.writeInHelp}</span></>}
    </Alert>}
    <span className="sr-only" role="status">{message}</span>
  </div>;
}
