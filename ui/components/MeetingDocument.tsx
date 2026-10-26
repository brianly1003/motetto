import {useEffect, useRef, useState} from 'react';
import type {Meeting} from '@/lib/bindings/Meeting';
import type {MeetingMedia} from '@/lib/bindings/MeetingMedia';
import MeetingActions, { deletable } from './MeetingActions';
import NotesView from './NotesView';
import {useReadIn, ReadInPicker, ReadInBar} from './ReadIn';
import Transcript from './Transcript';
import DocumentFind from './DocumentFind';
import {meetingDate, meetingTitle, timeLabel, fileSize, durationLabel, notesTime, myNoteTimes, withoutTranscript, taskLine, noteSections} from '@/lib/document';
import {RecordingPage, ProcessingPage, RewritingPage, RetranscribingPage} from './RecordingPage';
import type {AppState} from '@/lib/bindings/AppState';
import type {Stage} from '@/lib/bindings/Stage';
import type {Gap} from '@/lib/useSession';
import {transcriptLines} from '@/lib/transcript';
import * as ipc from '@/lib/ipc';
import {strings as s} from '@/lib/strings';
import {MenuIcon} from './MenuIcon';
import {Alert} from './Alert';

const TITLE_MAX=80;

export type LiveSession = {state:AppState|null; stage:Stage|null; gaps:Gap[]; pending:boolean; act:(command:()=>Promise<void>)=>Promise<void>};

export default function MeetingDocument({meeting, find, onChange, onChanged, onError, onRename, onDelete, renameRequest=0, session, rewriting, onToast}: {find?:{tab:string;query:string}; session?:LiveSession; rewriting?:{stage:Stage|null; transcribe:boolean}; onToast?:(text:string,undo?:()=>Promise<void>)=>void; meeting:Meeting; onChange:(meeting:Meeting)=>void; onChanged:(removed:boolean)=>Promise<void>; onError:(error:string)=>void; onRename:(title:string)=>Promise<void>; onDelete:()=>void; renameRequest?:number}) {
  const [tab,setTab]=useState(find?.tab??'Notes');
  const readIn=useReadIn(meeting,onChange,onError,onToast);
  // Write in from the Read in line opens Write the notes again in that language.
  const [writeIn,setWriteIn]=useState<{code:string}|null>(null);
  // Used once: MeetingActions opens on it first (child effects run before this one).
  useEffect(()=>{if(writeIn)setWriteIn(null);},[writeIn]);
  // A translation's headings look like the original's: its lines are the notes' lines.
  const headings=new Map(noteSections(meeting.notes_md).map(section=>[section.line,section.title]));
  const body=useRef<HTMLDivElement>(null);
  // While this meeting records it is a working page; after Stop, the notes are being written.
  const liveType=session?.state?.meeting_id===meeting.summary.id?session.state.state.type:null;
  const live=liveType==='Recording'||liveType==='Paused'||liveType==='Stopped';
  const writing=!live&&(liveType==='Finishing'||meeting.summary.status==='Finishing');
  // Rename (SPEC 7.6): one editor, this title. Enter saves, Esc or an empty title keeps the old one.
  const title=useRef<HTMLHeadingElement>(null);
  const [editing,setEditing]=useState(false);
  const [count,setCount]=useState(0);
  const [titleKey,setTitleKey]=useState(0);
  // A ref, not state: a blur that arrives after Enter or Esc must not save a second time.
  const editingNow=useRef(false);
  // Locked notes are read-only for review (SPEC 7.11); Rust refuses edits too.
  const locked=meeting.summary.locked;
  async function lock(on:boolean){
    try{onChange({...meeting,summary:await ipc.setLocked(meeting.summary.id,on)});setLockMessage(on?s.lockedNow:s.unlocked);}
    catch(error){onError(String(error));}
  }
  const [lockMessage,setLockMessage]=useState('');
  function startRename(){
    const h=title.current;if(!h||editingNow.current||locked)return;
    editingNow.current=true;setEditing(true);setCount(h.textContent?.length??0);
    requestAnimationFrame(()=>{h.focus();const range=document.createRange();range.selectNodeContents(h);const selection=getSelection();selection?.removeAllRanges();selection?.addRange(range);});
  }
  function capTitle(){
    const h=title.current;if(!h)return;
    if((h.textContent??'').length>TITLE_MAX){h.textContent=(h.textContent??'').slice(0,TITLE_MAX);const range=document.createRange();range.selectNodeContents(h);range.collapse(false);const selection=getSelection();selection?.removeAllRanges();selection?.addRange(range);}
    setCount(h.textContent?.length??0);
  }
  function endRename(save:boolean){
    if(!editingNow.current)return;
    editingNow.current=false;
    const value=(title.current?.textContent??'').replace(/\s+/g,' ').trim();
    setEditing(false);setTitleKey(key=>key+1);
    if(save&&value&&value!==meetingTitle(meeting.summary))void onRename(value);
  }
  useEffect(()=>{if(renameRequest)startRename();},[renameRequest]);
  const [media,setMedia]=useState<MeetingMedia|null>(null);
  const [playing,setPlaying]=useState(false);
  // The player's button, and Space in the Transcript (owner 2026-10-20). From the end, play restarts.
  function togglePlay(){if(!duration)return;setShowPlayer(true);if(cursor.current>=duration){cursor.current=0;seekRequested.current=true;setPosition(0);}setPlaying(value=>!value);}
  const [position,setPosition]=useState(0);
  const [showPlayer,setShowPlayer]=useState(false);
  const audio=useRef<(HTMLAudioElement|null)[]>([]);
  const cursor=useRef(0);
  const seekRequested=useRef(true);
  useEffect(()=>{
    // The tracks are still being written while it records or its notes are written.
    if(live||writing)return;
    let active=true;
    ipc.meetingMedia(meeting.summary.id).then(value=>{if(active)setMedia(value);}).catch(error=>{if(active)onError(String(error));});
    return()=>{active=false;};
  },[meeting.summary.id,live,writing]);
  useEffect(()=>{
    if(!playing || !media) return;
    let active=true;
    let lastTick=performance.now();
    const positioned=new Set<HTMLAudioElement>();
    const starting=new Set<HTMLAudioElement>();
    let leader: number | undefined;
    const tick=()=>{
      const now=performance.now();
      const explicitSeek=seekRequested.current;
      if(explicitSeek){positioned.clear();leader=undefined;seekRequested.current=false;}
      let time=cursor.current;
      if(leader!==undefined){
        const element=audio.current[leader];
        const track=media.tracks[leader];
        // The playing audio owns the clock. Buffering must not skip recorded speech.
        if(element && positioned.has(element))time=track.offset_ms+element.currentTime*1000;
      }else if(!explicitSeek && !media.tracks.some(track=>time>=track.offset_ms && time<track.offset_ms+track.duration_ms)){
        time+=now-lastTick;
      }
      lastTick=now;
      time=Math.min(media.duration_ms,time);
      cursor.current=time;setPosition(time);
      if(time>=media.duration_ms){setPlaying(false);return;}
      // ⌥↩ in the transcript plays one line, then stops (SPEC 7.11).
      if(stopAt.current!==null&&time>=stopAt.current){stopAt.current=null;setPlaying(false);return;}
      const available=media.tracks.map((track,index)=>({track,index})).filter(({track})=>time>=track.offset_ms && time<track.offset_ms+track.duration_ms);
      // Prefer the longest remaining track so a shorter track ending cannot stall its partner.
      if(!available.some(({index})=>index===leader))leader=available.sort((a,b)=>(b.track.offset_ms+b.track.duration_ms)-(a.track.offset_ms+a.track.duration_ms))[0]?.index;
      audio.current.forEach((element,index)=>{
        const track=media.tracks[index];
        if(!element||!track)return;
        const local=(time-track.offset_ms)/1000;
        if(local<0||local>=track.duration_ms/1000){element.pause();positioned.delete(element);return;}
        if(!element.readyState)return;
        if(!positioned.has(element)){
          element.currentTime=local;positioned.add(element);
        // Re-sync a partner only when it is clearly off: in WebKit a seek restarts the audio
        // pipeline, so a 50 ms limit seeked the audible track every ~180 ms and it stuttered.
        // Left alone, two tracks hold a steady ~85 ms start offset.
        }else if(index!==leader && !element.seeking && !element.paused && !starting.has(element) && Math.abs(element.currentTime-local)>.25){
          element.currentTime=local;
        }
        if(element.paused && !starting.has(element)){
          starting.add(element);
          void element.play().catch(error=>{if(active && error.name!=="AbortError"){setPlaying(false);onError(`Couldn't play this audio: ${String(error)}`);}}).finally(()=>starting.delete(element));
        }
      });
    };
    tick();const timer=setInterval(tick,50);
    return()=>{active=false;clearInterval(timer);audio.current.forEach(element=>element?.pause());};
  },[playing,media]);
  const stopAt=useRef<number|null>(null);
  function replay(start:number,end:number){seek(start);stopAt.current=end;}
  function seek(time:number){
    stopAt.current=null;
    if(!media?.duration_ms)return;
    cursor.current=Math.max(0,Math.min(time,media.duration_ms));
    seekRequested.current=true;setPosition(cursor.current);setShowPlayer(true);setPlaying(true);
  }
  const sourceTimes=[...transcriptLines(meeting.transcript,[]).flatMap(line=>line.ms===null?[]:[line.ms]),...myNoteTimes(meeting.notes_md)];
  // Each time cited in the notes, with the line it belongs to (the dot's tooltip).
  const items=new Map<number,string>();
  for(const line of withoutTranscript(meeting.notes_md).split('\n'))for(const match of line.matchAll(/\[(\d+):(\d{2})\]/g)){
    const ms=(Number(match[1])*60+Number(match[2]))*1000;
    if(sourceTimes.includes(ms)&&!items.has(ms))items.set(ms,(taskLine(line)?.text??line).replace(/\[\d+:\d{2}\]|\*\*|^[\s#*-]+/g,'').replace(/\s+/g,' ').trim());
  }
  const duration=media?.duration_ms??0;
  const tracks=media?.tracks??[];
  // One scale for both sides, set by the meeting's loudest moment, so a quietly recorded call
  // still shows its speech and You and Them stay comparable; the square root lifts quiet words.
  const loudest=Math.max(0,...tracks.flatMap(track=>track.waveform));
  function envelope(track:MeetingMedia['tracks'][number]) {
    const x=(time:number)=>duration?time/duration*1000:0;
    const sign=track.speaker==='You'?-1:1;
    const height=(peak:number)=>loudest>0?Math.sqrt(peak/loudest)*17:0;
    return `M${x(track.offset_ms)} 20 `+track.waveform.map((peak,index)=>`L${x(track.offset_ms+index/track.waveform.length*track.duration_ms)} ${20+sign*height(peak)}`).join(' ')+` L${x(track.offset_ms+track.duration_ms)} 20 Z`;
  }
  // Stopped after the call (mockup stopJob): nothing failed, so a quiet warning with one way back.
  const [writingNotes,setWritingNotes]=useState(false);
  async function writeStopped(){
    setWritingNotes(true);
    try{await ipc.meetingAction(meeting.summary.id,'regenerate');await onChanged(false);}
    catch(error){if(String(error)===ipc.STOPPED)await onChanged(false);else onError(String(error));}
    finally{setWritingNotes(false);}
  }
  return <article className="document">
    {meeting.summary.status==='NotesStopped'&&!rewriting&&<Alert variant="warning" title={s.notesStopped} actions={<button className="doc-action" disabled={writingNotes||!ipc.desktopAvailable()} onClick={()=>void writeStopped()}>{s.writeNotes}</button>}>{s.notesStoppedBody}</Alert>}
    <header className={`dochead${editing?' editing':''}`}>
      <div className="titlerow"><div className="tedit"><h1 key={titleKey} ref={title} contentEditable={editing} suppressContentEditableWarning spellCheck={false}
        onClick={()=>{if(!editing)startRename();}} onInput={capTitle} onBlur={()=>endRename(true)}
        onKeyDown={event=>{if(!editingNow.current)return;if(event.key==='Enter'){event.preventDefault();endRename(true);}if(event.key==='Escape'){event.preventDefault();event.stopPropagation();endRename(false);}}}>{meetingTitle(meeting.summary)}</h1>
        {!locked&&<button type="button" className="pen" aria-label={s.rename} title={s.rename} onClick={startRename}><MenuIcon name="rename"/></button>}</div>
        {/* Copy and ••• have nothing to act on while the meeting records or its notes are written. */}
        {deletable(meeting.summary.status)&&!rewriting&&<MeetingActions meeting={meeting} shown={tab==='Notes'?readIn.markdown??undefined:undefined} reading={tab==='Notes'?readIn.view:null} ask={writeIn} hasAudio={tracks.length>0} onToast={onToast} onError={onError} onChanged={onChanged} onRename={startRename} onDelete={onDelete} onLock={on=>void lock(on)}/>}</div>
      <div className="edithint" aria-live="polite"><span><kbd>Enter</kbd> {s.renameSave}</span><span><kbd>Esc</kbd> {s.renameCancel}</span><span className={`count${count>=72?' near':''}`}>{count>=60?`${count}/${TITLE_MAX}`:''}</span></div>
      {live?<div className="document-meta"><span>{meetingDate(meeting.summary.created_ms)}</span>{session?.state?.app&&<><i className="sep"/><span>{session.state.app}</span></>}</div>:
      <div className="document-meta"><span>{meetingDate(meeting.summary.created_ms)}</span>{duration>0&&<><i className="sep"/><span>{durationLabel(duration)}</span></>}{meeting.summary.notes_ms!=null&&<><i className="sep"/><span>Notes in {notesTime(meeting.summary.notes_ms)}</span></>}{locked&&<><i className="sep"/><button type="button" className="locked-mark" title={s.unlockTitle} aria-label={s.unlockTitle} onClick={()=>void lock(false)}><MenuIcon name="lock"/>{s.locked}</button></>}<span className="sr-only" role="status">{lockMessage}</span></div>}
      {!live&&!writing&&!rewriting&&<div className="tl">
        <div className="timeline-wave"><svg viewBox="0 0 1000 40" preserveAspectRatio="none" aria-label="Recorded audio: You above, Them below">
          {tracks.map(track=><path key={track.path} className={track.speaker==='You'?'tl-me':'tl-them'} d={envelope(track)}/>)}
          <line x1="0" x2="1000" y1="20" y2="20" className="tl-mid"/>
          {duration>0&&meeting.summary.marks_ms.map(mark=><line key={mark} x1={mark/duration*1000} x2={mark/duration*1000} y1="2" y2="38" className="tl-mark"/>)}
          {showPlayer&&duration>0&&<line x1={position/duration*1000} x2={position/duration*1000} y1="0" y2="40" className="tl-thread"/>}
        </svg><input type="range" aria-label="Play recording from time" min="0" max={duration||1} value={position} disabled={!duration} onChange={event=>{seek(Number(event.target.value));setTab('Transcript');}}/>
          {/* Moments the notes cite: HTML over the stretched wave, so they stay round at any width. */}
          {duration>0&&<div className="tl-dots">{[...items].map(([ms,text])=><button key={ms} type="button" className="tl-dot" style={{left:`${ms/duration*100}%`}} title={`${timeLabel(ms)} · ${text}`} aria-label={`Play from ${timeLabel(ms)}: ${text}`} onClick={()=>{seek(ms);setTab('Transcript');}}/>)}</div>}</div>
        <div className="tl-legend"><span className="lm">You</span><span className="lt">Them</span><span>{duration?timeLabel(duration):'No saved audio'}</span></div>
      </div>}
    </header>
    {live&&session?.state?<RecordingPage session={{state:session.state,gaps:session.gaps,pending:session.pending,act:session.act}} onToast={onToast}/>:writing?<ProcessingPage id={meeting.summary.id} stage={session?.stage??null} durationMs={meeting.summary.duration_ms??session?.state?.elapsed_ms??0}/>:rewriting?.transcribe?<RetranscribingPage id={meeting.summary.id} stage={rewriting.stage}/>:rewriting?<RewritingPage id={meeting.summary.id} stage={rewriting.stage} lines={meeting.transcript.split('\n').filter(line=>line.trim()).length}/>:<>
    <div className="doctools"><div className="tabs" role="tablist" aria-label={s.meetingViews}>{['Notes','Transcript','Audio'].map((name,index)=><button key={name} id={`tab-${name}`} type="button" role="tab" aria-selected={tab===name} aria-controls="meeting-panel" tabIndex={tab===name?0:-1} onClick={()=>setTab(name)} onKeyDown={event=>{
      if(event.key===' ' && tab==='Transcript' && name==='Transcript' && !event.altKey && !event.ctrlKey && !event.metaKey){event.preventDefault();event.stopPropagation();togglePlay();return;}
      const names=['Notes','Transcript','Audio'];let next=index;
      if(event.key==='ArrowRight')next=(index+1)%3;else if(event.key==='ArrowLeft')next=(index+2)%3;else if(event.key==='Home')next=0;else if(event.key==='End')next=2;else return;
      event.preventDefault();setTab(names[next]);document.getElementById(`tab-${names[next]}`)?.focus();
    }}>{name}</button>)}</div>{tab==='Notes'&&meeting.notes_md&&readIn.desktop&&<ReadInPicker readIn={readIn} meeting={meeting}/>}<DocumentFind tab={tab} body={body} start={find?.query}/></div>
    <div ref={body} className="docbody" id="meeting-panel" role="tabpanel" aria-labelledby={`tab-${tab}`}>
      {tab==='Notes'?(meeting.notes_md?<div className="readin-col"><ReadInBar readIn={readIn} onWriteIn={locked||!deletable(meeting.summary.status)?undefined:code=>setWriteIn({code})}/><NotesView key={readIn.markdown===null?'notes':readIn.view} id={meeting.summary.id} markdown={readIn.markdown??meeting.notes_md} locked={locked} translated={readIn.markdown===null?undefined:{hint:readIn.stale?s.translatedStaleHint:s.translatedHint,labelOf:line=>headings.get(line),onToggle:readIn.toggle}} durationMs={duration} onSeek={duration?ms=>{seek(ms);setTab('Transcript');}:undefined} onChange={notes_md=>onChange({...meeting,notes_md})} onError={onError}/></div>:<p className="muted">{s.noNotes}</p>):tab==='Transcript'?<Transcript onTogglePlay={togglePlay} id={meeting.summary.id} text={meeting.transcript} heard={meeting.transcript_heard} locked={locked} marks={meeting.summary.marks_ms} onSeek={duration?seek:undefined} onReplay={duration?replay:undefined} onChange={(transcript,transcript_heard)=>onChange({...meeting,transcript,transcript_heard})} onError={onError} position={showPlayer?position:undefined}/>:<div className="notes"><section><h3>Audio</h3>{tracks.length?<div className="audio-table"><table><thead><tr><th>Track</th><th>Who</th><th>Size</th></tr></thead><tbody>{tracks.map(track=><tr key={track.path}><td className="mono"><button className="track-play" onClick={()=>seek(track.offset_ms)}>{track.name}</button></td><td>{track.speaker}</td><td className="mono">{fileSize(track.bytes)}</td></tr>)}</tbody></table></div>:<p className="tip">No saved audio is available. Notes and transcript are kept.</p>}</section></div>}
      <aside className="meeting-aside">
        <div className="panel"><h5>This meeting</h5><dl className="kv"><dt>Length</dt><dd>{duration?timeLabel(duration):'Not recorded'}</dd><dt>Language</dt><dd>{languageName(meeting.summary.language)}</dd><dt>Notes in</dt><dd>{meeting.summary.notes_ms!=null?notesTime(meeting.summary.notes_ms):"Not recorded"}</dd><dt>Marks</dt><dd>{meeting.summary.marks_ms.length}</dd></dl></div>
        <div className="panel"><h5>Spellings used</h5>{meeting.summary.spellings?.length?<div className="spelling-chips">{meeting.summary.spellings.map(term=><span key={term} className="chip plain">{term}</span>)}</div>:<p className="tip">No saved spellings</p>}</div>
        <div className="panel"><h5>Stays on this computer</h5><p className="tip">No account, no upload. Folder: <span>{media?.folder??'Loading…'}</span></p></div>
      </aside>
    </div>
    {(showPlayer||tab==='Transcript')&&duration>0&&<div className="player"><button className="doc-action" onClick={togglePlay}>{playing?'❚❚ Pause':'▶ Play'}</button><span className="mono">{timeLabel(position)}</span><div className="bar"><i style={{width:`${position/duration*100}%`}}/></div><span className="tip">{tracks.some(t=>t.speaker==='You')&&tracks.some(t=>t.speaker==='Them')?'Both sides':'Audio'}</span></div>}
    {tracks.map((track,index)=><audio key={track.path} ref={element=>{audio.current[index]=element;}} src={track.path} preload="metadata"/>)}
    </>}
  </article>;
}

/** "English" for "en"; a call in several languages shows their codes, "en, vi" (owner 2026-10-23,
 *  to fit the panel); older meetings saved no language. */
function languageName(code?: string|null): string {
  if (!code) return 'Not recorded';
  if (code.includes(',')) return code.split(',').join(', ');
  try { return new Intl.DisplayNames(['en'], {type: 'language'}).of(code) ?? code; } catch { return code; }
}
