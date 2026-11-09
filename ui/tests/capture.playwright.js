// Run as a Playwright page function. The screen setup overlay (mockup "Screen capture picker"):
// the setup bar, its menus, pointing at a window, drawing an area, the countdown on the target,
// the notes rule and Esc, against the mockup in light and dark at 1400×1000. IPC is mocked in this
// browser fixture only; real windows, displays and capture are not exercised.
async (page) => {
  const out = [];
  const keys = ['backgroundColor', 'color', 'borderRadius', 'fontSize', 'fontWeight', 'lineHeight', 'paddingTop', 'paddingRight', 'paddingBottom', 'paddingLeft', 'gap', 'boxShadow', 'backdropFilter', 'borderTopColor', 'borderTopWidth', 'width'];
  const styles = (loc) => loc.evaluate((e, k) => Object.fromEntries(k.map(x => [x, getComputedStyle(e)[x]])), keys);
  const differ = (a, b, what, skip = []) => keys.filter(k => !skip.includes(k) && a[k] !== b[k]).map(k => `${what} ${k}: ${a[k]} != ${b[k]}`);
  const fixture = ({kind}) => {
    Object.defineProperty(navigator, 'platform', {value: 'MacIntel'});
    let next = 0; const callbacks = {}, listeners = {};
    window.fixtureEmit = (event, payload) => { for (const h of listeners[event] || []) callbacks[h]?.({event, id: 0, payload}); };
    const memory = {
      Call: {mode: 'audio', mic: {kind: 'Default'}, app_sounds: true, notes: true, timer: 0, clicks: true, area: null, remember: true},
      Clip: {mode: 'window', mic: {kind: 'Off'}, app_sounds: true, notes: false, timer: 3, clicks: true, area: null, remember: true},
    };
    const mics = [
      {id: 'builtin', name: 'MacBook Pro Microphone', transport: 'BuiltIn', rate_hz: 48000, volume: .5, is_default: true},
      {id: 'shokz', name: 'OpenRun Pro 2 by Shokz', transport: 'Bluetooth', rate_hz: 16000, volume: null, is_default: false},
      {id: 'usb', name: 'Shure MV7', transport: 'Usb', rate_hz: 48000, volume: .5, is_default: false},
    ];
    // Front first, in points relative to display 1, as capture_windows returns them.
    const windows = [
      {id: 21, app: 'Safari', title: 'console.example.app/reports', x: 560, y: 150, width: 520, height: 380, display: 1, own: false},
      {id: 22, app: 'zoom.us', title: 'Weekly sync · push notifications', x: 40, y: 64, width: 600, height: 400, display: 1, own: false},
      {id: 23, app: 'Motetto', title: 'Motetto', x: 900, y: 600, width: 420, height: 240, display: 1, own: true},
    ];
    window.calls = [];
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {unregisterListener: (event, id) => { listeners[event] = (listeners[event] || []).filter(h => h !== id); }};
    window.__TAURI_INTERNALS__ = {metadata: {currentWindow: {label: 'capture-1'}, currentWebview: {windowLabel: 'capture-1', label: 'capture-1'}}, convertFileSrc: p => p,
      transformCallback: cb => { callbacks[++next] = cb; return next; }, unregisterCallback: id => delete callbacks[id],
      invoke: async (c, a) => {
        window.calls.push({c, a});
        if (c === 'plugin:event|listen') { (listeners[a.event] ||= []).push(a.handler); return a.handler; }
        if (c === 'get_state') return {state: {type: 'Idle'}, meeting_id: null, app: null, elapsed_ms: 0};
        if (c === 'capture_memory') return memory[a.kind];
        if (c === 'get_settings') return {screen_preset: 'smaller'};
        if (c === 'capture_windows') return windows;
        if (c === 'microphones') return {mics, chosen: null, active: mics[0], issue: null};
        if (c === 'capture_level') return .12;
        if (c === 'list_screen_sources') return [{pick: {choice: {kind: 'Display', id: 1}, name: 'Built-in Retina Display'}, sub: ''}];
        return null;
      }};
  };
  const browser = page.context().browser();
  const count = (p, c) => p.evaluate(c => window.calls.filter(x => x.c === c).length, c);
  const open = async (ctx, theme, kind = 'Clip', dock = 0) => {
    const p = await ctx.newPage();
    const errors = []; p.on('pageerror', e => errors.push(e.message));
    await p.emulateMedia({colorScheme: theme, reducedMotion: 'reduce'});
    await p.addInitScript(fixture, {kind});
    await p.goto(`http://127.0.0.1:3000/capture/?display=1&kind=${kind}&dock=${dock}`);
    await p.locator('#capbar').waitFor();
    // A desktop behind the transparent overlay, for the screenshots only.
    await p.addStyleTag({content: 'html body { background: #c9ccc4 !important; }'});
    p.errors = errors;
    return p;
  };
  for (const theme of ['light', 'dark']) {
    const r = {theme};
    const ctx = await browser.newContext({viewport: {width: 1400, height: 1000}, deviceScaleFactor: 2});
    const ref = await ctx.newPage();
    await ref.route('https://**/*', x => x.abort());
    await ref.emulateMedia({colorScheme: theme, reducedMotion: 'reduce'});
    await ref.goto('http://127.0.0.1:3001/motetto-app-mockup.html');
    await ref.evaluate(t => document.documentElement.setAttribute('data-theme', t), theme);
    await ref.getByRole('button', {name: 'macOS', exact: true}).click();
    await ref.locator('[data-act="capture"]').first().click();
    await ref.locator('#capbar .rec').waitFor();

    const p = await open(ctx, theme);
    // Bar: four modes, Record and the 3 s dial for a clip; Tab order ends on ✕.
    if ((await p.locator('[data-capmode]').count()) !== 4) throw Error('modes');
    // Zoom-style: icon over a short label, the full name as the tooltip.
    if (await p.locator('[data-capmode="display"] span').innerText() !== 'Screen' || await p.locator('[data-capmode="display"]').getAttribute('title') !== 'Entire screen') throw Error('short labels');
    const box = await p.locator('.capbar').boundingBox();
    if (Math.abs(box.height - 52) > 1) throw Error('bar height ' + box.height);
    if (!(await p.locator('.capbar .rec').innerText()).startsWith('Record')) throw Error('Record label');
    if ((await p.locator('.capbar .rectime').innerText()).trim() !== '3s') throw Error('dial label');
    const tabs = await p.evaluate(() => [...document.querySelectorAll('#capbar button')].filter(b => !b.disabled).map(b => b.dataset.capmode || b.dataset.cap));
    if (tabs.join() !== 'audio,display,window,area,mic,opts,timer,close') throw Error('Tab order ' + tabs);
    const diffs = [];
    diffs.push(...differ(await styles(p.locator('.capbar')), await styles(ref.locator('#capbar')), '.capbar', ['width']));
    diffs.push(...differ(await styles(p.locator('.capbar .mode[aria-checked="true"]')), await styles(ref.locator('#capbar .mode[aria-checked="true"]')), '.mode', ['width']));
    diffs.push(...differ(await styles(p.locator('.capbar .rectime')), await styles(ref.locator('#capbar .rectime')), '.rectime'));
    diffs.push(...differ(await styles(p.locator('.caphint')), await styles(ref.locator('#caphint')), '.caphint', ['width']));
    diffs.push(...differ(await styles(p.locator('.caphint kbd')), await styles(ref.locator('#caphint kbd')), '.caphint kbd'));
    await p.screenshot({path: `.playwright-out/capture-app-bar-${theme}.png`});
    await ref.locator('#stage').screenshot({path: `.playwright-out/capture-ref-bar-${theme}.png`});

    // Mic menu: None, the system default once, the others, then Sounds from apps.
    await p.locator('[data-cap="mic"]').click();
    const rows = await p.locator('.capmenu [role="menuitemradio"]').evaluateAll(b => b.map(x => x.firstElementChild.nextElementSibling.firstChild.textContent));
    if (rows.join('|') !== 'None|System default (MacBook Pro Microphone)|OpenRun Pro 2 by Shokz|Shure MV7|Sounds from apps') throw Error('mic rows ' + rows);
    await ref.locator('#capbar [data-cap="mic"]').click();
    diffs.push(...differ(await styles(p.locator('.capmenu')), await styles(ref.locator('.capmenu')), '.capmenu'));
    diffs.push(...differ(await styles(p.locator('.capmenu [aria-checked="true"]').first()), await styles(ref.locator('.capmenu [aria-checked="true"]').first()), '.capmenu row'));
    await p.screenshot({path: `.playwright-out/capture-app-mic-${theme}.png`});
    await ref.locator('#stage').screenshot({path: `.playwright-out/capture-ref-mic-${theme}.png`});
    await ref.keyboard.press('Escape');

    // Timer menu: the four starts with their reasons, and the three keys.
    await p.keyboard.press('Escape');
    if (await p.locator('.capmenu').count()) throw Error('Esc keeps the menu');
    if (await count(p, 'close_capture')) throw Error('Esc on a menu closed the setup');
    await p.locator('[data-cap="timer"]').click();
    const subs = await p.locator('.capmenu .sub').allInnerTexts();
    if (subs.join('|') !== 'The moment you press Record|Time to bring the window forward|Time to put the pointer in place|Time to set the scene') throw Error('timer subs ' + subs);
    if ((await p.locator('.capmenu .keys span').count()) !== 3) throw Error('keys legend');
    if ((await p.locator('.capmenu .keys').innerText()).replace(/\s+/g, ' ') !== 'Skip the timer this once ⌥Return Start during the countdown Return Back to the setup Esc') throw Error('keys text');
    await ref.locator('#capbar [data-cap="timer"]').click();
    diffs.push(...differ(await styles(p.locator('.capmenu kbd').first()), await styles(ref.locator('.capmenu kbd').first()), '.capmenu kbd', ['width']));
    await p.screenshot({path: `.playwright-out/capture-app-timer-${theme}.png`});
    await ref.locator('#stage').screenshot({path: `.playwright-out/capture-ref-timer-${theme}.png`});
    await ref.keyboard.press('Escape');
    await p.keyboard.press('Escape');

    // Window mode: hover outlines, a click chooses, another hover is dashed, ours says why not.
    await p.mouse.move(800, 300);
    if (!(await p.locator('#caphl').isVisible()) || (await p.locator('#caphl.chosen').count())) throw Error('hover outline');
    await p.mouse.click(800, 300);
    await p.locator('#caphl.chosen').waitFor();
    if ((await p.locator('#capbadge').innerText()).replace(/\s+/g, ' ') !== 'console.example.app/reports 1040 × 760 · chosen') throw Error('badge ' + await p.locator('#capbadge').innerText());
    if ((await p.locator('.capbar .rec .meta').innerText()) !== '1040×760') throw Error('Record meta');
    await p.mouse.move(200, 200);
    if (!(await p.locator('#caphov').isVisible())) throw Error('no dashed hover over another window');
    await p.mouse.move(1000, 700);
    if (!(await p.locator('#caphint').innerText()).includes("Motetto's own windows are never recorded")) throw Error('own hint');
    await p.mouse.click(1000, 700);
    if ((await p.locator('.capbar .rec .meta').innerText()) !== '1040×760' || !(await p.locator('#caphov.own').isVisible())) throw Error('own window chosen');
    await ref.mouse.move(...await ref.locator('.ptapp').evaluate(e => { const b = e.getBoundingClientRect(); return [b.x + 200, b.y + 200]; }));
    await ref.mouse.down(); await ref.mouse.up();
    await ref.locator('#caphl.chosen').waitFor();
    diffs.push(...differ(await styles(p.locator('#capbadge')), await styles(ref.locator('#capbadge')), '.capbadge', ['width']));
    diffs.push(...differ(await styles(p.locator('#caphl')), await styles(ref.locator('#caphl')), '.caphl', ['width']));
    await p.mouse.move(800, 300);
    await p.screenshot({path: `.playwright-out/capture-app-window-${theme}.png`});
    await ref.locator('#stage').screenshot({path: `.playwright-out/capture-ref-window-${theme}.png`});

    // Countdown on the chosen window: +5 s, Back keeps every choice.
    await p.locator('.capbar .rec').click();
    await p.locator('.capcd').waitFor();
    const cd = await p.locator('.capcd').boundingBox();
    const [cx, cy] = [cd.x + cd.width / 2, cd.y + cd.height / 2];
    if (cx < 560 || cx > 1080 || cy < 150 || cy > 530) throw Error(`countdown off the window: ${cx},${cy}`);
    await p.locator('[data-count="more"]').click();
    const more = await p.locator('.cdnum').innerText();
    if (more !== '8') throw Error('+5 s shows ' + more);
    await ref.locator('#capbar .rec').click();
    await ref.locator('.capcd').waitFor();
    diffs.push(...differ(await styles(p.locator('.capcd')), await styles(ref.locator('.capcd')), '.capcd'));
    diffs.push(...differ(await styles(p.locator('.capcd .go')), await styles(ref.locator('.capcd .go')), '.capcd .go'));
    diffs.push(...differ(await styles(p.locator('.capcd .cdnum')), await styles(ref.locator('.capcd .cdnum')), '.cdnum', ['width']));
    await p.screenshot({path: `.playwright-out/capture-app-countdown-${theme}.png`});
    await ref.locator('#stage').screenshot({path: `.playwright-out/capture-ref-countdown-${theme}.png`});
    await ref.locator('[data-count="back"]').click();
    await p.locator('[data-count="back"]').click();
    if (await p.locator('.capcd').count()) throw Error('Back kept the countdown');
    if (!(await p.locator('#caphl.chosen').count()) || (await p.locator('.capbar .rectime').innerText()).trim() !== '3s' || !(await p.locator('[data-capmode="window"][aria-checked="true"]').count())) throw Error('Back lost a choice');
    if (await count(p, 'start_capture')) throw Error('started before the countdown ended');
    // Return during the countdown starts at once, and only once.
    await p.locator('.capbar .rec').click();
    await p.locator('.capcd').waitFor();
    await p.locator('[data-capmode="window"]').evaluate(() => document.activeElement.blur());
    await p.keyboard.press('Enter');
    await p.waitForTimeout(400);
    if ((await count(p, 'start_capture')) !== 1) throw Error('Return in the countdown: start_capture × ' + await count(p, 'start_capture'));
    const started = await p.evaluate(() => window.calls.find(x => x.c === 'start_capture').a);
    if (started.kind !== 'Clip' || started.setup.screen?.choice.window_id !== 21 || started.setup.mic.kind !== 'Off' || started.setup.notes || started.memory.timer !== 3) throw Error('start args ' + JSON.stringify(started));
    if (p.errors.length) throw Error(p.errors.join());
    await p.close();

    // ⌥Return skips the timer this once.
    const q = await open(ctx, theme);
    await q.mouse.click(800, 300);
    await q.locator('[data-capmode="window"]').focus();
    await q.keyboard.press('Alt+Enter');
    await q.waitForTimeout(200);
    if ((await count(q, 'start_capture')) !== 1 || (await q.locator('.capcd').count())) throw Error('⌥Return did not start at once');
    await q.close();

    // Area: drag 200→600 × 200→500 is 800 × 600 pixels at 2×; → moves it one point.
    const a = await open(ctx, theme);
    await a.locator('[data-capmode="area"]').click();
    await a.mouse.move(200, 200); await a.mouse.down(); await a.mouse.move(400, 350); await a.mouse.move(600, 500); await a.mouse.up();
    if ((await a.locator('#capsize').innerText()) !== '800 × 600') throw Error('area size ' + await a.locator('#capsize').innerText());
    const left = await a.locator('#caparea').evaluate(e => e.offsetLeft);
    await a.keyboard.press('ArrowRight');
    if ((await a.locator('#caparea').evaluate(e => e.offsetLeft)) !== left + 1) throw Error('ArrowRight did not move the area');
    await ref.locator('#capbar [data-capmode="area"]').click();
    diffs.push(...differ(await styles(a.locator('#capsize')), await styles(ref.locator('#capsize')), '#capsize', ['width']));
    await a.screenshot({path: `.playwright-out/capture-app-area-${theme}.png`});
    await ref.locator('#stage').screenshot({path: `.playwright-out/capture-ref-area-${theme}.png`});

    // Notes rule: no mic, app sounds off, then notes: app sounds come back on.
    await a.locator('[data-cap="mic"]').click();
    await a.locator('[data-capset="sys"]').click();
    if ((await a.locator('[data-capset="sys"]').getAttribute('aria-checked')) !== 'false') throw Error('app sounds stayed on');
    await a.keyboard.press('Escape');
    await a.locator('[data-cap="opts"]').click();
    await a.locator('[data-capset="notes:1"]').click();
    await a.locator('[data-cap="mic"]').click();
    if ((await a.locator('[data-capset="sys"]').getAttribute('aria-checked')) !== 'true') throw Error('notes rule: app sounds still off');
    await a.keyboard.press('Escape');

    // Esc from Rust: the page answers; with nothing open it closes the setup.
    await a.evaluate(() => window.fixtureEmit('motetto://capture-esc', 7));
    await a.waitForTimeout(100);
    const acks = await a.evaluate(() => window.calls.filter(x => x.c === 'capture_esc_ack').map(x => x.a.n));
    if (acks.join() !== '7' || (await count(a, 'close_capture')) !== 1) throw Error('Esc event: acks ' + acks + ', close × ' + await count(a, 'close_capture'));
    await a.keyboard.press('Escape');
    if ((await count(a, 'close_capture')) !== 2) throw Error('Esc did not close the setup');
    if (a.errors.length) throw Error(a.errors.join());

    // Record now (a call): Audio only with the live level, and Return starts at once.
    const c = await open(ctx, theme, 'Call');
    if (!(await c.locator('[data-capmode="audio"][aria-checked="true"]').count()) || !(await c.locator('.caplvl').count())) throw Error('call setup');
    if ((await c.locator('.capbar .rectime').innerText()).trim() !== 'Now') throw Error('call timer');
    // The meter listens to the system default mic only, and is real: capture_level, never a loop.
    await c.waitForFunction(() => window.calls.some(x => x.c === 'capture_mic_preview' && x.a.on) && window.calls.some(x => x.c === 'capture_level'));
    if (!(await c.locator('#caphint').innerText()).includes("Check your mic's level")) throw Error('audio hint');
    // Audio only: clicks pass through except over the bar and the hint.
    await c.waitForFunction(() => window.calls.some(x => x.c === 'capture_controls' && x.a.through && x.a.rects.length === 2));
    const bar = await c.locator('.capbar').boundingBox();
    const sentBar = await c.evaluate(() => window.calls.filter(x => x.c === 'capture_controls').at(-1).a.rects);
    if (!sentBar.some(([x, y, w, h]) => Math.abs(x - bar.x) <= 1 && Math.abs(y - bar.y) <= 1 && Math.abs(w - bar.width) <= 1 && Math.abs(h - bar.height) <= 1)) throw Error('bar rect not sent ' + JSON.stringify(sentBar));
    await c.screenshot({path: `.playwright-out/capture-app-audio-${theme}.png`});
    await c.locator('[data-capmode="window"]').click();
    await c.waitForFunction(() => window.calls.filter(x => x.c === 'capture_controls').at(-1).a.through === false);
    // Entire screen: a click on the screen only chooses it; nothing records until Record.
    await c.locator('[data-capmode="display"]').click();
    await c.mouse.click(700, 300);
    await c.waitForTimeout(150);
    if (await count(c, 'start_capture')) throw Error('a click on the screen started recording');
    // None: the menu row must take the press (WebKit does not focus a clicked button).
    await c.locator('[data-cap="mic"]').click();
    await c.locator('[data-capset="mic:off"]').click();
    if ((await c.locator('[data-cap="mic"]').innerText()).trim() !== 'No mic') throw Error('None not chosen');
    // A menu's own button toggles it shut; a press anywhere else closes it and does nothing more.
    await c.locator('[data-cap="opts"]').click();
    await c.locator('[data-cap="opts"]').click();
    await c.waitForTimeout(150);
    if (await c.locator('.capmenu').count()) throw Error('the same button again left the menu open');
    await c.locator('[data-capmode="window"]').click();
    await c.locator('[data-cap="mic"]').click();
    await c.mouse.move(800, 300);
    await c.mouse.click(800, 300);
    await c.waitForTimeout(150);
    if ((await c.locator('.capmenu').count()) || (await c.locator('#caphl.chosen').count())) throw Error('a press closing the menu chose a window');
    await c.locator('[data-capmode="audio"]').click();
    await c.locator('[data-cap="opts"]').click();
    await c.waitForFunction(() => window.calls.filter(x => x.c === 'capture_controls').at(-1).a.through === false);
    await c.mouse.click(700, 300);
    await c.waitForTimeout(150);
    if (await c.locator('.capmenu').count()) throw Error('Audio only: a click outside left the menu open');
    await c.locator('[data-capmode="audio"]').click();
    await c.locator('[data-cap="mic"]').click();
    await c.locator('[data-capset="mic:usb"]').click();
    await c.waitForFunction(() => window.calls.filter(x => x.c === 'capture_mic_preview').at(-1).a.on === false);
    await c.locator('.caplvl').waitFor({state: 'detached'});
    if ((await c.locator('.caplvl').count()) || (await c.locator('#caphint').innerText()).includes("Check your mic's level")) throw Error('meter shown for a device it cannot listen to');
    await c.locator('[data-cap="mic"]').click();
    await c.locator('[data-capset="mic:"]').click();
    await c.locator('.caplvl').waitFor();
    await c.keyboard.press('Enter');
    await c.waitForTimeout(200);
    const call = await c.evaluate(() => window.calls.find(x => x.c === 'start_capture')?.a);
    if (!call || call.kind !== 'Call' || call.setup.screen !== null || call.setup.mic.kind !== 'Default' || !call.setup.notes) throw Error('call start ' + JSON.stringify(call));
    await c.close();

    // A second display's overlay: a click there (no drag) keeps the area on the main display,
    // and Return there records the main display's area.
    const d = await ctx.newPage();
    d.errors = []; d.on('pageerror', e => d.errors.push(e.message));
    await d.addInitScript(fixture, {kind: 'Clip'});
    await d.goto('http://127.0.0.1:3000/capture/?display=2&kind=Clip&main=0&main_display=1&mode=area');
    await d.locator('#capture').waitFor();
    await d.waitForFunction(() => window.calls.some(x => x.c === 'capture_memory'));
    await d.waitForTimeout(150);
    await d.mouse.click(300, 300);
    await d.waitForTimeout(150);
    const sentArea = await d.evaluate(() => window.calls.filter(x => x.c === 'plugin:event|emit' || x.c === 'send_capture_setup').map(x => x.a).at(-1));
    const moved = JSON.stringify(sentArea ?? {}).match(/"area":\{"display":(\w+)/)?.[1];
    if (moved !== undefined && moved !== 'null') throw Error('A click on another display moved the area there: ' + JSON.stringify(sentArea));
    await d.keyboard.press('Alt+Enter');
    await d.waitForFunction(() => window.calls.some(x => x.c === 'start_capture'));
    const startArea = await d.evaluate(() => window.calls.find(x => x.c === 'start_capture').a.setup.screen.choice);
    if (startArea.kind !== 'Area' || startArea.display !== 1) throw Error('Return on another display: ' + JSON.stringify(startArea));
    if (d.errors.length) throw Error(d.errors.join());
    await d.close();

    // Picture: Settings' preset until this setup picks one; High goes with the recording.
    const q2 = await open(ctx, theme, 'Clip');
    await q2.locator('[data-cap="opts"]').click();
    await q2.locator('[data-capset="pic:smaller"][aria-checked="true"]').waitFor();
    if (!(await q2.locator('.capmenu .note').innerText()).includes('About 120 MB an hour')) throw Error('size for Settings\' preset');
    await q2.locator('[data-capset="pic:high"]').click();
    await q2.locator('[data-cap="opts"]').click();
    if ((await q2.locator('[data-capset="pic:high"]').getAttribute('aria-checked')) !== 'true') throw Error('High not chosen');
    if (!(await q2.locator('.capmenu .note').innerText()).includes('About 1 GB an hour')) throw Error('size for High');
    await q2.locator('.capmenu').screenshot({path: `.playwright-out/capture-picture-${theme}.png`});
    await q2.keyboard.press('Escape');
    await q2.locator('[data-capmode="display"]').click();
    await q2.keyboard.press('Alt+Enter');
    await q2.waitForFunction(() => window.calls.some(x => x.c === 'start_capture'));
    const pic = await q2.evaluate(() => window.calls.find(x => x.c === 'start_capture').a);
    if (pic.setup.picture !== 'high' || pic.memory.picture !== 'high') throw Error('picture not sent ' + JSON.stringify(pic));
    if (q2.errors.length) throw Error(q2.errors.join());
    await q2.close();
    // The Dock's height at the bottom: the bar and its hint sit above it, never on it.
    const dk = await open(ctx, theme, 'Clip', 70);
    const above = await dk.evaluate(() => [innerHeight - document.querySelector('.capbar').getBoundingClientRect().bottom, document.querySelector('.capbar').getBoundingClientRect().top - document.querySelector('.caphint').getBoundingClientRect().bottom]);
    if (above[0] < 70 + 12 || above[1] < 4) throw Error('bar over the Dock ' + above);
    await dk.screenshot({path: `.playwright-out/capture-dock-${theme}.png`});
    await dk.close();
    const q3 = await open(ctx, theme, 'Call');
    await q3.locator('[data-cap="opts"]').click();
    if (await q3.locator('[data-capset^="pic:"]').count()) throw Error('Picture offered in Audio only');
    await q3.close();

    r.diffs = diffs;
    out.push(r);
    await ctx.close();
  }
  const all = out.flatMap(r => r.diffs.map(d => `${r.theme}: ${d}`));
  if (all.length) throw Error('Style differences:\n' + all.join('\n'));
  return out;
}
