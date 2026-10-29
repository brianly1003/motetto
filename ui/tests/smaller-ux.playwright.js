// Run as a Playwright page function. UI fixture test (IPC mocked): search
// results show where they matched and open there, Copy › Transcript, the drop zone answers a file
// drag (Tauri drag-enter/leave), and live captions stop following when scrolled up (Sidecar and
// recording page). Compared with the mockup in light and dark.
async (page) => {
  const out = [];
  const style = (loc, props) => loc.evaluate((el, props) => Object.fromEntries(props.map(k => [k, getComputedStyle(el)[k]])), props);
  const same = (a, b, what) => { for (const k of Object.keys(a)) if (a[k] !== b[k]) throw Error(`${what} ${k}: app ${a[k]} vs mockup ${b[k]}`); };
  const many = Array.from({length: 40}, (_, i) => `[00:${String(10 + i).padStart(2, '0')}] ${i % 2 ? 'Them' : 'Me'}: Line ${i} about the launch plan and who owns it.`).join('\n');
  for (const theme of ['light', 'dark']) {
    const ref = await page.context().newPage();
    await ref.route('https://**/*', r => r.abort());
    await ref.emulateMedia({colorScheme: theme, reducedMotion: 'reduce'});
    await ref.setViewportSize({width: 1400, height: 1000});
    await ref.goto('http://127.0.0.1:3001/motetto-app-mockup.html');
    await ref.evaluate(t => document.documentElement.setAttribute('data-theme', t), theme);
    await ref.getByRole('button', {name: 'After the call', exact: true}).click();

    const p = await page.context().newPage();
    const errors = []; p.on('pageerror', e => errors.push(e.message));
    await p.emulateMedia({colorScheme: theme, reducedMotion: 'reduce'});
    await p.setViewportSize({width: 1048, height: 664});
    await p.addInitScript(many => {
      const now = Date.now();
      const meeting = {summary: {id: '123', title: 'Design review', created_ms: now, status: 'Ready', marks_ms: [], duration_ms: 600000, notes_ms: null, spellings: []},
        notes_md: '### Overview\n- We agreed on the plan.', transcript: '[00:04] Me: Publish the roadmap.\n[00:09] Them: The roadmap goes out Friday.', transcript_heard: ''};
      const other = {...meeting.summary, id: '124', title: 'Roadmap sync', created_ms: now - 86400e3};
      let next = 0; const callbacks = {}, listeners = {};
      Object.defineProperty(navigator, 'clipboard', {value: {writeText: async text => { window.copiedText = text; }}});
      window.emit = (event, payload) => callbacks[listeners[event]]?.({event, id: 0, payload});
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {unregisterListener: () => {}};
      window.__TAURI_INTERNALS__ = {metadata: {currentWindow: {label: 'main'}, currentWebview: {windowLabel: 'main', label: 'main'}}, convertFileSrc: p => p,
        transformCallback: cb => { callbacks[++next] = cb; return next; }, unregisterCallback: id => delete callbacks[id],
        invoke: async (c, a) => {
          if (c === 'plugin:event|listen') { listeners[a.event] = a.handler; return a.handler; }
          if (c === 'get_state') return {state: {type: 'Idle'}, meeting_id: null, companion: {visible: false, size: 'Pill'}, elapsed_ms: 0, quit_pending: false};
          if (c === 'notification_permission') return true;
          if (c === 'list_meetings') return [meeting.summary, other];
          if (c === 'deleted_meetings') return [];
          if (c === 'library_bytes') return 1;
          if (c === 'importing') return null;
          if (c === 'open_action_items') return [];
          if (c === 'get_meeting') return a.id === '123' ? meeting : {...meeting, summary: other, transcript: ''};
          if (c === 'meeting_media') return {tracks: [], duration_ms: 600000, folder: '~'};
          if (c === 'search_meetings') return [
            {meeting: meeting.summary, snippet: {source: 'Transcript', text: 'The roadmap goes out Friday.', at_ms: 9000}},
            {meeting: other, snippet: null}];
          return null;
        }};
    }, many);
    await p.goto('http://127.0.0.1:3000/main/');

    // 1. Search results show where they matched, and open there.
    await p.getByPlaceholder('Search notes and transcripts').fill('roadmap');
    await p.locator('.meeting-row.hit').first().waitFor();
    const hit = p.locator('.meeting-row.hit').first();
    if (await p.locator('.list-heading').first().innerText() !== 'Results\n2' && !/Results\s*2/.test(await p.locator('.list-heading').first().innerText())) throw Error('Results heading');
    if (await hit.locator('.snip mark').innerText() !== 'roadmap') throw Error('Snippet not marked');
    if (await hit.locator('.where').innerText() !== 'In the transcript · 00:09') throw Error(`Where: ${await hit.locator('.where').innerText()}`);
    if (await p.locator('.meeting-row.hit').nth(1).locator('.snip').count()) throw Error('Title-only hit shows a snippet');
    if (await p.locator('.meeting-row.hit').nth(1).locator('strong mark').innerText() !== 'Roadmap') throw Error('Title not marked');
    await ref.locator('#q').fill('beams');
    const refHit = ref.locator('.mitem[data-find]').first();
    await refHit.waitFor();
    same(await style(hit.locator('.snip'), ['fontSize', 'lineHeight', 'color', 'webkitLineClamp']), await style(refHit.locator('.snip'), ['fontSize', 'lineHeight', 'color', 'webkitLineClamp']), 'snip');
    same(await style(hit.locator('.snip mark'), ['backgroundColor', 'borderRadius', 'paddingLeft']), await style(refHit.locator('.snip mark'), ['backgroundColor', 'borderRadius', 'paddingLeft']), 'mark');
    same(await style(hit.locator('.where'), ['fontSize', 'fontWeight', 'color']), await style(refHit.locator('.where'), ['fontSize', 'fontWeight', 'color']), 'where');
    await hit.screenshot({path: `/tmp/motetto-ux-hit-app-${theme}.png`}).catch(() => {});
    await hit.click();
    await p.locator('.findbar input').waitFor();
    if (await p.getByRole('tab', {name: 'Transcript'}).getAttribute('aria-selected') !== 'true') throw Error('Hit did not open the transcript');
    if (await p.locator('.findbar input').inputValue() !== 'roadmap') throw Error('Find not filled');
    await p.waitForFunction(() => /1 of 2/.test(document.querySelector('.findbar output')?.textContent || ''));
    await refHit.click();
    await ref.waitForFunction(() => /1 of/.test(document.querySelector('#findCount')?.textContent || ''));
    if (!await hit.evaluate(e => e === document.activeElement)) throw Error('Opening a result took focus from the sidebar');
    await p.keyboard.press('Escape');
    if (await p.locator('.findbar').count()) throw Error('Esc did not close Find');
    if (!await p.locator('.meeting-row.hit').first().evaluate(e => e === document.activeElement)) throw Error('Esc did not return focus to the result');
    await p.locator('.meeting-row.hit').nth(1).click();
    await p.getByRole('heading', {name: 'Roadmap sync'}).waitFor();
    if (await p.locator('.findbar').count()) throw Error('A title-only result opened Find');
    await p.locator('.meeting-row.hit').first().click();
    await p.getByRole('heading', {name: 'Design review'}).waitFor();
    await p.keyboard.press('Escape');

    // 2. Copy › Transcript copies the saved lines with Me/Them.
    await p.getByRole('button', {name: 'Copy ▾', exact: true}).click();
    const items = await p.getByRole('menu', {name: 'Copy'}).getByRole('menuitem').allInnerTexts();
    if (items.at(-1) !== 'Transcript') throw Error(`Copy menu: ${items}`);
    await p.getByRole('menuitem', {name: 'Transcript', exact: true}).click();
    if (await p.evaluate(() => window.copiedText) !== '[00:04] Me: Publish the roadmap.\n[00:09] Them: The roadmap goes out Friday.') throw Error('Transcript copy text');
    await ref.locator('[data-tab="notes"]').click().catch(() => {});
    await ref.getByRole('button', {name: /Copy/}).first().click();
    if ((await ref.locator('.dropdown [role=menuitem]').allInnerTexts()).at(-1).trim() !== 'Transcript') throw Error('Mockup copy menu');

    // 3. A dragged file: frame over the meeting page, then the Home drop zone answers; leave restores.
    await p.evaluate(() => window.emit('tauri://drag-enter', {paths: ['/Users/x/call.m4a'], position: {x: 400, y: 300}}));
    const frame = await p.locator('.shell').evaluate(el => getComputedStyle(el, '::after').content);
    if (!/Release to make notes/.test(frame)) throw Error(`No frame over the meeting: ${frame}`);
    await p.evaluate(() => window.emit('tauri://drag-leave', null));
    await p.getByPlaceholder('Search notes and transcripts').fill('');
    await p.getByRole('button', {name: 'Action items'}).first().click();
    // Home has no drop zone (owner 2026-10-30): Import is in its header and a file dragged over it
    // gets the same frame as over a meeting, as in the mockup.
    await p.locator('.homehead').waitFor();
    if (await p.locator('.home .drop').count()) throw Error('Drop zone still on Home');
    await p.evaluate(() => window.emit('tauri://drag-enter', {paths: ['/Users/x/call.m4a'], position: {x: 400, y: 300}}));
    await p.waitForFunction(() => /Release to make notes/.test(getComputedStyle(document.querySelector('.shell'), '::after').content));
    await p.locator('.home').screenshot({path: `/tmp/motetto-ux-drop-app-${theme}.png`}).catch(() => {});
    await ref.getByRole('button', {name: 'Library', exact: true}).click().catch(() => {});
    await ref.locator('.homehead').waitFor();
    await ref.evaluate(() => { const dt = new DataTransfer(); dt.items.add(new File(['x'], 'call.m4a')); document.querySelector('.mainwin').dispatchEvent(new DragEvent('dragenter', {dataTransfer: dt, bubbles: true, cancelable: true})); });
    if (!/One import at a time|Release to make notes/.test(await ref.locator('.mainwin .doc').evaluate(el => getComputedStyle(el, '::after').content))) throw Error('Mockup frame over Home');
    await p.evaluate(() => window.emit('tauri://drag-drop', {paths: ['/Users/x/call.m4a'], position: {x: 400, y: 300}}));
    await p.waitForFunction(() => getComputedStyle(document.querySelector('.shell'), '::after').content === 'none');
    if (errors.length) throw Error(errors.join('; '));
    out.push(`${theme}: library ok`);
    await p.close(); await ref.close();
  }

  // 4. Live captions: Sidecar and recording page follow only at the end.
  for (const theme of ['light', 'dark']) {
    for (const where of ['companion', 'main']) {
      const p = await page.context().newPage();
      const errors = []; p.on('pageerror', e => errors.push(e.message));
      await p.emulateMedia({colorScheme: theme, reducedMotion: 'reduce'});
      await p.setViewportSize(where === 'companion' ? {width: 340, height: 560} : {width: 1048, height: 664});
      await p.addInitScript(({where, many}) => {
        let next = 0; const callbacks = {}, listeners = {};
        window.text = many.split('\n').slice(0, 30).join('\n');
        const rec = {id: 'm1', title: 'Call', created_ms: Date.now(), status: 'Recording', marks_ms: [], duration_ms: null, notes_ms: null, spellings: []};
        const state = {state: {type: 'Recording'}, meeting_id: 'm1', app: 'Zoom', elapsed_ms: 60000, started_by: {type: 'App', app: 'Zoom'}, problem: null, more_problems: 0, quit_pending: false, dnd_active: false, onboarding_done: true, sharing: null, ask_off_pending: null, stops_at_ms: null,
          companion: {visible: true, size: 'Sidecar', pinned: true, peeking: false, display_id: ''}};
        window.more = () => { window.text = many; };
        window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {unregisterListener: () => {}};
        window.__TAURI_INTERNALS__ = {metadata: {currentWindow: {label: where}, currentWebview: {windowLabel: where, label: where}}, convertFileSrc: p => p,
          transformCallback: cb => { callbacks[++next] = cb; return next; }, unregisterCallback: id => delete callbacks[id],
          invoke: async (c, a) => {
            if (c === 'plugin:event|listen') { listeners[a.event] = a.handler; return a.handler; }
            if (c === 'get_state') return state; if (c === 'get_levels') return [0, 0];
            if (c === 'live_transcript') return ['m1', window.text, ''];
            if (c === 'get_my_notes') return {meeting_id: 'm1', text: '', version: 1};
            if (c === 'list_meetings') return [rec]; if (c === 'get_meeting') return {summary: rec, notes_md: '', transcript: '', transcript_heard: ''};
            if (c === 'deleted_meetings' || c === 'open_action_items') return []; if (c === 'library_bytes') return 1; if (c === 'importing') return null;
            if (c === 'meeting_media') return {tracks: [], duration_ms: 0, folder: '~'}; if (c === 'notification_permission') return true;
            return null;
          }};
      }, {where, many});
      await p.goto(`http://127.0.0.1:3000/${where}/`);
      if (where === 'main') { await p.getByRole('tab', {name: 'Live transcript'}).click(); }
      const box = p.locator(where === 'main' ? '.recbody' : '.live-pane .lines');
      await p.locator('.line', {hasText: 'Line 29'}).waitFor();
      const gap = () => box.evaluate(el => el.scrollHeight - el.scrollTop - el.clientHeight);
      await p.waitForFunction(sel => { const el = document.querySelector(sel); return el && el.scrollHeight - el.scrollTop - el.clientHeight < 2; }, where === 'main' ? '.recbody' : '.live-pane .lines');
      if (await p.locator('.jumpnow').count()) throw Error(`${where}: chip at the end`);
      await box.evaluate(el => { el.scrollTop = 0; });
      await p.getByRole('button', {name: 'Jump to now'}).waitFor();
      await p.evaluate(() => window.more());
      await p.locator('.line', {hasText: 'Line 39'}).waitFor();
      await p.waitForTimeout(300);
      if (await box.evaluate(el => el.scrollTop) !== 0) throw Error(`${where}: followed while scrolled up`);
      const chip = await p.getByRole('button', {name: 'Jump to now'}).boundingBox(), pane = await box.boundingBox();
      if (chip.y + chip.height > pane.y + pane.height || chip.y < pane.y) throw Error(`${where}: chip outside the pane`);
      await p.screenshot({path: `/tmp/motetto-ux-jump-${where}-${theme}.png`});
      await p.getByRole('button', {name: 'Jump to now'}).click();
      await p.waitForFunction(() => !document.querySelector('.jumpnow'));
      if (await gap() > 2) throw Error(`${where}: Jump to now did not reach the end`);
      if (errors.length) throw Error(errors.join('; '));
      out.push(`${theme}: ${where} live follow ok`);
      await p.close();
    }
  }
  return out;
}
