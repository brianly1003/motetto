// The script strip matches the mockup, rehearsing and presenting, in both themes: same script,
// same computed styles for every part (the window's shadow is the system's, so not compared).
async (page) => {
  const text = "# Opening (1m)\nThanks for having us. In the next fifteen minutes I'll show how Team X gets a weekly report in under a minute.\n\nThe short version: fewer spreadsheets, and numbers everyone trusts.\n\n# Demo (5m)\nThis is the reports page. These three cards are live numbers from the last seven days.\n\n[click New report] Let's make a new report for the sales team.\n\nI pick the metrics, and the chart builds itself while I talk.\n\n# Pricing (3m)\nPricing is per workspace, not per seat, so the whole team can join.\n\n# Questions (6m)\nThat's the tour. What would you like to see in more detail?";
  const parts = ['.pt', '.pth', '.ptgrab', '.ptchip', '.ptchip b', '.ptreh', '.ptreh > i', '.ptreh button', '.ptpace', '.ptpace > i', '.ptvis .h', '.ptvis .s', '.ptedit', '.ptclose', '.ptlines', '.ptln.prev', '.ptln.cur', '.ptln.cur [data-word]', '.ptsaid', '.ptnow', '.ptcue', '.ptln.ptahead', '.ptfoot', '.ptnav button', '.ptpage', '.ptprog', '.ptprog > i'];
  const props = ['font-family', 'font-size', 'font-weight', 'line-height', 'color', 'background-color', 'border-top', 'border-radius', 'padding', 'gap', 'letter-spacing', 'text-decoration-line', 'opacity'];
  const styles = p => p.evaluate(([parts, props]) => Object.fromEntries(parts.map(s => {
    const el = document.querySelector(s); if (!el || !el.getClientRects().length) return [s, null];
    const cs = getComputedStyle(el); return [s, Object.fromEntries(props.map(k => [k, cs.getPropertyValue(k)]))];
  })), [parts, props]);
  const result = [];
  for (const rehearsing of [true, false]) for (const theme of ['light', 'dark']) {
    const state = `${rehearsing ? 'rehearsing' : 'presenting'} ${theme}`;
    const m = await page.context().newPage();
    await m.route('https://**/*', r => r.abort());
    await m.emulateMedia({ colorScheme: theme }); await m.setViewportSize({ width: 1280, height: 800 });
    await m.addInitScript(() => { try { localStorage.removeItem('mn2-appearance'); } catch {} });
    await m.goto(`http://127.0.0.1:3001/motetto-app-mockup.html?v=${Date.now()}`);
    await m.locator('[data-go="script"]').click();
    if (rehearsing) { await m.locator('[data-ptv="lib"]').first().click(); await m.locator('#ptRehearse').click(); }
    await m.waitForTimeout(1500);
    const mock = await styles(m);
    await m.locator('#ptStrip').screenshot({ path: `.playwright-out/prompter-mockup-${rehearsing ? 'reh' : 'pre'}-${theme}.png` });
    const a = await page.context().newPage();
    const errors = []; a.on('pageerror', e => errors.push(e.message));
    await a.emulateMedia({ colorScheme: theme }); await a.setViewportSize({ width: 560, height: 204 });
    await a.addInitScript(([text, rehearsing]) => {
      let id = 0; const callbacks = {}, listeners = {};
      window.emit = (event, payload) => (listeners[event] || []).forEach(k => callbacks[k]?.({ payload }));
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
      window.__TAURI_INTERNALS__ = { metadata: { currentWindow: { label: 'prompter' }, currentWebview: { windowLabel: 'prompter', label: 'prompter' } }, transformCallback(cb) { callbacks[++id] = cb; return id; }, unregisterCallback() {}, invoke: async (cmd, args) => {
        if (cmd === 'plugin:event|listen') { (listeners[args.event] ||= []).push(args.handler); return args.handler; }
        if (cmd === 'get_state') return { state: { type: 'Idle' }, share_visible: false, elapsed_ms: 0, companion: { visible: false, size: 'Pill' } };
        if (cmd === 'prompter_state') return { script: 'Team X demo', layout: 'Strip', check: 'hidden', editing: false, rehearsing };
        if (cmd === 'get_settings') return { prompter: { layout: 'Strip', text_size: 22, last_script: 'Team X demo' }, shortcuts: {} };
        if (cmd === 'scripts_list') return [{ name: 'Team X demo', modified_ms: 1, bytes: text.length }];
        if (cmd === 'script_read') return text;
        return null;
      } };
    }, [text, rehearsing]);
    await a.goto('http://127.0.0.1:3014/prompter/');
    await a.waitForSelector('[data-word]');
    // The mockup opens a rehearsal on its first words and presenting on "Let's make a new".
    await a.evaluate(t => window.emit('motetto://follow-heard', { text: t, inference_ms: 300, end_ms: 1 }), rehearsing ? 'thanks for having us in the' : 'the last seven days lets make a new');
    await a.waitForTimeout(800);
    const app = await styles(a);
    await a.screenshot({ path: `.playwright-out/prompter-app-${rehearsing ? 'reh' : 'pre'}-${theme}.png` });
    const diff = [];
    for (const s of parts) {
      if (!mock[s] !== !app[s]) { diff.push(`${s} shown only in the ${mock[s] ? 'mockup' : 'app'}`); continue; }
      if (mock[s]) for (const k of props) if (mock[s][k] !== app[s][k]) diff.push(`${s} ${k}: mockup ${mock[s][k]}, app ${app[s][k]}`);
    }
    // The chip opens the script menu (▾) only when presenting: a rehearsal belongs to one script.
    for (const [who, p] of [['mockup', m], ['app', a]])
      if (/▾/.test(await p.locator('.ptchip').first().innerText()) === rehearsing) diff.push(`${who}: chip ▾ ${rehearsing ? 'while rehearsing' : 'missing'}`);
    // One focus ring, the app-wide one; the editor has none (the editing strip is outlined).
    const ring = p => p.locator('.ptnav button').first().evaluate(b => { b.disabled = false; b.focus({ focusVisible: true }); const cs = getComputedStyle(b); return `${cs.boxShadow} | outline ${cs.outlineStyle}`; });
    const [mockRing, appRing] = [await ring(m), await ring(a)];
    if (mockRing !== appRing) diff.push(`focus ring: mockup ${mockRing}, app ${appRing}`);
    await m.locator('#ptEdit').click();
    await a.evaluate(() => window.emit('motetto://prompter-editing', true));
    for (const [who, p] of [['mockup', m], ['app', a]]) {
      const box = await p.locator('.pt textarea').evaluate(t => { t.focus({ focusVisible: true }); const cs = getComputedStyle(t); return `${cs.boxShadow} | outline ${cs.outlineStyle}`; });
      if (box !== 'none | outline none') diff.push(`${who}: editor ring ${box}`);
    }
    if (diff.length) throw Error(`${state}:\n${diff.join('\n')}`);
    if (errors.length) throw Error(`${state}: ${errors.join('\n')}`);
    result.push(state);
    await m.close(); await a.close();
  }
  return result;
}
