// Run as a Playwright page function. Library home against the mockup's Library screen.
async (page) => {
  const out = [];
  for (const theme of ['dark', 'light']) {
    const ref = await page.context().newPage();
    await ref.route('https://**/*', r => r.abort());
    await ref.emulateMedia({colorScheme: theme});
    await ref.setViewportSize({width: 1400, height: 1000});
    await ref.goto('http://127.0.0.1:3001/motetto-app-mockup.html');
    await ref.evaluate(t => document.documentElement.setAttribute('data-theme', t), theme);
    await ref.getByRole('button', {name: 'macOS', exact: true}).click();
    await ref.getByRole('button', {name: 'Library', exact: true}).click();
    const p = await page.context().newPage();
    const errors = []; p.on('pageerror', e => errors.push(e.message));
    await p.emulateMedia({colorScheme: theme});
    await p.setViewportSize({width: 1048, height: 664});
    await p.addInitScript(() => {
      Object.defineProperty(navigator, 'platform', {value: 'MacIntel'});
      const now = Date.now();
      const mk = (id, title, ago, status, dur) => ({id, title, created_ms: now - ago, status, marks_ms: [], duration_ms: dur, notes_ms: 48000, spellings: []});
      const list = [mk('1', 'FCM vs Pusher Beams', 600e3, 'Ready', 87000), mk('2', 'Promotion rollover review', 86400e3, 'Finishing', 2580000), mk('3', 'Payment proxy page', 3 * 86400e3, 'Ready', 2280000), mk('4', 'Daily standup', 3.1 * 86400e3, 'Ready', 660000), mk('5', 'Beta flag decision', 9 * 86400e3, 'NeedsAttention', 1440000)];
      const notes = {'1': '### Action items\n- [ ] You: Share the device table design before Thursday [01:20]\n- [ ] Kevin: Check whether the 2021 Firebase project is still active [00:56]\n- [x] Book a follow-up [01:14]\n', '3': '### Action items\n- [ ] You: Send the V4/B4 performance list to Kevin [21:40]\n', '4': '### Action items\n- [ ] Linh: Update the API docs for the wallet endpoint [06:12]\n'};
      window.calls = [];
      let next = 0;
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {unregisterListener: () => {}};
      window.__TAURI_INTERNALS__ = {metadata: {currentWindow: {label: 'main'}, currentWebview: {windowLabel: 'main', label: 'main'}}, convertFileSrc: p => p, transformCallback: () => ++next, unregisterCallback: () => {}, invoke: async (c, a) => {
        window.calls.push({c, a});
        if (c === 'plugin:event|listen') return next;
        if (c === 'get_state') return {state: {type: 'Idle'}, meeting_id: null, companion: {visible: false, size: 'Pill'}, elapsed_ms: 0, quit_pending: false};
        if (c === 'notification_permission') return true;
        if (c === 'list_meetings') return list;
        if (c === 'deleted_meetings') return [];
        if (c === 'library_bytes') return 2.1e9;
        if (c === 'open_action_items') return list.filter(m => notes[m.id] && /- \[ \]/.test(notes[m.id])).map(m => [m, notes[m.id]]);
        if (c === 'set_action_item_done') { if (a.expected !== notes[a.id]) throw Error('stale'); const l = notes[a.id].split('\n'); l[a.line] = l[a.line].replace(/\[[ x]\]/, a.done ? '[x]' : '[ ]'); notes[a.id] = l.join('\n'); return notes[a.id]; }
        if (c === 'get_meeting') return {summary: list.find(m => m.id === a.id), notes_md: notes[a.id] || '', transcript: ''};
        if (c === 'meeting_media') return {tracks: [], duration_ms: 0, folder: '~'};
        return false;
      }};
    });
    await p.goto('http://127.0.0.1:3000/main/');
    await p.locator('.openitems .oi').first().waitFor();
    const differences = [];
    const props = ['padding', 'gap', 'fontSize', 'fontWeight', 'lineHeight', 'borderRadius', 'backgroundColor', 'color', 'borderColor', 'borderStyle', 'borderWidth', 'display', 'textAlign', 'fontFamily'];
    const pairs = [['.home', '.home'], ['.home h1', '.home h2'], ['.homehead .tip', '.homehead .tip'], ['.hseg', '.hseg'], ['.hseg button[aria-checked="true"]', '.hseg button[aria-checked="true"]'], ['.hseg button[aria-checked="false"]', '.hseg button[aria-checked="false"]'], ['.homeacts .btn', '.homeacts #import'], ['.olist', '.olist'], ['.mhead', '.mhead'], ['.mname', '.mname'], ['.mhead .mday', '.mhead .mday'], ['.mact .n', '.mact .n'], ['.mact .iconbtn', '.mact .iconbtn'], ['.mlabel', '.mlabel'], ['.mgroup .openitems .oi', '.mgroup .openitems .oi'], ['.openitems .owner:not(.me)', '.openitems .owner'], ['.srcchip', '.srcchip'], ['.meeting-row > span', '.mitem .mm'], ['.meeting-row', '.mitem']];
    for (const [a, b] of pairs) {
      const x = await p.locator(a).first().evaluate((e, ps) => Object.fromEntries(ps.map(k => [k, getComputedStyle(e)[k]])), props);
      const y = await ref.locator('.mainwin ' + b).first().evaluate((e, ps) => Object.fromEntries(ps.map(k => [k, getComputedStyle(e)[k]])), props);
      for (const k of props) if (x[k] !== y[k]) differences.push(`${a} ${k}: ${x[k]} != ${y[k]}`);
    }
    // A long status chip stays on the row's right, as in the mockup; the date text wraps instead.
    const chip = await p.locator('.meeting-row', {hasText: 'Promotion rollover review'}).evaluate(row => { const [meta, chip] = [row.querySelector('span'), row.querySelector('i')]; return chip.getBoundingClientRect().top < meta.getBoundingClientRect().bottom - 4; });
    if (!chip) differences.push('status chip wrapped below the date');
    await p.screenshot({path: `.playwright-out/home-${theme}.png`});
    await p.locator('.openitems .oi input').first().click();
    await p.getByText('Marked done.').waitFor();
    await p.waitForFunction(() => document.querySelectorAll('.openitems .oi').length === 3);
    await p.getByRole('button', {name: 'Undo'}).click();
    await p.waitForFunction(() => document.querySelectorAll('.openitems .oi').length === 4);
    // No drop zone on Home: Import stays in the header (owner 2026-10-30).
    if (await p.locator('.home .drop').count()) differences.push('drop zone still on Home');
    // One header per meeting, newest first; "Waiting on others" only where yours come first.
    const heads = await p.locator('.mhead .mname').allInnerTexts();
    if (heads.join('|') !== 'FCM vs Pusher Beams|Payment proxy page|Daily standup') differences.push(`groups: ${heads}`);
    if ((await p.locator('.mlabel').count()) !== 1) differences.push('Waiting on others label count');
    await p.getByRole('radio', {name: /Yours/}).click();
    if ((await p.locator('.mgroup').count()) !== 2) differences.push('Yours filter groups');
    await p.getByRole('radio', {name: 'All'}).click();
    await p.locator('.mact .iconbtn').first().click();
    await p.getByText('Action items copied.').waitFor();
    await p.locator('.srcchip').first().click();
    await p.waitForFunction(() => window.calls.some(c => c.c === 'get_meeting' && c.a.id === '1'));
    if (differences.length || errors.length) throw Error(JSON.stringify({theme, differences, errors}));
    out.push({theme, differences, errors});
    await p.close(); await ref.close();
  }
  // The empty library (no meetings) against the mockup's first-launch state.
  for (const theme of ['dark', 'light']) {
    const ref = await page.context().newPage();
    await ref.route('https://**/*', r => r.abort());
    await ref.emulateMedia({colorScheme: theme});
    await ref.setViewportSize({width: 1400, height: 1000});
    await ref.goto('http://127.0.0.1:3001/motetto-app-mockup.html');
    await ref.evaluate(t => document.documentElement.setAttribute('data-theme', t), theme);
    await ref.getByRole('button', {name: 'macOS', exact: true}).click();
    await ref.getByRole('button', {name: 'Library', exact: true}).click();
    await ref.locator('#toggleEmpty').click();
    const p = await page.context().newPage();
    const errors = []; p.on('pageerror', e => errors.push(e.message));
    await p.emulateMedia({colorScheme: theme});
    await p.setViewportSize({width: 1048, height: 664});
    await p.addInitScript(() => {
      Object.defineProperty(navigator, 'platform', {value: 'MacIntel'});
      window.calls = []; let next = 0;
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {unregisterListener: () => {}};
      window.__TAURI_INTERNALS__ = {metadata: {currentWindow: {label: 'main'}, currentWebview: {windowLabel: 'main', label: 'main'}}, transformCallback: () => ++next, unregisterCallback: () => {}, invoke: async (c, a) => {
        window.calls.push({c, a});
        if (c === 'plugin:event|listen') return next;
        if (c === 'get_state') return {state: {type: 'Idle'}, meeting_id: null, companion: {visible: false, size: 'Pill'}, elapsed_ms: 0, quit_pending: false};
        if (c === 'notification_permission') return true;
        if (c === 'list_meetings' || c === 'deleted_meetings') return []; if (c === 'library_bytes') return 0;
        return null; }};
    });
    await p.goto('http://127.0.0.1:3000/main/');
    await p.getByText('Your first meeting will appear here.').waitFor();
    const differences = [];
    const props = ['padding', 'gap', 'fontSize', 'fontWeight', 'fontFamily', 'borderRadius', 'backgroundColor', 'color', 'borderColor', 'borderStyle', 'borderWidth', 'display', 'width', 'height', 'opacity'];
    for (const [a, b] of [['.home h1', '.home h2'], ['.empty', '.empty'], ['.empty .mark', '.empty .mark'], ['.empty b', '.empty b'], ['.empty > span', '.empty > span'], ['.empty .field', '.empty .field'], ['.empty .btn:not(.ghost)', '.empty [data-act="startManual"]'], ['.empty .btn.ghost', '.empty #import']]) {
      const x = await p.locator(a).first().evaluate((e, ps) => Object.fromEntries(ps.map(k => [k, getComputedStyle(e)[k]])), props);
      const y = await ref.locator('.mainwin ' + b).first().evaluate((e, ps) => Object.fromEntries(ps.map(k => [k, getComputedStyle(e)[k]])), props);
      for (const k of props) if (x[k] !== y[k] && !(k === 'width' && !a.includes('mark')) && !(k === 'height' && !a.includes('mark'))) differences.push(`${a} ${k}: ${x[k]} != ${y[k]}`);
    }
    if ((await p.locator('.empty').innerText()) !== (await ref.locator('.mainwin .empty').innerText())) differences.push('empty-state text differs');
    await p.screenshot({path: `.playwright-out/empty-app-${theme}.png`});
    await ref.locator('.mainwin').screenshot({path: `.playwright-out/empty-ref-${theme}.png`});
    await p.getByRole('button', {name: 'Record now'}).click();
    await p.waitForFunction(() => window.calls.some(c => c.c === 'start_recording'));
    if (differences.length || errors.length) throw Error(JSON.stringify({theme, empty: true, differences, errors}));
    out.push({theme, empty: true, differences, errors});
    await p.close(); await ref.close();
  }
  return out;
}
