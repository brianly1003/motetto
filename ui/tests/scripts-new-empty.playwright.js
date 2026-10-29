// One blank script at a time (owner 2026-10-30, SPEC 14.4): New script while a script is still
// empty opens that one with a hint instead of making another; fast repeated clicks make one
// script; once it has words, New script makes the next one.
async (page) => {
  const fails = [];
  const p = await page.context().newPage(); p.on('pageerror', e => fails.push(e.message));
  await p.setViewportSize({ width: 1200, height: 760 });
  await p.addInitScript(() => {
    const files = { 'Team X demo': { text: '# Opening (1m)\nHello.', ms: 1000 } };
    let next = 0; const callbacks = {};
    window.calls = []; window.files = files;
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = { metadata: { currentWindow: { label: 'main' }, currentWebview: { windowLabel: 'main', label: 'main' } }, convertFileSrc: x => x, transformCallback: cb => { const id = ++next; callbacks[id] = cb; return id; }, unregisterCallback: id => { delete callbacks[id]; }, invoke: async (c, a) => {
      if (c === 'plugin:event|listen') return a.handler;
      if (c === 'get_state') return { state: { type: 'Idle' }, meeting_id: null, companion: { visible: false, size: 'Pill' }, elapsed_ms: 0, quit_pending: false, share_visible: false };
      if (c === 'list_meetings' || c === 'deleted_meetings' || c === 'open_action_items') return [];
      if (c === 'scripts_list') return Object.entries(files).map(([name, f]) => ({ name, modified_ms: f.ms, bytes: f.text.length })).sort((x, y) => y.modified_ms - x.modified_ms);
      if (c === 'get_settings') return { notes_folder: '/Users/someone/Motetto', prompter: { layout: 'Strip', text_size: 22, last_script: null }, shortcuts: { T: 'Ctrl+Alt+T' } };
      if (c === 'script_read') return files[a.name].text;
      if (c === 'script_save') { await new Promise(resolve => setTimeout(resolve, 80)); window.calls.push(`save:${a.name}:${a.baseModifiedMs}`); if (a.baseModifiedMs === null && files[a.name]) throw 'A script with this name already exists.'; files[a.name] = { text: a.text, ms: Date.now() }; return files[a.name].ms; }
      return false; } };
  });
  await p.goto('http://127.0.0.1:3000/main/');
  await p.getByRole('button', { name: 'Scripts' }).first().click();
  const list = p.getByRole('navigation', { name: 'Scripts' });
  await list.waitFor();
  const newScript = list.getByRole('button', { name: /New script/ });
  const created = () => p.evaluate(() => window.calls.filter(c => c.endsWith(':null')).length);
  // Five fast clicks make one script, opened with the caret in its editor.
  for (let n = 0; n < 5; n++) await newScript.click({ delay: 0 });
  await p.waitForFunction(() => document.querySelector('.sp-name')?.value === 'Untitled script');
  await p.waitForTimeout(400);
  if (await created() !== 1) fails.push('fast clicks made ' + await created());
  if (!(await p.evaluate(() => document.activeElement?.matches('.sp-editor textarea')))) fails.push('new script editor not focused');
  // Pressing again while it is empty: nothing new, the hint and the tooltip say why.
  await newScript.click();
  await p.locator('.sp-emptyhint').waitFor();
  if (await created() !== 1) fails.push('second empty script');
  if (!/is still empty/.test(await newScript.getAttribute('title') || '')) fails.push('tooltip: ' + await newScript.getAttribute('title'));
  // From another script, New script goes back to the empty one.
  await list.getByRole('button', { name: /Team X demo/ }).click();
  await p.waitForFunction(() => document.querySelector('.sp-name')?.value === 'Team X demo');
  await newScript.click();
  await p.waitForFunction(() => document.querySelector('.sp-name')?.value === 'Untitled script');
  await p.locator('.sp-emptyhint').waitFor();
  if (await created() !== 1) fails.push('empty script elsewhere ignored');
  // Duplicate of an empty script is another empty script: it opens this one instead.
  await p.getByRole('button', { name: 'More for this script' }).click();
  await p.getByRole('menuitem', { name: 'Duplicate' }).click();
  await p.waitForTimeout(300);
  if (await created() !== 1) fails.push('duplicated an empty script');
  await p.screenshot({ path: '.playwright-out/scripts-new-empty.png' });
  // Words make it a script; the hint goes and the next New script is a new one.
  await p.locator('.sp-editor textarea').fill('# Opening (1m)\nHi.');
  if (await p.locator('.sp-emptyhint').count()) fails.push('hint stayed after typing');
  await p.waitForTimeout(500);
  await newScript.click();
  await p.waitForFunction(() => document.querySelector('.sp-name')?.value === 'Untitled script 2');
  if (await created() !== 2) fails.push('no new script after typing');
  if (fails.length) throw Error(fails.join('; '));
  await p.close();
  return 'ok';
}
