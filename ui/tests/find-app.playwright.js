// Production document find: keyboard, highlights and unmodified editable content.
async (page) => {
 const results=[];
 for(const theme of ['light','dark']){
  const p=await page.context().newPage();p.setDefaultTimeout(7000);
  const errors=[];p.on('pageerror',e=>errors.push(e.message));
  const ref=await page.context().newPage();ref.setDefaultTimeout(7000);await ref.route('https://**/*',r=>r.abort());
  await ref.goto('http://127.0.0.1:3001/motetto-app-mockup.html');
  await ref.getByRole('button',{name:'macOS',exact:true}).click();await ref.getByRole('button',{name:'After the call',exact:true}).click();
  await ref.evaluate(t=>document.documentElement.dataset.theme=t,theme);

  await p.route('https://**/*',r=>r.abort());
  await p.emulateMedia({colorScheme:theme});
  await p.addInitScript(()=>{
   const summary={id:'find',title:'Find fixture',created_ms:Date.now(),status:'Ready',marks_ms:[],duration_ms:60000,notes_ms:5000,spellings:[],locked:false};
   const meeting={summary,notes_md:'### Overview\n- Decided to **keep FCM** and build device management ourselves.\n\n'+Array.from({length:20},(_,i)=>`### Topic ${i}\n- FCM item ${i}: a longer paragraph about the device table design and Android delivery.\n`).join('\n'),transcript:Array.from({length:24},(_,i)=>`[00:${String(i).padStart(2,'0')}] ${i%2?'Them':'Me'}: FCM item ${i}: a longer line about the device table design and Android delivery.`).join('\n'),transcript_heard:''};
   let next=0;window.saved=[];
   window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener:()=>{}};
   window.__TAURI_INTERNALS__={convertFileSrc:x=>x,transformCallback:()=>++next,unregisterCallback:()=>{},invoke:async(c,a)=>{
    if(c==='plugin:event|listen')return next;
    if(c==='get_state')return {state:{type:'Idle'},meeting_id:null,companion:{visible:false,size:'Pill'},elapsed_ms:0,quit_pending:false};
    if(c==='list_meetings')return [summary,{...summary,id:'other',title:'Other meeting'}];
    if(c==='get_meeting')return a.id==='find'?meeting:{...meeting,summary:{...summary,id:'other',title:'Other meeting'}};
    if(c==='open_action_items'||c==='deleted_meetings')return [];
    if(c==='meeting_media')return {tracks:[],duration_ms:60000,folder:'/local/Find fixture'};
    if(c==='save_transcript_line'){window.saved.push(c);const lines=meeting.transcript.split('\n');lines[a.index]=lines[a.index].replace(/: .*/,': '+a.text);meeting.transcript=lines.join('\n');return meeting.transcript;}
    if(c==='save_notes'){window.saved.push(c);meeting.notes_md=a.markdown;return meeting.notes_md;}
    return false;
   }};
  });
  await p.goto('http://127.0.0.1:3000/main/',{waitUntil:'domcontentloaded'});
  await p.getByRole('button',{name:/Find fixture/}).first().click();
  for(const width of [760,1200]){
   await p.setViewportSize({width,height:1000});

   for(const tab of ['Notes','Transcript']){
    await p.getByRole('tab',{name:tab,exact:true}).click();
    const scope=tab==='Notes'?'.docbody .notes':'.tlist';
    const content=()=>p.locator(scope).evaluate(e=>{const copy=e.cloneNode(true);copy.querySelectorAll('[style=""]').forEach(e=>e.removeAttribute('style'));return copy.innerHTML;});
    const before=await content();
    const topBefore=await p.locator('.docbody').evaluate(e=>e.getBoundingClientRect().top);
    await p.keyboard.press('Meta+f');
    const layout=await p.evaluate(()=>{const tabs=document.querySelector('.tabs').getBoundingClientRect(),find=document.querySelector('.findbar').getBoundingClientRect(),tools=document.querySelector('.doctools').getBoundingClientRect();return {tabs:{left:tabs.left,right:tabs.right,top:tabs.top,bottom:tabs.bottom},find:{left:find.left,right:find.right,top:find.top,bottom:find.bottom},right:tools.right,overflow:document.querySelector('.findbar').scrollWidth>document.querySelector('.findbar').clientWidth};});
    if(layout.find.right-layout.find.left>280.5)throw Error('Find is wider than the compact 280px limit');
    if(layout.find.left<layout.tabs.right||layout.find.top>=layout.tabs.bottom||layout.find.bottom<=layout.tabs.top||layout.find.right>layout.right||layout.overflow)throw Error('Find does not fit beside tabs: '+JSON.stringify(layout));
    if(await p.locator('.docbody').evaluate(e=>e.getBoundingClientRect().top)!==topBefore)throw Error('Opening find consumed vertical space');
    const input=p.getByRole('textbox',{name:'Find in '+tab.toLowerCase(),exact:true});
    await input.fill('FCM');
    await p.waitForFunction(()=>CSS.highlights.get('motetto-find')?.size>1);
    const count=await p.evaluate(()=>CSS.highlights.get('motetto-find').size);
    if(await p.locator('.findbar output').innerText()!==`${count} matches`)throw Error('Wrong initial count');
    await input.press('Enter');
    if(await p.locator('.findbar output').innerText()!==`1 of ${count}`)throw Error('Enter did not jump to first');
    await input.press('Shift+Enter');
    if(await p.locator('.findbar output').innerText()!==`${count} of ${count}`)throw Error('Previous did not wrap');
    const rect=await p.evaluate(()=>{const r=[...CSS.highlights.get('motetto-find-current')][0].getBoundingClientRect(),b=document.querySelector('.docbody').getBoundingClientRect();return {top:r.top,bottom:r.bottom,min:b.top,max:b.bottom};});
    if(rect.top<rect.min||rect.bottom>rect.max)throw Error('Active result not scrolled into view: '+JSON.stringify(rect));
    await p.getByRole('button',{name:'Next match',exact:true}).click();
    if(await p.locator('.findbar output').innerText()!==`1 of ${count}`)throw Error('Next button did not wrap');
    await input.fill('fcm');await input.press('Enter');
    if(await p.evaluate(()=>CSS.highlights.get('motetto-find').size)!==count)throw Error('Case-insensitive search changed count');
    await p.keyboard.press('Control+f');
    if(!await input.evaluate(e=>e===document.activeElement&&e.selectionEnd===e.value.length&&e.selectionStart===0))throw Error('Ctrl+F did not select query');
    await p.locator('.doctools').screenshot({path:`.playwright-out/find-app-${tab}-${theme}-${width}.png`});
    if(width===1200)await p.locator('.document').screenshot({path:`.playwright-out/find-app-document-${tab}-${theme}.png`});
    if(tab==='Notes'){
     await input.fill('keep FCM');await input.press('Enter');
     if(await p.locator('.findbar output').innerText()!=='1 of 1')throw Error('Phrase across inline formatting not found');
    }
    await input.fill('[not a regex]*');await input.press('Enter');
    if(await p.locator('.findbar output').innerText()!=='No matches')throw Error('No matches state missing');
    await ref.setViewportSize({width,height:1000});await ref.locator('.mainwin').evaluate((e,width)=>{e.style.width=(width+2)+'px';e.style.left='0';},width);
    await ref.getByRole('tab',{name:tab,exact:true}).click();await ref.keyboard.press('Meta+f');
    await ref.getByRole('textbox',{name:'Find in '+tab.toLowerCase(),exact:true}).fill('[not a regex]*');
    for(const selector of ['.findbar','.findbar input','.findbar output','.findbar button','.findbar > svg','.findbar button svg']){
     const styles=e=>{const s=getComputedStyle(e);return [s.fontSize,s.fontWeight,s.lineHeight,s.color,s.backgroundColor,s.borderRadius,s.padding,s.gap,s.boxShadow,s.strokeWidth];};
     const app=await p.locator(selector).first().evaluate(styles),mock=await ref.locator(selector).first().evaluate(styles);
     if(JSON.stringify(app)!==JSON.stringify(mock))throw Error('Find style mismatch '+selector+': '+JSON.stringify({app,mock}));
    }
    await ref.keyboard.press('Escape');

    if(!await p.getByRole('button',{name:'Next match',exact:true}).isDisabled())throw Error('No matches still navigable');
    await input.fill('');if(await p.locator('.findbar output').innerText()!=='')throw Error('Empty query should clear');
    await input.fill('FCM');await input.press('Enter');await input.press('Escape');
    if(await p.locator('.findbar').count()||await p.evaluate(()=>CSS.highlights.has('motetto-find')))throw Error('Escape left find visible');
    await p.keyboard.press('Meta+f');
    if(await input.inputValue()!=='')throw Error('Escape retained the search query');
    await input.fill('FCM');await p.getByRole('button',{name:'Close find',exact:true}).click();
    await p.keyboard.press('Meta+f');
    if(await input.inputValue()!=='')throw Error('Close button retained the search query');
    await input.press('Escape');
    const after=await content();if(after!==before){let i=0;while(before[i]===after[i])i++;throw Error('Find changed editable content '+tab+' at '+i+': '+before.slice(i-60,i+180)+' -> '+after.slice(i-60,i+180));}
   }
  }
  await p.keyboard.press('Meta+f');await p.getByRole('textbox',{name:'Find in transcript',exact:true}).fill('FCM');
  await p.getByRole('tab',{name:'Audio',exact:true}).click();
  if(await p.locator('.findbar').count()||await p.evaluate(()=>CSS.highlights.has('motetto-find')))throw Error('Tab switch left stale highlights');
  await p.getByRole('tab',{name:'Notes',exact:true}).click();await p.keyboard.press('Meta+f');
  await p.getByRole('button',{name:'Close find',exact:true}).click();
  if(await p.locator('.findbar').count())throw Error('Close button failed');
  await p.keyboard.press('Meta+f');await p.locator('.findbar input').fill('FCM');
  await p.getByRole('button',{name:/Other meeting/}).first().click();
  await p.getByRole('heading',{name:'Other meeting',exact:true}).waitFor();
  if(await p.locator('.findbar').count()||await p.evaluate(()=>CSS.highlights.has('motetto-find')))throw Error('Meeting switch left stale search');
  await p.keyboard.press('Meta+f');if(await p.locator('.findbar input').inputValue()!=='')throw Error('Meeting switch retained query');
  await p.locator('.findbar input').fill('FCM');await p.locator('.homerow').click();
  if(await p.locator('.findbar').count()||await p.evaluate(()=>CSS.highlights.has('motetto-find')))throw Error('Action items left stale search');
  await p.getByRole('button',{name:/Other meeting/}).first().click();await p.keyboard.press('Meta+f');
  if(await p.locator('.findbar input').inputValue()!=='')throw Error('Action items retained query');
  await p.keyboard.press('Escape');
  if(await p.evaluate(()=>window.saved.length))throw Error('Searching saved document edits');
  await p.getByRole('tab',{name:'Transcript',exact:true}).click();await p.keyboard.press('Meta+f');
  const query=p.getByRole('textbox',{name:'Find in transcript',exact:true});await query.fill('FCM');
  await p.locator('.tline .words').first().fill('A changed phrase');
  await query.focus();
  await p.waitForFunction(()=>CSS.highlights.get('motetto-find')?.size===23);
  await query.fill('changed phrase');await query.press('Enter');
  if(await p.locator('.findbar output').innerText()!=='1 of 1')throw Error('Search did not refresh after editing');
  if(await p.evaluate(()=>window.saved.length)!==1)throw Error('Editing did not save exactly once');

  if(errors.length)throw Error(errors.join('\n'));
  await p.close();await ref.close();results.push(theme+' PASS');
 }
 return results;
}
