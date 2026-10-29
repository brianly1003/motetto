// Settings › Privacy › Script following (SPEC 14.5): on a Mac it shows what macOS says about
// Speech Recognition; Denied opens its System Settings pane; Windows has nothing to allow.
async page => {
  const fails = [];
  for (const [platform, grant, theme] of [['macos', 'Allowed', 'light'], ['macos', 'Denied', 'dark'], ['macos', 'NotAsked', 'light'], ['windows', 'NotAsked', 'dark']]) {
    const p = await page.context().newPage(); p.on('pageerror', e => fails.push(e.message));
    await p.emulateMedia({ colorScheme: theme });
    await p.setViewportSize({ width: 960, height: 760 });
    await p.addInitScript(([platform, grant]) => {
      let id = 0; window.calls = [];
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
      window.__TAURI_INTERNALS__ = { transformCallback: () => ++id, unregisterCallback: () => {}, invoke: async (c) => {
        window.calls.push(c);
        if (c === 'plugin:event|listen') return id;
        if (c === 'get_settings') return { appearance: 'system', keep_audio: 'days30', keep_video: 'same', recording_reminder: true };
        if (c === 'storage_summary') return { meetings: 1, audio_bytes: 1, video_bytes: 0 };
        if (c === 'speech_permission') return grant;
        if (c === 'about') return { platform, version: '0.1.0', licences: '' };
        return null;
      } };
    }, [platform, grant]);
    await p.goto('http://127.0.0.1:3014/settings/?tab=privacy');
    const row = p.locator('.formrow', { hasText: 'Script following' });
    await row.waitFor({ timeout: 3000 }).catch(() => fails.push(`${platform}/${grant}: no Script following row`));
    const text = await row.innerText().catch(() => '');
    const want = platform === 'windows' ? /Nothing to allow/ : grant === 'Allowed' ? /Allowed/ : grant === 'Denied' ? /Denied - Open settings/ : /Asked when the script first follows you/;
    if (!want.test(text)) fails.push(`${platform}/${grant}: ${text}`);
    if (platform === 'macos' && grant === 'Denied') {
      await row.getByRole('button', { name: 'Denied - Open settings' }).click();
      if (!(await p.evaluate(() => window.calls)).includes('open_speech_settings')) fails.push('Denied did not open the pane');
    }
    await row.screenshot({ path: `.playwright-out/settings-speech-${platform}-${grant}-${theme}.png` }).catch(() => {});
    await p.close();
  }
  return fails.length ? fails : 'ok';
}
