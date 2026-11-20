// Who said this? (SPEC 7.11): from a line, the speaker popover asks what to change. "This line"
// moves only that line to someone else on the call, or to a new voice with the typed name; "N in a
// row" moves the run around it; "All N" renames the voice. The notes stay; Undo moves it back.
// Styles are compared with the mockup's in the same state.
async (page) => {
  const fails = [];
  const PARTS = ['h4', '.help', '.segm', '.segm [aria-checked="true"]', '.segm [aria-checked="false"]', 'input[type=text]', '.input-clear > button', '.input-clear > button svg', '.field-check', '.field-check-desc', '.row2 .btn.primary'];
  const CSS = ['fontFamily', 'fontSize', 'fontWeight', 'lineHeight', 'color', 'backgroundColor', 'borderTopColor', 'borderTopWidth', 'borderRadius', 'boxShadow', 'paddingTop', 'paddingLeft', 'height', 'opacity', 'justifySelf'];
  const parts = p => p.locator('.pop.spk').evaluate((pop, [parts, css]) => Object.fromEntries(parts.map(sel => {
    const e = pop.querySelector(sel); if (!e) return [sel, null];
    const c = getComputedStyle(e); return [sel, Object.fromEntries(css.map(k => [k, c[k]]))];
  })), [PARTS, CSS]);
  for (const theme of ['light', 'dark']) {
    // The mockup: After the call, Find speakers, then a name on a line, This line.
    const ref = await page.context().newPage();
    await ref.emulateMedia({ colorScheme: theme });
    await ref.setViewportSize({ width: 1200, height: 720 });
    await ref.goto('http://127.0.0.1:3001/motetto-app-mockup.html');
    await ref.getByRole('button', { name: 'macOS', exact: true }).click();
    await ref.locator('[data-go="notes"]').click();
    await ref.locator('[data-dd="more"]').click({ timeout: 30000 }); await ref.locator('[data-speakers]').click(); await ref.locator('#spGo').click();
    await ref.locator('.who.voice').first().click();
    await ref.locator('[data-spkscope="line"]').click();
    await ref.locator('#spkName').fill('Kate');
    await ref.waitForTimeout(250);
    const refParts = await parts(ref);
    await ref.close();

    const p = await page.context().newPage(); p.on('pageerror', e => fails.push(e.message));
    await p.emulateMedia({ colorScheme: theme });
    await p.setViewportSize({ width: 1200, height: 720 });
    await p.addInitScript(() => {
      const summary = { id: '1', title: 'Standup', created_ms: Date.now(), status: 'Ready', marks_ms: [], duration_ms: 60000, notes_ms: 8000, spellings: [], locked: false, title_source: 'placeholder', speaker_names: { 2: 'Nathan' } };
      const heard = '[00:00] Me: Xong chưa?\n[00:02] Speaker 2: Bên em đủ rồi anh ơi.\n[00:04] Speaker 2: Ok đủ.\n[00:09] Speaker 3: Anh Met với chị Kate.\n[00:15] Speaker 2: Nói nhỏ lại tí.';
      const meeting = { summary, transcript: heard, transcript_heard: '', notes_md: '### Overview\nSpeaker 2 is ready.\n' };
      let next = 0; const callbacks = {}, listeners = {};
      window.calls = [];
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
      window.__TAURI_INTERNALS__ = { convertFileSrc: x => x, transformCallback: cb => { const id = ++next; callbacks[id] = cb; return id; }, unregisterCallback: id => { delete callbacks[id]; }, invoke: async (c, a) => {
        if (c === 'plugin:event|listen') { (listeners[a.event] ||= []).push(a.handler); return a.handler; }
        if (c === 'get_state') return { state: { type: 'Idle' }, meeting_id: null, companion: { visible: false, size: 'Pill' }, elapsed_ms: 0, quit_pending: false };
        if (c === 'list_meetings') return [summary]; if (c === 'deleted_meetings') return []; if (c === 'get_meeting') return meeting;
        if (c === 'set_speaker_names') { window.calls.push(['names', JSON.stringify(a.names)]); summary.speaker_names = Object.fromEntries(Object.entries(a.names).filter(([, v]) => v)); return { ...summary }; }
        if (c === 'move_transcript_lines') {
          window.calls.push(['move', JSON.stringify([a.indices, a.voice])]);
          const rows = meeting.transcript.split('\n');
          a.indices.forEach(i => { rows[i] = rows[i].replace(/^(\[[0-9:]+\] )Speaker \d+:/, `$1Speaker ${a.voice}:`); });
          meeting.transcript = rows.join('\n'); return meeting.transcript;
        }
        if (c === 'save_notes') { window.calls.push(['notes', a.markdown]); return a.markdown; }
        if (c === 'meeting_media') return { tracks: [{ path: '/a.wav', name: 'mic.wav', speaker: 'You', offset_ms: 0, duration_ms: 60000, bytes: 1, waveform: [0.3] }], duration_ms: 60000, folder: 'x' };
        return false; } };
    });
    await p.goto('http://127.0.0.1:3000/main/');
    await p.getByRole('button', { name: /Standup/ }).first().click();
    await p.getByRole('tab', { name: 'Transcript', exact: true }).click();
    const who = () => p.locator('.tline .who').allInnerTexts();
    await p.locator('.tline .who.voice').first().waitFor();
    // The second Nathan line (00:04): its run is 00:02-00:04, Nathan has 3 lines.
    await p.locator('.tline').nth(2).locator('.who.voice').click();
    const scopes = await p.locator('.pop.spk .segm [role=radio]').allInnerTexts();
    // It opens on This line, so a slip never renames every line.
    if (await p.locator('.pop.spk .segm [aria-checked="true"]').innerText() !== 'This line') fails.push(`${theme}: This line is not the default`);
    if (await p.locator('.pop.spk h4').innerText() !== 'Who said this?') fails.push(`${theme}: title ${await p.locator('.pop.spk h4').innerText()}`);
    // One line per row on every tab: the popover keeps its height (and place) whichever is chosen.
    const heights = [];
    // A press on a tab never takes focus from the name field, so one click always switches.
    await p.evaluate(() => { window.left = 0; document.querySelector('.pop.spk input[type=text]').addEventListener('blur', () => window.left++); });
    for (const name of ['This line', '2 in a row', 'All 3']) {
      await p.getByRole('radio', { name }).click(); await p.waitForTimeout(50);
      if (await p.getByRole('radio', { name }).getAttribute('aria-checked') !== 'true') fails.push(`${theme}: one click did not choose ${name}`);
      heights.push(await p.locator('.pop.spk').evaluate(e => e.getBoundingClientRect().height));
      const tall = await p.locator('.pop.spk').evaluate(pop => [pop.querySelector(':scope > .help'), pop.querySelector('h4'), pop.querySelector('.sample q'), pop.querySelector('.sugg'), pop.querySelector('.field-check-desc')].filter(e => e && e.getBoundingClientRect().height > 30).map(e => e.className || e.tagName));
      if (tall.length) fails.push(`${theme}: ${name} wraps ${tall}`);
    }
    const wide = await p.locator('.pop.spk').evaluate(pop => pop.scrollWidth > pop.clientWidth + 1 || [...pop.children].filter(c => !c.matches('.sugg')).some(c => c.getBoundingClientRect().right > pop.getBoundingClientRect().right - 11));
    if (wide) fails.push(`${theme}: a row runs past the popover`);
    // An input method that reorders a click (OpenKey in VI mode: release, then press) still
    // switches on the first click.
    await p.locator('.pop.spk [role=radio]', { hasText: 'in a row' }).evaluate(button => {
      const at = button.getBoundingClientRect(), o = { bubbles: true, cancelable: true, button: 0, clientX: at.x + 5, clientY: at.y + 5, pointerId: 1, isPrimary: true };
      button.dispatchEvent(new PointerEvent('pointerup', o)); button.dispatchEvent(new MouseEvent('mouseup', o));
      button.dispatchEvent(new PointerEvent('pointerdown', o)); button.dispatchEvent(new MouseEvent('mousedown', o));
    });
    if (await p.locator('.pop.spk [role=radio][aria-checked=true]').innerText() !== '2 in a row') fails.push(`${theme}: a reordered click (release first) did not switch`);
    // The next reordered click, on another tab, works too (the late press is not a drag).
    await p.locator('.pop.spk [role=radio]', { hasText: 'This line' }).evaluate(button => {
      const at = button.getBoundingClientRect(), o = { bubbles: true, cancelable: true, button: 0, clientX: at.x + 5, clientY: at.y + 5, pointerId: 1, isPrimary: true };
      button.dispatchEvent(new PointerEvent('pointerup', o)); button.dispatchEvent(new MouseEvent('mouseup', o));
      button.dispatchEvent(new PointerEvent('pointerdown', o)); button.dispatchEvent(new MouseEvent('mousedown', o));
    });
    if (await p.locator('.pop.spk [role=radio][aria-checked=true]').innerText() !== 'This line') fails.push(`${theme}: a second reordered click on another tab did not switch`);
    await p.getByRole('radio', { name: 'All 3' }).click();
    // A chip too; then Play and the checkbox each work on one click.
    const field = p.locator('.pop.spk input[type=text]');
    await p.locator('.pop.spk .sugg .chip').first().click();
    if (await field.inputValue() !== (await p.locator('.pop.spk .sugg .chip').first().innerText()).trim()) fails.push(`${theme}: one click on a chip did not fill the name`);
    if (await p.evaluate(() => window.left)) fails.push(`${theme}: a tab or chip press took focus from the name field`);
    const tick = p.locator('.pop.spk .field-check .checkbox'), was = await tick.isChecked();
    await p.locator('.pop.spk .field-check').click();
    if (await tick.isChecked() === was) fails.push(`${theme}: one click did not toggle Also in the notes`);
    await p.locator('.pop.spk .field-check').click();
    const play = p.locator('.pop.spk .sample .btn');
    await play.click(); await p.waitForTimeout(100);
    if (await play.getAttribute('aria-label') !== 'Pause') fails.push(`${theme}: one click on Play did not play`);
    await play.click(); await p.waitForTimeout(100);
    if (await play.getAttribute('aria-label') === 'Pause') fails.push(`${theme}: one click on Pause did not pause`);
    await field.fill('Nathan');
    if (new Set(heights.map(Math.round)).size !== 1) fails.push(`${theme}: popover height changes across tabs ${heights}`);
    if (JSON.stringify(scopes) !== JSON.stringify(['This line', '2 in a row', 'All 3'])) fails.push(`${theme}: scopes ${scopes}`);
    // This line: who said it, everyone else on the call offered, the notes left alone.
    await p.getByRole('radio', { name: 'This line' }).click();
    await p.waitForTimeout(250);
    if (await p.locator('.pop.spk h4').innerText() !== 'Who said this?') fails.push(`${theme}: line title ${await p.locator('.pop.spk h4').innerText()}`);
    if (!/Only this line moves; Nathan's other lines stay\./.test(await p.locator('.pop.spk .help').first().innerText())) fails.push(`${theme}: line help`);
    // The name typed stays when the choice changes; here the voice's own, so Move waits.
    if (await p.locator('.pop.spk input[type=text]').inputValue() !== 'Nathan' || !await p.evaluate(() => document.activeElement?.matches('.pop.spk input[type=text]'))) fails.push(`${theme}: name not kept and focused`);
    if (!await p.locator('.pop.spk .btn.primary').isDisabled()) fails.push(`${theme}: Move allowed to the same person`);
    // The clear button inside the field: shown with text, clears and keeps focus, hides when empty.
    const clear = p.locator('.pop.spk .input-clear > button');
    if (await clear.evaluate(e => getComputedStyle(e).opacity) !== '1') fails.push(`${theme}: clear button hidden with text`);
    await clear.click();
    if (await p.locator('.pop.spk input[type=text]').inputValue() !== '' || !await p.evaluate(() => document.activeElement?.matches('.pop.spk input[type=text]'))) fails.push(`${theme}: clear did not empty and focus the field`);
    await p.waitForTimeout(200);
    if (await clear.evaluate(e => getComputedStyle(e).opacity) !== '0') fails.push(`${theme}: clear button shown on an empty field`);
    const box = await p.locator('.pop.spk input[type=text]').boundingBox(), x = await clear.boundingBox();
    if (Math.round(box.x + box.width - (x.x + x.width)) !== 6 || Math.abs((x.y + x.height / 2) - (box.y + box.height / 2)) > 0.5) fails.push(`${theme}: clear button not inside the right edge ${JSON.stringify([box, x])}`);
    // Switching back and forth keeps what was typed.
    await p.keyboard.type('Ka'); await p.getByRole('radio', { name: 'All 3' }).click(); await p.getByRole('radio', { name: 'This line' }).click();
    if (await p.locator('.pop.spk input[type=text]').inputValue() !== 'Ka') fails.push(`${theme}: switching cleared the name`);
    await p.locator('.pop.spk input[type=text]').fill('Kate');
    if (!/Ok đủ/.test(await p.locator('.pop.spk .sample q').innerText())) fails.push(`${theme}: sample is not this line`);
    const chips = await p.locator('.pop.spk .sugg .chip').allInnerTexts();
    if (chips[0] !== 'Speaker 3' || !chips.includes('Met') || chips.includes('Nathan')) fails.push(`${theme}: chips ${chips}`);
    if (!await p.locator('.pop.spk .field-check .checkbox').isDisabled()) fails.push(`${theme}: notes option not disabled`);
    if (await p.locator('.pop.spk .btn.primary').innerText() !== 'Move' || await p.locator('.pop.spk .btn.primary').isDisabled()) fails.push(`${theme}: Move not ready for Kate`);
    await p.waitForTimeout(250);
    const got = await parts(p);
    for (const sel of PARTS) {
      if (!got[sel] || !refParts[sel]) { fails.push(`${theme}: part "${sel}" ${got[sel] ? 'not in the mockup' : 'missing'}`); continue; }
      for (const k of CSS) if (got[sel][k] !== refParts[sel][k]) fails.push(`${theme}: "${sel}" ${k} ${got[sel][k]} vs mockup ${refParts[sel][k]}`);
    }
    await p.locator('.pop.spk').screenshot({ path: `.playwright-out/speaker-line-pop-${theme}.png` });
    // A new name makes a new voice for this line only.
    await p.locator('.pop.spk input[type=text]').press('Enter');
    await p.getByText('This line is now Kate').waitFor();
    if (JSON.stringify(await who()) !== JSON.stringify(['You', 'Nathan', 'Kate', 'Speaker 3', 'Nathan'])) fails.push(`${theme}: after move ${await who()}`);
    const calls = await p.evaluate(() => window.calls);
    if (calls[0]?.[1] !== '{"2":"Nathan","4":"Kate"}' || calls[1]?.[1] !== '[[2],4]' || calls.some(c => c[0] === 'notes')) fails.push(`${theme}: calls ${JSON.stringify(calls)}`);
    // Undo moves it back and forgets the new voice.
    await p.getByRole('button', { name: 'Undo' }).click(); await p.waitForTimeout(300);
    if (JSON.stringify(await who()) !== JSON.stringify(['You', 'Nathan', 'Nathan', 'Speaker 3', 'Nathan'])) fails.push(`${theme}: Undo left ${await who()}`);
    if (JSON.stringify(await p.evaluate(() => window.calls.slice(2))) !== JSON.stringify([['move', '[[2],2]'], ['names', '{"2":"Nathan"}']])) fails.push(`${theme}: undo calls ${JSON.stringify(await p.evaluate(() => window.calls.slice(2)))}`);
    // 2 in a row to someone already on the call (a chip): both lines, no new voice.
    await p.locator('.tline').nth(1).locator('.who.voice').click();
    await p.getByRole('radio', { name: '2 in a row' }).click();
    if (await p.locator('.pop.spk h4').innerText() !== 'Who said these 2 lines?') fails.push(`${theme}: run title`);
    await p.locator('.pop.spk .sugg .chip', { hasText: 'Speaker 3' }).click(); await p.keyboard.press('Enter');
    await p.getByText('2 lines are now Speaker 3').waitFor();
    if (JSON.stringify(await who()) !== JSON.stringify(['You', 'Speaker 3', 'Speaker 3', 'Speaker 3', 'Nathan'])) fails.push(`${theme}: after run ${await who()}`);
    // From People there is no line: no choice, every line.
    await p.locator('.people .person').nth(1).click();
    if (await p.locator('.pop.spk .segm').count()) fails.push(`${theme}: People offered a line choice`);
    await p.keyboard.press('Escape');
    await p.screenshot({ path: `.playwright-out/speaker-lines-${theme}.png` });
    await p.close();
  }
  return fails.length ? fails : 'PASS';
}
