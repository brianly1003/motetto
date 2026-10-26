// Browser IPC fixtures only. Persistence, retention and local-only discovery have Rust tests.
async (page)=>{
 const reference=await page.context().newPage();
 await reference.route('https://**/*',route=>route.abort());
 await reference.setViewportSize({width:1200,height:900});
 await reference.goto('http://127.0.0.1:3141/motetto-app-mockup.html?screen=settings');
 await reference.getByRole('button',{name:'macOS',exact:true}).click();
 await reference.getByRole('button',{name:'Settings',exact:true}).click();
 const results=[];
 for(const theme of ['light','dark']){
  const p=await page.context().newPage();let errors=[];p.on('pageerror',e=>errors.push(e.message));
  await p.setViewportSize({width:958,height:760});await p.emulateMedia({colorScheme:theme});
  await reference.evaluate(theme=>document.documentElement.setAttribute('data-theme',theme),theme);
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
  await p.goto('http://127.0.0.1:3004/settings/?tab=calls');
  await p.getByRole('checkbox',{name:'Ask when a call starts'}).waitFor();
  await p.getByRole('checkbox',{name:'Ask when a call starts'}).uncheck();
  await p.getByRole('status').filter({hasText:'Saved'}).waitFor();
  await p.getByRole('combobox',{name:'When Zoom starts a call',exact:true}).selectOption('off');
  await p.waitForFunction(()=>window.testCalls.some(c=>c.command==='set_app_mode'&&c.args.mode==='off'));
  await p.getByRole('tab',{name:'Calls',exact:true}).click();
  const diffs=[];
  await reference.getByRole('tab',{name:'Calls',exact:true}).click();
  for(const selector of ['.settabs','.settabs button','.setpane','.formrow','.formrow > .lbl','.help','.tbl','.tbl th','.tbl td','.tbl select','.switch']){
   const props=['padding','gap','fontSize','fontWeight','color','backgroundColor','borderRadius','gridTemplateColumns'].filter(key=>selector!=='.tbl select'||key!=='padding');
   const styles=await p.locator(selector).first().evaluate((el,props)=>Object.fromEntries(props.map(k=>[k,getComputedStyle(el)[k]])),props);
   const expected=await reference.locator(selector).first().evaluate((el,props)=>Object.fromEntries(props.map(k=>[k,getComputedStyle(el)[k]])),props);
   for(const key of props)if(styles[key]!==expected[key])diffs.push(`${selector} ${key}: ${styles[key]} != ${expected[key]}`);
  }
  // Native popup arrows consume space outside CSS padding; compare rendered geometry instead.
  const actualSelect=await p.locator('.tbl select').first().boundingBox();
  const referenceSelect=await reference.locator('.tbl select').first().boundingBox();
  if(Math.abs(actualSelect.width-referenceSelect.width)>1||Math.abs(actualSelect.height-referenceSelect.height)>1)diffs.push('Call mode select geometry differs from the mockup');
  await reference.locator('.setpane input#auto').uncheck();
  await reference.getByRole('combobox',{name:'When Zoom starts a call',exact:true}).selectOption('Off');
  await p.screenshot({path:`.playwright-out/settings-calls-${theme}.png`});
  await reference.locator('.set').screenshot({path:`.playwright-out/settings-reference-${theme}.png`});
  await p.getByRole('button',{name:'Add app…',exact:true}).click();
  await p.getByRole('button',{name:'Add com.example.Discord',exact:true}).click();
  await p.getByRole('combobox',{name:'When com.example.Discord starts a call',exact:true}).waitFor();
  await p.getByRole('tab',{name:'General',exact:true}).click();
  await p.getByRole('button',{name:'Run setup again',exact:true}).click();
  await p.waitForFunction(()=>window.setupOpened===true);
  const calls=await p.evaluate(()=>window.testCalls.map(c=>c.command));
  const stepCall=await p.evaluate(()=>window.testCalls.find(c=>c.command==='set_onboarding_step'));
  if(stepCall.args.step!==0||calls.indexOf('set_onboarding_step')>calls.indexOf('open_setup')||calls.includes('delete_model'))throw Error('Run setup again: '+JSON.stringify(calls));
  await p.getByRole('combobox',{name:'Notes language',exact:true}).selectOption('vi');
  await p.getByRole('status').filter({hasText:'Saved'}).waitFor();
  await p.evaluate(()=>window.failNextSave=true);
  await p.getByRole('checkbox',{name:'Start and stop sounds'}).click();
  await p.getByRole('alert').filter({hasText:'Disk is full'}).waitFor();
  if(!await p.getByRole('checkbox',{name:'Start and stop sounds'}).isChecked())throw Error('Failed save did not restore the saved value');
  await p.getByRole('tab',{name:'General',exact:true}).focus();await p.keyboard.press('ArrowRight');
  // Audio follows General (owner 2026-10-23, DESIGN 4.15).
  if(await p.getByRole('tab',{name:'Audio',exact:true}).getAttribute('aria-selected')!=='true')throw Error('Arrow tab navigation failed');
  await p.getByRole('tab',{name:'Models',exact:true}).click();
  // The catalog (SPEC 5.1): each file reads label, size, tier; the machine line comes from hardware_info.
  const speechOptions=await p.getByRole('combobox',{name:'Speech model',exact:true}).locator('option').allTextContents();
  if(speechOptions.join('|')!=='large-v3 · 1.1 GB · Best|large-v3-turbo · 574.0 MB · Balanced|small · 487.6 MB · Light')throw Error('Speech options: '+speechOptions);
  const notesOptions=await p.getByRole('combobox',{name:'Notes model',exact:true}).locator('option').allTextContents();
  if(notesOptions.join('|')!=='Qwen3 8B · 5.0 GB · Best|Qwen3.5 4B · 2.7 GB · Balanced|Qwen3.5 2B · 1.3 GB · Light')throw Error('Notes options: '+notesOptions);
  if(await p.getByText(/planned in M5/).count())throw Error('Placeholder text is still shown');
  await p.getByText(/This Mac: M1 Max, 32 GB\. No speed test has run yet\./).waitFor();
  // Mockup parity for the Models rows.
  await reference.getByRole('tab',{name:'Models',exact:true}).click();
  const modelDiffs=[];
  for(const selector of ['.formrow','.formrow > .lbl','.field','.note','.note .btn']){
   const props=['padding','gap','fontSize','fontWeight','color','backgroundColor','borderRadius','gridTemplateColumns'];
   const styles=await p.locator(selector).first().evaluate((el,props)=>Object.fromEntries(props.map(k=>[k,getComputedStyle(el)[k]])),props);
   const expected=await reference.locator(selector).first().evaluate((el,props)=>Object.fromEntries(props.map(k=>[k,getComputedStyle(el)[k]])),props);
   for(const key of props)if(styles[key]!==expected[key])modelDiffs.push(`Models ${selector} ${key}: ${styles[key]} != ${expected[key]}`);
  }
  for(const name of ['Speech model','Notes model']){
   const a=await p.getByRole('combobox',{name,exact:true}).boundingBox(),b=await reference.getByRole('combobox',{name,exact:true}).boundingBox();
   if(Math.abs(a.width-b.width)>1||Math.abs(a.height-b.height)>1)modelDiffs.push(`${name} geometry ${a.width}x${a.height} != ${b.width}x${b.height}`);
  }
  await p.screenshot({path:`.playwright-out/settings-models-${theme}.png`});
  await reference.locator('.set').screenshot({path:`.playwright-out/settings-models-reference-${theme}.png`});
  diffs.push(...modelDiffs);
  // A file that is not installed: the old model stays, nothing is saved, the download line appears.
  await p.getByRole('combobox',{name:'Notes model',exact:true}).selectOption('notes-qwen3.5-4b');
  await p.getByText('Not downloaded',{exact:true}).waitFor();
  await p.getByRole('button',{name:'Download 2.7 GB',exact:true}).waitFor();
  if(await p.evaluate(()=>window.testCalls.some(c=>c.command==='save_settings'&&c.args.settings.notes_model==='notes-qwen3.5-4b')))throw Error('An absent notes model was saved');
  await p.getByRole('button',{name:'Download 2.7 GB',exact:true}).click();
  await p.waitForFunction(()=>(window.downloads||[]).includes('notes-qwen3.5-4b'));
  await p.getByRole('combobox',{name:'Speech model',exact:true}).selectOption('speech-small');
  await p.waitForFunction(()=>window.testCalls.some(c=>c.command==='save_settings'&&c.args.settings.speech_model==='speech-small'));
  await p.getByRole('status').filter({hasText:'Saved'}).waitFor();
  await p.getByRole('button',{name:'Run the speed test again',exact:true}).click();
  await p.getByText(/Last speed test: 0.25× real time\. This speech model keeps up live\./).waitFor();
  if(!await p.evaluate(()=>window.testCalls.some(c=>c.command==='run_speed_test'&&c.args.tier==='Light')))throw Error('The speed test did not name the tier of the chosen speech model');
  await p.getByRole('button',{name:'Delete unused models (1.1 GB)',exact:true}).click();
  await p.getByRole('status').filter({hasText:'Unused models deleted'}).waitFor();
  if(!await p.evaluate(()=>window.testCalls.some(c=>c.command==='delete_unused_models')))throw Error('Delete unused did not call the backend');
  await p.getByRole('tab',{name:'Spellings',exact:true}).click();
  await p.getByRole('button',{name:'Add spelling',exact:true}).click();
  await p.getByRole('textbox',{name:'Spelling',exact:true}).fill('FCM');await p.getByRole('textbox',{name:'Often heard as',exact:true}).fill('F C M');
  await p.getByRole('button',{name:'Add',exact:true}).click();
  await p.getByRole('button',{name:'Remove FCM',exact:true}).waitFor();
  await p.getByRole('button',{name:'Remove FCM',exact:true}).click();await p.getByText('No spellings yet.',{exact:true}).waitFor();
  await p.getByRole('tab',{name:'Privacy',exact:true}).click();
  await p.getByRole('button',{name:'Delete all meetings…',exact:true}).click();
  await p.getByRole('button',{name:'Cancel',exact:true}).click();
  if(await p.evaluate(()=>window.deleted))throw Error('Cancel deleted data');
  await p.getByRole('button',{name:'Delete all meetings…',exact:true}).click();
  await p.getByRole('button',{name:'Delete all',exact:true}).click();
  await p.getByRole('status').filter({hasText:'All meetings deleted'}).waitFor();
  await p.getByRole('tab',{name:'About',exact:true}).click();await p.getByText('Motetto 0.1.0',{exact:true}).waitFor();
  await p.getByRole('button',{name:'Show logs folder',exact:true}).click();
  for(const tab of ['General','Audio','Calls','Models','Spellings','Privacy','About']){
   await p.getByRole('tab',{name:tab,exact:true}).click();await p.setViewportSize({width:760,height:520});
   const overflow=await p.evaluate(()=>document.documentElement.scrollWidth>innerWidth||document.querySelector('.setpane').scrollWidth>document.querySelector('.setpane').clientWidth);
   if(overflow)throw Error(`${tab} overflows at minimum width`);
   await p.screenshot({path:`.playwright-out/settings-${tab.toLowerCase()}-minimum-${theme}.png`});
  }
  if(errors.length)throw Error(errors.join('\n'));
  if(diffs.length)throw Error(diffs.join('\n'));
  results.push({theme,differences:diffs,interactions:'passed',minimum:'all seven tabs passed'});
  await p.close();
 }
 await reference.close();return results;
}
