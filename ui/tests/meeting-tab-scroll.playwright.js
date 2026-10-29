// Tabs share a panel, but Notes must not inherit the transcript's playback scroll position.
async page => {
  const browser = await page.context().browser().browserType().launch({channel:'chrome'});
  const context = await browser.newContext();
  const errors = [];
  try {
    for (const theme of ['light','dark']) {
      const p = await context.newPage();
      p.on('pageerror', e => errors.push(e.message));
      await p.setViewportSize({width:1200,height:720});
      await p.emulateMedia({colorScheme:theme});
      await p.addInitScript(() => {
        const time = s => `${String(Math.floor(s/60)).padStart(2,'0')}:${String(s%60).padStart(2,'0')}`;
        const summary = {id:'1',title:'Tab scroll check',created_ms:Date.now(),status:'Ready',marks_ms:[],duration_ms:300000,notes_ms:1000,spellings:[],locked:false};
        const meeting = {summary,transcript:Array.from({length:240},(_,i)=>`[${time(i)}] Them: Transcript line ${i}`).join('\n'),transcript_heard:'',notes_md:'### Action items\n- None\n\n### Overview\n- Notes start here.\n\n### Discussion\n'+Array.from({length:90},(_,i)=>`- Discussion point ${i}.`).join('\n')};
        let next=0,position=180000,playing=false,last=Date.now();
        window.playbackCalls=[];
        window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener:()=>{}};
        window.__TAURI_INTERNALS__={convertFileSrc:x=>x,transformCallback:()=>++next,unregisterCallback:()=>{},invoke:async(c,a)=>{
          if(c==='plugin:event|listen')return next;
          if(c==='get_state')return {state:{type:'Idle'},meeting_id:null,companion:{visible:false,size:'Pill'},elapsed_ms:0,quit_pending:false};
          if(c==='list_meetings')return [summary];
          if(c==='deleted_meetings'||c==='open_action_items')return [];
          if(c==='get_meeting')return meeting;
          if(c==='meeting_media')return {tracks:[{name:'system.wav',path:'fixture',speaker:'Them',offset_ms:0,duration_ms:300000,bytes:100,waveform:[.2,.8,.4]}],duration_ms:300000,folder:'fixture'};
          if(c==='notification_permission')return true;
          if(c==='playback_open')return true;
          if(c==='playback_set'){window.playbackCalls.push(a);playing=a.playing;if(a.atMs!==null)position=a.atMs;last=Date.now();return position;}
          if(c==='playback_position'){const now=Date.now();if(playing)position+=now-last;last=now;return position;}
          return null;
        }};
      });
      await p.goto('http://127.0.0.1:3014/main/');
      await p.getByRole('button',{name:/Tab scroll check/}).first().click();
      await p.getByRole('tab',{name:'Transcript',exact:true}).click();
      await p.locator('.tline[data-line="180"] button.ts').click();
      await p.getByRole('button',{name:'❚❚ Pause',exact:true}).waitFor();
      await p.waitForFunction(()=>document.querySelector('.docbody').scrollTop>500);
      const before=await p.locator('.player .mono').innerText();
      await p.getByRole('tab',{name:'Notes',exact:true}).click();
      const panel=p.locator('.docbody');
      if(await panel.evaluate(e=>e.scrollTop)!==0)throw Error(`${theme}: Notes inherited the transcript scroll offset`);
      await p.waitForTimeout(350);
      if(await panel.evaluate(e=>e.scrollTop)!==0)throw Error(`${theme}: playback moved Notes after switching`);
      await p.screenshot({path:`.playwright-out/meeting-notes-top-${theme}.png`});
      await panel.evaluate(e=>{e.scrollTop=500;});
      await p.waitForTimeout(1100);
      if(await panel.evaluate(e=>e.scrollTop)!==500)throw Error(`${theme}: playback reset manual Notes scrolling`);
      if(await p.locator('.player .mono').innerText()===before)throw Error('Playback stopped when switching tabs');
      if(await p.evaluate(()=>window.playbackCalls.some(call=>!call.playing)))throw Error('Tab switch paused native playback');
      await p.getByRole('tab',{name:'Notes',exact:true}).focus();
      await p.keyboard.press('ArrowRight');
      await p.waitForFunction(()=>document.querySelector('.docbody').scrollTop>500);
      await p.keyboard.press('ArrowLeft');
      if(await panel.evaluate(e=>e.scrollTop)!==0)throw Error(`${theme}: keyboard tab switch did not reset Notes`);
      await p.setViewportSize({width:760,height:520});
      await p.getByRole('tab',{name:'Transcript',exact:true}).click();
      await p.waitForFunction(()=>document.querySelector('.docbody').scrollTop>500);
      await p.getByRole('tab',{name:'Notes',exact:true}).click();
      if(await panel.evaluate(e=>e.scrollTop)!==0)throw Error(`${theme}: Notes not at top at minimum size`);
      const ref=await context.newPage();
      await ref.route('https://**/*',route=>route.abort());
      await ref.emulateMedia({colorScheme:theme});
      await ref.setViewportSize({width:1400,height:1000});
      await ref.goto('http://127.0.0.1:3001/motetto-app-mockup.html');
      await ref.getByRole('button',{name:'After the call',exact:true}).click();
      await ref.locator('.mainwin').evaluate(e=>{e.style.height='520px';});
      await ref.getByRole('tab',{name:'Transcript',exact:true}).click();
      await ref.locator('.docbody').evaluate(e=>{e.scrollTop=e.scrollHeight;});
      if(await ref.locator('.docbody').evaluate(e=>e.scrollTop)<=0)throw Error('Reference transcript did not scroll');
      await ref.getByRole('tab',{name:'Notes',exact:true}).click();
      if(await ref.locator('.docbody').evaluate(e=>e.scrollTop)!==0)throw Error('Reference Notes did not start at top');
      await ref.locator('.mainwin').screenshot({path:`.playwright-out/meeting-notes-top-reference-${theme}.png`});
      await ref.close();
      await p.close();
    }
    if(errors.length)throw Error(errors.join('\n'));
    return 'PASS: Notes opens at top, playback continues, Transcript follows, mouse/keyboard, both themes and minimum size';
  } finally {await browser.close();}
}
