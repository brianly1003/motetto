// Not now (owner 2026-10-26): the offer stays hidden on every meeting until Motetto quits;
// a fresh start (a reload here) asks again. English, picked, recent and the system's, is suggested once.
async (page) => {
  const fails = [];
  const p = await page.context().newPage(); p.on('pageerror', e => fails.push(e.message));
  await p.setViewportSize({ width: 1200, height: 760 });
  await p.addInitScript(() => {
    try { localStorage.setItem('motetto.readIn', 'en'); localStorage.setItem('motetto.readIn.recent', '["en","ms"]'); } catch {}
    Object.defineProperty(navigator, 'languages', { value: ['en-US', 'en', 'vi-VN'] });
    const at = Date.now();
    const summaries = ['Kênh A', 'Kênh B'].map((title, i) => ({ id: String(i + 1), title, created_ms: at - i * 60000, status: 'Ready', marks_ms: [], duration_ms: 87000, notes_ms: 48000, spellings: [], locked: false, language: 'vi', notes_language: null }));
    let next = 0; const callbacks = {};
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = { convertFileSrc: x => x, transformCallback: cb => { const id = ++next; callbacks[id] = cb; return id; }, unregisterCallback: id => { delete callbacks[id]; }, invoke: async (c, a) => {
      if (c === 'plugin:event|listen') return a.handler;
      if (c === 'get_state') return { state: { type: 'Idle' }, meeting_id: null, companion: { visible: false, size: 'Pill' }, elapsed_ms: 0, quit_pending: false };
      if (c === 'list_meetings') return summaries; if (c === 'deleted_meetings') return [];
      if (c === 'get_meeting') { const summary = summaries.find(m => m.id === a.id); return { summary, transcript: '[00:10] Me: chào', transcript_heard: '', notes_md: `### Tổng quan\n- ${summary.title}\n` }; }
      if (c === 'meeting_media') return { tracks: [], duration_ms: 87000, folder: 'x' };
      if (c === 'notes_translations') return [];
      if (c === 'translating') return null;
      return false; } };
  });
  const open = async title => { await p.getByRole('button', { name: new RegExp(title) }).first().click(); await p.locator('.notes').getByText(title).waitFor(); };
  const offer = () => p.getByText('Read these notes in English?');
  await p.goto('http://127.0.0.1:3000/main/');
  await open('Kênh A');
  await offer().waitFor();
  await p.getByRole('button', { name: 'Not now' }).click();
  if (await offer().count()) fails.push('Not now did not hide the offer');
  await open('Kênh B');
  if (await offer().count()) fails.push('the offer came back on another meeting');
  await open('Kênh A');
  if (await offer().count()) fails.push('the offer came back on the first meeting');
  await p.reload();
  await open('Kênh A');
  await offer().waitFor({ timeout: 3000 }).catch(() => fails.push('a fresh start did not ask again'));
  await p.locator('.readin-button').click();
  const rows = await p.getByRole('option').allInnerTexts();
  if (rows.filter(r => /English/.test(r)).length !== 1) fails.push('English listed more than once: ' + JSON.stringify(rows.slice(0, 6)));
  if (!/Melayu/.test(rows[2] ?? '') || !/Tiếng Việt/.test(rows[3] ?? '')) fails.push('suggested order: ' + JSON.stringify(rows.slice(0, 5)));
  await p.close();
  return fails.length ? fails : 'PASS';
}
