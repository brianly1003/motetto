// Write the notes again in a language (owner 2026-10-26): from the Read in line and from •••, the
// question offers the language and sends it; notes written in Melayu read as the original.
async (page) => {
  const fails = [];
  for (const theme of ['light', 'dark']) {
    const p = await page.context().newPage(); p.on('pageerror', e => fails.push(e.message));
    await p.emulateMedia({ colorScheme: theme });
    await p.setViewportSize({ width: 1200, height: 760 });
    await p.addInitScript(() => {
      try { localStorage.setItem('motetto.readIn', 'ms'); } catch {}
      const summary = { id: '1', title: 'FCM vs Pusher Beams', created_ms: Date.now(), status: 'Ready', marks_ms: [], duration_ms: 87000, notes_ms: 48000, spellings: [], locked: false, language: 'en,vi', notes_language: null };
      const meeting = { summary, transcript: '[01:20] Me: share', transcript_heard: '', notes_md: '### Action items\n- [ ] You: Share the device table design [01:20]\n' };
      let next = 0, saved = true; const callbacks = {};
      window.calls = [];
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
      window.__TAURI_INTERNALS__ = { convertFileSrc: x => x, transformCallback: cb => { const id = ++next; callbacks[id] = cb; return id; }, unregisterCallback: id => { delete callbacks[id]; }, invoke: async (c, a) => {
        if (c === 'plugin:event|listen') return a.handler;
        if (c === 'get_state') return { state: { type: 'Idle' }, meeting_id: null, companion: { visible: false, size: 'Pill' }, elapsed_ms: 0, quit_pending: false };
        if (c === 'list_meetings') return [summary]; if (c === 'deleted_meetings') return []; if (c === 'get_meeting') return meeting;
        if (c === 'meeting_media') return { tracks: [], duration_ms: 87000, folder: 'x' };
        if (c === 'get_settings') return { notes_language: 'same_as_call' };
        if (c === 'notes_translations') return saved ? [{ code: 'ms', markdown: '', stale: false }] : [];
        if (c === 'get_translation') return saved ? { code: 'ms', markdown: '### Item tindakan\n- [ ] You: Kongsi reka bentuk jadual peranti [01:20]\n', stale: false } : null;
        if (c === 'translating') return null;
        if (c === 'meeting_action') {
          window.calls.push(a);
          // As rewrite.rs does: Melayu notes, the Melayu translation removed.
          meeting.notes_md = '### Item tindakan\n- [ ] You: Kongsi reka bentuk jadual peranti [01:20]\n';
          summary.notes_language = a.language === 'same_as_call' ? null : a.language; saved = false; return null;
        }
        return false; } };
    });
    await p.goto('http://127.0.0.1:3000/main/');
    await p.getByRole('button', { name: /FCM vs Pusher Beams/ }).first().click();

    // Reading the Melayu translation: the line offers Write in Melayu, which asks with it chosen.
    await p.getByText('Kongsi reka bentuk').waitFor();
    await p.getByRole('button', { name: 'Write in Melayu', exact: true }).click();
    const dialog = p.getByRole('alertdialog');
    await dialog.waitFor();
    const select = dialog.getByRole('combobox');
    if (await select.inputValue() !== 'ms') fails.push(`${theme}: Write in did not preselect Melayu`);
    if (!await dialog.getByRole('option', { name: 'Same as the call (English, Vietnamese)' }).count()) fails.push(`${theme}: call languages missing`);
    if (await dialog.locator('optgroup').count() !== 2) fails.push(`${theme}: expected Suggested and All languages`);
    await dialog.screenshot({ path: `.playwright-out/write-in-question-${theme}.png` });
    await p.locator('.readin-bar').screenshot({ path: `.playwright-out/write-in-bar-${theme}.png` });
    // The field and the line's button compute the mockup's styles.
    const ref = await page.context().newPage();
    await ref.route('https://**/*', r => r.abort());
    await ref.emulateMedia({ colorScheme: theme });
    await ref.setViewportSize({ width: 1400, height: 1000 });
    await ref.goto('http://127.0.0.1:3001/motetto-app-mockup.html');
    await ref.evaluate(t => document.documentElement.setAttribute('data-theme', t), theme);
    await ref.getByRole('button', { name: 'macOS', exact: true }).click();
    await ref.getByRole('button', { name: 'After the call', exact: true }).click();
    await ref.locator('.readin-button').click();
    await ref.getByRole('combobox', { name: 'Search languages' }).fill('malay');
    await ref.keyboard.press('Enter');
    await ref.locator('#readinWrite').click();
    const keys = ['fontSize', 'fontWeight', 'color', 'backgroundColor', 'borderTopColor', 'borderRadius', 'height', 'padding', 'gap'];
    const styles = loc => loc.first().evaluate((e, keys) => Object.fromEntries(keys.map(k => [k, getComputedStyle(e)[k]])), keys);
    for (const [app, mock] of [['.writein', '.writein'], ['.writein select', '.writein select'], ['.writein > span', '.writein > span'], ['.writein-help', '.writein-help']]) {
      const a = await styles(dialog.locator(app)), m = await styles(ref.locator(mock));
      for (const k of keys) if (a[k] !== m[k] && !(k === 'height' && app === '.writein-help')) fails.push(`${theme} ${app} ${k}: app ${a[k]} vs mockup ${m[k]}`);
    }
    await ref.close();
    // Choosing another language changes the button; Cancel sends nothing.
    await select.selectOption('vi');
    if (!await dialog.getByRole('button', { name: 'Write in Tiếng Việt' }).count()) fails.push(`${theme}: button did not follow the language`);
    await select.selectOption('same_as_call');
    if (!await dialog.getByRole('button', { name: 'Confirm' }).count()) fails.push(`${theme}: Same as the call should confirm plainly`);
    await dialog.getByRole('button', { name: 'Cancel' }).click();
    if (await p.evaluate(() => window.calls.length)) fails.push(`${theme}: Cancel sent a request`);

    // From •••: the language you are reading is offered first.
    await p.getByRole('button', { name: 'More actions' }).click();
    await p.getByRole('menuitem', { name: 'Write the notes again' }).click();
    if (await p.getByRole('alertdialog').getByRole('combobox').inputValue() !== 'ms') fails.push(`${theme}: ••• did not offer the reading language`);
    await p.getByRole('alertdialog').getByRole('button', { name: 'Write in Melayu' }).click();
    await p.waitForFunction(() => window.calls.length === 1);
    const call = await p.evaluate(() => window.calls[0]);
    if (call.action !== 'regenerate' || call.language !== 'ms') fails.push(`${theme}: sent ${JSON.stringify(call)}`);

    // Written in Melayu: it is the original now. No offer, the button names it, no Melayu row.
    await p.getByRole('button', { name: /Read in/ }).filter({ hasText: 'Melayu' }).waitFor();
    if (await p.locator('.readin-bar').count()) fails.push(`${theme}: a Read in line shows over notes already in Melayu`);
    await p.locator('.readin-button').click();
    if (!await p.getByText('Written in Melayu').count()) fails.push(`${theme}: Original does not say Written in Melayu`);
    await p.getByRole('combobox', { name: 'Search languages' }).fill('malay');
    if (await p.locator('.readin-row').count()) fails.push(`${theme}: Melayu still offered as a translation`);
    await p.keyboard.press('Escape');
    await p.locator('.document').screenshot({ path: `.playwright-out/write-in-done-${theme}.png` });
    await p.close();
  }
  return fails.length ? fails : 'PASS';
}
