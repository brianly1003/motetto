"use client";
import { Alert } from "@/components/Alert";
import {NOTE_LANGUAGES, nativeName} from '@/lib/languages';
import {useEffect,useRef,useState,type CSSProperties,type ReactNode} from 'react';
import * as ipc from '@/lib/ipc';
import type {Settings} from '@/lib/bindings/Settings';
import {fileSize} from '@/lib/document';
import {strings as str} from '@/lib/strings';
import type {Catalog} from '@/lib/bindings/Catalog';
import type {SetupState} from '@/lib/bindings/SetupState';
import type {Hardware} from '@/lib/bindings/Hardware';
import type {SpeedResult} from '@/lib/bindings/SpeedResult';
import type {TierId} from '@/lib/bindings/TierId';
import NotificationPermission from '@/components/NotificationPermission';
import {MicLevel,MicNote,micSub,pickMic,useMicReport} from '@/components/Microphone';

const CALL_ORDER=['zoom','teams','teams_classic','meet','whatsapp','telegram','browser','slack','webex','facetime'];
const TABS=['General','Audio','Screen','Calls','Models','Spellings','Privacy','About'];
const SHORTCUTS:Record<string,string>={S:'Start or stop',M:'Mark this moment',J:'Jot a note',N:'Open my notes',Space:'Tuck to the edge',H:'Sharing mode',P:'Peek while sharing'};
function Row({label,help,children}:{label:string;help?:string;children:ReactNode}){return <div className="formrow"><span className="lbl">{label}</span><div>{children}{help&&<div className="help">{help}</div>}</div></div>;}
function Switch({label,on,change}:{label:string;on:boolean;change:(value:boolean)=>void}){return <label className="switch"><input type="checkbox" aria-label={label} checked={on} onChange={e=>change(e.target.checked)}/><span/></label>;}
const LANGUAGES=[['en','English'],['vi','Tiếng Việt'],['zh','中文'],['ja','日本語']] as const;

export default function SettingsPage(){
  const [tab,setTab]=useState('General');
  const [screenPermission,setScreenPermission]=useState<string|null>(null);
  useEffect(()=>{if(tab==='Screen')void ipc.permissions().then(p=>setScreenPermission(p.system_audio)).catch(()=>setScreenPermission(null));},[tab]);
  const [settings,setSettings]=useState<Settings|null>(null);
  const [apps,setApps]=useState<ipc.CallApp[]>([]);
  const [catalog,setCatalog]=useState<Catalog|null>(null);
  const [hw,setHw]=useState<Hardware|null>(null);
  // A catalog file picked in Settings that is not on disk yet: nothing is saved until it is verified (SPEC 8.4).
  const [setup,setSetup]=useState<SetupState|null>(null);
  const [pick,setPick]=useState<{speech?:string;notes?:string}>({});
  const [bytes,setBytes]=useState<Record<string,[number,number]>>({});
  const [failure,setFailure]=useState<Record<string,string>>({});
  const [storage,setStorage]=useState<ipc.StorageSummary|null>(null);
  const [info,setInfo]=useState<{version:string;licences:string;platform:string}|null>(null);
  const [pending,setPending]=useState(false);
  const [working,setWorking]=useState('Saving…');
  const [error,setError]=useState('');
  const [saved,setSaved]=useState('');
  const [wipe,setWipe]=useState(false);
  const [recent,setRecent]=useState<string[]|null>(null);
  const [spelling,setSpelling]=useState<string|null>(null);
  const [heard,setHeard]=useState('');
  const [speed,setSpeed]=useState<SpeedResult|null>(null);
  const live=useRef<{settings:Settings|null;pick:{speech?:string;notes?:string}}>({settings:null,pick:{}});
  live.current={settings,pick};
  const saving=useRef(false);
  const mic=useMicReport(tab==='Audio');
  const [volume,setVolume]=useState<number|null>(null);
  const volumeTimer=useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(()=>{setVolume(mic.report?.active?.volume??null);},[mic.report?.active?.id,mic.report?.active?.volume]);
  const micSaved=(text:string)=>{setError('');setSaved(text);clearTimeout(timer.current);timer.current=setTimeout(()=>setSaved(''),2400);void mic.refresh();};
  const timer=useRef<ReturnType<typeof setTimeout>>(undefined);
  async function refresh(){const [s,a]=await Promise.all([ipc.getSettings(),ipc.callApps()]);setSettings(s);setApps(a);}
  useEffect(()=>{
    const requested=new URLSearchParams(location.search).get('tab');
    if(requested){const match=TABS.find(t=>t.toLowerCase()===requested);if(match)setTab(match);}
    if(!ipc.desktopAvailable()){setError('Open Settings in the Motetto desktop app.');return;}
    let active=true;const off:Array<()=>void>=[];
    void refresh().catch(e=>setError(String(e)));
    void ipc.about().then(setInfo).catch(e=>setError(String(e)));
    Promise.all([ipc.subscribe('motetto://settings-changed',()=>{if(!saving.current)void refresh().catch(e=>setError(String(e)));}),ipc.onSettingsTab(value=>{const match=TABS.find(t=>t.toLowerCase()===value);if(match)setTab(match);})]).then(callbacks=>{if(active)off.push(...callbacks);else callbacks.forEach(fn=>fn());}).catch(e=>setError(String(e)));
    return()=>{active=false;off.forEach(fn=>fn());clearTimeout(timer.current);};
  },[]);
  useEffect(()=>{
    if(!ipc.desktopAvailable())return;
    setWipe(false);setError('');
    if(tab==='Models')void Promise.all([ipc.modelCatalog(),ipc.hardwareInfo(),ipc.setupState()]).then(([c,h,st])=>{setCatalog(c);setHw(h);setSetup(st);}).catch(e=>setError(String(e)));
    if(tab==='Privacy')void ipc.storageSummary().then(setStorage).catch(e=>setError(String(e)));
    if(tab==='About')void ipc.about().then(setInfo).catch(e=>setError(String(e)));
  },[tab]);
  // Whichever tab is open: setup finishing a download or moving on changes settings and the catalog (SPEC 8.4).
  async function reload(){
    if(!saving.current)await refresh();
    const [c,st]=await Promise.all([ipc.modelCatalog(),ipc.setupState()]);
    setCatalog(c);setSetup(st);return c;
  }
  useEffect(()=>{
    if(!ipc.desktopAvailable())return;
    let active=true;const off:Array<()=>void>=[];
    const again=()=>void reload().catch(e=>setError(String(e)));
    window.addEventListener('focus',again);
    void ipc.onSessionEvent(event=>{
      if(!active)return;
      if(event.type==='DownloadProgress')setBytes(old=>({...old,[event.id]:[event.bytes,event.total]}));
      else if(event.type==='DownloadFailed'){setBytes(old=>{const {[event.id]:_,...rest}=old;return rest;});setFailure(old=>({...old,[event.id]:event.checksum?str.dlChecksum:event.reason}));again();}
      else if(event.type==='SetupChanged')again();
      else if(event.type==='DownloadDone'){
        setBytes(old=>{const {[event.id]:_,...rest}=old;return rest;});
        void reload().then(()=>{
          // The file is verified: now the choice that waited for it is saved.
          const {settings:current,pick:waiting}=live.current;
          for(const key of ['speech','notes'] as const){
            if(current&&waiting[key]===event.id){setPick(old=>({...old,[key]:undefined}));void action(()=>ipc.saveSettings({...current,[`${key}_model`]:event.id}));}
          }
        }).catch(e=>setError(String(e)));
      }
    }).then(fn=>{if(active)off.push(fn);else fn();}).catch(e=>setError(String(e)));
    return()=>{active=false;window.removeEventListener('focus',again);off.forEach(fn=>fn());};
  },[]);
  async function action(run:()=>Promise<unknown>,message='Saved',workingMessage='Saving…'){
    if(saving.current)return;
    saving.current=true;setPending(true);setWorking(workingMessage);setError('');setSaved('');clearTimeout(timer.current);
    try{await run();await refresh();setSaved(message);timer.current=setTimeout(()=>setSaved(''),1400);}
    catch(e){setError(String(e));await refresh().catch(()=>{});}
    finally{saving.current=false;setPending(false);}
  }
  const save=(patch:Partial<Settings>)=>settings&&action(()=>ipc.saveSettings({...settings,...patch}));
  function number(label:string,key:'idle_stop_s'|'discard_short_s'|'part_chars',min:number,max:number){return <input type="number" aria-label={label} value={settings?.[key]??''} min={min} max={max} style={{width:key==='part_chars'?90:70}} onChange={e=>{if(settings)setSettings({...settings,[key]:e.target.value===''?0:Number(e.target.value)});}} onBlur={e=>{if(e.currentTarget.validity.valid)void save({[key]:Number(e.currentTarget.value)});else{setError(`Use ${min}-${max} for ${label.toLowerCase()}.`);void refresh();}}} onKeyDown={e=>{if(e.key==='Enter')e.currentTarget.blur();}}/>;}
  function changeTab(value:string){if(!pending)setTab(value);}
  return <main className="settings-window" aria-label="Settings">
    <div className="settabs" role="tablist" aria-label="Settings" onKeyDown={e=>{
      const index=TABS.indexOf(tab);let next=index;
      if(e.key==='ArrowRight')next=(index+1)%TABS.length;else if(e.key==='ArrowLeft')next=(index+TABS.length-1)%TABS.length;else if(e.key==='Home')next=0;else if(e.key==='End')next=TABS.length-1;else return;
      e.preventDefault();changeTab(TABS[next]);document.getElementById(`tab-${TABS[next]}`)?.focus();
    }}>{TABS.map(name=><button key={name} id={`tab-${name}`} type="button" role="tab" tabIndex={tab===name?0:-1} aria-controls="settings-pane" aria-selected={tab===name} disabled={pending} onClick={()=>changeTab(name)}>{name}</button>)}</div>
    <section className="setpane" id="settings-pane" data-tab={tab} role="tabpanel" aria-labelledby={`tab-${tab}`} tabIndex={0}>
      {error&&<Alert variant="danger" className="settings-error" title="Settings need attention" role="alert">{error}</Alert>}
      {!settings?<p role="status">{error?'Settings are unavailable.':'Loading settings…'}</p>:<fieldset disabled={pending} className="settings-fields">
      {tab==='General'&&<>
        <Row label="Appearance"><fieldset className="appearance" aria-label="Appearance" aria-describedby="appearanceHelp"><div className="appearance-options">{['system','light','dark'].map(mode=><label key={mode} className="appearance-option"><span className={`appearance-preview ${mode}`} aria-hidden="true">{(mode==='system'?['light','dark']:[mode]).map(look=><span key={look} className={`appearance-mini ${look}`}><span><i/><i/><i/></span><span><i/><i/><i/></span></span>)}</span><span className="appearance-caption"><input type="radio" name="appearance" value={mode} checked={(settings.appearance??'system')===mode} onChange={async e=>{const input=e.currentTarget;await save({appearance:mode});input.focus();}}/>{mode[0].toUpperCase()+mode.slice(1)}</span></label>)}</div></fieldset><div className="help" id="appearanceHelp" role="status">{!settings.appearance||settings.appearance==='system'?"Matches your computer's appearance.":`Always uses the ${settings.appearance} appearance.`} Applies to all Motetto windows.</div></Row>
        <Row label="Notes folder" help="Each meeting keeps its notes, transcript and audio in one folder."><div className="field"><input type="text" readOnly value={settings.notes_folder} aria-label="Notes folder" style={{width:300}}/><button className="btn sm" title="Choose an empty folder. Motetto verifies the copy; original files are kept for recovery." onClick={()=>void action(()=>ipc.moveNotesFolder(),'Folder moved','Copying and verifying meetings…')}>Choose…</button></div></Row>
        <Row label="Open at login" help={`Motetto waits quietly in the ${info?.platform==='windows'?'tray':'menu bar'}.`}><Switch label="Open at login" on={settings.open_at_login} change={value=>void save({open_at_login:value})}/></Row>
        <Row label="Notes language" help="Writes notes in this language. Use Read in on a meeting for another language."><div className="field"><select aria-label="Notes language" value={settings.notes_language} onChange={e=>void save({notes_language:e.target.value})}><option value="same_as_call">Same as the call</option>{[...NOTE_LANGUAGES].sort((a,b)=>nativeName(a).localeCompare(nativeName(b))).map(id=><option key={id} value={id}>{nativeName(id)}</option>)}</select></div></Row>
        <Row label="During a call" help="Always visible while recording. Shrink to a dot or tuck to the screen edge."><div className="field"><select aria-label="Companion size when a recording starts" value={settings.companion.default_size} onChange={e=>void save({companion:{...settings.companion,default_size:e.target.value}})}><option value="pill">Pill</option><option value="dot">Dot</option></select><Switch label="Show the latest line in the Pill" on={settings.companion.captions} change={captions=>void save({companion:{...settings.companion,captions}})}/><span>Show the latest line</span></div></Row>
        <Row label="Sounds" help="A short chime when a recording starts and stops."><Switch label="Start and stop sounds" on={settings.sounds} change={sounds=>void save({sounds})}/></Row>
        <Row label="Shortcuts" help="Edit a shortcut and leave the field to register it."><dl className="kv settings-shortcuts">{Object.entries(SHORTCUTS).map(([key,label])=><div key={key}><dt>{label}</dt><dd><input aria-label={`${label} shortcut`} value={settings.shortcuts[key]} onChange={e=>setSettings({...settings,shortcuts:{...settings.shortcuts,[key]:e.target.value}})} onBlur={()=>void save({shortcuts:settings.shortcuts})} onKeyDown={e=>{if(e.key==='Enter')e.currentTarget.blur();}}/></dd></div>)}<div><dt>Command palette</dt><dd>{info?.platform==='windows'?'Ctrl+K':'⌘K'}</dd></div></dl></Row>
        <Row label="First run"><button className="btn sm" onClick={()=>void action(async()=>{await ipc.setOnboardingStep(0);await ipc.openSetup();},'')}>Run setup again</button></Row>
      </>}
      {tab==='Audio'&&(()=>{
        // The microphone (DESIGN 4.15): the level moves only while Motetto records; it never opens the mic just to show it.
        const r=mic.report,m=r?.active,system=r?.mics.find(x=>x.is_default),mac=info?.platform!=='windows';
        const sound=<button className="btn sm" onClick={()=>void ipc.openSoundSettings().catch(e=>setError(String(e)))}>Open Sound settings…</button>;
        if(!mac)return <Row label="Microphone" help="Motetto records the default microphone. Choose it in Windows sound settings.">{sound}</Row>;
        if(!r)return <p role="status">Loading microphones…</p>;
        return <>
          <Row label="Microphone" help={m?`Motetto records ${m.name} (${micSub(m)}). Your call app keeps its own choice; pick the same mic there if you want both to match. The level moves while Motetto records.`:'No microphone is connected.'}>
            <div className="field"><select id="mic-select" aria-label="Microphone" style={{width:320}} value={r.chosen??''} onChange={e=>void pickMic(r,e.target.value||null,micSaved,false).catch(err=>setError(String(err)))}>
              <option value="">System default ({system?.name??'none'})</option>
              {r.chosen&&!r.mics.some(x=>x.id===r.chosen)&&<option value={r.chosen}>Not connected</option>}
              {r.mics.map(x=><option key={x.id} value={x.id}>{x.name} · {micSub(x)}</option>)}
            </select></div>
            <div className="setlevel"><span>Level</span><MicLevel bars={20} className="bars"/></div>
            <MicNote report={r} done={micSaved} onError={setError} className="micnote recnote setnote"/>
          </Row>
          <Row label="Input volume" help={m?.volume!=null?'The same setting as System Settings › Sound. Keep loud speech out of the red.':undefined}>
            {m&&volume!==null?<label className="volrow"><input type="range" min={0} max={100} value={Math.round(volume*100)} style={{"--v":volume} as CSSProperties} aria-label={`Input volume for ${m.name}`} onChange={e=>{const v=Number(e.target.value)/100;setVolume(v);clearTimeout(volumeTimer.current);volumeTimer.current=setTimeout(()=>void ipc.setMicVolume(m.id,v).then(()=>micSaved('Saved')).catch(err=>setError(String(err))),150);}}/><output>{Math.round(volume*100)}%</output></label>
              :<span className="help" style={{margin:0}}>{m?`${m.name} sets its own level.`:''}</span>}
          </Row>
          <Row label="All apps" help="Changes the default microphone for every app, including your call app.">{sound}</Row>
        </>;
      })()}
      {tab==='Calls'&&<>
        <Row label="Ask when a call starts" help="Motetto asks every time. Nothing is recorded until you choose Record. Turn this off to record only with Record now."><Switch label="Ask when a call starts" on={settings.ask_on_call} change={on=>void action(()=>ipc.setAskOnCall(on))}/></Row>
        <Row label="Call apps" help="Voice messages don't count: messaging apps must also play sound for 5 seconds."><div className="tbl"><table><thead><tr><th>App</th><th>Counts as a call when</th><th>When a call starts</th></tr></thead><tbody>{apps.filter(app=>info?.platform!=='windows'||app.app_id!=='facetime').sort((a,b)=>(CALL_ORDER.includes(a.app_id)?CALL_ORDER.indexOf(a.app_id):99)-(CALL_ORDER.includes(b.app_id)?CALL_ORDER.indexOf(b.app_id):99)).map(app=><tr key={app.app_id}><td>{app.app_id==='browser'?'Other browser calls':app.app_id==='slack'?'Slack huddles':app.name}</td><td>{app.app_id==='meet'?'a browser window titled "Meet"':app.single_signal?'it uses the mic':app.app_id==='browser'?'a browser uses the mic and plays sound':'it uses the mic and plays sound'}</td><td><select aria-label={`When ${app.name} starts a call`} value={app.mode} onChange={e=>void action(()=>ipc.setAppMode(app.app_id,e.target.value as 'ask'|'off'))}><option value="ask">Ask</option><option value="off">Off</option></select></td></tr>)}</tbody></table></div><div className="field settings-add"><button className="btn sm" onClick={()=>void action(async()=>setRecent(await ipc.recentMicApps()),'')}>Add app…</button><span className="help">Lists apps that used the microphone this week.</span></div>{recent&&<div className="note">{recent.length?recent.map(id=><div className="field" key={id}><span>{id}</span><button className="btn sm" onClick={()=>void action(async()=>{await ipc.addApp(id);setRecent(null);})}>Add {id}</button></div>):<span>No new microphone apps have been seen this week.</span>}<button className="btn sm ghost" onClick={()=>setRecent(null)}>Cancel</button></div>}</Row>
        <Row label="Stop when the mic is quiet for"><div className="field">{number('Idle seconds','idle_stop_s',10,300)}<span>seconds</span></div></Row>
        <Row label="Discard recordings shorter than" help="0 keeps every recording."><div className="field">{number('Discard seconds','discard_short_s',0,120)}<span>seconds</span></div></Row>
      </>}
      {tab==='Models'&&(()=>{
        // Options are the catalog's files: label · size · tier. Choosing one that is not on disk waits for its download.
        const tierOf=(id:string)=>catalog?.tiers.find(t=>t.speech===id||t.notes===id)?.id;
        const files=(kind:'Speech'|'Notes')=>catalog?.files.filter(f=>f.kind===kind)??[];
        const choose=(kind:'speech'|'notes',id:string)=>{
          const file=catalog?.files.find(f=>f.id===id);if(!file)return;
          setFailure(old=>({...old,[id]:''}));
          if(file.on_disk){setPick(old=>({...old,[kind]:undefined}));void save({[`${kind}_model`]:id});}else setPick(old=>({...old,[kind]:id}));
        };
        const model=(kind:'speech'|'notes',label:string,className:string,extra?:ReactNode)=>{
          const current=settings[`${kind}_model`];
          // Setup's own download: the tier's file is shown as the choice, with its status, until it is verified and saved.
          const tierFile=!current&&!pick[kind]&&settings.tier&&catalog?catalog.tiers.find(t=>t.id===(setup?.tier??settings.tier))?.[kind]:undefined;
          const tierWait=tierFile&&!catalog?.files.find(f=>f.id===tierFile)?.on_disk?tierFile:undefined;
          const tierProgress=tierWait&&(bytes[tierWait]??setup?.files.find(f=>f[0]===tierWait)?.slice(1));
          // Only what setup is doing: an idle, finished or failed runner isn't "waiting" for this file.
          const phase=setup?.phase;
          const tierLine=!tierWait||!phase?'':phase.type==='Downloading'&&phase.id===tierWait?`Downloading… ${Math.floor((tierProgress?tierProgress[0]/Math.max(1,tierProgress[1]):0)*100)}%`
            :phase.type==='Paused'?'Download paused':phase.type==='Failed'&&phase.id===tierWait?phase.reason
            :phase.type==='Downloading'||phase.type==='SpeedTest'||phase.type==='AwaitingAnswer'?'Waiting to download':'';
          // A tier file already on disk is shown too, between its DownloadDone and the saved choice.
          const shown=pick[kind]??(current||tierFile||''),waiting=pick[kind]?catalog?.files.find(f=>f.id===pick[kind]):undefined;
          const progress=waiting&&bytes[waiting.id];
          return <Row label={kind==='speech'?'Speech':'Notes'} help={waiting?'The current model stays in use until the new one is downloaded and checked.':undefined}>
            <div className="field"><select className={className} aria-label={label} value={shown} onChange={e=>choose(kind,e.target.value)}>{!shown&&<option value="">Choose a model</option>}{files(kind==='speech'?'Speech':'Notes').map(f=>{const tier=tierOf(f.id);return <option key={f.id} value={f.id}>{f.label} · {fileSize(f.bytes)}{tier&&` · ${tier}`}</option>;})}</select>{extra}</div>
            {tierLine&&<div className="field model-wait"><span role="status">{tierLine}</span></div>}
            {waiting&&<div className="field model-wait"><span>{str.modelNeedsDownload}</span>{progress?<span className="mono" role="status">{Math.floor(progress[0]/Math.max(1,progress[1])*100)}%</span>:<button className="btn sm" onClick={()=>{setFailure(old=>({...old,[waiting.id]:''}));void ipc.downloadModel(waiting.id).catch(e=>setError(String(e)));}}>{str.dlButton.replace('{size}',fileSize(waiting.bytes))}</button>}</div>}
            {waiting&&failure[waiting.id]&&<div className="help" role="status">{failure[waiting.id]}</div>}
          </Row>;
        };
        const used=[settings.speech_model,settings.notes_model];
        const unused=catalog?.files.filter(f=>f.on_disk&&f.id!==catalog.vad&&!used.includes(f.id)).reduce((sum,f)=>sum+f.bytes,0)??0;
        const mac=info?.platform!=='windows';
        const result=speed??settings.speed_test;
        const testTier:TierId|undefined=tierOf(settings.speech_model)??hw?.suggested;
        // `speed_slow` from setup's speed test waits for an answer before the notes model downloads.
        const offer=settings.slow_question;
        const slowRtf=settings.speed_test?.rtf??0;
        return <>
        {offer&&<div className="note ask" role="alert"><span>{str.speedSlow.replace('{x}',slowRtf.toFixed(2))}</span><span className="controls">
          <button className="btn primary sm" onClick={()=>void action(()=>ipc.answerSlow(true))}>{str.useTier.replace('{tier}',offer)}</button>
          <button className="btn sm" onClick={()=>void action(()=>ipc.answerSlow(false))}>{str.keepTier.replace('{tier}',settings.tier??'')}</button></span></div>}
        {model('speech','Speech model','speech-model',<span className="chip plain">Local</span>)}
        {model('notes','Notes model','notes-model')}
        <Row label="Call language" help="Detect handles mixed-language calls; pick one if detection guesses wrong."><div className="field"><select className="call-language" aria-label="Call language" value={settings.call_language} onChange={e=>void save({call_language:e.target.value})}><option value="detect">Detect</option>{LANGUAGES.map(([id,label])=><option key={id} value={id}>{{en:'English',vi:'Vietnamese',zh:'Chinese',ja:'Japanese'}[id]??label}</option>)}</select></div></Row>
        <Row label="Long calls"><div className="field"><span>Summarize in parts of</span>{number('Part size in characters','part_chars',8000,48000)}<span>characters</span></div></Row>
        <div className="note">{hw?`This ${mac?'Mac':'PC'}: ${hw.label}. `:''}{result?`Last speed test: ${result.rtf.toFixed(2)}× real time${result.tier!==tierOf(settings.speech_model)?` (${result.tier})`:''}. ${result.keeps_up?'This speech model keeps up live.':'This speech model may fall behind with two tracks.'}`:'No speed test has run yet.'} <button className="btn sm" disabled={!testTier} onClick={()=>testTier&&void action(async()=>setSpeed(await ipc.runSpeedTest(testTier)),'Speed test complete','Running the speed test…')}>Run the speed test again</button><div className="help">Uses a short English recording that ships with Motetto; nothing is recorded.</div></div>
        {unused>0&&<Row label="Unused models" help="Downloaded files the chosen models don't use. They can be downloaded again."><button className="btn sm" onClick={()=>void action(async()=>{await ipc.deleteUnusedModels();setCatalog(await ipc.modelCatalog());},'Unused models deleted')}>{str.unusedModels.replace('{size}',fileSize(unused))}</button></Row>}
        </>;
      })()}
      {tab==='Spellings'&&<>
        <p className="settings-intro">Names and terms the models can&apos;t know. Add spellings here for future meetings. Motetto uses up to 40 names as data for local speech and notes models.</p>
        <div className="tbl"><table><thead><tr><th>Spelling</th><th>Often heard as</th><th>From</th><th>Used in</th><th/></tr></thead><tbody>{settings.spellings.map((entry,index)=><tr key={index}><td><b>{entry.spelling}</b></td><td>{entry.heard||'—'}</td><td><span className="chip plain">Added by you</span></td><td className="mono">Not tracked</td><td><button className="btn sm ghost" aria-label={`Remove ${entry.spelling}`} onClick={()=>void save({spellings:settings.spellings.filter((_,i)=>i!==index)})}>Remove</button></td></tr>)}{!settings.spellings.length&&<tr><td colSpan={5}>No spellings yet.</td></tr>}</tbody></table></div>
        {spelling===null?<div className="field"><button className="btn sm" onClick={()=>{setSpelling('');setHeard('');}}>Add spelling</button></div>:<div className="field"><input autoFocus aria-label="Spelling" maxLength={40} value={spelling} onChange={e=>setSpelling(e.target.value)}/><input aria-label="Often heard as" maxLength={80} placeholder="Often heard as" value={heard} onChange={e=>setHeard(e.target.value)}/><button className="btn sm" disabled={!spelling.trim()} onClick={()=>void action(async()=>{await ipc.saveSettings({...settings,spellings:[...settings.spellings,{spelling:spelling.trim(),heard:heard.trim()}]});setSpelling(null);})}>Add</button><button className="btn sm ghost" onClick={()=>setSpelling(null)}>Cancel</button></div>}
      </>}
      {tab==='Screen'&&<>
        <div className="note"><b>Audio only, unless you choose.</b> Pick Audio + screen when Motetto asks to record, or the screen button in the companion during a call. Each new call starts with audio only again.</div>
        <Row label="Offer Audio + screen" help="Shows the choice on the card that asks to record. Off hides it; the companion still has it."><Switch label="Offer Audio + screen when asking to record" on={settings.screen_offer_on_ask} change={on=>void save({screen_offer_on_ask:on})}/></Row>
        <Row label="Record from" help="The call's window holds the call and whatever people share in it. The whole display also catches notifications, other apps and what you type in them."><div className="field"><select aria-label="Record from" value={settings.screen_source} onChange={e=>void save({screen_source:e.target.value})}><option value="call_window">The call's window</option><option value="display">The whole display</option></select></div></Row>
        <Row label="When you share your screen" help="The call app hides its window while you share, so your demo would be missing. Turn on sharing mode with your shortcut in General. Motetto asks once and returns to the window when you turn sharing mode off."><div className="field"><select aria-label="When you share your screen" value={settings.screen_on_own_share} onChange={e=>void save({screen_on_own_share:e.target.value})}><option value="ask">Ask to record your display</option><option value="keep_window">Keep the call's window</option></select></div></Row>
        <Row label="Picture" help="Screen shares cost little because most of the picture stays still; a lot of camera video can double the size. Encoded by this computer's own hardware (H.264)."><div className="field"><select aria-label="Picture quality" value={settings.screen_preset} onChange={e=>void save({screen_preset:e.target.value})}><option value="sharp">Sharp · up to 1080p, 15 fps · about 110 MB an hour</option><option value="smaller">Smaller · up to 720p, 10 fps · about 70 MB an hour</option></select></div></Row>
        <Row label="Show the pointer"><Switch label="Show the pointer in the video" on={settings.screen_cursor} change={on=>void save({screen_cursor:on})}/></Row>
        <Row label="Permission" help="Screen & System Audio Recording, which Motetto already uses for the other side of the call. macOS shows its own recording indicator while the screen is recorded.">{screenPermission==='Allowed'?<span className="chip ok">Allowed</span>:<button type="button" className="btn sm" onClick={()=>void ipc.openPermissionSettings('SystemAudio')}>{screenPermission==='Denied'?'Denied - Open settings':'Check permission'}</button>}</Row>
      </>}
      {tab==='Privacy'&&<>
        <div className="note"><b>Everything stays on this {info?.platform==='windows'?'PC':'Mac'}.</b> No account, no analytics, no crash reports. The network is used only to download models when you ask.</div>
        <Row label="Keep audio" help="Notes and transcripts are kept until you delete them. Retention runs after notes finish and when Motetto opens."><div className="field"><select aria-label="Keep audio" value={settings.keep_audio} onChange={e=>void save({keep_audio:e.target.value})}><option value="30_days">30 days after the notes</option><option value="forever">Until I delete the meeting</option><option value="after_notes">Delete right after the notes</option></select></div></Row>
        <Row label="Recording reminder" help={'Shows "Tell the people on the call" when Motetto asks to record.'}><Switch label="Recording reminder" on={settings.recording_reminder} change={recording_reminder=>void save({recording_reminder})}/></Row>
        <Row label="Keep video" help="Never longer than the audio, which the Video tab plays."><div className="field"><select aria-label="Keep video" value={settings.keep_video} onChange={e=>void save({keep_video:e.target.value})}><option value="same_as_audio">Same as the audio</option><option value="7_days">7 days after the notes</option><option value="forever">Until I delete the meeting</option></select></div></Row>
        <Row label="Storage"><dl className="kv settings-storage"><dt>Meetings</dt><dd>{storage?.meetings??'…'}</dd><dt>Audio</dt><dd>{storage?fileSize(storage.audio_bytes):'…'}</dd><dt>Video</dt><dd>{storage?fileSize(storage.video_bytes):'…'}</dd><dt>Notes and transcripts</dt><dd>{storage?fileSize(storage.notes_bytes):'…'}</dd><dt>Free on disk</dt><dd>{storage?fileSize(storage.free_bytes):'…'}</dd></dl></Row>
        <Row label="Delete everything"><div className="field">{wipe?<><span>Delete {storage?.meetings??'all'} meetings and {storage?fileSize(storage.audio_bytes+storage.video_bytes):'all'} of audio and video? This can&apos;t be undone. Recently deleted meetings are included.</span><button className="btn sm rec" onClick={()=>void action(async()=>{await ipc.deleteAllMeetings();setStorage(await ipc.storageSummary());setWipe(false);},'All meetings deleted')}>Delete all</button><button className="btn sm ghost" onClick={()=>setWipe(false)}>Cancel</button></>:<button className="btn sm" disabled={!storage?.meetings} onClick={()=>setWipe(true)}>Delete all meetings…</button>}</div></Row>
        <NotificationPermission/>
      </>}
      {tab==='About'&&<>
        <Row label="Version"><span className="mono">Motetto {info?.version??'…'}</span></Row>
        <Row label="Updates" help="A public releases page has not been configured for this local build. Motetto never checks on its own."><button className="btn sm" disabled>Check for updates</button></Row>
        <Row label="Built with" help="Model installers and bundled full licence texts are planned for distribution in M9."><span>{info?.licences??'…'}</span></Row>
        <Row label="Logs" help="Logs never contain transcript text."><button className="btn sm" onClick={()=>void action(ipc.openLogsFolder,'Logs folder opened')}>Show logs folder</button></Row>
      </>}
      </fieldset>}
      <span className="saved" role="status" aria-live="polite">{pending?working:saved}</span>
    </section>
  </main>;
}
