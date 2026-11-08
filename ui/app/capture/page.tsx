"use client";
// The screen setup overlay (Rust: capture_overlay.rs), one transparent window per display: the
// setup bar on the main display, and on every display the window, area or screen to record.
// Everything is chosen before the first frame; a countdown on the target can still go Back.
import { Suspense, useCallback, useEffect, useRef, useState } from "react";
import { useSearchParams } from "next/navigation";
import * as ipc from "@/lib/ipc";
import { capMode, capSet, countLeft, fromMemory, memoryOf, nudge, recordSetup, screenPick, sizeOf, type Area, type Cap, type Mode, type Screen } from "@/lib/capture";
import type { CaptureKind } from "@/lib/bindings/CaptureKind";
import type { CaptureMemory } from "@/lib/bindings/CaptureMemory";
import type { CaptureWindow } from "@/lib/bindings/CaptureWindow";
import type { MicChoice } from "@/lib/bindings/MicChoice";
import { useMicReport } from "@/components/Microphone";
import { CaptureMenu, micRow, type Which } from "@/components/capture/CaptureMenus";
import { Countdown } from "@/components/capture/Countdown";
import { Picker } from "@/components/capture/Picker";
import { CapHint, SetupBar } from "@/components/capture/SetupBar";
import "../../styles/capture.css";

const DEFAULTS: Record<CaptureKind, CaptureMemory> = {
  Call: { mode: "audio", mic: { kind: "Default" }, app_sounds: true, notes: true, timer: 0, clicks: true, area: null, remember: true },
  Clip: { mode: "window", mic: { kind: "Off" }, app_sounds: true, notes: false, timer: 3, clicks: true, area: null, remember: true },
};
type Sync = { from: number; cap: Cap };

function Setup() {
  const params = useSearchParams();
  const display = Number(params.get("display") ?? 0), kind: CaptureKind = params.get("kind") === "Call" ? "Call" : "Clip";
  const main = params.get("main") !== "0", modeParam = params.get("mode");
  // The display holding the bar: an area drawn on no display in particular is on it.
  const mainDisplay = Number(params.get("main_display") ?? display);
  const [cap, setCapState] = useState<Cap | null>(null);
  const capRef = useRef<Cap | null>(null);
  const [live, setLive] = useState(false);
  const [windows, setWindows] = useState<CaptureWindow[]>([]);
  const [hover, setHover] = useState<CaptureWindow | null>(null);
  const [drawn, setDrawn] = useState<Area | null>(null);
  const [menu, setMenu] = useState<{ which: Which; anchor: HTMLElement } | null>(null);
  const [name, setName] = useState("this display");
  // Settings › Screen video: the picture while this setup hasn't chosen one.
  const [preset, setPreset] = useState("sharp");
  // Re-renders the countdown; the seconds left are read from the clock when drawn.
  const [, setNow] = useState(0);
  const started = useRef(false);
  const { report } = useMicReport(main);
  const scale = typeof window === "undefined" ? 1 : devicePixelRatio || 1;
  const here = (): Screen => ({ id: display, name, w: Math.round(innerWidth * scale), h: Math.round(innerHeight * scale) });
  const mine = useCallback((d: number | null) => d === null ? main : d === display, [main, display]);

  // Every change goes to the other displays' overlays, so there is one setup.
  const update = useCallback((next: Cap, send = true) => {
    capRef.current = next; setCapState(next);
    if (send) void ipc.sendCaptureSetup<Sync>({ from: display, cap: next }).catch(() => {});
  }, [display]);

  useEffect(() => {
    let active = true, off: (() => void) | undefined;
    const view = { w: innerWidth, h: innerHeight };
    void Promise.all([
      ipc.captureMemory(kind).catch(() => DEFAULTS[kind]),
      // A clip whose window closed is still under way: the window chosen here goes on with it.
      ipc.getState().then(s => ["Recording", "Paused"].includes(s.state.type) || (s.state.type === "Stopped" && !!s.screen_closed)).catch(() => false),
    ]).then(([memory, recording]) => {
      if (!active) return;
      setLive(recording);
      update(fromMemory(memory, modeParam, recording, view), false);
    });
    void ipc.captureWindows().then(w => { if (active) setWindows(w); }).catch(() => {});
    void ipc.getSettings().then(s => { if (active) setPreset(s.screen_preset); }).catch(() => {});
    void ipc.listScreenSources().then(o => {
      const d = o.find(o => o.pick.choice.kind === "Display" && o.pick.choice.id === display);
      if (active && d) setName(d.pick.name);
    }).catch(() => {});
    void ipc.onCaptureSetup<Sync>(s => { if (s.from !== display) { capRef.current = s.cap; setCapState(s.cap); } }).then(fn => { if (active) off = fn; else fn(); });
    return () => { active = false; off?.(); };
  }, [display, kind, modeParam, update]);

  // The bar's first focus: the chosen mode, so Return records and Tab walks the bar.
  const focused = useRef(false);
  useEffect(() => {
    if (!cap || focused.current || !main) return;
    focused.current = true;
    document.querySelector<HTMLElement>('#capbar [aria-checked="true"]')?.focus();
  }, [cap, main]);

  const shown = cap?.mode === "display" ? cap.screen ?? (main ? here() : null) : null;
  const what = !cap ? "" : cap.mode === "area" ? sizeOf((drawn ?? cap.area).w, (drawn ?? cap.area).h, scale)
    : cap.mode === "display" ? shown ? `${shown.w}×${shown.h}` : ""
    : cap.mode === "window" ? (cap.sel ?? (hover && !hover.own ? hover : null)) ? sizeOf((cap.sel ?? hover)!.width, (cap.sel ?? hover)!.height, scale) : "" : "";

  async function start(c: Cap) {
    if (started.current) return;
    started.current = true;
    const pick = screenPick(c, mainDisplay, scale, here());
    try {
      if (live) { if (pick) await ipc.pickCapture(pick); else started.current = false; }
      else await ipc.startCapture(kind, recordSetup(c, pick), memoryOf(c));
    } catch {
      // Back to the setup: a countdown that ended must not try again every tick.
      started.current = false;
      const now = capRef.current;
      if (now?.count) update({ ...now, count: null });
    }
  }

  function go(skipTimer = false) {
    const c = capRef.current;
    if (!c || c.count) return;
    let next = { ...c };
    if (next.mode === "display" && !next.screen) next.screen = here();
    if (next.mode === "window" && !next.sel) { if (!hover || hover.own) return; next.sel = hover; }
    setMenu(null);
    const n = !live && !skipTimer ? next.timer : 0;
    if (!n) { update(next); void start(next); return; }
    next = { ...next, count: { total: n, until: Date.now() + n * 1000, by: display } };
    update(next);
  }

  // The countdown: every display shows it; the display that started it starts the recording.
  useEffect(() => {
    if (!cap?.count) return;
    const timer = setInterval(() => {
      const t = Date.now(); setNow(t);
      const c = capRef.current;
      if (c?.count && c.count.by === display && countLeft(c.count, t) <= 0) void start(c);
    }, 100);
    return () => clearInterval(timer);
  });
  const startNow = () => {
    const c = capRef.current;
    if (!c?.count) return;
    if (c.count.by === display) void start(c);
    else update({ ...c, count: { ...c.count, until: Date.now() } });
  };
  const back = () => {
    const c = capRef.current;
    if (!c?.count) return;
    update({ ...c, count: null });
    requestAnimationFrame(() => document.querySelector<HTMLElement>("#capbar .rec")?.focus());
  };

  // Esc: a menu closes, the countdown goes back, else the setup closes. The overlay with the
  // keyboard answers; with none answering, Rust closes every overlay itself.
  const escape = () => {
    if (menu) { const b = menu.anchor; setMenu(null); b.focus(); return; }
    if (capRef.current?.count) { back(); return; }
    void ipc.closeCapture();
  };
  const escRef = useRef(escape); escRef.current = escape;
  useEffect(() => {
    let active = true, off: (() => void) | undefined;
    void ipc.onCaptureEsc(n => {
      if (!document.hasFocus()) return;
      void ipc.captureEscAck(n).catch(() => {});
      escRef.current();
    }).then(fn => { if (active) off = fn; else fn(); }).catch(() => {});
    return () => { active = false; off?.(); };
  }, []);

  // Any press but on a menu or its own button (which toggles it) closes the open menu, including
  // on the bar and, in Audio only, on the veil that lets clicks through; so does a click elsewhere.
  const menuRef = useRef(menu); menuRef.current = menu;
  const closing = useRef(false);
  useEffect(() => {
    const press = (e: PointerEvent) => {
      const t = e.target as Element;
      closing.current = !!menuRef.current && !t.closest?.(".capbar, .capmenu, .capcd");
      if (!t.closest?.(".capmenu, [aria-haspopup]")) setMenu(null);
    };
    const blur = () => setMenu(null);
    addEventListener("pointerdown", press, true);
    addEventListener("blur", blur);
    return () => { removeEventListener("pointerdown", press, true); removeEventListener("blur", blur); };
  }, []);

  const keyRef = useRef<(e: KeyboardEvent) => void>(() => {});
  keyRef.current = (e: KeyboardEvent) => {
    const c = capRef.current, t = e.target as Element;
    if (!c) return;
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); escape(); return; }
    if (e.key === "Enter" && c.count) { if (!t.closest?.("[data-count]")) { e.preventDefault(); startNow(); } return; }
    if (e.key === "Enter" && !t.closest?.('.capmenu, [data-cap="go"], [data-cap="close"], [data-cap="timer"]')) { e.preventDefault(); go(e.altKey); return; }
    if (e.key.startsWith("Arrow") && c.mode === "area" && !c.count && mine(c.area.display) && !t.closest?.(".capmenu")) {
      e.preventDefault(); update({ ...c, area: nudge(c.area, e.key, e.shiftKey, { w: innerWidth, h: innerHeight }) });
    }
  };
  useEffect(() => {
    const key = (e: KeyboardEvent) => keyRef.current(e);
    addEventListener("keydown", key, true);
    return () => removeEventListener("keydown", key, true);
  }, []);

  // The mic meter before recording: only the system default mic can be listened to (meter only).
  const preview = main && !live && !!cap && cap.mic.kind !== "Off" && !!report?.mics.some(m => m.is_default) && micRow(cap.mic, report) === null;
  const [previewing, setPreviewing] = useState(false);
  useEffect(() => {
    if (!preview || !ipc.desktopAvailable()) { setPreviewing(false); return; }
    let active = true;
    void ipc.captureMicPreview(true).then(() => { if (active) setPreviewing(true); }).catch(() => { if (active) setPreviewing(false); });
    return () => { active = false; void ipc.captureMicPreview(false).catch(() => {}); };
  }, [preview]);

  // Rust lets clicks through in Audio only, except over what this page shows.
  const sent = useRef("");
  useEffect(() => {
    if (!cap) return;
    const timer = setTimeout(() => {
      const rects = Array.from(document.querySelectorAll(".capbar, .caphint, .capmenu, .capcd"), e => {
        const r = e.getBoundingClientRect();
        return [r.left, r.top, r.width, r.height].map(Math.round) as [number, number, number, number];
      }).filter(r => r[2] > 0 && r[3] > 0);
      // An open menu takes the next click anywhere, so a click outside closes it.
      const through = cap.mode === "audio" && !menu;
      const key = JSON.stringify([through, rects]);
      if (key === sent.current) return;
      sent.current = key;
      void ipc.captureControls(through, rects).catch(() => {});
    }, 50);
    return () => clearTimeout(timer);
  });

  if (!cap) return null;
  const set = (key: string, mic?: MicChoice) => {
    update(capSet(cap, key, mic));
    if (!["sys", "clicks", "remember"].includes(key.split(":")[0])) { const b = menu?.anchor; setMenu(null); b?.focus(); }
  };
  const ready = cap.mode !== "window" || !!cap.sel;
  // Where the countdown sits: on the chosen window or area, else the middle of the screen.
  const target = cap.mode === "window" ? cap.sel?.display ?? null : cap.mode === "area" ? cap.area.display : cap.mode === "display" ? cap.screen?.id ?? null : null;
  const r = cap.mode === "area" ? cap.area : cap.mode === "window" && cap.sel ? { x: cap.sel.x, y: cap.sel.y, w: cap.sel.width, h: cap.sel.height } : null;
  const aim = {
    name: cap.mode === "area" ? `the area (${sizeOf(cap.area.w, cap.area.h, scale)})` : cap.mode === "window" ? cap.sel?.title || cap.sel?.app || "" : cap.mode === "display" ? cap.screen?.name ?? name : "the call's sound",
    x: Math.max(144, Math.min(innerWidth - 144, r ? r.x + r.w / 2 : innerWidth / 2)),
    y: Math.max(130, Math.min(innerHeight - 130, r ? r.y + r.h / 2 : innerHeight / 2 - 40)),
  };
  const ownHover = cap.mode === "window" && !!hover?.own;
  return <Picker cap={cap} display={display} mine={mine} windows={windows} hover={hover} scale={scale}
    highlight={cap.mode === "display" && mine(cap.screen?.id ?? null)} area={drawn}
    onHover={setHover}
    onArea={(a, done) => { if (done) { setDrawn(null); update({ ...cap, area: a }); } else setDrawn(a); }}
    onChoose={w => update({ ...cap, sel: w })}
    onDisplay={() => update({ ...cap, screen: here() })}
    closing={closing}
    onEnter={() => { if (cap.mode === "display" && !cap.count && cap.screen?.id !== display) update({ ...cap, screen: here() }); }}>
    {main && <>
      <CapHint cap={cap} own={ownHover} screenName={name} meter={previewing}/>
      <SetupBar cap={cap} live={live} ready={ready} what={what} report={report} menu={menu?.which ?? null} meter={previewing}
        onMode={(m: Mode) => { setHover(null); update(capMode(cap, m)); }}
        onMenu={(which, anchor) => setMenu(menu?.which === which ? null : { which, anchor })}
        onGo={() => go()} onClose={() => void ipc.closeCapture()}/>
      {menu && <CaptureMenu which={menu.which} cap={cap} report={report} anchor={menu.anchor} preset={preset} onSet={set} onClose={() => setMenu(null)}/>}
    </>}
    {!main && ownHover && <CapHint cap={cap} own screenName={name}/>}
    {cap.count && mine(target) && <Countdown left={countLeft(cap.count, Date.now())} total={cap.count.total} name={aim.name} noMic={cap.mic.kind === "Off"} x={aim.x} y={aim.y} onNow={startNow} onMore={() => {
      const c = capRef.current; if (!c?.count) return;
      const left = countLeft(c.count, Date.now()) + 5;
      update({ ...c, count: { ...c.count, until: c.count.until + 5000, total: Math.max(c.count.total, left) } });
    }} onBack={back}/>}
  </Picker>;
}

export default function Capture() {
  return <Suspense><Setup/></Suspense>;
}
