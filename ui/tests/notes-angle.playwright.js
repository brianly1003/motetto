// "<You>" in a task is text, not HTML (owner 2026-10-26): those lines stay editable, an edit keeps
// them as written, and ⌘Enter on the line above leaves them editable. An autolink stays read-only.
async (page) => {
  const fails = [];
  const p = await page.context().newPage(); p.on('pageerror', e => fails.push(e.message));
  await p.setViewportSize({ width: 1200, height: 760 });
  await p.addInitScript(() => {
    const summary = { id: '1', title: 'FCM', created_ms: Date.now(), status: 'Ready', marks_ms: [], duration_ms: 200000, notes_ms: 48000, spellings: [], locked: false, language: 'en', notes_language: null };
    const meeting = { summary, transcript: '[02:35] Me: x', transcript_heard: '', notes_md: '### Action items\n- [ ] You: Write up the design [02:35]\n- [ ] <You> Ask Team X if they need it [02:39]\n### Overview\n- See <https://example.com>\n' };
    let next = 0; const callbacks = {};
    window.meeting = meeting;
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = { convertFileSrc: x => x, transformCallback: cb => { const id = ++next; callbacks[id] = cb; return id; }, unregisterCallback: id => { delete callbacks[id]; }, invoke: async (c, a) => {
      if (c === 'plugin:event|listen') return a.handler;
      if (c === 'get_state') return { state: { type: 'Idle' }, meeting_id: null, companion: { visible: false, size: 'Pill' }, elapsed_ms: 0, quit_pending: false };
      if (c === 'list_meetings') return [summary]; if (c === 'deleted_meetings') return []; if (c === 'get_meeting') return meeting;
      if (c === 'meeting_media') return { tracks: [], duration_ms: 200000, folder: 'x' };
      if (c === 'notes_translations') return []; if (c === 'translating') return null;
      if (c === 'save_notes') { meeting.notes_md = a.markdown; return a.markdown; }
      return false; } };
  });
  await p.goto('http://127.0.0.1:3000/main/');
  await p.getByRole('button', { name: /FCM/ }).first().click();
  const tasks = p.locator('ul.todo > li');
  await tasks.first().waitFor();
  const editable = async () => p.locator('.notes [data-lines]').evaluateAll(els => els.map(e => e.dataset.editable + ':' + e.textContent.trim().slice(0, 12)));
  const before = await editable();
  if (!before.some(t => t.startsWith('1:<You> Ask'))) fails.push('the <You> task is read-only: ' + JSON.stringify(before));
  if (!before.some(t => t.startsWith('0:See'))) fails.push('the autolink is editable: ' + JSON.stringify(before));

  // ⌘Enter at the end of the first task, then the <You> task is still editable and edits keep it.
  const first = p.locator('.task-text').first(), box = await first.boundingBox();
  await p.mouse.click(box.x + 20, box.y + box.height / 2);
  await p.keyboard.press('End');
  await p.keyboard.press('Meta+Enter');
  await p.keyboard.type('Book the room');
  await p.keyboard.press('Escape');
  await p.waitForFunction(() => window.meeting.notes_md.includes('Book the room'));
  const third = tasks.nth(2).locator('.task-text');
  if (await tasks.nth(2).getAttribute('data-editable') !== '1') fails.push('the <You> task became read-only after Enter');
  const tb = await third.boundingBox();
  await p.mouse.click(tb.x + tb.width - 60, tb.y + tb.height / 2);
  await p.keyboard.press('Home');
  await p.keyboard.type('Please ');
  await p.keyboard.press('Escape');
  await p.waitForFunction(() => window.meeting.notes_md.includes('Please'));
  const md = await p.evaluate(() => window.meeting.notes_md);
  const want = '### Action items\n- [ ] You: Write up the design [02:35]\n- [ ] Book the room\n- [ ] Please <You> Ask Team X if they need it [02:39]\n### Overview\n- See <https://example.com>\n';
  if (md !== want) fails.push('saved: ' + JSON.stringify(md));
  await p.close();
  return fails.length ? fails : 'PASS';
}
