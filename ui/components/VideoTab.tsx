"use client";
import {useEffect,useRef,useState} from 'react';
import type {MeetingMedia} from '@/lib/bindings/MeetingMedia';
import type {VideoPart} from '@/lib/bindings/VideoPart';
import {partAt,retentionLabel} from '@/lib/screen';
import {transcriptLines} from '@/lib/transcript';
import {timeLabel,fileSize} from '@/lib/document';
import {strings as s} from '@/lib/strings';
import * as ipc from '@/lib/ipc';

const Play=()=> <svg viewBox="0 0 16 16" fill="currentColor" aria-hidden="true"><path d="M4.5 2.8v10.4a.6.6 0 0 0 .9.5l8-5.2a.6.6 0 0 0 0-1l-8-5.2a.6.6 0 0 0-.9.5z"/></svg>;
// A paused, muted video at that time, not a canvas copy: drawImage asks WebKit's GPU process for
// the frame synchronously, and while the frame's bytes are still loading the window froze for seconds.
function Thumbnail({part,ms}: {part:VideoPart;ms:number}) {
  const video=useRef<HTMLVideoElement>(null);
  useEffect(()=>{
    const element=video.current;if(!element)return;let active=true;
    const show=()=>{if(active)element.currentTime=ms/1000;};element.addEventListener('loadedmetadata',show);
    const observer=new IntersectionObserver(entries=>{if(entries.some(e=>e.isIntersecting)){element.src=part.path;observer.disconnect();}},{rootMargin:'100px'});observer.observe(element);
    return()=>{active=false;observer.disconnect();element.removeEventListener('loadedmetadata',show);element.removeAttribute('src');element.load();};
  },[part.path,ms]);
  return <video ref={video} muted playsInline preload="metadata" tabIndex={-1} aria-hidden="true"/>;
}

export default function VideoTab({media,position,playing,seek,togglePlay,rate,setRate,lines,meetingId,onToast,onMediaChange,onError}: {media:MeetingMedia;position:number;playing:boolean;seek:(ms:number)=>void;togglePlay:()=>void;rate:number;setRate:(rate:number)=>void;lines:ReturnType<typeof transcriptLines>;meetingId:string;onToast?:(text:string,undo?:()=>Promise<void>)=>void;onMediaChange:(media:MeetingMedia)=>void;onError:(error:string)=>void}) {
  const video=useRef<HTMLVideoElement>(null),frame=useRef<HTMLDivElement>(null),lineBox=useRef<HTMLDivElement>(null);
  const [cc,setCC]=useState(true),[confirm,setConfirm]=useState(false),[busy,setBusy]=useState(false),[error,setError]=useState('');
  const [retention,setRetention]=useState('Kept no longer than the audio (Settings › Privacy).');
  useEffect(()=>{let active=true;void ipc.getSettings().then(settings=>{if(active)setRetention(retentionLabel(settings.keep_audio,settings.keep_video));}).catch(()=>{});return()=>{active=false;};},[]);
  const current=partAt(media.videos,position),path=current?.part.path;
  const latest=useRef({position,playing,rate,error});latest.current={position,playing,rate,error};
  const syncVideo=useRef<(()=>void)|null>(null);
  useEffect(()=>{
    const element=video.current;if(!element)return;let active=true,starting=false;setError('');
    const sync=()=>{if(!active||!path)return;const state=latest.current;const at=partAt(media.videos,state.position);if(!at||at.part.path!==path||state.error)return;
      if(element.readyState>=1&&!element.seeking&&Math.abs(element.currentTime-at.fileMs/1000)>.25)element.currentTime=at.fileMs/1000;
      // Repeated rate/play commands can stall WebKit's native media process.
      if(element.playbackRate!==state.rate)element.playbackRate=state.rate;
      if(state.playing){if(element.paused&&!starting){starting=true;void element.play().catch(e=>{if(active&&e.name!=='AbortError')setError(`Couldn't play this video: ${String(e)}`);}).finally(()=>{starting=false;});}}else if(!element.paused)element.pause();
    };
    syncVideo.current=sync;
    element.addEventListener('loadedmetadata',sync);element.addEventListener('canplay',sync);
    if(path){element.src=path;sync();}else{if(!element.paused)element.pause();element.removeAttribute('src');element.load();}
    return()=>{active=false;syncVideo.current=null;element.removeEventListener('loadedmetadata',sync);element.removeEventListener('canplay',sync);if(!element.paused)element.pause();element.removeAttribute('src');element.load();};
  },[path,media.videos]);
  useEffect(()=>{syncVideo.current?.();},[position,playing,rate,error]);
  const timed=lines.filter(l=>l.ms!==null),index=timed.findLastIndex(l=>l.ms!<=position),line=timed[index];
  useEffect(()=>{lineBox.current?.querySelector('.on')?.scrollIntoView({block:'nearest'});},[index]);
  // Screen changes with a picture, the one playing now, and how far playback is through it (up to
  // the next change or the end of its part).
  const shown=media.moments.flatMap(m=>{const at=partAt(media.videos,m.ms);return at?[{m,at}]:[];});
  const active=current?shown.findLastIndex(({m})=>m.ms<=position):-1;
  const seen=active<0?0:(()=>{const {m,at}=shown[active];const end=Math.min(shown[active+1]?.m.ms??Infinity,at.part.offset_ms+at.part.duration_ms);return Math.min(1,Math.max(0,(position-m.ms)/Math.max(1,end-m.ms)));})();
  // The strip keeps the current screen in view without scrolling the page.
  const strip=useRef<HTMLDivElement>(null);
  useEffect(()=>{const box=strip.current,on=box?.querySelector<HTMLElement>('.vthumb.on');if(!box||!on)return;
    if(on.offsetLeft<box.scrollLeft||on.offsetLeft+on.offsetWidth>box.scrollLeft+box.clientWidth)box.scrollTo({left:on.offsetLeft-4,behavior:matchMedia('(prefers-reduced-motion: reduce)').matches?'auto':'smooth'});},[active]);
  const size=media.videos.reduce((n,p)=>n+p.bytes,0);
  // An imported file is also the meeting's audio source, so it goes with the audio, not on its own.
  const imported=media.videos.every(p=>p.kind==='file');
  async function remove(){
    setBusy(true);if(playing)togglePlay();const element=video.current;if(element){element.pause();element.removeAttribute('src');element.load();}
    try{await ipc.deleteVideo(meetingId);onMediaChange(await ipc.meetingMedia(meetingId));onToast?.(s.video_deleted,async()=>{await ipc.restoreVideo(meetingId);onMediaChange(await ipc.meetingMedia(meetingId));});}
    catch(e){onError(String(e));}finally{setBusy(false);}
  }
  return <><div className="vid"><div ref={frame} className="vframe"><div className="vscene"><video ref={video} muted playsInline preload="metadata" onError={()=>{if(video.current?.getAttribute('src'))setError('This video could not be played. The audio remains available.');}}/>{(!current||error)&&<div className="vgap" role="status">{error||s.video_gap}</div>}</div>{cc&&line&&<div className="vcap"><b className={line.speaker==='You'?'':'them'}>{line.speaker}</b>{line.text}</div>}<button type="button" className="vtoggle" aria-label={playing?'Pause video':'Play video'} onClick={togglePlay}>{!playing&&<span className="vbig"><Play/></span>}</button></div>
    <div className="vctl"><button type="button" className="iconbtn" aria-label={playing?'Pause':'Play'} onClick={togglePlay}>{playing?<svg viewBox="0 0 16 16" fill="currentColor" aria-hidden="true"><rect x="4" y="3" width="2.6" height="10" rx="1"/><rect x="9.4" y="3" width="2.6" height="10" rx="1"/></svg>:<Play/>}</button><span className="mono">{timeLabel(position)} / {timeLabel(media.duration_ms)}</span><div className="vscrub"><i style={{width:`${position/(media.duration_ms||1)*100}%`}}/>{media.videos.filter(p=>p.while_sharing).map(p=><b key={p.path} title="You shared your screen" style={{left:`${p.offset_ms/media.duration_ms*100}%`,width:`${p.duration_ms/media.duration_ms*100}%`}}/>)}{shown.map(({m})=><span key={m.ms} className="tick" style={{left:`${m.ms/(media.duration_ms||1)*100}%`}}/>)}<input type="range" aria-label="Seek video" min={0} max={media.duration_ms} value={position} onChange={e=>seek(Number(e.target.value))}/></div><select aria-label="Speed" value={rate} onChange={e=>setRate(Number(e.target.value))}>{[1,1.5,2].map(r=><option key={r} value={r}>{r}×</option>)}</select><button type="button" className="iconbtn cc" aria-pressed={cc} title="Show the transcript line on the video" onClick={()=>setCC(!cc)}>CC</button><button type="button" className="iconbtn" aria-label="Full screen" onClick={()=>void frame.current?.requestFullscreen().catch(e=>onError(String(e)))}><svg viewBox="0 0 16 16" fill="none" stroke="currentColor" aria-hidden="true"><path d="M6 2H2v4M10 2h4v4M2 10v4h4M14 10v4h-4"/></svg></button></div>
    <section className="vmom" aria-label="Screen changes"><h3>Screen changes <span className="n">{shown.length}</span><span className="tip">{s.video_changes}</span></h3><div ref={strip} className="vthumbs">{shown.map(({m,at},i)=><button type="button" key={m.ms} className={`vthumb${i===active?' on':''}`} aria-label={`Watch from ${timeLabel(m.ms)}${m.yours?', your screen':''}`} onClick={()=>seek(m.ms)}><div className="vscene"><Thumbnail part={at.part} ms={at.fileMs}/></div><span className="at">{timeLabel(m.ms)}</span>{m.yours&&<span className="yours">Yours</span>}{i===active&&<i className="seen" style={{width:`${seen*100}%`}}/>}</button>)}</div></section></div>
    <div className="vside"><div className="panel"><h5>Transcript</h5><div ref={lineBox} className="vlines">{timed.map((l,i)=><button type="button" key={i} className={`vline${i===index?' on':''}`} onClick={()=>seek(l.ms!)}><span className="ts">{l.time}</span><span><span className={`who ${l.speaker==='You'?'me':'them'}`}>{l.speaker}</span>{l.text}</span></button>)}</div></div><div className="panel"><h5>This video</h5><dl className="kv"><dt>{imported?'Imported':'Recorded'}</dt><dd>{[...new Set(media.videos.filter(p=>!p.while_sharing).map(p=>p.source))].join(', ')}</dd>{media.videos.some(p=>p.while_sharing)&&<><dt>While you shared</dt><dd>{[...new Set(media.videos.filter(p=>p.while_sharing).map(p=>p.source))].join(', ')}</dd></>}<dt>Picture</dt><dd>{[...new Set(media.videos.map(p=>`${p.width}×${p.height}`))].join(', ')}</dd><dt>Size</dt><dd>{fileSize(size)}</dd></dl>{imported?<p>Your imported file. It is kept as long as the audio (Settings › Privacy).</p>:<p>H.264 and AAC. It plays in any video app. {retention} Deleted video is recoverable for 30 days.</p>}<div className="field">{confirm&&!imported?<><span>{s.video_delete_confirm.replace('{size}',fileSize(size))}</span><button type="button" className="btn sm rec" disabled={busy} onClick={()=>void remove()}>Delete video</button><button type="button" className="btn sm ghost" disabled={busy} autoFocus onClick={()=>setConfirm(false)} onKeyDown={e=>{if(e.key==='Escape')setConfirm(false);}}>Cancel</button></>:<><button type="button" className="btn sm" onClick={()=>void ipc.meetingAction(meetingId,'reveal').catch(e=>onError(String(e)))}>Show in Finder</button>{!imported&&<button type="button" className="btn sm ghost" onClick={()=>setConfirm(true)}>Delete video</button>}</>}</div></div></div></>;
}
