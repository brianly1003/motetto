// Run as a Playwright page function. A screen recording kept as just the video, against the
// mockup (clipHtml, trimHtml, .pillvo): its page while it records, the saved page (facts, click
// dots, the notes card), Trim (handles, keys, the reach-for-Stop chip, Return, Undo, Esc), the
// library row and the trimmed original in Recently deleted. IPC is mocked in this browser fixture
// only: no capture, no export and no real trim run. The picture is a generated test video, served
// from .playwright-out/clip-fixture.webm when present (ffmpeg testsrc2, 1280x800, 44 s).
async (page) => {
  const out = [];
  const keys = ['fontSize', 'fontWeight', 'lineHeight', 'borderRadius', 'backgroundColor', 'color', 'paddingTop', 'paddingLeft', 'paddingRight', 'gap', 'borderTopWidth', 'borderTopColor', 'borderTopStyle', 'height', 'boxShadow', 'backgroundImage'];
  const styles = (loc, skip = []) => loc.first().evaluate((e, k) => Object.fromEntries(k.map(x => [x, getComputedStyle(e)[x]])), keys.filter(k => !skip.includes(k)));
  const diffs = [];
  // The app's CSS build writes a gradient's `0%` position as `0px`: the same place.
  const norm = (v) => v.replace(/ 0px([,)])/g, ' 0%$1');
  const same = async (app, ref, what, skip = []) => { const a = await styles(app, skip), b = await styles(ref, skip); for (const k in a) if (norm(a[k]) !== norm(b[k])) diffs.push(`${what} ${k}: ${a[k]} != ${b[k]}`); };
  const assert = (ok, message) => { if (!ok) throw Error(message); };
  const fixture = ({label, live, silent}) => {
    Object.defineProperty(navigator, 'platform', {value: 'MacIntel'});
    const now = Date.now();
    const facts = {source: 'console.example.app/reports window', what: 'window', mic_off: true, app_sounds: !silent, clicks: true, fps: 15, width: 1280, height: 800};
    let clip = {id: 'clip1', title: 'Screen recording at 10:02', created_ms: now - 60000, status: live ? 'Recording' : 'Ready', has_video: !live, marks_ms: [], duration_ms: live ? null : 44000, notes_ms: null, spellings: [], locked: false, title_source: 'placeholder', language: null, notes_language: null, apps: [], unconfirmed: [], reach_ms: live ? null : 1600, kind: 'screen', facts, deleted_kind: 'meeting'};
    const full = {...clip, id: 'clip1-full', deleted_kind: 'trimmed_original', status: 'Ready'};
    const setup = {mic: {kind: 'Off'}, app_sounds: !silent, notes: false, clicks: true, screen: {choice: {type: 'Window', window_id: 7, app: 'Safari'}, name: facts.source}};
    const state = {state: live ? {type: 'Recording'} : {type: 'Idle'}, meeting_id: live ? 'clip1' : null, app: null, elapsed_ms: 74000, started_by: {type: 'Manual'}, ask_on_call: true, problem: null, more_problems: 0, quit_pending: false, dnd_active: false, onboarding_done: true, sharing: null, share_visible: true, screen: null, share_video_offer: false, companion: {visible: true, size: 'Pill', pinned: false, peeking: false, display_id: ''}, ask_off_pending: null, stops_at_ms: null, discarded: null, setup: live ? setup : null, screen_closed: null};
    const media = () => ({tracks: [], videos: [{path: 'http://127.0.0.1:3000/clip-fixture.webm', offset_ms: 0, duration_ms: clip.duration_ms, source: facts.source, kind: 'window', while_sharing: false, width: 1280, height: 800, bytes: 1147123}], moments: [], clicks: [5000, 12000, 30000], duration_ms: clip.duration_ms, folder: '~/Motetto/clip1'});
    const settings = {companion: {captions: true}, shortcuts: {N: 'Ctrl+Alt+N'}, keep_audio: '30_days', keep_video: '30_days'};
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
        if (c === 'deleted_meetings') return [full];
        if (c === 'get_meeting') return {summary: clip, notes_md: '', transcript: '', transcript_heard: null};
        if (c === 'meeting_media') return media();
        if (c === 'trim_video') { clip = {...clip, duration_ms: a.toMs - a.fromMs, reach_ms: null}; return clip; }
        if (c === 'restore_trim') { clip = {...clip, duration_ms: 44000, reach_ms: 1600}; return clip; }
        if (c === 'library_bytes') return 1;
        if (c === 'importing') return null;
        if (c === 'open_action_items' || c === 'scripts_list' || c === 'list_screen_sources' || c === 'notes_translations' || c === 'call_apps') return [];
        if (c === 'microphones') return {mics: [], chosen: null, active: null, issue: null};
        if (c === 'get_levels') return [0, 0];
        if (c === 'live_transcript') return ['clip1', '', ''];
        if (c === 'resume_remaining_ms') return 9000;
        if (c === 'notification_permission') return true;
        if (c === 'playback_open') return false;
        return null;
      }};
  };
  const video = async (p) => p.route('http://127.0.0.1:3000/clip-fixture.webm', r => r.fulfill({path: '.playwright-out/clip-fixture.webm', contentType: 'video/webm'}).catch(() => r.fulfill({status: 404})));
  for (const theme of ['light', 'dark']) {
    const r = {theme};
    // The mockup: Record the screen only, a window, Record, then Stop after 7 s (the reach chip needs > 6 s).
    const ref = await page.context().newPage();
    await ref.route('https://**/*', x => x.abort());
    await ref.emulateMedia({colorScheme: theme, reducedMotion: 'reduce'});
    await ref.setViewportSize({width: 1400, height: 1000});
    await ref.goto('http://127.0.0.1:3001/motetto-app-mockup.html');
    await ref.evaluate(t => document.documentElement.setAttribute('data-theme', t), theme);
    await ref.getByRole('button', {name: 'macOS', exact: true}).click();
    await ref.getByRole('button', {name: 'During a call', exact: true}).click();
    await ref.getByRole('button', {name: 'Record the screen only', exact: true}).click();
    await ref.locator('#capbar .rec').waitFor();
    await ref.mouse.move(...await ref.locator('.ptapp').evaluate(e => { const b = e.getBoundingClientRect(); return [b.x + 200, b.y + 200]; }));
    await ref.mouse.down(); await ref.mouse.up();
    await ref.locator('#caphl.chosen').waitFor();
    await ref.locator('#capbar .rec').click();
    await ref.locator('.capcd').waitFor();
    await ref.keyboard.press('Enter');
    await ref.locator('#pill .pillvo').waitFor();
    await ref.getByRole('button', {name: 'Library', exact: true}).click();
    await ref.locator('#sidelist .mitem').first().click();
    await ref.locator('#doc .cliprec').waitFor();

    // 1. Recording: the recbar, ● REC on the stage, the note; the Pill says Video only.
    const p = await page.context().newPage();
    const errors = []; p.on('pageerror', e => errors.push(e.message));
    await p.emulateMedia({colorScheme: theme, reducedMotion: 'reduce'});
    await p.setViewportSize({width: 1048, height: 664});
    await p.addInitScript(fixture, {label: 'main', live: true, silent: false});
    await p.goto('http://127.0.0.1:3000/main/');
    await p.locator('.cliprec').waitFor();
    const recbar = await p.locator('.recbar').innerText();
    assert(/^Recording\n01:14\nThe screen · console\.example\.app\/reports window · no mic\nPause\nStop$/.test(recbar), `Recbar: ${JSON.stringify(recbar)}`);
    assert(await p.locator('.cliprec').innerText() === '● REC', 'REC label');
    assert(await p.locator('.clipnote').innerText() === "Motetto's own windows are never in the video. It stays on this Mac.", 'Recording note');
    assert(await p.locator('.clipframe video').count() === 0, 'A picture shows while the part is written');
    await same(p.locator('.recbar'), ref.locator('#doc .recbar'), '.recbar', ['height']);
    await same(p.locator('.recbar .btn.rec'), ref.locator('#doc .recbar .btn.rec'), 'recbar Stop');
    await same(p.locator('.clipstage'), ref.locator('#doc .clipstage'), '.clipstage live', ['height']);
    await same(p.locator('.cliprec'), ref.locator('#doc .cliprec'), '.cliprec');
    await same(p.locator('.clipnote'), ref.locator('#doc .clipnote'), '.clipnote', ['height']);
    await p.screenshot({path: `.playwright-out/screen-item-recording-${theme}.png`});
    await ref.locator('#doc').screenshot({path: `.playwright-out/screen-item-ref-recording-${theme}.png`});
    const c = await page.context().newPage();
    c.on('pageerror', e => errors.push(e.message));
    await c.emulateMedia({colorScheme: theme, reducedMotion: 'reduce'});
    await c.setViewportSize({width: 460, height: 72});
    await c.addInitScript(fixture, {label: 'companion', live: true, silent: false});
    await c.goto('http://127.0.0.1:3000/companion/');
    await c.locator('.pillvo').waitFor();
    assert(await c.locator('.pillvo').innerText() === 'Video only · no mic', 'Pill text');
    assert(!await c.locator('.duet').isVisible() && !await c.locator('.caption').isVisible(), 'The Pill shows the Duet or captions');
    await same(c.locator('.pillvo'), ref.locator('#pill .pillvo'), '.pillvo', ['height']);
    await same(c.locator('.pillvo svg'), ref.locator('#pill .pillvo svg'), '.pillvo svg');
    await c.screenshot({path: `.playwright-out/screen-item-pill-${theme}.png`});
    await ref.locator('#pill').screenshot({path: `.playwright-out/screen-item-ref-pill-${theme}.png`});
    r.recording = 'recbar, REC, note and Pill passed';

    // The mockup saves after Stop; its page is then the ready clip.
    await ref.waitForTimeout(6500);
    await ref.locator('#doc [data-act="stop"]').click();
    await ref.locator('#doc .headacts').waitFor({timeout: 15000});

    // 2. Saved: head actions, facts, the notes card, a dot per click.
    const q = await page.context().newPage();
    q.on('pageerror', e => errors.push(e.message));
    await q.emulateMedia({colorScheme: theme, reducedMotion: 'reduce'});
    await q.setViewportSize({width: 1048, height: 760});
    await video(q);
    await q.addInitScript(fixture, {label: 'main', live: false, silent: false});
    await q.goto('http://127.0.0.1:3000/main/');
    const row = q.locator('.meeting-row').first();
    await row.waitFor();
    assert((await row.innerText()).includes('Video only') && await row.locator('.media-video').count() === 1, `Library row: ${await row.innerText()}`);
    assert(await row.locator('i.plain').innerText() === 'Video only', 'Video only chip');
    await row.click();
    await q.locator('.clipctl').waitFor();
    assert(JSON.stringify(await q.locator('.headacts .btn').allInnerTexts()) === JSON.stringify(['Trim', 'Show in Finder', 'Export MP4…']), 'Head actions');
    const facts = await q.locator('.cfact').allInnerTexts();
    assert(JSON.stringify(facts) === JSON.stringify(['console.example.app/reports window', '15 fps · MP4', 'No mic', 'App sounds', 'Clicks shown']), `Facts: ${facts}`);
    const refFacts = await ref.locator('#doc .cfact').allInnerTexts();
    assert(JSON.stringify(refFacts) === JSON.stringify(facts), `Facts differ from the mockup: ${refFacts}`);
    const card = await q.locator('.clipcard').innerText();
    assert(card.replace(/\s+/g, ' ') === 'Just the video Motetto can transcribe its app sounds and write notes, on this Mac. The video stays as it is. Write notes', `Card: ${card}`);
    assert(card === await ref.locator('#doc .clipcard').innerText(), `Card differs from the mockup: ${JSON.stringify(await ref.locator('#doc .clipcard').innerText())}`);
    assert(await q.locator('.clipctl .cdot').count() === 3, 'One dot per click');
    const dot = await q.locator('.clipctl .cdot').nth(1).evaluate(e => e.style.left);
    assert(Math.abs(parseFloat(dot) - 12000 / 44000 * 100) < .01, `Dot at ${dot}`);
    const meta = await q.locator('.document-meta').innerText();
    assert(/Screen recording/.test(meta) && /44 s/.test(meta) && /1\.1 MB/.test(meta), `Meta: ${meta}`);
    for (const [a, b, what, skip] of [
      ['.headacts .btn', '#doc .headacts .btn', 'Trim button', ['height']],
      ['.cfact', '#doc .cfact', '.cfact', []],
      ['.cfact svg', '#doc .cfact svg', '.cfact svg', []],
      ['.clipcard', '#doc .clipcard', '.clipcard', ['height']],
      ['.clipcard .cic', '#doc .clipcard .cic', '.clipcard .cic', []],
      ['.clipcard .btn', '#doc .clipcard .btn', 'Write notes', []],
      ['.clipstage', '#doc .clipstage', '.clipstage', ['height']],
      ['.clipframe', '#doc .clipframe', '.clipframe', ['height']],
      ['.vctl.clipctl', '#doc .vctl.clipctl', '.clipctl', ['height']],
      ['.clipctl .cdot', '#doc .clipctl .cdot', '.cdot', []],
      ['.document-meta .cvid', '#doc .meta .cvid', '.cvid', ['height']],
    ]) await same(q.locator(a), ref.locator(b), what, skip);
    await q.screenshot({path: `.playwright-out/screen-item-ready-${theme}.png`});
    await ref.locator('#doc').screenshot({path: `.playwright-out/screen-item-ref-ready-${theme}.png`});
    // Space plays without focus anywhere.
    await q.locator('body').click({position: {x: 1040, y: 750}});
    await q.keyboard.press('Space');
    await q.waitForFunction(() => document.querySelector('.clipctl [aria-label="Pause"]'));
    await q.keyboard.press('Space');
    r.ready = 'actions, facts, card, dots, meta and Space passed';

    // 3. Trim.
    await q.keyboard.press('Meta+t');
    await q.locator('.trimbar').waitFor();
    await ref.locator('#doc').click({position: {x: 5, y: 5}});
    await ref.keyboard.press('Meta+t');
    await ref.locator('#doc .trimbar').waitFor();
    assert(await q.locator('.headacts .btn').first().getAttribute('aria-pressed') === 'true', 'Trim not pressed');
    assert(await q.getByRole('button', {name: 'Trim', exact: true}).last().isDisabled(), 'Trim enabled at the full range');
    assert(await q.locator('.trimbar .trow .mono').innerText() === 'Keeps 00:44.0 of 00:44.0', `Status: ${await q.locator('.trimbar .trow .mono').innerText()}`);
    await same(q.locator('.headacts .btn[aria-pressed="true"]'), ref.locator('#doc .headacts .btn[aria-pressed="true"]'), 'Trim pressed', ['height']);
    for (const [a, b, what, skip] of [
      ['.trimbar', '#doc .trimbar', '.trimbar', ['height']],
      ['.tstrip', '#doc .tstrip', '.tstrip', []],
      ['.twin', '#doc .twin', '.twin', []],
      ['.twin .th.a', '#doc .twin .th.a', '.th', []],
      ['.treach', '#doc .treach', '.treach', []],
      ['.tsug', '#doc .tsug', '.tsug', []],
      ['.tkeys', '#doc .tkeys', '.tkeys', ['height']],
      ['.trimbar .trow', '#doc .trimbar .trow', '.trow', ['height']],
      ['.trimbar .btn.primary', '#doc .trimbar .btn.primary', 'Trim (disabled)', []],
    ]) await same(q.locator(a), ref.locator(b), what, skip);
    await q.screenshot({path: `.playwright-out/trim-open-${theme}.png`});
    await ref.locator('#doc').screenshot({path: `.playwright-out/trim-ref-open-${theme}.png`});
    const strip = await q.locator('#tstrip').boundingBox();
    const ha = q.locator('[data-th="a"]'), hb = q.locator('[data-th="b"]');
    const hbox = await ha.boundingBox();
    await q.mouse.move(hbox.x + hbox.width / 2, hbox.y + hbox.height / 2);
    await q.mouse.down();
    await q.mouse.move(strip.x + strip.width * .2, hbox.y + hbox.height / 2, {steps: 5});
    await q.mouse.up();
    const now = async (h) => Number(await h.getAttribute('aria-valuenow'));
    assert(Math.abs(await now(ha) - 8.8) <= .1, `Dragged to ${await now(ha)}`);
    const dragged = await now(ha);
    await q.keyboard.press('ArrowRight');
    assert(Math.abs(await now(ha) - (dragged + .1)) < .001, `ArrowRight: ${await now(ha)}`);
    await q.keyboard.press('Shift+ArrowRight');
    assert(Math.abs(await now(ha) - (dragged + 1.1)) < .001, `Shift+ArrowRight: ${await now(ha)}`);
    const chip = q.locator('.tsug');
    assert(await chip.innerText() === 'Cut the last 1.6 s · reaching for Stop', `Chip: ${await chip.innerText()}`);
    await chip.click();
    assert(await now(hb) === 42.4 && await chip.count() === 0, `Chip end ${await now(hb)}`);
    assert(!await q.getByRole('button', {name: 'Trim', exact: true}).last().isDisabled(), 'Trim still disabled');
    await q.screenshot({path: `.playwright-out/trim-cut-${theme}.png`});
    await hb.focus();
    await q.keyboard.press('Enter');
    await q.getByText(/^Trimmed to /).waitFor();
    const call = await q.evaluate(() => window.calls.find(x => x.c === 'trim_video').a);
    assert(call.id === 'clip1' && call.fromMs === Math.round(dragged * 1000 + 1100) && call.toMs === 42400, `trim_video ${JSON.stringify(call)}`);
    const toast = await q.locator('.toast span').first().innerText();
    const kept = Math.round((42400 - call.fromMs) / 1000);
    assert(toast === `Trimmed to ${kept} s. The full video is in Recently deleted for 30 days.`, `Toast: ${toast}`);
    assert(await q.locator('.trimbar').count() === 0, 'The strip stayed open');
    await q.locator('.toast').getByRole('button', {name: 'Undo'}).click();
    await q.waitForFunction(() => window.calls.some(x => x.c === 'restore_trim' && x.a.id === 'clip1'));
    await q.locator('.clipctl').waitFor();
    // Esc cancels without trimming.
    await q.locator('body').click({position: {x: 1040, y: 750}});
    await q.keyboard.press('Meta+t');
    await q.locator('.trimbar').waitFor();
    await q.keyboard.press('Escape');
    await q.locator('.trimbar').waitFor({state: 'detached'});
    assert(await q.evaluate(() => window.calls.filter(x => x.c === 'trim_video').length) === 1, 'Esc trimmed');
    r.trim = 'open, disabled, drag, keys, chip, Return, toast, Undo and Esc passed';

    // 4. No sound: the quiet card.
    const z = await page.context().newPage();
    z.on('pageerror', e => errors.push(e.message));
    await z.emulateMedia({colorScheme: theme, reducedMotion: 'reduce'});
    await z.setViewportSize({width: 1048, height: 760});
    await video(z);
    await z.addInitScript(fixture, {label: 'main', live: false, silent: true});
    await z.goto('http://127.0.0.1:3000/main/');
    await z.locator('.meeting-row').first().click();
    await z.locator('.clipcard.quiet').waitFor();
    assert((await z.locator('.clipcard').innerText()).replace(/\s+/g, ' ') === "No sound in this video You recorded it with the microphone and app sounds off, so there's nothing to transcribe.", 'Quiet card');
    assert(await z.locator('.cfact').nth(3).innerText() === 'No app sounds', 'No app sounds chip');
    r.silent = 'quiet card passed';

    // 5. Recently deleted: the trimmed original.
    await z.locator('.trash').click();
    const trashRow = z.locator('.trashlist .trow').first();
    await trashRow.waitFor();
    assert((await trashRow.locator('b').innerText()) === 'Full video of Screen recording at 10:02', `Trash row: ${await trashRow.innerText()}`);
    await z.screenshot({path: `.playwright-out/screen-item-trash-${theme}.png`});
    await z.getByRole('button', {name: 'Restore all'}).click();
    await z.waitForFunction(() => window.calls.some(x => x.c === 'meeting_action' && x.a.id === 'clip1-full' && x.a.action === 'restore'));
    await z.locator('.toast').waitFor();
    assert(await z.locator('.toast').getByRole('button', {name: 'Undo'}).count() === 0, 'Undo offered for a full video put back');
    r.trash = 'full video row and Restore all passed';

    assert(!errors.length, JSON.stringify(errors));
    r.diffs = diffs.splice(0);
    out.push(r);
    for (const x of [ref, p, c, q, z]) await x.close();
  }
  if (out.some(r => r.diffs.length)) throw Error(JSON.stringify(out.map(r => r.diffs)));
  return out;
}
