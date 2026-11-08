// Run as a Playwright page function. IPC is mocked only in this browser fixture.
// Volume sliders (owner 2026-10-23): their own track and thumb; held, the thumb turns to clear
// glass and grows so the fill shows through, as in macOS Sound settings. Set APP/MOCK if not 3000/3001.
async (page) => {
  const APP = globalThis.APP || 'http://127.0.0.1:3000', MOCK = globalThis.MOCK || 'http://127.0.0.1:3001';
  const out = [];
  const keys = ['appearance', 'height', 'backgroundColor', 'cursor'];
  const styles = (loc) => loc.evaluate((e, k) => Object.fromEntries(k.map(x => [x, getComputedStyle(e)[x]])), keys);
  // The thumb's middle, from the value: the track runs 10 px in from each end (20 px thumb).
  const thumb = async (loc) => { const [b, v] = await Promise.all([loc.boundingBox(), loc.evaluate(e => +e.value / 100)]); return {x: b.x + 10 + (b.width - 20) * v, y: b.y + b.height / 2, box: b}; };
  for (const theme of ['light', 'dark']) {
    const ref = await page.context().newPage();
    await ref.route('https://**/*', x => x.abort());
    await ref.emulateMedia({colorScheme: theme});
    await ref.setViewportSize({width: 1400, height: 1000});
    await ref.goto(`${MOCK}/motetto-app-mockup.html`);
    await ref.evaluate(t => document.documentElement.setAttribute('data-theme', t), theme);
    await ref.getByRole('button', {name: 'Settings', exact: true}).click();
    await ref.getByRole('tab', {name: 'Audio'}).click();
    if (!await ref.locator('.volrow input').count()) {
      const select = ref.locator('.setpane select').first();
      const values = await select.locator('option').evaluateAll(o => o.map(x => x.value).filter(Boolean));
      for (const v of values) { await select.selectOption(v); if (await ref.locator('.volrow input').count()) break; }
    }
    const mockSlider = ref.locator('.volrow input');
    const p = await page.context().newPage();
    const errors = []; p.on('pageerror', e => errors.push(e.message));
    await p.emulateMedia({colorScheme: theme});
    await p.setViewportSize({width: 900, height: 640});
    await p.addInitScript(() => {
      Object.defineProperty(navigator, 'platform', {value: 'MacIntel'});
      const mics = [{id: 'builtin', name: 'MacBook Pro Microphone', transport: 'BuiltIn', rate_hz: 48000, volume: .5, is_default: true}];
      const report = () => ({mics, chosen: null, active: mics[0], issue: null});
      const settings = {version: 1, notes_folder: '~/Documents/Motetto', open_at_login: true, notes_language: 'same_as_call', ask_on_call: true, app_modes: {}, apps: [], idle_stop_s: 30, speech_model: '', notes_model: '', call_language: 'detect', part_chars: 24000, keep_audio: '30_days', recording_reminder: true, mic_device: null, sounds: true, companion: {default_size: 'pill', captions: true}, spellings: [], shortcuts: {S: 'Ctrl+Alt+S', M: 'Ctrl+Alt+M', J: 'Ctrl+Alt+J', N: 'Ctrl+Alt+N', Space: 'Ctrl+Alt+Space', H: 'Ctrl+Alt+H', P: 'Ctrl+Alt+P'}};
      let next = 0; window.volumes = [];
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {unregisterListener: () => {}};
      window.__TAURI_INTERNALS__ = {metadata: {currentWindow: {label: 'settings'}, currentWebview: {windowLabel: 'settings', label: 'settings'}}, convertFileSrc: p => p,
        transformCallback: () => ++next, unregisterCallback: () => {},
        invoke: async (c, a) => {
          if (c === 'plugin:event|listen') return next;
          if (c === 'microphones') return report();
          if (c === 'set_mic_volume') { window.volumes.push(a.volume); mics[0].volume = a.volume; return report(); }
          if (c === 'get_levels') return [0, 0];
          if (c === 'get_settings') return settings;
          if (c === 'call_apps') return [];
          if (c === 'about') return {version: '1.0.0', licences: '', platform: 'macos'};
          return null;
        }};
    });
    await p.goto(`${APP}/settings/?tab=audio`);
    const slider = p.getByRole('slider', {name: 'Input volume for MacBook Pro Microphone'});
    await slider.waitFor();
    const [a, m] = [await styles(slider), await styles(mockSlider)];
    for (const k of keys) if (a[k] !== m[k]) throw Error(`Slider ${k}: ${a[k]} != ${m[k]}`);
    if (a.appearance !== 'none') throw Error('Slider still uses the native look');
    if (await slider.evaluate(e => e.style.getPropertyValue('--v')) !== '0.5') throw Error('Fill does not follow the value');
    const rest = await thumb(slider);
    const clip = {x: rest.box.x - 20, y: rest.box.y - 14, width: rest.box.width + 90, height: rest.box.height + 28};
    await p.screenshot({path: `.playwright-out/slider-rest-${theme}.png`, clip});
    // Hold the thumb and drag it: glass while held, and the fill and the % follow.
    await p.mouse.move(rest.x, rest.y);
    await p.mouse.down();
    await p.mouse.move(rest.box.x + 10 + (rest.box.width - 20) * .8, rest.y, {steps: 8});
    await p.waitForTimeout(200);
    await p.screenshot({path: `.playwright-out/slider-held-${theme}.png`, clip});
    if (!await slider.evaluate(e => e.matches(':active'))) throw Error('Slider is not :active while held');
    const v = +(await slider.evaluate(e => e.style.getPropertyValue('--v')));
    if (Math.abs(v - .8) > .03) throw Error(`Fill at ${v} while dragged to 0.8`);
    if (await p.locator('.volrow output').innerText() !== `${Math.round(v * 100)}%`) throw Error('Percentage does not follow');
    await p.mouse.up();
    await p.waitForTimeout(400);
    if (await slider.evaluate(e => e.matches(':active'))) throw Error('Still :active after release');
    await p.screenshot({path: `.playwright-out/slider-released-${theme}.png`, clip});
    const saved = await p.evaluate(() => window.volumes.at(-1));
    if (Math.abs(saved - v) > .011) throw Error(`Saved ${saved}, slider ${v}`);
    // The mockup draws the same slider; hold its thumb too for the side-by-side.
    const r = await thumb(mockSlider);
    await mockSlider.scrollIntoViewIfNeeded();
    const r2 = await thumb(mockSlider);
    await ref.mouse.move(r2.x, r2.y); await ref.mouse.down(); await ref.waitForTimeout(200);
    await ref.screenshot({path: `.playwright-out/slider-mockup-held-${theme}.png`, clip: {x: r2.box.x - 20, y: r2.box.y - 14, width: r2.box.width + 90, height: r2.box.height + 28}});
    await ref.mouse.up();
    if (errors.length) throw Error(errors.join('\n'));
    out.push(`${theme}: rest .5, held and dragged to ${v.toFixed(2)}, saved ${saved}`);
    await p.close(); await ref.close();
  }
  return out;
}
