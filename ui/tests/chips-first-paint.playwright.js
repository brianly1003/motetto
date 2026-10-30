// Time chips render with the notes, not after the audio is read (owner 2026-10-30): "[09:47]" used to
// show as raw text until meeting_media answered, then turn into a chip.
async (page) => {
  const fails = [];
  const p = await page.context().newPage(); p.on('pageerror', e => fails.push(e.message));
  await p.setViewportSize({ width: 1200, height: 800 });
  await p.addInitScript(() => {
    const summary = { id: '1', title: 'Chips', created_ms: Date.now(), status: 'Ready', marks_ms: [], duration_ms: 600000, notes_ms: 8000, spellings: [], locked: false, language: 'en', notes_language: null, apps: [], unconfirmed: [] };
    const meeting = { summary, transcript: '[09:47] Them: Ship it on Friday.', transcript_heard: '', notes_md: '### Overview\n- Ship on **Friday** [09:47]\n', has_video: false };
    let next = 0;
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = { convertFileSrc: x => x, transformCallback: () => ++next, unregisterCallback: () => {}, invoke: async (c, a) => {
      if (c === 'plugin:event|listen') return a.handler;
      if (c === 'get_state') return { state: { type: 'Idle' }, meeting_id: null, companion: { visible: false, size: 'Pill' }, elapsed_ms: 0, quit_pending: false };
      if (c === 'list_meetings') return [summary]; if (c === 'deleted_meetings') return []; if (c === 'get_meeting') return meeting;
      if (c === 'meeting_media') { await new Promise(r => setTimeout(r, 1500)); return { tracks: [], videos: [], duration_ms: 600000, folder: 'x' }; }
      if (c === 'notes_translations') return []; if (c === 'translating') return null;
      return false; } };
  });
  await p.goto('http://127.0.0.1:3000/main/');
  await p.getByRole('button', { name: /Chips/ }).first().click();
  const line = p.locator('.notes li', { hasText: 'Ship on' });
  await line.waitFor();
  // Seen before the media answers: a chip, never the bracketed text.
  if (/\[09:47\]/.test(await line.innerText())) fails.push('raw time before the media loaded');
  if (await line.locator('button.src').count() !== 1) fails.push('no chip on first paint');
  await p.waitForTimeout(1800);
  if (await line.locator('button.src').count() !== 1) fails.push('chip lost after the media loaded');
  await p.close();
  return fails.length ? fails : 'PASS';
}
