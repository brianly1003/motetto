// UI fixture: run with Playwright and with the cached WebKit runner.
async(page)=>{
 const results=[];const engine=page.context().browser().browserType().name();
 for(const theme of ['light','dark']){
  const p=await page.context().newPage();const errors=[];p.on('pageerror',e=>errors.push(e.message));
  await p.emulateMedia({colorScheme:theme});await p.setViewportSize({width:878,height:614});
  await p.addInitScript(()=>{
   const settings={version:1,notes_folder:'/Users/test/Documents/Motetto/imports',open_at_login:false,notes_language:'same_as_call',ask_on_call:true,app_modes:{},apps:[],idle_stop_s:30,discard_short_s:20,speech_model:'speech-large-v3',notes_model:'notes-qwen3-8b',speed_test:null,call_language:'detect',part_chars:24000,keep_audio:'forever',recording_reminder:true,sounds:true,companion:{default_size:'pill',captions:true},spellings:[],shortcuts:Object.fromEntries(['S','M','J','N','Space','H','P'].map(k=>[k,`Ctrl+Alt+${k}`]))};
   const apps=[['zoom','Zoom',true,'ask'],['teams','Microsoft Teams',true,'ask'],['meet','Google Meet',false,'ask'],['whatsapp','WhatsApp',false,'ask'],['telegram','Telegram',false,'ask'],['browser','Browser',false,'ask'],['slack','Slack huddles',false,'off'],['webex','Webex',true,'off'],['facetime','FaceTime',true,'off']].map(([app_id,name,single_signal,mode])=>({app_id,name,single_signal,mode,mac_ids:[],win_ids:[],title_patterns:[]}));
   let next=0;window.testCalls=[];window.onDisk=['speech-large-v3','speech-small','vad-silero','notes-qwen3-8b'];window.failNextSave=false;window.deleted=false;
   window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener:()=>{}};
   window.__TAURI_INTERNALS__={transformCallback:()=>++next,unregisterCallback:()=>{},invoke:async(command,args)=>{
    window.testCalls.push({command,args});
    if(command==='plugin:event|listen')return next;
    if(command==='plugin:event|unlisten')return;
    if(command==='get_settings')return structuredClone(settings);
    if(command==='call_apps')return structuredClone(apps);
    if(command==='save_settings'){
     if(window.failNextSave){window.failNextSave=false;throw Error('Disk is full. Settings were not saved.');}
     Object.assign(settings,args.settings);return structuredClone(settings);
    }
    if(command==='set_ask_on_call'){settings.ask_on_call=args.on;return;}
    if(command==='set_app_mode'){apps.find(app=>app.app_id===args.appId).mode=args.mode;return;}
    if(command==='recent_mic_apps')return ['com.example.Discord'];
    if(command==='add_app'){apps.push({app_id:args.appId,name:args.appId,mode:'ask',single_signal:false,mac_ids:[args.appId],win_ids:[],title_patterns:[]});return structuredClone(settings);}
    if(command==='model_catalog')return {vad:'vad-silero',tiers:[{id:'Best',speech:'speech-large-v3',notes:'notes-qwen3-8b'},{id:'Balanced',speech:'speech-large-v3-turbo',notes:'notes-qwen3.5-4b'},{id:'Light',speech:'speech-small',notes:'notes-qwen3.5-2b'}],files:[['speech-large-v3','Speech','large-v3',1081140203],['speech-large-v3-turbo','Speech','large-v3-turbo',574041195],['speech-small','Speech','small',487601967],['vad-silero','Vad','Voice detection',885098],['notes-qwen3-8b','Notes','Qwen3 8B',5027783488],['notes-qwen3.5-4b','Notes','Qwen3.5 4B',2740937888],['notes-qwen3.5-2b','Notes','Qwen3.5 2B',1280835840]].map(([id,kind,label,bytes])=>({id,kind,label,bytes,file:id,url:'',sha256:'',on_disk:window.onDisk.includes(id)}))};
    if(command==='setup_state')return structuredClone(window.setupState||{tier:'Best',phase:{type:'Idle'},files:[],speed:null,keep_slow:false});
    if(command==='hardware_info')return {chip:'M1 Max',memory_gb:32,suggested:'Best',label:'M1 Max, 32 GB'};
    if(command==='run_speed_test')return {tier:args.tier,rtf:.25,keeps_up:true,measured_at:'1'};
    if(command==='download_model'){window.downloads=[...(window.downloads||[]),args.id];return;}
    if(command==='delete_unused_models')return 574041195;
    if(command==='set_onboarding_step')return;
    if(command==='open_setup'){window.setupOpened=true;return;}
    if(command==='storage_summary')return {meetings:window.deleted?0:5,audio_bytes:window.deleted?0:2100000000,notes_bytes:3400000,free_bytes:212000000000};
    if(command==='delete_all_meetings'){window.deleted=true;return;}
    if(command==='notification_permission')return true;
    if(command==='about')return {version:'0.1.0',platform:'macos',licences:'whisper.cpp (MIT) · Tauri (MIT or Apache-2.0)'};
    if(command==='open_logs_folder')return;
    if(command==='move_notes_folder')return null;
    throw Error(`Unexpected IPC: ${command}`);
   }};
  });

  await p.goto('http://127.0.0.1:3004/settings/?tab=models');
  await p.getByRole('combobox',{name:'Notes model',exact:true}).waitFor();
  const controls=await p.locator('.settings-window .field select').evaluateAll(els=>els.map(el=>{
   const s=getComputedStyle(el),rect=el.getBoundingClientRect();
   return {label:el.ariaLabel,width:rect.width,height:rect.height,font:s.fontSize,padding:s.padding,appearance:s.appearance,arrow:s.backgroundImage};
  }));
  // Approved Chromium mockup: 30.5px high; intrinsic widths 261.5 / 227.5 / 110px.
  const widths={'Speech model':261.5,'Notes model':227.5,'Call language':110};
  for(const c of controls){
   if(Math.abs(c.height-30.5)>1||Math.abs(c.width-widths[c.label])>1||c.font!=='14px'||c.appearance!=='none'||!c.arrow.includes('svg'))throw Error('Dropdown does not match the approved geometry: '+JSON.stringify(c));
  }
  await p.screenshot({path:`.playwright-out/settings-selects-${engine}-${theme}.png`});
  await p.getByRole('combobox',{name:'Call language',exact:true}).selectOption('vi');
  await p.waitForFunction(()=>window.testCalls.some(c=>c.command==='save_settings'&&c.args.settings.call_language==='vi'));
  await p.getByRole('status').filter({hasText:'Saved'}).waitFor();
  await p.keyboard.press('Tab');await p.getByRole('combobox',{name:'Call language',exact:true}).focus();
  const focus=await p.getByRole('combobox',{name:'Call language',exact:true}).evaluate(el=>({outline:getComputedStyle(el).outlineStyle,shadow:getComputedStyle(el).boxShadow}));
  if(focus.outline!=='none'||focus.shadow==='none')throw Error('Dropdown lost keyboard focus feedback');
  await p.setViewportSize({width:960,height:760});
  for(const tab of ['General','Calls','Models','Spellings','Privacy','About']){
   await p.getByRole('tab',{name:tab,exact:true}).click();
   const bounds=await p.locator('.setpane').evaluate(e=>({height:e.clientHeight,content:e.scrollHeight,width:e.clientWidth,contentWidth:e.scrollWidth}));
   if(bounds.content>bounds.height+1||bounds.contentWidth>bounds.width+1)throw Error(tab+' needs scrolling at the default size: '+JSON.stringify(bounds));
  }
  await p.getByRole('tab',{name:'General',exact:true}).click();
  await p.screenshot({path:`.playwright-out/settings-full-${engine}-${theme}.png`});
  await p.setViewportSize({width:760,height:520});
  for(const tab of ['General','Calls','Models','Privacy']){
   await p.getByRole('tab',{name:tab,exact:true}).click();
   if(tab==='Calls'){const size=await p.locator('.tbl select').first().boundingBox();if(size.width!==51||size.height!==22)throw Error('Call mode selects lost their compact mockup size');}
   const overflow=await p.evaluate(()=>document.querySelector('.setpane').scrollWidth>document.querySelector('.setpane').clientWidth);
   if(overflow)throw Error(tab+' dropdowns overflow at minimum size');
  }
  if(errors.length)throw Error(errors.join('\n'));
  results.push({engine,theme,controls,keyboard:'passed',selection:'saved',minimum:'passed'});
  await p.close();
 }return results;
}
