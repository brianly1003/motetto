"use client";
import { Alert } from "@/components/Alert";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import LibraryTitlebar from "@/components/LibraryTitlebar";
import Duet from "@/components/MotettoMark";
import RecentlyDeleted from "@/components/RecentlyDeleted";
import MeetingDocument from "@/components/MeetingDocument";
import { ImportingPage } from "@/components/RecordingPage";
import { notesText, deletable } from "@/components/MeetingActions";
import { Dots, MenuIcon, MenuItem } from "@/components/MenuIcon";
import {meetingDate,meetingGroup,meetingTitle,durationLabel,fileSize,timeLabel} from "@/lib/document";
import { useSession } from "@/lib/useSession";
import CommandPalette from "@/components/CommandPalette";
import NotificationPermission from "@/components/NotificationPermission";
import SessionControls from "@/components/SessionControls";
import LibraryHome, { openItems } from "@/components/LibraryHome";
import ScriptsPage from "@/components/ScriptsPage";
import type { Rehearsal } from "@/lib/bindings/Rehearsal";
import * as ipc from "@/lib/ipc";
import { strings as s } from "@/lib/strings";
import type { Meeting } from "@/lib/bindings/Meeting";
import type { MeetingSummary } from "@/lib/bindings/MeetingSummary";
import type { Stage } from "@/lib/bindings/Stage";
import { markStage, startRun } from "@/lib/stageClock";
import type { SearchHit } from "@/lib/bindings/SearchHit";
import { KbdGroup } from "@/components/ui/Kbd";

const GROUPS=["Today","This week","Earlier"];

export default function Main(){
  const session=useSession();
  const live=session.state;
  const [deleted,setDeleted]=useState<MeetingSummary[]>([]);
  const [meetings,setMeetings]=useState<MeetingSummary[]>([]);
  const [bytes,setBytes]=useState(0);
  const [selected,setSelected]=useState<Meeting|null>(null);
  const [query,setQuery]=useState("");
  const [palette,setPalette]=useState(false);
  const [matches,setMatches]=useState<SearchHit[]>([]);
  // A search result opens its meeting on the tab it matched, with Find on the words (n remounts it).
  const [dragging,setDragging]=useState(false);
  const [found,setFound]=useState<{id:string;tab:string;query:string;n:number}|null>(null);
  // A play chip on Home: open that meeting and play from its moment, once.
  const [jump,setJump]=useState<{id:string;ms:number}|null>(null);
  const [loading,setLoading]=useState(true);
  // An import (or Write the notes again) runs beside everything else: its meeting shows its progress
  // in the list and on its own page; only Record and a second Import wait (owner, 2026-10-20).
  const [job,setJob]=useState<ipc.Job|null>(null);
  const jobRef=useRef<ipc.Job|null>(null);
  const [jobStage,setJobStage]=useState<Stage|null>(null);
  const latestStage=useRef<{id:string;stage:Stage}|null>(null);
  const jobId=job?.id;
  const rewritingId=job&&!job.import?job.id:null;
  // Listen before the file picker opens: a short file can reach transcription before its row loads.
  useEffect(()=>{
    if(!ipc.desktopAvailable())return;
    let active=true,off:(()=>void)|undefined;
    void ipc.onSessionEvent(event=>{
      if(!active||event.type!=="Stage")return;
      latestStage.current={id:event.meeting_id,stage:event.stage};
      if(event.meeting_id===jobRef.current?.id){markStage(event.meeting_id,event.stage.type);setJobStage(event.stage);}
    }).then(stop=>{if(active)off=stop;else stop();});
    return ()=>{active=false;off?.();};
  },[]);
  useEffect(()=>{
    if(!jobId){setJobStage(null);latestStage.current=null;return;}
    startRun(jobId);
    const first=latestStage.current?.id===jobId?latestStage.current.stage:null;
    if(first)markStage(jobId,first.type);
    setJobStage(first);
  },[jobId]);
  // Cancel import: the tool stops in Rust, then the import resolves with nothing and the toast says so.
  const [stopping,setStopping]=useState(false);
  const stoppingRef=useRef(false);
  const [desktop,setDesktop]=useState(false);
  const [mac,setMac]=useState(false);
  const [error,setError]=useState("");
  const [quitPending,setQuitPending]=useState(false);
  // Recently deleted is a page you open, never part of the list (SPEC 7.7).
  const [trashOpen,setTrashOpen]=useState(false);
  const [scriptsOpen,setScriptsOpen]=useState(false);
  const [scriptCount,setScriptCount]=useState(0);
  useEffect(()=>{void ipc.scriptsList().then(list=>setScriptCount(Array.isArray(list)?list.length:0)).catch(()=>{});},[]);
  // A rehearsal ended: its report opens on the Scripts page (SPEC 14.6), wherever the library was.
  const [rehearsed,setRehearsed]=useState<Rehearsal|null>(null);
  // "Open Scripts in Motetto" from the strip's script menu.
  useEffect(()=>{const off=ipc.listen<null>("motetto://open-scripts",()=>{selection.current++;setSelected(null);setTrashOpen(false);setScriptsOpen(true);});return()=>{void off.then(fn=>fn());};},[]);
  useEffect(()=>{const off=ipc.listen<Rehearsal>("motetto://rehearsal",r=>{if(r.duration_ms===null)return;selection.current++;setSelected(null);setTrashOpen(false);setScriptsOpen(true);setRehearsed(r);});return()=>{void off.then(fn=>fn());};},[]);
  const [sidebarHidden,setSidebarHidden]=useState(false);
  const sidebar=useRef<HTMLElement>(null),sidebarButton=useRef<HTMLButtonElement>(null);
  useLayoutEffect(()=>{
    let hidden=false;try{hidden=localStorage.getItem("motetto-sidebar")==="hidden";}catch{}
    setSidebarHidden(hidden);document.documentElement.dataset.sidebar=hidden?"hidden":"shown";
  },[]);
  function toggleSidebar(){
    const hidden=!sidebarHidden;
    if(hidden&&sidebar.current?.contains(document.activeElement))sidebarButton.current?.focus();
    if(hidden)setRowMenu(null);
    setSidebarHidden(hidden);
    document.documentElement.dataset.sidebar=hidden?"hidden":"shown";
    try{localStorage.setItem("motetto-sidebar",hidden?"hidden":"shown");}catch{}
  }
  useEffect(()=>{
    const key=(event:KeyboardEvent)=>{
      const modified=mac?event.metaKey&&!event.ctrlKey:event.ctrlKey&&!event.metaKey;
      if(!modified||event.altKey||event.shiftKey||event.repeat||!(event.code==="Backslash"||event.key==="\\"))return;
      event.preventDefault();toggleSidebar();
    };
    document.addEventListener("keydown",key);
    return()=>document.removeEventListener("keydown",key);
  },[mac,sidebarHidden]);
  const [rowMenu,setRowMenu]=useState<{id:string;left:number;top:number}|null>(null);
  const [rename,setRename]=useState<{id:string;n:number}|null>(null);
  const [version,setVersion]=useState(0);
  const [openCount,setOpenCount]=useState<number|null>(null);
  const [toast,setToast]=useState<{text:string;undo?:()=>Promise<void>}|null>(null);
  const toastTimer=useRef<ReturnType<typeof setTimeout>>(undefined);
  const menuRef=useRef<HTMLDivElement>(null);
  const selection=useRef(0);
  const openId=useRef<string|null>(null);
  // The meeting being opened or shown. select/tryOpen set it at once, so a library refresh that
  // finishes later re-reads the new meeting, never the one the page showed before (owner 2026-10-20).
  useLayoutEffect(()=>{openId.current=selected?.summary.id??null;},[selected]);
  const pendingOpen=useRef<string|null>(null);
  // Search results refresh 150 ms after the list; never show a meeting that has just been deleted.
  const hits=query.trim()?matches.filter(hit=>meetings.some(meeting=>meeting.id===hit.meeting.id)):null;
  const shown=hits?hits.map(hit=>hit.meeting):meetings;
  // Sidebar order, so a deleted meeting is followed by the one below it.
  const order=GROUPS.flatMap(group=>shown.filter(meeting=>meetingGroup(meeting.created_ms)===group).map(meeting=>meeting.id));
  useEffect(()=>{
    if (!query.trim()) return;
    let active=true;
    const timer=setTimeout(()=>{ipc.searchMeetings(query).then(result=>{if(active)setMatches(result);}).catch(error=>{if(active)setError(String(error));});},150);
    return()=>{active=false;clearTimeout(timer);};
  },[query,meetings]);
  useEffect(()=>{
    if(!ipc.desktopAvailable())return;
    let off:(()=>void)|undefined,active=true;
    ipc.onFileDrag(setDragging).then(cleanup=>{if(active)off=cleanup;else cleanup();}).catch(()=>{});
    return()=>{active=false;off?.();};
  },[]);
  useEffect(()=>{
    const key=(event:KeyboardEvent)=>{if((event.metaKey||event.ctrlKey)&&event.key===","){event.preventDefault();void ipc.openSettings().catch(e=>setError(String(e)));}if((event.metaKey||event.ctrlKey)&&event.key.toLowerCase()==="k"){event.preventDefault();setPalette(value=>!value);}};
    document.addEventListener("keydown",key);return()=>document.removeEventListener("keydown",key);
  },[]);
  // ⌘⌫ / Ctrl+Delete deletes the open meeting, never while typing.
  useEffect(()=>{
    const key=(event:KeyboardEvent)=>{
      const typing=(event.target as Element|null)?.closest?.("input, textarea, [contenteditable='true']");
      if(typing||!selected||trashOpen||!deletable(selected.summary.status))return;
      if((event.metaKey&&event.key==="Backspace")||(event.ctrlKey&&event.key==="Delete")){event.preventDefault();void deleteMeeting(selected.summary.id);}
    };
    document.addEventListener("keydown",key);return()=>document.removeEventListener("keydown",key);
  });
  useEffect(()=>{
    if(!rowMenu)return;
    const close=(event:PointerEvent)=>{if(!(event.target as Element|null)?.closest?.(".rowmenu, .rowmore"))setRowMenu(null);};
    document.addEventListener("pointerdown",close);return()=>document.removeEventListener("pointerdown",close);
  },[rowMenu]);
  // Beside the row, top edges aligned (a right-click opens it at the pointer); kept inside the window.
  useLayoutEffect(()=>{
    const menu=menuRef.current;if(!menu||!rowMenu)return;
    const top=Math.max(8,Math.min(rowMenu.top,innerHeight-8-menu.offsetHeight));
    if(top!==rowMenu.top)menu.style.top=`${top}px`;
    const left=Math.max(8,Math.min(rowMenu.left,innerWidth-8-menu.offsetWidth));
    if(left!==rowMenu.left)menu.style.left=`${left}px`;
    menu.querySelector<HTMLButtonElement>("button")?.focus();
  },[rowMenu]);
  function notify(text:string,undo?:()=>Promise<void>){
    clearTimeout(toastTimer.current);setToast({text,undo});
    toastTimer.current=setTimeout(()=>setToast(null),undo?5000:3600);
  }
  async function refresh(){setVersion(n=>n+1);try{setMeetings(await ipc.listMeetings());setBytes(await ipc.libraryBytes());setOpenCount(openItems(await ipc.openActionItems()).length);const gone=await ipc.deletedMeetings();setDeleted(gone);if(!gone.length)setTrashOpen(false);setScriptsOpen(false);const next=await ipc.importing(),was=jobRef.current;jobRef.current=next;setJob(next);if(next?.import&&next.id&&next.id!==was?.id)void select(next.id);if(!next)stopped();}catch(e){setError(String(e));}}
  // The sidebar's Action items row: back to Home from a meeting or Recently deleted.
  function goHome(){selection.current++;setRename(null);setSelected(null);setTrashOpen(false);setScriptsOpen(false);}
  async function select(id:string){setRename(null);setJump(value=>value?.id===id?value:null);setTrashOpen(false);setScriptsOpen(false);openId.current=id;const request=++selection.current;try{const meeting=await ipc.getMeeting(id);if(request===selection.current){setSelected(meeting);}}catch(e){if(request===selection.current)setError(String(e));}}
  async function selectForRename(id:string){await select(id);setRename(value=>({id,n:(value?.n??0)+1}));}
  // No confirmation: Undo and 30 days in Recently deleted are the safety net (SPEC 7.7).
  async function deleteMeeting(id:string){
    setRowMenu(null);
    const index=order.indexOf(id), wasOpen=selected?.summary.id===id;
    try{await ipc.meetingAction(id,"delete");}catch(e){setError(String(e));return;}
    await refresh();
    if(wasOpen){const next=order[index+1]??order[index-1];selection.current++;setSelected(null);if(next&&next!==id)await select(next);}
    notify(s.deleted,async()=>{await ipc.meetingAction(id,"restore");await refresh();if(wasOpen)await select(id);});
  }
  async function renameMeeting(id:string,title:string){
    try{const summary=await ipc.renameMeeting(id,title);setSelected(meeting=>meeting&&meeting.summary.id===id?{...meeting,summary}:meeting);await refresh();notify(s.renamed);}
    catch(e){setError(String(e));}
  }
  async function rowAction(action:"open"|"rename"|"copy"|"reveal"|"delete",id:string){
    setRowMenu(null);
    try{
      if(action==="open")await select(id);
      if(action==="rename")await selectForRename(id);
      if(action==="copy"){const meeting=await ipc.getMeeting(id);await navigator.clipboard.writeText(notesText(meeting.notes_md,"items"));notify(s.copied);}
      if(action==="reveal")await ipc.meetingAction(id,"reveal");
      if(action==="delete")await deleteMeeting(id);
    }catch(e){setError(String(e));}
  }
  useEffect(()=>{
    setMac(/Mac/.test(navigator.platform));
    const available=ipc.desktopAvailable();setDesktop(available);
    if(!available){setLoading(false);return;}
    let active=true;const cleanups:Array<()=>void>=[];
    const subscribe=async(name:string,callback:()=>void)=>{const off=await ipc.subscribe(name,callback);if(active)cleanups.push(off);else off();};
    Promise.all([ipc.onOpenMeeting(id=>{if(active){setQuery("");void select(id);}}).then(off=>{if(active)cleanups.push(off);else off();}),refresh(),subscribe("motetto://library-changed",()=>{void refresh().then(()=>{if(pendingOpen.current)void tryOpen(pendingOpen.current);else if(openId.current)void reread(openId.current);});}),subscribe("motetto://quit-pending",()=>setQuitPending(true))])
      .catch(e=>{if(active)setError(String(e));}).finally(()=>{if(active)setLoading(false);});
    return()=>{active=false;selection.current++;cleanups.forEach(off=>off());};
  },[]);
  // A new recording always opens (owner's direction 2026-10-20, DESIGN 4.8). Its folder can appear
  // a moment after the state, so it opens on the worker's library-changed if not yet there.
  const liveId=live&&["Recording","Paused","Stopped"].includes(live.state.type)?live.meeting_id:null;
  // Re-read the open meeting; a short recording that was discarded simply closes.
  async function reread(id:string){const request=++selection.current;try{const meeting=await ipc.getMeeting(id);if(request===selection.current)setSelected(meeting);}catch{if(request===selection.current&&openId.current===id)setSelected(null);}}
  async function tryOpen(id:string){try{const meeting=await ipc.getMeeting(id);pendingOpen.current=null;openId.current=id;setRename(null);setTrashOpen(false);setScriptsOpen(false);selection.current++;setSelected(meeting);}catch{/* not created yet */}}
  useEffect(()=>{
    if(!liveId)return;
    pendingOpen.current=liveId;
    void refresh().then(()=>tryOpen(liveId));
  },[liveId]);
  // The live meeting's page follows the session: Stopped, then Writing the notes, then Ready.
  useEffect(()=>{if(openId.current&&openId.current===live?.meeting_id)void reread(openId.current);},[live?.state.type]);
  async function recordNow(){setTrashOpen(false);setScriptsOpen(false);await session.act(ipc.startRecording);}
  // Recover = Write the notes again on the saved parts (it transcribes them first), one meeting at a
  // time on its own progress page; Discard = Recently deleted, with Undo.
  async function recover(ids:string[]){for(const id of ids){await select(id);notify(s.recovered);try{await ipc.meetingAction(id,"regenerate");}catch(e){setError(String(e));break;}finally{await refresh();}}}
  async function discardRecovery(ids:string[]){try{for(const id of ids)await ipc.meetingAction(id,"delete");}catch(e){setError(String(e));await refresh();return;}await refresh();notify(s.recoveryDiscarded,async()=>{for(const id of ids)await ipc.meetingAction(id,"restore");await refresh();});}
  function cancelImport(){stoppingRef.current=true;setStopping(true);ipc.cancelImport().catch(e=>{stoppingRef.current=false;setStopping(false);setError(String(e));});}
  function stopped(){if(!stoppingRef.current)return;stoppingRef.current=false;setStopping(false);notify(s.importCancelled);}
  async function importRecording(){setError("");try{const meeting=await ipc.importAudio();await refresh();if(meeting)await select(meeting.id);}catch(e){setError(String(e));await refresh();}}
  // Record and Import share one worker: a second job waits, with the reason on the button.
  const importBlocked=job?s.importBusy:live&&live.state.type!=="Idle"?s.importRecording:"";
  const menuMeeting=rowMenu?shown.find(meeting=>meeting.id===rowMenu.id):undefined;
  return <><LibraryTitlebar mac={mac} hidden={sidebarHidden} onToggle={toggleSidebar} buttonRef={sidebarButton}/><main className="shell" data-sidebar={sidebarHidden?"hidden":"shown"} data-drag={dragging?importBlocked||s.dropRelease:undefined} data-blocked={dragging&&importBlocked?"":undefined}>
    <aside className="sidebar" ref={sidebar} id="library-sidebar" inert={sidebarHidden} aria-hidden={sidebarHidden} aria-label={s.meetings} onClickCapture={event=>{
      (event.target as Element).closest<HTMLButtonElement>(".homerow, .meeting-row, .trash")?.focus({preventScroll:true});
    }} onKeyDown={event=>{
      if(event.key!=="ArrowUp"&&event.key!=="ArrowDown"||event.metaKey||event.ctrlKey||event.altKey||event.shiftKey)return;
      const rows=Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>(".homerow, .meeting-row, .trash"));
      const index=rows.indexOf(event.target as HTMLButtonElement);if(index<0)return;
      event.preventDefault();
      const next=rows[Math.max(0,Math.min(rows.length-1,index+(event.key==="ArrowDown"?1:-1)))];
      if(next===rows[index])return;
      next.focus({preventScroll:true});next.scrollIntoView({block:"nearest"});next.click();
    }}>
      <label className="search"><span className="sr-only">{s.search}</span><svg viewBox="0 0 16 16" aria-hidden="true"><circle cx="7" cy="7" r="4.5"/><path d="M10.5 10.5 14 14"/></svg><input placeholder={s.search} value={query} onChange={e=>setQuery(e.target.value)}/><button className="search-key" aria-label="Command palette" onClick={event=>{event.preventDefault();setPalette(true);}}><KbdGroup keys="⌘K"/></button></label>
      {meetings.length>0&&<button className="homerow" aria-current={!trashOpen&&!scriptsOpen&&!selected?"page":undefined} onClick={goHome}><MenuIcon name="items"/><span>{s.actionItems}</span><span className="n" aria-label={openCount?s.openCount.replace("{n}",String(openCount)):undefined}>{openCount||""}</span></button>}
      {/* Scripts, as in the mockup: under Action items, with their names listed below it while open. */}
      <button className="homerow scripts-entry" aria-current={scriptsOpen?"page":undefined} onClick={()=>{selection.current++;setSelected(null);setTrashOpen(false);setScriptsOpen(true);}}><MenuIcon name="script"/><span>{s.scripts}</span><span className="n">{scriptCount||""}</span></button>
      {scriptsOpen&&scriptCount>0&&<nav className="sp-sub" id="scripts-sub" aria-label={s.scripts}/>}
      <nav className="meeting-list" aria-label={s.meetings}>
        {hits?hits.length>0&&<div><div className="list-heading">{s.results}<span>{hits.length}</span></div>{hits.map(({meeting,snippet})=>{const current=!trashOpen&&selected?.summary.id===meeting.id;return <button key={meeting.id} className={`meeting-row hit ${current?"selected":""}`} aria-current={current?"page":undefined} onClick={()=>{setFound(was=>snippet?{id:meeting.id,tab:snippet.source==="Transcript"?"Transcript":"Notes",query:query.trim(),n:(was?.n??0)+1}:null);void select(meeting.id);}}>
          <strong title={meetingTitle(meeting)}><Marked text={meetingTitle(meeting)} query={query}/></strong>
          {snippet&&<em className="snip"><Marked text={snippet.text} query={query}/></em>}
          <span><span>{meetingDate(meeting.created_ms)}</span>{snippet&&<b className="where">{s.matchedIn[snippet.source]}{snippet.at_ms!=null&&<> · {timeLabel(snippet.at_ms)}</>}</b>}</span></button>;})}</div>
        :GROUPS.map(group=>{const items=shown.filter(meeting=>meetingGroup(meeting.created_ms)===group);return items.length?<div key={group}><div className="list-heading">{group}<span>{items.length}</span></div>{items.map(meeting=>{const current=!trashOpen&&selected?.summary.id===meeting.id;return <div key={meeting.id} className={`mrow${rowMenu?.id===meeting.id?" open":""}`} onContextMenu={event=>{event.preventDefault();setRowMenu({id:meeting.id,left:event.clientX,top:event.clientY});}}>
          <button className={`meeting-row ${current?"selected":""}`} aria-current={current?"page":undefined} onClick={()=>select(meeting.id)}><strong title={meetingTitle(meeting)}>{meetingTitle(meeting)}</strong><span>{meetingDate(meeting.created_ms)}{meeting.duration_ms!=null?` · ${durationLabel(meeting.duration_ms)}`:""}{(()=>{const type=live?.meeting_id===meeting.id?live.state.type:null;
            const [cls,label]=type==="Recording"?["rec",s.chipRecording]:type==="Paused"?["plain",s.paused]:type==="Stopped"?["plain",s.chipStopped]:job?.id===meeting.id?["attention",job.transcribe?s.chipTranscribing:s.finishing]:meeting.status==="Ready"?["ready",s.ready]:meeting.status==="Finishing"||type==="Finishing"?["attention",s.finishing]:["attention",s.failed];
            return <i className={cls}>{label}</i>;})()}</span></button>
          <button className="rowmore" aria-label={`${s.rowActions} ${meetingTitle(meeting)}`} aria-haspopup="menu" aria-expanded={rowMenu?.id===meeting.id} onClick={event=>{
            if(rowMenu?.id===meeting.id){setRowMenu(null);return;}
            const row=event.currentTarget.parentElement!.getBoundingClientRect();setRowMenu({id:meeting.id,left:row.right+14,top:row.top});
          }}><Dots/></button></div>;})}</div>:null;})}
        {query&&!shown.length&&<p className="list-empty">{s.none}</p>}
      </nav>
      {deleted.length>0&&<button className="trash" aria-current={trashOpen?"page":undefined} onClick={()=>{selection.current++;setSelected(null);setScriptsOpen(false);setTrashOpen(true);}}><MenuIcon name="trash"/><span>{s.deletedMeetings}</span><span className="n">{deleted.length}</span></button>}
      <div className="privacy"><svg viewBox="0 0 20 20" aria-hidden="true"><rect x="5" y="8" width="10" height="9" rx="2"/><path d="M7 8V5a3 3 0 0 1 6 0v3"/></svg>{(()=>{const facts=`${meetings.length} ${meetings.length===1?s.meeting:s.meetings.toLowerCase()}${bytes>0?` · ${fileSize(bytes)}`:""}`;return <span className="where" title={`${s.privacy} · ${facts}`}><span>{s.privacy}</span><b>&nbsp;· {facts}</b></span>;})()}</div>
    </aside>
    <section className="workspace" aria-busy={loading}>
      <NotificationPermission/><SessionControls hideActive={!!selected}/>
      {error&&<Alert variant="danger" role="alert" title={s.error} onDismiss={()=>setError("")}>{error}</Alert>}
      {quitPending&&<Alert title="Saving before quitting">{s.quitPending}</Alert>}
      {loading?<div className="empty" role="status"><p>{s.loading}</p></div>
        :scriptsOpen?<ScriptsPage onError={setError} device={mac?"Mac":"PC"} pending={rehearsed} onTaken={()=>setRehearsed(null)} onCount={setScriptCount}/>
        :trashOpen?<RecentlyDeleted deleted={deleted} refresh={refresh} notify={notify} onError={setError}/>
        :selected&&job?.import&&job.id===selected.summary.id?<article className="document"><header className="dochead"><h1>{meetingTitle(selected.summary)}</h1><div className="document-meta"><span>{meetingDate(selected.summary.created_ms)}</span></div></header><ImportingPage id={selected.summary.id} stage={jobStage} stopping={stopping} onCancel={cancelImport}/></article>
        :selected?<MeetingDocument key={selected.summary.id+(found?.id===selected.summary.id?`-${found.n}`:"")} find={found?.id===selected.summary.id?found:undefined} at={jump?.id===selected.summary.id?jump.ms:undefined} session={session} meeting={selected} rewriting={rewritingId===selected.summary.id?{stage:jobStage,transcribe:!!job?.transcribe}:undefined} onToast={(text,undo)=>notify(text,undo)} onChange={setSelected} onError={setError} renameRequest={rename?.id===selected.summary.id?rename.n:0} onRename={title=>renameMeeting(selected.summary.id,title)} onDelete={()=>void deleteMeeting(selected.summary.id)} onChanged={async removed=>{await refresh();if(removed){selection.current++;setSelected(null);}else await select(selected.summary.id);}}/>
        :meetings.length?<LibraryHome version={version} onOpen={id=>void select(id)} onPlay={(id,ms)=>{setJump({id,ms});void select(id);}} onCopied={()=>notify(s.itemsCopied)} onImport={importRecording} onError={setError} onCount={setOpenCount} disabled={!desktop||!!importBlocked} disabledReason={importBlocked} onDone={undo=>notify(s.itemDone,undo)} interrupted={meetings.filter(meeting=>meeting.status==="Interrupted"&&meeting.id!==job?.id)} onRecover={ids=>void recover(ids)} onDiscard={ids=>void discardRecovery(ids)}/>
        :<div className="home"><h1>{s.meetings}</h1><div className="empty"><Duet className="mark lg"/><b>{meetings.length?s.libraryTitle:s.emptyTitle}</b><span>{meetings.length?s.libraryBody:s.emptyBody}</span>
          <div className="field">{!meetings.length&&<button className="btn" onClick={()=>void recordNow()} disabled={!desktop||live?.state.type!=="Idle"}>{s.recordNow}</button>}<button className="btn ghost" onClick={importRecording} disabled={!desktop||!!importBlocked} title={importBlocked||undefined}>{s.import}</button></div>{!desktop&&<span className="hint">{s.browser}</span>}</div></div>}
      {toast&&<div className="toast" role="status"><span>{toast.text}</span>{toast.undo&&<button onClick={()=>{const undo=toast.undo!;clearTimeout(toastTimer.current);setToast(null);void undo().catch(e=>setError(String(e)));}}>{s.undo}</button>}<button type="button" className="toast-close" aria-label="Dismiss notification" title="Dismiss notification" onClick={()=>{clearTimeout(toastTimer.current);setToast(null);}}><svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" aria-hidden="true"><path d="m4 4 8 8m0-8-8 8"/></svg></button></div>}
    </section>
    {rowMenu&&menuMeeting&&<div ref={menuRef} className="gmenu rowmenu" role="menu" aria-label={`${s.rowActions} ${meetingTitle(menuMeeting)}`} style={{left:rowMenu.left,top:rowMenu.top}} onKeyDown={event=>{
      if(event.key==="Escape"){setRowMenu(null);document.querySelector<HTMLButtonElement>(`.mrow.open .rowmore`)?.focus();}
      if(["ArrowDown","ArrowUp"].includes(event.key)){event.preventDefault();const items=Array.from(menuRef.current?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]:not(:disabled)')??[]);const index=items.indexOf(document.activeElement as HTMLButtonElement);items[(index+(event.key==="ArrowUp"?-1:1)+items.length)%items.length]?.focus();}
    }}>
      <MenuItem icon="open" label={s.open} onClick={()=>void rowAction("open",menuMeeting.id)}/>
      <MenuItem icon="rename" label={s.rename} onClick={()=>void rowAction("rename",menuMeeting.id)}/>
      <MenuItem icon="copy" label={s.copyActionItems} onClick={()=>void rowAction("copy",menuMeeting.id)}/>
      <MenuItem icon="folder" label={s.reveal} onClick={()=>void rowAction("reveal",menuMeeting.id)}/>
      <div className="sep"/>
      <MenuItem icon="trash" label={s.deleteMeeting} danger disabled={!deletable(menuMeeting.status)} title={deletable(menuMeeting.status)?undefined:s.deleteBusy} onClick={()=>void rowAction("delete",menuMeeting.id)}/>
    </div>}
    {palette&&<CommandPalette onClose={()=>setPalette(false)} onSelect={id=>{setPalette(false);void select(id);}} canRecord={!job} onImport={importBlocked?undefined:()=>{setPalette(false);void importRecording();}} onHome={meetings.length?()=>{setPalette(false);goHome();}:undefined}/>}
  </main></>;
}

// The search words, marked wherever they appear (any case), as in the mockup's results.
function Marked({text,query}:{text:string;query:string}) {
  const words=query.trim();
  if(!words)return <>{text}</>;
  return <>{text.split(new RegExp(`(${words.replace(/[.*+?^${}()|[\]\\]/g,"\\$&")})`,"iu")).map((part,i)=>i%2?<mark key={i}>{part}</mark>:part)}</>;
}
