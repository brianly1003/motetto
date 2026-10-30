import {useEffect, useState, type Ref} from 'react';
import Duet from './MotettoMark';
import {strings as s} from '@/lib/strings';
import { KbdGroup } from "@/components/ui/Kbd";

export default function LibraryTitlebar({mac, hidden, onToggle, buttonRef}: {mac:boolean;hidden:boolean;onToggle:()=>void;buttonRef:Ref<HTMLButtonElement>}) {
  const [tipOff,setTipOff]=useState(false);
  useEffect(()=>{
    const dismiss=(event:KeyboardEvent)=>{if(event.key==='Escape')setTipOff(true);};
    document.addEventListener('keydown',dismiss);
    return()=>document.removeEventListener('keydown',dismiss);
  },[]);
  const label=hidden?s.sidebarShow:s.sidebarHide;
  return <header className="window-titlebar" data-platform={mac?'mac':'win'} data-tauri-drag-region={mac?'':undefined}>
    <div className="sidebar-control" data-tip-off={tipOff?'':undefined}>
      <button ref={buttonRef} type="button" className="sidebar-toggle" aria-controls="library-sidebar" aria-describedby="sidebar-tooltip" aria-expanded={!hidden} aria-label={label} aria-keyshortcuts={mac?'Meta+\\':'Control+\\'} onClick={onToggle} onPointerEnter={()=>setTipOff(false)} onFocus={()=>setTipOff(false)}>
        <svg viewBox="0 0 20 20" aria-hidden="true"><rect x="2.5" y="3" width="15" height="14" rx="2"/><path d="M7.5 3v14"/><path className="sidebar-pane" d="M4.5 3.8h2.3v12.4H4.5a1.2 1.2 0 0 1-1.2-1.2V5a1.2 1.2 0 0 1 1.2-1.2Z"/></svg>
      </button>
      <span className="sidebar-tip" role="tooltip" id="sidebar-tooltip"><span>{label}</span><KbdGroup keys={mac?'⌘\\':'Ctrl+\\'}/></span>
    </div>
    <span className="title-brand"><Duet className="mark"/><span>Motetto</span></span>
  </header>;
}
