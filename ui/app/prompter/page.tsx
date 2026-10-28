"use client";
// The script prompter strip (SPEC 14.3, DESIGN 4.18, mockup Script screen). Rust owns the window:
// it lets the mouse through except over the rectangles this page reports, and runs the share check.
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import * as ipc from "@/lib/ipc";
import { useSession } from "@/lib/useSession";
import { strings as s } from "@/lib/strings";
import { follow, pace, parseScript, type At } from "@/lib/script";
import type { Heard } from "@/lib/bindings/Heard";
import type { Rehearsal } from "@/lib/bindings/Rehearsal";
import "../../styles/prompter.css";

type Check = "checking" | "hidden" | "visible" | "unknown" | "shown" | "off" | "moved";
const fmt = (sec: number) => `${String(Math.floor(sec / 60)).padStart(2, "0")}:${String(sec % 60).padStart(2, "0")}`;
const keyLabel = (accel: string | undefined) => !accel ? "" : /Mac/.test(navigator.platform) ? accel.replace("Ctrl+", "⌃").replace("Alt+", "⌥").replace("Shift+", "⇧").replace("Down", "↓").replace("Up", "↑").replace("Backslash", "\\") : accel;

export default function Prompter() {
  const { state } = useSession();
  const [name, setName] = useState<string | null>(null);
  const [text, setText] = useState("");
  const base = useRef<number | null>(null);
  const [at, setAt] = useState<At>({ line: 0, word: 0 });
  const [waiting, setWaiting] = useState(false);
  const [check, setCheck] = useState<Check>("checking");
  const [through, setThrough] = useState(false);
  const [editing, setEditing] = useState(false);
  const [layout, setLayout] = useState<ipc.PrompterLayout>("Strip");
  const [size, setSize] = useState(22);
  const [paused, setPaused] = useState(false);
  const [slow, setSlow] = useState(false);
  const [peek, setPeek] = useState(false);
  const [how, setHow] = useState(false);
  const [oneDisplay, setOneDisplay] = useState(false);
  const [marker, setMarker] = useState(false);
  const [note, setNote] = useState("");
  const [closeError, setCloseError] = useState("");
  const [closing, setClosing] = useState(false);
  const [keys, setKeys] = useState<Record<string, string>>({});
  const [started] = useState(() => Date.now());
  const [now, setNow] = useState(() => Date.now());
  const root = useRef<HTMLDivElement>(null);
  const editor = useRef<HTMLTextAreaElement>(null);
  const script = useMemo(() => parseScript(text), [text]);
  const shown = !!state?.share_visible;
  const [rehearsing, setRehearsing] = useState(false);
  const [lastHeard, setLastHeard] = useState(0);
  // "Following" only while something follows: a rehearsal, or a recording with live captions.
  const recording = rehearsing || (state?.state.type === "Recording" && now - lastHeard < 10_000);

  const loading = useRef(false);
  async function load(next: string | null) {
    const settings = await ipc.getSettings().catch(() => null);
    if (settings) { setLayout(settings.prompter.layout); setSize(settings.prompter.text_size); setKeys(settings.shortcuts); }
    const chosen = next ?? settings?.prompter.last_script ?? null;
    if (!chosen) { setName(null); return; }
    // Name, text and base change together, so a pending save never writes one script's text
    // under another's name.
    loading.current = true;
    const list = await ipc.scriptsList().catch(() => []);
    const text = await ipc.scriptRead(chosen).catch(() => "");
    base.current = (Array.isArray(list) ? list : []).find(info => info.name === chosen)?.modified_ms ?? null;
    setName(chosen); setText(text); setAt({ line: 0, word: 0 });
    loading.current = false;
  }
  useEffect(() => {
    // What Rust said before this page listened: the script, the check, editing, the layout.
    void ipc.prompterState().then(page => {
      setCheck(page.check as Check); setEditing(page.editing); setLayout(page.layout); setRehearsing(page.rehearsing);
      void load(page.script);
    }).catch(() => void load(null));
    const offs = [
      ipc.listen<string | null>("motetto://prompter-script", next => void load(next)),
      ipc.listen<Check>("motetto://prompter-check", setCheck),
      ipc.listen<boolean>("motetto://prompter-through", setThrough),
      ipc.listen<boolean>("motetto://prompter-editing", setEditing),
      ipc.listen<ipc.PrompterLayout>("motetto://prompter-layout", setLayout),
      ipc.listen<boolean>("motetto://prompter-marker", on => {
        setMarker(on);
        // Two frames, so the marker is on screen before Rust captures.
        if (on) requestAnimationFrame(() => requestAnimationFrame(() => void ipc.prompterMarkerDrawn()));
      }),
      ipc.listen<string>("motetto://prompter-key", key => setKey.current(key)),
      ipc.listen<Rehearsal>("motetto://rehearsal", r => setRehearsing(r.duration_ms === null)),
    ];
    const tick = setInterval(() => setNow(Date.now()), 1000);
    return () => { clearInterval(tick); offs.forEach(off => void off.then(fn => fn())); };
  }, []);
  const lines = script.lines;
  const setKey = useRef<(key: string) => void>(() => {});
  setKey.current = key => {
    if (key === "Down" || key === "Up") setAt(a => ({ line: Math.max(0, Math.min(lines.length - 1, a.line + (key === "Down" ? 1 : -1))), word: 0 }));
    if (key === "ShiftDown") setAt(a => { const next = script.sections.find(x => x.first > a.line); return next ? { line: next.first, word: 0 } : a; });
    if (key === "Backslash") setPaused(p => !p);
    if (key === "Equal" || key === "Minus") setSize(v => {
      const next = Math.max(14, Math.min(40, v + (key === "Equal" ? 2 : -2)));
      void ipc.getSettings().then(saved => ipc.saveSettings({ ...saved, prompter: { ...saved.prompter, text_size: next } })).catch(() => {});
      return next;
    });
  };
  // Following your voice: Rust sends what it heard in the last 4 s of the mic (follow.rs),
  // with the next script words as its hint.
  const [heard, setHeard] = useState<Heard | null>(null);
  useEffect(() => {
    const off = ipc.listen<Heard>("motetto://follow-heard", setHeard);
    return () => { void off.then(fn => fn()); };
  }, []);
  useEffect(() => {
    if (heard) setLastHeard(Date.now());
    if (!heard || paused || editing || !lines.length) return;
    setSlow(heard.inference_ms > 2000);
    setAt(a => { const next = follow(lines, a, heard.text); setWaiting(next.waiting); return { line: next.line, word: next.word }; });
  }, [heard]);
  useEffect(() => {
    // Spoken words only (no cues), from where you are.
    const ahead = lines.slice(at.line, at.line + 3).flatMap(l => l.words).slice(at.word).join(" ");
    void ipc.followHint(ahead).catch(() => {});
  }, [at.line, at.word, lines]);
  // Saved 300 ms after the last keystroke; a conflict is said in the bar, not lost.
  useEffect(() => {
    if (!editing || !name || loading.current) return;
    const save = setTimeout(() => {
      ipc.scriptSave(name, text, base.current).then(time => { base.current = time; setNote(""); }).catch(e => setNote(String(e)));
    }, 300);
    return () => clearTimeout(save);
  }, [text, editing, name]);
  useEffect(() => {
    if (!editing) return;
    const box = editor.current; if (!box) return;
    const where = text.indexOf(lines[at.line]?.text ?? "");
    box.focus();
    if (where >= 0) box.setSelectionRange(where, where);
    // A click in another app ends editing there; that app keeps the keyboard.
    const focus = ipc.onCompanionFocus(focused => { if (!focused) void ipc.prompterEdit(false, false); });
    return () => { void focus.then(off => off()); };
  }, [editing]);
  useEffect(() => { if (!peek) return; const t = setTimeout(() => setPeek(false), 15000); return () => clearTimeout(t); }, [peek]);
  // The rectangles that take the mouse; everything else lets clicks through to your demo.
  useLayoutEffect(() => {
    const el = root.current; if (!el) return;
    const send = () => void ipc.prompterControls([...el.querySelectorAll<HTMLElement>("[data-control]")].map(c => { const r = c.getBoundingClientRect(); return [r.left, r.top, r.width, r.height] as [number, number, number, number]; })).catch(() => {});
    send();
    const watch = new ResizeObserver(send); watch.observe(el);
    return () => watch.disconnect();
  });

  async function close() {
    setClosing(true); setCloseError("");
    try {
      if (editing && name) base.current = await ipc.scriptSave(name, text, base.current);
      if (rehearsing) await ipc.rehearseStop();
      await ipc.prompterHide();
    } catch (error) { setCloseError(String(error)); }
    finally { setClosing(false); }
  }

  const leaking = !shown && !peek && (check === "visible" || check === "unknown");
  const checking = !shown && check === "checking";
  // The editor shows the whole script: only when the share can't see it.
  const editable = editing && !leaking && !checking;
  const c = lines[at.line];
  const p = pace(script.sections, at.line, Math.round((now - started) / 1000));
  const followText = !recording ? s.ptFollowOff.replace("{down}", keyLabel(keys.Down)).replace("{up}", keyLabel(keys.Up)) : paused ? s.ptPaused.replace("{key}", keyLabel(keys.Backslash)) : slow ? s.ptFollowSlow : s.ptFollowing;
  // Outside the strip, so the strip's fade never fades the marker.
  const markerEl = marker && <div className="ptmarker" aria-hidden="true">{Array.from({ length: 16 }, (_, i) => <i key={i} className={(Math.floor(i / 4) + i % 4) % 2 ? "b" : ""} />)}</div>;

  if (layout === "Tucked") return <div ref={root}>{markerEl}<button type="button" className="pttuck" data-control aria-label={s.ptShowScript} title={s.ptShowScript} onClick={() => void ipc.prompterLayout("Strip")}><svg viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden="true"><path d="M2 3h10M2 7h10M2 11h6" /></svg></button></div>;
  const words = (line: NonNullable<typeof c>, live: boolean) => {
    let i = 0;
    // A long current line shows from a few words before where you are, so you never read
    // past the strip's three lines.
    const from = live && line.words.length > 24 ? Math.max(0, at.word - 8) : 0;
    // Words and cues joined by plain spaces, so an underline never runs into the gap.
    return line.text.split(/(\[[^\]]*\])/).flatMap(part => part.startsWith("[") ? [<span key={`c${i}`} className="ptcue">{part.slice(1, -1)}</span>] : part.split(/\s+/).filter(Boolean).map(w => { const k = i++; return k < from ? null : <span key={k} className={live ? k < at.word ? "ptsaid" : k === at.word ? "ptnow" : "" : ""}>{w}</span>; })).filter(Boolean).flatMap((node, n) => n ? [" ", node] : from ? ["… ", node] : [node]);
  };
  return <div ref={root}>{markerEl}<div className={`pt${through && !marker && !checking ? " through" : ""}${shown ? " shown" : ""}${leaking ? " leaking" : ""}${editable ? " editing" : ""}`} style={{ "--pt-size": `${size}px` } as React.CSSProperties} role="region" aria-label={name ?? s.scripts}>
    <div className="pth">
      <button type="button" className="ptgrab" data-control data-tauri-drag-region title={s.ptDrag} aria-label={s.ptDrag}>⠿</button>
      {c && !checking && !(script.sections.length === 1 && script.sections[0].minutes === 0 && !text.trimStart().startsWith("#")) && <span className="ptchip">{`${c.section + 1}/${script.sections.length} · ${script.sections[c.section]?.title ?? ""}`}</span>}
      {editable && <span className="ptedhint">{note || s.ptEditing}</span>}
      <span className={`ptpace${p.behind ? " behind" : ""}`} title={p.behind ? s.ptBehind : s.ptOnPlan}>{p.totalS > 0 && <i />}{fmt(Math.round((now - started) / 1000))}{p.totalS ? ` / ${fmt(p.totalS)}` : ""}</span>
      {rehearsing && <span className="ptreh" role="status"><i /><span title={s.rehearsing}>{s.rehearsing}</span><button type="button" data-control onClick={() => void ipc.rehearseStop().catch(() => {})}>{s.rehearsalStop}</button></span>}
      {!rehearsing && <button type="button" className={`ptvis${checking ? " checking" : ""}`} role="switch" aria-checked={shown} data-control title={s.ptShowInShare} onClick={() => void ipc.setShareVisible(!shown)}>
        <span className="h"><svg viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.3" aria-hidden="true"><path d="M1 6s1.8-3.5 5-3.5S11 6 11 6s-1.8 3.5-5 3.5S1 6 1 6Z" /><path d="M2 10 10 2" /></svg>{checking ? s.ptChecking : s.ptHidden}</span>
        <span className="s"><svg viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.3" aria-hidden="true"><path d="M1 6s1.8-3.5 5-3.5S11 6 11 6s-1.8 3.5-5 3.5S1 6 1 6Z" /><circle cx="6" cy="6" r="1.5" /></svg>{s.ptShown}</span>
      </button>}
      <button type="button" className="ptedit" data-control title={s.ptEdit} aria-label={s.ptEdit} onClick={() => void ipc.prompterEdit(!editing, false)}>✎</button>
      <button type="button" className="ptclose" data-control title={s.ptClose} aria-label={s.ptClose} disabled={closing} onClick={() => void close()}><svg viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true"><path d="m3 3 6 6m0-6-6 6" /></svg></button>
    </div>
    {closeError && <div className="ptleak" role="alert">{closeError}</div>}
    {editable ? <textarea ref={editor} className="pted" aria-label={s.ptEdit} value={text} onChange={e => setText(e.target.value)} onKeyDown={e => { if (e.key === "Escape") { e.preventDefault(); void ipc.prompterEdit(false, true); } }} />
      : leaking ? <div className="ptleak" role="alert">
        <b>{check === "unknown" ? s.ptUnknown : s.ptLeak}</b> {check === "unknown" ? s.ptUnknownBody : s.ptLeakBody}
        <div className="acts">
          {!oneDisplay && <button type="button" className="primary" data-control onClick={() => void ipc.prompterMoveDisplay().then(moved => setOneDisplay(!moved))}>{s.ptMoveDisplay}</button>}
          <button type="button" data-control onClick={() => setHow(v => !v)} aria-expanded={how}>{s.ptShareWindow}</button>
          <button type="button" data-control onClick={() => setPeek(true)}>{s.ptPeek}</button>
          {check === "unknown" && <button type="button" data-control onClick={() => void ipc.openPermissionSettings("SystemAudio")}>{s.ptAllowCheck}</button>}
        </div>
        {how && <p>{s.ptShareWindowHow}</p>}
      </div>
      : <>
        <div className="ptlines" style={checking ? { visibility: "hidden" } : undefined}>
          {!c ? <div className="ptln cur">{name ? s.ptEmpty : s.ptNoScript}</div> : <>
            {lines[at.line - 1] && <div className="ptln prev">{words(lines[at.line - 1], false)}</div>}
            <div className="ptln cur">{words(c, true)}</div>
            {lines[at.line + 1] && <div className="ptln">{words(lines[at.line + 1], false)}</div>}
          </>}
        </div>
        <div className="ptfoot"><div className="ptprog"><i style={{ width: `${lines.length ? (at.line + 1) / lines.length * 100 : 0}%` }} /></div><span className={`ptfollow${paused || slow || !recording ? " wait" : ""}`} data-waiting={waiting || undefined}>{followText}</span></div>
      </>}
  </div></div>;
}
