// Playing a meeting, then Transcribe again or Write the notes again: the steps page replaces the
// player and playback stops (owner 2026-10-28), native audio included. A fake clock stands in for
// the engine.
async (page) => {
  const fails = [];
  for (const action of ['Transcribe again', 'Write the notes again']) {
    const p = await page.context().newPage(); p.on('pageerror', e => fails.push(e.message));
    await p.setViewportSize({ width: 1200, height: 720 });
    await p.addInitScript(() => {
      const summary = { id: '1', title: 'Playing', created_ms: Date.now(), status: 'Ready', marks_ms: [], duration_ms: 60000, notes_ms: 8000, spellings: [], locked: false, title_source: 'placeholder' };
      const meeting = { summary, transcript: '[00:00] Them: First synthetic line.\n[00:30] Me: Second synthetic line.', transcript_heard: '', notes_md: '### Overview\nSynthetic.' };
      const clock = { playing: false, base: 0, since: 0 };
      const now = () => clock.playing ? clock.base + (performance.now() - clock.since) : clock.base;
      window.playing = () => clock.playing;
      let next = 0, job = null; const callbacks = {}, listeners = {};
      window.emit = (event, payload) => (listeners[event] || []).forEach(id => callbacks[id]?.({ event, id, payload }));
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
      window.__TAURI_INTERNALS__ = { convertFileSrc: x => x, transformCallback: cb => { const id = ++next; callbacks[id] = cb; return id; }, unregisterCallback: id => { delete callbacks[id]; }, invoke: async (c, a) => {
        if (c === 'plugin:event|listen') { (listeners[a.event] ||= []).push(a.handler); return a.handler; }
        if (c === 'playback_open') return true;
        if (c === 'playback_set') { clock.base = a.atMs ?? now(); clock.playing = a.playing; clock.since = performance.now(); return now(); }
        if (c === 'playback_position') return now();
        if (c === 'playback_close') return;
        if (c === 'get_state') return { state: { type: 'Idle' }, meeting_id: null, companion: { visible: false, size: 'Pill' }, elapsed_ms: 0, quit_pending: false };
        if (c === 'list_meetings') return [summary]; if (c === 'deleted_meetings') return []; if (c === 'get_meeting') return meeting;
        if (c === 'importing') return job;
        if (c === 'meeting_action') { job = { id: '1', import: false, transcribe: a.action === 'retranscribe' }; window.emit('motetto://library-changed', null); return new Promise(() => {}); }
        if (c === 'meeting_media') return { tracks: [{ path: 'none.wav', name: 'part-0000.mic.wav', speaker: 'You', offset_ms: 0, duration_ms: 60000, bytes: 1, waveform: Array(200).fill(.2) }], videos: [], moments: [], duration_ms: 60000, folder: 'x' };
        return false; } };
    });
    await p.goto('http://127.0.0.1:3000/main/');
    await p.getByRole('button', { name: /Playing/ }).first().click();
    await p.getByRole('tab', { name: 'Transcript' }).click();
    await p.getByRole('button', { name: '▶ Play' }).click();
    await p.getByText('00:01', { exact: true }).waitFor({ timeout: 3000 }).catch(() => fails.push(`${action}: did not start playing`));
    await p.getByRole('button', { name: 'More actions' }).click();
    const item = p.getByRole('menuitem', { name: action });
    for (let i = 0; i < 20 && await item.isDisabled(); i++) await p.waitForTimeout(50); // waits for the audio list
    await item.click();
    await p.getByRole('alertdialog').getByRole('button', { name: 'Confirm' }).click();
    await p.locator('.writing').waitFor();
    await p.waitForTimeout(300);
    if (await p.evaluate(() => window.playing())) fails.push(`${action}: audio still plays behind the steps page`);
    await p.close();
  }
  return fails.length ? fails : 'PASS';
}
