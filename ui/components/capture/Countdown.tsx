"use client";
// The countdown card (mockup capCount): on what will be recorded, so it is seen once more before
// the first frame. Start now, +5 s or Back; the ring empties toward the start.
import { useEffect, useRef, useState } from "react";

const RING = 295.3;

export function Countdown({ left, total, name, noMic, x, y, onNow, onMore, onBack }: {
  left: number; total: number; name: string; noMic: boolean; x: number; y: number;
  onNow: () => void; onMore: () => void; onBack: () => void;
}) {
  const num = useRef<HTMLSpanElement>(null), go = useRef<HTMLButtonElement>(null), shown = useRef(left);
  // The ring starts full, then shows where it will be one second from now.
  const [armed, setArmed] = useState(false);
  useEffect(() => { go.current?.focus(); const raf = requestAnimationFrame(() => setArmed(true)); return () => cancelAnimationFrame(raf); }, []);
  useEffect(() => {
    if (shown.current === left || !num.current) return;
    shown.current = left;
    const n = num.current; n.classList.remove("tick"); void n.offsetWidth; n.classList.add("tick");
  }, [left]);
  return <div className="capcd" id="capcount" role="alertdialog" aria-label="Recording starts soon" style={{ left: x, top: y }}>
    <div className="ring"><svg viewBox="0 0 104 104" aria-hidden="true"><circle className="track" cx="52" cy="52" r="47"/><circle className="prog" cx="52" cy="52" r="47" style={{ strokeDashoffset: armed ? String(RING * (1 - (left - 1) / total)) : "0" }}/></svg><span ref={num} className="cdnum" aria-live="assertive">{left}</span></div>
    <div className="what">Recording <b>{name}</b>{noMic ? " · no mic" : ""}</div>
    <div className="acts"><button ref={go} type="button" className="go" data-count="now" onClick={onNow}>Start now</button><button type="button" data-count="more" onClick={onMore}>+5 s</button><button type="button" data-count="back" onClick={onBack}>Back</button></div>
    <small>This card is never in the video.</small>
  </div>;
}
