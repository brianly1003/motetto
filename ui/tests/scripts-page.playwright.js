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
    window.calls = [];
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = { metadata: { currentWindow: { label: 'main' }, currentWebview: { windowLabel: 'main', label: 'main' } }, convertFileSrc: x => x, transformCallback: cb => { const id = ++next; callbacks[id] = cb; return id; }, unregisterCallback: id => { delete callbacks[id]; }, invoke: async (c, a) => {
      if (c === 'plugin:event|listen') { (listeners[a.event] ||= []).push(a.handler); return a.handler; }
      if (c === 'get_state') return { state: { type: 'Idle' }, meeting_id: null, companion: { visible: false, size: 'Pill' }, elapsed_ms: 0, quit_pending: false, share_visible: false };
      if (c === 'list_meetings' || c === 'deleted_meetings' || c === 'open_action_items') return [];
      if (c === 'scripts_list') return Object.entries(files).map(([name, f]) => ({ name, modified_ms: f.ms, bytes: f.text.length })).sort((x, y) => y.modified_ms - x.modified_ms);
      if (c === 'script_read') return files[a.name].text;
      if (c === 'script_save') { window.calls.push(`save:${a.name}:${a.baseModifiedMs}`); if (a.baseModifiedMs === null && files[a.name]) throw 'A script with this name already exists.'; files[a.name] = { text: a.text, ms: (files[a.name]?.ms ?? 0) + 1 }; return files[a.name].ms; }
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
  if (!/2 sections/.test(rows[0])) fails.push('open script shows no section count: ' + rows[0]);
  const box = p.getByRole('textbox', { name: 'Team X demo' });
  if (!/# Opening/.test(await box.inputValue())) fails.push('script text not loaded');
  await box.press('End'); await box.type('\n\nMore.');
  await p.waitForTimeout(500);
  if (!(await p.evaluate(() => window.calls)).includes('save:Team X demo:1000')) fails.push('no save with the loaded time: ' + JSON.stringify(await p.evaluate(() => window.calls)));
  await p.getByRole('button', { name: 'Present' }).click();
  if (!(await p.evaluate(() => window.calls)).includes('prompter_show:{"name":"Team X demo"}')) fails.push('Present did not open the prompter');
  const title = p.getByRole('textbox', { name: 'Script name' });
  await title.fill('Team X launch'); await title.press('Enter');
  await p.waitForTimeout(200);
  if (!(await p.evaluate(() => window.calls)).includes('rename:Team X demo:Team X launch')) fails.push('rename');
  await list.getByRole('button', { name: /New script/ }).click();
  await p.waitForTimeout(300);
  if (!(await p.evaluate(() => window.calls)).includes('save:Untitled script:null')) fails.push('new script');
  if (await title.inputValue() !== 'Untitled script') fails.push('new script not opened: ' + await title.inputValue());
  await p.close();
  return fails.length ? fails : 'ok';
}
