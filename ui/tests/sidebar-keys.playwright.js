// Sidebar arrows follow displayed navigation entries and open their page.
async (page) => {
 const results=[];
 for(const theme of ['light','dark']){
  const p=await page.context().newPage();p.setDefaultTimeout(7000);
  await p.emulateMedia({colorScheme:theme});
  await p.addInitScript(() => {
    const mk = (id, title, notes) => ({ summary: { id, title, created_ms: Date.now() - +id * 6e5, status: 'Ready', marks_ms: [], duration_ms: 87000, notes_ms: 48000, spellings: [], locked: false }, transcript: '[00:01] Me: hi', transcript_heard: '', notes_md: notes });
    const ms = [mk('1', 'Recorded meeting', '### Action items\n- [ ] Me: Send the budget proposal by **Friday** [02:15]\n- [ ] Anna: Review the draft [02:20]\n'), mk('2', 'Daily Meeting', '### Action items\n- [ ] Me: Brush my teeth [01:00]\n- [x] Me: Done thing\n')];
    let next = 0; window.ms = ms;
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = { convertFileSrc: x => x, transformCallback: () => ++next, unregisterCallback: () => {}, invoke: async (c, a) => {
      if (c === 'plugin:event|listen') return next; if (c === 'get_state') return { state: { type: 'Idle' }, meeting_id: null, companion: { visible: false, size: 'Pill' }, elapsed_ms: 0, quit_pending: false };
      if (c === 'list_meetings') return ms.map(m => m.summary); if (c === 'deleted_meetings') return [{...ms[0].summary,id:'gone',title:'Deleted meeting'}]; if (c === 'get_meeting') return ms.find(m => m.summary.id === a.id);
      if (c === 'search_meetings') return ms.filter(m=>m.summary.title.toLowerCase().includes(a.query.toLowerCase())).map(m=>({meeting:m.summary,snippet:null}));
      if (c === 'open_action_items') return ms.map(m => [m.summary, m.notes_md]);
      if (c === 'set_action_item_done') { const m = ms.find(m => m.summary.id === a.id); const l = m.notes_md.split('\n'); l[a.line] = l[a.line].replace('[ ]', a.done ? '[x]' : '[ ]'); m.notes_md = l.join('\n'); return m.notes_md; }
      if (c === 'meeting_media') return { tracks: [], duration_ms: 87000, folder: 'x' }; if (c === 'library_bytes') return 1000; return false; } };
  });

  const ref=await page.context().newPage();ref.setDefaultTimeout(7000);
  await ref.route('https://**/*',r=>r.abort());
  await ref.goto('http://127.0.0.1:3001/motetto-app-mockup.html');
  await ref.getByRole('button',{name:'macOS',exact:true}).click();
  await ref.getByRole('button',{name:'After the call',exact:true}).click();
  await ref.evaluate(t=>document.documentElement.dataset.theme=t,theme);
  await p.goto('http://127.0.0.1:3000/main/',{waitUntil:'domcontentloaded'});
  for(const [target,side,meeting,current] of [[p,'.sidebar','.meeting-row','page'],[ref,'.side','.mitem','true']]){
   const rows=target.locator(`${side} .homerow, ${side} ${meeting}, ${side} .trash`);
   await rows.first().waitFor();
   for(const width of [760,1200]){
    await target.setViewportSize({width,height:720});
    await rows.first().click();
    const check=async(index)=>{
     await target.waitForFunction(({side,meeting,index,current})=>{
      const rows=[...document.querySelectorAll(`${side} .homerow, ${side} ${meeting}, ${side} .trash`)];
      return rows[index]===document.activeElement&&rows[index]?.getAttribute('aria-current')===current;
     },{side,meeting,index,current});
    };
    await target.keyboard.press('ArrowUp');await check(0);
    await target.keyboard.press('ArrowDown');await check(1);
    if(await rows.nth(1).evaluate(e=>getComputedStyle(e).boxShadow.includes('inset')))throw Error('Sidebar focus draws a ring');
    const title=await rows.nth(1).locator('strong,.mt').innerText();
    if(!(await target.locator(target===p?'.workspace':'#doc').innerText()).includes(title))throw Error('Detail did not open: '+title);
    await target.keyboard.press('ArrowDown');await check(2);
    await target.keyboard.press('ArrowUp');await check(1);
    await target.keyboard.press('ArrowUp');await check(0);
    const count=await rows.count();
    for(let i=0;i<count+2;i++)await target.keyboard.press('ArrowDown');
    await check(count-1);
    if(await rows.nth(count-1).evaluate(e=>e.matches('.trash')) && !(await target.locator(target===p?'.workspace':'#doc').innerText()).includes('Recently deleted'))throw Error('Trash did not open');
    await target.keyboard.press('ArrowUp');await check(count-2);
    await target.locator(side).screenshot({path:`.playwright-out/sidebar-keys-${target===p?'app':'mock'}-${theme}-${width}.png`});
   }
  }
  // Search keeps caret movement; filtered meeting navigation uses only visible rows.
  const search=p.locator('.search input');await search.fill('Daily');
  await p.waitForFunction(()=>document.querySelectorAll('.meeting-row').length===1);
  await search.press('ArrowDown');if(!await search.evaluate(e=>e===document.activeElement))throw Error('Sidebar arrows stole search focus');
  await p.locator('.homerow').click();await p.keyboard.press('ArrowDown');
  await p.waitForFunction(()=>document.querySelector('.meeting-row')===document.activeElement&&document.activeElement?.getAttribute('aria-current')==='page');
  await p.close();await ref.close();results.push(theme+' PASS');
 }
 return results;
}
