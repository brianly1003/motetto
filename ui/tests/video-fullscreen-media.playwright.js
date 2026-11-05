// Generated local media: serve fullscreen.mp4 on loopback port 3002 with byte ranges.
// Create it with ffmpeg: -f lavfi -i testsrc2=size=640x400:rate=15 -t 22
// -c:v libx264 -pix_fmt yuv420p -movflags +faststart fullscreen.mp4.
async page => {
  const results=[];
  const assert=(ok,message)=>{if(!ok)throw Error(message);};
  for(const theme of ['light','dark']){
    const ctx=await page.context().browser().newContext({viewport:{width:1200,height:760},colorScheme:theme,reducedMotion:'reduce'});
    const p=await ctx.newPage(),errors=[];p.on('pageerror',e=>errors.push(e.message));
    await ctx.route('https://**/*',r=>r.abort());
    await p.addInitScript(()=>{
      let n=0;
      const summary={id:'1',title:'Fullscreen fixture',created_ms:1000,status:'Ready',marks_ms:[],duration_ms:90000,notes_ms:0,spellings:[],locked:false};
      window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener(){}};
      window.__TAURI_INTERNALS__={convertFileSrc:x=>x,transformCallback:()=>++n,unregisterCallback(){},invoke:async c=>{
        if(c==='plugin:event|listen')return n;
        if(c==='get_state')return {state:{type:'Idle'},companion:{visible:false,size:'Pill'},elapsed_ms:0,quit_pending:false};
        if(c==='list_meetings')return [summary];
        if(['deleted_meetings','open_action_items','scripts_list'].includes(c))return [];
        if(c==='get_meeting')return {summary,notes_md:'### Overview\nSynthetic fixture.',transcript:'[00:00] Me: First synthetic line.\n[00:30] Them: Second synthetic line.',transcript_heard:'',has_video:true};
        if(c==='meeting_media')return {tracks:[],videos:[{path:'http://127.0.0.1:3002/fullscreen.mp4',kind:'window',offset_ms:0,duration_ms:22000,source:'Window',while_sharing:false,width:1280,height:720,bytes:100}],moments:[],duration_ms:90000,folder:'fixture'};
        if(c==='get_settings')return {keep_audio:'forever',keep_video:'same_as_audio'};
        return null;
      }};
    });
    await p.goto('http://127.0.0.1:3000/main/');
    await p.getByRole('button',{name:/Fullscreen fixture/}).first().click();
    await p.getByRole('tab',{name:'Video',exact:true}).click();
    await p.locator('.vctl').waitFor();
    await p.getByRole('button',{name:'Full screen',exact:true}).click();
    await p.locator('.vplayer:fullscreen').waitFor();
    assert(await p.locator('.vplayer .vctl').isVisible(),'Playback bar missing in fullscreen');
    const time=()=>p.locator('.vctl .mono').innerText();
    await p.getByRole('button',{name:'Play video',exact:true}).click();
    await p.waitForFunction(()=>document.querySelector('.vscene video')?.readyState>=2);
    await p.getByRole('button',{name:'Pause video',exact:true}).click();
    await p.getByRole('button',{name:'Play video',exact:true}).focus();
    await p.keyboard.press('ArrowRight');assert((await time()).startsWith('00:10'),'Right arrow must seek 10 seconds');
    await p.waitForFunction(()=>Math.abs(document.querySelector('.vscene video').currentTime-10)<.3);
    await p.keyboard.press('ArrowLeft');assert((await time()).startsWith('00:00'),'Left arrow must seek 10 seconds');
    await p.keyboard.press('ArrowLeft');assert((await time()).startsWith('00:00'),'Backward seek must clamp at zero');
    await p.waitForFunction(()=>document.querySelector('.vscene video').currentTime<.3);
    await p.screenshot({path:`.playwright-out/video-fullscreen-media-${theme}.png`});
    assert(!errors.length,errors.join('\n'));
    results.push({theme,decoded:true,seekForward10:true,seekBack10:true,paused:true});
    await ctx.close();
  }
  return results;
}
