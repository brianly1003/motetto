// Scripts, first use (owner 2026-10-28): with no scripts, Scripts shows one paste box and three
// steps, no empty list column and no button to find; pasting creates the script named from its
// first line and the editor keeps the caret, so typing carries on.
async (page) => {
  const fails = [];
  for (const theme of ['light', 'dark']) {
    const p = await page.context().newPage(); p.on('pageerror', e => fails.push(e.message));
    await p.emulateMedia({ colorScheme: theme });
    await p.setViewportSize({ width: 1200, height: 760 });
    await p.addInitScript(() => {
      const files = {};
      let next = 0; const callbacks = {};
      window.calls = [];
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
      window.__TAURI_INTERNALS__ = { metadata: { currentWindow: { label: 'main' }, currentWebview: { windowLabel: 'main', label: 'main' } }, convertFileSrc: x => x, transformCallback: cb => { const id = ++next; callbacks[id] = cb; return id; }, unregisterCallback: id => { delete callbacks[id]; }, invoke: async (c, a) => {
        if (c === 'plugin:event|listen') return a.handler;
        if (c === 'get_state') return { state: { type: 'Idle' }, meeting_id: null, companion: { visible: false, size: 'Pill' }, elapsed_ms: 0, quit_pending: false, share_visible: false };
        if (c === 'list_meetings' || c === 'deleted_meetings' || c === 'open_action_items') return [];
        if (c === 'scripts_list') return Object.entries(files).map(([name, f]) => ({ name, modified_ms: f.ms, bytes: f.text.length }));
        if (c === 'script_read') return files[a.name].text;
        if (c === 'script_save') { window.calls.push(`save:${a.name}:${a.baseModifiedMs}`); files[a.name] = { text: a.text, ms: 1000 }; return 1000; }
        return null; } };
    });
    await p.goto('http://127.0.0.1:3000/main/');
    await p.getByRole('button', { name: 'Scripts' }).first().click();
    const box = p.getByRole('textbox', { name: 'Paste or type your script' });
    await box.waitFor();
    if (await p.getByRole('navigation', { name: 'Scripts' }).count()) fails.push(theme + ': empty list column shown');
    if ((await p.locator('.scripts-steps li').count()) !== 3) fails.push(theme + ': steps');
    if (await p.locator('.empty').count()) fails.push(theme + ': old empty card');
    await p.screenshot({ path: `.playwright-out/scripts-first-${theme}.png` });
    await box.fill('# Opening (1m)\nThanks for having us.');
    const editor = p.getByRole('textbox', { name: 'Opening' });
    await editor.waitFor({ timeout: 3000 }).catch(() => fails.push(theme + ': editor did not open on the new script'));
    if (!(await p.evaluate(() => window.calls)).includes('save:Opening:null')) fails.push(theme + ': not created as Opening');
    if (!await editor.evaluate(e => e === document.activeElement).catch(() => false)) fails.push(theme + ': editor lost focus');
    await p.keyboard.type(' More.');
    if (!/Thanks for having us\. More\.$/.test(await editor.inputValue())) fails.push(theme + ': typing did not carry on: ' + await editor.inputValue());
    await p.screenshot({ path: `.playwright-out/scripts-editor-${theme}.png` });
    await p.close();
  }
  return fails.length ? fails : 'ok';
}
