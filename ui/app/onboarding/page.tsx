"use client";
// First run (DESIGN 4.11, SPEC 5.2, mockup renderOnb). Rust runs setup with this window open or
// closed, so everything shown is read back from it: on open, on focus and on Continue.
import {Fragment,useEffect,useRef,useState,type ReactNode} from 'react';
import * as ipc from '@/lib/ipc';
import {strings as s} from '@/lib/strings';
import {fileSize} from '@/lib/document';
import type {Catalog} from '@/lib/bindings/Catalog';
import type {Hardware} from '@/lib/bindings/Hardware';
import type {Permissions} from '@/lib/bindings/Permissions';
import type {PermissionKind} from '@/lib/bindings/PermissionKind';
import type {Settings} from '@/lib/bindings/Settings';
import type {SetupState} from '@/lib/bindings/SetupState';
import type {TierId} from '@/lib/bindings/TierId';
import "../../styles/onboarding.css";

const STEPS=['Welcome','Permissions','Models','Try it'];
const TIERS:TierId[]=['Best','Balanced','Light'];
const icMic=<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4"><rect x="5.5" y="1.5" width="5" height="8.5" rx="2.5"/><path d="M3 8a5 5 0 0 0 10 0M8 13v1.5"/></svg>;
const icScr=<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4"><rect x="1.5" y="2.5" width="13" height="9" rx="1.5"/><path d="M5 14h6"/></svg>;
const icBell=<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4"><path d="M4 11V7a4 4 0 0 1 8 0v4l1.5 1.5h-11zM6.5 14h3"/></svg>;
// "{Mac | PC}" style choices and {name} slots; a slot may be a node (bold parts of models_body).
const pick=(text:string,mac:boolean)=>text.replace(/\{([^{}|]+) \| ([^{}|]+)\}/g,(_,a,b)=>mac?a:b);
function fill(text:string,slots:Record<string,ReactNode>){
  return text.split(/(\{\w+\})/).map((part,i)=>{const key=part.slice(1,-1);return <Fragment key={i}>{/^\{\w+\}$/.test(part)&&key in slots?slots[key]:part}</Fragment>;});
}
const promise=(text:string)=>{const [title,body]=text.split(': ');return <div><b>{title}</b>{body[0].toUpperCase()+body.slice(1)}</div>;};
const times=(rtf:number)=>rtf.toFixed(2);

export default function Onboarding(){
  const [mac,setMac]=useState(true);
  const [step,setStep]=useState(0);
  const [consent,setConsent]=useState(false);
  const [settings,setSettings]=useState<Settings|null>(null);
  const [perms,setPerms]=useState<Permissions|null>(null);
  const [hw,setHw]=useState<Hardware|null>(null);
  const [catalog,setCatalog]=useState<Catalog|null>(null);
  const [setup,setSetup]=useState<SetupState|null>(null);
  const [bytes,setBytes]=useState<Record<string,number>>({});
  const [chosen,setChosen]=useState<TierId|null>(null);
  const [pressed,setPressed]=useState(false);
  const [starting,setStarting]=useState(false);
  const [failure,setFailure]=useState<{id:string;text:string}|null>(null);
  const [relaunch,setRelaunch]=useState(false);
  const [returned,setReturned]=useState(false);
  const [heard,setHeard]=useState<string|null>(null);
  const [testing,setTesting]=useState(false);
  const [error,setError]=useState('');
  const setupTier=useRef<TierId|null>(null);
  const screenWas=useRef<string|null>(null);
  const screenOpened=useRef(false);

  // The user's pick stays until setup reports that tier, or setup itself moved to another one;
  // until then the old tier (or the suggested one) must not show through.
  function showSetup(state:SetupState){
    const moved=setupTier.current!==null&&state.tier!==setupTier.current;
    setChosen(pick=>pick===state.tier||moved?null:pick);
    // Download pressed: "Starting…" until the runner reports work (connecting can take seconds).
    if(['Downloading','SpeedTest','AwaitingAnswer','Failed','Done'].includes(state.phase.type))setStarting(false);
    setupTier.current=state.tier;setSetup(state);
    setBytes(old=>({...old,...Object.fromEntries(state.files.map(([id,have])=>[id,have]))}));
  }
  // Whether a file is done comes only from model_catalog (verified on disk), never from an event.
  async function readSetup(){
    const [state,cat,saved]=await Promise.all([ipc.setupState(),ipc.modelCatalog(),ipc.getSettings()]);
    showSetup(state);setCatalog(cat);setSettings(saved);
    return saved;
  }
  async function readPermissions(){
    const now=await ipc.permissions();
    // macOS applies Screen & System Audio Recording to a new process only: after a grant, reopen.
    if(now.system_audio==='Allowed'&&screenWas.current!==null&&screenWas.current!=='Allowed')setRelaunch(true);
    screenWas.current=now.system_audio;setPerms(now);
  }
  async function readAll(restoreStep:boolean){
    try{
      const [saved]=await Promise.all([readSetup(),readPermissions()]);
      if(saved.consent_confirmed_at)setConsent(true);
      if(saved.tier)setPressed(true);
      if(restoreStep)setStep(Math.min(3,saved.onboarding_step));
    }catch(e){setError(String(e));}
  }

  useEffect(()=>{
    setMac(/Mac/.test(navigator.platform));
    if(!ipc.desktopAvailable()){setError('Open Motetto to set it up.');return;}
    let active=true;const off:Array<()=>void>=[];
    void readAll(true);
    void ipc.hardwareInfo().then(value=>{if(active)setHw(value);}).catch(e=>setError(String(e)));
    // Back from System Settings: macOS may report a Screen grant only after a restart, so Reopen is
    // offered beside Open System Settings. The step is restored on open only, never mid-save.
    const focus=()=>{if(!active)return;if(screenOpened.current)setReturned(true);void readAll(false);};
    window.addEventListener('focus',focus);
    void ipc.onSessionEvent(event=>{
      if(!active)return;
      if(event.type==='DownloadProgress'){setStarting(false);setBytes(old=>({...old,[event.id]:event.bytes}));}
      else if(event.type==='DownloadDone'){setFailure(null);void readSetup().catch(e=>setError(String(e)));}
      else if(event.type==='DownloadFailed'){setFailure({id:event.id,text:event.checksum?s.dlChecksum:s.dlFailed.replace('{reason}',event.reason)});void readSetup().catch(e=>setError(String(e)));}
      else if(event.type==='SpeedTest'){
        // Setup's answer only when it is for the setup tier (a Settings test of another tier isn't).
        const {type:_,...speed}=event;
        if(speed.tier===setupTier.current)setSetup(old=>old&&{...old,speed});
        else void readSetup().catch(e=>setError(String(e)));
      }
      else if(event.type==='SetupChanged'){
        const {type:_,...state}=event;showSetup(state);
        void Promise.all([ipc.modelCatalog(),ipc.getSettings()]).then(([cat,saved])=>{if(active){setCatalog(cat);setSettings(saved);}}).catch(e=>setError(String(e)));
      }
    }).then(fn=>{if(active)off.push(fn);else fn();}).catch(e=>setError(String(e)));
    return()=>{active=false;window.removeEventListener('focus',focus);off.forEach(fn=>fn());};
  },[]);

  async function act(run:()=>Promise<unknown>){setError('');try{await run();}catch(e){setError(String(e));}}
  function go(next:number){setStep(next);void act(()=>ipc.setOnboardingStep(next));}
  async function proceed(){setError('');go(Math.min(3,step+1));await readAll(false);}
  function tick(on:boolean){setConsent(on);if(on)void act(()=>ipc.setOnboardingStep(0,true));}
  async function ask(kind:PermissionKind,grant:string){
    if(kind==='SystemAudio')screenOpened.current=true;
    await act(async()=>{
      if(grant==='NotAsked'&&kind!=='SystemAudio')await ipc.requestPermission(kind);
      else await ipc.openPermissionSettings(kind);
      await readPermissions();
    });
  }

  const tier=chosen??setup?.tier??hw?.suggested??'Best';
  const tierInfo=catalog?.tiers.find(t=>t.id===tier);
  const file=(id?:string)=>catalog?.files.find(f=>f.id===id);
  const rows=tierInfo&&catalog?([['Speech model',file(tierInfo.speech)],['Voice detection',file(catalog.vad)],['Notes model (in the background)',file(tierInfo.notes)]] as const).filter(([,f])=>f):[];
  const tierBytes=(id:TierId)=>{const t=catalog?.tiers.find(x=>x.id===id);return t?[t.speech,catalog!.vad,t.notes].reduce((sum,f)=>sum+(file(f)?.bytes??0),0):0;};
  const allVerified=rows.length>0&&rows.every(([,f])=>f!.on_disk);
  const phase=setup?.phase.type??'Idle';
  const running=phase==='Downloading'||phase==='SpeedTest';
  const started=rows.some(([,f])=>f!.on_disk||(bytes[f!.id]??0)>0);
  const paused=!starting&&!allVerified&&(phase==='Paused'||(phase==='Idle'&&started&&!!settings?.tier));
  const firstOpen=rows.find(([,f])=>!f!.on_disk)?.[1]?.id;
  const failed=setup?.phase.type==='Failed'?setup.phase:null;
  const speechReady=!!tierInfo&&!!file(tierInfo.speech)?.on_disk;
  const notesReady=!!tierInfo&&!!file(tierInfo.notes)?.on_disk;
  const speed=setup?.speed&&setup.speed.tier===setup.tier?setup.speed:null;
  const maxStep=consent?step+1:0;

  async function download(t:TierId){
    const was=pressed;setChosen(t);setPressed(true);setStarting(true);setFailure(null);setError('');
    try{await ipc.startSetup(t);}catch(e){setStarting(false);setPressed(was);setError(String(e));}
  }
  function choose(t:TierId){setChosen(t);if(running)void download(t);}
  // Radio group keys: arrows move the choice and the focus, wrapping (WAI-ARIA radio group).
  function radioKeys(e:React.KeyboardEvent<HTMLDivElement>){
    const move={ArrowRight:1,ArrowDown:1,ArrowLeft:-1,ArrowUp:-1}[e.key];if(!move)return;
    e.preventDefault();const next=TIERS[(TIERS.indexOf(tier)+move+TIERS.length)%TIERS.length];choose(next);
    e.currentTarget.querySelectorAll<HTMLButtonElement>('[role="radio"]')[TIERS.indexOf(next)]?.focus();
  }
  async function record(){setTesting(true);setError('');try{setHeard((await ipc.testRecording()).text.trim());}catch(e){setError(String(e));}finally{setTesting(false);}}

  const stepLine=<div className="eyebrow">Step {step+1} of 4</div>;
  const errorLine=error&&<div className="note warn" role="alert">{error}</div>;
  let pane:ReactNode=null;
  if(step===0)pane=<>{stepLine}<h3>{pick(s.onbWelcomeTitle,mac)}</h3><p>{s.onbWelcomeBody}</p>
    <div className="promise">{promise(s.promiseUpload)}{promise(s.promiseAccount)}{promise(pick(s.promiseVisible,mac))}</div>
    <label className="consent"><input type="checkbox" checked={consent} onChange={e=>tick(e.target.checked)}/> {s.consentLine}</label>{errorLine}
    <div className="foot"><span className="tip">About two minutes, plus the download.</span><button type="button" className="btn primary" disabled={!consent} onClick={()=>void proceed()}>Continue</button></div></>;
  if(step===1){
    const allowed=<span className="chip ok">Allowed</span>;
    const label={Microphone:'Microphone',SystemAudio:'Screen & System Audio Recording',Notifications:'Notifications'};
    const control=(kind:PermissionKind,grant:string,allow:string)=>grant==='Allowed'?allowed:grant==='NotAsked'&&kind!=='SystemAudio'?<button type="button" className="btn sm" onClick={()=>void ask(kind,grant)}>{allow}</button>
      :<button type="button" className="btn sm" aria-label={`Open System Settings for ${label[kind]}`} onClick={()=>void ask(kind,grant)}>Open System Settings</button>;
    const reopen=<button type="button" className="btn sm" onClick={()=>void act(()=>ipc.relaunch())}>{s.permRelaunchButton}</button>;
    const mic=perms?.microphone??'Allowed',screen=perms?.system_audio??'Allowed',bell=perms?.notifications??'Allowed';
    const screenOff=screen!=='Allowed'&&!relaunch;
    pane=<>{stepLine}<h3>{pick(s.permTitle,mac)}</h3><p>{mac?'macOS asks you to approve each one.':'Windows only asks about the microphone and notifications.'} {s.permBody}</p>
      <div className={`perm ${mic==='Denied'?'denied':''}`}><div className="pi">{icMic}</div><div><b>Microphone</b><small>Your side of the call.</small></div>{control('Microphone',mic,'Allow microphone')}</div>
      {mac?<div className={`perm ${screenOff?'denied':''}`}><div className="pi">{icScr}</div><div><b>Screen &amp; System Audio Recording</b><small>{relaunch?s.permRelaunch:screenOff?s.permScreenOff:'The other side of the call (Them). Only the audio is used.'}</small></div>{relaunch?reopen:returned&&screenOff?<span className="controls">{control('SystemAudio',screen,'')}{reopen}</span>:control('SystemAudio',screen,'')}</div>
        :<div className="perm"><div className="pi">{icScr}</div><div><b>Computer sound</b><small>The other side of the call (Them). No permission needed on Windows.</small></div><span className="chip ok">Ready</span></div>}
      <div className={`perm ${bell==='Denied'?'denied':''}`}><div className="pi">{icBell}</div><div><b>Notifications</b><small>So you always see when a recording starts.</small></div>{control('Notifications',bell,s.allowNotifications)}</div>{errorLine}
      <div className="foot"><button type="button" className="btn ghost" onClick={()=>go(0)}>Back</button><button type="button" className="btn primary" onClick={()=>void proceed()}>Continue</button></div></>;
  }
  if(step===2){
    const article=/^([aeiou]|M\d)/i.test(hw?.chip??'')?'an':'a';
    const machine=hw?.chip&&hw.memory_gb?<>This {mac?'Mac':'PC'} has {article} <b>{hw.chip} with {hw.memory_gb} GB</b> of memory</>:hw?.memory_gb?<>This {mac?'Mac':'PC'} has <b>{hw.memory_gb} GB</b> of memory</>:hw?.label??'';
    const speedLine=speed?.keeps_up?<span className="chip ok mono">{s.speedOk.replace('{x}',times(speed.rtf))}</span>
      :speed?<p>{speed.tier==='Light'?s.speedLightSlow:s.speedSlow.replace('{x}',times(speed.rtf))}</p>
      :phase==='SpeedTest'?<span className="chip amber">{s.speedRunning}</span>:null;
    const button=starting?<button type="button" className="btn" disabled>{s.dlStarting}</button>
      :failed?<button type="button" className="btn" onClick={()=>void download(tier)}>Retry</button>
      :phase==='Downloading'?<button type="button" className="btn ghost" onClick={()=>void act(()=>ipc.pauseDownload())}>{s.dlPause}</button>
      :!running&&!allVerified&&(phase==='Idle'||phase==='Paused')?<button type="button" className="btn" onClick={()=>void download(tier)}>{paused||(started&&!!settings?.tier)?s.dlResume:s.dlButton.replace('{size}',fileSize(tierBytes(tier)))}</button>:null;
    pane=<>{stepLine}<h3>{pick(s.modelsTitle,mac)}</h3><p>{fill(s.modelsBody,{hardware:machine,tier:<b>{hw?.suggested??tier}</b>})}</p>
      <div className="tiers" role="radiogroup" aria-label="Model set" onKeyDown={radioKeys}>{TIERS.map(id=>{const t=catalog?.tiers.find(x=>x.id===id);return <button type="button" key={id} className="tier" role="radio" aria-checked={id===tier} tabIndex={id===tier?0:-1} onClick={()=>choose(id)}>
        <b>{id}{id===hw?.suggested&&<> <span className="chip ok">Suggested</span></>}</b><small>{t?(mac?t.suits_mac:t.suits_pc):''}</small><small className="mono">{t?`${file(t.speech)?.label} · ${file(t.notes)?.label}`:''}</small><small>{fileSize(tierBytes(id))}</small></button>;})}</div>
      <div className="dl">{rows.map(([name,f])=>{const done=f!.on_disk,pct=done?100:Math.min(99,Math.round((bytes[f!.id]??0)/f!.bytes*100));
        const why=failure?.id===f!.id?failure.text:failed?.id===f!.id?s.dlFailed.replace('{reason}',failed.reason):paused&&firstOpen===f!.id?s.dlPaused:'';
        return <div className="dlrow" key={name}><span>{name} <span className="tip">{fileSize(f!.bytes)}</span></span><span className="mono">{done?'✓':starting&&firstOpen===f!.id?s.dlStarting:`${pct}%`}</span><div className="progress" role="progressbar" aria-label={name} aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct}><i style={{width:`${pct}%`}}/></div>{why&&<span className={`why ${why===s.dlPaused?'':'err'}`}>{why}</span>}</div>;})}</div>
      <div>{speedLine}{phase==='Done'&&allVerified&&<>{speedLine&&' '}<span className="chip ok">{s.modelsDone}</span></>}</div>
      {failed&&!rows.some(([,f])=>f!.id===failed.id)&&<div className="note warn" role="alert">{failed.reason}</div>}
      <div className="note">{s.dlNote}</div>{errorLine}
      <div className="foot"><button type="button" className="btn ghost" onClick={()=>go(1)}>Back</button><span className="controls">{button}<button type="button" className="btn primary" disabled={!pressed} onClick={()=>void proceed()}>Continue</button></span></div></>;
  }
  if(step===3){
    const offer=setup?.phase.type==='AwaitingAnswer'?setup.phase.offer:null;
    pane=<>{stepLine}<h3>{s.tryTitle}</h3><p>{s.tryBody}</p>
      <div className="testbox">{heard?<><q>{heard}</q><span className="chip ok">{s.tryOk}</span></>:heard===''?<span className="chip amber">{s.tryNothing}</span>:<span className="tip">{speechReady?'Your sentence will appear here.':s.tryWait}</span>}
        <div className="field"><button type="button" className="btn" disabled={!speechReady||testing} aria-busy={testing} onClick={()=>void record()}>{heard!==null?s.tryAgain:`● ${s.tryButton}`}</button></div></div>
      {offer&&setup&&<div className="note ask" role="alert"><span>{s.speedSlow.replace('{x}',times(setup.speed?.rtf??0))}</span><span className="controls">
        <button type="button" className="btn primary sm" onClick={()=>void act(async()=>{await ipc.answerSlow(true);await readSetup();})}>{s.useTier.replace('{tier}',offer)}</button>
        <button type="button" className="btn sm" onClick={()=>void act(async()=>{await ipc.answerSlow(false);await readSetup();})}>{s.keepTier.replace('{tier}',setup.tier)}</button></span></div>}
      {!offer&&speed&&!speed.keeps_up&&speed.tier==='Light'&&<p>{s.speedLightSlow}</p>}
      <p>{s.onbDoneBody}{notesReady?'':` ${s.notesModelPending}`}</p>{errorLine}
      <div className="foot"><button type="button" className="btn ghost" onClick={()=>go(2)}>Back</button><button type="button" className="btn primary" onClick={()=>void act(()=>ipc.finishOnboarding())}>Done</button></div></>;
  }
  return <main className="onb" aria-label="Set up Motetto"><div className="onbin">
    <nav className="onbnav" aria-label="Setup steps">{STEPS.map((name,i)=><button type="button" key={name} className={i<step?'done':''} aria-current={i===step?'step':'false'} disabled={i>maxStep} onClick={()=>go(i)}><span className="n">{i<step?'✓':i+1}</span>{name}</button>)}</nav>
    <div className="onbpane">{pane}</div></div></main>;
}
