"use client";
// Pointing at what to record (mockup renderCapHover / renderCapArea and the capture pointer
// handlers): the window under the pointer outlined and chosen with a click, the display under the
// pointer, or an area drawn, moved and resized by its 8 handles. One overlay per display.
// A click only chooses: nothing records until Record or Return.
import { useRef, type ReactNode, type RefObject } from "react";
import { dragArea, sizeOf, windowAt, type Area, type Cap, type Drag } from "@/lib/capture";
import type { CaptureWindow } from "@/lib/bindings/CaptureWindow";
import { CHECK_IC, WIN_IC, X_IC } from "./icons";

const HANDLES = ["nw", "n", "ne", "e", "se", "s", "sw", "w"];
const box = (r: { x: number; y: number; w: number; h: number }) => ({ left: r.x, top: r.y, width: r.w, height: r.h });
const rect = (w: CaptureWindow) => ({ x: w.x, y: w.y, w: w.width, h: w.height });

export function Picker({ cap, display, mine, windows, hover, scale, highlight, area, onHover, onArea, onChoose, onDisplay, onEnter, closing, children }: {
  cap: Cap; display: number; mine: (d: number | null) => boolean; windows: CaptureWindow[]; hover: CaptureWindow | null; scale: number;
  /** Entire screen: this display is the one under the pointer. */
  highlight: boolean;
  /** The area while it is drawn here (shared once the drag ends). */
  area: Area | null;
  onHover: (w: CaptureWindow | null) => void; onArea: (a: Area, done: boolean) => void; onChoose: (w: CaptureWindow) => void;
  onDisplay: () => void; onEnter: () => void;
  /** Set by the page when this press closed a menu: it only closes it (mockup), choosing nothing. */
  closing: RefObject<boolean>; children: ReactNode;
}) {
  const drag = useRef<Drag | null>(null), areaEl = useRef<HTMLDivElement>(null);
  const counting = !!cap.count;
  const outside = (t: EventTarget) => !(t as Element).closest(".capbar, .capmenu, .capcd");
  const bounds = () => ({ w: innerWidth, h: innerHeight });
  const selHere = cap.sel && cap.sel.display === display ? cap.sel : null;
  const h = cap.mode === "window" ? selHere ?? hover : null;
  const other = cap.mode === "window" && selHere && hover && hover.id !== selHere.id ? hover : null;
  const a = area ?? (mine(cap.area.display) ? cap.area : null);
  const size = a && sizeOf(a.w, a.h, scale, " × ");
  return <div className="capture" id="capture" role="dialog" aria-label="Choose what to record" data-mode={cap.mode} data-away={cap.mode === "display" && !highlight ? "" : undefined} data-counting={counting ? "" : undefined}
    onPointerEnter={onEnter}
    onPointerMove={e => {
      if (counting) return;
      if (cap.mode === "window" && outside(e.target)) {
        const w = windowAt(windows, display, e.clientX, e.clientY);
        if (w?.id !== hover?.id) onHover(w);
      }
      if (drag.current) onArea(dragArea(drag.current, e.clientX, e.clientY, bounds(), display), false);
    }}
    onPointerDown={e => {
      if (closing.current || counting || e.button !== 0 || !outside(e.target)) return;
      if (cap.mode !== "area") return;
      const handle = (e.target as HTMLElement).closest<HTMLElement>(".h")?.dataset.h;
      const from = a ?? { ...cap.area, display };
      drag.current = { h: handle || ((e.target as Element).closest("#caparea") ? "move" : "new"), x: e.clientX, y: e.clientY, from };
      if (drag.current.h === "new") onArea({ display, x: e.clientX, y: e.clientY, w: 0, h: 0 }, false);
      areaEl.current?.classList.add("drag");
      e.currentTarget.setPointerCapture(e.pointerId); e.preventDefault();
    }}
    onPointerUp={() => {
      const d = drag.current;
      if (!d) return;
      drag.current = null; areaEl.current?.classList.remove("drag");
      // A click without a drag keeps the old area; a tiny one grows to something usable.
      const now = area ?? d.from;
      // A click on another display leaves the area where it was.
      onArea(d.h === "new" && (now.w < 40 || now.h < 30) ? (mine(cap.area.display) ? d.from : cap.area) : now, true);
    }}
    onClick={e => {
      if (closing.current) { closing.current = false; return; }
      if (counting || !outside(e.target)) return;
      if (cap.mode === "display") onDisplay();
      if (cap.mode === "window") {
        const w = windowAt(windows, display, e.clientX, e.clientY);
        if (w && !w.own) onChoose(w);
      }
    }}>
    <div className="capveil"/>
    <div className={`caphl${h?.own ? " own" : ""}${h && h === selHere ? " chosen" : ""}`} id="caphl" style={h ? { display: "block", ...box(rect(h)) } : { display: "none" }}>
      <div className="capbadge" id="capbadge">{h && (h.own
        ? <><span className="ico">{X_IC}</span><b>Motetto</b><span>Its own windows stay out of every video.</span></>
        : h === selHere ? <><span className="ico sel">{CHECK_IC}</span><b>{h.title || h.app}</b><span>{sizeOf(h.width, h.height, scale, " × ")} · chosen</span></>
        : <><span className="ico">{WIN_IC}</span><b>{h.title || h.app}</b><span>{sizeOf(h.width, h.height, scale, " × ")} · click to choose</span></>)}</div>
    </div>
    <div className={`caphov${other?.own ? " own" : ""}`} id="caphov" style={other ? { display: "block", ...box(rect(other)) } : { display: "none" }}/>
    {a && <div ref={areaEl} className="caparea" id="caparea" style={box(a)}>
      <div className="thirds"/><span className={`size${a.y < 44 ? " below" : ""}`} id="capsize">{size}</span>
      {HANDLES.map(k => <i key={k} className="h" data-h={k}/>)}
    </div>}
    {children}
  </div>;
}
