// Read in matches the approved mockup (owner 2026-10-26): the button, its list and the line over
// the notes compute the same styles in the app and the mockup, light and dark.
async (page) => {
  const fails = [];
  const props = ['fontSize', 'fontWeight', 'lineHeight', 'color', 'backgroundColor', 'borderTopColor', 'borderRadius', 'padding', 'gap', 'height', 'width'];
  const styles = (loc, keys) => loc.first().evaluate((e, keys) => Object.fromEntries(keys.map(k => [k, getComputedStyle(e)[k]])), keys);
  for (const theme of ['light', 'dark']) {
    const ref = await page.context().newPage();
    await ref.route('https://**/*', r => r.abort());
    await ref.emulateMedia({ colorScheme: theme });
    await ref.setViewportSize({ width: 1400, height: 1000 });
    await ref.goto('http://127.0.0.1:3001/motetto-app-mockup.html');
    await ref.evaluate(t => document.documentElement.setAttribute('data-theme', t), theme);
    await ref.getByRole('button', { name: 'macOS', exact: true }).click();
    await ref.getByRole('button', { name: 'After the call', exact: true }).click();

    const p = await page.context().newPage(); p.on('pageerror', e => fails.push(e.message));
    await p.emulateMedia({ colorScheme: theme });
    await p.setViewportSize({ width: 1200, height: 760 });
    await p.addInitScript(() => {
      try { localStorage.removeItem('motetto.readIn'); } catch {}
      const summary = { id: '1', title: 'FCM vs Pusher Beams', created_ms: Date.now(), status: 'Ready', marks_ms: [], duration_ms: 87000, notes_ms: 48000, spellings: [], locked: false, language: 'en' };
      const notes = '### Action items\n- [ ] You: Share the device table design [01:20]\n### Overview\n- Compared **FCM**.\n';
      const meeting = { summary, transcript: '[01:20] Me: share', transcript_heard: '', notes_md: notes };
      let next = 0, saved = false; const callbacks = {}, listeners = {};
      window.emit = (event, payload) => (listeners[event] || []).forEach(id => callbacks[id]?.({ event, id, payload }));
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
      window.__TAURI_INTERNALS__ = { convertFileSrc: x => x, transformCallback: cb => { const id = ++next; callbacks[id] = cb; return id; }, unregisterCallback: id => { delete callbacks[id]; }, invoke: async (c, a) => {
        if (c === 'plugin:event|listen') { (listeners[a.event] ||= []).push(a.handler); return a.handler; }
        if (c === 'get_state') return { state: { type: 'Idle' }, meeting_id: null, companion: { visible: false, size: 'Pill' }, elapsed_ms: 0, quit_pending: false };
        if (c === 'notification_permission') return true;
        if (c === 'list_meetings') return [summary]; if (c === 'deleted_meetings') return []; if (c === 'get_meeting') return meeting;
        if (c === 'meeting_media') return { tracks: [], duration_ms: 87000, folder: 'x' };
        if (c === 'notes_translations') return saved ? [{ code: 'ms', markdown: '', stale: false }] : [];
        if (c === 'get_translation') return saved ? { code: 'ms', markdown: '### Item tindakan\n- [ ] You: Kongsi reka bentuk jadual peranti [01:20]\n### Gambaran keseluruhan\n- Membandingkan **FCM**.\n', stale: false } : null;
        if (c === 'translating') return null;
        if (c === 'translate_notes') { window.emit('motetto://translate', { id: '1', code: 'ms', part: 1, of: 2, done: false, error: null }); await new Promise(r => { window.finishRun = r; }); saved = true; window.emit('motetto://translate', { id: '1', code: 'ms', part: 2, of: 2, done: true, error: null }); return null; }
        return false; } };
    });
    await p.goto('http://127.0.0.1:3000/main/');
    await p.getByRole('button', { name: /FCM vs Pusher Beams/ }).first().click();
    await p.getByRole('button', { name: /Read in/ }).waitFor();

    const compare = async (what, app, mock, keys = props) => {
      const a = await styles(app, keys), m = await styles(mock, keys);
      for (const k of keys) if (a[k] !== m[k]) fails.push(`${theme} ${what} ${k}: app ${a[k]} vs mockup ${m[k]}`);
    };
    await compare('button', p.locator('.readin-button'), ref.locator('.readin-button'));
    // The list, opened in both, searched to one language.
    await p.locator('.readin-button').click(); await ref.locator('.readin-button').click();
    await compare('popover', p.locator('.readin-pop'), ref.locator('.readin-pop'), props.filter(k => k !== 'height'));
    await compare('search', p.locator('.readin-search'), ref.locator('.readin-search'));
    await compare('group', p.locator('.readin-group'), ref.locator('.readin-group'), props.filter(k => k !== 'width'));
    await compare('row', p.locator('.readin-row').nth(1), ref.locator('.readin-row').nth(1), props.filter(k => k !== 'width'));
    await compare('foot', p.locator('.readin-foot'), ref.locator('.readin-foot'), props.filter(k => k !== 'height'));
    await p.locator('.readin-pop').screenshot({ path: `.playwright-out/read-in-pop-app-${theme}.png` });
    await ref.locator('.readin-pop').screenshot({ path: `.playwright-out/read-in-pop-mockup-${theme}.png` });
    await p.getByRole('combobox', { name: 'Search languages' }).fill('malay');
    await ref.getByRole('combobox', { name: 'Search languages' }).fill('malay');
    await p.keyboard.press('Enter'); await ref.keyboard.press('Enter');
    await p.locator('.readin-bar.running').waitFor(); await ref.locator('.readin-bar.running').waitFor();
    await compare('running bar', p.locator('.readin-bar'), ref.locator('.readin-bar'), props.filter(k => k !== 'width' && k !== 'height'));
    await p.evaluate(() => window.finishRun());
    await p.getByText('Kongsi reka bentuk').waitFor(); await ref.locator('#readinOrig').waitFor();
    await compare('translated bar', p.locator('.readin-bar'), ref.locator('.readin-bar'), props.filter(k => k !== 'width' && k !== 'height'));
    await compare('label heading', p.locator('.notes h3'), ref.locator('#doc .notes h3'), ['fontSize', 'fontWeight', 'color']);
    await p.locator('.document').screenshot({ path: `.playwright-out/read-in-app-${theme}.png` });
    await ref.locator('.mainwin').screenshot({ path: `.playwright-out/read-in-mockup-${theme}.png` });
    await p.close(); await ref.close();
  }
  return fails.length ? fails : 'PASS';
}
