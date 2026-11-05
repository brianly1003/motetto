"use client";
// The setup bar's menus (mockup capMenu): the microphone with None first, Options, and the start
// timer with its keys. They open above their button, as the bar sits at the bottom of the screen.
import { useLayoutEffect, useRef, type ReactNode } from "react";
import type { Cap } from "@/lib/capture";
import type { MicChoice } from "@/lib/bindings/MicChoice";
import type { MicReport } from "@/lib/bindings/MicReport";
import { micChoices, micSub } from "@/components/Microphone";
import { CHECK_IC, CLICK_IC, Dial, FILM_IC, K, LOCK_IC, MIC_IC, MICOFF_IC, NOTES_IC, SPK_IC } from "./icons";

export type Which = "mic" | "opts" | "timer";
const SUB: Record<number, string> = { 0: "The moment you press Record", 3: "Time to bring the window forward", 5: "Time to put the pointer in place", 10: "Time to set the scene" };

/** The device the mic choice records from: `null` is the system default's row. */
export function micRow(mic: MicChoice, report: MicReport | null): string | null | "off" {
  if (mic.kind === "Off") return "off";
  const { system, chosen } = report ? micChoices(report) : { system: undefined, chosen: null };
  const id = mic.kind === "Device" ? mic.id : chosen;
  return id === system?.id ? null : id;
}

export function micName(mic: MicChoice, report: MicReport | null) {
  const row = micRow(mic, report);
  if (row === "off") return "No mic";
  const system = report?.mics.find(m => m.is_default);
  if (row === null) return system ? `System default (${system.name})` : "System default";
  return report?.mics.find(m => m.id === row)?.name ?? "Microphone";
}

export function CaptureMenu({ which, cap, report, anchor, onSet, onClose }: {
  which: Which; cap: Cap; report: MicReport | null; anchor: HTMLElement | null;
  onSet: (key: string, mic?: MicChoice) => void; onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  // Above its button, kept on screen (mockup: 8 from the edges, 10 over the bar).
  useLayoutEffect(() => {
    const m = ref.current, b = anchor?.getBoundingClientRect();
    if (!m || !b) return;
    m.style.left = Math.max(8, Math.min(b.left, innerWidth - m.offsetWidth - 8)) + "px";
    m.style.top = Math.max(8, b.top - m.offsetHeight - 10) + "px";
  });
  useLayoutEffect(() => { ref.current?.querySelector<HTMLElement>('[aria-checked="true"]')?.focus(); }, [which]);
  const row = (set: string, ic: ReactNode, label: string, on: boolean, sub = "", mic?: MicChoice) =>
    <button key={set} type="button" role="menuitemradio" aria-checked={on} data-capset={set} onClick={() => onSet(set, mic)}>
      {ic}<span>{label}{sub && <span className="sub">{sub}</span>}</span><span className="ck">{on && CHECK_IC}</span>
    </button>;
  let body: ReactNode;
  if (which === "timer") {
    body = <>
      <div className="mh">Start recording</div>
      {[0, 3, 5, 10].map(n => row("timer:" + n, <Dial n={n}/>, n ? `After ${n} seconds` : "Now", cap.timer === n, SUB[n]))}
      <div className="sep"/>
      <div className="keys"><span>Skip the timer this once<kbd className="kbd">{K("⌥", "Alt+")}Return</kbd></span><span>Start during the countdown<kbd className="kbd">Return</kbd></span><span>Back to the setup<kbd className="kbd">Esc</kbd></span></div>
    </>;
  } else if (which === "mic") {
    const choices = report ? micChoices(report) : null, at = micRow(cap.mic, report);
    // The system default's row follows the default unless Settings names a mic of its own.
    const systemPick: MicChoice = !report?.chosen || report.chosen === choices?.system?.id ? { kind: "Default" } : { kind: "Device", id: choices!.system!.id };
    body = <>
      <div className="mh">Microphone</div>
      {row("mic:off", MICOFF_IC, "None", at === "off", "Your voice isn't recorded", { kind: "Off" })}
      {choices?.system && row("mic:", MIC_IC, `System default (${choices.system.name})`, at === null, "", systemPick)}
      {choices?.others.map(m => row("mic:" + m.id, MIC_IC, m.name, at === m.id, micSub(m), { kind: "Device", id: m.id }))}
      <div className="sep"/><div className="mh">Computer audio</div>
      {row("sys", SPK_IC, "Sounds from apps", cap.sys, cap.sys ? "Videos, calls and alerts, on their own track" : "Off: the video is silent unless the mic is on")}
    </>;
  } else {
    const silent = cap.mic.kind === "Off" && !cap.sys;
    body = <>
      {cap.mode !== "audio" && <><div className="mh">After recording</div>{row("notes:0", FILM_IC, "Just the video", !cap.notes, "No transcript, no notes")}</>}
      {cap.mode !== "audio" && row("notes:1", NOTES_IC, "Transcript and notes", cap.notes, silent ? "Needs a mic or computer audio" : `Written on this ${K("Mac", "PC")}, like a call`)}
      <div className="sep"/><div className="mh">Options</div>
      {row("clicks", CLICK_IC, "Show mouse clicks", cap.clicks)}
      {row("remember", CHECK_IC, "Remember last selection", cap.remember)}
      <div className="sep"/><p className="note">{LOCK_IC} Saved to your Motetto library on this {K("Mac", "PC")}. About 110 MB an hour.</p>
    </>;
  }
  return <div ref={ref} className="capmenu" data-for={which} role="menu" aria-label={which === "mic" ? "Microphone" : which === "timer" ? "Start recording" : "Options"}
    onKeyDown={e => {
      if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
      e.preventDefault();
      const items = Array.from(ref.current?.querySelectorAll<HTMLElement>('[role="menuitemradio"]') ?? []);
      const at = items.indexOf(document.activeElement as HTMLElement);
      items[(at + (e.key === "ArrowUp" ? -1 : 1) + items.length) % items.length]?.focus();
    }}
    onBlur={e => { if (!e.currentTarget.contains(e.relatedTarget as Node) && !(e.relatedTarget as Element | null)?.closest?.("[data-cap]")) onClose(); }}>
    {body}
  </div>;
}
