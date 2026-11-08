// The @ picker in the companion's My notes (SPEC 7.11): picked names are written as one word.
async (page) => {
  const p = await page.context().newPage(); const errs = []; p.on('pageerror', e => errs.push(e.message));
  await p.setViewportSize({ width: 360, height: 640 });
  await p.addInitScript(() => {
    let next = 0; const callbacks = {}, listeners = {};
    const state = { state: { type: 'Recording' }, meeting_id: 'fixture', app: null, elapsed_ms: 319000, started_by: { type: 'Manual' }, auto_start: false, problem: null, more_problems: 0, quit_pending: false, dnd_active: false, onboarding_done: true, sharing: null, never_pending: null, companion: { visible: true, size: 'Sidecar', pinned: true, peeking: false, display_id: null } };
    let notes = { meeting_id: 'fixture', text: '[05:00] [] Share the design @Linh_Tran', version: 1 };
    window.testCalls = [];
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = { metadata: { currentWindow: { label: 'companion' }, currentWebview: { windowLabel: 'companion', label: 'companion' } },
      transformCallback: cb => { callbacks[++next] = cb; return next; }, unregisterCallback: id => delete callbacks[id],
      invoke: async (c, a) => { window.testCalls.push({ command: c, args: a });
        if (c === 'plugin:event|listen') { listeners[a.event] = a.handler; return next; }
        if (c === 'get_state') return state; if (c === 'list_screen_sources') return []; if (c === 'get_levels') return [0, 0]; if (c === 'live_transcript') return ['fixture', '', ''];
        if (c === 'get_my_notes') return notes; if (c === 'save_my_notes') { notes = { ...notes, text: a.text, version: notes.version + 1 }; return notes; }
        if (c === 'resume_remaining_ms') return 8000; return null; } };
  });
  await p.goto('http://127.0.0.1:3000/companion/');
  const editor = p.getByRole('textbox', { name: 'My notes. Each line keeps the time you wrote it.' }); await editor.waitFor();
  await p.waitForTimeout(600);
  await editor.click(); await p.keyboard.press('End'); await p.keyboard.press('Enter');
  await p.keyboard.type('[] call the bank @li');
  const options = await p.locator('.mention [role=option]').allInnerTexts();
  await p.keyboard.press('Enter'); await p.waitForTimeout(100);
  const reopened = await p.locator('.mention').count();
  await p.keyboard.press('Enter'); await p.keyboard.type('[] send the deck @Anna');
  const fresh = await p.locator('.mention [role=option]').allInnerTexts();
  await p.keyboard.press('Enter'); await p.keyboard.type('today'); await p.waitForTimeout(100);
  const after = await p.locator('.mention').count();
  await p.waitForTimeout(500);
  const saved = await p.evaluate(() => [...window.testCalls].reverse().find(c => c.command === 'save_my_notes')?.args.text);
  await p.close();
  const failures = [];
  if (JSON.stringify(options) !== '["Linh Tran"]') failures.push(`options: ${JSON.stringify(options)}`);
  if (reopened || after) failures.push('picker reopened after a pick');
  if (fresh.length !== 1 || !fresh[0].includes('Anna') || !fresh[0].includes('new')) failures.push(`new person: ${JSON.stringify(fresh)}`);
  if (!/\n\[[0-9]+:[0-9]{2}\] \[\] call the bank @Linh_Tran \n\[[0-9]+:[0-9]{2}\] \[\] send the deck @Anna today/.test(saved ?? '')) failures.push(`saved: ${saved}`);
  if (failures.length || errs.length) throw Error(JSON.stringify({ failures, errs }));
  return 'ok';
}
