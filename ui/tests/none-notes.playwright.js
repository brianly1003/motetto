// Notes saved with a placeholder "nothing" under Decisions ("- &lt;none>", marked unconfirmed by an
// older notes writer) show as "None" like Action items: no "Couldn't confirm" mark, no Quick check
// row, no escaped "&lt;" anywhere (owner 2026-11-08).
async (page) => {
  const fails = [];
  const p = await page.context().newPage(); p.on('pageerror', e => fails.push(e.message));
  await p.setViewportSize({ width: 1200, height: 800 });
  await p.addInitScript(() => {
    const summary = { id: '1', title: 'Meeting recording tool testing', created_ms: Date.now(), status: 'Ready', marks_ms: [], duration_ms: 30000, notes_ms: 10000, spellings: [], locked: false, title_source: 'model', unconfirmed: ['&lt;none>'] };
    const meeting = { summary, transcript: '[00:01] Me: Testing the recording tool.', transcript_heard: '', notes_md: '### Action items\n- None\n### Decisions\n- &lt;none>\n### Overview\n- The call was a sound check.' };
    let next = 0; const callbacks = {};
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = { convertFileSrc: x => x, transformCallback: cb => { const id = ++next; callbacks[id] = cb; return id; }, unregisterCallback: () => {}, invoke: async (c) => {
      if (c === 'plugin:event|listen') return 1;
      if (c === 'get_state') return { state: { type: 'Idle' }, meeting_id: null, companion: { visible: false, size: 'Pill' }, elapsed_ms: 0, quit_pending: false };
      if (c === 'list_meetings') return [summary]; if (c === 'deleted_meetings') return []; if (c === 'get_meeting') return meeting;
      if (c === 'meeting_media') return { tracks: [], videos: [], moments: [], clicks: [], duration_ms: 30000, folder: 'x' };
      return false; } };
  });
  await p.goto('http://127.0.0.1:3000/main/');
  await p.getByRole('button', { name: /Meeting recording tool testing/ }).first().click();
  await p.waitForSelector('text=The call was a sound check.');
  const body = await p.locator('.document').innerText();
  if (body.includes('&lt;') || body.includes('<none>')) fails.push('placeholder or escaped text shown');
  if (await p.getByText("Couldn't confirm in the call").count()) fails.push('a None marked unconfirmed');
  if (await p.locator('[aria-label="Quick check"]').count()) fails.push('Quick check shown for a None');
  const decisions = await p.evaluate(() => [...document.querySelectorAll('.document h3, .document h4')].find(h => /Decisions/.test(h.textContent))?.parentElement?.innerText ?? '');
  if (!/Decisions\s+None/.test(decisions)) fails.push('Decisions does not read None: ' + JSON.stringify(decisions.slice(0, 80)));
  await p.screenshot({ path: '.playwright-out/none-notes.png' });
  await p.close();
  if (fails.length) throw new Error(fails.join('\n'));
  return 'ok';
}
