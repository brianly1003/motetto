// Stop on the steps pages (owner 2026-10-25, mockup stopJob): Write the notes again and Transcribe
// again keep what was there and say so; after a call the meeting waits as Notes not written, with
// Write the notes. IPC is mocked: Stop rejects the running job the way Rust does.
async (page) => {
  const fails = [];
  const open = async (theme, finishing) => {
    const p = await page.context().newPage(); p.on('pageerror', e => fails.push(e.message));
    await p.emulateMedia({ colorScheme: theme, reducedMotion: 'reduce' });
    await p.setViewportSize({ width: 1200, height: 720 });
    await p.addInitScript(finishing => {
      const summary = { id: '1', title: 'Again', created_ms: Date.now(), status: finishing ? 'Finishing' : 'Ready', marks_ms: [], duration_ms: 60000, notes_ms: 8000, spellings: [], locked: false, title_source: 'placeholder' };
      const meeting = { summary, transcript: '[00:04] Me: Hello\n[00:09] Them: Hi there', transcript_heard: '', notes_md: finishing ? '' : '### Action items\n- None\n### Overview\n- Old notes.\n' };
      let next = 0, job = null, reject = null; const callbacks = {}, listeners = {};
      let state = { state: finishing ? { type: 'Finishing', meeting_id: '1' } : { type: 'Idle' }, meeting_id: finishing ? '1' : null, companion: { visible: false, size: 'Pill' }, elapsed_ms: 60000, quit_pending: false };
      window.emit = (event, payload) => (listeners[event] || []).forEach(id => callbacks[id]?.({ event, id, payload }));
      window.calls = [];
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
      window.__TAURI_INTERNALS__ = { convertFileSrc: x => x, transformCallback: cb => { const id = ++next; callbacks[id] = cb; return id; }, unregisterCallback: id => { delete callbacks[id]; }, invoke: async (c, a) => {
        if (c === 'plugin:event|listen') { (listeners[a.event] ||= []).push(a.handler); return a.handler; }
        if (c === 'plugin:event|unlisten') return null;
        if (c === 'get_state') return state;
        if (c === 'list_meetings') return [summary]; if (c === 'deleted_meetings') return []; if (c === 'get_meeting') return meeting;
        if (c === 'importing') return job;
        if (c === 'stop_processing') {
          window.calls.push(['stop_processing', a.id]);
          // After a call: the worker stops, the meeting is NotesStopped and the session idle.
          if (state.state.type === 'Finishing') setTimeout(() => {
            summary.status = 'NotesStopped'; state = { ...state, state: { type: 'Idle' }, meeting_id: null };
            window.emit('motetto://event', { type: 'StateChanged', ...state }); window.emit('motetto://library-changed', null);
          }, 50);
          else setTimeout(() => reject?.('stopped'), 50);
          return null;
        }
        if (c === 'meeting_action') {
          window.calls.push(['meeting_action', a.action]);
          job = { id: '1', import: false, transcribe: a.action === 'retranscribe' }; window.emit('motetto://library-changed', null);
          try { await new Promise((resolve, no) => { reject = no; window.finish = resolve; }); }
          finally { job = null; window.emit('motetto://library-changed', null); }
          summary.status = 'Ready'; meeting.notes_md = '### Action items\n- None\n### Overview\n- New notes.\n';
          return null;
        }
        if (c === 'meeting_media') return { tracks: [{ path: '/a.wav', name: 'mic.wav', speaker: 'You', offset_ms: 0, duration_ms: 60000, bytes: 1, waveform: [0.3] }], duration_ms: 60000, folder: 'x' };
        return false; } };
    }, finishing);
    await p.goto('http://127.0.0.1:3000/main/');
    await p.getByRole('button', { name: /Again/ }).first().click();
    return p;
  };
  const row = p => p.locator('.writing .proc-cancel');
  const out = {};
  for (const theme of ['light', 'dark']) {
    for (const [action, item, hint, toast] of [['regenerate', 'Write the notes again', 'Your current notes stay as they are.', 'Stopped. Your notes are unchanged.'], ['retranscribe', 'Transcribe again', 'Your current transcript stays as it is.', 'Stopped. Your transcript is unchanged.']]) {
      const p = await open(theme, false);
      await p.getByRole('button', { name: 'More actions' }).click();
      const menu = p.getByRole('menuitem', { name: item });
      for (let i = 0; i < 20 && await menu.isDisabled(); i++) await p.waitForTimeout(50); // waits for the audio list
      await menu.click();
      await p.getByRole('alertdialog').getByRole('button', { name: 'Confirm' }).click();
      await row(p).waitFor();
      if (await row(p).locator('.hint').innerText() !== hint) fails.push(`${action}: hint ${await row(p).locator('.hint').innerText()}`);
      await row(p).screenshot({ path: `.playwright-out/stop-${action}-${theme}.png` });
      await row(p).getByRole('button', { name: 'Stop' }).click();
      if (!await p.locator('.writing.stopping').count() || await row(p).getByRole('button').innerText() !== 'Stopping…') fails.push(`${action}: no Stopping… state`);
      await p.getByText(toast).waitFor({ timeout: 3000 }).catch(() => fails.push(`${action}: no toast "${toast}"`));
      if (await p.locator('.writing').count()) fails.push(`${action}: steps page still shown after Stop`);
      if (action === 'regenerate' && !await p.getByText('Old notes.').count()) fails.push('regenerate: the old notes are not shown');
      if (await p.getByRole('alert').filter({ hasText: 'stopped' }).count()) fails.push(`${action}: shown as an error`);
      const calls = JSON.stringify(await p.evaluate(() => window.calls));
      if (calls !== JSON.stringify([['meeting_action', action], ['stop_processing', '1']])) fails.push(`${action}: calls ${calls}`);
      await p.close();
    }
    // After a call: Stop, then the meeting waits as Notes not written; Write the notes starts again.
    const p = await open(theme, true);
    await row(p).waitFor();
    if (await row(p).locator('.hint').innerText() !== 'The recording is kept. You can write the notes later.') fails.push('after call: hint');
    await p.locator('.writing').screenshot({ path: `.playwright-out/stop-aftercall-${theme}.png` });
    await row(p).getByRole('button', { name: 'Stop' }).click();
    const stopped = p.locator('.alert', { hasText: 'Notes not written' });
    await stopped.waitFor({ timeout: 3000 }).catch(() => fails.push('after call: no Notes not written alert'));
    if (!/You stopped them\. The recording and transcript are saved\./.test(await stopped.innerText().catch(() => ''))) fails.push('after call: alert text');
    out[`chip-${theme}`] = await p.locator('.meeting-row').first().locator('i').innerText();
    if (out[`chip-${theme}`] !== 'Needs attention') fails.push('after call: chip ' + out[`chip-${theme}`]);
    await p.locator('.document').screenshot({ path: `.playwright-out/stop-stopped-${theme}.png` });
    await stopped.getByRole('button', { name: 'Write the notes' }).click();
    await p.locator('.writing h4').waitFor();
    if (await p.locator('.alert', { hasText: 'Notes not written' }).count()) fails.push('after call: alert stays while the notes are written');
    await p.evaluate(() => window.finish());
    await p.getByText('New notes.').waitFor({ timeout: 3000 }).catch(() => fails.push('after call: new notes not shown'));
    await p.close();
  }
  return fails.length ? fails : { result: 'PASS', ...out };
}
