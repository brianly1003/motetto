// Scrolling the transcript by hand while it plays (owner 2026-10-25): the audio and the highlight
// carry on, even off screen, while the view stays where the reader puts it; 1.5 s after the
// scrolling stops, the view goes back to the playing line.
async (page) => {
  const fails = [];
  for (const theme of ['light', 'dark']) {
    const p = await page.context().newPage(); p.on('pageerror', e => fails.push(e.message));
    await p.emulateMedia({ colorScheme: theme });
    await p.setViewportSize({ width: 1200, height: 720 });
    await p.addInitScript(() => {
      const summary = { id: '1', title: 'Follow', created_ms: Date.now(), status: 'Ready', marks_ms: [], duration_ms: 300000, notes_ms: 8000, spellings: [], locked: false, title_source: 'placeholder' };
      const time = s => `${String(Math.floor(s / 60)).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`;
      const transcript = Array.from({ length: 200 }, (_, i) => `[${time(i)}] Line number ${i}`).join('\n');
      const meeting = { summary, transcript, transcript_heard: '', notes_md: '### Action items\n- None\n' };
      let next = 0;
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
      window.__TAURI_INTERNALS__ = { convertFileSrc: x => x, transformCallback: () => ++next, unregisterCallback: () => {}, invoke: async (c) => {
        if (c === 'plugin:event|listen') return next; if (c === 'get_state') return { state: { type: 'Idle' }, meeting_id: null, companion: { visible: false, size: 'Pill' }, elapsed_ms: 0, quit_pending: false };
        if (c === 'list_meetings') return [summary]; if (c === 'deleted_meetings') return []; if (c === 'get_meeting') return meeting;
        // No tracks: the player's clock runs on its own, one line a second.
        if (c === 'meeting_media') return { tracks: [], duration_ms: 300000, folder: 'x' }; return false; } };
    });
    await p.goto('http://127.0.0.1:3000/main/'); await p.bringToFront(); // a tab in the background gets no animation frames
    await p.getByRole('button', { name: /Follow/ }).first().click();
    await p.getByRole('tab', { name: 'Transcript' }).click();
    await p.locator('.tline[data-line="60"] button.ts').click();
    const body = p.locator('.docbody');
    const playing = async () => +(await p.locator('.tline.playing').getAttribute('data-line'));
    const inView = async () => {
      const r = await p.locator('.tline.playing').boundingBox(), v = await body.boundingBox();
      return r && r.y >= v.y - 1 && r.y + r.height <= v.y + v.height;
    };
    const top = () => body.evaluate(el => el.scrollTop);
    await p.waitForTimeout(1500);
    if (!await inView()) fails.push(`${theme}: not following before scrolling`);
    // Scroll well up for 2.5 s: the highlight moves on off screen, the view stays where it was put.
    const box = await body.boundingBox(); await p.mouse.move(box.x + 300, box.y + 200);
    const started = await playing();
    for (let i = 0; i < 10; i++) { await p.mouse.wheel(0, -150); await p.waitForTimeout(250); }
    const held = await top();
    if (await playing() < started + 2) fails.push(`${theme}: the highlight did not move on: ${started} -> ${await playing()}`);
    // Just after the scrolling stops the view is still the reader's.
    await p.waitForTimeout(700);
    if (Math.abs(await top() - held) > 1 || await inView()) fails.push(`${theme}: the view moved before the scrolling had stopped for 1.5 s`);
    await p.screenshot({ path: `.playwright-out/transcript-reading-${theme}.png` });
    // Then it goes back to the playing line and follows again.
    await p.waitForTimeout(1800);
    if (!await inView()) fails.push(`${theme}: the view did not go back after the scrolling stopped`);
    const back = await playing(); await p.waitForTimeout(2500);
    if (!await inView()) fails.push(`${theme}: not following after going back (${back} -> ${await playing()})`);
    // A seek while reading: the clicked line plays and the view does not jump away from it.
    for (let i = 0; i < 4; i++) { await p.mouse.wheel(0, -200); await p.waitForTimeout(120); }
    const line = await body.evaluate(el => { const v = el.getBoundingClientRect(); return [...el.querySelectorAll('.tline')].find(r => r.getBoundingClientRect().top > v.top + 60)?.dataset.line; });
    await p.locator(`.tline[data-line="${line}"] button.ts`).click(); await p.waitForTimeout(2000);
    if (await playing() < +line || !await inView()) fails.push(`${theme}: a seek while reading lost the playing line (${line} -> ${await playing()})`);
    // Stopped audio: scrolling away stays where the reader put it (owner 2026-10-25); Play brings the line back.
    await p.locator('.player .doc-action').click(); await p.waitForTimeout(300);
    if (!/Play/.test(await p.locator('.player .doc-action').innerText())) fails.push(`${theme}: did not pause`);
    await body.evaluate(el => { el.scrollTop = 0; }); // jump far away from the playing line
    await p.mouse.wheel(0, 120); await p.waitForTimeout(150);
    const away = await top();
    if (await inView()) fails.push(`${theme}: the playing line is still in view, the check proves nothing`);
    await p.waitForTimeout(2500);
    if (Math.abs(await top() - away) > 1 || await inView()) fails.push(`${theme}: the view followed while the audio was stopped`);
    await p.locator('.player .doc-action').click(); await p.waitForTimeout(800);
    if (!await inView()) fails.push(`${theme}: Play did not bring the playing line back`);
    // Space on the Notes tab plays and pauses when nothing has focus.
    await p.getByRole('tab', { name: 'Notes' }).click(); await p.evaluate(() => document.activeElement?.blur());
    const label = () => p.locator('.player .doc-action').innerText();
    const before = await label(); await p.keyboard.press(' '); await p.waitForTimeout(200);
    const after = await label();
    if (/Pause/.test(before) === /Pause/.test(after)) fails.push(`${theme}: Space on Notes did not toggle (${before} -> ${after})`);
    await p.close();
  }
  return fails.length ? fails : 'PASS';
}
