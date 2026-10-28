// 16 px line icons for the meeting menus and the row "•••" (20 px, three 2.5 px dots), as drawn
// in docs/app/motetto-app-mockup.html.
const paths = {
  screen: <><rect x="1.8" y="2.6" width="12.4" height="8.6" rx="1.4"/><path d="M5.5 13.8h5M8 11.2v2.6"/></>,
  window: <><rect x="2.2" y="3" width="11.6" height="10" rx="1.4"/><path d="M2.2 5.8h11.6M4 4.4h.1M5.6 4.4h.1"/></>,
  open: <path d="M9.5 2.5h4v4M13.5 2.5 8 8M12 9.5v3a1 1 0 0 1-1 1H3.5a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1h3"/>,
  rename: <><path d="M10.8 2.7l2.5 2.5L6 12.5l-3.2.7.7-3.2z"/><path d="M9.3 4.2l2.5 2.5"/></>,
  copy: <><rect x="5.5" y="5.5" width="8" height="8" rx="1.5"/><path d="M10.5 3.5v-.5a1 1 0 0 0-1-1H3.5a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h.5"/></>,
  folder: <path d="M2.5 4.5a1 1 0 0 1 1-1h3l1.5 1.5h4.5a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1h-9a1 1 0 0 1-1-1z"/>,
  again: <><path d="M13 8a5 5 0 1 1-1.5-3.6"/><path d="M13 2.5v3h-3"/></>,
  wave: <path d="M3 6.5v3M5.5 4.5v7M8 2.5v11M10.5 5v6M13 6.5v3"/>,
  items: <><rect x="2.8" y="2.8" width="10.4" height="10.4" rx="2.6"/><path d="m5.6 8.1 1.7 1.7 3.2-3.4"/></>,
  trash: <path d="M2.8 4.5h10.4M6.3 4.5V3h3.4v1.5M4.3 4.5l.7 8.5h6l.7-8.5"/>,
  script: <path d="M3 4h10M3 8h10M3 12h6"/>,
  lock: <><rect x="3" y="7" width="10" height="7" rx="1.5"/><path d="M5.5 7V5a2.5 2.5 0 0 1 5 0v2"/></>,
};
export type IconName = keyof typeof paths;

export function MenuIcon({ name }: { name: IconName }) {
  return <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{paths[name]}</svg>;
}

export function Dots() {
  return <svg viewBox="0 0 20 20" fill="currentColor" stroke="none" aria-hidden="true"><circle cx="10" cy="5.5" r="1.25"/><circle cx="10" cy="10" r="1.25"/><circle cx="10" cy="14.5" r="1.25"/></svg>;
}

// A setting in a menu (Lock notes): the label, then an on/off switch; the menu stays open.
export function MenuSwitch({ icon, label, checked, onChange }: { icon: IconName; label: string; checked: boolean; onChange: (on: boolean) => void }) {
  return <button type="button" role="menuitemcheckbox" aria-checked={checked} onClick={() => onChange(!checked)}><MenuIcon name={icon}/><span>{label}</span><i className="mswitch" aria-hidden="true"/></button>;
}

export function MenuItem({ icon, label, onClick, danger, disabled, title }: { icon: IconName; label: string; onClick: () => void; danger?: boolean; disabled?: boolean; title?: string }) {
  return <button type="button" role="menuitem" className={danger ? "danger" : undefined} disabled={disabled} title={title} onClick={onClick}><MenuIcon name={icon}/><span>{label}</span></button>;
}
