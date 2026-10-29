// Measured transcription progress matches the approved mockup (owner 2026-10-28): the step's percent
// and time left, the line below it and the bar compute the same styles in the app and the mockup,
// light and dark.
async (page) => {
  const fails = [];
  const props = ['fontSize', 'fontFamily', 'color', 'marginTop', 'gap', 'justifyContent', 'whiteSpace', 'textOverflow'];
  const styles = loc => loc.first().evaluate((e, keys) => Object.fromEntries(keys.map(k => [k, getComputedStyle(e)[k]])), props);
  const parts = root => ({ measured: root.locator('.step.measured > small'), sub: root.locator('.step.measured .sub'), line: root.locator('.step.measured .sub > span') });
  for (const theme of ['light', 'dark']) {
    const ref = await page.context().newPage();
    await ref.route('https://**/*', r => r.abort());
    await ref.emulateMedia({ colorScheme: theme, reducedMotion: 'no-preference' });
    await ref.setViewportSize({ width: 1400, height: 1000 });
    await ref.goto('http://127.0.0.1:3001/motetto-app-mockup.html');
    await ref.evaluate(t => document.documentElement.setAttribute('data-theme', t), theme);
    await ref.getByRole('button', { name: 'macOS', exact: true }).click();
    await ref.getByRole('button', { name: 'After the call', exact: true }).click();
    await ref.getByRole('button', { name: 'More actions' }).click();
    await ref.locator('[data-retx]').first().click();
    await ref.locator('#rxGo').click();
    await ref.locator('.step.measured').waitFor({ timeout: 1000 });
    const mock = {};
    for (const [k, loc] of Object.entries(parts(ref))) mock[k] = await styles(loc);
    await ref.locator('.processing').screenshot({ path: `.playwright-out/transcribe-progress-mockup-${theme}.png` });

    const p = await page.context().newPage(); p.on('pageerror', e => fails.push(e.message));
    await p.emulateMedia({ colorScheme: theme });
    await p.setViewportSize({ width: 1200, height: 760 });
    await p.addInitScript(() => {
      const summary = { id: '1', title: 'Again', created_ms: Date.now(), status: 'Ready', marks_ms: [], duration_ms: 60000, notes_ms: 8000, spellings: [], locked: false, title_source: 'placeholder' };
      const meeting = { summary, transcript: '[00:04] Me: Hello', transcript_heard: '', notes_md: '### Overview\n- Notes.\n' };
      let next = 0; const callbacks = {}, listeners = {};
      window.emit = (event, payload) => (listeners[event] || []).forEach(id => callbacks[id]?.({ event, id, payload }));
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
      window.__TAURI_INTERNALS__ = { convertFileSrc: x => x, transformCallback: cb => { const id = ++next; callbacks[id] = cb; return id; }, unregisterCallback: id => { delete callbacks[id]; }, invoke: async (c, a) => {
        if (c === 'plugin:event|listen') { (listeners[a.event] ||= []).push(a.handler); return a.handler; }
        if (c === 'get_state') return { state: { type: 'Idle' }, meeting_id: null, companion: { visible: false, size: 'Pill' }, elapsed_ms: 0, quit_pending: false };
        if (c === 'list_meetings') return [summary]; if (c === 'deleted_meetings') return []; if (c === 'get_meeting') return meeting;
        if (c === 'importing') return { id: '1', import: false, transcribe: true };
        if (c === 'meeting_media') return { tracks: [], duration_ms: 60000, folder: 'x' };
        return false; } };
    });
    await p.goto('http://127.0.0.1:3000/main/');
    await p.getByRole('button', { name: /Again/ }).first().click();
    await p.locator('.writing').waitFor();
    await p.evaluate(() => window.emit('motetto://event', { type: 'Stage', meeting_id: '1', stage: { type: 'Transcribing', progress: { percent: 43, eta_s: 250, step: { type: 'Checking', side: 'Them', piece: 37, of: 148 } } }, percent: 43, eta_s: 250 }));
    await p.locator('.step.measured').waitFor();
    await p.waitForTimeout(600); // the bar's width transition
    const app = {};
    for (const [k, loc] of Object.entries(parts(p))) app[k] = await styles(loc);
    for (const k of Object.keys(mock)) for (const prop of props) {
      // Families differ only by the app's local font stack; sizes, colours and layout must match.
      if (prop === 'fontFamily') continue;
      if (mock[k][prop] !== app[k][prop]) fails.push(`${theme} ${k} ${prop}: mockup ${mock[k][prop]}, app ${app[k][prop]}`);
    }
    const texts = async root => [await root.locator('.step.measured > small').innerText(), await root.locator('.step.measured .sub > span').innerText()].join(' / ');
    if (await texts(ref) !== await texts(p)) fails.push(`${theme} text: mockup ${await texts(ref)}, app ${await texts(p)}`);
    await p.locator('.writing').screenshot({ path: `.playwright-out/transcribe-progress-app-${theme}.png` });
    await ref.close(); await p.close();
  }

  // After a call (mockup runProcessing, app ProcessingPage): the same steps and bar in each phase.
  const stepsOf = root => root.locator('.step').evaluateAll(rows => rows.map(r => `${r.className.replace(/step|measured/g, '').trim() || 'todo'}:${r.children[1].textContent}${r.classList.contains('measured') ? ' ' + (/%/.test(r.children[2].textContent) ? r.children[2].textContent : 'elapsed') + ' / ' + r.querySelector('.sub > span').textContent : ''}`).join(' | '));
  const barOf = root => root.locator('.progress i, .procbar i').first().evaluate(i => Math.round(parseFloat(i.style.width) * 10) / 10);
  for (const theme of ['light', 'dark']) {
    const ref = await page.context().newPage();
    await ref.route('https://**/*', r => r.abort());
    await ref.emulateMedia({ colorScheme: theme, reducedMotion: 'no-preference' });
    await ref.setViewportSize({ width: 1400, height: 1000 });
    await ref.goto('http://127.0.0.1:3001/motetto-app-mockup.html');
    await ref.evaluate(t => document.documentElement.setAttribute('data-theme', t), theme);
    await ref.getByRole('button', { name: 'macOS', exact: true }).click();
    await ref.getByRole('button', { name: 'During a call', exact: true }).click();
    await ref.locator('[data-act="stop"]:visible').first().click();
    await ref.locator('.processing .step.measured').waitFor({ timeout: 15000 });
    const mock = [[await stepsOf(ref.locator('.processing')), await barOf(ref.locator('.processing'))]];
    await ref.locator('.processing').screenshot({ path: `.playwright-out/after-call-mockup-${theme}.png` });
    await ref.locator('.processing .step.now', { hasText: 'Summarizing' }).waitFor();
    mock.push([await stepsOf(ref.locator('.processing')), await barOf(ref.locator('.processing'))]);
    await ref.close();

    const p = await page.context().newPage(); p.on('pageerror', e => fails.push(e.message));
    await p.emulateMedia({ colorScheme: theme });
    await p.setViewportSize({ width: 1200, height: 760 });
    await p.addInitScript(() => {
      const summary = { id: 'call', title: 'Meeting at 10:02', created_ms: Date.now(), status: 'Finishing', marks_ms: [], duration_ms: 87000, notes_ms: 0, spellings: [], locked: false, title_source: 'placeholder' };
      let next = 0; const callbacks = {}, listeners = {};
      window.emit = (event, payload) => (listeners[event] || []).forEach(id => callbacks[id]?.({ event, id, payload }));
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
      window.__TAURI_INTERNALS__ = { convertFileSrc: x => x, transformCallback: cb => { const id = ++next; callbacks[id] = cb; return id; }, unregisterCallback: id => { delete callbacks[id]; }, invoke: async (c, a) => {
        if (c === 'plugin:event|listen') { (listeners[a.event] ||= []).push(a.handler); return a.handler; }
        if (c === 'get_state') return { state: { type: 'Finishing', meeting_id: 'call' }, meeting_id: 'call', companion: { visible: false, size: 'Pill' }, elapsed_ms: 87000, quit_pending: false };
        if (c === 'list_meetings') return [summary]; if (c === 'get_meeting') return { summary, transcript: '', transcript_heard: '', notes_md: '' };
        if (c === 'deleted_meetings' || c === 'open_action_items') return [];
        if (c === 'meeting_media') return { tracks: [], duration_ms: 87000, folder: 'x' };
        return null; } };
    });
    await p.goto('http://127.0.0.1:3000/main/');
    await p.getByRole('button', { name: /Meeting at 10:02/ }).first().click();
    await p.locator('.writing').waitFor();
    const stage = s => p.evaluate(s => window.emit('motetto://event', { type: 'Stage', meeting_id: 'call', stage: s, percent: 0, eta_s: null }), s);
    await stage({ type: 'Transcribing', progress: { percent: 58, eta_s: null, step: { type: 'Transcribing', side: 'Them' } } });
    await p.locator('.writing .step.measured').waitFor();
    await p.waitForTimeout(600); // the bar's width transition, for the screenshot
    const app = [[await stepsOf(p.locator('.writing')), await barOf(p.locator('.writing'))]];
    await p.locator('.writing').screenshot({ path: `.playwright-out/after-call-app-${theme}.png` });
    await stage({ type: 'Summarizing', part: 1, of: 1, progress: { percent: null, eta_s: null, step: { type: 'Writing', words: 86 } } });
    await p.locator('.writing .step.now', { hasText: 'Summarizing' }).waitFor();
    app.push([await stepsOf(p.locator('.writing')), await barOf(p.locator('.writing'))]);
    for (const i of [0, 1]) if (JSON.stringify(mock[i]) !== JSON.stringify(app[i])) fails.push(`${theme} after a call, phase ${i}: mockup ${JSON.stringify(mock[i])}, app ${JSON.stringify(app[i])}`);
    await p.close();
  }
  return fails.length ? fails : 'PASS';
}
