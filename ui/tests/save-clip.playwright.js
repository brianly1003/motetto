// Save a clip (SPEC 15, DESIGN 4.20): ••• opens the panel in place of the timeline, the floating
// player hides, the transcript fades outside the range and a time takes its line, the track switch
// changes the words and adds the consent line, the chips come from the preview, Save sends the
// words and closes with a toast, Esc and Cancel close without saving, an import clips its one track.
async (page) => {
  const fails = [];
  const open = async ({ imported = false, cancel = false, theme = 'light' } = {}) => {
    const p = await page.context().newPage(); p.on('pageerror', e => fails.push(e.message));
    await p.setViewportSize({ width: 1280, height: 860 });
    // Appearance follows the system by default, as on a Mac in Light or Dark mode.
    await p.emulateMedia({ colorScheme: theme });
    await p.addInitScript(({ imported, cancel, theme }) => {
      const summary = { id: '1', title: 'FCM vs Pusher Beams', created_ms: Date.now(), status: 'Ready', marks_ms: [], duration_ms: 87000, notes_ms: 8000, spellings: [], locked: false, title_source: 'user', apps: ['Zoom'] };
      const meeting = { summary, transcript: "[00:07] Me: As you already know, FCM, Firebase Cloud Messaging, is Google's push delivery service.\n[00:16] Me: After studying push beams, I see it also relies on FCM.\n[00:28] Them: So Beams adds device management and reporting on top of FCM?\n[00:36] Me: Yes, but it is hard to fit into our back office.", transcript_heard: '', notes_md: '### Action items\n- None' };
      const wave = (k) => Array.from({ length: 200 }, (_, i) => 0.2 + 0.6 * Math.abs(Math.sin(i * 0.3 + k)));
      const tracks = imported
        ? [{ path: '/source.16k.wav', name: 'source.16k.wav', speaker: 'Audio', offset_ms: 0, duration_ms: 87000, bytes: 1, waveform: wave(0) }]
        : [{ path: '/m.wav', name: 'part-0000.mic.aec.wav', speaker: 'You', offset_ms: 0, duration_ms: 87000, bytes: 1, waveform: wave(0) },
           { path: '/s.wav', name: 'part-0000.system.wav', speaker: 'Them', offset_ms: 0, duration_ms: 87000, bytes: 1, waveform: wave(1) }];
      let next = 0; const callbacks = {}, listeners = {};
      window.calls = [];
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
      window.__TAURI_INTERNALS__ = { convertFileSrc: x => x, transformCallback: cb => { const id = ++next; callbacks[id] = cb; return id; }, unregisterCallback: id => { delete callbacks[id]; }, invoke: async (c, a) => {
        if (c === 'plugin:event|listen') { (listeners[a.event] ||= []).push(a.handler); return a.handler; }
        if (c === 'get_state') return { state: { type: 'Idle' }, meeting_id: null, companion: { visible: false, size: 'Pill' }, elapsed_ms: 0, quit_pending: false };
        if (c === 'list_meetings') return [summary]; if (c === 'deleted_meetings') return []; if (c === 'get_meeting') return meeting;
        if (c === 'meeting_media') return { tracks, videos: [], moments: [], clicks: [], duration_ms: 87000, folder: 'x' };
        if (c.startsWith('clip_') || c === 'save_clip' || c === 'reveal_clip') window.calls.push({ c, a });
        if (c === 'clip_levels') return Array.from({ length: a.bins }, (_, i) => (i > 60 && i < 66) ? 0.01 : 0.5 + 0.4 * Math.abs(Math.sin(i)));
        if (c === 'clip_preview') return { path: '/preview.wav', rate: 48000, bits: 32, float: true, peak_db: -5.84, clipped: 0, duration_ms: a.toMs - a.fromMs };
        if (c === 'save_clip') return cancel ? null : `/Users/me/Desktop/FCM vs Pusher Beams ${String(Math.floor(a.fromMs / 60000)).padStart(2, '0')}-${String(Math.floor(a.fromMs / 1000) % 60).padStart(2, '0')}.wav`;
        return false; } };
      if (imported) meeting.transcript = meeting.transcript.replace(/\] (Me|Them): /g, '] ');
    }, { imported, cancel, theme });
    await p.goto('http://127.0.0.1:3000/main/');
    await p.getByRole('button', { name: /FCM vs Pusher Beams/ }).first().click();
    await p.waitForSelector('.tl');
    await p.getByRole('button', { name: 'More actions' }).click();
    return p;
  };

  // An import clips its one track: no track switch, no line about other people's voices.
  let p = await open({ imported: true });
  const item = p.getByRole('menuitem', { name: 'Save a clip…' });
  if (await item.isDisabled()) fails.push('Save a clip disabled on an import');
  await item.click();
  await p.waitForSelector('.clipbar');
  if (await p.locator('.clipbar [role=radiogroup]').count()) fails.push('track switch on an import');
  if (await p.locator('.clipbar .cnote').count()) fails.push('other voices line on an import');
  if (await p.locator('.cwords .who').textContent() !== 'Audio') fails.push('import words not labelled Audio');
  if (!(await p.locator('.cwords q').textContent()).startsWith('As you already know')) fails.push('import opens on its first line');
  await p.waitForFunction(() => window.calls.some(c => c.c === 'clip_preview'));
  if (!(await p.evaluate(() => window.calls.filter(c => 'track' in c.a).every(c => c.a.track === 'Them')))) fails.push('import clips ask for another track');
  await p.close();

  p = await open();
  await p.getByRole('menuitem', { name: 'Save a clip…' }).click();
  await p.waitForSelector('.clipbar');
  if (await p.locator('.tl').count()) fails.push('the timeline still shows under the panel');
  if (await p.locator('.player').count()) fails.push('the floating player shows while clipping');
  if (!await p.locator('.tlist[data-clip]').count()) fails.push('transcript not in clip mode');
  // Opens on the first line of You: 00:07 - 00:16, its words, and one line in the clip.
  if (!(await p.locator('.clipbar .trow .mono').textContent()).includes('00:07.0 - 00:16.0')) fails.push('default range ' + await p.locator('.clipbar .trow .mono').textContent());
  if (!(await p.locator('.cwords q').textContent()).startsWith('As you already know')) fails.push('words of the first line');
  if (await p.locator('.tline.inclip').count() !== 1) fails.push('in-clip lines ' + await p.locator('.tline.inclip').count());
  const faded = await p.locator('.tline:not(.inclip)').first().evaluate(e => getComputedStyle(e).opacity);
  if (faded !== '0.42') fails.push('outside lines not faded: ' + faded);
  const rule = await p.locator('.tline.inclip').evaluate(e => [getComputedStyle(e).borderLeftWidth, getComputedStyle(e).boxShadow]);
  if (rule[0] !== '0px' || rule[1] !== 'none') fails.push('a left rule on clip lines ' + rule);
  await p.waitForFunction(() => window.calls.some(x => x.c === 'clip_preview'));
  await p.waitForSelector('.cchips .chip >> text=48 kHz · 32-bit float');
  for (const chip of ['No clipping', 'Peak −5.8 dB', '5-10 s · voice sample']) if (!await p.locator('.cchips .chip', { hasText: chip }).count()) fails.push('chip ' + chip);
  if (await p.locator('.cnote').count()) fails.push('consent line on You');
  // A Them line's time takes that line and switches the track.
  await p.locator('.tline[data-line="2"] .ts').click();
  if (!(await p.locator('.clipbar .trow .mono').textContent()).includes('00:28.0 - 00:36.0')) fails.push('line pick range');
  if (await p.locator('.clipbar').getAttribute('data-track') !== 'Them') fails.push('line pick track');
  if (!await p.locator('.cnote').count()) fails.push('no consent line on Them');
  // Keys: Shift+→ on the end handle adds a second.
  await p.locator('[data-ch="b"]').focus(); await p.keyboard.press('Shift+ArrowRight');
  if (!(await p.locator('.clipbar .trow .mono').textContent()).includes('00:37.0')) fails.push('Shift+→ ' + await p.locator('.clipbar .trow .mono').textContent());
  // Arrows move the chosen edge with nothing focused (after Play, Loop or a transcript time), and
  // the hint says which edge; the end first, the start once its handle was used.
  const range = async () => (await p.locator('.clipbar .trow .mono').textContent()).match(/(\d+):(\d+\.\d) - (\d+):(\d+\.\d)/).slice(1).map(Number);
  const secs = ([am, as, bm, bs]) => [am * 60 + as, bm * 60 + bs];
  await p.evaluate(() => document.activeElement?.blur());
  if (!(await p.locator('.clipbar .tkeys').last().textContent()).includes('move the end')) fails.push('hint does not name the end');
  await p.keyboard.press('ArrowRight');
  if (secs(await range())[1] !== 37.1) fails.push('→ with nothing focused: ' + await range());
  await p.locator('[data-ch="a"]').focus(); await p.evaluate(() => document.activeElement?.blur());
  await p.keyboard.press('ArrowLeft');
  if (secs(await range())[0] !== 27.9) fails.push('← after using the start: ' + await range());
  if (!(await p.locator('.clipbar .tkeys').last().textContent()).includes('move the start')) fails.push('hint does not name the start');
  // A drag snaps to the pause in the close-up (31.7-32.1 s here); ⌥ drags exactly.
  await p.waitForTimeout(400);
  const box = await p.locator('#cstrip, .clipbar .tstrip').first().boundingBox(), at = (ms) => box.x + (ms / 87000) * box.width;
  const drag = async (to, alt) => {
    const h = await p.locator('[data-ch="b"]').boundingBox();
    await p.mouse.move(h.x + h.width / 2, h.y + h.height / 2); await p.mouse.down();
    if (alt) await p.keyboard.down('Alt');
    await p.mouse.move(at(to), h.y + h.height / 2, { steps: 4 }); await p.mouse.up();
    if (alt) await p.keyboard.up('Alt');
    return secs(await range())[1];
  };
  const snapped = await drag(31500, false);
  if (snapped < 31.6 || snapped > 32.2) fails.push('drag did not snap to the pause: ' + snapped);
  await p.waitForTimeout(400);
  const exact = await drag(33500, true);
  if (Math.abs(exact - 33.5) > 0.15) fails.push('⌥-drag not exact: ' + exact);
  await p.screenshot({ path: '.playwright-out/save-clip-light.png' });
  // Save: the words of Them in the range, then a toast and the timeline back.
  await p.getByRole('button', { name: 'Save clip…' }).click();
  await p.waitForSelector('.tl');
  const save = await p.evaluate(() => window.calls.find(x => x.c === 'save_clip')?.a);
  if (!save || save.track !== 'Them' || save.fromMs !== 27900 || Math.abs(save.toMs - 33500) > 150 || save.words !== 'So Beams adds device management and reporting on top of FCM?') fails.push('save args ' + JSON.stringify(save));
  if (!await p.getByText('Saved “FCM vs Pusher Beams 00-27.wav” and its words. The meeting is unchanged.').count()) fails.push('no saved toast');
  if (!await p.evaluate(() => window.calls.some(x => x.c === 'clip_preview_clear'))) fails.push('previews not cleared on close');
  await p.close();

  // Cancelled save panel: the panel stays, no toast. Esc then closes.
  p = await open({ cancel: true, theme: 'dark' });
  await p.getByRole('menuitem', { name: 'Save a clip…' }).click();
  await p.waitForSelector('.clipbar');
  await p.waitForSelector('.cchips .chip >> text=No clipping');
  await p.screenshot({ path: '.playwright-out/save-clip-dark.png' });
  await p.getByRole('button', { name: 'Save clip…' }).click();
  await p.waitForTimeout(200);
  if (!await p.locator('.clipbar').count()) fails.push('panel closed after a cancelled save panel');
  if (await p.getByText(/Saved “/).count()) fails.push('toast after a cancelled save');
  await p.keyboard.press('Escape');
  await p.waitForSelector('.tl');
  if (await p.locator('.clipbar').count()) fails.push('Esc did not close');
  await p.close();
  if (fails.length) throw new Error(fails.join('\n'));
  return 'ok';
}
