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
  return fails.length ? fails : 'PASS';
}
