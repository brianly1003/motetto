// Speaker names (SPEC 7.11): after Find speakers a `Speaker N` reads like You (its colour, no dot) and
// opens "Who is Speaker N?"; a name follows every line of that voice, the notes' `Speaker N` too,
// Undo puts both back; two voices given one name show as one person; People lights a person's lines.
// Styles are compared with the mockup's after its Find speakers.
async (page) => {
  const fails = [];
  // Every part of the popover, as the mockup draws it: the same CSS, not only the same look.
  const PARTS = ['', 'h4', 'h4 i', '.help', 'input[type=text]', '.sample', '.sample .btn', '.sample q', '.sugg', '.sugg .chip', '.field-check', '.field-check-label', '.field-check-desc', '.row2', '.row2 .btn.ghost'];
  const CSS = ['fontFamily', 'fontSize', 'fontWeight', 'lineHeight', 'fontFeatureSettings', 'color', 'backgroundColor', 'borderTopColor', 'borderTopWidth', 'borderRadius', 'boxShadow', 'paddingTop', 'paddingLeft', 'rowGap', 'columnGap', 'display', 'alignItems'];
  const parts = page => page.locator('.pop.spk').evaluate((pop, [parts, css]) => Object.fromEntries(parts.map(sel => {
    const e = sel ? pop.querySelector(sel) : pop; if (!e) return [sel, null];
    const c = getComputedStyle(e); return [sel, Object.fromEntries(css.filter(k => !(sel === '.sugg .chip' && k === 'color')).map(k => [k, c[k]]))];
  })), [PARTS, CSS]);
  // A named voice's chip: its dot centred on the text, as far from it as the mockup's.
  const dot = page => page.locator('.pop.spk').evaluate(pop => {
    const chip = [...pop.querySelectorAll('.sugg .chip')].find(c => c.querySelector('i')); if (!chip) return null;
    const i = chip.querySelector('i').getBoundingClientRect(), b = chip.getBoundingClientRect();
    return { w: Math.round(i.width), h: Math.round(i.height), centre: Math.round((i.top + i.height / 2) - (b.top + b.height / 2)), left: Math.round(i.left - b.left) };
  });
  // From a line the popover starts on This line; these checks rename every line, so they pick All.
  const toAll = async pg => { const all = pg.locator('.pop.spk [role=radio]', { hasText: /^All / }); if (await all.count()) { await all.click(); await pg.waitForTimeout(200); } };
  const style = (loc, props) => loc.evaluate((e, props) => { const c = getComputedStyle(e); return Object.fromEntries(props.map(k => [k, c[k]])); }, props);
  for (const theme of ['light', 'dark']) {
    // The mockup: After the call (FCM vs Pusher Beams), ••• › Find speakers › Confirm. Captured in each theme.
    const ref = await page.context().newPage();
    await ref.emulateMedia({ colorScheme: theme });
    await ref.setViewportSize({ width: 1200, height: 720 });
    await ref.goto('http://127.0.0.1:3001/motetto-app-mockup.html');
    await ref.getByRole('button', { name: 'macOS', exact: true }).click();
    await ref.locator('[data-go="notes"]').click();
    // The notes are written first (the processing steps); the ••• menu shows once they are ready.
    await ref.locator('[data-dd="more"]').click({ timeout: 30000 }); await ref.locator('[data-speakers]').click(); await ref.locator('#spGo').click();
    await ref.locator('.who.voice').first().waitFor();
    const PILL = ['fontFamily', 'fontSize', 'fontWeight', 'lineHeight', 'paddingLeft', 'paddingRight', 'borderRadius', 'height', 'left'];
    const refPill = await style(ref.locator('.who.voice').first(), PILL);
    const refDots = await ref.locator('.tline .who.voice i').count();
    await ref.locator('.who.voice').first().click(); await toAll(ref);
    const POP = ['width', 'borderRadius', 'paddingTop', 'rowGap', 'boxShadow'];
    const refPop = await style(ref.locator('.pop.spk'), POP);
    const ICON = ['fill', 'stroke'];
    const BOX = ['width', 'height', 'borderRadius', 'borderTopWidth', 'borderTopColor', 'backgroundColor', 'boxShadow', 'appearance'];
    const refBox = await style(ref.locator('.pop.spk .checkbox'), BOX);
    const refCheck = await ref.locator('.pop.spk .checkbox').evaluate(e => { const b = getComputedStyle(e, '::before'); return { w: b.width, bg: b.backgroundColor, op: b.opacity }; });
    const refField = await style(ref.locator('.pop.spk .field-check-label'), ['fontSize', 'fontWeight', 'color']);
    const refIcon = await style(ref.locator('.pop.spk .sample svg'), ICON);
    const refParts = await parts(ref);
    await ref.locator('#spkName').fill('Kevin'); await ref.locator('#spkGo').click();
    await ref.locator('.who.voice', { hasText: /^Speaker \d+$/ }).first().click();
    const refDot = await dot(ref);
    const refPerson = await style(ref.locator('.people .person').nth(1), ['fontSize', 'fontWeight', 'gridTemplateColumns', 'paddingTop', 'borderRadius']);
    await ref.close();

    const p = await page.context().newPage(); p.on('pageerror', e => fails.push(e.message));
    await p.emulateMedia({ colorScheme: theme });
    await p.setViewportSize({ width: 1200, height: 720 });
    await p.addInitScript(() => {
      const summary = { id: '1', title: 'Rollover', created_ms: Date.now(), status: 'Ready', marks_ms: [], duration_ms: 60000, notes_ms: 8000, spellings: [], locked: false, title_source: 'placeholder', speaker_names: {} };
      const transcript = '[00:01] Me: So what happens next?\n[00:04] Speaker 2: Then it bets the whole 110 on this one.\n[00:09] Speaker 3: Wait, so it bets all of it? Ask Linh.\n[00:14] Speaker 2: Right, and that covers the rollover.';
      const meeting = { summary, transcript, transcript_heard: '', notes_md: '### Action items\n- [ ] Speaker 2: Check the rollover rule [00:14]\n\n### Overview\nSpeaker 2 explained the bet; Speaker 21 was not there.\n' };
      let next = 0; const callbacks = {}, listeners = {};
      window.calls = [];
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
      window.__TAURI_INTERNALS__ = { convertFileSrc: x => x, transformCallback: cb => { const id = ++next; callbacks[id] = cb; return id; }, unregisterCallback: id => { delete callbacks[id]; }, invoke: async (c, a) => {
        if (c === 'plugin:event|listen') { (listeners[a.event] ||= []).push(a.handler); return a.handler; }
        if (c === 'get_state') return { state: { type: 'Idle' }, meeting_id: null, companion: { visible: false, size: 'Pill' }, elapsed_ms: 0, quit_pending: false };
        if (c === 'list_meetings') return [summary]; if (c === 'deleted_meetings') return []; if (c === 'get_meeting') return meeting;
        if (c === 'set_speaker_names') { window.calls.push(['names', JSON.stringify(a.names)]); summary.speaker_names = Object.fromEntries(Object.entries(a.names).filter(([, v]) => v)); return { ...summary }; }
        if (c === 'save_notes') { if (a.expected !== meeting.notes_md) throw 'stale'; window.calls.push(['notes', a.markdown]); meeting.notes_md = a.markdown; return a.markdown; }
        if (c === 'set_locked') { summary.locked = a.locked; return { ...summary }; }
        if (c === 'meeting_media') return { tracks: [{ path: '/a.wav', name: 'mic.wav', speaker: 'You', offset_ms: 0, duration_ms: 60000, bytes: 1, waveform: [0.3] }], duration_ms: 60000, folder: 'x' };
        return false; } };
    });
    await p.goto('http://127.0.0.1:3000/main/');
    await p.getByRole('button', { name: /Rollover/ }).first().click();
    await p.getByRole('tab', { name: 'Transcript', exact: true }).click();
    const pills = p.locator('.tline .who.voice');
    await pills.first().waitFor();
    // Reads like You: no dot, and the mockup's pill.
    if (await p.locator('.tline .who.voice i').count() !== refDots || refDots !== 0) fails.push(`${theme}: a dot on the transcript name`);
    const pill = await style(pills.first(), PILL);
    for (const k of PILL) if (pill[k] !== refPill[k]) fails.push(`${theme}: pill ${k} ${pill[k]} vs mockup ${refPill[k]}`);
    // The words start at the same place on every row, and nothing wraps.
    const starts = await p.locator('.tline .tx').evaluateAll(rows => rows.map(r => Math.round(r.getBoundingClientRect().left)));
    if (new Set(starts).size !== 1) fails.push(`${theme}: words not aligned ${starts}`);
    const heights = await p.locator('.tline .who').evaluateAll(rows => rows.map(r => Math.round(r.getBoundingClientRect().height)));
    if (heights.some(h => h > 22)) fails.push(`${theme}: a name wrapped ${heights}`);
    // People: You, Speaker 2, Speaker 3; hovering one lights only their lines.
    const people = () => p.locator('.people .person b').allInnerTexts();
    if (JSON.stringify(await people()) !== JSON.stringify(['You', 'Speaker 2 unnamed', 'Speaker 3 unnamed'])) fails.push(`${theme}: people ${await people()}`);
    const person = await style(p.locator('.people .person').nth(1), ['fontSize', 'fontWeight', 'gridTemplateColumns', 'paddingTop', 'borderRadius']);
    for (const k of Object.keys(refPerson)) if (k !== 'gridTemplateColumns' && person[k] !== refPerson[k]) fails.push(`${theme}: person ${k} ${person[k]} vs mockup ${refPerson[k]}`);
    await p.locator('.people .person').nth(2).hover();
    const lit = await p.locator('.tlist.spot .tline.lit').count();
    if (lit !== 1) fails.push(`${theme}: hovering Speaker 3 lit ${lit} lines`);
    await p.mouse.move(0, 0);
    // Who is Speaker 2? Name field focused, the sample, a name said in the call, the notes option.
    await pills.first().click();
    // A line's popover starts on This line, so a slip never renames every line.
    if (await p.locator('.pop.spk [role=radio][aria-checked=true]').innerText() !== 'This line') fails.push(`${theme}: the popover did not start on This line`);
    await toAll(p);
    const pop = p.locator('.pop.spk');
    await pop.waitFor();
    const popStyle = await style(pop, POP);
    for (const k of POP) if (popStyle[k] !== refPop[k]) fails.push(`${theme}: popover ${k} ${popStyle[k]} vs mockup ${refPop[k]}`);
    const got = await parts(p);
    for (const sel of PARTS) {
      if (!got[sel] || !refParts[sel]) { fails.push(`${theme}: popover part "${sel}" ${got[sel] ? 'not in the mockup' : 'missing'}`); continue; }
      for (const k of Object.keys(refParts[sel])) if (got[sel][k] !== refParts[sel][k]) fails.push(`${theme}: popover "${sel}" ${k} ${got[sel][k]} vs mockup ${refParts[sel][k]}`);
    }
    const icon = await style(p.locator('.pop.spk .sample svg'), ICON);
    for (const k of ICON) if (icon[k] !== refIcon[k]) fails.push(`${theme}: play icon ${k} ${icon[k]} vs mockup ${refIcon[k]}`);
    const text = await pop.innerText();
    for (const want of ['Who is Speaker 2?', 'All 2 of its lines take the name.', 'Linh', 'Also in the notes', 'Speaker 2 is in the notes 2 times', 'Then it bets the whole 110']) if (!text.includes(want)) fails.push(`${theme}: popover lacks "${want}"`);
    // The checkbox atom, as the mockup draws it: same box, fill, check and label in both themes; no ring.
    const box = await style(p.locator('.pop.spk .checkbox'), BOX);
    for (const k of BOX) if (box[k] !== refBox[k]) fails.push(`${theme}: checkbox ${k} ${box[k]} vs mockup ${refBox[k]}`);
    const check = await p.locator('.pop.spk .checkbox').evaluate(e => { const b = getComputedStyle(e, '::before'); return { w: b.width, bg: b.backgroundColor, op: b.opacity }; });
    if (JSON.stringify(check) !== JSON.stringify(refCheck)) fails.push(`${theme}: check mark ${JSON.stringify(check)} vs mockup ${JSON.stringify(refCheck)}`);
    const field = await style(p.locator('.pop.spk .field-check-label'), ['fontSize', 'fontWeight', 'color']);
    for (const k of Object.keys(refField)) if (field[k] !== refField[k]) fails.push(`${theme}: checkbox label ${k} ${field[k]} vs mockup ${refField[k]}`);
    if (!await p.evaluate(() => document.activeElement?.matches('.pop.spk input[type=text]'))) fails.push(`${theme}: name field not focused`);
    if (await p.locator('.pop.spk input[type=text]').evaluate(e => getComputedStyle(e).outlineStyle) !== 'none') fails.push(`${theme}: focus ring on the name field`);
    if (await pills.first().getAttribute('aria-expanded') !== 'true') fails.push(`${theme}: name not marked expanded`);
    await p.keyboard.type('Kevin'); await p.keyboard.press('Enter');
    await p.getByText('Speaker 2 is now Kevin · 2 lines').waitFor();
    const names = await p.locator('.tline .who').allInnerTexts();
    if (JSON.stringify(names) !== JSON.stringify(['You', 'Kevin', 'Speaker 3', 'Kevin'])) fails.push(`${theme}: after rename ${names}`);
    const calls = await p.evaluate(() => window.calls);
    if (calls[0]?.[1] !== '{"2":"Kevin"}') fails.push(`${theme}: names sent ${JSON.stringify(calls[0])}`);
    if (!calls[1]?.[1].includes('- [ ] Kevin: Check') || !calls[1][1].includes('Kevin explained') || !calls[1][1].includes('Speaker 21 was')) fails.push(`${theme}: notes ${JSON.stringify(calls[1])}`);
    if (await p.locator('.pop.spk').count()) fails.push(`${theme}: popover still open`);
    // Undo: names and notes go back.
    await p.getByRole('button', { name: 'Undo' }).click(); await p.waitForTimeout(300);
    if (JSON.stringify(await p.locator('.tline .who').allInnerTexts()) !== JSON.stringify(['You', 'Speaker 2', 'Speaker 3', 'Speaker 2'])) fails.push(`${theme}: Undo left ${await p.locator('.tline .who').allInnerTexts()}`);
    if (!(await p.evaluate(() => window.calls.at(-1)?.[1] ?? '')).includes('- [ ] Speaker 2: Check')) fails.push(`${theme}: Undo did not put the notes back`);
    // Merge: Speaker 2 is Kevin, then Speaker 3 named Kevin shows as one person.
    // Unticked, only the transcript takes the name; the notes keep `Speaker 2`.
    await pills.first().click(); await toAll(p);
    const also = p.locator('.pop.spk .field-check .checkbox');
    if (!await also.isChecked()) fails.push(`${theme}: Also in the notes not ticked by default`);
    await also.uncheck(); await p.locator('.pop.spk input[type=text]').fill('Kevin'); await p.locator('.pop.spk input[type=text]').press('Enter');
    await p.getByText('Speaker 2 is now Kevin', { exact: false }).first().waitFor();
    const lastCall = await p.evaluate(() => window.calls.at(-1));
    if (lastCall?.[0] !== 'names' || !(await p.evaluate(() => window.calls.filter(c => c[0] === 'notes').length === 2))) fails.push(`${theme}: unticked still changed the notes ${JSON.stringify(lastCall)}`);
    await p.locator('.tline .who.voice').nth(1).click();
    const named = await dot(p);
    if (JSON.stringify(named) !== JSON.stringify(refDot)) fails.push(`${theme}: named chip dot ${JSON.stringify(named)} vs mockup ${JSON.stringify(refDot)}`);
    // Offered for a voice the notes never name too, without a count.
    const also3 = await p.locator('.pop.spk .field-check').innerText().catch(() => '');
    if (!/^Also in the notes\s+The notes don't mention Speaker 3\.$/.test(also3.trim())) fails.push(`${theme}: Speaker 3 notes option "${also3}"`);
    // Space toggles the focused checkbox (a checkbox, not the player); no focus ring on it.
    const box3 = p.locator('.pop.spk .field-check .checkbox');
    await box3.focus(); await p.keyboard.press(' ');
    if (await box3.isChecked()) fails.push(`${theme}: Space did not untick the checkbox`);
    if (await box3.evaluate(e => getComputedStyle(e).outlineStyle) !== 'none') fails.push(`${theme}: focus ring on the checkbox`);
    await p.keyboard.press(' '); await p.locator('.pop.spk input[type=text]').focus();
    await p.keyboard.type('Kevin');
    const merge = await p.locator('.pop.spk .merge').innerText().catch(() => '');
    if (!/Speaker 2 is already Kevin/.test(merge)) fails.push(`${theme}: merge warning "${merge}"`);
    if (await p.locator('.pop.spk .btn.primary').innerText() !== 'Merge') fails.push(`${theme}: no Merge button`);
    await p.keyboard.press('Enter'); await p.waitForTimeout(300);
    if (JSON.stringify(await people()) !== JSON.stringify(['You', 'Kevin + Speaker 3'])) fails.push(`${theme}: merged people ${await people()}`);
    // Esc closes without saving.
    await p.locator('.tline .who.voice').first().click(); await p.keyboard.type('Zed'); await p.keyboard.press('Escape');
    if (await p.locator('.pop.spk').count() || (await p.locator('.tline .who').allInnerTexts()).includes('Zed')) fails.push(`${theme}: Esc did not cancel`);
    // After Esc the line has focus, not the name: Space plays (and pauses), it never reopens the popover.
    // Focus returns on the next frame (not in the commit that closes the popover).
    await p.waitForFunction(() => document.activeElement?.matches('.tline'), null, { timeout: 1000 }).catch(() => {});
    if (!await p.evaluate(() => document.activeElement?.matches('.tline'))) fails.push(`${theme}: focus after Esc on ${await p.evaluate(() => document.activeElement?.className)}`);
    await p.keyboard.press(' '); await p.waitForTimeout(200);
    if (await p.locator('.pop.spk').count()) fails.push(`${theme}: Space after Esc reopened the popover`);
    if (!/Pause/.test(await p.locator('.player .doc-action').innerText())) fails.push(`${theme}: Space after Esc did not play`);
    await p.keyboard.press(' ');
    if (!/Play/.test(await p.locator('.player .doc-action').innerText())) fails.push(`${theme}: Space did not pause`);
    // Even with focus on a (renamed) name, or a person in People, Space plays and pauses, again and again.
    for (const target of [p.locator('.tline .who.voice').first(), p.locator('.people .person').nth(1)]) {
      await target.focus();
      for (const want of ['Pause', 'Play', 'Pause', 'Play']) {
        await p.keyboard.press(' '); await p.waitForTimeout(150);
        if (await p.locator('.pop.spk').count()) { fails.push(`${theme}: Space on ${await target.innerText()} opened the popover`); await p.keyboard.press('Escape'); break; }
        if (!new RegExp(want).test(await p.locator('.player .doc-action').innerText())) { fails.push(`${theme}: Space on ${await target.innerText()} gave ${await p.locator('.player .doc-action').innerText()}, wanted ${want}`); break; }
      }
    }
    // The popover's Play plays this voice; then it is the player's toggle: Space on it pauses, and plays again.
    await p.locator('.tline .who.voice').first().click();
    const sample = p.locator('.pop.spk .sample .btn');
    await sample.click(); await p.waitForTimeout(200);
    if (!/Pause/.test(await p.locator('.player .doc-action').innerText()) || await sample.getAttribute('aria-label') !== 'Pause') fails.push(`${theme}: Play this voice did not play (${await sample.getAttribute('aria-label')})`);
    for (const want of ['Play', 'Pause']) {
      await p.keyboard.press(' '); await p.waitForTimeout(200);
      if (!await p.locator('.pop.spk').count()) { fails.push(`${theme}: Space on the sample closed the popover`); break; }
      if (!new RegExp(want).test(await p.locator('.player .doc-action').innerText())) fails.push(`${theme}: Space on the sample gave ${await p.locator('.player .doc-action').innerText()}, wanted ${want}`);
    }
    // In the name field Space types a space (names like "Phương Anh").
    await p.locator('.pop.spk input[type=text]').fill('Phương'); await p.locator('.pop.spk input[type=text]').press(' ');
    if (await p.locator('.pop.spk input[type=text]').inputValue() !== 'Phương ') fails.push(`${theme}: Space did not type in the name field`);
    await p.keyboard.press('Escape');
    if (/Pause/.test(await p.locator('.player .doc-action').innerText())) await p.keyboard.press(' ');
    // Enter on a focused name still opens it.
    await p.locator('.tline .who.voice').first().focus(); await p.keyboard.press('Enter');
    if (!await p.locator('.pop.spk').count()) fails.push(`${theme}: Enter on a name did not open it`);
    await p.keyboard.press('Escape');
    await p.screenshot({ path: `.playwright-out/speaker-names-${theme}.png` });
    await p.close();
  }
  return fails.length ? fails : 'PASS';
}
