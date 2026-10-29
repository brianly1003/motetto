// Long paragraphs remain readable at every size; keys and buttons share the voice position.
async (page) => {
  const result = [];
  for (const theme of ['light', 'dark']) {
    for (const [width, size, layout] of [[560, 22, 'Strip'], [300, 22, 'Column'], [560, 40, 'Strip']]) {
      const p = await page.context().newPage();
      const errors = []; p.on('pageerror', e => errors.push(e.message));
      await p.emulateMedia({ colorScheme: theme });
      await p.setViewportSize({ width, height: layout === 'Column' ? 600 : 204 });
      await p.addInitScript(({ size, layout }) => {
        const text = '# Demo (2m)\n' + Array.from({ length: 18 }, (_, n) => `Read the complete passage ${n + 1} carefully before continuing.`).join(' ') + ' Finish every remaining word.\n\nThe next paragraph starts here.';
        let id = 0; const callbacks = {}, listeners = {};
        window.emit = (event, payload) => (listeners[event] || []).forEach(k => callbacks[k]?.({ payload }));
        window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
        window.__TAURI_INTERNALS__ = { metadata: { currentWindow: { label: 'prompter' }, currentWebview: { windowLabel: 'prompter', label: 'prompter' } }, transformCallback(cb) { callbacks[++id] = cb; return id; }, unregisterCallback() {}, invoke: async (cmd, args) => {
          if (cmd === 'plugin:event|listen') { (listeners[args.event] ||= []).push(args.handler); return args.handler; }
          if (cmd === 'get_state') return { state: { type: 'Idle' }, share_visible: false, elapsed_ms: 0, companion: { visible: false, size: 'Pill' } };
          if (cmd === 'prompter_state') return { script: 'Demo', layout, check: 'hidden', editing: false, rehearsing: false };
          if (cmd === 'get_settings') return { prompter: { layout, text_size: size, last_script: 'Demo' }, shortcuts: { Down: 'Ctrl+Alt+Down', Up: 'Ctrl+Alt+Up' } };
          if (cmd === 'scripts_list') return [{ name: 'Demo', modified_ms: 1, bytes: text.length }];
          if (cmd === 'script_read') return text;
          if (cmd === 'prompter_controls') window.rects = args.rects;
          return null;
        } };
      }, { size, layout });
      await p.goto('http://127.0.0.1:3014/prompter/');
      await p.waitForFunction(() => +(document.querySelector('.ptpage')?.textContent.split('/')[1] || 0) > 1);
      const pages = +(await p.locator('.ptpage').textContent()).split('/')[1];
      const seen = new Set();
      const outside = await p.locator('.pth').evaluate(el => [...el.children].filter(e => e.getBoundingClientRect().right > innerWidth + 1).map(e => e.className));
      if (outside.length) throw Error('Header clipped: ' + outside);
      await p.getByRole('button', { name: 'Next passage', exact: true }).focus();
      if (!await p.getByRole('button', { name: 'Next passage', exact: true }).evaluate(e => e === document.activeElement)) throw Error('Passage button cannot take focus');
      for (let step = 0; step < pages; step++) {
        await p.waitForFunction(expected => document.querySelector('.ptpage')?.textContent === expected, `${step + 1}/${pages}`, { timeout: 1500 }).catch(async () => { throw Error(`Wrong page order: ${await p.locator('.ptpage').textContent()} expected ${step + 1}/${pages} at ${width}/${size}`); });
        // Rows rise and roll in: read what is shown once they have settled.
        await p.waitForFunction(() => !document.getAnimations().some(a => a.playState === 'running'), null, { timeout: 2000 });
        await p.waitForTimeout(150);
        const view = await p.locator('.ptln.cur').evaluate(el => {
          const r = el.getBoundingClientRect();
          return [...el.querySelectorAll('[data-word]')].filter(w => { const b = w.getBoundingClientRect(); return b.top >= r.top - 1 && b.bottom <= r.bottom + 1; }).map(w => +w.dataset.word);
        });
        view.forEach(w => seen.add(w));
        if (step === 1) await p.screenshot({ path: `.playwright-out/prompter-passages-reading-${theme}-${width}-${size}.png` });
        const footer = await p.locator('.ptfoot').boundingBox();
        if (footer.y + footer.height > (layout === 'Column' ? 600 : 204)) throw Error(`Footer clipped at ${width}/${size}`);
        if (step < pages - 1 && /The next paragraph/.test(await p.locator('.ptahead').textContent())) throw Error('Next paragraph appears before current paragraph ends');
        if (step % 2) await p.getByRole('button', { name: 'Next passage', exact: true }).click();
        else await p.evaluate(() => window.emit('motetto://prompter-key', 'Down'));
      }
      await p.waitForFunction(() => /The next paragraph/.test(document.querySelector('.ptln.cur')?.textContent || ''));
      const count = 18 * 8 + 4;
      if (seen.size !== count) throw Error(`Unread words: ${count - seen.size} at ${width}/${size}`);
      if (!/The next paragraph/.test(await p.locator('.ptln.cur').textContent())) throw Error('Did not reach next paragraph');
      await p.getByRole('button', { name: 'Previous passage', exact: true }).click();
      if (await p.locator('.ptpage').textContent() !== `${pages}/${pages}`) throw Error('Back skipped the paragraph ending');
      await p.evaluate(() => window.emit('motetto://prompter-key', 'Up'));
      await p.waitForFunction(expected => document.querySelector('.ptpage')?.textContent === expected, `${pages - 1}/${pages}`, { timeout: 1500 });
      await p.evaluate(() => window.emit('motetto://follow-heard', { text: 'finish every remaining', inference_ms: 800, end_ms: 10000 }));
      await p.waitForFunction(() => { const w = document.querySelector('.ptnow'), r = document.querySelector('.ptln.cur')?.getBoundingClientRect(), b = w?.getBoundingClientRect(); return !!b && b.top >= r.top && b.bottom <= r.bottom; });
      if (width === 560 && size === 22) {
        const word = await p.locator('.ptnow').textContent();
        await p.setViewportSize({ width: 420, height: 204 });
        await p.evaluate(() => window.emit('motetto://prompter-key', 'Equal'));
        await p.waitForFunction(() => getComputedStyle(document.querySelector('.ptln.cur')).fontSize === '24px');
        if (await p.locator('.ptnow').textContent() !== word) throw Error('Reflow lost the reading position');
        await p.waitForFunction(() => { const r = document.querySelector('.ptln.cur').getBoundingClientRect(), w = document.querySelector('.ptnow').getBoundingClientRect(); return w.top >= r.top && w.bottom <= r.bottom; });
      }
      await p.screenshot({ path: `.playwright-out/prompter-passages-${theme}-${width}-${size}.png` });
      if (errors.length) throw Error(errors.join('\n'));
      result.push({ theme, width, size, pages, words: seen.size });
      await p.close();
    }
  }
  return result;
}
