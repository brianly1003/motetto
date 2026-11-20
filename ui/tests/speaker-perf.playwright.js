// Speaker names on a long call (SPEC 7.11): a rename or a move made at the bottom of 2,000 lines
// shows on the lines in view at once, and the main thread never blocks long. Measures the time
// from Enter to the changed name under the popover's line, its glow's delay (none on the clicked
// line, at most a screen's worth on any line), and the longest task (Chromium only reports it).
async (page) => {
  const fails = [], report = [];
  const p = await page.context().newPage(); p.on('pageerror', e => fails.push(e.message));
  await p.setViewportSize({ width: 1200, height: 720 });
  await p.addInitScript(() => {
    const summary = { id: '1', title: 'Long call', created_ms: Date.now(), status: 'Ready', marks_ms: [], duration_ms: 3000000, notes_ms: 8000, spellings: [], locked: false, title_source: 'placeholder', speaker_names: { 2: 'Nathan', 3: 'Kate' } };
    const time = s => `${String(Math.floor(s / 60)).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`;
    // Runs of two lines per voice, so a line has a run around it.
    const transcript = Array.from({ length: 2000 }, (_, i) => `[${time(i)}] ${i % 10 === 9 ? 'Me' : `Speaker ${2 + Math.floor(i / 2) % 3}`}: Line number ${i} with a few more words so it wraps like a real line of speech does`).join('\n');
    const meeting = { summary, transcript, transcript_heard: '', notes_md: '### Overview\nSpeaker 2 spoke.\n' };
    // Every save is followed by the library's changed event, as from the app's own commands.
    let next = 0; const callbacks = {}, listeners = {};
    const changed = () => setTimeout(() => (listeners['motetto://library-changed'] || []).forEach(h => callbacks[h]?.({ event: 'motetto://library-changed', id: 0, payload: null })), 20);
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = { convertFileSrc: x => x, transformCallback: cb => { const id = ++next; callbacks[id] = cb; return id; }, unregisterCallback: id => { delete callbacks[id]; }, invoke: async (c, a) => {
      if (c === 'plugin:event|listen') { (listeners[a.event] ||= []).push(a.handler); return a.handler; } if (c === 'get_state') return { state: { type: 'Idle' }, meeting_id: null, companion: { visible: false, size: 'Pill' }, elapsed_ms: 0, quit_pending: false };
      if (c === 'list_meetings') return [summary]; if (c === 'deleted_meetings') return []; if (c === 'get_meeting') return meeting;
      if (c === 'set_speaker_names') { changed(); summary.speaker_names = Object.fromEntries(Object.entries(a.names).filter(([, v]) => v)); return { ...summary }; }
      if (c === 'move_transcript_lines') { changed(); const rows = meeting.transcript.split('\n'); a.indices.forEach(i => { rows[i] = rows[i].replace(/^(\[[0-9:]+\] )Speaker \d+:/, `$1Speaker ${a.voice}:`); }); meeting.transcript = rows.join('\n'); return meeting.transcript; }
      if (c === 'save_notes') return a.markdown;
      if (c === 'meeting_media') return { tracks: [], duration_ms: 3000000, folder: 'x' }; return false; } };
  });
  await p.goto('http://127.0.0.1:3000/main/'); await p.bringToFront();
  await p.getByRole('button', { name: /Long call/ }).first().click();
  await p.getByRole('tab', { name: 'Transcript' }).click();
  await p.locator('.tline[data-line="1999"]').waitFor({ state: 'attached' });
  await p.evaluate(() => { window.longest = 0; new PerformanceObserver(l => { for (const e of l.getEntries()) window.longest = Math.max(window.longest, e.duration); }).observe({ type: 'longtask', buffered: false }); });
  // Time until the line's name reads `want`.
  const shown = (line, want) => p.evaluate(([line, want]) => new Promise(done => {
    const t = performance.now();
    const look = () => {
      const who = document.querySelector(`.tline[data-line="${line}"] .who span`);
      if (who && who.textContent === want) done(Math.round(performance.now() - t));
      else if (performance.now() - t > 10000) done(-1); else requestAnimationFrame(look);
    };
    look();
  }), [line, want]);
  const step = async (label, line, scope, name, want) => {
    await p.locator(`.tline[data-line="${line}"]`).scrollIntoViewIfNeeded();
    await p.locator(`.tline[data-line="${line}"] .who.voice`).click();
    if (scope) await p.getByRole('radio', { name: scope }).click();
    await p.locator('.pop.spk input[type=text]').fill(name);
    await p.evaluate(() => { window.longest = 0; });
    const [ms] = await Promise.all([shown(line, want), p.locator('.pop.spk input[type=text]').press('Enter')]);
    const delays = await p.evaluate(line => [...document.querySelectorAll('.tline .who.voice.renamed')].map(e => [Number(e.closest('.tline').dataset.line), parseFloat(e.style.getPropertyValue('--d'))]), line);
    const own = delays.find(([l]) => l === line)?.[1], most = Math.max(0, ...delays.map(([, d]) => d));
    const far = Math.max(0, ...delays.map(([l]) => Math.abs(l - line)));
    if (own !== 0 || most > 480 || far > 40) fails.push(`${label}: glow waits ${own} ms here, up to ${most} ms, ${far} rows away`);
    await p.waitForTimeout(1600);
    const longest = Math.round(await p.evaluate(() => window.longest));
    report.push(`${label}: shown ${ms} ms, longest task ${longest} ms`);
    // WebKit draws a rename of a 2,000-line call in about 250 ms, Chromium in about 120 ms.
    if (ms < 0 || ms > 350) fails.push(`${label}: the name took ${ms} ms to show`);
    if (longest > 200) fails.push(`${label}: a ${longest} ms task blocked the page`);
  };
  // Every line of a voice, from a line at the bottom; then a run there moves to someone else.
  await step('rename all', 1996, null, 'Mike', 'Mike');
  await step('move a run', 1994, /in a row/, 'Holmet', 'Holmet');
  await p.close();
  return fails.length ? [...fails, ...report] : 'PASS';
}
