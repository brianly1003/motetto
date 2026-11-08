// The screen setup's icons: the mockup's paths, 16-point grid, 1.4 stroke.
import type { ReactNode } from "react";

const svg = (d: ReactNode, width = 1.4) => <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth={width} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{d}</svg>;
export const X_IC = svg(<path d="m4 4 8 8m0-8-8 8"/>, 1.6);
export const CHECK_IC = svg(<path d="m3.5 8.4 3 3 6-6.8"/>, 1.7);
export const MIC_IC = svg(<><rect x="6" y="1.8" width="4" height="8" rx="2"/><path d="M3.6 7.6a4.4 4.4 0 0 0 8.8 0M8 12v2.2"/></>);
export const MICOFF_IC = svg(<><rect x="6" y="1.8" width="4" height="8" rx="2"/><path d="M3.6 7.6a4.4 4.4 0 0 0 8.8 0M8 12v2.2M2.4 2.4l11.2 11.2"/></>);
export const SCR_IC = svg(<><rect x="1.8" y="2.6" width="12.4" height="8.6" rx="1.4"/><path d="M5.5 13.8h5M8 11.2v2.6"/></>);
export const WIN_IC = svg(<><rect x="2.2" y="3" width="11.6" height="10" rx="1.4"/><path d="M2.2 5.8h11.6M4 4.4h.1M5.6 4.4h.1"/></>);
export const AREA_IC = svg(<><path d="M2.4 5V2.4H5M11 2.4h2.6V5M13.6 11v2.6H11M5 13.6H2.4V11"/><rect x="5.4" y="5.4" width="5.2" height="5.2" rx=".8" strokeDasharray="1.5 1.3"/></>);
export const WAVE_IC = svg(<path d="M2 8h1.4M4.6 5.4v5.2M7 3v10M9.4 5.8v4.4M11.8 4.4v7.2M14 8h-.2"/>);
export const SPK_IC = svg(<><path d="M2.5 6h2.6L8.5 3.2v9.6L5.1 10H2.5z"/><path d="M10.8 5.6a3.4 3.4 0 0 1 0 4.8M12.6 3.8a6 6 0 0 1 0 8.4"/></>);
export const NOTES_IC = svg(<><path d="M4 2.2h5.6L12.5 5v8.8H4z"/><path d="M9.4 2.2V5h3M6 8h4.4M6 10.4h4.4"/></>);
export const FILM_IC = svg(<><rect x="2" y="3" width="12" height="10" rx="1.5"/><path d="M5 3v10M11 3v10M2 6h3M2 10h3M11 6h3M11 10h3"/></>);
export const CLICK_IC = svg(<><path d="M4 3.5l7.4 3.2-3.2 1-1 3.2z"/><path d="M8.4 8.4l3.6 3.6"/></>);
export const LOCK_IC = svg(<><rect x="3" y="7" width="10" height="7" rx="1.5"/><path d="M5.5 7V5a2.5 2.5 0 0 1 5 0v2"/></>);
export const CHEV_IC = <svg className="chv" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="m6 9 6 6 6-6"/></svg>;

/** A tiny dial: a bolt for Now, otherwise a slice of a 10 s clock face. */
export function Dial({ n }: { n: number }) {
  if (n >= 10) return <svg className="dial" viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="8" r="6.2" fill="currentColor"/></svg>;
  if (!n) return <svg className="dial" viewBox="0 0 16 16" aria-hidden="true"><path d="M9.2 1.6 3.6 9h4l-.8 5.4L12.4 7h-4z" fill="currentColor"/></svg>;
  const x = (8 + 6.2 * Math.sin(2 * Math.PI * n / 10)).toFixed(2), y = (8 - 6.2 * Math.cos(2 * Math.PI * n / 10)).toFixed(2);
  return <svg className="dial" viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="8" r="6.2" fill="none" stroke="currentColor" strokeWidth="1.4" opacity=".55"/><path d={`M8 8V1.8A6.2 6.2 0 ${n > 5 ? 1 : 0} 1 ${x} ${y}Z`} fill="currentColor"/></svg>;
}

/** "⌥" on a Mac, "Alt+" elsewhere. */
export const K = (mac: string, win: string) => typeof navigator !== "undefined" && /Mac/.test(navigator.platform) ? mac : win;
