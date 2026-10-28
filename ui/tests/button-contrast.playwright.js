// Every button keeps readable text at rest and on hover (owner 2026-10-28: a hovered Present
// button turned light grey under its light label). Hovers each visible button on the library,
// Scripts and a script's editor in light and dark, composites the backgrounds behind it, and
// fails below 4.5:1 (WCAG AA for normal text). Disabled buttons are exempt, as in WCAG.
async (page) => {
  const fails = [];
  const checked = [];
  for (const theme of ['light', 'dark']) {
    const p = await page.context().newPage(); p.on('pageerror', e => fails.push(e.message));
    await p.emulateMedia({ colorScheme: theme });
    await p.setViewportSize({ width: 1200, height: 760 });
    await p.addInitScript(() => {
      const summary = { id: '1', title: 'Launch sync', created_ms: Date.now(), status: 'Ready', marks_ms: [], duration_ms: 60000, notes_ms: 8000, spellings: [], locked: false };
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
      let n = 0;
      window.__TAURI_INTERNALS__ = { metadata: { currentWindow: { label: 'main' }, currentWebview: { windowLabel: 'main', label: 'main' } }, convertFileSrc: x => x, transformCallback: () => ++n, unregisterCallback: () => {}, invoke: async (c, a) => {
        if (c === 'plugin:event|listen') return a.handler;
        if (c === 'get_state') return { state: { type: 'Idle' }, companion: { visible: false, size: 'Pill' }, elapsed_ms: 0, quit_pending: false, share_visible: false };
        if (c === 'list_meetings') return [summary];
        if (c === 'deleted_meetings' || c === 'open_action_items') return [];
        if (c === 'scripts_list') return [{ name: 'Demo', modified_ms: Date.now(), bytes: 4 }];
        if (c === 'script_read') return '# Opening (1m)\nHello there.';
        return null; } };
    });
    await p.goto('http://127.0.0.1:3000/main/');
    await p.locator('.sidebar').waitFor();
    const views = [['library', async () => {}], ['scripts', async () => { await p.getByRole('button', { name: 'Scripts' }).first().click(); await p.getByRole('button', { name: 'Present' }).waitFor(); }]];
    for (const [view, open] of views) {
      await open();
      const count = await p.locator('button:visible').count();
      for (let i = 0; i < count; i++) {
        const button = p.locator('button:visible').nth(i);
        const label = (await button.innerText().catch(() => '')).trim().split('\n')[0];
        if (!label) continue; // icon-only buttons have no text to measure
        if (await button.isDisabled()) continue; // WCAG exempts disabled controls
        for (const state of ['rest', 'hover']) {
          if (state === 'hover') await button.hover({ timeout: 1000 }).catch(() => {}); else await p.mouse.move(1, 1);
          await p.waitForTimeout(state === 'hover' ? 180 : 20);
          const ratio = await button.evaluate(el => {
            // rgb()/rgba() in 0-255, or color(srgb r g b / a) in 0-1 (what color-mix() computes to).
            const parse = c => {
              const v = c.replace(/^color\(srgb/, '').match(/[\d.]+/g)?.map(Number) ?? [0, 0, 0, 0];
              const scale = c.startsWith('color(') ? 255 : 1;
              return [v[0] * scale, v[1] * scale, v[2] * scale, v.length > 3 ? v[3] : 1];
            };
            // The colour behind the text: this element's background over its ancestors'.
            const layers = [];
            for (let e = el; e; e = e.parentElement) { const bg = parse(getComputedStyle(e).backgroundColor); if (bg[3] > 0) layers.push(bg); if (bg[3] >= 1) break; }
            let ground = [255, 255, 255];
            for (const [r, g, b, a] of layers.reverse()) ground = [r * a + ground[0] * (1 - a), g * a + ground[1] * (1 - a), b * a + ground[2] * (1 - a)];
            const [fr, fg, fb, fa] = parse(getComputedStyle(el).color);
            const text = [fr * fa + ground[0] * (1 - fa), fg * fa + ground[1] * (1 - fa), fb * fa + ground[2] * (1 - fa)];
            const lum = ([r, g, b]) => [r, g, b].map(v => { v /= 255; return v <= .03928 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4; }).reduce((s, v, k) => s + v * [.2126, .7152, .0722][k], 0);
            const [hi, lo] = [lum(text), lum(ground)].sort((x, y) => y - x);
            return (hi + .05) / (lo + .05);
          });
          checked.push(`${theme} ${view} ${label} ${state} ${ratio.toFixed(1)}`);
          if (ratio < 4.5) fails.push(`${theme} ${view} "${label}" ${state}: ${ratio.toFixed(2)}:1`);
        }
      }
    }
    await p.close();
  }
  return fails.length ? fails : `ok: ${checked.length} button states`;
}
