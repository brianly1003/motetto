// UI fixtures only: processing labels move left to right without shifting layout or hiding text.
async (page) => {
  const p = await page.context().newPage();
  const engine = page.context().browser().browserType().name();
  const errors = []; p.on('pageerror', e => errors.push(e.message));
  await p.addInitScript(() => {
    const summary = { id: 'sweep', title: 'Meeting at 20:52', created_ms: Date.now(), status: 'Finishing', marks_ms: [], duration_ms: 192000, notes_ms: 0, spellings: [], locked: false, title_source: 'placeholder' };
    let next = 0;
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = { convertFileSrc: x => x, transformCallback: () => ++next, unregisterCallback: () => {}, invoke: async (c) => {
      if (c === 'plugin:event|listen') return next;
      if (c === 'get_state') return { state: { type: 'Finishing', meeting_id: 'sweep' }, meeting_id: 'sweep', companion: { visible: false, size: 'Pill' }, elapsed_ms: 192000, quit_pending: false };
      if (c === 'list_meetings') return [summary];
      if (c === 'get_meeting') return { summary, transcript: '', transcript_heard: '', notes_md: '' };
      if (c === 'deleted_meetings' || c === 'open_action_items') return [];
      if (c === 'library_bytes') return 102000000;
      if (c === 'meeting_media') return { tracks: [], duration_ms: 192000, folder: 'fixture' };
      return null;
    } };
  });
  const labels = p.locator('.step > span:nth-child(2)');
  for (const theme of ['light', 'dark']) {
    await p.emulateMedia({ colorScheme: theme, reducedMotion: 'no-preference' });
    await p.setViewportSize({ width: 1200, height: 720 });
    await p.goto('http://127.0.0.1:3000/main/');
    await p.getByRole('button', { name: /Meeting at 20:52/ }).first().click();
    await labels.first().waitFor();
    if ((await labels.allTextContents()).join('|') !== 'Recorded both sides|Transcribing your recording|Summarizing|Saving') throw Error('Missing processing labels');
    for (const width of [1200, 760]) {
      await p.setViewportSize({ width, height: 520 });
      const geometry = await labels.evaluateAll(els => els.map(el => {
        const animation = el.getAnimations().find(a => a.animationName === 'text-sweep');
        if (!animation || animation.effect.getTiming().duration !== 1800) throw Error('Label needs a 1.8s text sweep');
        animation.pause(); animation.currentTime = 0;
        const start = getComputedStyle(el).backgroundPositionX, rect = el.getBoundingClientRect();
        animation.currentTime = 900;
        const middle = getComputedStyle(el).backgroundPositionX;
        animation.currentTime = 1799;
        const end = getComputedStyle(el).backgroundPositionX;
        return { start, middle, end, width: rect.width, after: el.getBoundingClientRect().width, clip: getComputedStyle(el).backgroundClip, horizontal: getComputedStyle(el).backgroundImage.includes('90deg'), fits: el.scrollWidth <= el.clientWidth + 1 };
      }));
      if (geometry.some(g => parseFloat(g.start) <= parseFloat(g.middle) || parseFloat(g.middle) <= parseFloat(g.end) || g.width !== g.after || g.clip !== 'text' || !g.horizontal || !g.fits)) throw Error(JSON.stringify(geometry));
      await labels.evaluateAll(els => els.forEach(el => { el.getAnimations()[0].currentTime = 900; }));
      await p.locator('.writing').screenshot({ path: `.playwright-out/processing-sweep-${engine}-${theme}-${width}.png` });
    }
    await p.emulateMedia({ reducedMotion: 'reduce' });
    const reduced = await labels.evaluateAll(els => els.every(el => getComputedStyle(el).animationName === 'none' && getComputedStyle(el).backgroundImage === 'none' && getComputedStyle(el).color !== 'rgba(0, 0, 0, 0)'));
    if (!reduced) throw Error('Reduce Motion hides or animates labels');
  }
  await p.close();
  if (errors.length) throw Error(errors.join('\n'));
  return { engine, themes: 'light/dark', sizes: '1200/760', direction: 'left to right', reducedMotion: 'passed' };
}
