// Run as a Playwright page function. A screen recording open in the library while it records:
// Stop, the resume window ending, then the save marking it Ready must load its video, not leave
// "Loading video" on screen. IPC is mocked in this browser fixture only: nothing records.
async (page) => {
  const fixture = () => {
    Object.defineProperty(navigator, 'platform', {value: 'MacIntel'});
    const facts = {source: 'Built-in Retina Display', what: 'display', mic_off: true, app_sounds: true, clicks: true, fps: 15, width: 1670, height: 1080};
    window.clip = {id: 'clip1', title: 'Screen recording at 15:32', created_ms: Date.now(), status: 'Recording', has_video: true, marks_ms: [], duration_ms: null, notes_ms: null, spellings: [], locked: false, title_source: 'placeholder', language: null, notes_language: null, apps: [], unconfirmed: [], reach_ms: null, kind: 'screen', facts, deleted_kind: 'meeting'};
    const setup = {mic: {kind: 'Off'}, app_sounds: true, notes: false, clicks: true, screen: {choice: {kind: 'Display', id: 1}, name: 'Built-in Retina Display'}};
    window.st = {state: {type: 'Recording'}, meeting_id: 'clip1', app: null, elapsed_ms: 5000, started_by: {type: 'Manual'}, ask_on_call: true, problem: null, more_problems: 0, quit_pending: false, dnd_active: false, onboarding_done: true, sharing: null, share_visible: true, screen: {pick: setup.screen, while_sharing: null, waiting: false}, share_video_offer: false, companion: {visible: true, size: 'Pill', pinned: false, peeking: false, display_id: ''}, ask_off_pending: null, stops_at_ms: null, discarded: null, setup, screen_closed: null, held_clip: null};
    const media = {tracks: [], videos: [{path: 'http://127.0.0.1:3000/missing-clip.mp4', offset_ms: 0, duration_ms: 62000, source: 'Built-in Retina Display', kind: 'display', while_sharing: false, width: 1670, height: 1080, bytes: 2244508}], moments: [], clicks: [], duration_ms: 62000, folder: '~/Motetto/clip1'};
    window.calls = []; let next = 0; const callbacks = {}, listeners = {};
    window.emit = (event, payload) => { for (const h of listeners[event] || []) callbacks[h]?.({event, id: 0, payload}); };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {unregisterListener: () => {}};
    window.__TAURI_INTERNALS__ = {metadata: {currentWindow: {label: 'main'}, currentWebview: {windowLabel: 'main', label: 'main'}}, convertFileSrc: p => p,
      transformCallback: cb => { callbacks[++next] = cb; return next; }, unregisterCallback: id => delete callbacks[id],
      invoke: async (c, a) => {
        window.calls.push({c, a});
        if (c === 'plugin:event|listen') { (listeners[a.event] ||= []).push(a.handler); return a.handler; }
        if (c === 'get_state') return window.st;
        if (c === 'get_settings') return {companion: {captions: true}, shortcuts: {}, keep_audio: '30_days', keep_video: '30_days', screen_preset: 'sharp'};
        if (c === 'list_meetings') return [window.clip];
        if (c === 'deleted_meetings') return [];
        if (c === 'get_meeting') return {summary: {...window.clip}, notes_md: '', transcript: '', transcript_heard: null};
        if (c === 'meeting_media') return media;
        if (c === 'library_bytes') return 1;
        if (c === 'open_action_items' || c === 'scripts_list' || c === 'notes_translations' || c === 'call_apps' || c === 'list_screen_sources') return [];
        if (c === 'microphones') return {mics: [], chosen: null, active: null, issue: null};
        if (c === 'get_levels') return [0, 0];
        if (c === 'live_transcript') return ['clip1', '', ''];
        return null;
      }};
  };
  const p = await page.context().newPage();
  await p.route('http://127.0.0.1:3000/missing-clip.mp4', r => r.fulfill({status: 404}));
  await p.addInitScript(fixture);
  await p.goto('http://127.0.0.1:3000/main/');
  await p.waitForFunction(() => window.calls.some(x => x.c === 'get_meeting'));
  await p.waitForTimeout(500);
  const ev = (state, id) => p.evaluate(([state, id]) => { window.st = {...window.st, state, meeting_id: id}; window.emit('motetto://event', {type: 'StateChanged', ...window.st}); }, [state, id]);
  // Stop finishes the files: the length is read from them before the clip is saved.
  await p.evaluate(() => { window.clip = {...window.clip, duration_ms: 62200}; });
  await ev({type: 'Stopped', resume_until_ms: Date.now() + 10000}, 'clip1'); await p.waitForTimeout(300);
  await ev({type: 'Idle'}, null); await p.waitForTimeout(300);
  await p.evaluate(() => { window.clip = {...window.clip, status: 'Ready'}; window.emit('motetto://library-changed', null); });
  await p.waitForTimeout(1200);
  const loading = await p.getByText('Loading video').count();
  const media = await p.evaluate(() => window.calls.filter(x => x.c === 'meeting_media').length);
  await p.close();
  if (loading) throw Error(`stuck on Loading video (meeting_media calls: ${media})`);
  return {media};
}
