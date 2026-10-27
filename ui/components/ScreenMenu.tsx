"use client";
import {useEffect,useRef,useState,type CSSProperties} from 'react';
import {createPortal} from 'react-dom';
import type {ScreenPick} from '@/lib/bindings/ScreenPick';
import type {ScreenOption} from '@/lib/bindings/ScreenOption';
import type {AppState} from '@/lib/bindings/AppState';
import {MenuIcon} from './MenuIcon';
import {MicIcon} from './Microphone';
import * as ipc from '@/lib/ipc';
import {pillLabel,sizeLabel,shareEnded} from '@/lib/screen';
import {elapsedLabel} from '@/lib/session';
import {strings as s} from '@/lib/strings';

export function ScreenMenu({live,current,options,onPick,onClose,preset='sharp',style}: {live:boolean;current:ScreenPick|null;options:ScreenOption[];onPick:(pick:ScreenPick|null)=>void;onClose:()=>void;preset?:string;style?:CSSProperties}) {
  const ref=useRef<HTMLDivElement>(null);
  useEffect(()=>{(ref.current?.querySelector<HTMLButtonElement>('[aria-checked="true"]')??ref.current?.querySelector<HTMLButtonElement>('[role="menuitemradio"]'))?.focus();},[]);
  return <div ref={ref} className="gmenu dropdown micmenu scrmenu" role="menu" aria-label="Screen" style={style} onKeyDown={e=>{
    if(e.key==='Escape'){e.preventDefault();e.stopPropagation();onClose();}
    if(!['ArrowUp','ArrowDown','Home','End'].includes(e.key))return;e.preventDefault();
    const buttons=Array.from(ref.current?.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]')??[]);const at=buttons.indexOf(document.activeElement as HTMLButtonElement);
    buttons[e.key==='Home'?0:e.key==='End'?buttons.length-1:(at+(e.key==='ArrowUp'?-1:1)+buttons.length)%buttons.length]?.focus();
  }}>
    <div className="mhead">{live?s.scr_menu_title:s.scr_record_from}</div>
    {live&&<button type="button" role="menuitemradio" className="mic" aria-checked={!current} onClick={()=>onPick(null)}><MicIcon/><span className="t"><b>{s.scr_audio_only}</b><span>{current?'Stop the video. The audio goes on.':'Recording now.'}</span></span><span className="ck">{!current&&<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="m3.5 8.4 3 3 6-6.8"/></svg>}</span></button>}
    {options.map(option=><button type="button" key={JSON.stringify(option.pick.choice)} role="menuitemradio" className="mic" aria-checked={JSON.stringify(current?.choice)===JSON.stringify(option.pick.choice)} onClick={()=>onPick(option.pick)}><MenuIcon name={option.pick.choice.kind==='CallWindow'?'window':'screen'}/><span className="t"><b>{option.pick.name}</b><span>{option.sub}</span></span><span className="ck">{JSON.stringify(current?.choice)===JSON.stringify(option.pick.choice)&&<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="m3.5 8.4 3 3 6-6.8"/></svg>}</span></button>)}
    <div className="sep"/><p className="scrfoot"><MenuIcon name="lock"/><span>{s.scr_foot.replace('{size}',sizeLabel(preset))}</span></p>
  </div>;
}

export function ScreenControl({state,chip=false,companion=false,onToast}: {state:AppState;chip?:boolean;companion?:boolean;onToast?:(text:string,undo?:()=>Promise<void>)=>void}) {
  const [options,setOptions]=useState<ScreenOption[]>([]),[preset,setPreset]=useState('sharp');
  const [menu,setMenu]=useState<{left:number;top:number}|null>(null),[error,setError]=useState('');
  const [toast,setToast]=useState<{text:string;undo:()=>Promise<void>}|null>(null);
  const anchor=useRef<HTMLButtonElement>(null);
  const pick=state.screen?.while_sharing??state.screen?.pick??null;
  useEffect(()=>{let active=true;Promise.all([ipc.listScreenSources(),ipc.getSettings()]).then(([options,settings])=>{if(active){setOptions(options);setPreset(settings.screen_preset);}}).catch(()=>{});return()=>{active=false;};},[state.meeting_id,menu!==null]);
  useEffect(()=>{if(!menu)return;const outside=(e:PointerEvent)=>{if(!(e.target as Element)?.closest('.scrmenu,[data-scrmenu]'))setMenu(null);};document.addEventListener('pointerdown',outside);if(companion)void ipc.companionMenu(true,400).catch(()=>{});return()=>{document.removeEventListener('pointerdown',outside);if(companion)void ipc.companionMenu(false).catch(()=>{});};},[menu,companion]);
  useEffect(()=>{if(!menu||!companion)return;let active=true,off:(()=>void)|undefined;void ipc.onCompanionFocus(focused=>{if(active&&!focused)setMenu(null);}).then(fn=>{if(active)off=fn;else fn();}).catch(()=>{});return()=>{active=false;off?.();};},[menu,companion]);
  useEffect(()=>{if(!toast)return;const timer=setTimeout(()=>setToast(null),5000);if(companion)void ipc.companionMenu(true,50).catch(()=>{});return()=>{clearTimeout(timer);if(companion)void ipc.companionMenu(false).catch(()=>{});};},[toast,companion]);
  async function choose(next:ScreenPick|null){
    setError('');
    try {await ipc.setScreen(next);setMenu(null);const text=next?(pick?s.scr_switched:s.scr_added).replace('{source}',next.name).replace('{mm:ss}',elapsedLabel(state.elapsed_ms)):s.scr_stopped;const undo=()=>ipc.setScreen(pick);if(onToast)onToast(text,undo);else setToast({text,undo});}
    catch(e){setError(String(e));}
  }
  if(!options.length&&!pick)return null;
  const on=!!state.screen&&!state.screen.waiting&&state.state.type==='Recording';
  return <><button ref={anchor} type="button" className={chip?`micchip${on?' scron':''}`:`iconbtn scrbtn${on?' on':''}`} data-scrmenu aria-label={pillLabel(state.screen)} title={pillLabel(state.screen)} aria-haspopup="menu" aria-expanded={!!menu} onClick={e=>{if(menu)setMenu(null);else{const b=e.currentTarget.getBoundingClientRect();setMenu({left:Math.max(6,Math.min(b.left,innerWidth-306)),top:b.bottom+6});}}}><MenuIcon name={chip&&pick?.choice.kind==='CallWindow'?'window':'screen'}/>{chip&&<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" aria-hidden="true"><path d="m5 6.5 3 3 3-3"/></svg>}</button>
    {menu&&createPortal(<ScreenMenu live current={pick} options={options} preset={preset} onPick={next=>void choose(next)} onClose={()=>{setMenu(null);anchor.current?.focus();}} style={{position:'fixed',...menu,zIndex:65}}/>,document.body)}
    {(toast||error)&&createPortal(<div className="screen-undo" role={error?'alert':'status'}>{error||toast?.text}{toast&&!error&&<button type="button" className="quiet" onClick={()=>void toast.undo().then(()=>setToast(null)).catch(e=>setError(String(e)))}>Undo</button>}<button type="button" className="quiet" aria-label="Dismiss notification" onClick={()=>{setError('');setToast(null);}}>✕</button></div>,document.body)}
  </>;
}

export function ShareVideoOffer({state}: {state:AppState|null}) {
  const [display,setDisplay]=useState<ScreenPick|null>(null),[error,setError]=useState('');
  const previous=useRef<AppState|null>(null);
  const [ended,setEnded]=useState('');
  useEffect(()=>{if(state&&shareEnded(previous.current,state))setEnded(s.scr_share_ended.replace('{App}',state.app??'call'));previous.current=state;},[state]);
  useEffect(()=>{if(!ended)return;const t=setTimeout(()=>setEnded(''),5000);return()=>clearTimeout(t);},[ended]);
  useEffect(()=>{if(!state?.share_video_offer)return;let active=true;ipc.listScreenSources().then(o=>{if(active)setDisplay(o.find(o=>o.pick.choice.kind==='Display')?.pick??null);}).catch(e=>setError(String(e)));return()=>{active=false;};},[state?.share_video_offer]);
  useEffect(()=>{if(!state?.share_video_offer&&!state?.screen?.waiting&&!ended)return;void ipc.companionMenu(true,210).catch(()=>{});return()=>{void ipc.companionMenu(false).catch(()=>{});};},[state?.share_video_offer,state?.screen?.waiting,ended]);
  if(!state)return null;
  const run=(p:ScreenPick|null)=>void ipc.answerShareVideo(p).catch(e=>setError(String(e)));
  return <>{state.screen?.waiting&&<div className="micnote" role="status">{s.video_waiting.replace('{App}',state.app??'call')}</div>}{ended&&<div className="micnote" role="status">{ended}</div>}{state.share_video_offer&&<div className="sharecard scrswap" role="dialog" aria-label={s.scr_share_title}><b>{s.scr_share_title}</b><span>{error||s.scr_share_body.replace('{App}',state.app??'The call app')}</span><div className="field"><button type="button" className="btn sm primary" disabled={!display} onClick={()=>run(display)}>Record {display?.name??'display'}</button><button type="button" className="btn sm ghost" onClick={()=>run(null)}>{s.scr_share_no}</button></div><span className="tip">{s.scr_share_tip.replace('{App}',state.app??'call')}</span></div>}</>;
}
