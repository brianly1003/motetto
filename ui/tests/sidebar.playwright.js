// Sidebar visibility preserves the mounted page, playback and local preference; compare chrome to the reference.
async page => {
  const results=[];
  for(const theme of ['light','dark'])for(const os of ['mac','win']){
    const context=await page.context().browser().newContext({viewport:{width:1200,height:760},colorScheme:theme});
    await context.route('https://**/*',route=>route.abort());
    const p=await context.newPage(),errors=[];p.on('pageerror',e=>errors.push(e.message));
    await p.addInitScript(os=>{
      Object.defineProperty(navigator,'platform',{get:()=>os==='mac'?'MacIntel':'Win32'});
      const summary={id:'1',title:'Sidebar check',created_ms:Date.now(),status:'Ready',marks_ms:[],duration_ms:60000,notes_ms:1000,spellings:[],locked:false,title_source:'user'};
      const notes='### Overview\n'+Array.from({length:100},(_,i)=>`- Discussion point ${i}: keep the reader on this page.`).join('\n');
      let n=0;window.calls=[];window.playing=false;window.opened=0;
      window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener(){}};
      window.__TAURI_INTERNALS__={metadata:{currentWindow:{label:'main'},currentWebview:{windowLabel:'main',label:'main'}},convertFileSrc:x=>x,transformCallback:()=>++n,unregisterCallback(){},invoke:async(c,a)=>{
        if(c==='plugin:event|listen')return n;
        if(c==='get_state')return {state:{type:'Idle'},meeting_id:null,companion:{visible:false,size:'Pill'},elapsed_ms:0,quit_pending:false};
        if(c==='list_meetings')return [summary];
        if(c==='search_meetings')return [{meeting:summary,snippet:null}];
        if(c==='deleted_meetings'||c==='open_action_items')return [];
        if(c==='notification_permission')return true;
        if(c==='get_meeting')return {summary,notes_md:notes,transcript:'[00:00] Me: Synthetic example.\n[00:30] Them: Another line.',transcript_heard:'',has_video:false};
        if(c==='meeting_media')return {tracks:[{path:'fixture',name:'part-0000.mic.wav',speaker:'You',offset_ms:0,duration_ms:60000,bytes:100,waveform:[.2,.3]}],videos:[],moments:[],duration_ms:60000,folder:'fixture'};
        if(c==='playback_open'){window.opened++;return true;}
        if(c==='playback_set'){window.playing=a.playing;return a.atMs??1000;}
        if(c==='playback_position')return 1000;
        if(c==='scripts_list')return [{name:'Demo',modified_ms:1,bytes:20}];
        if(c==='script_read')return '# Demo (1m)\nKeep this text.';
        if(c==='get_settings')return {notes_folder:'/Users/example/Motetto',shortcuts:{T:'Ctrl+Alt+T'},appearance:'system',keep_audio:'forever',keep_video:'same_as_audio',prompter:{last_script:null}};
        window.calls.push(c);return null;
      }};
    },os);
    try{
      await p.goto('http://127.0.0.1:3014/main/');
      const toggle=p.getByRole('button',{name:'Hide sidebar',exact:true}),side=p.locator('#library-sidebar');
      await toggle.waitFor();
      if(await toggle.getAttribute('aria-expanded')!=='true')throw Error('Default is not expanded');
      await p.getByRole('button',{name:/Sidebar check/}).first().click();
      await p.getByRole('heading',{name:'Sidebar check',exact:true}).waitFor();
      const before=await p.locator('.workspace').boundingBox(),control=await toggle.boundingBox();
      await p.evaluate(()=>{const body=document.querySelector('.docbody');body.scrollTop=300;window.reader=body;});
      await toggle.click();await p.waitForTimeout(220);
      const reopen=p.getByRole('button',{name:'Show sidebar',exact:true});
      if(await side.evaluate(e=>!e.inert)||await side.getAttribute('aria-hidden')!=='true')throw Error('Hidden sidebar can take focus');
      const after=await p.locator('.workspace').boundingBox(),controlAfter=await reopen.boundingBox();
      if(Math.abs(after.width-before.width-300)>1)throw Error('Wrong collapsed width');
      if(control.x!==controlAfter.x||control.y!==controlAfter.y)throw Error('Toggle moved');
      if(!await p.evaluate(()=>window.reader===document.querySelector('.docbody')&&window.reader.scrollTop>0))throw Error('Reading position lost');
      await p.screenshot({path:`.playwright-out/sidebar-app-collapsed-${os}-${theme}.png`});
      const key=os==='mac'?'Meta+Backslash':'Control+Backslash';
      await p.keyboard.press(key);await p.waitForTimeout(220);
      await p.getByPlaceholder('Search notes and transcripts',{exact:true}).focus();
      await p.keyboard.press(key);
      if(!await reopen.evaluate(e=>e===document.activeElement))throw Error('Focus left in hidden navigation');
      await p.keyboard.press(os==='mac'?'Meta+k':'Control+k');
      await p.getByRole('dialog',{name:'Command palette'}).waitFor();
      await p.keyboard.press('Escape');
      await p.getByRole('tab',{name:'Transcript',exact:true}).click();
      await p.locator('.player button').click();
      await p.waitForFunction(()=>window.playing);
      const opened=await p.evaluate(()=>window.opened);
      await reopen.click();await p.keyboard.press(key);await p.waitForTimeout(220);
      if(!await p.evaluate(()=>window.playing)||await p.evaluate(()=>window.opened)!==opened)throw Error('Toggle interrupted playback');
      await p.reload();await p.waitForFunction(()=>document.querySelector('.shell')?.dataset.sidebar==='hidden');
      if(await side.evaluate(e=>!e.inert))throw Error('Reload did not restore hidden state');
      await p.getByRole('button',{name:'Show sidebar',exact:true}).click();await p.waitForTimeout(220);
      await p.getByRole('button',{name:/Scripts/}).first().click();
      await p.getByRole('textbox',{name:'Demo',exact:true}).waitFor({timeout:5000}).catch(async()=>{throw Error(JSON.stringify({errors,text:(await p.locator('body').innerText()).slice(-1000)}));});
      await p.evaluate(()=>{window.script=document.querySelector('.sp-editor textarea');});
      await p.getByRole('button',{name:'Hide sidebar',exact:true}).click();
      if(!await p.evaluate(()=>window.script===document.querySelector('.sp-editor textarea')&&window.script.value.includes('Keep this text')))throw Error('Script editor remounted');
      await p.emulateMedia({reducedMotion:'reduce'});await p.setViewportSize({width:760,height:520});
      if(await p.locator('.shell').evaluate(e=>getComputedStyle(e).transitionDuration)!=='0s')throw Error('Reduced motion ignored');
      await p.getByRole('button',{name:'Show sidebar',exact:true}).click();
      await p.getByRole('button',{name:'Hide sidebar',exact:true}).focus();await p.waitForTimeout(450);
      if(await p.getByRole('tooltip').textContent()!==`Hide sidebar${os==='mac'?'⌘\\':'Ctrl+\\'}`)throw Error('Tooltip differs');
      await p.keyboard.press('Escape');
      if(await p.getByRole('tooltip',{includeHidden:true}).evaluate(e=>getComputedStyle(e).visibility)!=='hidden')throw Error('Tooltip not dismissible');
      const ref=await context.newPage();await ref.goto('http://127.0.0.1:3001/motetto-app-mockup.html');
      await ref.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);
      await ref.locator(`button[data-os="${os}"]`).click();await ref.locator('button[data-go="notes"]').first().click();
      const keys=['width','height','paddingTop','paddingLeft','borderRadius','color','backgroundColor'];
      const styles=loc=>loc.evaluate((e,keys)=>Object.fromEntries(keys.map(k=>[k,getComputedStyle(e)[k]])),keys);
      await p.mouse.move(1,1);await p.getByRole('button',{name:'Hide sidebar',exact:true}).blur();
      const actual=await styles(p.locator('.sidebar-toggle')),expected=await styles(ref.locator('.mainwin .sidebar-toggle'));
      if(JSON.stringify(actual)!==JSON.stringify(expected))throw Error('Toggle differs from reference: '+JSON.stringify({actual,expected}));
      if(await p.locator('.sidebar-toggle svg').getAttribute('viewBox')!==await ref.locator('.mainwin .sidebar-toggle svg').getAttribute('viewBox'))throw Error('Icon differs');
      if(errors.length)throw Error(errors.join('\n'));
      if((await p.evaluate(()=>window.calls)).some(c=>['stop_recording','pause_recording','start_recording'].includes(c)))throw Error('Toggle issued recording command');
      results.push({theme,os,mouse:true,keyboard:true,persistence:true,scroll:true,playback:true,scripts:true,parity:true});
    }finally{await context.close();}
  }
  return results;
}
