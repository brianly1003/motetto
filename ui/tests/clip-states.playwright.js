// Run as a Playwright page function. A screen recording kept as just the video, in the states
// around its edges: a call asked about while it records, its window closed, the screen menu, the
// facts as recorded, ⌘T while typing, Esc with another menu open, a silent clip whose video
// fails, a full video under written notes in Recently deleted, and the red recording frame
// against the mockup's .recframe. IPC is mocked in this browser fixture only: nothing records.
async (page) => {
  const out = [];
  const assert = (ok, message) => { if (!ok) throw Error(message); };
  const fixture = ({label, over, clipOver, tracks, deleted}) => {
    Object.defineProperty(navigator, 'platform', {value: 'MacIntel'});
    const now = Date.now();
    const facts = {source: 'Safari window', what: 'window', mic_off: true, app_sounds: false, clicks: true, fps: 15, width: 1280, height: 800};
    const clip = {id: 'clip1', title: 'Screen recording at 10:02', created_ms: now - 60000, status: 'Ready', has_video: true, marks_ms: [], duration_ms: 44000, notes_ms: null, spellings: [], locked: false, title_source: 'placeholder', language: null, notes_language: null, apps: [], unconfirmed: [], reach_ms: null, kind: 'screen', facts, deleted_kind: 'meeting', ...clipOver};
    const setup = {mic: {kind: 'Off'}, app_sounds: true, notes: false, clicks: true, screen: {choice: {kind: 'Window', window_id: 7, app: 'Safari'}, name: 'Safari window'}};
    const state = {state: {type: 'Recording'}, meeting_id: 'clip1', app: null, elapsed_ms: 74000, started_by: {type: 'Manual'}, ask_on_call: true, problem: null, more_problems: 0, quit_pending: false, dnd_active: false, onboarding_done: true, sharing: null, share_visible: true, screen: {pick: setup.screen, while_sharing: null, waiting: false}, share_video_offer: false, companion: {visible: true, size: 'Pill', pinned: false, peeking: false, display_id: ''}, ask_off_pending: null, stops_at_ms: null, discarded: null, setup, screen_closed: null, held_clip: null, ...over};
    const media = {tracks: tracks || [], videos: [{path: 'http://127.0.0.1:3000/missing-clip.mp4', offset_ms: 0, duration_ms: 44000, source: 'Safari window', kind: 'window', while_sharing: false, width: 1280, height: 800, bytes: 1147123}], moments: [], clicks: [], duration_ms: 44000, folder: '~/Motetto/clip1'};
    const settings = {companion: {captions: true}, shortcuts: {N: 'Ctrl+Alt+N'}, keep_audio: '30_days', keep_video: '30_days', screen_preset: 'sharp'};
    window.calls = [];
    let next = 0; const callbacks = {};
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {unregisterListener: () => {}};
    window.__TAURI_INTERNALS__ = {metadata: {currentWindow: {label}, currentWebview: {windowLabel: label, label}}, convertFileSrc: p => p,
      transformCallback: cb => { callbacks[++next] = cb; return next; }, unregisterCallback: id => delete callbacks[id],
      invoke: async (c, a) => {
        window.calls.push({c, a});
        if (c === 'plugin:event|listen') return a.handler;
        if (c === 'get_state') return state;
        if (c === 'get_settings') return settings;
        if (c === 'list_meetings') return [clip];
        if (c === 'deleted_meetings') return deleted || [];
        if (c === 'get_meeting') return {summary: clip, notes_md: '', transcript: '', transcript_heard: null};
        if (c === 'meeting_media') return media;
        if (c === 'library_bytes') return 1;
        if (c === 'importing') return null;
        if (c === 'list_screen_sources') return [{pick: setup.screen, sub: 'Safari'}, {pick: {choice: {kind: 'Display', id: 1}, name: 'Built-in Display'}, sub: ''}];
        if (c === 'open_action_items' || c === 'scripts_list' || c === 'notes_translations' || c === 'call_apps') return [];
        if (c === 'microphones') return {mics: [], chosen: null, active: null, issue: null};
        if (c === 'get_levels') return [0, 0];
        if (c === 'live_transcript') return ['clip1', '', ''];
        if (c === 'resume_remaining_ms') return 9000;
        if (c === 'notification_permission') return true;
        if (c === 'playback_open') return false;
        return null;
      }};
  };
  const errors = [];
  const open = async (label, path, args, size = {width: 1048, height: 760}) => {
    const p = await page.context().newPage();
    p.on('pageerror', e => errors.push(e.message));
    await p.emulateMedia({reducedMotion: 'reduce'});
    await p.setViewportSize(size);
    await p.route('http://127.0.0.1:3000/missing-clip.mp4', r => r.fulfill({status: 404}));
    await p.addInitScript(fixture, {label, ...args});
    await p.goto(`http://127.0.0.1:3000/${path}/`);
    return p;
  };
  const called = (p, c) => p.evaluate(c => window.calls.filter(x => x.c === c).map(x => x.a), c);
  const asking = {state: {type: 'Asking', call_id: 'c1', app: 'Zoom'}, held_clip: {type: 'Recording'}};

  // 1. A call asked about while the clip records: its clock, Video only and Stop stay.
  let c = await open('companion', 'companion', {over: asking}, {width: 460, height: 72});
  await c.locator('.pillvo').waitFor();
  assert(await c.locator('.pill time.t').innerText() === '01:14', 'Clock behind the question');
  assert(await c.locator('.pillvo').innerText() === 'Video only · Zoom call started', `Question: ${await c.locator('.pillvo').innerText()}`);
  assert(await c.getByRole('button', {name: 'Record', exact: true}).count() === 1, 'Record the call');
  await c.getByRole('button', {name: 'Stop', exact: true}).click();
  await c.waitForFunction(() => window.calls.some(x => x.c === 'stop_recording'));
  out.push('asking over a clip: clock, Video only, Record, Stop');
  await c.close();

  // 2. The clip's screen menu has no Audio only.
  c = await open('companion', 'companion', {}, {width: 460, height: 420});
  await c.locator('.scrbtn').click();
  await c.locator('.scrmenu').waitFor();
  assert(await c.locator('.scrmenu').getByText('Audio only').count() === 0, 'Audio only offered for a clip');
  out.push('clip screen menu without Audio only');
  await c.close();

  // 2b. The Pill names a video-only recording; the open sidecar keeps only its icon, never
  // drawing the words under the buttons.
  c = await open('companion', 'companion', {}, {width: 460, height: 72});
  await c.locator('.pillvo').waitFor();
  assert((await c.locator('.pillvo').innerText()).startsWith('Video only'), 'Pill label');
  await c.close();
  c = await open('companion', 'companion', {over: {companion: {visible: true, size: 'Sidecar', pinned: false, peeking: false, display_id: ''}}}, {width: 340, height: 420});
  await c.locator('.pillvo').waitFor({state: 'attached'});
  const spill = await c.evaluate(() => {
    const vo = document.querySelector('.pillvo'), text = vo.querySelector(':scope > span');
    const right = getComputedStyle(text).display === 'none' ? vo.getBoundingClientRect().right : text.getBoundingClientRect().right;
    const next = vo.nextElementSibling && [...vo.parentElement.children].slice([...vo.parentElement.children].indexOf(vo) + 1).find(e => e.getBoundingClientRect().width > 0);
    return next ? right - next.getBoundingClientRect().left : 0;
  });
  assert(spill <= 0.5, `Video only runs ${spill}px under the sidecar buttons`);
  out.push('video only label fits the Pill and the sidecar');
  await c.close();

  // 3. The window closed: the Pill says so and Resume chooses another window.
  const closed = {state: {type: 'Stopped', resume_until_ms: 1e15}, screen_closed: 'Safari'};
  c = await open('companion', 'companion', {over: closed}, {width: 460, height: 72});
  await c.getByText('The Safari window closed.', {exact: true}).waitFor();
  await c.getByRole('button', {name: 'Resume', exact: true}).click();
  await c.waitForFunction(() => window.calls.some(x => x.c === 'open_capture'));
  let opened = await called(c, 'open_capture');
  assert(opened[0].kind === 'Clip' && opened[0].mode === 'window' && !(await called(c, 'resume_recording')).length, `Resume: ${JSON.stringify(opened)}`);
  await c.close();
  let m = await open('main', 'main', {over: closed});
  await m.locator('.recbar').waitFor();
  assert((await m.locator('.recbar').innerText()).includes('The Safari window closed.'), `Recbar: ${await m.locator('.recbar').innerText()}`);
  await m.locator('.recbar').getByRole('button', {name: 'Resume'}).click();
  await m.waitForFunction(() => window.calls.some(x => x.c === 'open_capture'));
  opened = await called(m, 'open_capture');
  assert(opened[0].kind === 'Clip' && opened[0].mode === 'window', `Page Resume: ${JSON.stringify(opened)}`);
  out.push('window closed: copy in the Pill and on the page, Resume opens the setup on Window');
  await m.close();

  // 4. The mic turned on mid-clip: the page says so while recording and once saved.
  m = await open('main', 'main', {over: {setup: {mic: {kind: 'Default'}, app_sounds: false, notes: false, clicks: true, screen: {choice: {kind: 'Window', window_id: 9, app: 'Notes'}, name: 'Notes window'}}, screen: {pick: {choice: {kind: 'Window', window_id: 9, app: 'Notes'}, name: 'Notes window'}, while_sharing: null, waiting: false}}});
  await m.locator('.recbar').waitFor();
  const recbar = await m.locator('.recbar').innerText();
  assert(recbar.includes('The screen · Notes window') && !recbar.includes('no mic'), `Recbar while recording: ${recbar}`);
  await m.close();
  const you = [{path: 'http://127.0.0.1:3000/clip1.mic.wav', name: 'part-0001.mic.wav', speaker: 'You', offset_ms: 5000, duration_ms: 39000, bytes: 1, waveform: []}];
  m = await open('main', 'main', {over: {state: {type: 'Idle'}, meeting_id: null, setup: null, screen: null}, tracks: you});
  await m.locator('.meeting-row').first().click();
  await m.locator('.cfacts').waitFor();
  const facts = await m.locator('.cfact').allInnerTexts();
  assert(facts.includes('Your mic') && !facts.includes('No mic'), `Facts: ${facts}`);
  assert(await m.getByRole('button', {name: 'Write notes'}).count() === 1, 'Write notes hidden though the mic was recorded');
  out.push('facts follow the recorded tracks');

  // 5. ⌘T while typing in a field is the field's; elsewhere it opens Trim.
  await m.evaluate(() => { const i = document.createElement('input'); i.id = 'typing'; document.body.append(i); });
  await m.locator('#typing').focus();
  await m.keyboard.press('Meta+t');
  await m.waitForTimeout(150);
  assert(await m.locator('.trimbar').count() === 0, '⌘T opened Trim while typing');
  await m.locator('#typing').evaluate(e => e.remove());
  await m.locator('.clipctl').click({position: {x: 300, y: 2}});
  await m.keyboard.press('Meta+t');
  await m.locator('.trimbar').waitFor();
  // 6. Esc with another menu open is that menu's; with none, it cancels Trim.
  await m.evaluate(() => { const d = document.createElement('div'); d.setAttribute('role', 'menu'); d.id = 'othermenu'; d.tabIndex = 0; document.body.append(d); d.focus(); });
  await m.keyboard.press('Escape');
  await m.waitForTimeout(150);
  assert(await m.locator('.trimbar').count() === 1, 'Esc for another menu cancelled Trim');
  await m.locator('#othermenu').evaluate(e => e.remove());
  await m.keyboard.press('Escape');
  await m.locator('.trimbar').waitFor({state: 'detached'});
  out.push('⌘T and Esc leave fields and other menus alone');
  await m.close();

  // 7. A silent clip whose video can't play promises no audio.
  m = await open('main', 'main', {over: {state: {type: 'Idle'}, meeting_id: null, setup: null, screen: null}});
  await m.locator('.meeting-row').first().click();
  await m.locator('.clipframe .vgap').waitFor();
  await m.waitForFunction(() => /could not be played/.test(document.querySelector('.clipframe .vgap')?.textContent || ''));
  const gap = await m.locator('.clipframe .vgap').innerText();
  assert(gap === 'This video could not be played.', `Silent clip error: ${gap}`);
  out.push('silent clip error copy');

  // 8. Recently deleted: a full video under written notes can't be restored.
  await m.close();
  const full = {id: 'clip1-full', title: 'Screen recording at 10:02', created_ms: Date.now() - 60000, status: 'Ready', has_video: true, marks_ms: [], duration_ms: 44000, notes_ms: null, spellings: [], locked: false, title_source: 'placeholder', language: null, notes_language: null, apps: [], unconfirmed: [], reach_ms: null, kind: 'meeting', facts: null, deleted_kind: 'trimmed_original'};
  m = await open('main', 'main', {over: {state: {type: 'Idle'}, meeting_id: null, setup: null, screen: null}, deleted: [full]});
  await m.locator('.trash').click();
  const row = m.locator('.trashlist .trow').first();
  await row.waitFor();
  assert(await row.getByRole('button', {name: 'Restore'}).count() === 0, 'Restore offered under written notes');
  assert((await row.innerText()).includes("Notes were written for the trimmed video, so the full video can't come back."), `Row: ${await row.innerText()}`);
  await m.getByRole('button', {name: 'Restore all'}).click();
  await m.waitForTimeout(200);
  assert(!(await called(m, 'meeting_action')).some(a => a.id === 'clip1-full'), 'Restore all restored it');
  out.push('full video under notes: no Restore');
  await m.close();

  // 9. The red frame, against the mockup's .recframe and its tab.
  const ref = await page.context().newPage();
  await ref.route('https://**/*', x => x.abort());
  await ref.goto('http://127.0.0.1:3001/motetto-app-mockup.html');
  const keys = ['borderTopWidth', 'borderTopStyle', 'borderTopColor', 'borderRadius', 'boxShadow'];
  const tabKeys = ['backgroundColor', 'color', 'fontSize', 'fontWeight', 'lineHeight', 'paddingTop', 'paddingLeft', 'borderBottomLeftRadius', 'whiteSpace'];
  const style = (p, sel) => p.evaluate(([sel, keys, tabKeys]) => { const e = document.querySelector(sel); const a = getComputedStyle(e), b = getComputedStyle(e, '::after'); return [Object.fromEntries(keys.map(k => [k, a[k]])), Object.fromEntries(tabKeys.map(k => [k, b[k]]))]; }, [sel, keys, tabKeys]);
  const f = await open('recframe', 'recframe', {}, {width: 646, height: 430});
  await f.locator('.recframe').waitFor({state: 'attached'});
  await f.waitForFunction(() => document.querySelector('.recframe')?.dataset.label === '● REC · Safari window');
  const [mine, refs] = [await style(f, '.recframe'), await style(ref, '#recframe')];
  const diffs = [...keys.filter(k => mine[0][k] !== refs[0][k]).map(k => `${k}: ${mine[0][k]} != ${refs[0][k]}`), ...tabKeys.filter(k => mine[1][k] !== refs[1][k]).map(k => `tab ${k}: ${mine[1][k]} != ${refs[1][k]}`)];
  assert(!diffs.length, `Frame differs from the mockup: ${diffs.join('; ')}`);
  const box = await f.locator('.recframe').boundingBox();
  assert(box.x === 1 && box.y === 1 && box.width === 644 && box.height === 404, `Frame box: ${JSON.stringify(box)}`);
  await f.screenshot({path: '.playwright-out/clip-states-recframe.png'});
  out.push('recording frame matches .recframe');
  await f.close(); await ref.close();

  assert(!errors.length, JSON.stringify(errors));
  return out;
}
