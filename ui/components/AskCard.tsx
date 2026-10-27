"use client";
import { useEffect, useRef, useState } from "react";
import {createPortal} from "react-dom";
import {ScreenMenu} from "./ScreenMenu";
import {MicIcon} from "./Microphone";
import {MenuIcon} from "./MenuIcon";
import {defaultPick} from "@/lib/screen";
import type {ScreenOption} from "@/lib/bindings/ScreenOption";
import type {ScreenPick} from "@/lib/bindings/ScreenPick";
import * as ipc from "@/lib/ipc";
import { strings as s } from "@/lib/strings";
import MotettoMark from "@/components/MotettoMark";

/** The Ask card (DESIGN 4.2): a detected call asks; nothing records until Record. Rust fades it to the Pill after 20 s. */
export function AskCard({ app, callId }: { app: string; callId: string }) {
  const [options,setOptions]=useState<ScreenOption[]>([]),[pick,setPick]=useState<ScreenPick|null>(null),[screenOn,setScreenOn]=useState(false),[offer,setOffer]=useState(true),[preset,setPreset]=useState('sharp');
  const [screenMenu,setScreenMenu]=useState<{left:number;top:number}|null>(null);
  useEffect(()=>{setScreenOn(false);setScreenMenu(null);let active=true;Promise.all([ipc.listScreenSources(),ipc.getSettings()]).then(([options,settings])=>{if(active){setOptions(options);setPick(defaultPick(options,settings.screen_source));setOffer(settings.screen_offer_on_ask);setPreset(settings.screen_preset);}}).catch(()=>{});return()=>{active=false;};},[callId]);
  useEffect(()=>{if(!screenMenu)return;void ipc.companionMenu(true,380).catch(()=>{});const close=(event:PointerEvent)=>{if(!(event.target as Element)?.closest('.scrmenu,[data-srcmenu]'))setScreenMenu(null);};document.addEventListener('pointerdown',close);return()=>{document.removeEventListener('pointerdown',close);void ipc.companionMenu(false).catch(()=>{});};},[screenMenu]);
  const [reminder,setReminder]=useState(true);
  useEffect(()=>{let active=true;ipc.getSettings().then(s=>{if(active)setReminder(s.recording_reminder);}).catch(()=>{});return()=>{active=false;};},[]);
  const [menu, setMenu] = useState(false);
  const [notice, setNotice] = useState("");
  // Leaving restarts the countdown line, like the backend's 20 s hold.
  const [lap, setLap] = useState(0);
  const split = useRef<HTMLDivElement>(null);
  // The open menu hangs below the card: Rust grows the window for it and restores 480x124 on close,
  // item chosen, Esc, click outside or unmount (the cleanup covers all of them).
  useEffect(() => {
    if (!menu) return;
    ipc.companionMenu(true).catch(() => {});
    const outside = (event: PointerEvent) => { if (!split.current?.contains(event.target as Node)) setMenu(false); };
    document.addEventListener("pointerdown", outside);
    return () => { document.removeEventListener("pointerdown", outside); ipc.companionMenu(false).catch(() => {}); };
  }, [menu]);
  // A card that unmounts while hovered must release the hold.
  useEffect(() => () => { ipc.holdAsk(false).catch(() => {}); }, []);
  async function run(command: () => Promise<void>) {
    setNotice("");
    try { await command(); } catch (e) { setNotice(String(e)); }
  }
  const hold = (on: boolean) => { if (!on) setLap(n => n + 1); void run(() => ipc.holdAsk(on)); };
  return <main className="askwrap"><div className="askcard" role="dialog" aria-label={s.askTitle.replace("{App}", app)}
    onMouseEnter={() => hold(true)} onMouseLeave={() => hold(false)}
    onKeyDown={event => { if (event.key === "Escape" && menu) { event.stopPropagation(); setMenu(false); } }}>
    <button type="button" className="x" aria-label={s.askClose} title={s.askNotThisCall} onClick={() => run(() => ipc.answerAsk(callId, false))}>✕</button>
    <div className="ic"><MotettoMark className="mark"/></div>
    <div><b>{s.askTitle.replace("{App}", app)}</b><p>{notice || s.askBody}</p></div>
    <div className="split" ref={split}>
      <button type="button" className="primary" onClick={() => run(() => ipc.answerAsk(callId, true, screenOn?pick:null))}>{s.askRecord}</button>
      <button type="button" className="primary more" aria-haspopup="menu" aria-expanded={menu} aria-label={s.askMore} onClick={() => setMenu(open => !open)}><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="m6 9 6 6 6-6"/></svg></button>
      {menu && <div className="dropdown" role="menu">
        <button type="button" role="menuitem" autoFocus onClick={() => run(() => ipc.answerAsk(callId, false))}><span>{s.askNotThisCall}</span></button>
        <button type="button" role="menuitem" onClick={() => run(() => ipc.dontAskForApp(callId))}><span>{s.askDontAsk.replace("{App}", app)}</span></button>
      </div>}
    </div>
    {offer&&options.length>0&&<div className="recwhat"><div className="segm" role="radiogroup" aria-label="What to record"><button type="button" role="radio" aria-checked={!screenOn} onClick={()=>setScreenOn(false)}><MicIcon/>{s.scr_audio}</button><button type="button" role="radio" aria-checked={screenOn} onClick={()=>setScreenOn(true)}><MenuIcon name="screen"/>{s.scr_audio_screen}</button></div>{screenOn?<button type="button" className="micchip" data-srcmenu aria-label={`Record from: ${pick?.name}. Change`} aria-haspopup="menu" aria-expanded={!!screenMenu} onClick={e=>{const b=e.currentTarget.getBoundingClientRect();setScreenMenu(screenMenu?null:{left:Math.max(6,Math.min(b.left,innerWidth-306)),top:b.bottom+6});}}><MenuIcon name={pick?.choice.kind==='CallWindow'?'window':'screen'}/><span className="nm">{pick?.name}</span><svg className="chv" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="m6 9 6 6 6-6"/></svg></button>:<span className="tip">{s.scr_none}</span>}</div>}
    {screenMenu&&createPortal(<ScreenMenu live={false} current={pick} options={options} preset={preset} onPick={p=>{setPick(p);setScreenMenu(null);}} onClose={()=>setScreenMenu(null)} style={{position:'fixed',...screenMenu,zIndex:65}}/>,document.body)}
    <div className="cline">{reminder?s.askConsent:"Nothing is recorded until you choose Record."}<i className="timer" key={lap}/></div>
  </div></main>;
}
