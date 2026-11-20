import {test} from 'node:test';
import assert from 'node:assert/strict';
import {existsSync, readFileSync, readdirSync} from 'node:fs';

// The compact type scale (DESIGN 2.2): interface text is 10, 11, 12, 13 or 14 px, whole pixels;
// larger sizes are document and display type, icon glyphs, the palette field and the countdown.
const UI = new Set([10, 11, 12, 13, 14]);
const LARGER = new Set([8, 15, 16, 17, 18, 20, 21, 22, 26, 28, 30, 34, 46]);
const dir = new URL('../styles/', import.meta.url);
const sheets = readdirSync(dir).filter(f => f.endsWith('.css') && f !== 'theme.css' && f !== 'fonts.css');
const sizes = css => [...css.replace(/\/\*[\s\S]*?\*\//g, '').matchAll(/(?:font-size:\s*|\bfont:\s*(?:italic\s+)?(?:\d{3}\s+)?)([0-9.]+)px/g)].map(m => Number(m[1]));

test('every font size in the app is on the scale', () => {
  for (const file of sheets) {
    const off = sizes(readFileSync(new URL(file, dir), 'utf8')).filter(px => !UI.has(px) && !LARGER.has(px));
    assert.deepEqual(off, [], `${file} uses sizes off the scale`);
  }
});

// The mockup is the contract: where it is present (a local design file), it keeps the same scale.
const mockup = new URL('../../docs/app/motetto-app-mockup.html', import.meta.url);
test('the mockup keeps the same scale', {skip: !existsSync(mockup)}, () => {
  const html = readFileSync(mockup, 'utf8');
  const css = [...html.matchAll(/<style[^>]*>([\s\S]*?)<\/style>/g)].map(m => m[1]).join('\n') + [...html.matchAll(/style="([^"]*)"/g)].map(m => m[1]).join(';');
  const off = sizes(css).filter(px => !UI.has(px) && !LARGER.has(px));
  assert.deepEqual(off, []);
});
