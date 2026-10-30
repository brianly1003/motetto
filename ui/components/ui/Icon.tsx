// Atoms: small icons drawn on lucide's 24-unit grid with a 2-unit round stroke, the set shadcn/ui
// uses (owner 2026-10-30: a text "▾" sat small and off-centre, the "⠿" grip off its square). An SVG
// centres by its box, not by a font's ascent.
const base = { viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", strokeWidth: 2, strokeLinecap: "round", strokeLinejoin: "round", "aria-hidden": true } as const;

/** The dropdown chevron, on every button that opens a menu. */
export function ChevronDown() {
  return <svg className="icon chev" {...base}><path d="m6 9 6 6 6-6"/></svg>;
}

/** The drag handle's six dots. */
export function GripVertical() {
  return <svg className="icon" {...base}><circle cx="9" cy="5" r="1"/><circle cx="9" cy="12" r="1"/><circle cx="9" cy="19" r="1"/><circle cx="15" cy="5" r="1"/><circle cx="15" cy="12" r="1"/><circle cx="15" cy="19" r="1"/></svg>;
}
