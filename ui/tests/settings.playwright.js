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
   const settings={version:1,notes_folder:'/Users/test/Documents/Motetto/imports',open_at_login:false,notes_language:'same_as_call',ask_on_call:true,app_modes:{},apps:[],idle_stop_s:30,discard_short_s:20,speech_model:'/local/ggml-large-v3-q5_0.bin',notes_model:'qwen3:8b',call_language:'detect',part_chars:24000,keep_audio:'forever',recording_reminder:true,sounds:true,companion:{default_size:'pill',captions:true},spellings:[],shortcuts:Object.fromEntries(['S','M','J','N','Space','H','P'].map(k=>[k,`Ctrl+Alt+${k}`]))};
   const apps=[['zoom','Zoom',true,'ask'],['teams','Microsoft Teams',true,'ask'],['meet','Google Meet',false,'ask'],['whatsapp','WhatsApp',false,'ask'],['telegram','Telegram',false,'ask'],['browser','Browser',false,'ask'],['slack','Slack huddles',false,'off'],['webex','Webex',true,'off'],['facetime','FaceTime',true,'off']].map(([app_id,name,single_signal,mode])=>({app_id,name,single_signal,mode,mac_ids:[],win_ids:[],title_patterns:[]}));
   let next=0;window.testCalls=[];window.failNextSave=false;window.deleted=false;
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
    if(command==='installed_models')return {speech:[settings.speech_model,'/local/ggml-small.bin'],notes:['qwen3:8b','qwen3.5:4b'],warning:null,suggested:'qwen3:8b',memory_gb:32};
    if(command==='run_speed_test')return .25;
    if(command==='storage_summary')return {meetings:window.deleted?0:5,audio_bytes:window.deleted?0:2100000000,notes_bytes:3400000,free_bytes:212000000000};
    if(command==='delete_all_meetings'){window.deleted=true;return;}
    if(command==='notification_permission')return true;
    if(command==='about')return {version:'0.1.0',platform:'macos',licences:'whisper.cpp (MIT) · Tauri (MIT or Apache-2.0)'};
    if(command==='open_logs_folder')return;
    if(command==='pick_speech_model'||command==='move_notes_folder')return null;
    throw Error(`Unexpected IPC: ${command}`);
   }};
  });
  await p.goto('http://127.0.0.1:3002/settings/?tab=calls');
  await p.getByRole('checkbox',{name:'Ask when a call starts'}).waitFor();
  await p.getByRole('checkbox',{name:'Ask when a call starts'}).uncheck();
  await p.getByRole('status').filter({hasText:'Saved'}).waitFor();
  await p.getByRole('combobox',{name:'When Zoom starts a call',exact:true}).selectOption('off');
  await p.waitForFunction(()=>window.testCalls.some(c=>c.command==='set_app_mode'&&c.args.mode==='off'));
  await p.getByRole('tab',{name:'Calls',exact:true}).click();
  const diffs=[];
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
  // The SPEC tiers by memory (owner 2026-10-25): each known model names its tier; the help says which suits this Mac.
  const notesOptions=await p.getByRole('combobox',{name:'Notes model',exact:true}).locator('option').allTextContents();
  if(!notesOptions.includes('qwen3:8b · Best')||!notesOptions.includes('qwen3.5:4b · Balanced'))throw Error('Tier labels: '+notesOptions);
  await p.getByText('This Mac has 32 GB of memory. Motetto suggests Best (qwen3:8b).',{exact:true}).waitFor();
  await p.getByRole('combobox',{name:'Notes model',exact:true}).selectOption('qwen3.5:4b');
  await p.getByRole('status').filter({hasText:'Saved'}).waitFor();
  await p.getByRole('button',{name:'Run the speed test again',exact:true}).click();
  await p.getByText(/Last speed test: 0.25/).waitFor();
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
