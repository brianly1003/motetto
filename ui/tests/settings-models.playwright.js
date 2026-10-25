// Settings > Models while first-run setup downloads the tier's models (owner acceptance, SPEC 8.4).
async(page)=>{
 const results=[];
 for(const theme of ['light','dark']){
  const p=await page.context().newPage();const errors=[];p.on('pageerror',e=>errors.push(e.message));
  await p.emulateMedia({colorScheme:theme});await p.setViewportSize({width:958,height:760});
  await p.addInitScript(()=>{
   const settings={version:1,notes_folder:'/Users/test/Documents/Motetto/imports',open_at_login:false,notes_language:'same_as_call',ask_on_call:true,app_modes:{},apps:[],idle_stop_s:30,discard_short_s:20,speech_model:'speech-large-v3-turbo',notes_model:'',tier:'Balanced',speed_test:null,call_language:'detect',part_chars:24000,keep_audio:'forever',recording_reminder:true,sounds:true,companion:{default_size:'pill',captions:true},spellings:[],shortcuts:Object.fromEntries(['S','M','J','N','Space','H','P'].map(k=>[k,`Ctrl+Alt+${k}`]))};
   const apps=[['zoom','Zoom',true,'ask'],['teams','Microsoft Teams',true,'ask'],['meet','Google Meet',false,'ask'],['whatsapp','WhatsApp',false,'ask'],['telegram','Telegram',false,'ask'],['browser','Browser',false,'ask'],['slack','Slack huddles',false,'off'],['webex','Webex',true,'off'],['facetime','FaceTime',true,'off']].map(([app_id,name,single_signal,mode])=>({app_id,name,single_signal,mode,mac_ids:[],win_ids:[],title_patterns:[]}));
   let next=0;window.testCalls=[];window.onDisk=['speech-large-v3-turbo','vad-silero'];window.failNextSave=false;window.deleted=false;
   const callbacks={},listeners={};window.emit=payload=>{const id=listeners['motetto://event'];if(id)callbacks[id]?.({event:'motetto://event',id:0,payload});};
   window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener:()=>{}};
   window.__TAURI_INTERNALS__={metadata:{currentWindow:{label:'settings'},currentWebview:{windowLabel:'settings',label:'settings'}},transformCallback:cb=>{callbacks[++next]=cb;return next;},unregisterCallback:id=>delete callbacks[id],invoke:async(command,args)=>{
    window.testCalls.push({command,args});
    if(command==='plugin:event|listen'){listeners[args.event]=args.handler;return args.handler;}
    if(command==='plugin:event|unlisten')return;
    if(command==='get_settings'){Object.assign(settings,window.settingsOverride||{});return structuredClone(settings);}
    if(command==='call_apps')return structuredClone(apps);
    if(command==='save_settings'){
     if(window.failNextSave){window.failNextSave=false;throw Error('Disk is full. Settings were not saved.');}
     Object.assign(settings,args.settings);window.savedSettings=structuredClone(settings);return structuredClone(settings);
    }
    if(command==='set_ask_on_call'){settings.ask_on_call=args.on;return;}
    if(command==='set_app_mode'){apps.find(app=>app.app_id===args.appId).mode=args.mode;return;}
    if(command==='recent_mic_apps')return ['com.example.Discord'];
    if(command==='add_app'){apps.push({app_id:args.appId,name:args.appId,mode:'ask',single_signal:false,mac_ids:[args.appId],win_ids:[],title_patterns:[]});return structuredClone(settings);}
    if(command==='model_catalog')return {vad:'vad-silero',tiers:[{id:'Best',speech:'speech-large-v3',notes:'notes-qwen3-8b'},{id:'Balanced',speech:'speech-large-v3-turbo',notes:'notes-qwen3.5-4b'},{id:'Light',speech:'speech-small',notes:'notes-qwen3.5-2b'}],files:[['speech-large-v3','Speech','large-v3',1081140203],['speech-large-v3-turbo','Speech','large-v3-turbo',574041195],['speech-small','Speech','small',487601967],['vad-silero','Vad','Voice detection',885098],['notes-qwen3-8b','Notes','Qwen3 8B',5027783488],['notes-qwen3.5-4b','Notes','Qwen3.5 4B',2740937888],['notes-qwen3.5-2b','Notes','Qwen3.5 2B',1280835840]].map(([id,kind,label,bytes])=>({id,kind,label,bytes,file:id,url:'',sha256:'',on_disk:window.onDisk.includes(id)}))};
    if(command==='setup_state')return structuredClone(window.setupState||{tier:'Best',phase:{type:'Idle'},files:[],speed:null,keep_slow:false});
    if(command==='hardware_info')return {chip:'M1 Max',memory_gb:32,suggested:'Best',label:'M1 Max, 32 GB'};
    if(command==='run_speed_test')return {tier:args.tier,rtf:.25,keeps_up:true,measured_at:'1'};
    if(command==='answer_slow'){window.answered=args.useLower;settings.slow_question=null;if(window.settingsOverride)delete window.settingsOverride.slow_question;return;}
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
  const notes=p.getByRole('combobox',{name:'Notes model',exact:true});
  // Setup idle with the tier's file not on disk: no made-up "Waiting to download".
  await p.waitForTimeout(100);
  if(await p.getByText('Waiting to download').count())throw Error(theme+': an idle setup reads as waiting to download');
  // The speed question waits in Settings too; Keep answers it.
  await p.evaluate(()=>{window.settingsOverride={slow_question:'Light',speed_test:{tier:'Balanced',rtf:.8,keeps_up:false,measured_at:'1'}};window.dispatchEvent(new Event('focus'));});
  await p.getByRole('alert').getByText('This computer transcribed at 0.80× real time; live notes need 0.5× or faster.',{exact:true}).waitFor();
  await p.getByRole('button',{name:'Use Light'}).waitFor();
  await p.screenshot({path:`.playwright-out/settings-models-slow-${theme}.png`});
  await p.getByRole('button',{name:'Keep Balanced'}).click();
  await p.waitForFunction(()=>window.answered===false);
  await p.getByRole('button',{name:'Keep Balanced'}).waitFor({state:'detached'});
  await p.evaluate(()=>{window.settingsOverride={speed_test:null};});
  // Setup is downloading the tier's notes model; the setting is still empty.
  await p.evaluate(()=>{window.setupState={tier:'Balanced',phase:{type:'Downloading',id:'notes-qwen3.5-4b'},files:[['speech-large-v3-turbo',574041195,574041195],['vad-silero',885098,885098],['notes-qwen3.5-4b',1151193913,2740937888]],speed:null,keep_slow:false};window.emit({type:'SetupChanged',...window.setupState});});
  await p.getByText('Downloading… 42%',{exact:true}).waitFor();
  if(await notes.inputValue()!=='notes-qwen3.5-4b')throw Error(theme+': the tier notes model is not shown as the choice');
  if((await notes.locator('option').allTextContents()).includes('Choose a model'))throw Error(theme+': Choose a model is still listed');
  await p.evaluate(()=>window.emit({type:'DownloadProgress',id:'notes-qwen3.5-4b',bytes:1370468944,total:2740937888}));
  await p.getByText('Downloading… 50%',{exact:true}).waitFor();
  if(await p.evaluate(()=>window.savedSettings!==undefined))throw Error(theme+': a model was saved before it was verified');
  await p.screenshot({path:`.playwright-out/settings-models-setup-${theme}.png`});
  // DownloadDone comes before the SetupChanged that saves the choice: the file on disk stays shown.
  await p.evaluate(()=>{window.onDisk.push('notes-qwen3.5-4b');window.emit({type:'DownloadDone',id:'notes-qwen3.5-4b'});});
  await p.waitForFunction(()=>window.testCalls.filter(c=>c.command==='model_catalog').length>=3);
  await p.waitForTimeout(150);
  if(await notes.inputValue()!=='notes-qwen3.5-4b')throw Error(theme+': the downloaded tier file is not shown before the choice is saved');
  if((await notes.locator('option').allTextContents()).includes('Choose a model'))throw Error(theme+': Choose a model flashed after DownloadDone');
  // Settings is on another tab when setup saves the choice.
  await p.getByRole('tab',{name:'Privacy',exact:true}).click();
  await p.evaluate(()=>{window.settingsOverride={notes_model:'notes-qwen3.5-4b'};window.setupState={...window.setupState,phase:{type:'Done'}};window.emit({type:'SetupChanged',...window.setupState});});
  await p.getByRole('tab',{name:'Models',exact:true}).click();
  await p.waitForFunction(()=>document.querySelector('select[aria-label="Notes model"]')?.value==='notes-qwen3.5-4b');
  if(await p.getByText(/Downloading|Waiting to download|Not downloaded/).count())throw Error(theme+': a status line remains after the download finished');
  await p.screenshot({path:`.playwright-out/settings-models-setup-done-${theme}.png`});
  // A failed download's reason is shown once, as is.
  const speech=p.getByRole('combobox',{name:'Speech model',exact:true});
  await speech.selectOption('speech-small');
  await p.getByRole('button',{name:'Download 487.6 MB'}).click();
  await p.waitForFunction(()=>(window.downloads||[]).includes('speech-small'));
  await p.evaluate(()=>window.emit({type:'DownloadFailed',id:'speech-small',reason:'Download stopped: no data for 30 seconds.',checksum:false}));
  await p.getByText('Download stopped: no data for 30 seconds.',{exact:true}).waitFor();
  if(await p.getByText(/Download stopped: Download stopped/).count())throw Error(theme+': Download stopped shown twice');
  if(errors.length)throw Error(errors.join('\n'));
  results.push({theme,setup:'passed'});await p.close();
 }return results;
}
