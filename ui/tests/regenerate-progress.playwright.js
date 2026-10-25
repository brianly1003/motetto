// Write the notes again shows its steps on the meeting's page (owner 2026-10-21): Transcript ready,
// Summarizing, Saving, following the Stage events Rust sends; then the new notes.
async (page) => {
  const fails = [];
  const p = await page.context().newPage(); p.on('pageerror', e => fails.push(e.message));
  await p.setViewportSize({ width: 1200, height: 720 });
  await p.addInitScript(() => {
    const summary = { id: '1', title: 'Again', created_ms: Date.now(), status: 'Ready', marks_ms: [], duration_ms: 60000, notes_ms: 8000, spellings: [], locked: false, title_source: 'placeholder' };
    const meeting = { summary, transcript: '[00:04] Me: Hello\n[00:09] Them: Hi there', transcript_heard: '', notes_md: '### Action items\n- None\n### Overview\n- Old notes.\n' };
    let next = 0, job = null; const callbacks = {}, listeners = {};
    // A tiny event bus: listen registers a callback id; emit calls it the way Tauri does.
    window.emit = (event, payload) => (listeners[event] || []).forEach(id => callbacks[id]?.({ event, id, payload }));
    window.finish = null;
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = { convertFileSrc: x => x, transformCallback: cb => { const id = ++next; callbacks[id] = cb; return id; }, unregisterCallback: id => { delete callbacks[id]; }, invoke: async (c, a) => {
      if (c === 'plugin:event|listen') { (listeners[a.event] ||= []).push(a.handler); return a.handler; }
      if (c === 'plugin:event|unlisten') return null;
      if (c === 'get_state') return { state: { type: 'Idle' }, meeting_id: null, companion: { visible: false, size: 'Pill' }, elapsed_ms: 0, quit_pending: false };
      if (c === 'list_meetings') return [summary]; if (c === 'deleted_meetings') return []; if (c === 'get_meeting') return meeting;
      if (c === 'importing') return job;
      if (c === 'meeting_action' && a.action === 'regenerate') {
        job = { id: '1', import: false }; window.emit('motetto://library-changed', null);
        await new Promise(resolve => { window.finish = resolve; });
        meeting.notes_md = '### Action items\n- None\n### Overview\n- New notes.\n'; job = null; window.emit('motetto://library-changed', null);
        return null;
      }
      if (c === 'meeting_media') return { tracks: [], duration_ms: 60000, folder: 'x' }; return false; } };
  });
  await p.goto('http://127.0.0.1:3000/main/');
  await p.getByRole('button', { name: /Again/ }).first().click();
  await p.getByRole('button', { name: 'More actions' }).click();
  await p.getByRole('menuitem', { name: 'Write the notes again' }).click();
  // Only the question (owner 2026-10-25): the notes.prev.md file name meant nothing on screen.
  if (!/Replace these notes\?$/.test((await p.getByRole('alertdialog').innerText()).split('\n').filter(l => /Replace/.test(l))[0] || '') || /notes\.prev/.test(await p.getByRole('alertdialog').innerText())) fails.push('the question is not just "Replace these notes?"');
  await p.getByRole('alertdialog').screenshot({ path: '.playwright-out/regenerate-confirm.png' });
  await p.getByRole('alertdialog').getByRole('button', { name: 'Confirm' }).click();
  const page_ = p.locator('.writing');
  await page_.getByText('Writing the notes again').waitFor();
  if (await p.getByRole('alertdialog').count()) fails.push('the question stays open while the notes are written');
  if (!/Writing notes/.test(await p.locator('.meeting-row').first().innerText())) fails.push('the sidebar row does not say Writing notes');
  if (await p.getByRole('button', { name: 'More actions' }).count()) fails.push('meeting actions offered while the notes are written');
  const steps = async () => p.locator('.writing .step').evaluateAll(rows => rows.map(r => `${r.className.replace('step', '').trim() || 'todo'}:${r.children[1].textContent}${r.children[2].textContent ? ' ' + r.children[2].textContent : ''}`));
  // Each step shows how long it took, and the one being done counts up (owner 2026-10-25).
  await p.waitForTimeout(1100);
  const first = (await steps()).join(' | ');
  if (first !== 'done:Transcript ready 2 lines | now:Summarizing 00:01 | todo:Saving') fails.push('before any stage: ' + first);
  // A long call is summarized in parts: the step says which part, the time still runs from the start.
  await p.evaluate(() => window.emit('motetto://event', { type: 'Stage', meeting_id: '1', stage: { type: 'Summarizing', part: 2, of: 4 }, percent: 0, eta_s: null }));
  await p.waitForTimeout(100);
  const part = (await steps()).join(' | ');
  if (part !== 'done:Transcript ready 2 lines | now:Summarizing part 2 of 4 00:01 | todo:Saving') fails.push('a part: ' + part);
  await p.evaluate(() => window.emit('motetto://event', { type: 'Stage', meeting_id: '1', stage: { type: 'Saving' }, percent: 0, eta_s: null }));
  await p.waitForTimeout(100);
  const saving = (await steps()).join(' | ');
  if (saving !== 'done:Transcript ready 2 lines | done:Summarizing 00:01 | now:Saving 00:00') fails.push('after Saving: ' + saving);
  await p.evaluate(() => window.emit('motetto://event', { type: 'Stage', meeting_id: 'other', stage: { type: 'Summarizing', part: 1, of: 1 }, percent: 0, eta_s: null }));
  await p.waitForTimeout(100);
  if ((await steps()).join(' | ') !== saving) fails.push("another meeting's stage moved these steps");
  await p.locator('.writing').screenshot({ path: '.playwright-out/regenerate-times.png' });
  await p.evaluate(() => window.finish());
  await p.getByText('New notes.').waitFor();
  if (await p.locator('.writing').count()) fails.push('steps page still shown after the notes were saved');
  if (!/Ready/.test(await p.locator('.meeting-row').first().innerText())) fails.push('the sidebar row is not Ready again');
  await p.screenshot({ path: '.playwright-out/regenerate-progress-done.png' });
  await p.close();
  return fails.length ? fails : 'PASS';
}
