// Verified notes (SPEC 7.13, mockup "Verified notes · proposed"): a task or decision shows its cited
// line on hover, an unconfirmed one carries the amber mark and a Quick check row, Keep clears the
// mark, Remove takes the line out of notes.md, and Undo puts it back.
async (page) => {
  const fails = [];
  const p = await page.context().newPage(); p.on('pageerror', e => fails.push(e.message));
  await p.setViewportSize({ width: 1200, height: 900 });
  await p.addInitScript(() => {
    const notes = '### Action items\n- [ ] You: Share the table design before Thursday [01:20]\n- [ ] Kevin: Check whether the 2021 project is still active [00:56]\n\n### Decisions\n- Keep **Plan A** and build the table ourselves [01:14]\n- Move to the paid plan above **10,000 devices** [01:00]\n\n### Overview\n- Compared two push services.\n';
    const transcript = '[00:56] Them: What does Plan B cost?\n[01:00] Me: Plan A has no fee. Plan B is free up to 1,000 devices.\n[01:14] Them: Then let\'s keep Plan A and build the table ourselves.\n[01:20] Me: Sure, I\'ll share the table design before Thursday.';
    const summary = { id: '1', title: 'Push services', created_ms: Date.now(), status: 'Ready', marks_ms: [], duration_ms: 87000, notes_ms: 48000, spellings: [], locked: false, language: 'en', notes_language: null, apps: [],
      unconfirmed: ['Kevin: Check whether the 2021 project is still active [00:56]', 'Move to the paid plan above **10,000 devices** [01:00]'] };
    window.calls = [];
    window.meeting = { summary, transcript, transcript_heard: '', notes_md: notes, has_video: false };
    let next = 0; const callbacks = {};
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = { convertFileSrc: x => x, transformCallback: cb => { const id = ++next; callbacks[id] = cb; return id; }, unregisterCallback: id => { delete callbacks[id]; }, invoke: async (c, a) => {
      window.calls.push(c);
      if (c === 'plugin:event|listen') return a.handler;
      if (c === 'get_state') return { state: { type: 'Idle' }, meeting_id: null, companion: { visible: false, size: 'Pill' }, elapsed_ms: 0, quit_pending: false };
      if (c === 'list_meetings') return [window.meeting.summary]; if (c === 'deleted_meetings') return [];
      if (c === 'get_meeting') return window.meeting;
      if (c === 'meeting_media') return { tracks: [], duration_ms: 87000, folder: 'x' };
      if (c === 'notes_translations') return []; if (c === 'translating') return null;
      if (c === 'save_notes') { if (a.expected !== window.meeting.notes_md) throw 'stale'; window.meeting.notes_md = a.markdown; return a.markdown; }
      if (c === 'set_unconfirmed') { window.meeting.summary = { ...window.meeting.summary, unconfirmed: a.lines }; return window.meeting.summary; }
      return false; } };
  });
  await p.goto('http://127.0.0.1:3000/main/');
  await p.getByRole('button', { name: /Push services/ }).first().click();
  const box = p.locator('.notes .suggest');
  await box.waitFor();
  const heads = await p.locator('.notes h3').allTextContents();
  if (heads.join('|') !== 'Action items|Decisions|Overview') fails.push('headings: ' + heads.join('|'));
  const order = await p.locator('.notes > *').evaluateAll(els => els.map(e => e.className || e.tagName));
  if (order.indexOf('suggest') !== 1) fails.push('Quick check is not right after Action items: ' + order.join(','));
  if (await p.locator('.notes .unsure').count() !== 2) fails.push('marks: ' + await p.locator('.notes .unsure').count());
  if (await box.locator('.sg').count() !== 2) fails.push('rows: ' + await box.locator('.sg').count());
  const tip = await box.locator('.sg .tip').first().textContent();
  if (tip !== 'Task from 00:56, where the call says') fails.push('row tip: ' + tip);
  // Quotes stay hidden until a line is pointed at.
  const kevin = p.locator('.todo li').nth(1);
  if (await kevin.locator('.aq').isVisible()) fails.push('quote shown before hover');
  // Showing it floats over the notes: nothing below moves (owner 2026-10-30), and it waits a beat.
  const below = () => p.locator('.notes .suggest').evaluate(e => e.getBoundingClientRect().top);
  // The notifications banner can land late and move the notes, so point again until it shows.
  let shown = false, before = 0;
  for (let i = 0; i < 5 && !shown; i++) {
    await kevin.hover(); before = await below();
    shown = await kevin.locator('.aq').waitFor({ state: 'visible', timeout: 1000 }).then(() => true, () => false);
  }
  if (!shown) fails.push('quote not shown on hover');
  if (await below() !== before) fails.push('showing the quote moved the notes');
  if ((await kevin.locator('.aq').textContent()) !== 'They saidWhat does Plan B cost?') fails.push('task quote: ' + await kevin.locator('.aq').textContent());
  const keep = p.locator('.notes ul:not(.todo) li').first();
  await keep.hover();
  if ((await keep.locator('.aq').textContent()) !== "They saidThen let's keep Plan A and build the table ourselves.") fails.push('decision quote: ' + await keep.locator('.aq').textContent());
  // Keep clears the task's mark; Remove takes the decision out; Undo puts it back.
  await box.locator('.sg').first().getByRole('button', { name: 'Keep' }).click();
  await p.waitForFunction(() => window.meeting.summary.unconfirmed.length === 1);
  if (await kevin.locator('.unsure').count()) fails.push('mark stays after Keep');
  await box.getByRole('button', { name: 'Remove' }).click();
  await p.waitForFunction(() => !window.meeting.notes_md.includes('paid plan'));
  if (await p.locator('.notes .suggest').count()) fails.push('Quick check stays with nothing to ask');
  if (!(await p.evaluate(() => window.meeting.summary.unconfirmed.length === 0))) fails.push('removed line still listed');
  await p.getByRole('button', { name: 'Undo' }).last().click();
  await p.waitForFunction(() => window.meeting.notes_md.includes('paid plan'));
  if (await p.locator('.notes .unsure').count() !== 1) fails.push('Undo did not bring the mark back');
  // Editing a decision saves its words only, not the mark or the quote.
  const paid = p.locator('.notes ul:not(.todo) li').nth(1), b = await paid.boundingBox();
  await p.mouse.click(b.x + 30, b.y + 9);
  await p.keyboard.press('End'); await p.keyboard.type(' now'); await p.keyboard.press('Escape');
  await p.waitForFunction(() => /devices\*\* \[01:00\] now|now \[01:00\]/.test(window.meeting.notes_md) || window.meeting.notes_md.includes(' now'));
  const md = await p.evaluate(() => window.meeting.notes_md);
  if (/Couldn|said/.test(md)) fails.push('mark or quote saved into the notes: ' + md);
  await p.close();
  return fails.length ? fails : 'PASS';
}
