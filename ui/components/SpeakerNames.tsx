import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type MouseEvent as PressEvent } from "react";
import type { Meeting } from "@/lib/bindings/Meeting";
import { transcriptLines } from "@/lib/transcript";
import { strings as s } from "@/lib/strings";
import * as ipc from "@/lib/ipc";
import { CheckboxField } from "@/components/ui/Checkbox";
import { useFirstClick } from "@/lib/firstClick";
import { InputClear } from "@/components/ui/InputClear";
import { notesMentions, personOf, renameInNotes, saidNames, voiceColor, voiceOf, type SpeakerNames } from "@/lib/speakerNames";

type Line = ReturnType<typeof transcriptLines>[number];
type Scope = "line" | "run" | "all";
const lineCount = (n: number) => n === 1 ? s.lineCount : s.linesCount.replace("{n}", String(n));
const PLAY = <svg viewBox="0 0 16 16" fill="currentColor" aria-hidden="true"><path d="M4.5 2.8v10.4a.6.6 0 0 0 .9.5l8-5.2a.6.6 0 0 0 0-1l-8-5.2a.6.6 0 0 0-.9.5z"/></svg>;
const PAUSE = <svg viewBox="0 0 16 16" fill="currentColor" aria-hidden="true"><rect x="4" y="3" width="2.6" height="10" rx="1"/><rect x="9.4" y="3" width="2.6" height="10" rx="1"/></svg>;
const WARN = <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M8 2.2 14.2 13H1.8z"/><path d="M8 6.4v3M8 11.1v.1"/></svg>;

/** The people heard: You, then each person on the call (voices sharing a name count as one). */
export function people(lines: Line[], names: SpeakerNames) {
  const voices = [...new Set(lines.map(l => voiceOf(l.speaker)).filter(Boolean))];
  const persons = [...new Set(voices.map(v => personOf(v, names)))];
  const count = (p: number) => lines.filter(l => p ? voiceOf(l.speaker) && personOf(voiceOf(l.speaker), names) === p : l.speaker === "You").length;
  return { persons, voices, count };
}

// People (mockup .panel.people): each person, their share of the lines; hover lights their lines,
// a click names them. Shown once Find speakers has told voices apart.
export function PeoplePanel({ meeting, locked, onOpen, onSpot, onTogglePlay }: { meeting: Meeting; locked: boolean; onOpen: (voice: number, anchor: HTMLElement) => void; onSpot: (person: number | null) => void; onTogglePlay?: () => void }) {
  const lines = transcriptLines(meeting.transcript, []), names = meeting.summary.speaker_names ?? {};
  const { persons, voices, count } = people(lines, names);
  if (!persons.length) return null;
  const row = (p: number) => {
    const merged = voices.filter(v => v !== p && personOf(v, names) === p);
    const name = p ? names[p] ?? `Speaker ${p}` : "You";
    return <button type="button" key={p} className="person" data-person={p} style={{ "--vc": p ? voiceColor(p) : "var(--voice-me)" } as CSSProperties}
      aria-haspopup={p ? "dialog" : undefined} disabled={!p || locked} title={p && !locked ? s.renameSpeaker.replace("{name}", name) : undefined}
      onMouseEnter={() => onSpot(p)} onMouseLeave={() => onSpot(null)} onFocus={() => onSpot(p)} onBlur={() => onSpot(null)}
      onKeyDown={event => { if (event.key === " ") { event.preventDefault(); onTogglePlay?.(); } }} onKeyUp={event => { if (event.key === " ") event.preventDefault(); }}
      onClick={event => { if (p) onOpen(p, event.currentTarget); }}>
      <i/><b>{name}{merged.length ? <small> + Speaker {merged.join(", ")}</small> : p && !names[p] ? <small> {s.unnamed}</small> : null}</b><em>{lineCount(count(p))}</em><span className="bar"><u style={{ width: `${Math.round(count(p) / Math.max(1, lines.length) * 100)}%` }}/></span>
    </button>;
  };
  return <div className="panel people"><h5>{s.people}</h5><div>{[0, ...persons].map(row)}</div><p className="tip">{s.peopleTip}</p></div>;
}

// Who is Speaker N? (mockup .pop.wide.spk): a name for every line by this voice, a sample to hear
// it, names said in the call or already given, and the notes' `Speaker N` with it. Opened from a
// line it first asks what to change: this line, the lines in a row by this voice, or every line
// (the default); a line or a run moves to someone else on the call, or to a new voice with the
// typed name, and the notes stay. Enter applies, Esc or a click elsewhere cancels; it follows its
// name while the page scrolls. The sample button is the player's toggle while audio plays.
export function SpeakerPopover({ meeting, voice, anchor, onClose, onChange, onRenamed, onSeek, playing = false, onTogglePlay, onToast, onError }: {
  meeting: Meeting; voice: number; anchor: HTMLElement; onClose: () => void; onChange: (meeting: Meeting) => void;
  onRenamed: (voices: number[], from: number) => void; onSeek?: (ms: number) => void; playing?: boolean; onTogglePlay?: () => void;
  onToast?: (text: string, undo?: () => Promise<void>, label?: string) => void; onError: (error: string) => void;
}) {
  const names = meeting.summary.speaker_names ?? {};
  const lines = transcriptLines(meeting.transcript, []);
  const mine = lines.map((line, index) => ({ line, index })).filter(({ line }) => voiceOf(line.speaker) === voice);
  // The line it was opened from, and the run of lines by this voice around it.
  const [at] = useState(() => Number(anchor.closest<HTMLElement>(".tline")?.dataset.line ?? -1));
  let from = at, to = at;
  if (at >= 0) { while (from > 0 && voiceOf(lines[from - 1].speaker) === voice) from--; while (to < lines.length - 1 && voiceOf(lines[to + 1].speaker) === voice) to++; }
  const run = at >= 0 ? to - from + 1 : 0;
  const scopes: [Scope, string][] = at < 0 || mine.length < 2 ? [] : [["line", s.scopeLine], ...(run > 1 && run < mine.length ? [["run", s.scopeRun.replace("{n}", String(run))] as [Scope, string]] : []), ["all", s.scopeAll.replace("{n}", String(mine.length))]];
  // From a line, only that line changes until the user picks more: a rename of every line cannot
  // be taken back once its toast has gone.
  const [scope, setScope] = useState<Scope>(() => scopes.length ? "line" : "all");
  const all = scope === "all";
  const picked = scope === "line" ? [at] : scope === "run" ? Array.from({ length: run }, (_, k) => from + k) : mine.map(({ index }) => index);
  const voices = [...new Set(lines.map(l => voiceOf(l.speaker)).filter(Boolean))];
  const self = personOf(voice, names);
  const longest = all ? [...mine].sort((a, b) => b.line.text.length - a.line.text.length)[0] : { line: lines[picked[0]], index: picked[0] };
  // Every line: names already given. A line: everyone else on the call. Then names said.
  const named: [string, number][] = all
    ? [...new Set(Object.values(names))].filter(v => v !== names[voice]).map(v => [v, Number(Object.entries(names).find(([, n]) => n === v)?.[0])])
    : [...new Set(voices.map(v => personOf(v, names)))].filter(p => p !== self).map(p => [names[p] ?? `Speaker ${p}`, p]);
  const said = saidNames(lines.map(l => l.text)).filter(v => !named.some(([w]) => w === v) && v !== names[voice]);
  const mentions = notesMentions(meeting.notes_md, voice);
  const [value, setValue] = useState(names[voice] ?? "");
  const [notes, setNotes] = useState(true);
  const [busy, setBusy] = useState(false);
  const box = useRef<HTMLDivElement>(null), input = useRef<HTMLInputElement>(null);
  const name = value.trim();
  const owner = Object.entries(names).find(([k, v]) => Number(k) !== voice && v === name)?.[0];
  const current = names[voice] ?? `Speaker ${voice}`;
  // A typed `Speaker K` or name of someone already on the call: their voice; 0 for a new voice.
  const target = (v: string) => {
    const k = Number(/^Speaker (\d+)$/.exec(v)?.[1] ?? 0);
    if (k && voices.includes(k)) return k;
    const named = Object.entries(names).find(([, n]) => n === v)?.[0];
    return named ? personOf(Number(named), names) : 0;
  };
  // A press anywhere in the popover but the name field keeps focus in the field, so typing goes on
  // where it was after a tab, a chip or the checkbox.
  const keep = (event: PressEvent) => { if (!(event.target as Element).closest("input[type=text]")) event.preventDefault(); };
  // The name typed stays when the choice changes.
  const choose = (next: Scope) => { setScope(next); input.current?.focus(); };

  // Under the name, or above it without room below; a name scrolled out of view closes it.
  const place = () => {
    const pop = box.current; if (!pop) return;
    const r = anchor.getBoundingClientRect();
    if (!anchor.isConnected || r.bottom < 0 || r.top > innerHeight) { onClose(); return; }
    const below = r.bottom + 6 + pop.offsetHeight <= innerHeight;
    pop.style.left = `${Math.max(12, Math.min(r.left - 6, innerWidth - pop.offsetWidth - 12))}px`;
    pop.style.top = `${below ? r.bottom + 6 : Math.max(8, r.top - pop.offsetHeight - 6)}px`;
  };
  useLayoutEffect(place);
  // One click per control even when an input method reorders the press and release (firstClick.ts).
  useFirstClick(box);
  useEffect(() => {
    input.current?.focus(); input.current?.select();
    anchor.setAttribute("aria-expanded", "true");
    const away = (event: MouseEvent) => { const t = event.target as Node; if (!box.current?.contains(t) && !anchor.contains(t)) onClose(); };
    const key = (event: KeyboardEvent) => { if (event.key === "Escape") { event.stopPropagation(); onClose(); } };
    document.addEventListener("mousedown", away, true); document.addEventListener("keydown", key, true);
    document.addEventListener("scroll", place, true); window.addEventListener("resize", place);
    return () => {
      anchor.setAttribute("aria-expanded", "false");
      document.removeEventListener("mousedown", away, true); document.removeEventListener("keydown", key, true);
      document.removeEventListener("scroll", place, true); window.removeEventListener("resize", place);
      // Focus goes back to the line (or the transcript, from People), as after Esc in a line's words:
      // Space then plays, and never presses the name again to reopen this. On the next frame: focus
      // in the same commit as a rename forces a layout of every row of a long call before paint.
      // Only when nothing else took focus in the meantime (a click or Tab to another name).
      const back = anchor.closest<HTMLElement>(".tline") ?? document.querySelector<HTMLElement>(".tlist");
      requestAnimationFrame(() => { if (back?.isConnected && (!document.activeElement || document.activeElement === document.body)) back.focus({ preventScroll: true }); });
    };
  }, [anchor]); // eslint-disable-line react-hooks/exhaustive-deps

  const blocked = busy || (all ? name === (names[voice] ?? "") : !name || target(name) === self);

  async function apply() {
    if (blocked) return;
    setBusy(true);
    const id = meeting.summary.id, before = { ...names };
    try {
      if (!all) {
        // Someone on the call takes these lines; a new name is a new voice first.
        let k = target(name), summary = meeting.summary;
        if (!k) { k = Math.max(...voices, ...Object.keys(names).map(Number)) + 1; summary = await ipc.setSpeakerNames(id, { ...names, [k]: name }); }
        const was = meeting.transcript_heard || meeting.transcript;
        const text = await ipc.moveTranscriptLines(id, picked, k);
        onChange({ ...meeting, summary, transcript: text, transcript_heard: text === was ? "" : was });
        onRenamed([k], picked[0]);
        onClose();
        const said = summary.speaker_names?.[k] ?? `Speaker ${k}`;
        onToast?.(picked.length === 1 ? s.movedOne.replace("{name}", said) : s.movedMany.replace("{n}", String(picked.length)).replace("{name}", said), async () => {
          const back = await ipc.moveTranscriptLines(id, picked, voice);
          const restored = summary === meeting.summary ? meeting.summary : await ipc.setSpeakerNames(id, before);
          onChange({ ...meeting, summary: restored, transcript: back, transcript_heard: back === was ? "" : was });
        });
        return;
      }
      const next = { ...names };
      if (name) next[voice] = name; else delete next[voice];
      const summary = await ipc.setSpeakerNames(id, next);
      const saved = summary.speaker_names?.[voice];
      let markdown = meeting.notes_md, notesBefore: string | null = null;
      if (saved && notes && mentions) { notesBefore = markdown; markdown = await ipc.saveNotes(id, renameInNotes(markdown, voice, saved), markdown); }
      onChange({ ...meeting, summary, notes_md: markdown });
      onRenamed(lines.map(l => voiceOf(l.speaker)).filter(v => v && personOf(v, summary.speaker_names ?? {}) === personOf(voice, summary.speaker_names ?? {})), at >= 0 ? at : mine[0]?.index ?? 0);
      onClose();
      onToast?.(saved ? s.speakerRenamed.replace("{n}", String(voice)).replace("{name}", saved).replace("{lines}", lineCount(mine.length)) : s.speakerBack.replace("{n}", String(voice)), async () => {
        const back = await ipc.setSpeakerNames(id, before);
        const restored = notesBefore === null ? markdown : await ipc.saveNotes(id, notesBefore, markdown);
        onChange({ ...meeting, summary: back, notes_md: restored });
      });
    } catch (error) { onError(String(error)); setBusy(false); }
  }

  const label = all ? s.speakerNameFor.replace("{n}", String(voice)) : s.whoSaid;
  return <div ref={box} className="pop wide spk" role="dialog" aria-label={label} style={{ "--vc": voiceColor(self) } as CSSProperties} onMouseDown={keep}>
    <h4><i/>{all ? s.whoIs.replace("{name}", current) : picked.length === 1 ? s.whoSaidOne : s.whoSaidMany.replace("{n}", String(picked.length))}</h4>
    <p className="help">{all ? (mine.length === 1 ? s.speakerLineHelp : s.speakerLinesHelp.replace("{n}", String(mine.length))) : picked.length === 1 ? s.moveHelpOne.replace("{name}", current) : s.moveHelpMany.replace("{n}", String(picked.length)).replace("{name}", current)}</p>
    {scopes.length > 0 && <div className="segm" role="radiogroup" aria-label={s.speakerScope}>{scopes.map(([k, text]) => <button key={k} type="button" role="radio" aria-checked={scope === k} onClick={() => choose(k)}>{text}</button>)}</div>}
    <InputClear ref={input} value={value} onValueChange={setValue} clearLabel={s.clear} placeholder={all ? `Speaker ${voice}` : s.movePlaceholder.replace("{k}", String(voices.find(k => personOf(k, names) !== self) ?? voice + 1))} maxLength={40} autoComplete="off" spellCheck={false} aria-label={label}
      onKeyDown={event => { if (event.key === "Enter") { event.preventDefault(); void apply(); } }}/>
    {longest && <div className="sample"><button type="button" className="btn sm" aria-label={playing ? s.pause : all ? s.playVoice : s.playLine} disabled={!onSeek || longest.line.ms === null} onClick={event => { event.currentTarget.focus(); if (playing) onTogglePlay?.(); else if (longest.line.ms !== null) onSeek?.(longest.line.ms); }}>{playing ? PAUSE : PLAY}</button><q>{longest.line.text}</q></div>}
    {named.length + said.length > 0 && <div className="sugg">
      {named.map(([v, k]) => <button type="button" key={`g-${v}`} className="chip plain" style={{ "--vc": voiceColor(personOf(k, names)) } as CSSProperties} onClick={() => { setValue(v); input.current?.focus(); }}><i/>{v}</button>)}
      {said.map(v => <button type="button" key={`s-${v}`} className="chip plain" title={s.saidInCall} onClick={() => { setValue(v); input.current?.focus(); }}>{v}</button>)}
    </div>}
    {all && owner && <div className="merge">{WARN}<span>{s.speakerMergeWarn.replace("{other}", `Speaker ${owner}`).replace("{name}", name)}</span></div>}
    {/* Always offered, as in the mockup: ticked, the notes' `Speaker N` takes the name too; unticked,
        only the transcript does. Moving single lines never changes the notes. */}
    <CheckboxField checked={all && notes} disabled={!all} onCheckedChange={setNotes} label={s.speakerAlsoNotes}
      description={all ? (mentions === 0 ? s.speakerNotesNone : mentions === 1 ? s.speakerNotesOne : s.speakerNotesMany.replace("{count}", String(mentions))).replace("{n}", String(voice)) : s.moveNotes}/>
    <div className="row2"><span className="help">{all && !name ? s.speakerBack.replace("{n}", String(voice)) : ""}</span><button type="button" className="btn sm ghost" onClick={onClose}>{s.cancel}</button><button type="button" className="btn sm primary" disabled={blocked} onClick={() => void apply()}>{!all ? s.move : owner ? s.speakerMerge : s.rename}</button></div>
  </div>;
}
