// macOS plays meeting audio natively: the page renders no audio elements, sends play, pause and
// seek to playback_set and follows playback_position. A fake clock stands in for the engine.
async (page) => {
  const fails = [];
  const p = await page.context().newPage(); p.on('pageerror', e => fails.push(e.message));
  await p.setViewportSize({ width: 1200, height: 720 });
  await p.addInitScript(() => {
    const summary = { id: '1', title: 'Native', created_ms: Date.now(), status: 'Ready', marks_ms: [], duration_ms: 60000, notes_ms: 8000, spellings: [], locked: false, title_source: 'placeholder' };
    const meeting = { summary, transcript: '[00:00] Them: First synthetic line.\n[00:30] Me: Second synthetic line.', transcript_heard: '', notes_md: '### Overview\nSynthetic.' };
    const clock = { playing: false, base: 0, since: 0 };
    const now = () => clock.playing ? clock.base + (performance.now() - clock.since) : clock.base;
    window.playbackCalls = [];
    let next = 0;
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = { convertFileSrc: x => x, transformCallback: () => ++next, unregisterCallback: () => {}, invoke: async (c, a) => {
      if (c.startsWith('playback_')) window.playbackCalls.push({ c, ...a });
      if (c === 'playback_open') return true;
      if (c === 'playback_set') { clock.base = a.atMs ?? now(); clock.playing = a.playing; clock.since = performance.now(); return now(); }
      if (c === 'playback_position') return now();
      if (c === 'playback_close') return;
      if (c === 'plugin:event|listen') return next; if (c === 'get_state') return { state: { type: 'Idle' }, meeting_id: null, companion: { visible: false, size: 'Pill' }, elapsed_ms: 0, quit_pending: false };
      if (c === 'list_meetings') return [summary]; if (c === 'deleted_meetings') return []; if (c === 'get_meeting') return meeting;
      if (c === 'meeting_media') return { tracks: [{ path: 'none.wav', name: 'part-0000.mic.wav', speaker: 'You', offset_ms: 0, duration_ms: 60000, bytes: 1, waveform: Array(200).fill(.2) }], videos: [], moments: [], duration_ms: 60000, folder: 'x' };
      return false; } };
  });
  await p.goto('http://127.0.0.1:3000/main/');
  await p.getByRole('button', { name: /Native/ }).first().click();
  await p.getByRole('tab', { name: 'Transcript' }).click();
  const calls = () => p.evaluate(() => window.playbackCalls);
  await p.waitForFunction(() => window.playbackCalls.some(c => c.c === 'playback_open'));
  if (await p.locator('audio').count()) fails.push('audio elements rendered while audio plays natively');
  await p.getByRole('button', { name: '▶ Play' }).click();
  await p.getByText('00:01', { exact: true }).waitFor({ timeout: 3000 }).catch(() => fails.push('position did not follow the native clock'));
  const start = (await calls()).find(c => c.c === 'playback_set');
  if (!start || !start.playing || start.atMs !== 0 || start.rate !== 1) fails.push('Play did not start at 0: ' + JSON.stringify(start));
  await p.getByRole('button', { name: '❚❚ Pause' }).click();
  await p.waitForTimeout(200);
  const pause = (await calls()).filter(c => c.c === 'playback_set').at(-1);
  if (!pause || pause.playing) fails.push('Pause did not stop native audio: ' + JSON.stringify(pause));
  await p.locator('.tline[data-line="1"] button.ts').click();
  await p.waitForTimeout(300);
  const seek = (await calls()).filter(c => c.c === 'playback_set' && c.playing).at(-1);
  if (!seek || seek.atMs !== 30000) fails.push('a transcript time did not seek natively: ' + JSON.stringify(seek));
  await p.close();
  return fails.length ? fails : 'PASS';
}
