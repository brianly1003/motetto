// Static app on 3014, reference on 3001. Video availability comes with the document; hold media replies to catch first-render tab shifts.
async page => {
  const results = [];
  for (const theme of ['light', 'dark']) {
    const p = await page.context().newPage(); const errors = [];
    p.on('pageerror', error => errors.push(error.message));
    await p.emulateMedia({colorScheme:theme});
    await p.setViewportSize({width:1200,height:760});
    await p.addInitScript(() => {
      const summaries = ['Screen recording','Audio recording'].map((title,i)=>({id:String(i+1),title,created_ms:1000+i,status:'Ready',marks_ms:[],duration_ms:60000,notes_ms:1000,spellings:[],locked:false,title_source:'user'}));
      let n=0; const replies={}; window.tabSnapshots=[];
      window.resolveMedia=(id,fail=false,empty=false)=>fail?replies[id].reject('Media unavailable'):replies[id].resolve({duration_ms:60000,folder:'fixture',moments:[],tracks:[],videos:id==='1'&&!empty?[{path:'',offset_ms:0,duration_ms:60000,source:'Window',while_sharing:false,width:1280,height:720,bytes:100}]:[]});
      new MutationObserver(()=>{const tabs=[...document.querySelectorAll('[role=tab]')].map(e=>e.textContent);if(tabs.length)window.tabSnapshots.push(tabs.join('|'));}).observe(document,{childList:true,subtree:true});
      window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener(){}};
      window.__TAURI_INTERNALS__={convertFileSrc:x=>x,transformCallback:()=>++n,unregisterCallback(){},invoke:async(c,a)=>{
        if(c==='plugin:event|listen')return n;
        if(c==='get_state')return {state:{type:'Idle'},meeting_id:null,companion:{visible:false,size:'Pill'},elapsed_ms:0,quit_pending:false};
        if(c==='list_meetings')return summaries;
        if(c==='deleted_meetings'||c==='open_action_items')return [];
        if(c==='get_meeting')return {summary:summaries.find(s=>s.id===a.id),notes_md:'### Overview\n- The notes are ready.',transcript:'[00:00] Me: Synthetic example.',transcript_heard:'',has_video:a.id==='1'};
        if(c==='meeting_media')return new Promise((resolve,reject)=>{replies[a.id]={resolve,reject};});
        if(c==='get_settings')return {keep_audio:'forever',keep_video:'same_as_audio'};
        if(c==='scripts_list')return [];
        return null;
      }};
    });
    await p.goto('http://127.0.0.1:3014/main/');
    await p.getByRole('button',{name:/Screen recording/}).first().click();
    await p.getByRole('tab',{name:'Video',exact:true}).waitFor();
    const expected='Notes|Transcript|Video|Audio';
    if((await p.evaluate(()=>window.tabSnapshots)).some(v=>v!==expected))throw Error('Tabs changed while media was pending');
    const ref=await page.context().newPage();
    await ref.setViewportSize({width:1200,height:760});
    await ref.route('https://**/*',route=>route.abort());
    await ref.goto('http://127.0.0.1:3001/motetto-app-mockup.html');
    await ref.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);
    await ref.locator('button[data-go="notes"]').first().click();
    if((await ref.locator('#doc .tabs button').allTextContents()).join('|')!==expected)throw Error('Tab order differs from reference');
    const styles=elements=>elements.map(e=>{const s=getComputedStyle(e);return ['fontSize','fontWeight','color','backgroundColor','paddingTop','paddingBottom','borderBottomWidth','borderBottomColor'].map(k=>s[k]);});
    if(JSON.stringify(await p.getByRole('tab').evaluateAll(styles))!==JSON.stringify(await ref.locator('#doc .tabs button').evaluateAll(styles)))throw Error('Tab styling differs from reference');
    await ref.locator('#doc .tabs').screenshot({path:`.playwright-out/video-tabs-reference-${theme}.png`});
    await ref.close();
    await p.getByRole('tab',{name:'Transcript',exact:true}).click();
    await p.getByRole('tab',{name:'Transcript',exact:true}).press('ArrowRight');
    await p.getByRole('tabpanel').getByText('Loading video…',{exact:true}).waitFor();
    await p.screenshot({path:`.playwright-out/video-tab-loading-${theme}.png`});
    await p.setViewportSize({width:760,height:520});
    if(await p.getByRole('tab').evaluateAll(tabs=>tabs.some(e=>e.getBoundingClientRect().right>innerWidth)))throw Error('Tabs overflow at minimum size');
    await p.setViewportSize({width:1200,height:760});
    await p.evaluate(()=>window.resolveMedia('1'));
    await p.locator('.vid').waitFor();
    if((await p.getByRole('tab').allTextContents()).join('|')!==expected)throw Error('Tabs changed when media loaded');
    await p.getByRole('button',{name:/Audio recording/}).first().click();
    await p.getByRole('heading',{name:'Audio recording',exact:true}).waitFor();
    if((await p.getByRole('tab').allTextContents()).join('|')!=='Notes|Transcript|Audio')throw Error('Audio-only recording has Video tab');
    await p.evaluate(()=>window.resolveMedia('2'));
    await p.getByRole('button',{name:/Screen recording/}).first().click();
    await p.getByRole('tab',{name:'Video',exact:true}).click();
    await p.evaluate(()=>window.resolveMedia('1',false,true));
    await p.waitForFunction(()=>document.querySelector('[role=tab][aria-selected=true]')?.textContent==='Audio');
    if(await p.getByRole('tab',{name:'Video',exact:true}).count())throw Error('Removed video is still offered');
    await p.getByRole('button',{name:/Audio recording/}).first().click();
    await p.getByRole('heading',{name:'Audio recording',exact:true}).waitFor();
    await p.evaluate(()=>window.resolveMedia('2'));
    await p.getByRole('button',{name:/Screen recording/}).first().click();
    await p.getByRole('tab',{name:'Video',exact:true}).click();
    await p.evaluate(()=>window.resolveMedia('1',true));
    await p.getByRole('tabpanel').getByText('Video could not be loaded.',{exact:true}).waitFor();
    await p.getByRole('tab',{name:'Notes',exact:true}).click();
    await p.getByText('The notes are ready.',{exact:true}).waitFor();
    if(errors.length)throw Error(errors.join('\n'));
    results.push({theme,initialTabs:expected,audioOnly:true,loading:true,errorRecovery:true});
    await p.close();
  }
  return results;
}
