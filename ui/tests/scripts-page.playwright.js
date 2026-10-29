// Scripts (owner 2026-10-28, SPEC 14.4): the library's Scripts entry lists the scripts, edits one
// with a save 300 ms after typing, renames it from its title, creates "Untitled script", and
// Present opens the prompter with that script.
async (page) => {
  const fails = [];
  const p = await page.context().newPage(); p.on('pageerror', e => fails.push(e.message));
  await p.setViewportSize({ width: 1200, height: 760 });
  await p.addInitScript(() => {
    const files = { 'Team X demo': { text: '# Opening (1m)\nHello.\n\n# Demo (5m)\nThis is it.', ms: 1000 }, 'Quarterly review': { text: 'Q', ms: 500 } };
    let next = 0; const callbacks = {}, listeners = {};
    window.calls = []; window.files = files;
    window.emitEvent = (event, payload) => (listeners[event] || []).forEach(id => callbacks[id]?.({ event, id, payload }));
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = { metadata: { currentWindow: { label: 'main' }, currentWebview: { windowLabel: 'main', label: 'main' } }, convertFileSrc: x => x, transformCallback: cb => { const id = ++next; callbacks[id] = cb; return id; }, unregisterCallback: id => { delete callbacks[id]; }, invoke: async (c, a) => {
      if (c === 'plugin:event|listen') { (listeners[a.event] ||= []).push(a.handler); return a.handler; }
      if (c === 'get_state') return { state: { type: 'Idle' }, meeting_id: null, companion: { visible: false, size: 'Pill' }, elapsed_ms: 0, quit_pending: false, share_visible: false };
      if (c === 'list_meetings' || c === 'deleted_meetings' || c === 'open_action_items') return [];
      if (c === 'scripts_list') return Object.entries(files).map(([name, f]) => ({ name, modified_ms: f.ms, bytes: f.text.length })).sort((x, y) => y.modified_ms - x.modified_ms);
      if (c === 'get_settings') return { notes_folder: '/Users/someone/Motetto', prompter: { layout: 'Strip', text_size: 22, last_script: 'Quarterly review' }, shortcuts: { T: 'Ctrl+Alt+T' } };
      if (c === 'script_read') return files[a.name].text;
      if (c === 'script_save') { if(window.saveDelay) await new Promise(resolve => setTimeout(resolve, window.saveDelay)); window.calls.push(`save:${a.name}:${a.baseModifiedMs}`); if (a.baseModifiedMs === null && files[a.name]) throw 'A script with this name already exists.'; if (a.baseModifiedMs !== null && a.baseModifiedMs !== files[a.name]?.ms) throw 'This script changed in another window.'; files[a.name] = { text: a.text, ms: (files[a.name]?.ms ?? 0) + 1 }; return files[a.name].ms; }
      if (c === 'script_rename') { files[a.to] = files[a.from]; delete files[a.from]; window.calls.push(`rename:${a.from}:${a.to}`); return null; }
      window.calls.push(c + (a && Object.keys(a).length ? ':' + JSON.stringify(a) : ''));
      return false; } };
  });
  await p.goto('http://127.0.0.1:3000/main/');
  await p.getByRole('button', { name: 'Scripts' }).first().click();
  const list = p.getByRole('navigation', { name: 'Scripts' });
  await list.waitFor();
  const rows = await list.getByRole('button').allInnerTexts();
  if (!/^Team X demo/.test(rows[0]) || !/Quarterly review/.test(rows[1]) || !/New script/.test(rows[2])) fails.push('rows: ' + JSON.stringify(rows));
  // Many scripts scroll in their own short list, so the meetings below stay in reach.
  if (await list.locator('.sp-subscroll').evaluate(e => getComputedStyle(e).maxHeight + ' ' + getComputedStyle(e).overflowY + ' ' + getComputedStyle(e).gridAutoRows) !== '189px auto max-content') fails.push('script list does not scroll on its own');
  // As the mockup: the meta line, the outline of time to say, the syntax legend and the path.
  const meta = await p.locator('.sp-meta').innerText();
  if (!/^2 sections · about 0:0\d to say · 6:00 planned · Saved$/.test(meta)) fails.push('meta: ' + meta);
  const outline = await p.locator('.sp-outline .sp-sec').allInnerTexts();
  if (outline.length !== 2 || !/Opening/.test(outline[0]) || !/\/ 1:00/.test(outline[0]) || !/\/ 5:00/.test(outline[1])) fails.push('outline: ' + JSON.stringify(outline));
  if ((await p.locator('.sp-legend code').count()) !== 3) fails.push('legend');
  if (!/~\/Motetto\/Scripts\/Team X demo\.md · on this (Mac|PC) only/.test(await p.locator('.sp-foot').innerText())) fails.push('foot: ' + await p.locator('.sp-foot').innerText());
  // The script ⌃⌥T opens carries the key in the list; Present moves it.
  const shownRow = () => p.evaluate(() => document.querySelector('#scripts-sub [data-shown]')?.textContent);
  if (!/^Quarterly review/.test(await shownRow() || '')) fails.push('shown key: ' + await shownRow());
  // ••• : Rename, Duplicate, Show in Finder or Explorer, Delete script.
  await p.getByRole('button', { name: 'More for this script' }).click();
  const items = await p.getByRole('menuitem').allInnerTexts();
  if (items.join('|') !== 'Rename|Duplicate|Show in Finder|Delete script' && items.join('|') !== 'Rename|Duplicate|Show in Explorer|Delete script') fails.push('menu: ' + items.join('|'));
  await p.getByRole('menuitem', { name: /Show in/ }).click();
  if (!(await p.evaluate(() => window.calls)).includes('script_reveal')) fails.push('Show in Finder did nothing');
  const box = p.getByRole('textbox', { name: 'Team X demo' });
  if (!/# Opening/.test(await box.inputValue())) fails.push('script text not loaded');
  await box.press('End'); await box.type('\n\nMore.');
  await p.waitForTimeout(500);
  if (!(await p.evaluate(() => window.calls)).includes('save:Team X demo:1000')) fails.push('no save with the loaded time: ' + JSON.stringify(await p.evaluate(() => window.calls)));
  // Reopening a script must use its current disk version, not the sidebar's initial time.
  await list.getByRole('button', { name: /^Quarterly review/ }).click();
  await p.getByRole('textbox', {name:'Quarterly review',exact:true}).waitFor();
  await list.getByRole('button', { name: /^Team X demo/ }).click();
  await box.waitFor();
  await box.fill((await box.inputValue())+' Again.');
  await p.waitForTimeout(500);
  if (!(await p.evaluate(() => window.calls)).includes('save:Team X demo:1001')) fails.push('reopened script kept a stale version');
  if (await p.getByText('This script changed in another window.',{exact:true}).count()) fails.push('false conflict after reopening');
  // A slow save must not make newer typing look saved or reuse the old version.
  await p.evaluate(() => window.saveDelay=600);
  await box.fill('First delayed draft'); await p.waitForTimeout(350);
  await box.fill('Latest delayed draft'); await p.waitForTimeout(650);
  if (!/Saving/.test(await p.locator('.sp-meta').innerText())) fails.push('older completion marked new typing saved');
  await p.waitForFunction(() => window.files['Team X demo'].text==='Latest delayed draft');
  await p.evaluate(() => window.saveDelay=0);
  await p.getByRole('button', { name: 'Present' }).click();
  if (!(await p.evaluate(() => window.calls)).includes('prompter_show:{"name":"Team X demo"}')) fails.push('Present did not open the prompter');
  await p.evaluate(() => window.emitEvent('motetto://prompter-script', 'Team X demo'));
  await p.waitForTimeout(100);
  if (!/^Team X demo/.test(await shownRow() || '')) fails.push('shown key did not follow Present: ' + await shownRow());
  for (const theme of ['light', 'dark']) { await p.emulateMedia({ colorScheme: theme }); await p.screenshot({ path: `.playwright-out/scripts-page-${theme}.png` }); }
  await p.emulateMedia({ colorScheme: 'light' });
  const title = p.getByRole('textbox', { name: 'Script name' });
  await title.fill('Team X launch'); await title.press('Enter');
  await p.waitForTimeout(200);
  if (!(await p.evaluate(() => window.calls)).includes('rename:Team X demo:Team X launch')) fails.push('rename');
  await list.getByRole('button', { name: /New script/ }).click();
  await p.waitForTimeout(300);
  if (!(await p.evaluate(() => window.calls)).includes('save:Untitled script:null')) fails.push('new script');
  if (await title.inputValue() !== 'Untitled script') fails.push('new script not opened: ' + await title.inputValue());
  // Genuine changes elsewhere still block overwrites and leave the local draft in the editor.
  await p.evaluate(() => { window.files['Untitled script']={text:'Other window text',ms:9000}; });
  const fresh=p.getByRole('textbox',{name:'Untitled script',exact:true});
  await fresh.fill('Keep my unsaved draft');
  await p.getByText('This script changed in another window.',{exact:true}).waitFor();
  if(await fresh.inputValue()!=='Keep my unsaved draft') fails.push('conflict lost the local draft');
  if(await p.evaluate(()=>window.files['Untitled script'].text)!=='Other window text') fails.push('conflict overwrote the other window');
  if(!/Saving/.test(await p.locator('.sp-meta').innerText())) fails.push('failed save falsely says Saved');
  await p.close();
  return fails.length ? fails : 'ok';
}
