"use client";
// The microphone picker (owner 2026-10-23, DESIGN 4.15; mockup .micchip / .micnote / .micmenu):
// it sits where call apps put theirs, beside your own side of the call, and a mic problem shows
// once, in amber, with the one button that fixes it.
import { useCallback, useEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import * as ipc from "@/lib/ipc";
import type { Mic } from "@/lib/bindings/Mic";
import type { MicReport } from "@/lib/bindings/MicReport";

const svg = (d: ReactNode, width = 1.4) => <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth={width} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{d}</svg>;
export const MicIcon = () => svg(<><rect x="6" y="1.8" width="4" height="8" rx="2"/><path d="M3.6 7.6a4.4 4.4 0 0 0 8.8 0M8 12v2.2"/></>);
export const MicOffIcon = () => svg(<><rect x="6" y="1.8" width="4" height="8" rx="2"/><path d="M3.6 7.6a4.4 4.4 0 0 0 8.8 0M8 12v2.2M2.4 2.4l11.2 11.2"/></>);
export const WarnIcon = () => svg(<><path d="M8 2.2 14.2 13H1.8z"/><path d="M8 6.4v3M8 11.1v.1"/></>, 1.5);
const CheckIcon = () => svg(<path d="m3.5 8.4 3 3 6-6.8"/>, 1.7);
const CloseIcon = () => svg(<path d="m4 4 8 8m0-8-8 8"/>, 1.6);
const GearIcon = () => svg(<><circle cx="8" cy="8" r="2.2"/><path d="M8 1.8v1.6M8 12.6v1.6M1.8 8h1.6M12.6 8h1.6M3.6 3.6l1.1 1.1M11.3 11.3l1.1 1.1M3.6 12.4l1.1-1.1M11.3 4.7l1.1-1.1"/></>);
const SlidersIcon = () => svg(<><path d="M2.5 4h11M2.5 8h11M2.5 12h11"/><circle cx="5.5" cy="4" r="1.3"/><circle cx="10.5" cy="8" r="1.3"/><circle cx="6.5" cy="12" r="1.3"/></>);
const Chevron = () => <svg className="chv" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="m6 9 6 6 6-6"/></svg>;

const KIND: Record<Mic["transport"], string> = { BuiltIn: "Built-in", Bluetooth: "Bluetooth", Usb: "USB", Virtual: "Virtual", Other: "External" };
/** Facts only, "Bluetooth · 16 kHz": Motetto can't rank mics, so it never calls one better (owner 2026-10-23). */
export const micSub = (m: Mic) => m.rate_hz > 0 ? `${KIND[m.transport]} · ${+(m.rate_hz / 1000).toFixed(1)} kHz` : KIND[m.transport];
/** Mirrors `motetto_core::mics::quieter`: a third less, never under a quarter. */
const quieter = (volume: number) => Math.max(volume * .67, .25);
const percent = (volume: number) => Math.round(volume * 100);

/** The rows to offer: System default, then every other mic. The default device is listed once,
 *  as System default (owner 2026-11-05), and a saved choice of that same device shows there. */
export function micChoices(report: MicReport) {
  const system = report.mics.find(m => m.is_default);
  return { system, others: report.mics.filter(m => m !== system), chosen: report.chosen === system?.id ? null : report.chosen };
}

/** The report, checked every 2 s while shown and at once when any window changes the mic. */
export function useMicReport(enabled = true) {
  const [report, setReport] = useState<MicReport | null>(null);
  const refresh = useCallback(() => ipc.microphones().then(setReport).catch(() => {}), []);
  useEffect(() => {
    if (!enabled || !ipc.desktopAvailable()) return;
    let off: (() => void) | undefined;
    let active = true;
    void refresh();
    const timer = setInterval(refresh, 2000);
    void ipc.subscribe("motetto://mic-changed", () => void refresh()).then(fn => { if (active) off = fn; else fn(); });
    return () => { active = false; clearInterval(timer); off?.(); };
  }, [enabled, refresh]);
  return { report, setReport, refresh };
}

type Done = (text: string, undo?: () => Promise<void>) => void;

/** Switches the mic and says so, with Undo back to the previous choice. `off`: the recording's
 *  mic is None now; a mic picked from here brings it back. */
export async function pickMic(report: MicReport, id: string | null, done: Done, recording: boolean, off = false) {
  const before = report.chosen;
  const next = await ipc.setMicrophone(id);
  const name = next.active?.name ?? "the system default";
  done(recording ? `Recording from ${name} now. It's still one meeting.` : `Motetto records from ${name}.`, async () => { if (off) await ipc.setMicChoice({ kind: "Off" }); else await ipc.setMicrophone(before); });
  return next;
}

/** None during a recording (owner 2026-11-05): the next part records no mic, so no Me track. */
export async function pickNoMic(report: MicReport, done: Done) {
  const before = report.chosen;
  const next = await ipc.setMicChoice({ kind: "Off" });
  done("Your mic is off. Only the other side is recorded now.", async () => { await ipc.setMicrophone(before); });
  return next;
}

/** Five bars following your side's level (a plain read; the companion's Duet reads it too). */
export function MicLevel({ bars = 5, className = "miclvl", hot = false }: { bars?: number; className?: string; hot?: boolean }) {
  const [level, setLevel] = useState(0);
  useEffect(() => {
    if (!ipc.desktopAvailable()) return;
    let active = true;
    const tick = () => ipc.getLevels().then(([mic]) => { if (active) setLevel(mic); }).catch(() => {});
    const timer = setInterval(tick, 150);
    return () => { active = false; clearInterval(timer); };
  }, []);
  const lit = Math.round(Math.min(1, level * 5) * bars);
  return <span className={className} aria-hidden="true">{Array.from({ length: bars }, (_, i) => <i key={i} className={i < lit ? (hot && i === bars - 1 ? "hot" : "on") : ""}/>)}</span>;
}

/** The amber row, only for what Motetto measured: clipping (3 of 5 two-second checks). One action
 *  on the chosen mic, and ✕ until the next recording. Motetto never warns about which mic you chose. */
export function MicNote({ report, done, onError = done, onHide, className = "micnote" }: { report: MicReport | null; done: Done; onError?: (error: string) => void; onHide?: () => void; className?: string }) {
  const [busy, setBusy] = useState(false);
  if (report?.issue !== "TooLoud" || !report.active) return null;
  const mic = report.active;
  const why = "Loud speech is clipping and words get distorted in the transcript.";
  const run = (work: () => Promise<unknown>) => { setBusy(true); void work().catch(e => onError(String(e))).finally(() => setBusy(false)); };
  const fix = mic.volume !== null
      ? <button type="button" className="btn sm" disabled={busy} onClick={() => run(async () => { const was = mic.volume!; const to = quieter(was); await ipc.setMicVolume(mic.id, to); done(`Input volume lowered to ${percent(to)}% for ${mic.name}.`, async () => { await ipc.setMicVolume(mic.id, was); }); })}>Lower input volume</button>
      : <button type="button" className="btn sm" onClick={() => run(ipc.openSoundSettings)}>Sound settings…</button>;
  return <div className={className} role="status">
    <WarnIcon/><span className="tx" title={why}>Your mic is too loud.</span>
    {fix}
    {onHide && <button type="button" className="x" aria-label="Hide until the next recording" title="Hide until the next recording" onClick={onHide}><CloseIcon/></button>}
  </div>;
}

/** The recording page's chip: the mic's name, its level and ▾; amber only while it clips. */
export function MicChip({ report, open, onToggle, off = false }: { report: MicReport | null; open: boolean; onToggle: (button: HTMLButtonElement) => void; off?: boolean }) {
  const mic = report?.active;
  if (!mic) return null;
  const name = off ? "No microphone" : mic.name, issue = off ? null : report.issue;
  return <button type="button" className={`micchip${issue ? " warn" : ""}`} aria-haspopup="menu" aria-expanded={open} aria-label={`Microphone: ${name}${issue ? ", needs attention" : ""}. Change`} title={name} onClick={e => onToggle(e.currentTarget)}>
    {issue ? <WarnIcon/> : <MicIcon/>}<span className="nm">{name}</span><MicLevel hot={issue === "TooLoud"}/><Chevron/>
  </button>;
}

/** Record from: system default, each mic (its kind and quality), input volume, Sound settings. */
/** `recording` offers None first (outside a recording the setup bar owns that choice); `off`:
 *  this recording's mic is None. */
export function MicMenu({ report, recording, off = false, done, onError = done, onClose, style, settingsLink = true }: { report: MicReport; recording: boolean; off?: boolean; done: Done; onError?: (error: string) => void; onClose: () => void; style?: CSSProperties; settingsLink?: boolean }) {
  const menu = useRef<HTMLDivElement>(null);
  const [volume, setVolume] = useState(report.active?.volume ?? null);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => { menu.current?.querySelector<HTMLElement>('[aria-checked="true"]')?.focus(); }, []);
  useEffect(() => () => clearTimeout(timer.current), []);
  const { system, others, chosen } = micChoices(report);
  const pick = (id: string | null) => { onClose(); void pickMic(report, id, done, recording, off).catch(e => onError(String(e))); };
  const option = (id: string | null, mic: Mic, label: string) => <button key={id ?? "system"} type="button" role="menuitemradio" className="mic" aria-checked={!off && chosen === id} onClick={() => pick(id)}>
    <MicIcon/><span className="t"><b>{label}</b><span>{micSub(mic)}</span></span><span className="ck">{!off && chosen === id && <CheckIcon/>}</span>
  </button>;
  const none = <button key="off" type="button" role="menuitemradio" className="mic" aria-checked={off} onClick={() => { onClose(); if (!off) void pickNoMic(report, done).catch(e => onError(String(e))); }}>
    <MicOffIcon/><span className="t"><b>None</b><span>Your voice isn&apos;t recorded</span></span><span className="ck">{off && <CheckIcon/>}</span>
  </button>;
  const active = report.active;
  return <div ref={menu} className="gmenu dropdown micmenu" role="menu" aria-label="Microphone" style={style} onKeyDown={e => {
    if (e.key === "Escape") { e.preventDefault(); onClose(); return; }
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const items = Array.from(menu.current?.querySelectorAll<HTMLElement>('[role^="menuitem"]') ?? []);
    const at = items.indexOf(document.activeElement as HTMLElement);
    items[(at + (e.key === "ArrowUp" ? -1 : 1) + items.length) % items.length]?.focus();
  }}>
    <div className="mhead">Record from</div>
    {recording && none}
    {system && option(null, system, `System default (${system.name})`)}
    {others.map(m => option(m.id, m, m.name))}
    <div className="sep"/>
    {active && volume !== null
      ? <label className="vol"><MicIcon/><input type="range" min={0} max={100} value={percent(volume)} style={{ "--v": volume } as CSSProperties} aria-label={`Input volume for ${active.name}`} onChange={e => {
          const next = Number(e.target.value) / 100; setVolume(next); clearTimeout(timer.current);
          timer.current = setTimeout(() => void ipc.setMicVolume(active.id, next).catch(error => onError(String(error))), 120);
        }}/><output>{percent(volume)}%</output></label>
      : active && <div className="mhead" style={{ fontWeight: 400 }}>{active.name} sets its own level.</div>}
    <div className="sep"/>
    <button type="button" role="menuitem" onClick={() => { onClose(); void ipc.openSoundSettings().catch(e => onError(String(e))); }}><GearIcon/><span>Sound settings…</span></button>
    {settingsLink && <button type="button" role="menuitem" onClick={() => { onClose(); void ipc.openSettings("audio").catch(e => onError(String(e))); }}><SlidersIcon/><span>Audio settings in Motetto</span></button>}
  </div>;
}
