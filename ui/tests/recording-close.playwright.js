// Run as a Playwright page function. The recording page after the owner's 2026-11-05 report:
// the default mic is listed once (as System default), the mic menu closes on a click outside, when
// the window loses focus and on Stop, and Discard does not show "No such file or directory" while
// the meeting moves to Recently deleted. IPC is mocked in this browser fixture only.
async (page) => {
  const out = {};
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
    window.fixtureDefault = id => { mics.forEach(m => m.is_default = m.id === id); chosen = id; };
    window.fixtureState = t => { state = {...state, state: {type: t}}; };
    let next = 0; const callbacks = {}, listeners = {};
    window.fixtureEmit = (event, payload) => { if (!listeners[event]) throw Error(`No listener for ${event}`); for (const h of listeners[event]) callbacks[h]?.({event, id: 0, payload}); };
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
        if (c === 'plugin:event|listen') { (listeners[a.event] ||= []).push(a.handler); return a.handler; }
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
        if (c === 'get_meeting' && window.gone) throw 'gone';
        if (c === 'get_meeting') return {summary: rec, notes_md: '', transcript: ''};
        if (c === 'meeting_media') { if (window.gone) throw 'No such file or directory (os error 2)'; return {tracks: [], videos: [], duration_ms: 0, folder: '~'}; }
                if (c === 'live_transcript') return ['rec1', '[00:07] Them: Can you hear me?', ''];
        if (c === 'get_my_notes') return {meeting_id: 'rec1', text: '', version: 1};
        if (c === 'resume_remaining_ms') return 9000;
        if (['scripts_list','list_screen_sources','notes_translations'].includes(c)) return [];
        return null;
      }};
  };
  const p = await page.context().newPage();
  const errors = []; p.on('pageerror', e => errors.push(e.message));
  await p.setViewportSize({width: 1048, height: 664});
  await p.addInitScript(fixture, {label: 'main', start: {type: 'Recording'}});
  await p.goto('http://127.0.0.1:3000/main/');
  const chip = p.locator('.micchip[aria-label^="Microphone"]'); await chip.waitFor();
  const menu = p.getByRole('menu', {name: 'Microphone'});
  // #1: the owner's case, built-in is the default and was also picked explicitly.
  await p.evaluate(() => window.fixtureDefault('builtin'));
  await p.waitForTimeout(2300);
  await chip.click(); await menu.waitFor();
  const items = await menu.getByRole('menuitemradio').allInnerTexts();
  const checked = await menu.getByRole('menuitemradio', {checked: true}).allInnerTexts();
  out.items = items; out.checked = checked;
  // None comes first during a recording (owner 2026-11-05).
  if (items.length !== 4 || !items[0].startsWith('None') || items.filter(x => x.includes('MacBook Pro Microphone')).length !== 1 || !checked[0]?.startsWith('System default')) throw Error('#1 ' + JSON.stringify(out));
  await p.screenshot({path: '.playwright-out/recording-close-menu.png'});
  // #2: window blur, click outside, and Stop all close it.
  await p.evaluate(() => window.dispatchEvent(new Event('blur')));
  await menu.waitFor({state: 'detached', timeout: 1500});
  await chip.click(); await menu.waitFor();
  await p.mouse.click(600, 500); await menu.waitFor({state: 'detached', timeout: 1500});
  await chip.click(); await menu.waitFor();
  const ev = (type, id) => p.evaluate(([type, id]) => window.fixtureEmit('motetto://event', {type: 'StateChanged', state: {type}, meeting_id: id, app: 'Zoom', elapsed_ms: 74000, started_by: {type: 'App', app: 'Zoom'}, auto_start: false, problem: null, more_problems: 0, quit_pending: false, dnd_active: false, onboarding_done: true, sharing: null, never_pending: null, discarded: null, companion: {visible: true, size: 'Pill', pinned: false, peeking: false, display_id: ''}}), [type, id]);
  await ev('Stopped', 'rec1');
  await menu.waitFor({state: 'detached', timeout: 1500});
  // #3: Discard idles the session while the folder moves; the page must not show an error.
  await p.evaluate(() => { window.gone = true; });
  await ev('Idle', null);
  await p.waitForTimeout(1500);
  out.alert = await p.getByRole('alert').allInnerTexts();
  if (out.alert.some(t => /No such file/.test(t))) throw Error('#3 ' + JSON.stringify(out));
  if (errors.length) throw Error(errors.join('; '));
  return 'default listed once, blur/outside/Stop close the menu, no error after Discard: passed';
}
