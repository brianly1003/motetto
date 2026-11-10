// Run as a Playwright page function. Microphone modes against the mockup, light and dark:
// Settings › Audio's mode list, the Audio tab's Cleaned / Original switch (same position, still
// playing), Transcribe again's Microphone choice and the screen setup bar's mic menu. IPC is mocked in this browser fixture only;
// real capture, denoising and native playback have Rust tests or need a live call.
async (page) => {
  const APP = 'http://127.0.0.1:3000', MOCK = 'http://127.0.0.1:3001/motetto-app-mockup.html';
  const keys = ['display', 'gridTemplateColumns', 'gap', 'padding', 'fontSize', 'fontWeight', 'color', 'backgroundColor', 'borderTopColor', 'borderTopWidth', 'borderRadius', 'maxWidth', 'outlineStyle', 'boxShadow'];
  const styles = (loc) => loc.first().evaluate((e, k) => Object.fromEntries(k.map(x => [x, getComputedStyle(e)[x]])), keys);
  const compare = async (a, b, what, out) => { const [x, y] = [await styles(a), await styles(b)]; for (const k of keys) if (x[k] !== y[k]) out.push(`${what} ${k}: ${x[k]} != ${y[k]}`); };
  const fixture = () => {
    Object.defineProperty(navigator, 'platform', {value: 'MacIntel'});
    const settings = {version: 1, notes_folder: '~/Documents/Motetto', open_at_login: true, notes_language: 'same_as_call', ask_on_call: true, app_modes: {}, apps: [], idle_stop_s: 30, speech_model: '', notes_model: '', call_language: 'detect', part_chars: 24000, keep_audio: '30_days', recording_reminder: true, mic_device: null, mic_mode: 'standard', sounds: true, companion: {default_size: 'pill', captions: true}, spellings: [], shortcuts: {S: 'Ctrl+Alt+S', M: 'Ctrl+Alt+M', J: 'Ctrl+Alt+J', N: 'Ctrl+Alt+N', Space: 'Ctrl+Alt+Space', H: 'Ctrl+Alt+H', P: 'Ctrl+Alt+P'}};
    const mic = {id: 'builtin', name: 'MacBook Pro Microphone', transport: 'BuiltIn', rate_hz: 48000, volume: .53, is_default: true};
    const summary = {id: '123', title: 'FCM vs Pusher Beams', created_ms: Date.now(), status: 'Ready', marks_ms: [], duration_ms: 87000, notes_ms: 48000, spellings: [], mic_mode: 'reduce_noise'};
    const meeting = {summary, notes_md: '### Overview\n- Push beams [00:10]', transcript: '[00:10] Me: Hello\n[00:20] Them: Hi', transcript_heard: ''};
    const track = (speaker, original) => ({path: `/fixture-${speaker}${original ? '-raw' : ''}.wav`, name: speaker === 'You' ? (original ? 'part-0000.mic.wav' : 'part-0000.mic.clean.wav') : 'part-0000.system.wav', speaker, offset_ms: 0, duration_ms: 87000, bytes: 2100000, waveform: Array(200).fill(.3)});
    let next = 0; window.calls = []; window.position = 0;
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {unregisterListener: () => {}};
    window.__TAURI_INTERNALS__ = {convertFileSrc: p => p, transformCallback: () => ++next, unregisterCallback: () => {}, invoke: async (c, a) => {
      window.calls.push({c, a});
      if (c === 'plugin:event|listen') return next;
      if (c === 'get_settings') return structuredClone(settings);
      if (c === 'save_settings') { Object.assign(settings, a.settings); return structuredClone(settings); }
      if (c === 'microphones') return {mics: [mic], chosen: null, active: mic, issue: null};
      if (c === 'get_levels') return [0, 0];
      if (c === 'call_apps') return [];
      if (c === 'about') return {version: '1.0.0', licences: '', platform: 'macos'};
      if (c === 'notification_permission') return true;
      if (c === 'get_state') return {state: {type: 'Idle'}, meeting_id: null, companion: {visible: false, size: 'Pill'}, elapsed_ms: 0, quit_pending: false};
      if (c === 'list_meetings') return [summary];
      if (c === 'deleted_meetings') return [];
      if (c === 'get_meeting') return meeting;
      if (c === 'meeting_media') return {tracks: [track('You', a.original), track('Them')], videos: [], moments: [], clicks: [], duration_ms: 87000, folder: '~', mic_mode: summary.mic_mode, mic_cleaned: true};
      if (c === 'playback_open') return true;
      if (c === 'playback_set') { if (a.atMs != null) window.position = a.atMs; return window.position; }
      if (c === 'playback_position') { window.position += 50; return window.position; }
      if (c === 'meeting_action') { if (a.micMode) summary.mic_mode = a.micMode; return; }
      return null;
    }};
  };
  const results = [];
  for (const theme of ['light', 'dark']) {
    const fails = [], errors = [];
    const ref = await page.context().newPage();
    await ref.route('https://**/*', x => x.abort());
    await ref.emulateMedia({colorScheme: theme, reducedMotion: 'reduce'});
    await ref.setViewportSize({width: 1400, height: 1000});
    await ref.goto(MOCK);
    await ref.evaluate(t => document.documentElement.setAttribute('data-theme', t), theme);
    await ref.getByRole('button', {name: 'macOS', exact: true}).click();

    // 1. Settings › Audio: three modes, Standard chosen, saved on change, no focus ring.
    await ref.locator('[data-go="settings"]').first().click();
    await ref.locator('#settabs').getByRole('tab', {name: 'Audio'}).click();
    const st = await page.context().newPage();
    st.on('pageerror', e => errors.push(e.message));
    await st.emulateMedia({colorScheme: theme, reducedMotion: 'reduce'});
    await st.setViewportSize({width: 900, height: 760});
    await st.addInitScript(fixture);
    await st.goto(`${APP}/settings/?tab=audio`);
    await st.locator('.micmodes').waitFor();
    const names = await st.locator('.micmode b').allInnerTexts();
    if (names.join('|') !== (await ref.locator('#setpane .micmode b').allInnerTexts()).join('|')) fails.push(`modes: ${names}`);
    if (await st.locator('.micmode small').allInnerTexts().then(x => x.join('|')) !== await ref.locator('#setpane .micmode small').allInnerTexts().then(x => x.join('|'))) fails.push('mode descriptions differ');
    if (!await st.getByRole('radio', {name: /^Standard/}).isChecked()) fails.push('Standard not chosen');
    const help = st.locator('.formrow:has(.micmodes) .help');
    if (await help.innerText() !== await ref.locator('#setpane .formrow:has(.micmodes) .help').innerText()) fails.push('help differs');
    for (const [a, b] of [['.micmodes', '.micmodes'], ['.micmode', '.micmode'], ['.micmode + .micmode', '.micmode + .micmode'], ['.micmode b', '.micmode b'], ['.micmode small', '.micmode small']]) await compare(st.locator(a), ref.locator('#setpane ' + b), a, fails);
    await st.getByRole('radio', {name: /^Reduce noise/}).click();
    await st.waitForFunction(() => window.calls.some(x => x.c === 'save_settings' && x.a.settings.mic_mode === 'reduce_noise'));
    const ring = await st.getByRole('radio', {name: /^Reduce noise/}).evaluate(e => { const s = getComputedStyle(e), l = getComputedStyle(e.closest('label')); return s.outlineStyle !== 'none' && s.outlineWidth !== '0px' || /rgb/.test(l.boxShadow) || l.outlineStyle !== 'none' && l.outlineWidth !== '0px'; });
    if (ring) fails.push('focus ring on a mode');
    await st.locator('.formrow:has(.micmodes)').screenshot({path: `.playwright-out/mic-modes-settings-${theme}.png`});
    await ref.locator('#setpane .formrow:has(.micmodes)').screenshot({path: `.playwright-out/mic-modes-settings-ref-${theme}.png`});
    await st.close();

    // 2. Audio tab: the mode, then Original keeps the position and keeps playing.
    await ref.getByRole('button', {name: 'After the call', exact: true}).click();
    await ref.locator('#doc').getByRole('tab', {name: 'Audio', exact: true}).click();
    const p = await page.context().newPage();
    p.on('pageerror', e => errors.push(e.message));
    await p.emulateMedia({colorScheme: theme, reducedMotion: 'reduce'});
    await p.setViewportSize({width: 1046, height: 662});
    await p.addInitScript(fixture);
    await p.goto(`${APP}/main/`);
    await p.getByRole('button', {name: /FCM vs Pusher Beams/}).first().click();
    await p.getByRole('tab', {name: 'Audio', exact: true}).click();
    await p.locator('.micplay').waitFor();
    if (await p.locator('.micplay > span').innerText() !== await ref.locator('#doc .micplay > span').innerText()) fails.push('mode line differs');
    if (await p.locator('.micplay + .tip').innerText() !== await ref.locator('#doc .micplay + .tip').innerText()) fails.push('switch help differs');
    for (const [a, b] of [['.micplay', '.micplay'], ['.micplay .segm', '.micplay .segm'], ['.micplay .segm button[aria-checked="true"]', '.micplay .segm button[aria-checked="true"]'], ['.micplay .segm button[aria-checked="false"]', '.micplay .segm button[aria-checked="false"]'], ['.micplay + .tip', '.micplay + .tip']]) await compare(p.locator(a), ref.locator('#doc ' + b), a, fails);
    await p.locator('.micplay').screenshot({path: `.playwright-out/mic-modes-audio-${theme}.png`});
    await ref.locator('#doc .micplay').screenshot({path: `.playwright-out/mic-modes-audio-ref-${theme}.png`});
    await p.getByRole('button', {name: 'part-0000.mic.clean.wav'}).click();
    await p.waitForFunction(() => window.position > 300);
    await p.getByRole('radio', {name: 'Original'}).click();
    await p.waitForFunction(() => window.calls.some(x => x.c === 'playback_open' && x.a.original));
    await p.waitForFunction(() => window.calls.some(x => x.c === 'meeting_media' && x.a.original));
    const before = await p.evaluate(() => window.position);
    await p.waitForTimeout(400);
    const opened = await p.evaluate(() => window.calls.findLastIndex(x => x.c === 'playback_open'));
    const resumed = await p.evaluate(i => window.calls.slice(i).find(x => x.c === 'playback_set' && x.a.playing), opened);
    if (!resumed || Math.abs(resumed.a.atMs - before) > 400) fails.push(`did not resume at the same moment: ${JSON.stringify(resumed)} vs ${before}`);
    if (!await p.getByRole('button', {name: '❚❚ Pause'}).count()) fails.push('stopped playing on switch');
    if (await p.getByRole('radio', {name: 'Original'}).getAttribute('aria-checked') !== 'true') fails.push('Original not shown as chosen');
    if (!await p.getByRole('button', {name: 'part-0000.mic.wav'}).count()) fails.push('track list does not show the recorded mic');
    await p.getByRole('button', {name: '❚❚ Pause'}).click();

    // 3. Transcribe again: Microphone, preselected with the meeting's mode, sent with the job.
    await ref.locator('#doc [data-dd="more"]').click();
    await ref.locator('[data-retx]').first().click();
    await ref.locator('#rxMic').waitFor();
    await ref.locator('.confirmbar').screenshot({path: `.playwright-out/mic-modes-rerun-ref-${theme}.png`});
    await p.bringToFront();
    await p.getByRole('button', {name: 'More actions'}).click();
    await p.getByRole('menuitem', {name: 'Transcribe again'}).click();
    const pick = p.getByRole('alertdialog').getByRole('combobox', {name: 'Microphone mode'});
    if (await pick.inputValue() !== 'reduce_noise') fails.push('rerun not preselected with the meeting mode');
    // The field is Write the notes again's: the app caps the whole question at 380 px instead of
    // the field's own 360 px max-width, as it already does for that field.
    await compare(p.locator('.doc-confirm .writein'), ref.locator('.confirmbar .writein'), 'rerun field', fails);
    if (fails.at(-1)?.startsWith('rerun field maxWidth')) fails.pop();
    if (await p.locator('.doc-confirm .writein-help').innerText() !== await ref.locator('.confirmbar .writein-help').innerText()) fails.push('rerun help differs');
    await pick.selectOption('original');
    await p.locator('.doc-confirm').screenshot({path: `.playwright-out/mic-modes-rerun-${theme}.png`});
    await p.getByRole('alertdialog').getByRole('button', {name: 'Confirm'}).click();
    await p.waitForFunction(() => window.calls.some(x => x.c === 'meeting_action' && x.a.action === 'retranscribe' && x.a.micMode === 'original'));
    await p.close();

    // 4. The setup bar's mic menu: one compact row and one line, hidden with no mic, saved at once.
    const bar = await page.context().newPage();
    bar.on('pageerror', e => errors.push(e.message));
    await bar.emulateMedia({colorScheme: theme, reducedMotion: 'reduce'});
    await bar.setViewportSize({width: 1400, height: 1000});
    await bar.addInitScript(() => {
      Object.defineProperty(navigator, 'platform', {value: 'MacIntel'});
      const mics = [{id: 'builtin', name: 'MacBook Pro Microphone', transport: 'BuiltIn', rate_hz: 48000, volume: .5, is_default: true}];
      let next = 0; window.calls = [];
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {unregisterListener: () => {}};
      window.__TAURI_INTERNALS__ = {metadata: {currentWindow: {label: 'capture-1'}, currentWebview: {windowLabel: 'capture-1', label: 'capture-1'}}, convertFileSrc: p => p, transformCallback: () => ++next, unregisterCallback: () => {}, invoke: async (c, a) => {
        window.calls.push({c, a});
        if (c === 'plugin:event|listen') return next;
        if (c === 'get_state') return {state: {type: 'Idle'}, meeting_id: null, app: null, elapsed_ms: 0};
        if (c === 'capture_memory') return {mode: 'audio', mic: {kind: 'Default'}, app_sounds: true, notes: true, timer: 0, clicks: true, area: null, remember: true};
        if (c === 'get_settings') return {screen_preset: 'sharp', mic_mode: 'standard'};
        if (c === 'capture_windows') return [];
        if (c === 'microphones') return {mics, chosen: null, active: mics[0], issue: null};
        if (c === 'capture_level') return .1;
        if (c === 'list_screen_sources') return [];
        return null;
      }};
    });
    await bar.goto(`${APP}/capture/?display=1&kind=Call&dock=0`);
    await bar.locator('#capbar').waitFor();
    await ref.close();
    const cref = await page.context().newPage();
    await cref.route('https://**/*', x => x.abort());
    await cref.setViewportSize({width: 1400, height: 1000});
    await cref.goto(MOCK);
    await cref.evaluate(t => document.documentElement.setAttribute('data-theme', t), theme);
    await cref.getByRole('button', {name: 'macOS', exact: true}).click();
    await cref.evaluate(() => document.querySelector('[data-act="capture"]').click());
    await cref.locator('#capbar [data-cap="mic"]').click();
    await cref.locator('.capmenu [data-capset="mic:"]').click();
    await cref.locator('#capbar [data-cap="mic"]').click();
    await bar.locator('[data-cap="mic"]').click();
    const seg = bar.getByRole('group', {name: 'Microphone mode'});
    await seg.waitFor();
    if ((await seg.getByRole('menuitemradio').allInnerTexts()).join('|') !== 'Standard|Reduce noise|Original') fails.push('setup bar modes');
    if (await seg.getByRole('menuitemradio', {name: 'Standard'}).getAttribute('aria-checked') !== 'true') fails.push('setup bar mode not read from Settings');
    for (const sel of ['.capseg', '.capseg button[aria-checked="true"]', '.capseg button[aria-checked="false"]', '.capseg + .note', '.mh']) await compare(bar.locator('.capmenu ' + sel), cref.locator('.capmenu ' + sel), 'bar ' + sel, fails);
    for (const id of ['reduce_noise', 'original', 'standard']) {
      await bar.locator(`[data-capset="micmode:${id}"]`).click();
      await cref.locator(`.capmenu [data-capset="micmode:${id}"]`).click();
      if (!await bar.evaluate(m => window.calls.some(x => x.c === 'set_mic_mode' && x.a.mode === m), id)) fails.push(`set_mic_mode ${id} not sent`);
      const [line, wraps] = await bar.locator('.capmenu .capseg + .note').evaluate(e => [e.textContent, e.scrollWidth > e.clientWidth || e.getBoundingClientRect().height > 24]);
      if (line !== await cref.locator('.capmenu .capseg + .note').textContent() || wraps) fails.push(`setup bar line for ${id}: ${line}`);
      if (!await bar.locator('.capmenu').count()) fails.push('choosing a mode closed the menu');
    }
    await bar.locator(`[data-capset="micmode:reduce_noise"]`).click();
    await bar.mouse.move(5, 5);
    await bar.locator('.capmenu').screenshot({path: `.playwright-out/mic-modes-bar-${theme}.png`});
    await bar.locator('.capmenu [data-capset="mic:off"]').click();
    await bar.locator('[data-cap="mic"]').click();
    await bar.locator('.capmenu').waitFor();
    if (await bar.locator('.capseg').count()) fails.push('mode row shows with no mic');
    await bar.close(); await cref.close();
    if (fails.length || errors.length) throw Error(JSON.stringify({theme, fails, errors}));
    results.push({theme, passed: 'settings, audio switch, rerun, setup bar'});
  }
  return results;
}
