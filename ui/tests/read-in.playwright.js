// Read in (owner 2026-10-26): the Notes tab's language list translates the notes on this computer,
// shows progress with Stop, opens the translation with the notes' look, ticks tasks in notes.md,
// remembers the language for the next meeting, and says when the notes changed since.
async (page) => {
  const fails = [];
  const p = await page.context().newPage(); p.on('pageerror', e => fails.push(e.message));
  await p.setViewportSize({ width: 1200, height: 760 });
  await p.addInitScript(() => {
    try { localStorage.removeItem('motetto.readIn'); localStorage.removeItem('motetto.readIn.recent'); } catch {}
    const summary = (id, title) => ({ id, title, created_ms: Date.now(), status: 'Ready', marks_ms: [], duration_ms: 60000, notes_ms: 8000, spellings: [], locked: false, title_source: 'model', language: 'en,vi' });
    const notes = '### Action items\n- [ ] You: Send the deck to **Team X** [00:05]\n### Overview\n- Planned the launch.\n### Launch plan\n- Ship on **Friday** [00:09]\n\n<details><summary>Transcript</summary>\n\n[00:05] Me: send\n</details>\n';
    const meetings = { 1: { summary: summary('1', 'Launch sync'), transcript: '[00:05] Me: send\n[00:09] Them: Friday', transcript_heard: '', notes_md: notes }, 2: { summary: summary('2', 'Second call'), transcript: '[00:05] Me: hi', transcript_heard: '', notes_md: notes } };
    const malay = '### Item tindakan\n- [ ] You: Hantar dek kepada **Team X** [00:05]\n### Gambaran keseluruhan\n- Merancang pelancaran.\n### Pelan pelancaran\n- Hantar pada **Jumaat** [00:09]\n\n<details><summary>Transcript</summary>\n\n[00:05] Me: send\n</details>\n';
    const saved = { 1: {}, 2: {} };
    let next = 0; const callbacks = {}, listeners = {};
    window.emit = (event, payload) => (listeners[event] || []).forEach(id => callbacks[id]?.({ event, id, payload }));
    window.calls = []; window.meetings = meetings; window.saved = saved;
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = { convertFileSrc: x => x, transformCallback: cb => { const id = ++next; callbacks[id] = cb; return id; }, unregisterCallback: id => { delete callbacks[id]; }, invoke: async (c, a) => {
      if (c === 'plugin:event|listen') { (listeners[a.event] ||= []).push(a.handler); return a.handler; }
      if (c === 'get_state') return { state: { type: 'Idle' }, meeting_id: null, companion: { visible: false, size: 'Pill' }, elapsed_ms: 0, quit_pending: false };
      if (c === 'list_meetings') return [meetings[1].summary, meetings[2].summary]; if (c === 'deleted_meetings') return []; if (c === 'get_meeting') return meetings[a.id];
      if (c === 'meeting_media') return { tracks: [{ path: '/a.wav', name: 'mic.wav', speaker: 'You', offset_ms: 0, duration_ms: 60000, bytes: 1, waveform: [0.3] }], duration_ms: 60000, folder: 'x' };
      if (c === 'notes_translations') return Object.entries(saved[a.id]).map(([code, t]) => ({ code, markdown: '', stale: t.from !== meetings[a.id].notes_md.replace(/\[x\]/g, '[ ]') }));
      if (c === 'get_translation') { const t = saved[a.id][a.code]; return t ? { code: a.code, markdown: t.markdown, stale: t.from !== meetings[a.id].notes_md.replace(/\[x\]/g, '[ ]') } : null; }
      if (c === 'translating') return null;
      if (c === 'stop_translation') { window.calls.push('stop'); window.stopRun?.(); return null; }
      if (c === 'translate_notes') {
        window.calls.push(`translate:${a.id}:${a.code}`);
        const base = { id: a.id, code: a.code, done: false, error: null };
        window.emit('motetto://translate', { ...base, part: 0, of: 0 });
        const result = await new Promise(resolve => { window.finishRun = () => resolve('ok'); window.stopRun = () => resolve('stopped'); window.partRun = () => window.emit('motetto://translate', { ...base, part: 1, of: 2 }); });
        if (result === 'stopped') { window.emit('motetto://translate', { ...base, part: 1, of: 2, done: true, error: 'stopped' }); throw 'stopped'; }
        saved[a.id][a.code] = { markdown: malay, from: meetings[a.id].notes_md.replace(/\[x\]/g, '[ ]') };
        window.emit('motetto://translate', { ...base, part: 2, of: 2, done: true, error: null });
        return { code: a.code, markdown: malay, stale: false };
      }
      if (c === 'set_action_item_done') { window.calls.push(`tick:${a.line}:${a.done}`); const lines = meetings[a.id].notes_md.split('\n'); lines[a.line] = lines[a.line].replace(/\[[ x]\]/, a.done ? '[x]' : '[ ]'); meetings[a.id].notes_md = lines.join('\n'); return meetings[a.id].notes_md; }
      return false; } };
  });
  await p.goto('http://127.0.0.1:3000/main/');
  await p.getByRole('button', { name: /Launch sync/ }).first().click();
  const button = p.getByRole('button', { name: /Read in/ });
  await button.waitFor();
  if (!/Original/.test(await button.innerText())) fails.push('button: ' + await button.innerText());
  // Before any pick, an "en,vi" call's English notes offer its other language (owner 2026-10-28).
  if (!/Read these notes in Tiếng Việt\?/.test(await p.locator('.readin-bar').innerText().catch(() => ''))) fails.push('no offer of the call\'s other language before any pick');

  // The list: Original with the call's languages, then a search by English name, accents ignored.
  await button.click();
  const original = p.getByRole('option', { name: /Original/ });
  if (await original.getAttribute('aria-selected') !== 'true') fails.push('Original not current');
  if (!/Call in English, Vietnamese/.test(await original.innerText())) fails.push('original row: ' + await original.innerText());
  if (!await p.getByRole('option').filter({ hasText: 'Tiếng Việt' }).count()) fails.push('no native names');
  await p.getByRole('combobox', { name: 'Search languages' }).fill('malay');
  const rows = await p.getByRole('option').allInnerTexts();
  if (rows.length !== 1 || !/Melayu/.test(rows[0])) fails.push('search malay: ' + JSON.stringify(rows));
  await p.getByRole('combobox', { name: 'Search languages' }).fill('tieng viet');
  if (!/Tiếng Việt/.test((await p.getByRole('option').allInnerTexts()).join())) fails.push('accent-free search failed');
  await p.getByRole('combobox', { name: 'Search languages' }).fill('mela');
  await p.keyboard.press('Enter');

  // Progress, with the original still readable underneath.
  const bar = p.locator('.readin-bar.running');
  await bar.waitFor();
  if (!/Translating into Melayu · starting/.test(await bar.innerText())) fails.push('start: ' + await bar.innerText());
  await p.evaluate(() => window.partRun());
  await p.waitForFunction(() => /part 1 of 2/.test(document.querySelector('.readin-bar')?.textContent ?? ''));
  if (!await p.getByText('Send the deck to').count()) fails.push('original hidden while translating');
  if (!(await p.evaluate(() => window.calls)).includes('translate:1:ms')) fails.push('translate call: ' + JSON.stringify(await p.evaluate(() => window.calls)));
  await p.screenshot({ path: '.playwright-out/read-in-running.png' });
  await p.evaluate(() => window.finishRun());

  // The translation: the notes' look (label headings stay small, topics serif), a working time chip.
  await p.getByText('Hantar dek kepada').waitFor();
  const look = await p.evaluate(() => [...document.querySelectorAll('.notes h3, .notes h4')].map(h => `${h.tagName}:${h.textContent.trim()}`));
  if (JSON.stringify(look) !== JSON.stringify(['H3:Item tindakan', 'H3:Gambaran keseluruhan', 'H4:Pelan pelancaran'])) fails.push('headings: ' + JSON.stringify(look));
  if (!/Melayu/.test(await button.innerText())) fails.push('button after: ' + await button.innerText());
  if (!/Melayu\s*Translated on this computer/.test(await p.locator('.readin-bar').innerText())) fails.push('bar: ' + await p.locator('.readin-bar').innerText());
  if (!await p.locator('.notes button.src').count()) fails.push('time chips lost');
  if (await p.locator('.notes [data-editable="1"]').count()) fails.push('translation editable');
  await p.screenshot({ path: '.playwright-out/read-in-translated.png' });
  await p.emulateMedia({ colorScheme: 'dark' });
  await p.screenshot({ path: '.playwright-out/read-in-translated-dark.png' });
  await p.emulateMedia({ colorScheme: 'light' });

  // A tick in the translation goes to notes.md's own line, and the translation stays fresh.
  await p.getByRole('checkbox').first().check();
  await p.waitForFunction(() => window.calls.includes('tick:1:true'));
  if (!await p.getByRole('checkbox').first().isChecked()) fails.push('tick not shown');
  if (await p.locator('.readin-col .readin-bar.stale').count()) fails.push('a tick made it stale');

  // Copy › Action items copies what is shown.
  await p.context().grantPermissions(['clipboard-read', 'clipboard-write']);
  await p.getByRole('button', { name: 'Copy ▾' }).click();
  await p.getByRole('menuitem', { name: /Action items/ }).click();
  const copied = await p.evaluate(() => navigator.clipboard.readText());
  if (!/Hantar dek/.test(copied) || /Hantar pada/.test(copied)) fails.push('copied: ' + copied);

  // Show original and back; the list marks Melayu as saved.
  await p.getByRole('button', { name: 'Show original' }).click();
  await p.getByText('Send the deck to').waitFor();
  if (!/Showing the original/.test(await p.locator('.readin-bar').innerText())) fails.push('peek bar missing');
  await p.getByRole('button', { name: 'Show Melayu' }).click();
  await p.getByText('Hantar dek kepada').waitFor();
  await button.click();
  if (!/Saved/.test(await p.getByRole('option', { name: /Melayu/ }).innerText())) fails.push('saved tag missing');
  await p.screenshot({ path: '.playwright-out/read-in-list.png' });
  await p.keyboard.press('Escape');

  // The next meeting remembers the language: it offers one click, and Stop leaves it as it was.
  await p.getByRole('button', { name: /Second call/ }).first().click();
  const offer = p.locator('.readin-bar.offer');
  await offer.waitFor();
  if (!/Read these notes in Melayu\?/.test(await offer.innerText())) fails.push('offer: ' + await offer.innerText());
  await offer.getByRole('button', { name: 'Translate' }).click();
  await p.locator('.readin-bar.running').waitFor();
  await p.getByRole('button', { name: 'Stop' }).click();
  await p.waitForFunction(() => !document.querySelector('.readin-bar.running'));
  if (!await p.getByText('Stopped. The notes are unchanged.').count()) fails.push('no stopped toast');
  if (!await p.getByText('Send the deck to').count()) fails.push('original lost after stop');

  // Back on the first meeting it opens in Melayu; an edit to the notes makes it out of date.
  await p.evaluate(() => { window.meetings[1].notes_md = window.meetings[1].notes_md.replace('Planned the launch.', 'Planned the launch and the beta.'); });
  await p.getByRole('button', { name: /Launch sync/ }).first().click();
  await p.locator('.readin-col .readin-bar.stale').waitFor();
  if (!/The notes changed after this translation/.test(await p.locator('.readin-col .readin-bar.stale').innerText())) fails.push('stale alert text');
  if (!await p.getByRole('checkbox').first().isDisabled()) fails.push('ticks allowed on a stale translation');
  // Same shape and place as the offer (owner 2026-10-30), not a separate card.
  const at = await p.locator('.readin-col .readin-bar.stale').evaluate(e => { const c = getComputedStyle(e), r = e.getBoundingClientRect(), notes = document.querySelector('.readin-col .notes').getBoundingClientRect(); return [r.left - notes.left, c.borderRadius, c.paddingLeft, c.fontSize]; });
  if (JSON.stringify(at) !== JSON.stringify([0, '10px', '12px', '12.5px'])) fails.push('stale bar shape: ' + JSON.stringify(at));
  await p.screenshot({ path: '.playwright-out/read-in-stale.png' });
  await p.emulateMedia({ colorScheme: 'dark' });
  await p.screenshot({ path: '.playwright-out/read-in-stale-dark.png' });
  await p.emulateMedia({ colorScheme: 'light' });

  // Original in the list stops reading in Melayu.
  await button.click();
  await p.getByRole('option', { name: /Original/ }).click();
  if (await p.evaluate(() => localStorage.getItem('motetto.readIn')) !== null) fails.push('preference kept after Original');
  await p.close();
  return fails.length ? fails : 'PASS';
}
