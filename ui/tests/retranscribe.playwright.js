// Transcribe again (owner 2026-10-21): in •••, disabled without audio; the question counts the
// corrections it replaces; the steps follow Stage events; the new transcript and a toast follow.
async (page) => {
  const fails = [];
  const open = async (withAudio) => {
    const p = await page.context().newPage(); p.on('pageerror', e => fails.push(e.message));
    await p.setViewportSize({ width: 1200, height: 720 });
    await p.addInitScript(audio => {
      const summary = { id: '1', title: 'Again', created_ms: Date.now(), status: 'Ready', marks_ms: [], duration_ms: 60000, notes_ms: 8000, spellings: [], locked: false, title_source: 'placeholder' };
      // Two of three lines corrected: the heard copy differs on lines 1 and 3.
      const meeting = { summary, transcript: '[00:04] Me: Hello there\n[00:09] Them: Hi\n[00:12] Me: Bye now', transcript_heard: '[00:04] Me: Hello\n[00:09] Them: Hi\n[00:12] Me: Bye', notes_md: '### Action items\n- None\n### Overview\n- Notes.\n' };
      let next = 0, job = null; const callbacks = {}, listeners = {};
      window.emit = (event, payload) => (listeners[event] || []).forEach(id => callbacks[id]?.({ event, id, payload }));
      window.calls = [];
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
      window.__TAURI_INTERNALS__ = { convertFileSrc: x => x, transformCallback: cb => { const id = ++next; callbacks[id] = cb; return id; }, unregisterCallback: id => { delete callbacks[id]; }, invoke: async (c, a) => {
        if (c === 'plugin:event|listen') { (listeners[a.event] ||= []).push(a.handler); return a.handler; }
        if (c === 'get_state') return { state: { type: 'Idle' }, meeting_id: null, companion: { visible: false, size: 'Pill' }, elapsed_ms: 0, quit_pending: false };
        if (c === 'list_meetings') return [summary]; if (c === 'deleted_meetings') return []; if (c === 'get_meeting') return meeting;
        if (c === 'importing') return job;
        if (c === 'meeting_action') {
          window.calls.push(a.action);
          job = { id: '1', import: false, transcribe: a.action === 'retranscribe' }; window.emit('motetto://library-changed', null);
          await new Promise(resolve => { window.finish = resolve; });
          meeting.transcript = '[00:04] Me: Hello, fresh\n[00:09] Them: Hi\n[00:12] Me: Bye'; meeting.transcript_heard = '';
          job = null; window.emit('motetto://library-changed', null);
          return null;
        }
        if (c === 'meeting_media') return { tracks: audio ? [{ path: '/a.wav', name: 'mic.wav', speaker: 'You', offset_ms: 0, duration_ms: 60000, bytes: 1, waveform: [0.3] }] : [], duration_ms: 60000, folder: 'x' };
        return false; } };
    }, withAudio);
    await p.goto('http://127.0.0.1:3000/main/');
    await p.getByRole('button', { name: /Again/ }).first().click();
    await p.getByRole('button', { name: 'More actions' }).click();
    return p;
  };
  // No saved audio: the item is there but disabled, with the reason.
  let p = await open(false);
  const none = p.getByRole('menuitem', { name: 'Transcribe again' });
  if (!await none.isDisabled()) fails.push('Transcribe again enabled with no saved audio');
  if (await none.getAttribute('title') !== 'The audio was deleted, so there is nothing to transcribe.') fails.push('no reason shown when disabled');
  await p.close();

  p = await open(true);
  const item = p.getByRole('menuitem', { name: 'Transcribe again' });
  await item.waitFor();
  for (let i = 0; i < 20 && await item.isDisabled(); i++) await p.waitForTimeout(50); // waits for the audio list
  await item.click();
  const ask = await p.getByRole('alertdialog').innerText();
  // Only the question and what it replaces, like Write the notes again (owner 2026-10-25).
  if (!/Your 2 corrections to the transcript will be replaced\./.test(ask) || /transcript\.prev\.txt/.test(ask)) fails.push('question: ' + ask);
  await p.getByRole('alertdialog').getByRole('button', { name: 'Confirm' }).click();
  await p.locator('.writing').getByText('Transcribing again').waitFor();
  const steps = () => p.locator('.writing .step').evaluateAll(rows => rows.map(r => `${r.className.replace('step', '').trim() || 'todo'}:${r.children[1].textContent}`).join(' | '));
  if (await steps() !== 'now:Transcribing the saved audio | todo:Saving') fails.push('steps at start: ' + await steps());
  // Measured progress (SPEC 6.6): percent and time left on the step, what it does below, the bar
  // filling the step's share (0 to 95 %).
  const progress = (percent, eta_s, step) => p.evaluate(a => window.emit('motetto://event', { type: 'Stage', meeting_id: '1', stage: { type: 'Transcribing', progress: a }, percent: a.percent, eta_s: a.eta_s }), { percent, eta_s, step });
  const now = () => p.locator('.writing .step.now').evaluate(r => [r.children[2].textContent, r.querySelector('.sub > span')?.textContent ?? ''].join(' / '));
  const bar = () => p.locator('.procbar i').evaluate(i => Math.round(i.getBoundingClientRect().width / i.parentElement.getBoundingClientRect().width * 1000) / 10);
  await p.waitForTimeout(500);
  if (await bar() !== 0) fails.push('bar before the first measurement is ' + await bar());
  await progress(12, null, { type: 'Converting', side: 'Them' }); await p.waitForTimeout(500);
  if (await now() !== '12% / Them: preparing the audio') fails.push('before the time is known: ' + await now());
  await progress(43, 250, { type: 'Checking', side: 'Them', piece: 37, of: 148 }); await p.waitForTimeout(500);
  if (await now() !== '43% · about 4 min left / Them: checking long lines, 37 of 148') fails.push('measured: ' + await now());
  if (Math.abs(await bar() - 40.9) > 0.6) fails.push('bar at 43% is ' + await bar());
  await p.locator('.writing').screenshot({ path: '.playwright-out/transcribe-progress.png' });
  await progress(97, 40, { type: 'RemovingEcho' }); await p.waitForTimeout(100);
  if (await now() !== '97% · less than a minute left / You: removing the call\'s echo') fails.push('last minute: ' + await now());
  await p.evaluate(() => window.emit('motetto://event', { type: 'Stage', meeting_id: '1', stage: { type: 'Saving' }, percent: 0, eta_s: null }));
  await p.waitForTimeout(100);
  if (await steps() !== 'done:Transcribing the saved audio | now:Saving') fails.push('steps at Saving: ' + await steps());
  if (!/Transcribing/.test(await p.locator('.meeting-row').first().innerText())) fails.push('sidebar row does not say Transcribing');
  await p.evaluate(() => window.finish());
  await p.getByText('Transcribed again. The notes still come from the earlier transcript.').waitFor();
  await p.getByRole('tab', { name: 'Transcript', exact: true }).click();
  await p.getByText('Hello, fresh').waitFor();
  if (JSON.stringify(await p.evaluate(() => window.calls)) !== '["retranscribe"]') fails.push('actions sent: ' + JSON.stringify(await p.evaluate(() => window.calls)));
  await p.close();
  return fails.length ? fails : 'PASS';
}
