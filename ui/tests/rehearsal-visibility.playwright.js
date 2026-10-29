// Rehearsal keeps privacy, microphone status, Stop and reading controls usable at every size.
async page => {
  const results=[];
  for(const theme of ['light','dark'])for(const [width,size,layout] of [[560,22,'Strip'],[300,22,'Column'],[560,40,'Strip']]){
    const p=await page.context().newPage(),errors=[];p.on('pageerror',e=>errors.push(e.message));
    await p.emulateMedia({colorScheme:theme,reducedMotion:'reduce'});
    await p.setViewportSize({width,height:layout==='Column'?600:204});
    await p.addInitScript(({size,layout})=>{
      let n=0;const callbacks={},listeners={};window.calls=[];
      window.state={state:{type:'Idle'},share_visible:false,meeting_id:null,elapsed_ms:0,companion:{visible:false,size:'Pill'}};
      window.emit=(event,payload)=>(listeners[event]||[]).forEach(k=>callbacks[k]?.({event,payload}));
      window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener(){}};
      window.__TAURI_INTERNALS__={metadata:{currentWindow:{label:'prompter'},currentWebview:{windowLabel:'prompter',label:'prompter'}},convertFileSrc:x=>x,transformCallback:f=>{callbacks[++n]=f;return n;},unregisterCallback(){},invoke:async(c,a)=>{
        if(c==='plugin:event|listen'){(listeners[a.event]||=[]).push(a.handler);return a.handler;}
        if(c==='get_state')return window.state;
        if(c==='prompter_state')return {script:'Demo',layout,check:'hidden',editing:false,rehearsing:true};
        if(c==='get_settings')return {prompter:{layout,text_size:size,last_script:'Demo'},shortcuts:{Down:'Ctrl+Alt+Down',Up:'Ctrl+Alt+Up'}};
        if(c==='scripts_list')return [{name:'Demo',modified_ms:1,bytes:20}];
        if(c==='script_read')return '# Opening (1m)\n'+Array(14).fill('A clear presentation gives every idea enough room to breathe.').join(' ')+'\n\nNow move to the next idea.';
        if(c==='prompter_controls'){window.rects=a.rects;return;}
        if(c==='set_share_visible'){
          window.calls.push([c,a.on]);if(window.failShare)throw 'Visibility could not be changed.';
          window.state={...window.state,share_visible:a.on};window.emit('motetto://event',{type:'StateChanged',...window.state});
          window.emit('motetto://prompter-check',a.on?'shown':'hidden');return;
        }
        window.calls.push([c]);return null;
      }};
    },{size,layout});
    try{
      await p.goto('http://127.0.0.1:3014/prompter/');
      const control=p.getByRole('switch');await control.waitFor();
      await p.locator('.pttext').waitFor();
      if(await control.getAttribute('aria-checked')!=='false')throw Error('Initial hidden state missing');
      await control.click();await p.waitForFunction(()=>document.querySelector('[role=switch]').getAttribute('aria-checked')==='true');
      if(!await p.locator('.pt').evaluate(e=>e.classList.contains('shown')))throw Error('Shown border missing');
      await control.focus();await p.keyboard.press('Space');
      await p.waitForFunction(()=>document.querySelector('[role=switch]').getAttribute('aria-checked')==='false');
      if((await p.evaluate(()=>window.calls)).some(c=>['rehearse_stop','stop_recording','pause_recording'].includes(c[0])))throw Error('Privacy switch stopped capture');
      if(!await p.locator('.ptreh').isVisible()||!await p.locator('.ptreh button').isVisible())throw Error('Mic announcement or Stop hidden');
      const overflow=await p.locator('.pth').evaluate(e=>[...e.querySelectorAll('[data-control]')].filter(c=>{const r=c.getBoundingClientRect();return r.left<0||r.right>innerWidth+1||r.bottom>innerHeight;}).map(e=>e.className));
      if(overflow.length)throw Error('Clipped control: '+overflow);
      const a=await control.boundingBox(),b=await p.locator('.ptreh button').boundingBox();
      if(a.x<b.x+b.width&&a.x+a.width>b.x&&a.y<b.y+b.height&&a.y+a.height>b.y)throw Error('Stop overlaps privacy switch');
      const footer=await p.locator('.ptfoot').boundingBox();if(footer.y+footer.height>(layout==='Column'?600:204)+1)throw Error('Reading controls clipped '+JSON.stringify({width,size,footer,parts:await p.locator('.pt').evaluate(e=>[...e.querySelectorAll('.pth,.ptlines,.ptln,.ptfoot')].map(e=>({cls:e.className,height:e.getBoundingClientRect().height,top:e.getBoundingClientRect().top,display:getComputedStyle(e).display})))}));
      await p.waitForFunction(()=>{const r=document.querySelector('[role=switch]').getBoundingClientRect();return window.rects?.some(([x,y,w,h])=>r.x+r.width/2>=x&&r.x+r.width/2<=x+w&&r.y+r.height/2>=y&&r.y+r.height/2<=y+h);});
      await p.screenshot({path:`.playwright-out/rehearsal-visibility-${theme}-${width}-${size}.png`});
      await p.evaluate(()=>window.failShare=true);await control.click();
      await p.getByRole('alert').filter({hasText:'Visibility could not be changed.'}).waitFor();
      if(await control.getAttribute('aria-checked')!=='false')throw Error('Failed toggle falsely says Shown');
      await p.locator('.ptreh button').click();
      if(!(await p.evaluate(()=>window.calls)).some(c=>c[0]==='rehearse_stop'))throw Error('Stop stopped working');
      if(errors.length)throw Error(errors.join('\n'));
      results.push({theme,width,size,mouse:true,keyboard:true,stop:true,noOverflow:true});
    }finally{await p.close();}
  }
  return results;
}
