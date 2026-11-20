// Spellings used: a meeting's own list is added with Enter (spelling, heard as), saved through
// set_meeting_spellings, removed with its ×; the global list reads only; a locked meeting shows no
// field. No focus ring on the field. Saving respells the transcript and says so. The panel and
// its global list fold; a clear button empties the field; the side column is resizable. A chip
// clicked is edited in the field; pairs for all meetings are added here too.
async (page) => {
  const fails = [];
  const open = async (locked) => {
    const p = await page.context().newPage(); p.on('pageerror', e => fails.push(e.message));
    await p.setViewportSize({ width: 1280, height: 800 });
    await p.addInitScript(locked => {
      const summary = { id: '1', title: 'Again', created_ms: Date.now(), status: 'Ready', marks_ms: [], duration_ms: 60000, notes_ms: 8000, spellings: [], locked, title_source: 'placeholder', meeting_spellings: locked ? [{ spelling: 'MSBO', heard: '' }] : [] };
      const meeting = { summary, transcript: '[00:04] Me: Hello\n[00:09] Them: Hi hô mét', transcript_heard: '[00:04] Me: Hello\n[00:09] Them: Hi hô mét', notes_md: '### Overview\n- Notes.\n' };
      let next = 0; const callbacks = {}, listeners = {};
      window.calls = []; window.globalSpellings = [{ spelling: 'FCM', heard: '' }, { spelling: 'check', heard: 'chết' }];
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
      window.__TAURI_INTERNALS__ = { convertFileSrc: x => x, transformCallback: cb => { const id = ++next; callbacks[id] = cb; return id; }, unregisterCallback: id => { delete callbacks[id]; }, invoke: async (c, a) => {
        if (c === 'plugin:event|listen') { (listeners[a.event] ||= []).push(a.handler); return a.handler; }
        if (c === 'get_state') return { state: { type: 'Idle' }, meeting_id: null, companion: { visible: false, size: 'Pill' }, elapsed_ms: 0, quit_pending: false };
        if (c === 'list_meetings') return [summary]; if (c === 'deleted_meetings') return []; if (c === 'get_meeting') return JSON.parse(JSON.stringify(meeting));
        if (c === 'importing') return null;
        if (c === 'get_settings') return { spellings: window.globalSpellings };
        if (c === 'set_global_spellings') { window.calls.push('global:' + JSON.stringify(a.spellings)); window.globalSpellings = a.spellings; return JSON.parse(JSON.stringify(meeting)); }
        if (c === 'set_meeting_spellings') {
          window.calls.push(JSON.stringify(a.spellings)); summary.meeting_spellings = a.spellings;
          // The backend respells the heard transcript with the current pairs, so a removed one goes back.
          meeting.transcript = meeting.transcript_heard;
          for (const row of a.spellings) for (const form of row.heard.split('; ').filter(Boolean)) meeting.transcript = meeting.transcript.split(form).join(row.spelling);
          return JSON.parse(JSON.stringify(meeting));
        }
        if (c === 'meeting_media') return { tracks: [{ path: '/a.wav', name: 'mic.wav', speaker: 'You', offset_ms: 0, duration_ms: 60000, bytes: 1, waveform: [0.3] }], duration_ms: 60000, folder: 'x' };
        return false; } };
    }, locked);
    await p.goto('http://127.0.0.1:3000/main/');
    // Folds and the side column's width are remembered per viewer: start from none.
    await p.evaluate(() => localStorage.clear()); await p.reload();
    await p.getByRole('button', { name: /Again/ }).first().click();
    await p.locator('.spell').waitFor();
    return p;
  };

  let p = await open(false);
  const panel = p.locator('.spell');
  // The global list starts folded to its count and opens on its toggle.
  if (await panel.locator('.spell-chip.global').count()) fails.push('global list open at first');
  await panel.getByRole('button', { name: /All meetings/ }).click();
  if (await panel.locator('.spell-chip.global').count() !== 2) fails.push('global list: ' + await panel.innerText());
  // The whole panel folds to its title and count.
  await panel.getByRole('button', { name: /Spellings used/ }).click();
  if (await panel.locator('input').count() || !/Spellings used\s*2/.test(await panel.innerText())) fails.push('folded panel: ' + await panel.innerText());
  await panel.getByRole('button', { name: /Spellings used/ }).click();
  // The clear button shows with text, clears both halves and gives the focus back.
  const meetingField = panel.locator('.spell-add[data-scope="meeting"]');
  const clear = meetingField.getByRole('button', { name: 'Clear' });
  if (await clear.evaluate(e => getComputedStyle(e).opacity) !== '0') fails.push('clear shown while empty');
  await meetingField.getByRole('textbox', { name: 'Heard as (optional, semicolons between forms)' }).fill('x');
  await panel.getByRole('textbox', { name: 'Spelling for this meeting' }).fill('y');
  await p.waitForTimeout(250);
  if (await clear.evaluate(e => getComputedStyle(e).opacity) !== '1') fails.push('clear hidden with text');
  await clear.click();
  if (await panel.getByRole('textbox', { name: 'Spelling for this meeting' }).inputValue() !== '' || await meetingField.getByRole('textbox', { name: 'Heard as (optional, semicolons between forms)' }).inputValue() !== '') fails.push('clear left text');
  // The side column is 220px at first, wider when dragged, and back with a double-click.
  const aside = p.locator('.meeting-aside'), handle = p.getByRole('separator', { name: 'Side column width' });
  const width = async () => Math.round((await aside.boundingBox()).width);
  if (await width() !== 220) fails.push('aside width ' + await width());
  const hb = await handle.boundingBox();
  await p.mouse.move(hb.x + hb.width / 2, hb.y + 80); await p.mouse.down(); await p.mouse.move(hb.x - 100, hb.y + 80, { steps: 5 }); await p.mouse.up();
  if (Math.abs(await width() - 327) > 3) fails.push('dragged width ' + await width());
  if (await p.evaluate(() => localStorage.getItem('motetto.asideW')) === null) fails.push('width not kept');
  await handle.dblclick();
  if (await width() !== 220) fails.push('double-click width ' + await width());
  await handle.focus(); await p.keyboard.press('ArrowLeft');
  if (await width() !== 236) fails.push('arrow width ' + await width());
  const handleRing = await handle.evaluate(e => [getComputedStyle(e).outlineStyle, getComputedStyle(e).boxShadow].join());
  if (handleRing !== 'none,none') fails.push('handle ring ' + handleRing);
  const add = meetingField.getByRole('button', { name: 'Add spelling' });
  if (!await add.isDisabled()) fails.push('Add enabled while empty');
  const name = panel.getByRole('textbox', { name: 'Spelling for this meeting' });
  await name.fill('Holmet');
  await meetingField.getByRole('textbox', { name: 'Heard as (optional, semicolons between forms)' }).fill('hô mét');
  await meetingField.getByRole('textbox', { name: 'Heard as (optional, semicolons between forms)' }).press('Enter');
  await panel.locator('.spell-chip:not(.global)').first().waitFor();
  if (await p.evaluate(() => window.calls[0]) !== JSON.stringify([{ spelling: 'Holmet', heard: 'hô mét' }])) fails.push('saved: ' + await p.evaluate(() => window.calls[0]));
  // The respelled line shows at once, with a notice.
  await p.getByText('Spelled right in 1 transcript line.').waitFor({ timeout: 2000 }).catch(() => fails.push('no respell notice'));
  if (await name.inputValue() !== '' || !await name.evaluate(e => e === document.activeElement)) fails.push('field not cleared and focused after adding');
  const ring = await name.evaluate(e => { const st = getComputedStyle(e), box = getComputedStyle(e.closest('.spell-add')); return [st.outlineStyle, st.boxShadow, box.boxShadow, box.borderColor]; });
  if (ring[0] !== 'none' || ring[1] !== 'none' || ring[2] !== 'none') fails.push('focus ring: ' + ring);
  // The chip reads `Holmet: “hô mét”`.
  if (await panel.locator('.spell-chip:not(.global)').first().innerText() !== 'Holmet:“hô mét”' && !/^Holmet:\s*“hô mét”/.test(await panel.locator('.spell-chip:not(.global)').first().innerText())) fails.push('chip: ' + await panel.locator('.spell-chip:not(.global)').first().innerText());
  // A repeat without heard forms is refused before saving; with them, they join the chip.
  await name.fill('holmet'); await name.press('Enter');
  if (await p.evaluate(() => window.calls.length) !== 1) fails.push('repeat saved');
  await meetingField.getByRole('textbox', { name: 'Heard as (optional, semicolons between forms)' }).fill('hô mẹt, hồ mét');
  await name.press('Enter');
  await p.waitForTimeout(100);
  if (await p.evaluate(() => window.calls[1]) !== JSON.stringify([{ spelling: 'Holmet', heard: 'hô mét; hô mẹt; hồ mét' }])) fails.push('merged: ' + await p.evaluate(() => window.calls[1]));
  await name.fill('');
  // Clicking a chip puts its pair in the field; Enter updates it; the button reads Update meanwhile.
  await panel.getByRole('button', { name: 'Edit Holmet' }).click();
  if (await name.inputValue() !== 'Holmet' || await meetingField.getByRole('textbox', { name: 'Heard as (optional, semicolons between forms)' }).inputValue() !== 'hô mét; hô mẹt; hồ mét') fails.push('edit did not load the pair');
  if (!await meetingField.getByRole('button', { name: 'Update spelling' }).count() || !await panel.locator('.spell-chip.editing').count()) fails.push('no edit state');
  await meetingField.getByRole('textbox', { name: 'Heard as (optional, semicolons between forms)' }).fill('hô mét;hồ mét');
  await name.press('Enter');
  await p.waitForTimeout(100);
  const edited = await p.evaluate(() => window.calls[window.calls.length - 1]);
  if (edited !== JSON.stringify([{ spelling: 'Holmet', heard: 'hô mét; hồ mét' }])) fails.push('updated: ' + edited);
  if (await panel.locator('.spell-chip.editing').count()) fails.push('still editing after update');
  // Escape leaves the field without saving.
  await panel.getByRole('button', { name: 'Edit Holmet' }).click(); await name.press('Escape');
  if (await name.inputValue() !== '' || await panel.locator('.spell-chip.editing').count()) fails.push('escape kept the edit');
  // A pair for all meetings is added here too: one call saves it in Settings and respells this meeting.
  const allField = panel.locator('.spell-add[data-scope="global"]');
  const before = await p.evaluate(() => window.calls.length);
  await allField.getByRole('textbox', { name: 'Spelling for all meetings' }).fill('claim');
  await allField.getByRole('textbox', { name: 'Heard as (optional, semicolons between forms)' }).fill('clam');
  await allField.getByRole('textbox', { name: 'Heard as (optional, semicolons between forms)' }).press('Enter');
  await p.waitForTimeout(150);
  const global = await p.evaluate(n => window.calls.slice(n), before);
  if (global[0] !== 'global:' + JSON.stringify([{ spelling: 'FCM', heard: '' }, { spelling: 'check', heard: 'chết' }, { spelling: 'claim', heard: 'clam' }]) || global.length !== 1) fails.push('global add: ' + JSON.stringify(global));
  if (!/claim:\s*“clam”/.test(await panel.locator('.spell-chip.global').last().innerText())) fails.push('global chip missing');
  // The × shows on hover and removes.
  const chip = panel.locator('.spell-chip:not(.global)').first();
  const x = chip.getByRole('button', { name: 'Remove Holmet' });
  await p.mouse.move(5, 5); await p.waitForTimeout(250);
  if (await x.evaluate(e => getComputedStyle(e).opacity) !== '0') fails.push('× visible before hover');
  await chip.hover();
  await p.waitForTimeout(250);
  if (await x.evaluate(e => getComputedStyle(e).opacity) !== '1') fails.push('× hidden on hover');
  await x.click();
  await p.waitForTimeout(100);
  if (await p.evaluate(() => window.calls[window.calls.length - 1]) !== '[]' || await panel.locator('.spell-chip:not(.global)').count()) fails.push('remove: ' + await p.evaluate(() => window.calls[window.calls.length - 1]));
  // No tip line about when they are used.
  if (/Used when you/.test(await panel.innerText())) fails.push('tip line shown');
  await p.getByRole('tab', { name: 'Transcript' }).click();
  // Holmet was removed: its respelled line is back to what was heard.
  await p.getByText('Hi hô mét').first().waitFor({ timeout: 2000 }).catch(() => fails.push('removed spelling not reverted on screen'));
  await p.close();

  p = await open(true);
  const lockedPanel = p.locator('.spell');
  if (await lockedPanel.locator('input').count() || await lockedPanel.locator('.spell-chip button').count() ) fails.push('locked meeting editable');
  if (!/MSBO/.test(await lockedPanel.innerText())) fails.push('locked list hidden');
  await p.close();
  return fails.length ? fails : 'ok';
}
