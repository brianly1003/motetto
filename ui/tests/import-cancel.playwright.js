// An import runs beside everything else (SPEC 7.7): its meeting opens on its own progress page,
// other meetings stay usable, a second Import waits with its reason, and Cancel shows Stopping…
// then closes the page with a toast. Home's checkboxes match the notes' tasks.
async (page) => {
  const fails = [];
  const browser = await page.context().browser().browserType().launch({ channel: 'chrome' });
  const context = await browser.newContext();
  try {
    for (const theme of ['light', 'dark']) {
      const p = await context.newPage();
      await p.emulateMedia({ colorScheme: theme }); p.on('pageerror', e => fails.push(e.message));
      await p.setViewportSize({ width: 1200, height: 720 });
      await p.addInitScript(() => {
        const mk = (id, title, status, notes) => ({ summary: { id, title, created_ms: Date.now() - +id * 1000, status, marks_ms: [], duration_ms: 87000, notes_ms: 48000, spellings: [], locked: false }, transcript: '[00:01] Me: hi', transcript_heard: '', notes_md: notes });
        const ms = [mk('1', 'Daily Meeting', 'Ready', '### Action items\n- [ ] Me: Brush my teeth [01:00]\n')];
        let next = 0, job = null, finish = null; const callbacks = {}, listeners = {};
        window.calls = [];
        const emit = (event, payload = null) => (listeners[event] || []).forEach(id => callbacks[id]?.({ event, payload }));
        window.emit = emit;
        window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
        window.__TAURI_INTERNALS__ = { convertFileSrc: x => x, transformCallback: cb => { callbacks[++next] = cb; return next; }, unregisterCallback: () => {}, invoke: async (c, a) => {
          window.calls.push(c);
          if (c === 'plugin:event|listen') { (listeners[a.event] ||= []).push(a.handler); return a.handler; }
          if (c === 'get_state') return { state: { type: 'Idle' }, meeting_id: null, companion: { visible: false, size: 'Pill' }, elapsed_ms: 0, quit_pending: false };
          if (c === 'list_meetings') return ms.map(m => m.summary); if (c === 'deleted_meetings') return []; if (c === 'get_meeting') { const m = ms.find(m => m.summary.id === a.id); if (!m) throw 'gone'; return m; }
          if (c === 'open_action_items') return ms.filter(m => m.notes_md.includes('- [ ]')).map(m => [m.summary, m.notes_md]); if (c === 'library_bytes') return 1000;
          if (c === 'meeting_media') return { tracks: [], duration_ms: 87000, folder: 'x' };
          if (c === 'set_action_item_done') { const m = ms.find(m => m.summary.id === a.id); m.notes_md = m.notes_md.replace('- [ ]', '- [x]'); return m.notes_md; }
          if (c === 'importing') return job;
          // Rust makes the meeting folder, then says so; the import resolves only when it ends.
          if (c === 'import_audio') { job = { id: null, import: true }; setTimeout(() => { ms.unshift(mk('2', 'Quarterly review', 'Finishing', '')); job = { id: '2', import: true }; emit('motetto://library-changed'); }, 100); return new Promise(r => { finish = r; }); }
          if (c === 'cancel_import') { setTimeout(() => { ms.shift(); job = null; finish(null); emit('motetto://library-changed'); }, 300); return null; }
          return false; } };
      });
      await p.goto('http://127.0.0.1:3014/main/');
      await p.locator('.openitems .oi').first().waitFor();
      const home = await p.locator('.openitems .oi input').first().evaluate(e => [e.offsetWidth, e.offsetHeight]);
      await p.getByRole('button', { name: 'Import a recording…' }).first().click();
      // The import's own page opens with Cancel; the list shows it writing.
      const cancel = p.getByRole('button', { name: 'Cancel import' });
      await cancel.waitFor();
      if (!(await p.locator('.dochead h1', { hasText: 'Quarterly review' }).count())) fails.push('progress page lacks the meeting title');
      if (!(await p.locator('.meeting-row[aria-current="page"]', { hasText: 'Writing notes' }).count())) fails.push('import row not current or not marked Writing notes');
      await p.getByText('Preparing your file', { exact: true }).waitFor();
      const stage = value => p.evaluate(stage => window.emit('motetto://event', { type: 'Stage', meeting_id: '2', stage, percent: 0, eta_s: null }), value);
      await stage({ type: 'Transcribing', progress: { percent: 43, eta_s: 250, step: { type: 'Transcribing', side: null } } });
      await p.getByText('43% · about 4 min left', { exact: true }).waitFor();
      await p.locator('.step.now').getByText('Transcribing the audio', { exact: true }).waitFor();
      if (await p.getByText('Recorded both sides', { exact: true }).count()) fails.push('import says it recorded both sides');
      await stage({ type: 'Summarizing', part: 2, of: 4 });
      await p.locator('.step.now').getByText('Summarizing part 2 of 4', { exact: true }).waitFor();
      await p.locator('.step.done').getByText('Transcript ready', { exact: true }).waitFor();
      const ref = await context.newPage();
      await ref.route('https://**/*', route => route.abort());
      await ref.emulateMedia({ colorScheme: theme });
      await ref.setViewportSize({ width: 1400, height: 1000 });
      await ref.goto('http://127.0.0.1:3001/motetto-app-mockup.html');
      await ref.getByRole('button', { name: 'Library', exact: true }).click();
      await ref.getByRole('button', { name: /Promotion rollover review/ }).first().click();
      const props = ['fontSize', 'fontFamily', 'gap', 'padding', 'color', 'borderRadius'];
      for (const [actual, expected] of [['.writing', '.processing'], ['.step', '.step'], ['.writing h4', '.processing h4'], ['.procbar', '.processing .progress']]) {
        const read = (root, selector) => root.locator(selector).first().evaluate((e, keys) => Object.fromEntries(keys.map(k => [k, getComputedStyle(e)[k]])), props);
        const a = await read(p, actual), b = await read(ref, expected);
        for (const key of props) if (a[key] !== b[key]) fails.push(`${theme} ${actual} ${key}: ${a[key]} != ${b[key]}`);
      }
      await p.screenshot({ path: `.playwright-out/import-summarizing-${theme}.png` });
      await ref.locator('.processing').screenshot({ path: `.playwright-out/import-reference-${theme}.png` });
      await ref.close();
      await p.setViewportSize({ width: 760, height: 520 });
      await cancel.scrollIntoViewIfNeeded();
      if (await p.evaluate(() => document.documentElement.scrollWidth > innerWidth)) fails.push('import steps overflow the minimum width');
      await p.screenshot({ path: `.playwright-out/import-minimum-${theme}.png` });
      await p.setViewportSize({ width: 1200, height: 720 });
      await stage({ type: 'Merging' });
      await p.locator('.step.now').getByText('Summarizing: merging the parts', { exact: true }).waitFor();
      await stage({ type: 'Saving' });
      await p.locator('.step.now').getByText('Saving', { exact: true }).waitFor();
      await p.screenshot({ path: `.playwright-out/import-steps-${theme}.png` });
      // Other meetings stay usable meanwhile.
      await p.getByRole('button', { name: /Daily Meeting/ }).first().click();
      await p.locator('.todo input').first().waitFor();
      const notes = await p.locator('.todo input').first().evaluate(e => [e.offsetWidth, e.offsetHeight]);
      if (home.join() !== notes.join()) fails.push(`checkboxes differ: Home ${home} vs Notes ${notes}`);
      await p.locator('.homerow').click();
      const again = p.getByRole('button', { name: 'Import a recording…' });
      if (!(await again.isDisabled()) || !(await again.getAttribute('title'))?.startsWith('One import at a time')) fails.push('second Import not disabled with its reason');
      await p.locator('.openitems .oi input').first().click();
      await p.waitForTimeout(300);
      if (!(await p.evaluate(() => window.calls.includes('set_action_item_done')))) fails.push('ticking an item elsewhere did not save');
      // Back to the import, then Cancel.
      await p.getByRole('button', { name: /Quarterly review/ }).first().click();
      await cancel.click();
      if (!(await p.getByRole('button', { name: 'Stopping…' }).isDisabled())) fails.push('Stopping… is not shown, disabled');
      await p.getByText('Import cancelled. Your file is unchanged.').waitFor();
      await p.waitForTimeout(300);
      if (await p.locator('.writing').count()) fails.push('progress page still open');
      if (await p.getByRole('button', { name: /Quarterly review/ }).count()) fails.push('cancelled meeting still listed');
      if (await p.getByRole('button', { name: 'Import a recording…' }).isDisabled()) fails.push('Import still disabled after cancel');
      if (await p.locator('.banner[role=alert]').count()) fails.push('error: ' + await p.locator('.banner[role=alert]').innerText());
      await p.close();
    }
    if (fails.length) throw Error(JSON.stringify(fails));
    return 'ok';
  } finally { await browser.close(); }
}
