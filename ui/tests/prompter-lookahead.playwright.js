// Following your voice never runs out of words, and the text only moves when you near the end
// of the bottom row: never when the highlight (which trails your voice) enters a row.
async (page) => {
  const result = [];
  for (const theme of ['light', 'dark']) {
    const p = await page.context().newPage();
    const errors = []; p.on('pageerror', e => errors.push(e.message));
    await p.emulateMedia({ colorScheme: theme });
    await p.setViewportSize({ width: 560, height: 204 });
    await p.addInitScript(() => {
      const text = 'Good morning, team. Today I want to demo how our infrastructure handles a critical security challenge: insider threats and data sabotage. In real-world scenarios, an employee preparing to leave the company might attempt to destroy or corrupt critical database records while their access is still active.\n\nThe next paragraph starts here.';
      let id = 0; const callbacks = {}, listeners = {};
      window.emit = (event, payload) => (listeners[event] || []).forEach(k => callbacks[k]?.({ payload }));
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
      window.__TAURI_INTERNALS__ = { metadata: { currentWindow: { label: 'prompter' }, currentWebview: { windowLabel: 'prompter', label: 'prompter' } }, transformCallback(cb) { callbacks[++id] = cb; return id; }, unregisterCallback() {}, invoke: async (cmd, args) => {
        if (cmd === 'plugin:event|listen') { (listeners[args.event] ||= []).push(args.handler); return args.handler; }
        if (cmd === 'get_state') return { state: { type: 'Idle' }, share_visible: false, elapsed_ms: 0, companion: { visible: false, size: 'Pill' } };
        if (cmd === 'prompter_state') return { script: 'Demo', layout: 'Strip', check: 'hidden', editing: false, rehearsing: true };
        if (cmd === 'get_settings') return { prompter: { layout: 'Strip', text_size: 22, last_script: 'Demo' }, shortcuts: {} };
        if (cmd === 'scripts_list') return [{ name: 'Demo', modified_ms: 1, bytes: text.length }];
        if (cmd === 'script_read') return text;
        return null;
      } };
    });
    await p.goto('http://127.0.0.1:3014/prompter/');
    await p.waitForSelector('[data-word]');
    const visible = () => p.locator('.ptln.cur').evaluate(el => {
      const r = el.getBoundingClientRect();
      return [...el.querySelectorAll('[data-word]')].filter(w => { const b = w.getBoundingClientRect(); return getComputedStyle(w).visibility !== 'hidden' && b.top >= r.top - 1 && b.bottom <= r.bottom + 1; }).map(w => w.textContent);
    });
    // Say the paragraph a few words at a time; after each step a word beyond the next one shows.
    const said = 'good morning team today i want to demo how our infrastructure handles a critical security challenge insider threats and data sabotage in real world scenarios an employee preparing to leave the company might attempt to destroy or corrupt critical database records while their access is still'.split(' ');
    for (let n = 3; n <= said.length; n += 3) {
      await p.evaluate(t => window.emit('motetto://follow-heard', { text: t, inference_ms: 300, end_ms: 1 }), said.slice(Math.max(0, n - 8), n).join(' '));
      await p.waitForTimeout(500);
      const moved = await p.locator('.ptln.cur').evaluate(el => { const was = window.lastTop; window.lastTop = el.scrollTop; return was !== undefined && was !== el.scrollTop; });
      // Words still to say on the highlighted row: a roll is only allowed with 3 or fewer left.
      const left = await p.locator('.ptln.cur').evaluate(el => {
        const now = el.querySelector('.ptnow'); if (!now) return 0;
        const top = now.getBoundingClientRect().top;
        return [...el.querySelectorAll('[data-word]')].filter(w => +w.dataset.word > +now.dataset.word && Math.abs(w.getBoundingClientRect().top - top) < 4).length;
      });
      if (moved && left > 3) throw Error(`${theme}: the text moved with ${left} words still to say on the row`);
      const words = await visible();
      const now = await p.locator('.ptnow').textContent().catch(() => '');
      const after = words.slice(words.indexOf(now) + 1);
      if (!words.includes(now)) throw Error(`${theme}: the word to say (${now}) is hidden after "${said[n - 1]}"`);
      if (n < said.length - 3 && after.length < 2) throw Error(`${theme}: only ${after.length} word(s) visible after "${now}"`);
      // The next line always shows what comes next, lighter and smaller than the rows you read.
      const next = await p.locator('.ptahead').evaluate(e => ({ text: e.textContent, shown: getComputedStyle(e).visibility !== 'hidden' && getComputedStyle(e).display !== 'none', size: parseFloat(getComputedStyle(e).fontSize) }));
      if (!next.shown || !next.text.trim()) throw Error(`${theme}: no next line after "${said[n - 1]}"`);
      if (next.size >= 22) throw Error(`${theme}: the next line is not smaller (${next.size}px)`);
      if (/^their/.test(now)) await p.screenshot({ path: `.playwright-out/prompter-lookahead-${theme}.png` });
    }
    if (errors.length) throw Error(errors.join('\n'));
    result.push(theme);
    await p.close();
  }
  return result;
}
