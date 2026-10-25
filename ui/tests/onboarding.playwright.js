// Browser IPC fixtures only (first run). Downloads, the speed test, permissions and relaunch have Rust tests
// and need the owner's native check.
async (page)=>{
 const reference=await page.context().newPage();
 await reference.route('https://**/*',route=>route.abort());
 await reference.setViewportSize({width:1200,height:900});
 await reference.goto('http://127.0.0.1:3141/motetto-app-mockup.html?screen=onboarding');
 await reference.getByRole('button',{name:'macOS',exact:true}).click();
 await reference.getByRole('button',{name:'First run',exact:true}).first().click();
 const props=['paddingTop','paddingRight','paddingBottom','paddingLeft','rowGap','columnGap','fontSize','fontWeight','fontFamily','lineHeight','color','backgroundColor','borderTopWidth','borderTopColor','borderRightWidth','borderRightColor','borderTopLeftRadius','boxShadow','display','alignItems','opacity'];
 const read=(target,selector)=>target.locator(selector).first().evaluate((el,props)=>Object.fromEntries(props.map(k=>[k,getComputedStyle(el)[k]])),props);
 async function compare(p,selectors,diffs){
  for(const selector of selectors){
   const actual=await read(p.locator('.onb'),selector),expected=await read(reference.locator('.onb'),selector);
   for(const key of props)if(actual[key]!==expected[key])diffs.push(`${selector} ${key}: ${actual[key]} != ${expected[key]}`);
  }
 }
 const results=[];
 for(const theme of ['light','dark']){
  const p=await page.context().newPage();const errors=[];p.on('pageerror',e=>errors.push(e.message));
  await p.setViewportSize({width:740,height:660});await p.emulateMedia({colorScheme:theme,reducedMotion:'reduce'});
  await reference.emulateMedia({colorScheme:theme});
  await reference.reload();await reference.getByRole('button',{name:'macOS',exact:true}).click();
  await reference.evaluate(theme=>document.documentElement.setAttribute('data-theme',theme),theme);
  await p.addInitScript(()=>{
   const file=(id,kind,label,bytes)=>({id,kind,label,file:id,url:'',bytes,sha256:'',on_disk:false});
   const catalog={vad:'vad-silero',files:[file('speech-large-v3','Speech','large-v3',1081140203),file('speech-large-v3-turbo','Speech','large-v3-turbo',574041195),file('speech-small','Speech','small',487601967),file('vad-silero','Vad','Voice detection',885098),file('notes-qwen3-8b','Notes','Qwen3 8B',5027783488),file('notes-qwen3.5-4b','Notes','Qwen3.5 4B',2740937888),file('notes-qwen3.5-2b','Notes','Qwen3.5 2B',1280835840)],
    tiers:[{id:'Best',speech:'speech-large-v3',notes:'notes-qwen3-8b',suits_mac:'16 GB memory or more',suits_pc:'GPU with 8 GB+ VRAM'},{id:'Balanced',speech:'speech-large-v3-turbo',notes:'notes-qwen3.5-4b',suits_mac:'8 GB memory',suits_pc:'GPU with 4-8 GB VRAM, or no GPU and 16 GB+ RAM'},{id:'Light',speech:'speech-small',notes:'notes-qwen3.5-2b',suits_mac:'Only if the speed test fails',suits_pc:'No usable GPU and under 16 GB RAM'}]};
   const saved=JSON.parse(sessionStorage.getItem('fx')||'null');
   const fx=window.fx=saved||{perms:{microphone:'Allowed',system_audio:'Denied',notifications:'NotAsked'},onDisk:[],
    settings:{onboarding_step:0,onboarding_done:false,consent_confirmed_at:null,tier:null,speech_path:''},
    setup:{tier:'Best',phase:{type:'Idle'},files:[['speech-large-v3',0,1081140203],['vad-silero',0,885098],['notes-qwen3-8b',0,5027783488]],speed:null,keep_slow:false}};
   window.saveFx=()=>sessionStorage.setItem('fx',JSON.stringify(fx));
   let next=0;const callbacks={},listeners={};window.calls=[];
   window.emit=payload=>{const id=listeners['motetto://event'];if(id)callbacks[id]?.({event:'motetto://event',id:0,payload});};
   window.setup=change=>{Object.assign(fx.setup,change);window.emit({type:'SetupChanged',...fx.setup});};
   window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener:()=>{}};
   window.__TAURI_INTERNALS__={metadata:{currentWindow:{label:'onboarding'},currentWebview:{windowLabel:'onboarding',label:'onboarding'}},transformCallback:cb=>{callbacks[++next]=cb;return next;},unregisterCallback:id=>delete callbacks[id],invoke:async(c,a)=>{
    window.calls.push({c,a});
    if(c==='plugin:event|listen'){listeners[a.event]=a.handler;return a.handler;}
    if(c==='plugin:event|unlisten')return;
    if(c==='get_settings')return structuredClone(fx.settings);
    if(c==='permissions')return structuredClone(fx.perms);
    if(c==='request_permission'){const key={Microphone:'microphone',SystemAudio:'system_audio',Notifications:'notifications'}[a.kind];fx.perms[key]='Allowed';return 'Allowed';}
    if(c==='open_permission_settings'||c==='relaunch'||c==='pause_download'||c==='answer_slow'||c==='finish_onboarding')return null;
    if(c==='hardware_info')return {chip:'M1 Max',memory_gb:32,suggested:'Best',label:'M1 Max, 32 GB'};
    if(c==='model_catalog')return {...catalog,files:catalog.files.map(f=>({...f,on_disk:fx.onDisk.includes(f.id)}))};
    if(c==='setup_state')return structuredClone(fx.setup);
    if(c==='start_setup'){fx.settings.tier=a.tier;return null;}
    if(c==='set_onboarding_step'){fx.settings.onboarding_step=a.step;if(a.consent)fx.settings.consent_confirmed_at='1';return null;}
    if(c==='test_recording')return {text:'Testing one two three.'};
    throw Error(`Unexpected IPC: ${c}`);
   }};
  });
  await p.goto('http://127.0.0.1:3004/onboarding/');
  const pane=p.locator('.onbpane'),diffs=[];
  const count=c=>p.evaluate(c=>window.calls.filter(x=>x.c===c).length,c);
  const expect=async(ok,message)=>{if(!await ok)throw Error(`${theme}: ${message}`);};

  // Step 1: Continue waits for consent; ticking saves it.
  await p.getByRole('heading',{name:'Meeting notes that stay on this Mac'}).waitFor();
  await p.getByText('Step 1 of 4',{exact:true}).waitFor();
  await expect(p.getByRole('button',{name:'Continue'}).isDisabled(),'Continue enabled before consent');
  await expect(p.locator('.onbnav button').nth(1).isDisabled(),'Permissions reachable before consent');
  await compare(p,['.onbnav','.onbnav button[aria-current="step"]','.onbnav button[disabled]','.onbnav .n','.onbpane','.onbpane h3','.onbpane p','.eyebrow','.promise div','.promise b','.consent','.consent input','.foot','.foot .tip','.btn.primary'],diffs);
  await p.screenshot({path:`.playwright-out/onboarding-welcome-${theme}.png`});
  await reference.locator('.onb').screenshot({path:`.playwright-out/onboarding-welcome-reference-${theme}.png`});
  await p.getByRole('checkbox').check();
  await p.waitForFunction(()=>window.calls.some(x=>x.c==='set_onboarding_step'&&x.a.step===0&&x.a.consent===true));
  await expect(p.getByRole('button',{name:'Continue'}).isEnabled(),'Continue stays disabled after consent');
  await reference.locator('#consent').check();
  await p.getByRole('button',{name:'Continue'}).hover();await reference.locator('[data-next]').hover();
  await compare(p,['.btn.primary:hover'],diffs);
  await reference.locator('[data-next]').click();

  // Step 2: rows follow permissions(); focus and Continue read them again.
  await p.getByRole('button',{name:'Continue'}).click();
  await p.getByRole('heading',{name:'Three quick permissions'}).waitFor();
  await p.waitForFunction(()=>window.calls.some(x=>x.c==='set_onboarding_step'&&x.a.step===1));
  const row=name=>pane.locator('.perm').filter({hasText:name});
  await row('Microphone').getByText('Allowed',{exact:true}).waitFor();
  await row('Screen & System Audio Recording').getByText("Off, so Motetto can't hear the other side of the call. Turn it on; Motetto then reopens itself.").waitFor();
  await expect(row('Screen & System Audio Recording').evaluate(el=>el.classList.contains('denied')),'Denied screen row lacks .denied');
  await compare(p,['.perm','.perm.denied','.perm .pi','.perm .pi svg','.perm b','.perm small','.perm .btn.sm','.perm .chip','.btn.ghost'],diffs);
  await p.screenshot({path:`.playwright-out/onboarding-permissions-${theme}.png`});
  await reference.locator('.onb').screenshot({path:`.playwright-out/onboarding-permissions-reference-${theme}.png`});
  await row('Notifications').getByRole('button',{name:'Allow notifications'}).click();
  await row('Notifications').getByText('Allowed',{exact:true}).waitFor();
  await p.evaluate(()=>{fx.perms.microphone='Denied';window.dispatchEvent(new Event('focus'));});
  await row('Microphone').getByRole('button',{name:'Open System Settings'}).waitFor();
  await p.evaluate(()=>{fx.perms.microphone='NotAsked';window.dispatchEvent(new Event('focus'));});
  await row('Microphone').getByRole('button',{name:'Allow microphone'}).click();
  await row('Microphone').getByText('Allowed',{exact:true}).waitFor();
  await row('Screen & System Audio Recording').getByRole('button',{name:'Open System Settings'}).click();
  await p.waitForFunction(()=>window.calls.some(x=>x.c==='open_permission_settings'&&x.a.kind==='SystemAudio'));
  await p.evaluate(()=>{fx.perms.system_audio='Allowed';window.dispatchEvent(new Event('focus'));});
  await row('Screen & System Audio Recording').getByText('Motetto needs to reopen to use this permission.',{exact:true}).waitFor();
  await row('Screen & System Audio Recording').getByRole('button',{name:'Reopen Motetto'}).click();
  await p.waitForFunction(()=>window.calls.some(x=>x.c==='relaunch'));
  await reference.locator('[data-next]').click();

  // Step 3: tiers, download progress, speed chip, notes in the background.
  const before={perms:await count('permissions'),setup:await count('setup_state')};
  await p.getByRole('button',{name:'Continue'}).click();
  await p.getByRole('heading',{name:'Models for this Mac'}).waitFor();
  await expect(count('permissions').then(n=>n>before.perms),'Continue did not re-read permissions');
  await expect(count('setup_state').then(n=>n>before.setup),'Continue did not re-read setup_state');
  await p.getByText('This Mac has an M1 Max with 32 GB of memory. Motetto suggests Best.',{exact:false}).waitFor();
  await expect(p.getByRole('radio',{name:/^Best/}).getAttribute('aria-checked').then(v=>v==='true'),'Suggested tier not selected');
  await expect(p.getByRole('button',{name:'Continue'}).isDisabled(),'Continue enabled before Download');
  await compare(p,['.tiers','.tier','.tier[aria-checked="true"]','.tier b','.tier small','.tier .chip.ok','.dl','.dlrow','.dlrow .progress','.dlrow .progress i','.note','.controls'],diffs);
  await p.screenshot({path:`.playwright-out/onboarding-models-${theme}.png`});
  await reference.locator('.onb').screenshot({path:`.playwright-out/onboarding-models-reference-${theme}.png`});
  // Try it waits for the speech model.
  await p.locator('.onbnav button').nth(3).click();
  await expect(p.getByRole('button',{name:/Record test/}).isDisabled(),'Record test enabled before the speech model');
  await p.getByText('Available when the speech model has downloaded.',{exact:true}).waitFor();
  await p.locator('.onbnav button').nth(2).click();
  await p.getByRole('button',{name:'Download 6.1 GB'}).click();
  await p.waitForFunction(()=>window.calls.some(x=>x.c==='start_setup'&&x.a.tier==='Best'));
  await expect(p.getByRole('button',{name:'Continue'}).isEnabled(),'Continue disabled after Download');
  const dlrow=name=>pane.locator('.dlrow').filter({hasText:name});
  await p.evaluate(()=>{window.setup({phase:{type:'Downloading',id:'speech-large-v3'}});window.emit({type:'DownloadProgress',id:'speech-large-v3',bytes:540570101,total:1081140203});});
  await dlrow('Speech model').getByText('50%',{exact:true}).waitFor();
  await p.getByRole('button',{name:'Pause'}).click();
  await p.waitForFunction(()=>window.calls.some(x=>x.c==='pause_download'));
  await p.evaluate(()=>{fx.setup.files[0][1]=540570101;window.setup({phase:{type:'Paused'}});});
  await p.getByText('Download paused. It continues from where it stopped.',{exact:true}).waitFor();
  const starts=await count('start_setup');
  await p.getByRole('button',{name:'Resume download'}).click();
  await expect(count('start_setup').then(n=>n>starts),'Resume did not call start_setup');
  await p.evaluate(()=>{window.setup({phase:{type:'Downloading',id:'speech-large-v3'}});window.emit({type:'DownloadProgress',id:'speech-large-v3',bytes:1081140203,total:1081140203});});
  await p.waitForTimeout(100);
  await expect(dlrow('Speech model').locator('.mono').textContent().then(t=>t!=='✓'),'Full .part shown as done before verification');
  await p.evaluate(()=>{fx.onDisk.push('speech-large-v3','vad-silero');fx.setup.files[0][1]=1081140203;fx.setup.files[1][1]=885098;window.emit({type:'DownloadDone',id:'speech-large-v3'});window.setup({phase:{type:'SpeedTest'}});});
  await dlrow('Speech model').locator('.mono').getByText('✓',{exact:true}).waitFor();
  await p.getByText('Running a 10-second speed test…',{exact:true}).waitFor();
  const reads=await count('setup_state');
  await p.evaluate(()=>window.emit({type:'SpeedTest',tier:'Light',rtf:0.9,keeps_up:false,measured_at:'1'}));
  await p.waitForFunction(n=>window.calls.filter(x=>x.c==='setup_state').length>n,reads);
  await expect(p.getByText(/0.90×/).count().then(n=>n===0),'Another tier\'s speed test shown as the answer');
  await p.evaluate(()=>{fx.setup.speed={tier:'Best',rtf:0.09,keeps_up:true,measured_at:'1'};window.emit({type:'SpeedTest',...fx.setup.speed});});
  await p.getByText('Speed test: 0.09× real time · keeps up live',{exact:true}).waitFor();
  await p.evaluate(()=>{window.setup({phase:{type:'Downloading',id:'notes-qwen3-8b'}});window.emit({type:'DownloadProgress',id:'notes-qwen3-8b',bytes:2011113395,total:5027783488});});
  await dlrow('Notes model (in the background)').getByText('40%',{exact:true}).waitFor();
  // A Settings download while paused leaves setup Idle: Resume still shows.
  await p.evaluate(()=>{fx.setup.files[2][1]=2011113395;window.setup({phase:{type:'Idle'}});});
  await p.getByRole('button',{name:'Resume download'}).click();
  await p.waitForFunction(n=>window.calls.filter(x=>x.c==='start_setup').length>n,starts+1);
  await p.evaluate(()=>window.setup({phase:{type:'Downloading',id:'notes-qwen3-8b'}}));
  await p.getByRole('button',{name:'Pause'}).waitFor();
  await p.screenshot({path:`.playwright-out/onboarding-models-downloading-${theme}.png`});
  if(await p.evaluate(()=>document.documentElement.scrollWidth>innerWidth||document.querySelector('.onbpane').scrollWidth>document.querySelector('.onbpane').clientWidth))throw Error(`${theme}: Models overflows at 740×660`);
  // The mockup's Continue waits for its simulated speed test; its nav reaches the same step.
  await reference.evaluate(()=>{document.querySelectorAll('#onbnav button').forEach(b=>b.disabled=false);document.querySelector('#onbnav [data-step="3"]').click();});

  // Step 4: test recording, slow answer, notes pending, Done.
  await p.getByRole('button',{name:'Continue'}).click();
  await p.getByRole('heading',{name:'Say something to test it'}).waitFor();
  await p.getByText("The notes model is still downloading; notes start working when it's done.",{exact:false}).waitFor();
  await compare(p,['.testbox','.testbox .tip','.testbox .btn'],diffs);
  await p.screenshot({path:`.playwright-out/onboarding-try-${theme}.png`});
  await reference.locator('.onb').screenshot({path:`.playwright-out/onboarding-try-reference-${theme}.png`});
  await p.evaluate(()=>{fx.setup.speed={tier:'Best',rtf:0.8,keeps_up:false,measured_at:'2'};window.setup({phase:{type:'AwaitingAnswer',offer:'Balanced'}});});
  await p.getByText('This computer transcribed at 0.80× real time; live notes need 0.5× or faster.',{exact:true}).waitFor();
  await p.getByRole('button',{name:'Keep Best'}).waitFor();
  await p.getByRole('button',{name:'Use Balanced'}).click();
  await p.waitForFunction(()=>window.calls.some(x=>x.c==='answer_slow'&&x.a.useLower===true));
  await p.getByRole('button',{name:/Record test/}).click();
  await p.locator('.testbox q').getByText('Testing one two three.').waitFor();
  await p.getByText('It heard you. The microphone and speech model work.',{exact:true}).waitFor();
  await p.getByRole('button',{name:'Record again'}).waitFor();
  await reference.locator('#test').click();
  await compare(p,['.testbox q','.testbox .chip.ok'],diffs);
  await p.screenshot({path:`.playwright-out/onboarding-try-heard-${theme}.png`});
  await reference.locator('.onb').screenshot({path:`.playwright-out/onboarding-try-heard-reference-${theme}.png`});
  await p.getByRole('button',{name:'Done'}).click();
  await p.waitForFunction(()=>window.calls.some(x=>x.c==='finish_onboarding'));

  // reopen_reads_state: no events after a reload; setup_state says notes at 60 %.
  await p.evaluate(()=>{fx.settings.onboarding_step=2;fx.settings.consent_confirmed_at='1';fx.settings.tier='Best';fx.onDisk=['speech-large-v3','vad-silero'];
   fx.setup={tier:'Best',phase:{type:'Paused'},files:[['speech-large-v3',1081140203,1081140203],['vad-silero',885098,885098],['notes-qwen3-8b',3016670093,5027783488]],speed:{tier:'Best',rtf:0.09,keeps_up:true,measured_at:'1'},keep_slow:false};window.saveFx();});
  await p.reload();
  await p.getByRole('heading',{name:'Models for this Mac'}).waitFor();
  await dlrow('Notes model (in the background)').getByText('60%',{exact:true}).waitFor();
  await expect(dlrow('Notes model (in the background)').locator('.mono').textContent().then(t=>t!=='✓'),'Notes row shown as done');
  await p.getByRole('button',{name:'Resume download'}).waitFor();
  await expect(p.getByRole('button',{name:'Continue'}).isEnabled(),'Continue disabled after reopening a started setup');
  await p.evaluate(()=>sessionStorage.removeItem('fx'));

  if(errors.length)throw Error(errors.join('\n'));
  if(diffs.length)throw Error(diffs.join('\n'));
  results.push({theme,differences:diffs.length,interactions:'passed'});
  await p.close();
 }
 await reference.close();return results;
}
