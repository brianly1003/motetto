// Deleting an empty script selects an existing script, or returns to first use.
async page => {
  const results=[];
  for(const theme of ['light','dark']) for(const remaining of [true,false]) {
    const p=await page.context().newPage(), errors=[];
    p.on('pageerror',e=>errors.push(e.message));
    await p.setViewportSize({width:1200,height:760});
    await p.emulateMedia({colorScheme:theme});
    await p.addInitScript(({remaining})=>{
      const files={'Untitled script':{text:'',ms:1000},...(remaining?{'Talk':{text:'My existing talk.',ms:500}}:{})};
      let n=0;const callbacks={}, listeners={};window.calls=[];window.files=files;
      window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener(){}};
      window.__TAURI_INTERNALS__={metadata:{currentWindow:{label:'main'},currentWebview:{windowLabel:'main',label:'main'}},convertFileSrc:x=>x,transformCallback:f=>{callbacks[++n]=f;return n;},unregisterCallback(){},invoke:async(c,a)=>{
        if(c==='plugin:event|listen'){(listeners[a.event]||=[]).push(a.handler);return a.handler;}
        if(c==='get_state')return {state:{type:'Idle'},meeting_id:null,elapsed_ms:0,companion:{visible:false,size:'Pill'},share_visible:false};
        if(['list_meetings','deleted_meetings','open_action_items'].includes(c))return [];
        if(c==='get_settings')return {notes_folder:'/Users/demo/Motetto',prompter:{last_script:null,layout:'Strip',text_size:22},shortcuts:{T:'Ctrl+Alt+T'}};
        if(c==='scripts_list')return Object.entries(files).map(([name,f])=>({name,modified_ms:f.ms,bytes:f.text.length}));
        if(c==='script_read'){window.calls.push(['read',a.name]);if(!files[a.name])throw a.name+'.md: No such file or directory (os error 2)';return files[a.name].text;}
        if(c==='script_save'){
          window.calls.push(['save',a.name]);await new Promise(resolve=>setTimeout(resolve,100));
          if(!files[a.name]||files[a.name].ms!==a.baseModifiedMs)throw 'Stale or deleted script';
          files[a.name]={text:a.text,ms:files[a.name].ms+1};return files[a.name].ms;
        }
        if(c==='script_delete'){
          if(window.failDelete)throw 'Could not delete the script.';
          window.calls.push(['delete',a.name]);delete files[a.name];return;
        }
        return null;
      }};
    },{remaining});
    try{
      await p.goto('http://127.0.0.1:3000/main/');
      await p.getByRole('button',{name:/^Scripts/}).first().click();
      const editor=p.getByRole('textbox',{name:'Untitled script',exact:true});await editor.waitFor();
      // A failed delete leaves the original editor intact and usable.
      await p.evaluate(()=>window.failDelete=true);
      await p.getByRole('button',{name:'More for this script'}).click();
      await p.getByRole('menuitem',{name:'Delete script',exact:true}).click();
      await p.getByText('Could not delete the script.',{exact:true}).waitFor();
      if(await editor.inputValue()!=='')throw Error('Failed delete replaced the draft');
      await p.getByRole('button',{name:'Dismiss',exact:true}).click();
      await p.evaluate(()=>window.failDelete=false);
      // Leave a pending blank edit, so deletion must finish it before moving the file.
      await editor.fill(' ');
      await p.getByRole('button',{name:'More for this script'}).click();
      await p.getByRole('menuitem',{name:'Delete script',exact:true}).click();
      if(remaining){
        const next=p.getByRole('textbox',{name:'Talk',exact:true});await next.waitFor();
        if(await next.inputValue()!=='My existing talk.')throw Error('Wrong remaining script');
      }else await p.locator('.scripts-first-box').waitFor();
      await p.waitForTimeout(450);
      const calls=await p.evaluate(()=>window.calls),deleted=calls.findIndex(c=>c[0]==='delete');
      if(deleted<0||!calls.slice(0,deleted).some(c=>c[0]==='save'))throw Error('Pending save did not finish before delete');
      if(calls.slice(deleted+1).some(c=>c[1]==='Untitled script'))throw Error('Deleted script read or saved again: '+JSON.stringify(calls));
      if(await p.getByText('Something needs attention',{exact:true}).count())throw Error('Error after successful deletion');
      if(await p.evaluate(()=>!!window.files['Untitled script']))throw Error('Deleted script was recreated');
      if(errors.length)throw Error(errors.join('\n'));
      await p.screenshot({path:`.playwright-out/scripts-delete-${theme}-${remaining?'remaining':'last'}.png`});
      results.push({theme,remaining,failedDeletePreservesDraft:true,noStaleRead:true,noLateSave:true});
    }finally{await p.close();}
  }
  return results;
}
