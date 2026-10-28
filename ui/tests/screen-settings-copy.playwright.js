// Compact Screen copy keeps every control and its privacy explanation visible.
async page => {
  const browser=await page.context().browser().browserType().launch({channel:'chrome'});
  const context=await browser.newContext();
  const results=[];
  try {
    for(const theme of ['light','dark']) {
      const p=await context.newPage(),ref=await context.newPage();
      const errors=[];p.on('pageerror',e=>errors.push(e.message));
      await p.emulateMedia({colorScheme:theme});
      await ref.emulateMedia({colorScheme:theme});
      await ref.route('https://**/*',r=>r.abort());
      await p.setViewportSize({width:960,height:760});
      await ref.setViewportSize({width:1400,height:1000});
      await p.addInitScript(()=>{
        const settings={appearance:'system',screen_offer_on_ask:true,screen_source:'call_window',screen_on_own_share:'ask',screen_preset:'sharp',screen_cursor:true,share_visible_default:false,share_reset_on_end:true,share_check:true};
        let id=0;window.savedSettings=[];
        window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener:()=>{}};
        window.__TAURI_INTERNALS__={transformCallback:()=>++id,unregisterCallback:()=>{},invoke:async(c,a)=>{
          if(c==='plugin:event|listen')return id;
          if(c==='get_settings')return {...settings};
          if(c==='save_settings'){Object.assign(settings,a.settings);window.savedSettings.push({...settings});return {...settings};}
          if(c==='call_apps')return [];
          if(c==='permissions')return {system_audio:'Allowed'};
          if(c==='about')return {platform:'macos',version:'0.1.0',licences:''};
          return null;
        }};
      });
      await p.goto('http://127.0.0.1:3014/settings/?tab=screen');
      await p.getByRole('checkbox',{name:'Check each share',exact:true}).waitFor();
      await ref.goto('http://127.0.0.1:3001/motetto-app-mockup.html');
      await ref.getByRole('button',{name:'macOS',exact:true}).click();
      await ref.getByRole('button',{name:'Settings',exact:true}).click();
      await ref.getByRole('tab',{name:'Screen',exact:true}).click();
      const panel=p.locator('.setpane');
      const dimensions=await panel.evaluate(e=>({visible:e.clientHeight,content:e.scrollHeight}));
      if(dimensions.content>dimensions.visible)throw Error(`${theme}: Screen needs scrolling: ${JSON.stringify(dimensions)}`);
      if(await p.locator('.formrow').count()!==9)throw Error('A Screen control was removed');
      const help=await p.locator('.help').allTextContents();
      if(!help.some(s=>s.includes('typing and notifications.'))||!help.some(s=>s.includes('never saved.')&&s.includes('script stays hidden.')))throw Error('Privacy details missing');
      for(const selector of ['.note','.formrow > .lbl','.help','.field select']) {
        const actual=await p.locator(selector).allTextContents(),expected=await ref.locator(`.set ${selector}`).allTextContents();
        if(JSON.stringify(actual)!==JSON.stringify(expected))throw Error(`${selector}: app and mockup text differs`);
      }
      await p.screenshot({path:`.playwright-out/screen-settings-compact-${theme}.png`});
      await ref.locator('.set').screenshot({path:`.playwright-out/screen-settings-compact-reference-${theme}.png`});
      await p.getByRole('checkbox',{name:'Show Motetto in shares',exact:true}).check();
      await p.waitForFunction(()=>window.savedSettings.some(s=>s.share_visible_default===true));
      await p.getByRole('checkbox',{name:'Hide after sharing',exact:true}).uncheck();
      await p.waitForFunction(()=>window.savedSettings.some(s=>s.share_reset_on_end===false));
      await p.getByRole('combobox',{name:'When sharing',exact:true}).selectOption('keep_window');
      await p.waitForFunction(()=>window.savedSettings.some(s=>s.screen_on_own_share==='keep_window'));
      await p.getByRole('combobox',{name:'Picture quality',exact:true}).selectOption('smaller');
      await p.waitForFunction(()=>window.savedSettings.some(s=>s.screen_preset==='smaller'));
      await p.setViewportSize({width:760,height:520});
      await p.getByText('Permission',{exact:true}).scrollIntoViewIfNeeded();
      if(await p.evaluate(()=>document.documentElement.scrollWidth>innerWidth))throw Error('Screen tab overflows horizontally');
      results.push({theme,...dimensions});
      if(errors.length)throw Error(errors.join('\n'));
      await p.close();await ref.close();
    }
    return {passed:true,results};
  } finally {await browser.close();}
}
