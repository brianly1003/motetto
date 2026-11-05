"use client";
// The dark glass setup bar (mockup renderCapture): what to record, the mic with its live level,
// Options, and Record with the start timer. During a recording it only switches the source.
import { useEffect, useState, type CSSProperties, type ReactNode } from "react";
import * as ipc from "@/lib/ipc";
import type { Cap, Mode } from "@/lib/capture";
import type { MicReport } from "@/lib/bindings/MicReport";
import { micName, type Which } from "./CaptureMenus";
import { AREA_IC, CHEV_IC, Dial, FILM_IC, K, MIC_IC, MICOFF_IC, NOTES_IC, SCR_IC, WAVE_IC, WIN_IC, X_IC } from "./icons";

/** The system default mic's level before recording (a meter-only listen, nothing written). */
function CapLevel() {
  const [level, setLevel] = useState(0);
  useEffect(() => {
    if (!ipc.desktopAvailable()) return;
    let active = true;
    const tick = () => ipc.captureLevel().then(mic => { if (active) setLevel(mic); }).catch(() => {});
    const timer = setInterval(tick, 150);
    return () => { active = false; clearInterval(timer); };
  }, []);
  const lvl = Math.max(.15, Math.min(1, level * 5));
  return <span className="caplvl" aria-hidden="true" style={{ "--lvl": lvl } as CSSProperties}><i/><i/><i/><i/><i/></span>;
}

export function SetupBar({ cap, live, ready, what, report, menu, meter, onMode, onMenu, onGo, onClose }: {
  cap: Cap; live: boolean; ready: boolean; what: string; report: MicReport | null; menu: Which | null;
  /** The live level shows only where Motetto can listen before recording (the system default mic). */
  meter: boolean;
  onMode: (mode: Mode) => void; onMenu: (which: Which, button: HTMLElement) => void; onGo: () => void; onClose: () => void;
}) {
  const intro = !live, off = cap.mic.kind === "Off", name = micName(cap.mic, report);
  const mode = (id: Mode, ic: ReactNode, label: string) =>
    <button type="button" role="radio" className="mode" aria-checked={cap.mode === id} data-capmode={id} title={label} onClick={() => onMode(id)}>{ic}<span>{label}</span></button>;
  const title = cap.mode === "window" && !ready ? "Click a window to choose it first" : intro && cap.timer ? `Starts after ${cap.timer} seconds (Return). ${K("⌥", "Alt+")}Return starts now` : "Start (Return)";
  return <div className="capbar" id="capbar" role="toolbar" aria-label="Screen recording">
    <div className="modes" role="radiogroup" aria-label="What to record">
      {intro && <>{mode("audio", WAVE_IC, "Audio only")}<span className="vr thin"/></>}
      {mode("display", SCR_IC, "Entire screen")}{mode("window", WIN_IC, "Window")}{mode("area", AREA_IC, "Area")}
    </div><span className="vr"/>
    {intro && <>
      <button type="button" className={`chipb${off ? " off" : ""}`} data-cap="mic" aria-haspopup="menu" aria-expanded={menu === "mic"} aria-label={`Microphone: ${name}. Change`} title="Microphone" onClick={e => onMenu("mic", e.currentTarget)}>
        {off ? MICOFF_IC : MIC_IC}<span className="nm">{name}</span>{meter && <CapLevel/>}{CHEV_IC}
      </button>
      <button type="button" className="chipb" data-cap="opts" aria-haspopup="menu" aria-expanded={menu === "opts"} title="Options" onClick={e => onMenu("opts", e.currentTarget)}>
        {cap.notes ? NOTES_IC : FILM_IC}<span className="nm">{cap.notes ? "Notes after" : "Just video"}</span>{CHEV_IC}
      </button><span className="vr"/>
    </>}
    <div className={`recsplit${ready ? "" : " off"}`}>
      <button type="button" className="rec" data-cap="go" disabled={!ready} title={title} onClick={onGo}><i/>{intro ? "Record" : "Switch"}{what && <span className="meta">{what}</span>}</button>
      {intro && <button type="button" className="rectime" data-cap="timer" aria-haspopup="menu" aria-expanded={menu === "timer"} aria-label={`Starts ${cap.timer ? `after ${cap.timer} seconds` : "now"}. Change`} title="When recording starts" onClick={e => onMenu("timer", e.currentTarget)}>
        <Dial n={cap.timer}/><span>{cap.timer ? cap.timer + "s" : "Now"}</span>{CHEV_IC}
      </button>}
    </div>
    {/* First on the bar, last for Tab. */}
    <button type="button" className="capx lead" data-cap="close" aria-label="Cancel" title="Cancel (Esc)" onClick={onClose}>{X_IC}</button><span className="vr lead"/>
  </div>;
}

/** The line over the bar: what a click does here, and the keys. */
export function CapHint({ cap, own, screenName, meter = true }: { cap: Cap; own: boolean; screenName: string; meter?: boolean }) {
  let body: ReactNode;
  if (own) body = <>Motetto&apos;s own windows are never recorded. Pick another window.</>;
  else if (cap.mode === "audio") body = meter ? <>Your mic and the call&apos;s sound, no screen. Check your mic&apos;s level, then <kbd className="kbd">Return</kbd> records</> : <>Your mic and the call&apos;s sound, no screen. <kbd className="kbd">Return</kbd> records</>;
  else if (cap.mode === "window") body = cap.sel ? <><b>{cap.sel.title || cap.sel.app}</b> chosen. <kbd className="kbd">Return</kbd> records · click another window to change</> : <>Click a window to choose it. Nothing records until you press Record. <kbd className="kbd">Esc</kbd> cancels</>;
  else if (cap.mode === "area") body = <>Drag to draw an area, then move it or drag its edges. <kbd className="kbd">Return</kbd> records · <kbd className="kbd">Esc</kbd> cancels</>;
  else body = <>Click anywhere to record {screenName}, notifications included. <kbd className="kbd">Esc</kbd> cancels</>;
  return <div className="caphint" id="caphint">{body}</div>;
}
