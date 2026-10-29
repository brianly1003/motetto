// The meeting line names the call app it was recorded from, after the length (mockup .meta);
// a meeting without one (an import, or saved before apps were kept) shows no empty slot.
async (page) => {
  const fails = [];
  const p = await page.context().newPage(); p.on('pageerror', e => fails.push(e.message));
  await p.setViewportSize({ width: 1200, height: 760 });
  await p.addInitScript(() => {
    const base = { created_ms: Date.now(), status: 'Ready', marks_ms: [], duration_ms: 87000, notes_ms: 48000, spellings: [], locked: false, language: 'en', notes_language: null };
    const summaries = [{ ...base, id: '1', title: 'FCM', apps: ['Zoom'] }, { ...base, id: '2', title: 'Imported', created_ms: base.created_ms - 60000 }];
    let next = 0; const callbacks = {};
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = { convertFileSrc: x => x, transformCallback: cb => { const id = ++next; callbacks[id] = cb; return id; }, unregisterCallback: id => { delete callbacks[id]; }, invoke: async (c, a) => {
      if (c === 'plugin:event|listen') return a.handler;
      if (c === 'get_state') return { state: { type: 'Idle' }, meeting_id: null, companion: { visible: false, size: 'Pill' }, elapsed_ms: 0, quit_pending: false };
      if (c === 'list_meetings') return summaries; if (c === 'deleted_meetings') return [];
      if (c === 'get_meeting') return { summary: summaries.find(s => s.id === a.id), transcript: '[00:01] Me: x', transcript_heard: '', notes_md: '### Overview\n- x\n' };
      if (c === 'meeting_media') return { tracks: [], duration_ms: 87000, folder: 'x' };
      if (c === 'notes_translations') return []; if (c === 'translating') return null;
      return false; } };
  });
  await p.goto('http://127.0.0.1:3000/main/');
  const meta = async (name) => { await p.getByRole('button', { name }).first().click(); await p.locator('h1', { hasText: name }).waitFor(); await p.waitForTimeout(500); return p.locator('.document-meta').evaluate(e => [...e.querySelectorAll(':scope > span:not(.sr-only)')].map(s => s.textContent)); };
  const zoom = await meta('FCM');
  if (zoom[2] !== 'Zoom') fails.push('Zoom meeting line: ' + JSON.stringify(zoom));
  const imported = await meta('Imported');
  if (imported.length !== 3 || imported.includes('')) fails.push('import line: ' + JSON.stringify(imported));
  await p.close();
  return fails.length ? fails : 'PASS';
}
