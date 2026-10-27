// The Video tab fits the window: from maximized down to 1024 x 680 the screen-change strip stays in
// view, and playing past the visible frames slides the strip without scrolling the page.
async (page) => {
  const fails = [];
  const ctx = await page.context().browser().newContext({ viewport: { width: 1719, height: 1000 } });
  const p = await ctx.newPage(); p.on('pageerror', e => fails.push(e.message));
  await p.addInitScript(() => {
    const summary = { id: '1', title: 'Fold', created_ms: Date.now(), status: 'Ready', marks_ms: [], duration_ms: 90000, notes_ms: null, spellings: [], title_source: 'user' };
    const video = { path: 'http://127.0.0.1:3002/screen.mp4', offset_ms: 0, duration_ms: 15000, source: 'Zoom window', while_sharing: false, width: 1826, height: 1080, bytes: 50000 };
    let n = 0;
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = { convertFileSrc: x => x, transformCallback: () => ++n, unregisterCallback: () => {}, invoke: async (c) => {
      if (c === 'plugin:event|listen') return n; if (c === 'get_state') return { state: { type: 'Idle' }, meeting_id: null, companion: { visible: false, size: 'Pill' }, elapsed_ms: 0, quit_pending: false };
      if (c === 'list_meetings') return [summary]; if (c === 'deleted_meetings') return [];
      if (c === 'get_meeting') return { summary, notes_md: 'x', transcript: '[00:01] Me: First synthetic line.\n[01:12] Them: Second synthetic line.', transcript_heard: '' };
      if (c === 'meeting_media') return { tracks: [], videos: [video, { ...video, offset_ms: 72000, while_sharing: true, source: 'Built-in Retina Display' }], moments: [0, 5000, 9000, 12000, 72000, 76000, 80000, 84000].map(ms => ({ ms, yours: ms >= 72000 })), duration_ms: 90000, folder: 'x' };
      if (c === 'get_settings') return { keep_audio: '30_days', keep_video: 'same_as_audio' }; return false; } };
  });
  await p.goto('http://127.0.0.1:3000/main/');
  await p.getByRole('button', { name: /Fold/ }).first().click();
  await p.getByRole('tab', { name: 'Video', exact: true }).click();
  await p.locator('.vthumb').first().waitFor();
  for (const [width, height] of [[1719, 1000], [1200, 800], [1024, 680]]) {
    await p.setViewportSize({ width, height }); await p.waitForTimeout(200);
    const bottom = await p.locator('.vthumbs').evaluate(e => e.getBoundingClientRect().bottom);
    if (bottom > height) fails.push(`${width}x${height}: the strip ends at ${Math.round(bottom)}, below the window`);
  }
  if (await p.locator('.vscrub .tick').count() !== 8) fails.push('the scrubber does not mark every screen change');
  await p.locator('.vscrub input').evaluate(e => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(e, '84500'); e.dispatchEvent(new Event('input', { bubbles: true })); });
  await p.waitForTimeout(800);
  const slid = await p.evaluate(() => { const s = document.querySelector('.vthumbs'), on = s.querySelector('.vthumb.on'); return { index: [...s.children].indexOf(on), visible: !!on && on.offsetLeft >= s.scrollLeft && on.offsetLeft + on.offsetWidth <= s.scrollLeft + s.clientWidth, page: document.querySelector('.docbody').scrollTop }; });
  if (slid.index !== 7 || !slid.visible) fails.push('the current screen is not shown in the strip: ' + JSON.stringify(slid));
  if (slid.page) fails.push('following the current screen scrolled the page');
  await ctx.close();
  return fails.length ? fails : 'PASS';
}
