// Keyboard help (owner 2026-10-30): no line under a long transcript or notes; the tab bar's keycap
// button or "?" opens the keys for the current tab, and Escape closes it.
async (page) => {
  const fails = [];
  const p = await page.context().newPage(); p.on('pageerror', e => fails.push(e.message));
  await p.setViewportSize({ width: 1200, height: 800 });
  await p.addInitScript(() => {
    const transcript = Array.from({ length: 1500 }, (_, i) => `[${String(Math.floor(i / 60)).padStart(2, '0')}:${String(i % 60).padStart(2, '0')}] ${i % 2 ? 'Them' : 'Me'}: Line ${i}`).join('\n');
    const summary = { id: '1', title: 'Long call', created_ms: Date.now(), status: 'Ready', marks_ms: [], duration_ms: 1500000, notes_ms: 8000, spellings: [], locked: false, language: 'en', notes_language: null, apps: [], unconfirmed: [] };
    const meeting = { summary, transcript, transcript_heard: '', notes_md: '### Overview\n- A long call [00:01]\n', has_video: false };
    let next = 0;
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = { convertFileSrc: x => x, transformCallback: () => ++next, unregisterCallback: () => {}, invoke: async (c, a) => {
      if (c === 'plugin:event|listen') return a.handler;
      if (c === 'get_state') return { state: { type: 'Idle' }, meeting_id: null, companion: { visible: false, size: 'Pill' }, elapsed_ms: 0, quit_pending: false };
      if (c === 'list_meetings') return [summary]; if (c === 'deleted_meetings') return []; if (c === 'get_meeting') return meeting;
      if (c === 'meeting_media') return { tracks: [], videos: [], duration_ms: 1500000, folder: 'x' };
      if (c === 'notes_translations') return []; if (c === 'translating') return null;
      return false; } };
  });
  await p.goto('http://127.0.0.1:3000/main/');
  await p.getByRole('button', { name: /Long call/ }).first().click();
  await p.getByRole('tab', { name: 'Transcript' }).click();
  await p.locator('.tline').first().waitFor();
  if (await p.getByText("Click a line's words to correct them").count()) fails.push('hint still under the transcript');
  const button = p.getByRole('button', { name: 'Keyboard shortcuts' });
  await button.click();
  const pop = p.locator('.keys-pop');
  if (!/Transcript/.test(await pop.innerText())) fails.push('popover: ' + await pop.innerText());
  if (!await pop.locator('dt', { hasText: 'Play the line' }).count()) fails.push('no Play the line key');
  await p.waitForTimeout(250); await p.screenshot({ path: '.playwright-out/shortcuts-transcript.png' });
  await p.keyboard.press('Escape');
  if (await pop.count()) fails.push('Escape did not close it');
  if (!await button.evaluate(b => b === document.activeElement)) fails.push('focus not back on the button');
  await p.getByRole('tab', { name: 'Notes' }).click();
  await p.locator('.notes li').first().waitFor();
  await p.locator('body').press('?');
  if (!/Notes/.test(await pop.innerText().catch(() => ''))) fails.push('? did not open the Notes keys');
  if (!await pop.locator('dt', { hasText: 'Assign an owner' }).count()) fails.push('no owner key');
  await p.emulateMedia({ colorScheme: 'dark' });
  await p.waitForTimeout(250); await p.screenshot({ path: '.playwright-out/shortcuts-notes-dark.png' });
  await p.mouse.click(600, 700);
  if (await pop.count()) fails.push('a click outside did not close it');
  await p.close();
  return fails.length ? fails : 'PASS';
}
