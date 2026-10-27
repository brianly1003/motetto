"use client";
import {useEffect} from 'react';
import * as ipc from '@/lib/ipc';

export default function Appearance(){
  useEffect(()=>{
    if(!ipc.desktopAvailable())return;
    let active=true,off:(()=>void)|undefined;
    const refresh=async()=>{
      const {appearance}=await ipc.getSettings();
      if(!active)return;
      if(appearance==='light'||appearance==='dark')document.documentElement.dataset.theme=appearance;
      else delete document.documentElement.dataset.theme;
      try{localStorage.setItem('motetto-appearance',appearance??'system');}catch{}
    };
    void ipc.subscribe('motetto://settings-changed',()=>void refresh().catch(()=>{})).then(cleanup=>{
      if(active){off=cleanup;void refresh().catch(()=>{});}else cleanup();
    }).catch(()=>{});
    return()=>{active=false;off?.();};
  },[]);
  return null;
}
