import { useEffect, useState } from "react";
import { strings as s } from "@/lib/strings";

// The side column's width (mockup .aside-resize): a handle between the page and the column,
// dragged or moved with the arrow keys, 220px (its width before) to 520px; double-click goes back
// to 220. The width is a per-viewer convenience kept in localStorage.
export const ASIDE_MIN = 220, ASIDE_MAX = 520;
const KEY = "motetto.asideW";
const clamp = (w: number) => Math.round(Math.min(ASIDE_MAX, Math.max(ASIDE_MIN, w)));

export function useAsideWidth() {
  const [width, setWidth] = useState(ASIDE_MIN);
  useEffect(() => { try { const saved = Number(localStorage.getItem(KEY)); if (saved) setWidth(clamp(saved)); } catch {} }, []);
  const set = (w: number, keep = false) => { const next = clamp(w); setWidth(next); if (keep) try { localStorage.setItem(KEY, String(next)); } catch {} };
  return [width, set] as const;
}

export default function AsideResize({ width, onWidth }: { width: number; onWidth: (width: number, keep?: boolean) => void }) {
  const [dragging, setDragging] = useState(false);
  return <div className="aside-resize" role="separator" tabIndex={0} aria-orientation="vertical" aria-label={s.asideWidth} aria-valuemin={ASIDE_MIN} aria-valuemax={ASIDE_MAX} aria-valuenow={width} title={s.asideWidthHint}
    data-dragging={dragging ? "" : undefined}
    onPointerDown={event => {
      if (event.button !== 0) return;
      event.preventDefault();
      const handle = event.currentTarget, x0 = event.clientX, w0 = width;
      handle.setPointerCapture(event.pointerId); setDragging(true);
      let last = w0;
      const move = (e: PointerEvent) => { last = w0 + x0 - e.clientX; onWidth(last); };
      const up = () => { setDragging(false); handle.removeEventListener("pointermove", move); onWidth(last, true); };
      handle.addEventListener("pointermove", move);
      handle.addEventListener("pointerup", up, { once: true });
      handle.addEventListener("pointercancel", up, { once: true });
    }}
    onDoubleClick={() => onWidth(ASIDE_MIN, true)}
    onKeyDown={event => {
      const step = ({ ArrowLeft: 16, ArrowRight: -16 } as Record<string, number>)[event.key];
      const to = ({ Home: ASIDE_MIN, End: ASIDE_MAX } as Record<string, number>)[event.key];
      if (step === undefined && to === undefined) return;
      event.preventDefault(); onWidth(to ?? width + step, true);
    }}><i/></div>;
}
