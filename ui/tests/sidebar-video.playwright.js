// Saved video availability is supplied with list/search results, without loading media previews.
async page=>{
  const results=[];
  const assert=(ok,message)=>{if(!ok)throw Error(message);};
  for(const theme of ['light','dark']){
    const ctx=await page.context().browser().newContext({viewport:{width:1200,height:900},colorScheme:theme});
    await ctx.route('https://**/*',r=>r.abort());
    const p=await ctx.newPage(),errors=[];p.on('pageerror',e=>errors.push(e.message));
    await p.addInitScript(()=>{
      let n=0;const callbacks={},listeners={};
      const meetings=['Video meeting','Audio meeting','Missing video meeting'].map((title,i)=>({id:String(i+1),title,created_ms:Date.now()-i,status:'Ready',marks_ms:[],duration_ms:87000,notes_ms:0,spellings:[],locked:false,has_video:i===0}));
      window.fixtureCalls=[];
      window.setVideoAvailable=on=>{meetings[0].has_video=on;const handler=listeners['motetto://library-changed'];if(handler)callbacks[handler]({event:'motetto://library-changed',id:handler,payload:null});};
      window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener(){}};
      window.__TAURI_INTERNALS__={convertFileSrc:x=>x,transformCallback:cb=>{callbacks[++n]=cb;return n;},unregisterCallback:id=>delete callbacks[id],invoke:async(c,a)=>{
        window.fixtureCalls.push(c);
        if(c==='plugin:event|listen'){listeners[a.event]=a.handler;return a.handler;}
        if(c==='get_state')return {state:{type:'Idle'},companion:{visible:false,size:'Pill'},elapsed_ms:0,quit_pending:false};
        if(c==='list_meetings')return structuredClone(meetings);
        if(c==='search_meetings')return structuredClone(meetings.filter(m=>m.title.toLowerCase().includes(a.query.toLowerCase())).map(meeting=>({meeting,snippet:null})));
        if(c==='get_meeting'){const summary=meetings.find(m=>m.id===a.id);return {summary:structuredClone(summary),notes_md:'### Overview\nSynthetic fixture.',transcript:'',transcript_heard:'',has_video:summary.has_video};}
        if(c==='meeting_media')return {tracks:[],videos:[],moments:[],duration_ms:87000,folder:'fixture'};
        if(['deleted_meetings','open_action_items','scripts_list'].includes(c))return [];
        if(c==='notification_permission')return true;
        if(c==='get_settings')return {keep_audio:'forever',keep_video:'same_as_audio'};
        return null;
      }};
    });
    await p.goto('http://127.0.0.1:3000/main/');
    await p.locator('.meeting-row').first().waitFor();
    const video=p.getByRole('button',{name:/^Video meeting/});
    assert(await video.getByRole('img',{name:'Video available'}).count()===1,'Saved video icon missing');
    assert(await p.locator('.meeting-list .media-video').count()===1,'Audio/missing video incorrectly marked');
    assert(await video.locator('.media-video').innerText()==='','Visible Video text remains');
    assert(await p.evaluate(()=>!window.fixtureCalls.includes('meeting_media')),'List fetched media previews');
    const ref=await ctx.newPage();await ref.goto('http://127.0.0.1:3001/motetto-app-mockup.html');await ref.evaluate(t=>document.documentElement.dataset.theme=t,theme);await ref.locator('button[data-go="notes"]').first().click();
    const properties=['color','width','height','display'];
    const styles=await video.locator('.media-video').evaluate((e,keys)=>keys.map(k=>getComputedStyle(e)[k]),properties);
    const expected=await ref.locator('.mitem[data-id="fcm"] .media-video').evaluate((e,keys)=>keys.map(k=>getComputedStyle(e)[k]),properties);
    assert(JSON.stringify(styles)===JSON.stringify(expected),'Indicator differs from mockup');
    await ref.close();
    await video.focus();await video.hover();
    await p.locator('.sidebar').screenshot({path:`.playwright-out/sidebar-video-app-${theme}.png`});
    await p.setViewportSize({width:760,height:520});
    assert(await video.evaluate(e=>{const a=e.getBoundingClientRect(),b=e.querySelector('.media-video').getBoundingClientRect();return b.left>=a.left&&b.right<=a.right;}),'Indicator overflows narrow sidebar');
    const search=p.getByPlaceholder('Search notes and transcripts');await search.fill('Video meeting');
    await p.locator('.hit .media-video').waitFor();
    await p.evaluate(()=>window.setVideoAvailable(false));
    await p.waitForFunction(()=>!document.querySelector('.meeting-list .media-video'));
    await p.evaluate(()=>window.setVideoAvailable(true));await p.locator('.hit .media-video').waitFor();
    await search.fill('');await p.locator('.mflags .media-video').waitFor();
    assert(!errors.length,errors.join('\n'));
    results.push({theme,availability:true,noMediaPreviews:true,search:true,refresh:true,parity:true,narrow:true});
    await ctx.close();
  }
  return results;
}
