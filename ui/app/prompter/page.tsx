"use client";
// The script prompter strip (SPEC 14.3, DESIGN 4.18, mockup Script screen). Rust owns the window:
// it lets the mouse through except over the rectangles this page reports, and runs the share check.
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import * as ipc from "@/lib/ipc";
import { useSession } from "@/lib/useSession";
import { strings as s } from "@/lib/strings";
import { follow, keyLabel, pace, parseScript, tokens, type At } from "@/lib/script";
import { scriptWriter } from "@/lib/scriptWriter";
import { readingPages, readingRows, readingWindow, type ReadingPage } from "@/lib/prompter";
import type { Heard } from "@/lib/bindings/Heard";
import type { Rehearsal } from "@/lib/bindings/Rehearsal";
import "../../styles/prompter.css";
import { ChevronDown, GripVertical } from "@/components/ui/Icon";

type Check = "checking" | "hidden" | "visible" | "unknown" | "shown" | "off" | "moved";
const fmt = (sec: number) => `${String(Math.floor(sec / 60)).padStart(2, "0")}:${String(sec % 60).padStart(2, "0")}`;

export default function Prompter() {
  const { state } = useSession();
  const [name, setName] = useState<string | null>(null);
  const [text, setText] = useState("");
  const writer = useRef<ReturnType<typeof scriptWriter> | null>(null);
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
  // The chip's script menu: the names while it is open.
  const [menu, setMenu] = useState<string[] | null>(null);
  // The pointer moving onto the lines (not a control) closes it, as a click there goes to your app.
  useEffect(() => { if (through) setMenu(null); }, [through]);
  const [keys, setKeys] = useState<Record<string, string>>({});
  const [started] = useState(() => Date.now());
  const [now, setNow] = useState(() => Date.now());
  const root = useRef<HTMLDivElement>(null);
  const editor = useRef<HTMLTextAreaElement>(null);
  const current = useRef<HTMLDivElement>(null);
  const pages = useRef<ReadingPage[]>([]);
  // The first word of the rows shown, per paragraph: the text only moves when it has to.
  const shownFrom = useRef<{ line: number; word: number } | null>(null);
  const [page, setPage] = useState({ index: 0, total: 1, last: true });
  const slideFrom = useRef<{ line: number; offset: number } | null>(null);
  const shownWords = useRef<{ line: number; words: Set<number> } | null>(null);
  const nextRef = useRef<HTMLDivElement>(null);
  function move(direction: number) {
    setAt(a => {
      const index = Math.max(0, pages.current.findLastIndex(p => p.word <= a.word));
      const next = pages.current[index + direction];
      // Stepping by hand shows that passage exactly, from its first row.
      if (next) { shownFrom.current = { line: a.line, word: next.word }; return { ...a, word: next.word }; }
      const line = Math.max(0, Math.min(script.lines.length - 1, a.line + direction));
      return line === a.line ? a : { line, word: direction > 0 ? 0 : script.lines[line].words.length };
    });
  }
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
    writer.current = scriptWriter(chosen, text, (Array.isArray(list) ? list : []).find(info => info.name === chosen)?.modified_ms ?? null, ipc.scriptSave);
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
    if (key === "Down" || key === "Up") move(key === "Down" ? 1 : -1);
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
    if (!name || loading.current) return;
    const write = writer.current;
    const save = setTimeout(() => {
      void write?.(text).then(() => { if (writer.current === write) setNote(""); }).catch(e => setNote(String(e)));
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
    const watch = new ResizeObserver(send); watch.observe(el.querySelector(".pt") ?? el);
    return () => watch.disconnect();
  });

  async function close() {
    setClosing(true); setCloseError("");
    try {
      await writer.current?.(text);
      if (rehearsing) await ipc.rehearseStop();
      await ipc.prompterHide();
    } catch (error) { setCloseError(String(error)); }
    finally { setClosing(false); }
  }

  const leaking = !shown && !peek && (check === "visible" || check === "unknown");
  const checking = !shown && check === "checking";
  // The editor shows the whole script: only when the share can't see it.
  const editable = editing && !leaking && !checking;
  useLayoutEffect(() => {
    const viewport = current.current, text = viewport?.firstElementChild as HTMLElement | null;
    const strip = root.current?.querySelector<HTMLElement>(".pt");
    if (!viewport || !text || !strip) return;
    let active = true;
    const measure = () => {
      if (!active) return;
      const style = getComputedStyle(strip), line = parseFloat(getComputedStyle(text).lineHeight);
      const fixed = parseFloat(style.paddingTop) + parseFloat(style.paddingBottom) + 2
        + strip.querySelector(".pth")!.getBoundingClientRect().height
        + strip.querySelector(".ptfoot")!.getBoundingClientRect().height + 20;
      // Keep the reading controls visible first, then fit the quieter context previews.
      let nextLine = size * .82 * 1.3 + 4;
      const noAhead = innerHeight - fixed - nextLine < line;
      strip.classList.toggle("noahead", noAhead);
      if (noAhead) nextLine = 0;
      const prevLine = rehearsing ? 0 : size * .68 * 1.3 + 4;
      const tight = rehearsing || innerHeight - fixed - nextLine - prevLine < line;
      strip.classList.toggle("tight", tight);
      const rows = Math.max(1, Math.min(3, Math.floor((innerHeight - fixed - nextLine - (tight ? 0 : prevLine)) / line)));
      pages.current = readingPages(text, rows);
      const kept = shownFrom.current?.line === at.line ? shownFrom.current.word : undefined;
      const view = readingWindow(readingRows(text), rows, at.word, line, kept);
      shownFrom.current = { line: at.line, word: view.start };
      const calm = matchMedia("(prefers-reduced-motion: reduce)").matches;
      // Words coming into the rows you read rise in one after another; a paragraph's first
      // showing does not (it slides in whole, below).
      const was = shownWords.current, showing = new Set<number>(), fresh: HTMLElement[] = [], later: string[] = [];
      text.querySelectorAll<HTMLElement>("[data-word]").forEach(span => {
        const word = Number(span.dataset.word), shown = word >= view.from && word < view.to;
        span.style.visibility = shown ? "" : "hidden";
        if (shown) { showing.add(word); if (was?.line === at.line && !was.words.has(word)) fresh.push(span); }
        if (word >= view.to) later.push(span.textContent ?? "");
      });
      shownWords.current = { line: at.line, words: showing };
      if (!calm) fresh.forEach((span, i) => span.animate(
        [{ opacity: 0, transform: "translateY(.3em)", filter: "blur(3px)" }, { opacity: 1, transform: "none", filter: "none" }],
        { duration: 420, delay: i * 18, easing: "cubic-bezier(.2, .7, .2, 1)", fill: "backwards" }));
      // The next line: the rest of this paragraph, or at its end the next paragraph's opening.
      const next = nextRef.current;
      if (next) {
        const words = later.length ? later.join(" ") : next.dataset.next ?? "";
        if (next.textContent !== words) {
          const had = !!next.textContent;
          next.textContent = words;
          if (had && !calm) next.animate([{ opacity: 0, transform: "translateY(4px)" }, { opacity: 1, transform: "none" }], { duration: 320, easing: "ease-out" });
        }
      }
      // A steady height: the next line never moves.
      viewport.style.height = `${rows * line + 4}px`;
      // A one-row roll slides so your eyes can follow it; anything else moves at once.
      const step = view.top - viewport.scrollTop;
      if (step > 0 && step < line * 1.5) viewport.scrollTo({ top: view.top, behavior: "smooth" });
      else viewport.scrollTop = view.top;
      // A new paragraph rises from the next line, where its opening showed, instead of jumping.
      const from = slideFrom.current;
      slideFrom.current = next && view.last ? { line: at.line, offset: next.getBoundingClientRect().top - viewport.getBoundingClientRect().top } : null;
      if (from?.line === at.line - 1 && view.top === 0 && from.offset > 0 && !calm)
        text.animate([{ transform: `translateY(${from.offset}px)`, opacity: .35 }, { transform: "none", opacity: 1 }], { duration: 460, easing: "cubic-bezier(.2, .7, .2, 1)" });
      const { page: index, total, last } = view;
      setPage(before => before.index === index && before.total === total && before.last === last ? before : { index, total, last });
    };
    measure();
    const watch = new ResizeObserver(measure); watch.observe(text);
    window.addEventListener("resize", measure);
    void document.fonts.ready.then(measure);
    return () => { active = false; watch.disconnect(); window.removeEventListener("resize", measure); };
  }, [at.line, at.word, text, size, layout, editable, leaking, checking, rehearsing, shown, through, marker]);
  const c = lines[at.line];
  const p = pace(script.sections, at.line, Math.round((now - started) / 1000));
  const followText = !recording ? s.ptFollowOff.replace("{down}", keyLabel(keys.Down)).replace("{up}", keyLabel(keys.Up)) : paused ? s.ptPaused.replace("{key}", keyLabel(keys.Backslash)) : slow ? s.ptFollowSlow : s.ptFollowing;
  // Outside the strip, so the strip's fade never fades the marker.
  const markerEl = marker && <div className="ptmarker" aria-hidden="true">{Array.from({ length: 16 }, (_, i) => <i key={i} className={(Math.floor(i / 4) + i % 4) % 2 ? "b" : ""} />)}</div>;

  if (layout === "Tucked") return <div ref={root}>{markerEl}<button type="button" className="pttuck" data-control aria-label={s.ptShowScript} title={s.ptShowScript} onClick={() => void ipc.prompterLayout("Strip")}><svg viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden="true"><path d="M2 3h10M2 7h10M2 11h6" /></svg></button></div>;
  const words = (line: NonNullable<typeof c>, live: boolean) => {
    let i = 0, key = 0;
    // Keep the complete paragraph laid out; its viewport advances without discarding words.
    return line.text.split(/(\[[^\]]*\])/).flatMap(part => part.startsWith("[") ? [<span key={`c${key++}`} data-word={i} className="ptcue">{part.slice(1, -1)}</span>] : part.split(/(\s+|[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}])/u).filter(Boolean).map(w => {
      if (/^\s+$/.test(w)) return w;
      const k = i; i += tokens(w).length;
      return <span key={`w${key++}`} data-word={k} className={live ? i <= at.word ? "ptsaid" : k <= at.word && at.word < i ? "ptnow" : "" : ""}>{w}</span>;
    }));
  };
  return <div ref={root}>{markerEl}<div className={`pt${rehearsing ? " rehearsing" : ""}${layout === "Column" ? " col" : ""}${through && !marker && !checking ? " through" : ""}${shown ? " shown" : ""}${leaking ? " leaking" : ""}${editable ? " editing" : ""}`} style={{ "--pt-size": `${size}px` } as React.CSSProperties} role="region" aria-label={name ?? s.scripts}>
    <div className="pth">
      <button type="button" className="ptgrab" data-control data-tauri-drag-region title={s.ptDrag} aria-label={s.ptDrag}><GripVertical/></button>
      {c && !checking && name && (() => {
        // "Team X demo · 2/4 Demo ▾": the script, where you are, and the menu to switch.
        const where = script.sections.length === 1 && script.sections[0].minutes === 0 && !text.trimStart().startsWith("#") ? "" : ` · ${c.section + 1}/${script.sections.length} ${script.sections[c.section]?.title ?? ""}`;
        return rehearsing ? <span className="ptchip"><b>{name}</b>{where}</span>
          : <button type="button" className="ptchip" data-control aria-haspopup="menu" aria-expanded={menu !== null} title={s.ptSwitch} onClick={() => void (menu ? setMenu(null) : ipc.scriptsList().then(list => setMenu(Array.isArray(list) ? list.map(info => info.name) : [])).catch(() => setMenu([])))}><b>{name}</b>{where}<ChevronDown/></button>;
      })()}
      {menu && <div className="ptmenu" role="menu" data-control>
        {menu.map(n => <button key={n} type="button" role="menuitemradio" aria-checked={n === name} onClick={() => { setMenu(null); if (n !== name) void ipc.prompterScript(n).catch(e => setCloseError(String(e))); }}><i>{n === name ? "✓" : ""}</i>{n}</button>)}
        <div className="sep" />
        <button type="button" role="menuitem" className="more" onClick={() => { setMenu(null); void ipc.openScripts().catch(() => {}); }}><i />{s.ptOpenScripts}</button>
      </div>}
      {editable && <span className="ptedhint">{note || s.ptEditing}</span>}
      {rehearsing && <span className="ptreh" role="status"><i /><span title={s.rehearsing}>{s.rehearsing}</span><button type="button" data-control onClick={() => void ipc.rehearseStop().catch(() => {})}>{s.rehearsalStop}</button></span>}
      <span className={`ptpace${p.behind ? " behind" : ""}`} title={p.behind ? s.ptBehind : s.ptOnPlan}>{p.totalS > 0 && <i />}{fmt(Math.round((now - started) / 1000))}{p.totalS ? ` / ${fmt(p.totalS)}` : ""}</span>
      <button type="button" className={`ptvis${checking ? " checking" : ""}`} role="switch" aria-checked={shown} data-control title={s.ptShowInShare} onClick={() => { setCloseError(""); void ipc.setShareVisible(!shown).catch(error => setCloseError(String(error))); }}>
        <span className="h"><svg viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.3" aria-hidden="true"><path d="M1 6s1.8-3.5 5-3.5S11 6 11 6s-1.8 3.5-5 3.5S1 6 1 6Z" /><path d="M2 10 10 2" /></svg>{checking ? s.ptChecking : s.ptHidden}</span>
        <span className="s"><svg viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.3" aria-hidden="true"><path d="M1 6s1.8-3.5 5-3.5S11 6 11 6s-1.8 3.5-5 3.5S1 6 1 6Z" /><circle cx="6" cy="6" r="1.5" /></svg>{s.ptShown}</span>
      </button>
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
            {/* The end of the paragraph before: what you just said, for a glance back. */}
            <div className="ptln prev" style={{ visibility: page.index > 0 || !lines[at.line - 1] ? "hidden" : undefined }}><span dir="ltr">{lines[at.line - 1] ? words(lines[at.line - 1], false) : " "}</span></div>
            <div className="ptln cur" ref={current}><div className="pttext">{words(c, true)}</div></div>
            {/* Filled as you read (measure above): what comes next, lighter and fading out. */}
            <div className="ptln ptahead" ref={nextRef} aria-hidden="true" data-next={lines[at.line + 1]?.text.replace(/\[([^\]]*)\]/g, "$1") ?? ""} />
          </>}
        </div>
        <div className="ptfoot"><div className="ptnav" role="group" aria-label={s.ptReadControls}>
          <button type="button" data-control aria-label={s.ptPrevious} title={`${s.ptPrevious} · ${keyLabel(keys.Up)}`} disabled={!at.line && !page.index} onClick={() => move(-1)}><svg viewBox="0 0 12 12" aria-hidden="true"><path d="m8 2-4 4 4 4" /></svg></button>
          <span className="ptpage" title={s.ptParagraph.replace("{n}", String(at.line + 1))} aria-label={`${s.ptParagraph.replace("{n}", String(at.line + 1))} · ${page.index + 1}/${page.total}`}>{page.index + 1}/{page.total}</span>
          <button type="button" data-control aria-label={s.ptNext} title={`${page.index < page.total - 1 ? s.ptContinue : s.ptNext} · ${keyLabel(keys.Down)}`} disabled={at.line >= lines.length - 1 && page.index >= page.total - 1} onClick={() => move(1)}><svg viewBox="0 0 12 12" aria-hidden="true"><path d="m4 2 4 4-4 4" /></svg></button>
        </div><div className="ptprog"><i style={{ width: `${lines.length ? (at.line + (page.index + 1) / page.total) / lines.length * 100 : 0}%` }} /></div><span className={`ptfollow${paused || slow || !recording ? " wait" : ""}`} data-waiting={waiting || undefined}>{followText}</span></div>
      </>}
  </div></div>;
}
