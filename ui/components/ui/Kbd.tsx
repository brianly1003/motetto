// Atom: a key, or keys pressed together, after shadcn/ui's Kbd (owner 2026-10-30). Every place the
// app shows a key uses this, so keys look the same in menus, tooltips, hints and the shortcuts list.
import type { ReactNode } from "react";

export function Kbd({ children }: { children: ReactNode }) {
  return <kbd className="kbd">{children}</kbd>;
}

/** "⇧⌘C" -> ⇧ ⌘ C; "Ctrl+\\" -> Ctrl \; "Enter" stays one key. An array is taken as given. */
export function splitKeys(keys: string): string[] {
  return keys.split("+").flatMap(part => part.match(/[⌘⌥⇧⌃]|[^⌘⌥⇧⌃]+/g) ?? []).filter(Boolean);
}

export function KbdGroup({ keys, label }: { keys: string | string[]; label?: string }) {
  const list = typeof keys === "string" ? splitKeys(keys) : keys;
  return <span className="kbd-group" aria-label={label}>{list.map((key, i) => <Kbd key={i}>{key}</Kbd>)}</span>;
}
