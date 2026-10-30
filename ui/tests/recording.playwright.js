// Run as a Playwright page function. The library while a meeting records,
// against the mockup's New recording view (DESIGN 4.8). IPC is mocked; capture is not.
async (page) => {
  const out = [];
  for (const theme of ['light', 'dark']) {
    const ref = await page.context().newPage();
    await ref.route('https://**/*', r => r.abort());
    await ref.emulateMedia({colorScheme: theme, reducedMotion: 'reduce'});
    await ref.setViewportSize({width: 1400, height: 1000});
    await ref.goto('http://127.0.0.1:3001/motetto-app-mockup.html');
    await ref.evaluate(t => document.documentElement.setAttribute('data-theme', t), theme);
    await ref.getByRole('button', {name: 'macOS', exact: true}).click();
    await ref.getByRole('button', {name: 'New recording', exact: true}).click();
    const p = await page.context().newPage();
    const errors = []; p.on('pageerror', e => errors.push(e.message));
    await p.emulateMedia({colorScheme: theme, reducedMotion: 'reduce'});
    await p.setViewportSize({width: 1048, height: 664});
    await p.addInitScript(() => {
      Object.defineProperty(navigator, 'platform', {value: 'MacIntel'});
      const now = Date.now();
      const rec = {id: 'rec1', title: 'Recorded meeting', created_ms: now - 74000, status: 'Recording', marks_ms: [], duration_ms: null, notes_ms: null, spellings: []};
      const old = {id: 'old1', title: 'Earlier call', created_ms: now - 86400e3, status: 'Ready', marks_ms: [], duration_ms: 600000, notes_ms: 40000, spellings: []};
      let next = 0; const callbacks = {}, listeners = {};
      let created = false, notes = {meeting_id: 'rec1', text: '[00:20] # Beams vs FCM', version: 1};
      let state = {state: {type: 'Idle'}, meeting_id: null, app: null, elapsed_ms: 0, started_by: null, auto_start: false, problem: null, more_problems: 0, quit_pending: false, dnd_active: false, onboarding_done: true, sharing: null, never_pending: null, companion: {visible: false, size: 'Pill', pinned: false, peeking: false, display_id: ''}};
      window.calls = [];
      window.fixtureTranscript = '[00:07] Me: As you already know, FCM is Google\'s push delivery service.\n[00:28] Them: So Beams adds device management on top of FCM?';
      const emit = (event, payload) => { const id = listeners[event]; if (id) callbacks[id]?.({event, id: 0, payload}); };
      window.fixtureState = value => { state = {...state, ...value}; emit('motetto://event', {type: 'StateChanged', ...state}); };
      window.fixtureStart = () => { created = true; window.fixtureState({state: {type: 'Recording'}, meeting_id: 'rec1', app: 'Zoom', elapsed_ms: 74000, started_by: {type: 'App', app: 'Zoom'}}); emit('motetto://library-changed', null); };
      window.fixtureStatus = status => { rec.status = status; emit('motetto://library-changed', null); };
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {unregisterListener: () => {}};
      window.__TAURI_INTERNALS__ = {metadata: {currentWindow: {label: 'main'}, currentWebview: {windowLabel: 'main', label: 'main'}}, convertFileSrc: p => p,
        transformCallback: cb => { callbacks[++next] = cb; return next; }, unregisterCallback: id => delete callbacks[id],
        invoke: async (c, a) => {
          window.calls.push({c, a});
          if (c === 'plugin:event|listen') { listeners[a.event] = a.handler; return a.handler; }
          if (c === 'get_state') return state;
          if (c === 'notification_permission') return true;
          if (c === 'list_meetings') return created ? [rec, old] : [old];
          if (c === 'deleted_meetings') return [];
          if (c === 'library_bytes') return 1;
          if (c === 'importing') return false;
          if (c === 'open_action_items') return [];
          if (c === 'get_meeting') { if (a.id === 'rec1' && !created) throw Error('not created'); return {summary: a.id === 'rec1' ? rec : old, notes_md: a.id === 'old1' ? '### Overview\n- Old notes' : '', transcript: ''}; }
          if (c === 'meeting_media') return {tracks: [], duration_ms: 0, folder: '~'};
          if (c === 'live_transcript') return ['rec1', window.fixtureTranscript, ''];
          if (c === 'get_my_notes') return notes;
          if (c === 'save_my_notes') { notes = {...notes, text: a.text, version: notes.version + 1}; return notes; }
          if (c === 'add_jot') { notes = {...notes, text: notes.text + '\n[01:14] ' + a.text, version: notes.version + 1}; return notes; }
          if (c === 'mark_moment') return state.elapsed_ms;
          if (c === 'resume_remaining_ms') return 9000;
          if (c === 'pause_recording') window.fixtureState({state: {type: 'Paused'}});
          if (c === 'resume_recording') window.fixtureState({state: {type: 'Recording'}});
          if (c === 'stop_recording') window.fixtureState({state: {type: 'Stopped', resume_until_ms: 10000}});
          return null;
        }};
    });
    await p.goto('http://127.0.0.1:3000/main/');
    const r = {};
    // Reading another meeting when the call starts: the new meeting is first, selected and open.
    await p.getByRole('button', {name: /Earlier call/}).first().click();
    await p.getByRole('heading', {name: 'Earlier call'}).waitFor();
    await p.evaluate(() => window.fixtureStart());
    await p.locator('.meeting-row', {hasText: 'Meeting at'}).waitFor();
    await p.waitForTimeout(300);
    await p.locator('.recbar').waitFor();
    r.openPage = await p.locator('.document h1').innerText();
    r.firstRow = await p.locator('.meeting-row').first().innerText();
    if (!/^Meeting at/.test(r.openPage)) throw Error(`The new recording did not open: ${r.openPage}`);
    if (await p.locator('.meeting-row.selected').innerText() !== r.firstRow) throw Error('The new recording is not the selected row');
    if (!/^Meeting at \d\d:\d\d/.test(r.firstRow) || !r.firstRow.includes('Recording')) throw Error(`New meeting not first with a Recording chip: ${r.firstRow}`);
    // The working page.
    r.bar = await p.locator('.recbar').innerText();
    if (!r.bar.includes('Recording') || !r.bar.includes('01:14') || !r.bar.includes('Both sides')) throw Error(`Recording bar: ${r.bar}`);
    if (await p.getByRole('button', {name: 'Copy', exact: true}).count() || await p.getByRole('button', {name: 'More actions'}).count()) throw Error('Copy or ••• shown while recording');
    if (await p.locator('.session-banner').count()) throw Error('Session banner repeats the recording bar');
    // Another meeting's page: its sidebar chip already says Recording, so no banner (owner 2026-10-20).
    await p.getByRole('button', {name: /Earlier call/}).first().click();
    await p.getByRole('heading', {name: 'Earlier call'}).waitFor();
    if (await p.locator('.session-banner').count()) throw Error('Session banner shown on another meeting');
    await p.locator('.meeting-row', {hasText: 'Meeting at'}).first().click();
    await p.locator('.recbar').waitFor();
    if (await p.locator('.recbody .panehead').count()) throw Error('My notes repeats its tab name (mockup has none)');
    // Visual parity with the mockup's recording page.
    const props = ['padding', 'gap', 'fontSize', 'fontWeight', 'borderRadius', 'backgroundColor', 'color', 'borderColor', 'display', 'fontFamily'];
    const differences = [];
    for (const [a, b] of [['.recbar', '.recbar'], ['.recbar .t', '.recbar .t'], ['.recbar .tip', '.recbar .tip'], ['.recbar .dot', '.recbar .dot'], ['.document .tabs', '#doc .tabs'], ['.document .tabs button', '#doc .tabs button'], ['.recbody', '.recbody'], ['.recfoot', '.recfoot'], ['.recbody > .tip', '.recbody > .tip'], ['.recbody code', '.recbody code'], ['.recbody .editor > div', '.recbody .editor > div'], ['.meeting-row i.rec', '#sidelist .chip.rec']]) {
      const x = await p.locator(a).first().evaluate((e, ps) => Object.fromEntries(ps.map(k => [k, getComputedStyle(e)[k]])), props);
      const y = await ref.locator(b).first().evaluate((e, ps) => Object.fromEntries(ps.map(k => [k, getComputedStyle(e)[k]])), props);
      for (const k of props) if (x[k] !== y[k]) differences.push(`${a} ${k}: ${x[k]} != ${y[k]}`);
    }
    await p.screenshot({path: `.playwright-out/app-recording-${theme}.png`});
    await ref.locator('.mainwin').screenshot({path: `.playwright-out/ref-recording-${theme}.png`});
    // My notes saves to the same my-notes.md as the companion.
    const editor = p.getByRole('textbox', {name: 'My notes. Each line keeps the time you wrote it.'});
    await editor.click(); await p.keyboard.press('End'); await p.keyboard.type(' pricing');
    await p.waitForFunction(() => window.calls.some(c => c.c === 'save_my_notes' && c.a.text.includes('pricing')));
    // Live transcript: listening row; clicking a line quotes it into My notes.
    await p.getByRole('tab', {name: 'Live transcript'}).click();
    await p.locator('.reclines button.line').first().waitFor();
    if (await p.locator('.reclines .line.pending').count() !== 1) throw Error('No listening row while recording');
    await p.locator('.reclines button.line').first().click();
    await p.waitForFunction(() => window.calls.some(c => c.c === 'add_jot' && c.a.text.startsWith('> As you already know')));
    if (await p.getByRole('tab', {name: 'My notes'}).getAttribute('aria-selected') !== 'true') throw Error('Quoting did not return to My notes');
    // Mark.
    await p.getByRole('button', {name: '⚑ Mark'}).click();
    await p.waitForFunction(() => document.querySelector('.recbar')?.textContent.includes('Marked 01:14'));
    // Pause: grey dot, Resume and Stop, chip Paused, no listening row; Resume leaves a gap line.
    await p.getByRole('button', {name: /Pause/}).click();
    await p.waitForFunction(() => document.querySelector('.recbar b')?.textContent === 'Paused');
    r.paused = await p.locator('.recbar').innerText();
    r.pausedChip = await p.locator('.meeting-row').first().locator('i').innerText();
    if (r.pausedChip !== 'Paused' || await p.locator('.recbar .dot.pulse').count()) throw Error(`Paused state: ${r.pausedChip}`);
    await p.screenshot({path: `.playwright-out/app-paused-${theme}.png`});
    await p.getByRole('tab', {name: 'Live transcript'}).click();
    if (await p.locator('.reclines .line.pending').count()) throw Error('Listening row while paused');
    await p.waitForTimeout(1100);
    await p.getByRole('button', {name: 'Resume', exact: true}).click();
    await p.locator('.reclines .line.gap').waitFor();
    r.gap = await p.locator('.reclines .line.gap').innerText();
    // Stop: Stopped with Resume and its countdown, chip Stopped.
    await p.getByRole('button', {name: 'Stop', exact: true}).click();
    await p.waitForFunction(() => document.querySelector('.recbar b')?.textContent === 'Stopped');
    r.stopped = await p.locator('.recbar').innerText();
    r.stoppedChip = await p.locator('.meeting-row').first().locator('i').innerText();
    // Writing the notes: steps, still no Copy or •••.
    await p.evaluate(() => { window.fixtureStatus('Finishing'); window.fixtureState({state: {type: 'Finishing', meeting_id: 'rec1'}}); });
    await p.locator('.writing h4').waitFor();
    r.writing = await p.locator('.writing').innerText();
    if (await p.getByRole('button', {name: 'Copy', exact: true}).count()) throw Error('Copy shown while the notes are written');
    await p.screenshot({path: `.playwright-out/app-writing-${theme}.png`});
    if (differences.length || errors.length) throw Error(JSON.stringify({theme, differences, errors, r}));
    out.push({theme, differences, errors, ...r});
    await p.close(); await ref.close();
  }
  // From the library home, a new recording opens straight away.
  const p = await page.context().newPage();
  await p.addInitScript(() => {
    let next = 0; const callbacks = {}, listeners = {}; let created = false;
    const rec = {id: 'rec1', title: 'Recorded meeting', created_ms: Date.now(), status: 'Recording', marks_ms: [], duration_ms: null, notes_ms: null, spellings: []};
    let state = {state: {type: 'Idle'}, meeting_id: null, app: null, elapsed_ms: 0, started_by: null, auto_start: false, problem: null, more_problems: 0, quit_pending: false, dnd_active: false, onboarding_done: true, sharing: null, never_pending: null, companion: {visible: false, size: 'Pill', pinned: false, peeking: false, display_id: ''}};
    const emit = (event, payload) => { const id = listeners[event]; if (id) callbacks[id]?.({event, id: 0, payload}); };
    window.fixtureStart = () => { state = {...state, state: {type: 'Recording'}, meeting_id: 'rec1', elapsed_ms: 3000}; emit('motetto://event', {type: 'StateChanged', ...state}); setTimeout(() => { created = true; emit('motetto://library-changed', null); }, 300); };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {unregisterListener: () => {}};
    window.__TAURI_INTERNALS__ = {metadata: {currentWindow: {label: 'main'}, currentWebview: {windowLabel: 'main', label: 'main'}}, transformCallback: cb => { callbacks[++next] = cb; return next; }, unregisterCallback: () => {}, invoke: async (c, a) => {
      if (c === 'plugin:event|listen') { listeners[a.event] = a.handler; return a.handler; }
      if (c === 'get_state') return state; if (c === 'notification_permission') return true;
      if (c === 'list_meetings') return created ? [rec] : []; if (c === 'deleted_meetings') return []; if (c === 'library_bytes') return 0;
      if (c === 'get_meeting') { if (!created) throw Error('not created'); return {summary: rec, notes_md: '', transcript: ''}; }
      if (c === 'live_transcript') return ['rec1', '', '']; if (c === 'get_my_notes') return {meeting_id: 'rec1', text: '', version: 0};
      return null; }};
  });
  const errors = []; p.on('pageerror', e => errors.push(e.message));
  await p.goto('http://127.0.0.1:3000/main/');
  await p.getByText('Your first meeting will appear here.').waitFor();
  // The folder appears 300 ms after the state, as when the worker creates it: no error banner.
  await p.evaluate(() => window.fixtureStart());
  await p.locator('.recbar').waitFor();
  if (await p.locator('.banner[role="alert"]').count()) throw Error('Opening before the folder existed showed an error');
  out.push({home: 'opened the new recording', errors});
  await p.close();
  return out;
}
