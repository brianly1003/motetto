// Script prompter (owner 2026-10-28, SPEC 14): the strip shows the previous, current and next
// line with said words in the Me colour, follows what Rust heard (follow-heard) and sends hints,
// sends its control rectangles, hides the text when the share check fails or can't run, switches
// Hidden / Shown, edits in place with a debounced save and leaves editing on Esc.
async (page) => {
  const fails = [];
  const p = await page.context().newPage(); p.on('pageerror', e => fails.push(e.message));
  await p.setViewportSize({ width: 560, height: 132 });
  await p.addInitScript(() => {
    const script = '# Opening (1m)\nThanks for having us today.\n\nThe short version is simple.\n\n# Demo (5m)\nThis is the reports page.\n\n[click New] Make one for sales.';
    const shortcuts = { Down: 'Ctrl+Alt+Down', Up: 'Ctrl+Alt+Up', Backslash: 'Ctrl+Alt+Backslash' };
    let next = 0; const callbacks = {}, listeners = {};
    window.emit = (event, payload) => (listeners[event] || []).forEach(id => callbacks[id]?.({ event, id, payload }));
    window.calls = []; window.live = ''; window.state = { state: { type: 'Recording' }, share_visible: false, sharing: null, companion: { visible: true, size: 'Pill' }, elapsed_ms: 0 };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = { metadata: { currentWindow: { label: 'prompter' }, currentWebview: { windowLabel: 'prompter', label: 'prompter' } }, convertFileSrc: x => x, transformCallback: cb => { const id = ++next; callbacks[id] = cb; return id; }, unregisterCallback: id => { delete callbacks[id]; }, invoke: async (c, a) => {
      if (c === 'plugin:event|listen') { (listeners[a.event] ||= []).push(a.handler); return a.handler; }
      if (c === 'get_state') return window.state;
      if (c === 'prompter_state') return { script: 'Demo', check: window.firstCheck || 'checking', editing: false, layout: 'Strip' };
      if (c === 'get_settings') return { prompter: { layout: 'Strip', text_size: 22, last_script: 'Demo' }, shortcuts };
      if (c === 'scripts_list') return [{ name: 'Demo', modified_ms: 1000, bytes: script.length }];
      if (c === 'script_read') return script;
      if (c === 'live_transcript') return ['m1', window.live, ''];
      if (c === 'script_save') { window.calls.push(`save:${a.name}:${a.baseModifiedMs}:${a.text.length}`); return 2000; }
      if (c === 'prompter_controls') { window.rects = a.rects; return null; }
      if (c === 'rehearse_stop' && window.failStop) throw new Error('Microphone is still stopping.');
      window.calls.push(c + (a && Object.keys(a).length ? ':' + JSON.stringify(a) : ''));
      return null; } };
  });
  await p.goto('http://127.0.0.1:3014/prompter/');
  await p.locator('.ptln.cur').waitFor({ state: 'attached' });
  const cur = () => p.locator('.ptln.cur').textContent();
  if (!/Thanks for having us today/.test(await cur())) fails.push('first line: ' + await cur());
  // While the check runs the text is not shown (behaves as visible).
  if (await p.locator('.ptlines').evaluate(e => getComputedStyle(e).visibility) !== 'hidden') fails.push('text shown while checking');
  if (!/Checking/.test(await p.locator('.ptvis').innerText())) fails.push('switch does not say Checking');
  if (await p.locator('.ptchip').count()) fails.push('section title shown while checking');
  // Editing asked while the check runs (E pressed early): the editor stays shut.
  await p.evaluate(() => window.emit('motetto://prompter-editing', true));
  if (await p.locator('textarea.pted').count()) fails.push('editor opened while checking');
  await p.evaluate(() => window.emit('motetto://prompter-editing', false));
  // The marker sits outside the strip and keeps full colour while the pointer fades the strip.
  await p.evaluate(() => { window.emit('motetto://prompter-through', true); window.emit('motetto://prompter-marker', true); });
  await p.waitForTimeout(200);
  const marker = await p.locator('.ptmarker').evaluate(e => ({ inside: !!e.closest('.pt'), opacity: getComputedStyle(e).opacity }));
  if (marker.inside || marker.opacity !== '1') fails.push('marker fades with the strip: ' + JSON.stringify(marker));
  if (+(await p.locator('.pt').evaluate(e => getComputedStyle(e).opacity)) < 1) fails.push('strip faded while the marker is drawn');
  await p.evaluate(() => { window.emit('motetto://prompter-marker', false); window.emit('motetto://prompter-through', false); });
  await p.evaluate(() => window.emit('motetto://prompter-check', 'hidden'));
  if (await p.locator('.ptlines').evaluate(e => getComputedStyle(e).visibility) !== 'visible') fails.push('text hidden after the check passed');
  if (await p.locator('.ptchip').innerText() !== '1/2 · Opening') fails.push('chip: ' + await p.locator('.ptchip').innerText());

  // Following: words you said turn the Me colour; the next line comes up at the end of a line.
  await p.evaluate(() => window.emit('motetto://follow-heard', { text: 'thanks for having', end_ms: 4000, inference_ms: 800 }));
  await p.waitForFunction(() => document.querySelectorAll('.ptln.cur .ptsaid').length === 3, null, { timeout: 3000 }).catch(() => fails.push('said words not marked'));
  const said = await p.locator('.ptln.cur .ptsaid').first().evaluate(e => getComputedStyle(e).color);
  const me = await p.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--voice-me').trim());
  if (!said || !me) fails.push('no Me colour');
  await p.evaluate(() => window.emit('motetto://follow-heard', { text: 'for having us today the short', end_ms: 5000, inference_ms: 800 }));
  await p.waitForFunction(() => /The short version/.test(document.querySelector('.ptln.cur')?.textContent || ''), null, { timeout: 3000 }).catch(() => fails.push('did not move to the next line'));
  if (!/Thanks for having us/.test(await p.locator('.ptln.prev').innerText().catch(() => ''))) fails.push('previous line not shown');
  // Keys from Rust move by line and by section.
  await p.evaluate(() => window.emit('motetto://prompter-key', 'ShiftDown'));
  if (!/This is the reports page/.test(await cur())) fails.push('next section: ' + await cur());
  await p.evaluate(() => window.emit('motetto://prompter-key', 'Down'));
  if (!(await p.evaluate(() => window.calls)).some(c => c.startsWith('follow_hint') && /make one for sales/.test(c) && !/click/.test(c))) fails.push('no hint with the next words: ' + JSON.stringify((await p.evaluate(() => window.calls)).filter(c => c.startsWith('follow_hint')).slice(-2)));
  if (!/click New/.test(await p.locator('.ptln.cur .ptcue').innerText().catch(() => ''))) fails.push('cue chip missing');
  await p.evaluate(() => window.emit('motetto://prompter-key', 'Backslash'));
  if (!/Paused · ⌃⌥\\ to follow|Paused · Ctrl\+Alt\+Backslash to follow/.test(await p.locator('.ptfollow').innerText())) fails.push('pause: ' + await p.locator('.ptfollow').innerText());

  // The mouse is taken only by the handle, the switch, edit and close.
  await p.waitForFunction(() => window.rects?.length === 4, null, { timeout: 2000 }).catch(() => fails.push('control rects: ' + JSON.stringify(p.evaluate(() => window.rects))));
  // Fading while the pointer is over the strip (Rust tells the page).
  await p.evaluate(() => window.emit('motetto://prompter-through', true));
  await p.waitForTimeout(200);
  if (+(await p.locator('.pt').evaluate(e => getComputedStyle(e).opacity)) > 0.2) fails.push('no fade');
  await p.evaluate(() => window.emit('motetto://prompter-through', false));

  // Hidden / Shown.
  await p.getByRole('switch').click();
  if (!(await p.evaluate(() => window.calls)).includes('set_share_visible:{"on":true}')) fails.push('switch did not ask for Shown');

  // Check failed: text gone, the ways out; Show for 15 s brings it back.
  await p.evaluate(() => window.emit('motetto://prompter-check', 'visible'));
  await p.waitForFunction(() => !document.querySelector('.ptlines'), null, { timeout: 1000 }).catch(() => fails.push('text drawn while the share can see it'));
  if (!/Your share can see this window/.test(await p.locator('.ptleak').innerText())) fails.push('leak message');
  await p.getByRole('button', { name: 'Show for 15 s' }).click();
  if (!await p.locator('.ptlines').count()) fails.push('Show for 15 s did not show the text');
  await p.evaluate(() => window.emit('motetto://prompter-check', 'unknown'));
  await p.reload(); await p.locator('.ptvis').waitFor();
  await p.evaluate(() => window.emit('motetto://prompter-check', 'unknown'));
  await p.waitForFunction(() => /couldn't check your share/.test(document.querySelector('.ptleak')?.textContent || '') && !document.querySelector('.ptlines'), null, { timeout: 1000 }).catch(() => fails.push('unknown_check_hides_text'));
  await p.getByRole('button', { name: 'Allow the check' }).click();
  if (!(await p.evaluate(() => window.calls)).some(c => c.startsWith('open_permission_settings'))) fails.push('Allow the check did nothing');
  await p.evaluate(() => window.emit('motetto://prompter-check', 'moved'));
  await p.waitForFunction(() => !!document.querySelector('.ptlines'), null, { timeout: 1000 }).catch(() => fails.push('moved to another display hides the text'));
  await p.evaluate(() => window.emit('motetto://prompter-check', 'hidden'));

  // Edit in place: a debounced save with the loaded time, Esc leaves and gives the keyboard back.
  await p.getByRole('button', { name: 'Edit the script' }).click();
  if (!(await p.evaluate(() => window.calls)).includes('prompter_edit:{"on":true,"restore":false}')) fails.push('✎ did not ask to edit');
  await p.evaluate(() => window.emit('motetto://prompter-editing', true));
  const box = p.locator('textarea.pted'); await box.waitFor();
  // The long editing hint shrinks; every header control stays inside the strip.
  const over = await p.evaluate(() => [...document.querySelectorAll('.pth > *')].filter(e => e.getBoundingClientRect().right > document.querySelector('.pt').getBoundingClientRect().right).map(e => e.className));
  if (over.length) fails.push('header overflows while editing: ' + over);
  await box.press('End'); await box.type(' Extra');
  await p.waitForTimeout(500);
  if (!(await p.evaluate(() => window.calls)).some(c => /^save:Demo:1000:/.test(c))) fails.push('no debounced save: ' + JSON.stringify(await p.evaluate(() => window.calls)));
  await box.press('Escape');
  if (!(await p.evaluate(() => window.calls)).includes('prompter_edit:{"on":false,"restore":true}')) fails.push('Esc did not give the keyboard back');

  // Marker for the share check: drawn, then acknowledged.
  await p.evaluate(() => window.emit('motetto://prompter-editing', false));
  await p.evaluate(() => window.emit('motetto://prompter-marker', true));
  await p.waitForFunction(() => window.calls.includes('prompter_marker_drawn'), null, { timeout: 2000 }).catch(() => fails.push('marker not acknowledged'));
  if (await p.locator('.ptmarker i').count() !== 16) fails.push('marker cells');
  const colours = await p.locator('.ptmarker i').evaluateAll(cells => cells.slice(0, 2).map(c => getComputedStyle(c).backgroundColor));
  if (colours[0] !== 'rgb(255, 0, 255)' || colours[1] !== 'rgb(0, 255, 0)') fails.push('marker colours ' + colours);
  // Rehearsing: the bar replaces the share switch, and Stop ends the rehearsal.
  await p.evaluate(() => window.emit('motetto://rehearsal', { id: '1', script: 'Demo', duration_ms: null }));
  await p.locator('.ptreh').waitFor({ timeout: 2000 }).catch(() => fails.push('no rehearsing bar'));
  if (!/Rehearsing · mic only · stays on this Mac/.test(await p.locator('.ptreh').innerText().catch(() => ''))) fails.push('rehearsing wording');
  if (await p.getByRole('switch').count()) fails.push('share switch shown while rehearsing');
  await p.locator('.ptreh button').click();
  if (!(await p.evaluate(() => window.calls)).includes('rehearse_stop')) fails.push('Stop did not end the rehearsal');
  if (!(await p.evaluate(() => window.rects?.length)) || (await p.evaluate(() => window.rects.length)) < 3) fails.push('Stop is not a control rectangle');
  await p.evaluate(() => { window.failStop = true; window.calls = []; });
  await p.getByRole('button', { name: 'Close script', exact: true }).click();
  await p.getByRole('alert').filter({ hasText: 'Microphone is still stopping.' }).waitFor();
  if ((await p.evaluate(() => window.calls)).includes('prompter_hide')) fails.push('failed Stop hid the strip');
  await p.evaluate(() => { window.failStop = false; window.calls = []; });
  // Closing stops rehearsal before hiding; a failed stop leaves its controls visible.
  await p.getByRole('button', { name: 'Close script', exact: true }).click();
  const closing = await p.evaluate(() => window.calls.filter(c => ['rehearse_stop', 'prompter_hide'].includes(c)).slice(-2));
  if (closing.join(',') !== 'rehearse_stop,prompter_hide') fails.push('close order: ' + closing);
  await p.evaluate(() => { window.emit('motetto://rehearsal', { duration_ms: 1000 }); window.calls = []; });
  await p.getByRole('button', { name: 'Close script', exact: true }).click();
  if ((await p.evaluate(() => window.calls)).join(',') !== 'prompter_hide') fails.push('closing script affected recording');
  for (const theme of ['light', 'dark']) {
    await p.emulateMedia({ colorScheme: theme });
    for (const width of [300, 560]) {
      await p.setViewportSize({ width, height: 204 });
      await p.evaluate(() => window.emit('motetto://rehearsal', { duration_ms: null }));
      const r = await p.locator('.ptclose').boundingBox();
      if (!r || r.x < 0 || r.x + r.width > width) fails.push('close clipped: ' + theme + width);
    }
    await p.screenshot({ path: '.playwright-out/prompter-close-' + theme + '.png' });
  }
  await p.close();
  return fails.length ? fails : 'ok';
}
