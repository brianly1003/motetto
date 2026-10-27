// Browser fixtures: appearance persistence, cross-window events and approved General layout.
async page => {
  const browser = await page.context().browser().browserType().launch({ channel: 'chrome' });
  const context = await browser.newContext();
  context.setDefaultTimeout(8000);
  context.setDefaultNavigationTimeout(8000);
  const p = await context.newPage();
  const ref = await context.newPage();
  const errors = [];
  let step = 'layout';
  try {
    p.on('pageerror', e => errors.push(e.message));
    await ref.route('https://**/*', r => r.abort());
    await ref.goto('http://127.0.0.1:3001/motetto-app-mockup.html');
    await ref.getByRole('button', { name: 'Settings', exact: true }).click();
    await ref.getByRole('tab', { name: 'General', exact: true }).click();
    await p.addInitScript(() => {
      const defaults = { version: 1, appearance: 'system', notes_folder: '~/Documents/Motetto', open_at_login: false, notes_language: 'same_as_call', ask_on_call: true, apps: [], app_modes: {}, idle_stop_s: 30, discard_short_s: 20, sounds: true, companion: { default_size: 'pill', captions: true }, shortcuts: Object.fromEntries(['S', 'M', 'J', 'N', 'Space', 'H', 'P'].map(k => [k, `Ctrl+Alt+${k}`])) };
      const callbacks = {}, listeners = {};
      let next = 0;
      window.emit = (event, payload) => (listeners[event] || []).forEach(id => callbacks[id]?.({ event, id, payload }));
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
      window.__TAURI_INTERNALS__ = {
        transformCallback: callback => { callbacks[++next] = callback; return next; },
        unregisterCallback: id => { delete callbacks[id]; },
        invoke: async (command, args) => {
          const settings = JSON.parse(localStorage.getItem('appearance-fixture') || 'null') || defaults;
          if (command === 'plugin:event|listen') { (listeners[args.event] ||= []).push(args.handler); if(args.event === 'motetto://settings-changed')window.appearanceListening = true; return args.handler; }
          if (command === 'plugin:event|unlisten') { listeners[args.event] = (listeners[args.event] || []).filter(id => id !== args.eventId); return; }
          if (command === 'get_settings') return settings;
          if (command === 'save_settings') {
            if (window.failSave) { window.failSave = false; throw Error('Disk is full. Settings were not saved.'); }
            localStorage.setItem('appearance-fixture', JSON.stringify(args.settings));
            window.emit('motetto://settings-changed', null);
            return args.settings;
          }
          if (command === 'call_apps') return [];
          if (command === 'about') return { platform: window.fixturePlatform || 'macos', version: '0.1.0', licences: '' };
          if (command === 'get_state') return { state: { type: 'Idle' }, meeting_id: null, companion: { visible: false, size: 'Pill' }, elapsed_ms: 0, quit_pending: false };
          return null;
        }
      };
    });
    await p.goto('http://127.0.0.1:3014/settings/');
    await p.evaluate(() => { localStorage.removeItem('appearance-fixture'); localStorage.removeItem('motetto-appearance'); });
    await p.reload();
    const radio = mode => p.getByRole('radio', { name: mode, exact: true });
    const theme = () => p.evaluate(() => document.documentElement.dataset.theme || 'system');
    for (const platform of ['macOS', 'Windows']) {
      await p.evaluate(value => { window.fixturePlatform = value; }, platform === 'Windows' ? 'windows' : 'macos');
      await p.getByRole('tab', { name: 'About', exact: true }).click();
      await p.getByRole('tab', { name: 'General', exact: true }).click();
      await ref.getByRole('button', { name: platform, exact: true }).click();
      for (const mode of ['Light', 'Dark']) {
        // Explicit choices must override the opposite system mode.
        await p.emulateMedia({ colorScheme: mode === 'Light' ? 'dark' : 'light' });
        await radio(mode).click();
        await p.waitForFunction(value => document.documentElement.dataset.theme === value, mode.toLowerCase(), { timeout: 8000 });
        await p.getByRole('button', { name: 'Run setup again', exact: true }).waitFor({ state: 'visible' });
        await p.waitForFunction(() => !document.querySelector('.settings-fields').disabled, null, { timeout: 8000 });
        await ref.getByRole('radio', { name: mode, exact: true }).check();
        for (const [width, height] of [[958, 760], [720, 646]]) {
          await p.setViewportSize({ width, height });
          await ref.locator('.set').evaluate((e, size) => { e.style.width = `${size[0] + 2}px`; e.style.height = `${size[1] + 36}px`; }, [width, height]);
          const fits = await p.locator('.setpane').evaluate(e => e.scrollHeight <= e.clientHeight && e.scrollWidth <= e.clientWidth);
          if (!fits) throw Error(`${platform} ${mode}: General scrolls at ${width}x${height}`);
          const props = ['padding', 'gap', 'fontSize', 'fontWeight', 'backgroundColor', 'color', 'borderRadius', 'outlineWidth', 'gridTemplateColumns'];
          for (const selector of ['.setpane', '.formrow', '.appearance-options', '.appearance-preview', '.appearance-caption', '.appearance-mini', '.help']) {
            const read = async (target) => target.locator(selector).first().evaluate((e, keys) => Object.fromEntries(keys.map(k => [k, getComputedStyle(e)[k]])), props);
            const actual = await read(p), expected = await read(ref);
            for (const key of props) if (actual[key] !== expected[key]) throw Error(`${selector} ${key}: ${actual[key]} != ${expected[key]}`);
          }
          if (width === 720) await p.screenshot({ path: `.playwright-out/general-app-${platform}-${mode}.png` });
        }
      }
    }
    step = 'reload';
    await p.reload();
    await radio('Dark').waitFor();
    if (!await radio('Dark').isChecked() || await theme() !== 'dark') throw Error('Appearance did not survive reload');
    step = 'failed save';
    await p.evaluate(() => { window.failSave = true; });
    await radio('Light').click();
    await p.getByText('Disk is full. Settings were not saved.').waitFor();
    if (await theme() !== 'dark' || !await radio('Dark').isChecked()) throw Error('Failed save changed appearance');
    step = 'system';
    await radio('System').click();
    await p.waitForFunction(() => !document.documentElement.dataset.theme, null, { timeout: 8000 });
    for (const mode of ['light', 'dark']) {
      await p.emulateMedia({ colorScheme: mode });
      const scheme = await p.locator('html').evaluate(e => getComputedStyle(e).colorScheme);
      if (scheme !== mode) throw Error(`System did not follow ${mode}`);
    }
    await radio('System').focus();
    await p.keyboard.press('ArrowRight');
    await p.waitForFunction(() => document.documentElement.dataset.theme === 'light' && document.activeElement?.value === 'light', null, { timeout: 8000 });
    await radio('System').click();
    await p.waitForFunction(() => !document.documentElement.dataset.theme, null, { timeout: 8000 });
    // Another window receives the same settings event and reads the saved preference.
    step = 'other window';
    const other = await context.newPage();
    await other.addInitScript(() => {
      let id = 0;
      const callbacks = {}, listeners = {};
      window.emit = event => (listeners[event] || []).forEach(key => callbacks[key]?.({ event, id: key, payload: null }));
      window.__TAURI_INTERNALS__ = { transformCallback: cb => { callbacks[++id] = cb; return id; }, unregisterCallback: () => {}, invoke: async (command, args) => {
        if (command === 'plugin:event|listen') { (listeners[args.event] ||= []).push(args.handler); if(args.event === 'motetto://settings-changed')window.appearanceListening = true; return args.handler; }
        if (command === 'get_settings') return JSON.parse(localStorage.getItem('appearance-fixture'));
        return null;
      } };
    });
    await other.goto('http://127.0.0.1:3014/settings/');
    await other.getByRole('radio', { name: 'System', exact: true }).waitFor();
    await other.waitForFunction(() => window.appearanceListening, null, { timeout: 8000 });
    await radio('Dark').click();
    await p.waitForFunction(() => document.documentElement.dataset.theme === 'dark', null, { timeout: 8000 });
    await other.evaluate(() => window.emit('motetto://settings-changed'));
    await other.waitForFunction(() => document.documentElement.dataset.theme === 'dark', null, { timeout: 8000 });
    await other.close();
    const cold = await context.newPage();
    await cold.route('**/_next/static/chunks/**', route => route.abort());
    await cold.goto('http://127.0.0.1:3014/settings/');
    if (await cold.locator('html').getAttribute('data-theme') !== 'dark') throw Error('Saved appearance was not applied before hydration');
    await cold.close();
    await p.close(); await ref.close();
    if (errors.length) throw Error(errors.join('\n'));
    return 'PASS: General parity, both platforms/themes, no scrolling, persistence, failed save, System and cross-window updates';
  } catch (error) {
    throw Error(step + ': ' + error.message + ' State: ' + await p.evaluate(() => JSON.stringify({theme:document.documentElement.dataset.theme,checked:document.querySelector("input[name=appearance]:checked")?.value,error:document.querySelector(".settings-error")?.textContent})).catch(()=>'closed'));
  } finally { await browser.close(); }
}
