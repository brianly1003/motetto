// Rehearse (owner 2026-10-28, SPEC 14.6): Rehearse starts a mic-only rehearsal; when it stops the
// Scripts page shows what was measured (time by section, pace, skipped lines, off-script,
// fillers), asks the notes model only for shorter long lines, Use in script saves the change,
// Keep keeps the audio, and leaving the report without Keep deletes it.
async (page) => {
  const fails = [];
  const p = await page.context().newPage(); p.on('pageerror', e => fails.push(e.message));
  await p.setViewportSize({ width: 1200, height: 900 });
  await p.addInitScript(() => {
    const long = 'Pick the metrics and watch the chart build itself while I keep talking about it for a while longer.';
    const files = { 'Team X demo': { text: `# Opening (1m)\nThanks for having us today everyone.\n\nThe short version is simple and quick.\n\n# Demo (2m)\nThis is the reports page with three cards.\n\n${long}`, ms: 1000 } };
    const at = (s, t) => `[00:${String(s).padStart(2, '0')}] Me: ${t}`;
    const transcript = [at(0, 'um thanks for having us today everyone'), at(10, 'this is the reports page with three cards'), at(20, 'pick the metrics and watch the chart build itself while I keep talking about it for a while longer')].join('\n');
    let next = 0; const callbacks = {}, listeners = {};
    window.emit = (event, payload) => (listeners[event] || []).forEach(id => callbacks[id]?.({ event, id, payload }));
    window.calls = []; window.files = files;
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = { metadata: { currentWindow: { label: 'main' }, currentWebview: { windowLabel: 'main', label: 'main' } }, convertFileSrc: x => x, transformCallback: cb => { const id = ++next; callbacks[id] = cb; return id; }, unregisterCallback: id => { delete callbacks[id]; }, invoke: async (c, a) => {
      if (c === 'plugin:event|listen') { (listeners[a.event] ||= []).push(a.handler); return a.handler; }
      if (c === 'get_state') return { state: { type: 'Idle' }, meeting_id: null, companion: { visible: false, size: 'Pill' }, elapsed_ms: 0, quit_pending: false, share_visible: false };
      if (c === 'list_meetings' || c === 'deleted_meetings' || c === 'open_action_items') return [];
      if (c === 'scripts_list') return Object.entries(files).map(([name, f]) => ({ name, modified_ms: f.ms, bytes: f.text.length }));
      if (c === 'script_read') return files[a.name].text;
      if (c === 'script_save') { window.calls.push(`save:${a.baseModifiedMs}:${a.text.includes('Pick metrics and watch the chart.')}`); files[a.name] = { text: a.text, ms: 2000 }; return 2000; }
      if (c === 'rehearse_start') { window.calls.push('start:' + a.script); return { id: '42', script: a.script, duration_ms: null }; }
      if (c === 'rehearsal_transcript') { window.calls.push('transcript:' + a.id); return transcript; }
      if (c === 'rehearsal_shorten') { window.calls.push('shorten'); return 'Pick metrics and watch the chart.'; }
      window.calls.push(c + (a && Object.keys(a).length ? ':' + JSON.stringify(a) : ''));
      return null; } };
  });
  await p.goto('http://127.0.0.1:3000/main/');
  await p.getByRole('button', { name: 'Scripts' }).first().click();
  await p.getByRole('button', { name: 'Rehearse' }).click();
  // Wherever the library is, a finished rehearsal opens its report.
  await p.getByRole('button', { name: 'Action items' }).first().click().catch(() => {});
  if (!(await p.evaluate(() => window.calls)).includes('start:Team X demo')) fails.push('Rehearse did not start');
  await p.evaluate(() => window.emit('motetto://rehearsal', { id: '42', script: 'Team X demo', duration_ms: 70000 }));
  const big = p.locator('.ptrep .big'); await big.waitFor();
  if (!/^1:10/.test(await big.innerText()) || !/of 3:00 planned/.test(await big.innerText())) fails.push('total: ' + await big.innerText());
  const kp = await p.locator('.ptrep .kp').innerText();
  if (!/1\s*lines skipped/.test(kp) || !/1\s*filler words/.test(kp)) fails.push('numbers: ' + kp);
  const secs = await p.locator('.ptrep .sec').allInnerTexts();
  if (secs.length !== 2 || !/1 Opening/.test(secs[0]) || !/0:1[01] \/ 1:00/.test(secs[0])) fails.push('sections: ' + JSON.stringify(secs));
  // Listen back: Play starts the native player on this rehearsal from the start; Pause stops it.
  await p.getByRole('button', { name: '▶ Play' }).click();
  await p.waitForTimeout(250);
  if (!(await p.evaluate(() => window.calls)).includes('playback_set:{"id":"rehearsal:42","playing":true,"atMs":0,"rate":1}')) fails.push('Play did not start the rehearsal audio: ' + JSON.stringify((await p.evaluate(() => window.calls)).filter(c => c.startsWith('playback') || c.startsWith('rehearsal_audio'))));
  await p.getByRole('button', { name: '❚❚ Pause' }).click();
  await p.waitForTimeout(100);
  if ((await p.evaluate(() => window.calls)).filter(c => c.startsWith('playback_set')).slice(-1)[0] !== 'playback_set:{"id":"rehearsal:42","playing":false,"atMs":null,"rate":1}') fails.push('Pause did not stop the audio');
  if (!/The short version is simple and quick/.test(await p.locator('.ptrep li').filter({ hasText: 'Skipped' }).innerText().catch(() => ''))) fails.push('skipped line not named');
  await p.waitForFunction(() => /Pick metrics and watch the chart\./.test(document.querySelector('.ptrep .sugg')?.textContent || ''), null, { timeout: 3000 }).catch(() => fails.push('no shorter suggestion'));
  if ((await p.evaluate(() => window.calls)).filter(c => c === 'shorten').length !== 1) fails.push('shorten asked for other than the one long line');
  await p.getByRole('button', { name: 'Use in script' }).click();
  await p.waitForTimeout(200);
  if (!(await p.evaluate(() => window.calls)).includes('save:1000:true')) fails.push('Use in script did not save: ' + JSON.stringify(await p.evaluate(() => window.calls)));
  await p.getByRole('button', { name: 'Undo' }).click();
  await p.waitForTimeout(200);
  if ((await p.evaluate(() => window.calls)).filter(c => c.startsWith('save:')).slice(-1)[0] !== 'save:2000:false') fails.push('Undo did not put the line back: ' + JSON.stringify(await p.evaluate(() => window.calls)));
  await p.getByRole('button', { name: 'Use in script' }).click();
  await p.waitForTimeout(200);
  await p.locator('.ptrep').screenshot({ path: '.playwright-out/rehearsal-report-app.png' }).catch(() => {});
  await p.getByRole('button', { name: 'Keep this recording' }).click();
  if (!(await p.evaluate(() => window.calls)).includes('rehearsal_keep:{"id":"42","script":"Team X demo"}')) fails.push('Keep');
  await p.getByRole('button', { name: 'Edit script' }).click();
  await p.waitForTimeout(200);
  if ((await p.evaluate(() => window.calls)).some(c => c.startsWith('rehearsal_discard'))) fails.push('kept audio was deleted');
  if (!(await p.evaluate(() => window.calls)).includes('playback_close:{"id":"rehearsal:42"}')) fails.push('leaving the report did not close the player');
  // A second rehearsal not kept: leaving deletes it.
  await p.getByRole('button', { name: 'Rehearse' }).click();
  await p.evaluate(() => window.emit('motetto://rehearsal', { id: '43', script: 'Team X demo', duration_ms: 60000 }));
  await big.waitFor();
  if (!/vs rehearsal 1/.test(await p.locator('.ptrep .kp').innerText())) fails.push('no comparison with the last rehearsal');
  await p.getByRole('button', { name: 'Edit script' }).click();
  await p.waitForTimeout(200);
  if (!(await p.evaluate(() => window.calls)).includes('rehearsal_discard:{"id":"43"}')) fails.push('not kept: audio not deleted');
  // Leaving the library (a meeting starting) deletes the audio; coming back must not reopen its report.
  await p.getByRole('button', { name: 'Rehearse' }).click();
  await p.evaluate(() => window.emit('motetto://rehearsal', { id: '44', script: 'Team X demo', duration_ms: 60000 }));
  await big.waitFor();
  await p.evaluate(() => window.emit('motetto://open-meeting', 'm1'));
  await p.waitForFunction(() => !document.querySelector('.ptrep'));
  await p.getByRole('button', { name: 'Scripts' }).first().click();
  await p.waitForTimeout(300);
  if ((await p.evaluate(() => window.calls)).filter(c => c === 'transcript:44').length !== 1 || await p.locator('.ptrep, .scripts-hint[role=alert]').count()) fails.push('deleted rehearsal reopened: ' + JSON.stringify(await p.evaluate(() => window.calls)));
  await p.close();
  return fails.length ? fails : 'ok';
}
