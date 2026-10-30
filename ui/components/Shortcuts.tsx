import { useEffect, useRef, useState } from "react";
import { strings as s } from "@/lib/strings";
import { KbdGroup } from "./ui/Kbd";

// The keyboard help for the Notes and Transcript tabs (owner 2026-10-30): it used to be a line under
// the content, so a long call put it thousands of lines down. A keycap button in the tab bar, or "?",
// opens it here instead; it lists only what works in the current state.
const mac = () => typeof navigator !== "undefined" && /Mac/.test(navigator.platform);
type Row = { label: string; keys: string[]; edit?: boolean };

function rows(tab: string, m: boolean): Row[] {
  const [cmd, alt, enter, shift] = m ? ["⌘", "⌥", "↩", "⇧"] : ["Ctrl", "Alt", "Enter", "Shift"];
  if (tab === "Transcript") return [
    { label: s.keysCorrect, keys: [s.keysClick], edit: true },
    { label: s.keysMove, keys: [m ? "↑" : "Up", m ? "↓" : "Down"] },
    { label: s.keysPlay, keys: ["Space"] },
    { label: s.keysEditLine, keys: [enter], edit: true },
    { label: s.keysPlayLine, keys: [alt, enter] },
    { label: s.keysUndo, keys: [cmd, "Z"], edit: true },
    { label: s.keysFind, keys: [cmd, "F"] },
  ];
  return [
    { label: s.keysEdit, keys: [s.keysClick], edit: true },
    { label: s.keysSave, keys: [enter], edit: true },
    { label: s.keysBreak, keys: [shift, enter], edit: true },
    { label: s.keysNewLine, keys: [cmd, enter], edit: true },
    { label: s.keysOwner, keys: ["@"], edit: true },
    { label: s.keysUndo, keys: [cmd, "Z"], edit: true },
    { label: s.keysFind, keys: [cmd, "F"] },
  ];
}

/** `note`: why some keys are missing (locked notes, a translation), shown under the list. */
export default function Shortcuts({ tab, readOnly, note }: { tab: string; readOnly: boolean; note: string }) {
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null), pop = useRef<HTMLDivElement>(null);
  const close = (focus: boolean) => { setOpen(false); if (focus) button.current?.focus(); };
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape" && open) { e.preventDefault(); e.stopPropagation(); close(true); return; }
      const t = e.target as HTMLElement | null;
      if (e.key !== "?" || e.metaKey || e.ctrlKey || e.altKey || t?.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t?.tagName ?? "")) return;
      e.preventDefault(); setOpen(o => !o);
    };
    const away = (e: PointerEvent) => { if (open && !pop.current?.contains(e.target as Node) && !button.current?.contains(e.target as Node)) close(false); };
    document.addEventListener("keydown", key, true); document.addEventListener("pointerdown", away);
    return () => { document.removeEventListener("keydown", key, true); document.removeEventListener("pointerdown", away); };
  }, [open]);
  const shown = rows(tab, mac()).filter(row => !readOnly || !row.edit);
  return <div className="keys">
    <button ref={button} type="button" className={`keys-button${open ? " on" : ""}`} aria-haspopup="dialog" aria-expanded={open} aria-label={s.keysTitle} title={`${s.keysTitle} (?)`} onClick={() => setOpen(!open)}>
      <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" aria-hidden="true"><rect x="1.5" y="4" width="13" height="8.5" rx="1.8"/><path d="M4.3 6.7h.01M6.8 6.7h.01M9.2 6.7h.01M11.7 6.7h.01M5 9.6h6"/></svg>
    </button>
    {open && <div ref={pop} className="keys-pop" role="dialog" aria-label={s.keysTitle}>
      <div className="keys-head"><b>{s.keysTitle}</b><span>{tab === "Transcript" ? s.keysTranscript : s.keysNotes}</span></div>
      <dl className="keys-list">{shown.map(row => <div key={row.label}><dt>{row.label}</dt><dd><KbdGroup keys={row.keys}/></dd></div>)}</dl>
      <p className="keys-foot">{note}</p>
    </div>}
  </div>;
}
