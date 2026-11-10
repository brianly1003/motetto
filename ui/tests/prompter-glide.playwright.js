// Between what the recogniser hears, the highlight glides on at your reading pace (as in the
// mockup), at most 2 words past the last word heard and never past the end of a sentence or
// clause; it never steps back, stops when you pause, and one stray match never starts it.
async (page) => {
  const p = await page.context().newPage();
  const errors = []; p.on('pageerror', e => errors.push(e.message));
  await p.setViewportSize({ width: 560, height: 204 });
  await p.addInitScript(() => {
    const text = 'Good morning, team. Today I want to demo how our infrastructure handles a critical security challenge: insider threats and data sabotage.\n\nThe next paragraph starts here.';
    let id = 0; const callbacks = {}, listeners = {};
    window.emit = (event, payload) => (listeners[event] || []).forEach(k => callbacks[k]?.({ payload }));
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener(event, id) { listeners[event] = (listeners[event] || []).filter(k => k !== id); } };
    window.__TAURI_INTERNALS__ = { metadata: { currentWindow: { label: 'prompter' }, currentWebview: { windowLabel: 'prompter', label: 'prompter' } }, transformCallback(cb) { callbacks[++id] = cb; return id; }, unregisterCallback() {}, invoke: async (cmd, args) => {
      if (cmd === 'plugin:event|listen') { (listeners[args.event] ||= []).push(args.handler); return args.handler; }
      if (cmd === 'get_state') return { state: { type: 'Idle' }, share_visible: false, elapsed_ms: 0, companion: { visible: false, size: 'Pill' } };
      if (cmd === 'prompter_state') return { script: 'Demo', layout: 'Strip', check: 'hidden', editing: false, rehearsing: true };
      if (cmd === 'get_settings') return { prompter: { layout: 'Strip', text_size: 22, last_script: 'Demo' }, shortcuts: { Up: 'Ctrl+Alt+Up', Down: 'Ctrl+Alt+Down' } };
      if (cmd === 'scripts_list') return [{ name: 'Demo', modified_ms: 1, bytes: text.length }];
      if (cmd === 'script_read') return text;
      return null;
    } };
  });
  await p.goto('http://127.0.0.1:3014/prompter/');
  await p.waitForSelector('[data-word]');
  const heard = t => p.evaluate(t => window.emit('motetto://follow-heard', { text: t, inference_ms: 300, end_ms: 1 }), t);
  const now = () => p.evaluate(() => +(document.querySelector('.ptln.cur .ptnow')?.dataset.word ?? -1));
  // One match alone: the highlight sits on the next word and does not glide.
  await heard('good morning team');
  await p.waitForTimeout(700);
  if (await now() !== 3) throw Error(`one match glided: on word ${await now()}, heard up to 3`);
  // Reading in step (a second match): the highlight moves on past the heard word, word by word.
  await heard('morning team today i');
  const seen = [];
  for (let i = 0; i < 12; i++) { seen.push(await now()); await p.waitForTimeout(100); }
  if (Math.max(...seen) <= 5) throw Error(`no glide past the heard word 5: ${seen}`);
  if (Math.max(...seen) > 7) throw Error(`glided more than 2 words past 5: ${seen}`);
  if (seen.some((w, i) => i && w < seen[i - 1])) throw Error(`the highlight stepped back: ${seen}`);
  // A pause: nothing heard for 1.5 s, so it stops where it is.
  await p.waitForTimeout(1600);
  const paused = await now();
  await p.waitForTimeout(800);
  if (await now() !== paused) throw Error(`glided on in silence: ${paused} -> ${await now()}`);
  // Heard behind the highlight: it waits for your voice instead of stepping back.
  await heard('today i want');
  await p.waitForTimeout(150);
  if (await now() < paused) throw Error(`stepped back to ${await now()} from ${paused}`);
  // Your voice passes it: the highlight follows the voice again.
  await heard('want to demo how our infrastructure handles');
  await p.waitForTimeout(150);
  if (await now() < 12) throw Error(`did not follow the voice past the glide: on ${await now()}`);
  // Heard to a clause's end ("challenge:"): the next clause waits for your voice.
  await heard('a critical security challenge');
  await p.waitForTimeout(600);
  if (await now() !== 16) throw Error(`glided past the clause end: on ${await now()}, heard up to 16`);
  // A move by hand shows exactly where you moved, with no glide on top.
  await p.evaluate(() => window.emit('motetto://prompter-key', 'Down'));
  await p.waitForTimeout(600);
  const passage = await p.evaluate(() => [...document.querySelectorAll('.ptln.cur [data-word]')].find(w => w.classList.contains('ptnow'))?.textContent);
  await p.screenshot({ path: '.playwright-out/prompter-glide.png' });
  if (errors.length) throw Error(errors.join('\n'));
  await p.close();
  return { seen, paused, passage };
}
