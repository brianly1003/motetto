// Run as a Playwright page function. The microphone picker (DESIGN 4.15)
// on the recording page, Settings › Audio, the companion Pill and the Ask card, against the
// mockup. The user decides: no warning about the chosen mic, facts in the list, and only measured
// clipping shows the amber line (owner 2026-10-23). IPC is mocked in this browser fixture only; real devices and capture are not exercised.
async (page) => {
  const out = [];
  const keys = ['height', 'fontSize', 'borderRadius', 'backgroundColor', 'color', 'paddingLeft', 'gap'];
  const styles = (loc) => loc.evaluate((e, k) => Object.fromEntries(k.map(x => [x, getComputedStyle(e)[x]])), keys);
  const same = (a, b, what) => { for (const k of keys) if (a[k] !== b[k]) throw Error(`${what} ${k}: ${a[k]} != ${b[k]}`); };
  const fixture = ({label, start}) => {
    Object.defineProperty(navigator, 'platform', {value: 'MacIntel'});
    const mics = [
      {id: 'shokz', name: 'OpenRun Pro 2 by Shokz', transport: 'Bluetooth', rate_hz: 16000, volume: null, is_default: true},
      {id: 'builtin', name: 'MacBook Pro Microphone', transport: 'BuiltIn', rate_hz: 48000, volume: .53, is_default: false},
      {id: 'usb', name: 'Shure MV7', transport: 'Usb', rate_hz: 48000, volume: .5, is_default: false},
    ];
    let chosen = null, loud = false;
    const report = () => {
      const active = mics.find(m => m.id === chosen) || mics.find(m => m.is_default);
      return {mics, chosen, active, issue: loud ? 'TooLoud' : null};
    };
    window.fixtureLoud = () => { loud = true; };
    let next = 0; const callbacks = {}, listeners = {};
    window.fixtureEmit = (event, payload) => { if (!listeners[event]) throw Error(`No listener for ${event}`); callbacks[listeners[event]]({event, id: 0, payload}); };
    const now = Date.now();
    const rec = {id: 'rec1', title: 'Recorded meeting', created_ms: now - 74000, status: 'Recording', marks_ms: [], duration_ms: null, notes_ms: null, spellings: []};
    let state = {state: start, meeting_id: 'rec1', app: 'Zoom', elapsed_ms: 74000, started_by: {type: 'App', app: 'Zoom'}, auto_start: false, problem: null, more_problems: 0, quit_pending: false, dnd_active: false, onboarding_done: true, sharing: null, never_pending: null, companion: {visible: true, size: start.type === 'Asking' ? 'Ask' : 'Pill', pinned: false, peeking: false, display_id: ''}};
    const settings = {version: 1, notes_folder: '~/Documents/Motetto', open_at_login: true, notes_language: 'same_as_call', ask_on_call: true, app_modes: {}, apps: [], idle_stop_s: 30, discard_short_s: 20, speech_model: '', notes_model: '', call_language: 'detect', part_chars: 24000, keep_audio: '30_days', recording_reminder: true, mic_device: null, sounds: true, companion: {default_size: 'pill', captions: true, pinned: false, auto_collapse: true}, spellings: [], shortcuts: {S: 'Ctrl+Alt+S', M: 'Ctrl+Alt+M', J: 'Ctrl+Alt+J', N: 'Ctrl+Alt+N', Space: 'Ctrl+Alt+Space', H: 'Ctrl+Alt+H', P: 'Ctrl+Alt+P'}};
    window.calls = [];
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {unregisterListener: () => {}};
    window.__TAURI_INTERNALS__ = {metadata: {currentWindow: {label}, currentWebview: {windowLabel: label, label}}, convertFileSrc: p => p,
      transformCallback: cb => { callbacks[++next] = cb; return next; }, unregisterCallback: id => delete callbacks[id],
      invoke: async (c, a) => {
        window.calls.push({c, a});
        if (c === 'plugin:event|listen') { listeners[a.event] = a.handler; return a.handler; }
        if (c === 'get_state') return state;
        if (c === 'microphones') return report();
        if (c === 'set_microphone') { chosen = a.id; loud = false; return report(); }
        if (c === 'set_mic_volume') { mics.find(m => m.id === a.id).volume = a.volume; loud = false; return report(); }
        if (c === 'get_levels') return [loud ? .3 : .08, 0];
        if (c === 'get_settings') return settings;
        if (c === 'call_apps') return [];
        if (c === 'about') return {version: '1.0.0', licences: '', platform: 'macos'};
        if (c === 'notification_permission') return true;
        if (c === 'list_meetings') return [rec];
        if (c === 'deleted_meetings') return [];
        if (c === 'library_bytes') return 1;
        if (c === 'importing') return false;
        if (c === 'open_action_items') return [];
        if (c === 'get_meeting') return {summary: rec, notes_md: '', transcript: ''};
        if (c === 'meeting_media') return {tracks: [], duration_ms: 0, folder: '~'};
        if (c === 'live_transcript') return ['rec1', '[00:07] Them: Can you hear me?', ''];
        if (c === 'get_my_notes') return {meeting_id: 'rec1', text: '', version: 1};
        if (c === 'resume_remaining_ms') return 9000;
        return null;
      }};
  };
  for (const theme of ['light', 'dark']) {
    const r = {theme};
    const ref = await page.context().newPage();
    await ref.route('https://**/*', x => x.abort());
    await ref.emulateMedia({colorScheme: theme, reducedMotion: 'reduce'});
    await ref.setViewportSize({width: 1400, height: 1000});
    await ref.goto('http://127.0.0.1:3001/motetto-app-mockup.html');
    await ref.evaluate(t => document.documentElement.setAttribute('data-theme', t), theme);
    await ref.getByRole('button', {name: 'macOS', exact: true}).click();
    await ref.getByRole('button', {name: 'New recording', exact: true}).click();
    await ref.locator('.recbar .micchip').waitFor();

    // 1. The recording page: amber chip, the note under the bar, Record from, the switch.
    const p = await page.context().newPage();
    const errors = []; p.on('pageerror', e => errors.push(e.message));
    await p.emulateMedia({colorScheme: theme, reducedMotion: 'reduce'});
    await p.setViewportSize({width: 1048, height: 664});
    await p.addInitScript(fixture, {label: 'main', start: {type: 'Recording'}});
    await p.goto('http://127.0.0.1:3000/main/');
    // A recording opens on its own (owner 2026-10-20).
    const chip = p.locator('.recbar .micchip');
    await chip.waitFor();
    if ((await chip.getAttribute('class')).includes('warn')) throw Error('A Bluetooth mic was flagged');
    same(await styles(chip), await styles(ref.locator('.recbar .micchip')), 'Chip');
    const note = p.locator('.recnote');
    if (await note.count()) throw Error('A note shows without clipping');
    await p.screenshot({path: `.playwright-out/mic-app-recpage-${theme}.png`, clip: {x: 0, y: 0, width: 1048, height: 260}});
    await chip.click();
    const menu = p.getByRole('menu', {name: 'Microphone'});
    await menu.waitFor();
    await ref.locator('.recbar .micchip').click();
    same(await styles(menu), await styles(ref.locator('.micmenu')), 'Menu');
    const items = await menu.getByRole('menuitemradio').allInnerTexts();
    if (items.length !== 4 || !items[0].startsWith('System default (OpenRun Pro 2 by Shokz)')) throw Error(`Menu items: ${items}`);
    if (await menu.getByRole('menuitemradio', {checked: true}).count() !== 1) throw Error('One choice is not checked');
    if (!(await menu.innerText()).includes('OpenRun Pro 2 by Shokz sets its own level.')) throw Error('Bluetooth volume line missing');
    if (!items[1].includes('Bluetooth · 16 kHz') || !items[2].includes('Built-in · 48 kHz') || /quality/.test(items.join())) throw Error(`Menu labels: ${items}`);
    await p.screenshot({path: `.playwright-out/mic-app-recmenu-${theme}.png`, clip: {x: 448, y: 0, width: 600, height: 520}});
    await p.keyboard.press('ArrowDown');
    await p.keyboard.press('Escape');
    if (await menu.count()) throw Error('Escape left the menu open');
    await chip.click();
    await menu.waitFor();
    await menu.getByRole('menuitemradio', {name: /MacBook Pro Microphone/}).click();
    await p.getByText('Recording from MacBook Pro Microphone now. It\'s still one meeting.').waitFor();
    if (!await p.evaluate(() => window.calls.some(x => x.c === 'set_microphone' && x.a.id === 'builtin'))) throw Error('Switch not sent');
    // Too loud (measured): the one warning; its action lowers the chosen mic's volume by a third.
    await p.evaluate(() => window.fixtureLoud());
    await p.locator('.recnote').waitFor({timeout: 4000});
    if (await note.innerText() !== 'Your mic is too loud.\nLower input volume') throw Error(`Note: ${await note.innerText()}`);
    await ref.keyboard.press('Escape'); await ref.mouse.click(5, 5);
    await ref.evaluate(() => document.getElementById('simLoud').click());
    await ref.locator('#recnote:not([hidden])').waitFor();
    same(await styles(note), await styles(ref.locator('#recnote')), 'Recording-page note');
    await p.getByRole('button', {name: 'Lower input volume'}).click();
    await p.getByText(/Input volume lowered to 36% for MacBook Pro Microphone\./).waitFor();
    r.page = 'chip, note, menu, switch and lower volume passed';

    // 2. Settings › Audio.
    const st = await page.context().newPage();
    st.on('pageerror', e => errors.push(e.message));
    await st.emulateMedia({colorScheme: theme, reducedMotion: 'reduce'});
    await st.setViewportSize({width: 900, height: 640});
    await st.addInitScript(fixture, {label: 'settings', start: {type: 'Idle'}});
    await st.goto('http://127.0.0.1:3000/settings/?tab=audio');
    await st.getByRole('tab', {name: 'Audio', selected: true}).waitFor();
    await st.getByRole('combobox', {name: 'Microphone'}).waitFor();
    if (await st.getByRole('combobox', {name: 'Microphone'}).inputValue() !== '') throw Error('Settings is not on System default');
    if (await st.locator('.setnote').count()) throw Error('Settings warns about the chosen mic');
    const options = await st.locator('#mic-select option').allInnerTexts();
    if (!options.includes('OpenRun Pro 2 by Shokz · Bluetooth · 16 kHz') || /quality/.test(options.join())) throw Error(`Options: ${options}`);
    if (await st.getByRole('slider').count()) throw Error('A Bluetooth mic shows a volume slider');
    await st.screenshot({path: `.playwright-out/mic-app-settings-${theme}.png`});
    await st.getByRole('combobox', {name: 'Microphone'}).selectOption('builtin');
    await st.getByRole('slider', {name: 'Input volume for MacBook Pro Microphone'}).waitFor();
    if (await st.locator('.volrow output').innerText() !== '53%') throw Error('Volume not shown');
    r.settings = 'Audio tab, choice and volume passed';

    // 3. The companion Pill: "You" opens Record from; clipping takes the caption's line.
    const c = await page.context().newPage();
    c.on('pageerror', e => errors.push(e.message));
    await c.emulateMedia({colorScheme: theme, reducedMotion: 'reduce'});
    await c.setViewportSize({width: 460, height: 72});
    await c.addInitScript(fixture, {label: 'companion', start: {type: 'Recording'}});
    await c.goto('http://127.0.0.1:3000/companion/');
    await c.locator('.caption').waitFor();
    if (await c.locator('.pill .micnote').count() || (await c.locator('.duet .micbtn').getAttribute('class')).includes('warn')) throw Error('The Pill warns about a Bluetooth mic');
    await c.evaluate(() => window.fixtureLoud());
    await c.locator('.pill .micnote').waitFor({timeout: 4000});
    if (await c.locator('.caption').count()) throw Error('Caption shows under the mic note');
    const pillNote = await c.locator('.pill .micnote').boundingBox();
    if (Math.round(pillNote.height) !== 26) throw Error(`Pill note is ${pillNote.height}px, not the caption's 26`);
    if (!(await c.locator('.duet .micbtn').getAttribute('class')).includes('warn')) throw Error('You is not amber');
    await c.screenshot({path: `.playwright-out/mic-app-pill-${theme}.png`});
    await c.setViewportSize({width: 460, height: 460});
    await c.locator('.duet .micbtn').click();
    await c.getByRole('menu', {name: 'Microphone'}).waitFor();
    await c.waitForFunction(() => window.calls.some(x => x.c === 'companion_menu' && x.a.open && x.a.extra > 200));
    await c.screenshot({path: `.playwright-out/mic-app-pillmenu-${theme}.png`});
    await c.keyboard.press('Escape');
    await c.waitForFunction(() => window.calls.at(-1)?.c === 'companion_menu' ? !window.calls.at(-1).a.open : window.calls.some(x => x.c === 'companion_menu' && !x.a.open));
    // A click outside closes it: in the space the window grew for the menu, and in another app.
    const pillMenu = c.getByRole('menu', {name: 'Microphone'});
    await c.locator('.duet .micbtn').click();
    await pillMenu.waitFor();
    await c.mouse.click(3, 450);
    await pillMenu.waitFor({state: 'hidden', timeout: 2000});
    await c.locator('.duet .micbtn').click();
    await pillMenu.waitFor();
    await c.waitForFunction(() => window.calls.some(x => x.c === 'plugin:event|listen' && x.a.event === 'tauri://blur'));
    await c.evaluate(() => window.fixtureEmit('tauri://blur'));
    await pillMenu.waitFor({state: 'hidden', timeout: 2000});
    // You still toggles it, and a click inside the menu keeps it open.
    await c.locator('.duet .micbtn').click();
    await pillMenu.waitFor();
    await pillMenu.getByText('Record from').click();
    if (!await pillMenu.isVisible()) throw Error('A click inside the menu closed it');
    await c.locator('.duet .micbtn').click();
    await pillMenu.waitFor({state: 'hidden', timeout: 2000});
    await c.locator('.duet .micbtn').click();
    await c.getByRole('menu', {name: 'Microphone'}).getByRole('menuitemradio', {name: /MacBook Pro Microphone/}).click();
    await c.locator('.caption').waitFor();
    r.pill = 'note in the caption line, You menu, window size and switch passed';

    // 4. The Ask card says nothing about the mic.
    const a = await page.context().newPage();
    a.on('pageerror', e => errors.push(e.message));
    await a.emulateMedia({colorScheme: theme, reducedMotion: 'reduce'});
    await a.setViewportSize({width: 480, height: 170});
    await a.addInitScript(fixture, {label: 'companion', start: {type: 'Asking', app: 'Zoom', call_id: 'c1'}});
    await a.goto('http://127.0.0.1:3000/companion/');
    await a.getByText('Record this Zoom call?').waitFor();
    await a.waitForTimeout(500);
    if (await a.locator('.askcard .micline').count() || await a.evaluate(() => window.calls.some(x => x.c === 'microphones'))) throw Error('The Ask card looks at the mic');
    r.ask = 'no mic line passed';

    const overflow = await p.evaluate(() => document.documentElement.scrollWidth > innerWidth);
    if (overflow || errors.length) throw Error(JSON.stringify({overflow, errors}));
    out.push(r);
    for (const x of [ref, p, st, c, a]) await x.close();
  }
  return out;
}
