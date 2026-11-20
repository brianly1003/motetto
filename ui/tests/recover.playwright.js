// Run as a Playwright page function. Recover banner on Home (SPEC 6.1) against
// the mockup's Library screen: styles in both themes, one and several meetings, Recover, Discard, Undo.
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
      const day = new Date(); day.setHours(9, 14, 0, 0);
      const mk = (id, title, created, status) => ({id, title, created_ms: created, status, marks_ms: [], duration_ms: null, notes_ms: null, spellings: []});
      window.list = JSON.parse(localStorage.getItem('recoverList') || 'null') || [mk('1', 'Recorded meeting', day.getTime(), 'Interrupted'), mk('2', 'FCM vs Pusher Beams', day.getTime() - 3600e3, 'Ready')];
      window.calls = [];
      let next = 0;
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {unregisterListener: () => {}};
      window.__TAURI_INTERNALS__ = {metadata: {currentWindow: {label: 'main'}, currentWebview: {windowLabel: 'main', label: 'main'}}, convertFileSrc: p => p, transformCallback: () => ++next, unregisterCallback: () => {}, invoke: async (c, a) => {
        window.calls.push({c, a});
        if (c === 'plugin:event|listen') return next;
        if (c === 'get_state') return {state: {type: 'Idle'}, meeting_id: null, companion: {visible: false, size: 'Pill'}, elapsed_ms: 0, quit_pending: false};
        if (c === 'notification_permission') return true;
        if (c === 'list_meetings') return window.list;
        if (c === 'deleted_meetings') return [];
        if (c === 'library_bytes') return 1e6;
        if (c === 'open_action_items') return [];
        if (c === 'importing') return null;
        if (c === 'get_meeting') return {summary: window.list.find(m => m.id === a.id), notes_md: '', transcript: ''};
        if (c === 'meeting_media') return {tracks: [], duration_ms: 0, folder: '~'};
        if (c === 'meeting_action') {
          const m = window.list.find(x => x.id === a.id);
          if (a.action === 'regenerate') m.status = 'Ready';
          if (a.action === 'delete') { window.gone = (window.gone || []).concat(m); window.list = window.list.filter(x => x.id !== a.id); }
          if (a.action === 'restore') { window.list = window.list.concat(window.gone.filter(x => x.id === a.id)); }
          return null;
        }
        return false;
      }};
    });
    await p.goto('http://127.0.0.1:3000/main/');
    const banner = p.locator('.home .banner.alert');
    await banner.waitFor();
    const differences = [];
    const props = ['padding', 'margin', 'gap', 'fontSize', 'fontWeight', 'lineHeight', 'borderRadius', 'backgroundColor', 'color', 'borderColor', 'borderStyle', 'borderWidth', 'display', 'gridTemplateColumns', 'fontFamily'];
    const pairs = [['.home .banner', '.home .banner'], ['.home .banner .alert-icon', '.home .banner .alert-icon'], ['.home .banner .alert-title', '.home .banner .alert-title'], ['.home .banner .alert-description', '.home .banner .alert-description'], ['.home .banner .alert-actions', '.home .banner .alert-actions'], ['.home .banner .btn.sm:not(.ghost)', '#recover'], ['.home .banner .btn.ghost', '#discardRec']];
    for (const [a, b] of pairs) {
      const x = await p.locator(a).first().evaluate((e, ps) => Object.fromEntries(ps.map(k => [k, getComputedStyle(e)[k]])), props);
      const y = await ref.locator('.mainwin ' + b).first().evaluate((e, ps) => Object.fromEntries(ps.map(k => [k, getComputedStyle(e)[k]])), props);
      // The text column follows the window width; the icon and actions columns must match.
      const tracks = v => v.split(' ').filter((_, i, all) => i !== 1 || all.length < 2).join(' ');
      for (const k of props) if (k === 'gridTemplateColumns' ? tracks(x[k]) !== tracks(y[k]) : x[k] !== y[k]) differences.push(`${a} ${k}: ${x[k]} != ${y[k]}`);
    }
    const iconSame = await p.locator('.home .banner .alert-icon').innerHTML() === await ref.locator('.mainwin .home .banner .alert-icon').innerHTML();
    if (!iconSame) differences.push('warning icon paths differ');
    const text = await banner.innerText();
    if (!/Recover an interrupted recording/.test(text) || !/Motetto closed during a recording at 09:14\. The saved audio is still on this computer\./.test(text)) differences.push(`text: ${text}`);
    await p.screenshot({path: `.playwright-out/recover-${theme}.png`});
    // Recover opens the meeting and runs Write the notes again on its saved parts.
    await p.getByRole('button', {name: 'Recover', exact: true}).click();
    await p.waitForFunction(() => window.calls.some(c => c.c === 'meeting_action' && c.a.id === '1' && c.a.action === 'regenerate'));
    await p.getByText('Recovered. The recording is being transcribed.').waitFor();
    // Two interrupted meetings: one banner; Discard sends both to Recently deleted, Undo restores them.
    await p.evaluate(() => localStorage.setItem('recoverList', JSON.stringify([{...window.list[0], id: '3', status: 'Interrupted'}, {...window.list[0], id: '4', status: 'Interrupted'}, ...window.list])));
    await p.goto('http://127.0.0.1:3000/main/');
    await p.getByText('2 recordings were interrupted.', {exact: false}).waitFor();
    if (await p.locator('.home .banner.alert').count() !== 1) differences.push('more than one banner');
    await p.getByRole('button', {name: 'Discard', exact: true}).click();
    await p.getByText("Discarded the unfinished recording. It's in Recently deleted.").waitFor();
    await p.waitForFunction(() => !document.querySelector('.home .banner.alert'));
    await p.getByRole('button', {name: 'Undo'}).click();
    await p.locator('.home .banner.alert').waitFor();
    const deletes = await p.evaluate(() => window.calls.filter(c => c.c === 'meeting_action').map(c => `${c.a.action}:${c.a.id}`).slice(-4));
    if (deletes.join() !== 'delete:3,delete:4,restore:3,restore:4') differences.push(`actions: ${deletes}`);
    if (differences.length || errors.length) throw Error(JSON.stringify({theme, differences, errors}));
    await p.evaluate(() => localStorage.removeItem('recoverList'));
    out.push({theme, ok: true});
    await p.close(); await ref.close();
  }
  return out;
}
