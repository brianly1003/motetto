async (page) => {
  const results=[];
  const ref=await page.context().newPage();ref.setDefaultTimeout(5000);await ref.route('https://**/*',r=>r.abort());
  await ref.goto('http://127.0.0.1:3001/motetto-app-mockup.html');
  await ref.getByRole('button',{name:'macOS',exact:true}).click();
  await ref.getByRole('button',{name:'After the call',exact:true}).click();
  await ref.getByRole('tab',{name:'Transcript',exact:true}).click();
  for(const theme of ['light','dark']) {
    const p=await page.context().newPage();p.setDefaultTimeout(5000);const errors=[];p.on('pageerror',e=>errors.push(e.message));
    await p.emulateMedia({colorScheme:theme});await ref.evaluate(t=>document.documentElement.dataset.theme=t,theme);
    await p.addInitScript(()=>{
      Object.defineProperty(navigator,'platform',{value:'MacIntel'});
      const summary={id:'123',title:'FCM vs Pusher Beams',created_ms:Date.now(),status:'Ready',marks_ms:[],duration_ms:87000,notes_ms:48000,spellings:[]};
      const meeting={summary,notes_md:'### Action items\n- [ ] Kevin: Check whether the 2021 Firebase project is still active [00:56]\n- [ ] You: Share the device table design [01:20]\n\n### Overview\n- Compared **FCM** with push beams for Android push in the IGB app.\n- Decided to **keep FCM** and build device management ourselves.\n\n### Cost\n- **FCM has no messaging fee.** [01:00]\n- See [the pricing page](https://example.com) later.\n\n<details><summary>Transcript</summary>\n\n[00:16] Me: hi\n\n</details>\n',transcript:'[00:16] Me: Pusher beams relies on FCM.\n[00:56] Them: Check the Firebase project.\n[01:00] Me: FCM has no messaging fee.\n[01:20] Me: I will share the table.',transcript_heard:''};
      const tracks=[{path:'/fixture0.wav',name:'mic.wav',speaker:'You',offset_ms:0,duration_ms:87000,bytes:2100000,waveform:Array(200).fill(.3)}];
      let next=0;window.testCalls=[];window.meeting=meeting;
      window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener:()=>{}};
      window.__TAURI_INTERNALS__={convertFileSrc:path=>path,transformCallback:()=>++next,unregisterCallback:()=>{},invoke:async(command,args)=>{
        window.testCalls.push({command,args});
        if(command==='plugin:event|listen')return next;
        if(command==='get_state')return {state:{type:'Idle'},meeting_id:null,companion:{visible:false,size:'Pill'},elapsed_ms:0,quit_pending:false};
        if(command==='notification_permission')return true;
        if(command==='list_meetings')return [summary];
        if(command==='deleted_meetings')return [];
        if(command==='get_meeting')return meeting;
        if(command==='meeting_media')return {tracks,duration_ms:87000,folder:'~/Documents/Motetto/123'};
        if(command==='save_notes'){ if(summary.locked)throw Error('These notes are locked. Unlock them to edit.'); if(args.expected!==meeting.notes_md)throw Error('Stale notes'); meeting.notes_md=args.markdown; return args.markdown; }
        if(command==='set_locked'){ summary.locked=args.locked; return {...summary}; }
        // As transcript_edits.rs: only the text after "[mm:ss] Speaker: " changes; heard text kept for Revert.
        if(command==='save_transcript_line'){
          if(summary.locked)throw Error('These notes are locked. Unlock them to edit.');
          const heard=meeting.transcript_heard||meeting.transcript, lines=meeting.transcript.split('\n');
          const i=lines.map((l,k)=>[l,k]).filter(([l])=>l.trim())[args.index][1];
          lines[i]=/^\[[^\]]*\] [^:]*: /.exec(lines[i])[0]+args.text; meeting.transcript=lines.join('\n');
          meeting.transcript_heard=meeting.transcript===heard?'':heard; return meeting.transcript;
        }
        return false;
      }};
    });
    await p.goto('http://127.0.0.1:3000/main/');
    await p.getByRole('button',{name:/FCM vs Pusher Beams/}).first().click();
    await p.getByRole('tab',{name:'Transcript',exact:true}).click();
    const props=['fontFamily','fontSize','fontWeight','lineHeight','color'];
    const read=(e,props)=>Object.fromEntries(props.map(k=>[k,getComputedStyle(e)[k]]));
    for(const width of [760,1200]){
      await p.setViewportSize({width,height:720});await ref.setViewportSize({width,height:1000});
      for(const selector of ['.tline .ts','.tline .who.me','.tline .who.them','.tline .words','.panel h5','.kv dt','.kv dd','.player .mono','.player .tip']) {
        if(!await p.locator(selector).count())throw Error('App missing '+selector+' '+await p.locator('.player').count());
        if(!await ref.locator(selector).count())throw Error('Mockup missing '+selector);
        const actual=await p.locator(selector).first().evaluate(read,props),expected=await ref.locator(selector).first().evaluate(read,props);
        if(JSON.stringify(actual)!==JSON.stringify(expected))throw Error(selector+' '+JSON.stringify({actual,expected}));
      }
      const words=p.locator('.tline .words').first(),row=p.locator('.tline').first();
      const before=await words.evaluate(read,props),height=(await row.boundingBox()).height;
      if(before.fontSize!=='14px'||before.lineHeight!=='22px')throw Error('Body type not normalized');
      await words.focus();
      if(JSON.stringify(before)!==JSON.stringify(await words.evaluate(read,props)))throw Error('Focus changes text typography');
      if((await row.boundingBox()).height!==height)throw Error('Focus moves rows');
      if(await p.locator('.tline .ts').first().evaluate(e=>getComputedStyle(e).color)!==await p.locator('.tline .who').first().evaluate(e=>getComputedStyle(e).color))throw Error('Editing timestamp lost speaker color');
      await p.keyboard.press('Escape');await p.mouse.move(0,0);
      const fits=await p.locator('.transcript-edit').evaluate(e=>e.scrollWidth<=e.clientWidth);
      if(!fits)throw Error('Transcript overflows narrow window');
      await p.locator('.docbody').screenshot({path:`.playwright-out/transcript-type-app-${theme}-${width}.png`});
      await ref.locator('.docbody').screenshot({path:`.playwright-out/transcript-type-mockup-${theme}-${width}.png`});
    }
    await p.locator('.tline .words').first().fill('Updated transcript text.');
    await p.keyboard.press('Escape');await p.locator('.edot').first().focus();
    const q=p.locator('.epop q').first();await q.waitFor();
    const quote=await q.evaluate(read,props);
    if(quote.fontSize!=='12px'||quote.lineHeight!=='18px'||quote.fontFamily.includes('Georgia'))throw Error('Heard text typography differs: '+JSON.stringify(quote));
    if(errors.length)throw Error(errors.join('\n'));
    await p.close();results.push(theme+' PASS');
  }
  await ref.close();return results;
}
