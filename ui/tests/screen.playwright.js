// Browser fixtures for screen choices, playback and settings. Capture and native windows are separate checks.
async (page) => {
  const results=[];
  const fixture=({label,kind})=>{
    const callbacks={},listeners={};let n=0;
    window.fixtureMedia={rateWrites:0,videoPlays:0};
    const rate=Object.getOwnPropertyDescriptor(HTMLMediaElement.prototype,'playbackRate');
    Object.defineProperty(HTMLMediaElement.prototype,'playbackRate',{...rate,set(value){window.fixtureMedia.rateWrites++;rate.set.call(this,value);}});
    const play=HTMLMediaElement.prototype.play;
    HTMLMediaElement.prototype.play=function(){if(this.tagName==='VIDEO')window.fixtureMedia.videoPlays++;return play.call(this);};
    const windowPick={choice:{kind:'CallWindow',app_id:'zoom'},name:'Zoom window'},displayPick={choice:{kind:'Display',id:1},name:'Built-in Retina Display'};
    const options=[{pick:windowPick,sub:'Only the call, with whatever people share. Recommended.'},{pick:displayPick,sub:'Everything on it, including notifications and other apps.'}];
    const settings={version:1,notes_folder:'Fixture',open_at_login:false,notes_language:'same_as_call',ask_on_call:true,app_modes:{},apps:[],idle_stop_s:30,discard_short_s:20,speech_model:'',notes_model:'',onboarding_done:true,onboarding_step:4,consent_confirmed_at:'2026-10-27',tier:null,speed_test:null,live_transcription:true,slow_question:null,screen_requested:false,adopted:{},speech_path:'',vad_path:'',notes_path:'',call_language:'detect',part_chars:24000,keep_audio:'30_days',keep_video:'same_as_audio',screen_offer_on_ask:true,screen_source:'call_window',screen_on_own_share:'ask',screen_preset:'sharp',screen_cursor:true,recording_reminder:true,mic_device:null,sounds:true,companion:{default_size:'pill',captions:true},spellings:[],shortcuts:{}};
    const summary={id:'1',title:'Screen fixture',created_ms:Date.now(),status:kind==='ready'?'Ready':'Recording',marks_ms:[],duration_ms:90000,notes_ms:null,spellings:[],title_source:'user',language:'en',notes_language:null};
    const text='[00:00] Them: First synthetic sentence.\n[00:30] Me: Second synthetic sentence.\n[01:12] Them: Last synthetic sentence.';
    let state={state:kind==='ask'?{type:'Asking',call_id:'c1',app:'Zoom'}:kind==='ready'?{type:'Idle'}:{type:'Recording'},meeting_id:kind==='ready'?null:'1',app:'Zoom',elapsed_ms:74000,started_by:{type:'App',app:'Zoom'},ask_on_call:true,problem:null,more_problems:0,quit_pending:false,dnd_active:false,onboarding_done:true,sharing:null,screen:null,share_video_offer:false,ask_off_pending:null,stops_at_ms:null,discarded:null,companion:{visible:true,size:kind==='ask'?'Ask':'Pill',pinned:false,peeking:false,display_id:''}};
    const video={path:'http://127.0.0.1:3002/screen.mp4',offset_ms:0,duration_ms:15000,source:'Zoom window',while_sharing:false,width:320,height:200,bytes:50000};
    let deleted=false;
    const emit=()=>{const h=listeners['motetto://event'];if(h)callbacks[h]({event:'motetto://event',id:0,payload:{type:'StateChanged',...state}});};
    window.fixtureState=patch=>{state={...state,...patch};emit();};window.fixtureCalls=[];
    window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener:()=>{}};
    window.__TAURI_INTERNALS__={metadata:{currentWindow:{label},currentWebview:{label,windowLabel:label}},convertFileSrc:p=>p,transformCallback:cb=>{callbacks[++n]=cb;return n;},unregisterCallback:id=>delete callbacks[id],invoke:async(c,a)=>{
      window.fixtureCalls.push({c,a});
      if(c==='plugin:event|listen'){listeners[a.event]=a.handler;return a.handler;}
      if(c==='get_state')return state;if(c==='get_settings')return settings;if(c==='save_settings'){Object.assign(settings,a.settings);return settings;}
      if(c==='list_screen_sources')return options;
      if(c==='answer_ask'){state={...state,state:{type:'Recording'},companion:{...state.companion,size:'Pill'},screen:a.screen?{pick:a.screen,while_sharing:null,waiting:false}:null};emit();return;}
      if(c==='set_screen'){state.screen=a.screen?{pick:a.screen,while_sharing:null,waiting:false}:null;emit();return;}
      if(c==='answer_share_video'){state.screen.while_sharing=a.display;state.share_video_offer=false;emit();return;}
      if(c==='list_meetings')return [summary];if(c==='get_meeting')return {summary,notes_md:'### Overview\nSynthetic fixture.',transcript:text,transcript_heard:text};
      if(c==='meeting_media')return {tracks:[{path:'http://127.0.0.1:3002/audio.wav',name:'part-0000.system.wav',speaker:'Them',offset_ms:0,duration_ms:90000,bytes:100000,waveform:Array(200).fill(.2)}],videos:deleted?[]:[video,{...video,offset_ms:72000,while_sharing:true,source:'Built-in Retina Display'}],moments:deleted?[]:[{ms:0,yours:false},{ms:72000,yours:true}],duration_ms:90000,folder:'Fixture'};
      if(c==='delete_video'){deleted=true;return;}if(c==='restore_video'){deleted=false;return;}
      if(c==='notification_permission')return true;if(c==='library_bytes')return 200000;if(c==='importing')return null;if(['deleted_meetings','open_action_items','call_apps'].includes(c))return [];
      if(c==='microphones')return {mics:[],chosen:null,active:null,issue:null};if(c==='get_levels')return [.1,.2];if(c==='live_transcript')return ['1',text,''];if(c==='get_my_notes')return {meeting_id:'1',text:'',version:1};
      if(c==='storage_summary')return {meetings:1,audio_bytes:100000,video_bytes:100000,notes_bytes:100,free_bytes:9000000000};if(c==='permissions')return {microphone:'Allowed',system_audio:'Allowed',notifications:'Allowed'};if(c==='about')return {version:'0.1.0',platform:'macos',licences:''};return null;
    }};
  };
  for(const theme of ['light','dark']){
    const context=await page.context().browser().newContext({viewport:{width:1200,height:850},colorScheme:theme,reducedMotion:'reduce'});
    await context.route('https://**/*',r=>r.abort());
    const errors=[];context.on('page', p=>p.on('pageerror',e=>errors.push(e.message)));
    const ref=await context.newPage();await ref.emulateMedia({colorScheme:theme,reducedMotion:'no-preference'});await ref.goto('http://127.0.0.1:3001/motetto-app-mockup.html');await ref.evaluate(t=>document.documentElement.dataset.theme=t,theme);
    await ref.locator('#play').click();await ref.locator('#ask:not(.off)').waitFor();await ref.locator('#play').click();await ref.locator('#ask [data-askwhat="screen"]').click();
    await ref.locator('#ask').screenshot({path:`.playwright-out/screen-ref-ask-${theme}.png`});
    const comparisons=[];
    const compare=async(a,b,label)=>{
      const keys=['fontSize','fontWeight','color','backgroundColor','borderRadius','paddingTop','paddingBottom','paddingLeft','gap'];
      const read=loc=>loc.evaluate((e,keys)=>Object.fromEntries(keys.map(k=>[k,getComputedStyle(e)[k]])),keys);
      const x=await read(a),y=await read(b);const diff=Object.fromEntries(keys.filter(k=>x[k]!==y[k]).map(k=>[k,{app:x[k],mockup:y[k]}]));comparisons.push({label,differences:diff});if(Object.keys(diff).length)throw Error(`${label}: ${JSON.stringify(diff)}`);
    };
    const c=await context.newPage();await c.addInitScript(fixture,{label:'companion',kind:'ask'});await c.goto('http://127.0.0.1:3000/companion/');
    await c.getByRole('radio',{name:'Audio',exact:true}).waitFor();if(await c.getByRole('radio',{name:'Audio',exact:true}).getAttribute('aria-checked')!=='true')throw Error('new call did not default to Audio');
    await c.locator('.askcard').screenshot({path:`.playwright-out/screen-ask-audio-${theme}.png`});
    await c.getByRole('radio',{name:'Audio + screen',exact:true}).click();await c.locator('.askcard').screenshot({path:`.playwright-out/screen-ask-on-${theme}.png`});
    await compare(c.locator('.segm'),ref.locator('#ask .segm'),'Ask segmented control');
    await compare(c.locator('.askcard .cline'),ref.locator('#ask .cline'),'Ask consent');
    await ref.locator('#ask [data-srcmenu]').click();
    await c.getByRole('button',{name:'Record from: Zoom window. Change',exact:true}).click();
    await compare(c.locator('.scrmenu'),ref.locator('.scrmenu'),'Screen menu');await ref.locator('.scrmenu').screenshot({path:`.playwright-out/screen-ref-menu-${theme}.png`});
await c.getByRole('menu',{name:'Screen',exact:true}).screenshot({path:`.playwright-out/screen-source-menu-${theme}.png`});
    await c.getByRole('menuitemradio').filter({hasText:'Built-in Retina Display'}).click();await c.getByRole('button',{name:'Record',exact:true}).click();
    await ref.locator('.scrmenu [data-scrsrc="window"]').click();await ref.locator('#ask [data-act="askYes"]').click();
    await compare(c.locator('.scrbtn'),ref.locator('#pill .scrbtn'),'Pill screen button');
    await ref.locator('#pill').screenshot({path:`.playwright-out/screen-ref-pill-${theme}.png`});
    await c.locator('.pill[data-screen="on"]').waitFor();await c.locator('.pill').screenshot({path:`.playwright-out/screen-pill-${theme}.png`});
    await c.getByRole('button',{name:'Recording the screen: Built-in Retina Display',exact:true}).click();await c.getByRole('menuitemradio').filter({hasText:'Audio only'}).click();
    if(await c.locator('.pill').getAttribute('data-screen'))throw Error('screen indicator stayed on');await c.getByRole('button',{name:'Undo',exact:true}).click();await c.locator('.pill[data-screen="on"]').waitFor();
    for(const size of ['Dot','Tucked']) {
      await c.evaluate(size=>window.fixtureState({companion:{visible:true,size,pinned:false,peeking:false,display_id:''}}),size);
      await ref.locator('#pill').evaluate((e,size)=>e.dataset.size=size.toLowerCase(),size);
      const selector=size==='Dot'?'.dotbtn .dot':'.handle .dot';await c.locator(selector).waitFor();
      const a=await c.locator(selector).evaluate(e=>getComputedStyle(e).boxShadow),b=await ref.locator(`#pill ${selector}`).evaluate(e=>getComputedStyle(e).boxShadow);if(a!==b)throw Error(`${size} ring: ${a} != ${b}`);
      await c.locator('.pill').screenshot({path:`.playwright-out/screen-${size.toLowerCase()}-${theme}.png`});
    }
    await c.evaluate(()=>window.fixtureState({companion:{visible:true,size:'Pill',pinned:false,peeking:false,display_id:''}}));
    await ref.locator('#pill').evaluate(e=>e.dataset.size='pill');
    await c.evaluate(()=>window.fixtureState({screen:{pick:{choice:{kind:'CallWindow',app_id:'zoom'},name:'Zoom window'},while_sharing:null,waiting:false},share_video_offer:true,sharing:{hidden_from_capture:null,shared_display:null,other_displays:[]}}));
    await c.getByRole('dialog',{name:'Record your screen while you share?',exact:true}).waitFor();await c.locator('.scrswap').screenshot({path:`.playwright-out/screen-share-${theme}.png`});await c.getByRole('button',{name:'Record Built-in Retina Display',exact:true}).click();
    const widths=await c.locator('.pillbar').evaluate(e=>({width:e.clientWidth,scroll:e.scrollWidth}));if(widths.scroll>widths.width+1)throw Error(`Pill overflow ${JSON.stringify(widths)}`);
    const rec=await context.newPage();await rec.addInitScript(fixture,{label:'main',kind:'recording'});await rec.goto('http://127.0.0.1:3000/main/');await rec.getByRole('button',{name:/Screen fixture/}).first().click();
    await rec.locator('.recbar [data-scrmenu]').click();await rec.getByRole('menuitemradio').filter({hasText:'Zoom window'}).click();await rec.locator('.recbar .micchip.scron').waitFor();
    await rec.locator('.recbar').screenshot({path:`.playwright-out/screen-recording-bar-${theme}.png`});
    const st=await context.newPage();await st.addInitScript(fixture,{label:'settings',kind:'ready'});await st.goto('http://127.0.0.1:3000/settings/?tab=screen');await st.getByRole('combobox',{name:'Picture quality',exact:true}).selectOption('smaller');await st.screenshot({path:`.playwright-out/screen-settings-${theme}.png`});
    await st.setViewportSize({width:760,height:520});
    if(await st.locator('.setpane').evaluate(e=>e.scrollWidth>e.clientWidth+1))throw Error('Screen settings overflow at minimum width');
    await st.setViewportSize({width:1200,height:850});
    await ref.getByRole('button',{name:'Settings',exact:true}).click();await ref.locator('#settabs [data-set="Screen"]').click();
    await compare(st.getByRole('combobox',{name:'Picture quality',exact:true}),ref.getByRole('combobox',{name:'Picture quality',exact:true}),'Settings picture select');
    await ref.locator('#setpane').screenshot({path:`.playwright-out/screen-ref-settings-${theme}.png`});
    const m=await context.newPage();await m.addInitScript(fixture,{label:'main',kind:'ready'});await m.goto('http://127.0.0.1:3000/main/');await m.getByRole('button',{name:/Screen fixture/}).first().click();await m.getByRole('tab',{name:'Video',exact:true}).click();await m.locator('.vframe video').waitFor();await m.locator('.docbody').screenshot({path:`.playwright-out/screen-video-${theme}.png`});
    await ref.getByRole('button',{name:'After the call',exact:true}).click();await ref.getByRole('tab',{name:'Video',exact:true}).click();
    await compare(m.locator('.vctl'),ref.locator('.vctl'),'Video controls');
    await compare(m.locator('.vctl .cc'),ref.locator('.vctl .cc'),'Video CC');
    await compare(m.locator('.vline .who.them').first(),ref.locator('.vline .who.them').first(),'Video Them colour');
    await compare(m.locator('.vline .who.me').first(),ref.locator('.vline .who.me').first(),'Video You colour');
    await ref.locator('.docbody').screenshot({path:`.playwright-out/screen-ref-video-${theme}.png`});
    await m.setViewportSize({width:760,height:520});
    if(await m.locator('.docbody').evaluate(e=>e.scrollWidth>e.clientWidth+1))throw Error('Video overflows at minimum width');
    await m.setViewportSize({width:1200,height:850});
    await m.getByRole('button',{name:'CC',exact:true}).click();if(await m.getByRole('button',{name:'CC',exact:true}).getAttribute('aria-pressed')!=='false')throw Error('CC did not toggle');await m.getByRole('combobox',{name:'Speed',exact:true}).selectOption('1.5');
    await m.getByRole('button',{name:'Play video',exact:true}).click({position:{x:20,y:20}});
    await m.getByRole('button',{name:'Pause',exact:true}).waitFor();
    await m.waitForTimeout(200);const commands=await m.evaluate(()=>({...window.fixtureMedia}));
    await m.waitForTimeout(700);
    const later=await m.evaluate(()=>({...window.fixtureMedia}));
    if(later.rateWrites!==commands.rateWrites||later.videoPlays!==commands.videoPlays)throw Error('Clock ticks repeated media rate/play commands');
    await m.getByRole('button',{name:'Pause video',exact:true}).click({position:{x:20,y:20}});
    await m.getByRole('button',{name:'Play video',exact:true}).waitFor();
    if(!await m.locator('.vframe video').evaluate(v=>v.paused))throw Error('Clicking the picture did not pause video');
    if(await m.locator('audio').evaluateAll(elements=>elements.some(e=>!e.paused)))throw Error('Clicking the picture did not pause audio');
    await m.getByRole('button',{name:'Play video',exact:true}).focus();await m.keyboard.press('Space');
    await m.getByRole('button',{name:'Pause video',exact:true}).waitFor();await m.keyboard.press('Enter');
    await m.getByRole('button',{name:'Play video',exact:true}).waitFor();
    await m.locator('.vlines button').nth(1).click();await m.getByText('Screen not recorded here. The audio plays.',{exact:true}).waitFor();await m.locator('.docbody').screenshot({path:`.playwright-out/screen-video-gap-${theme}.png`});
    await m.getByRole('button',{name:'Delete video',exact:true}).click();await m.locator('.vside').screenshot({path:`.playwright-out/screen-video-delete-${theme}.png`});await m.getByRole('button',{name:'Delete video',exact:true}).click();await m.getByRole('tab',{name:'Audio',exact:true}).waitFor();
    await m.getByRole('button',{name:'Undo',exact:true}).click();await m.getByRole('tab',{name:'Video',exact:true}).waitFor();
    results.push({theme,comparisons});
    if(errors.length)throw Error(errors.join('\n'));
    results.push({theme,recordedScreenChoice:true,audioOnlyAndUndo:true,shareChoice:true,settingsSave:true,videoGapCCRateDeleteUndo:true,pictureClickAndKeyboard:true,noRepeatedMediaCommands:true});await context.close();
  }
  return results;
}
