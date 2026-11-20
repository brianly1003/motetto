// Find speakers: in •••, disabled without audio; the question says nothing is transcribed again
// and corrections stay; the steps follow Stage events (models, voices, saving); a toast follows.
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
          window.calls.push(a.people ? `${a.action}:${a.people}` : a.action);
          job = { id: '1', import: false, transcribe: a.action === 'retranscribe', speakers: a.action === 'find_speakers' }; window.emit('motetto://library-changed', null);
          await new Promise(resolve => { window.finish = resolve; });
          meeting.transcript = '[00:04] Me: Hello there\n[00:09] Speaker 2: Hi\n[00:10] Speaker 3: Hey\n[00:12] Me: Bye now'; meeting.transcript_heard = '[00:04] Me: Hello\n[00:09] Speaker 2: Hi\n[00:10] Speaker 3: Hey\n[00:12] Me: Bye';
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
  let p = await open(false);
  const none = p.getByRole('menuitem', { name: 'Find speakers' });
  if (!await none.isDisabled()) fails.push('Find speakers enabled with no saved audio');
  await p.close();

  p = await open(true);
  const item = p.getByRole('menuitem', { name: 'Find speakers' });
  await item.waitFor();
  for (let i = 0; i < 20 && await item.isDisabled(); i++) await p.waitForTimeout(50);
  await item.click();
  const ask = await p.getByRole('alertdialog').innerText();
  if (!/Your corrections stay, and nothing is transcribed again\./.test(ask) || !/speaker models \(34 MB\)/.test(ask)) fails.push('question: ' + ask);
  // How many other people: Not sure by default; 4 is sent with the action.
  const people = p.getByRole('combobox', { name: 'How many other people were on the call' });
  if (await people.inputValue() !== '0') fails.push('people preset: ' + await people.inputValue());
  if (!(await people.locator('option').allTextContents()).includes('1')) fails.push('people: no 1');
  await people.selectOption('4');
  await p.getByRole('alertdialog').getByRole('button', { name: 'Confirm' }).click();
  await p.locator('.writing').getByText('Finding speakers').first().waitFor();
  const steps = () => p.locator('.writing .step').evaluateAll(rows => rows.map(r => `${r.className.replace('step', '').trim() || 'todo'}:${r.children[1].textContent}`).join(' | '));
  if (await steps() !== 'now:Getting the speaker models | todo:Listening for each voice | todo:Saving') fails.push('steps at start: ' + await steps());
  const stage = st => p.evaluate(st => window.emit('motetto://event', { type: 'Stage', meeting_id: '1', stage: st, percent: st.percent ?? 0, eta_s: null }), st);
  await stage({ type: 'GettingSpeakerModels', percent: 40 }); await p.waitForTimeout(300);
  const right = await p.locator('.writing .step.now').evaluate(r => r.children[2]?.textContent ?? '');
  if (right !== '40%') fails.push('download percent: ' + right);
  await stage({ type: 'FindingSpeakers' }); await p.waitForTimeout(200);
  if (await steps() !== 'done:Speaker models ready | now:Listening for each voice | todo:Saving') fails.push('steps finding: ' + await steps());
  if (!/Finding speakers/.test(await p.locator('.meeting-row').first().innerText())) fails.push('sidebar row does not say Finding speakers');
  await p.locator('.writing').screenshot({ path: '.playwright-out/find-speakers.png' });
  await stage({ type: 'Saving' }); await p.waitForTimeout(100);
  if (await steps() !== 'done:Speaker models ready | done:Listening for each voice | now:Saving') fails.push('steps saving: ' + await steps());
  await p.evaluate(() => window.finish());
  await p.getByText('Speakers found. The notes still use the earlier labels.').waitFor();
  await p.getByRole('tab', { name: 'Transcript', exact: true }).click();
  await p.getByText('Hey').first().waitFor();
  if (JSON.stringify(await p.evaluate(() => window.calls)) !== '["find_speakers:4"]') fails.push('actions sent: ' + JSON.stringify(await p.evaluate(() => window.calls)));
  await p.close();
  return fails.length ? fails : 'PASS';
}
