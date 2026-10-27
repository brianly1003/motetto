// Appearance design: previews, immediate switching, persistence and System following the computer.
async (page) => {
  const p = await page.context().newPage();
  const errors = [];
  p.on('pageerror', e => errors.push(e.message));
  await p.route('https://**/*', r => r.abort());
  await p.setViewportSize({ width: 1400, height: 1000 });
  await p.goto('http://127.0.0.1:3001/motetto-app-mockup.html');
  await p.evaluate(() => localStorage.removeItem('mn2-appearance'));
  await p.reload();
  await p.getByRole('button', { name: 'Settings', exact: true }).click();
  await p.getByRole('tab', { name: 'General', exact: true }).click();
  const group = p.getByRole('group', { name: 'Appearance', exact: true });
  const radio = name => group.getByRole('radio', { name, exact: true });
  const surface = () => p.locator('.set').evaluate(e => getComputedStyle(e).backgroundColor);
  const backgrounds = {};
  for (const platform of ['macOS', 'Windows']) {
    await p.getByRole('button', { name: platform, exact: true }).click();
    for (const mode of ['Light', 'Dark']) {
      await radio(mode).check();
      if (!await radio(mode).isChecked()) throw Error(`${platform}: ${mode} not selected`);
      const expected = mode.toLowerCase();
      const stored = await p.evaluate(() => localStorage.getItem('mn2-appearance'));
      if (stored !== expected) throw Error(`${mode} not remembered`);
      backgrounds[mode] = await surface();
      const outline = await p.locator('.appearance-option').filter({ has: p.getByRole('radio', { name: mode, exact: true }) }).locator('.appearance-preview').evaluate(e => getComputedStyle(e).outlineWidth);
      if (outline !== '2px') throw Error('Selected preview has no outline');
      for (const [width, height] of [[960, 796], [720, 680]]) {
        await p.locator('.set').evaluate((e, size) => { e.style.width = `${size[0]}px`; e.style.height = `${size[1]}px`; }, [width, height]);
        const fits = await p.locator('#setpane').evaluate(e => {
          const pane = e.getBoundingClientRect();
          return e.scrollHeight <= e.clientHeight && e.scrollWidth <= e.clientWidth
            && [...e.querySelectorAll('input, select, button, .general-shortcuts dt, .general-shortcuts dd')].every(control => {
              const r = control.getBoundingClientRect();
              return r.top >= pane.top && r.bottom <= pane.bottom && r.left >= pane.left && r.right <= pane.right;
            });
        });
        if (!fits) throw Error(`${platform} ${mode}: General needs scrolling or clips controls at ${width}x${height}`);
        if (width === 720) await p.locator('.set').screenshot({ path: `.playwright-out/general-compact-${platform}-${expected}.png` });
      }
      await p.locator('.set').evaluate(e => { e.style.width = ''; e.style.height = ''; });
      await p.locator('.set').screenshot({ path: `.playwright-out/appearance-${platform}-${expected}.png` });
    }
  }
  if (backgrounds.Light === backgrounds.Dark) throw Error('Appearance did not change');
  await p.reload();
  await p.getByRole('tab', { name: 'General', exact: true }).click();
  if (!await radio('Dark').isChecked() || await surface() !== backgrounds.Dark) throw Error('Dark did not survive reload');
  await radio('System').check();
  for (const mode of ['light', 'dark']) {
    await p.emulateMedia({ colorScheme: mode });
    if (await surface() !== backgrounds[mode === 'light' ? 'Light' : 'Dark']) throw Error(`System did not follow ${mode}`);
  }
  await radio('Light').check();
  await p.emulateMedia({ colorScheme: 'dark' });
  if (await surface() !== backgrounds.Light) throw Error('System overrode explicit Light');
  await radio('System').check();
  await radio('System').focus();
  await p.keyboard.press('ArrowRight');
  if (!await radio('Light').isChecked()) throw Error('Arrow key did not choose Light');
  const focus = await p.locator('.appearance-option').filter({ has: p.getByRole('radio', { name: 'Light', exact: true }) }).locator('.appearance-preview').evaluate(e => getComputedStyle(e).boxShadow);
  if (focus === 'none') throw Error('Keyboard focus has no preview indicator');
  // Check the control at the minimum settings width without changing the desktop demo framing.
  await p.locator('.set').evaluate(e => { e.style.width = '720px'; });
  if (await group.evaluate(e => e.scrollWidth > e.clientWidth)) throw Error('Appearance choices overflow');
  await p.locator('.set').screenshot({ path: '.playwright-out/appearance-minimum-focus.png' });
  await p.getByRole('tab', { name: 'Audio', exact: true }).click();
  await p.getByRole('tab', { name: 'General', exact: true }).click();
  if (!await radio('Light').isChecked()) throw Error('Tab change lost selection');
  await radio('System').check();
  await p.close();
  if (errors.length) throw Error(errors.join('\n'));
  return 'PASS: light/dark, macOS/Windows, persistence, System changes, keyboard, General fits at 960x796 and 720x680';
}
