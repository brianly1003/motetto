import {test} from 'node:test';
import assert from 'node:assert/strict';
import {parseScript, follow} from '../lib/script.ts';
import {report as measure, spoken} from '../lib/rehearsal.ts';
const report = (s, t, ms) => measure(s, t, ms, follow);
const script = parseScript('# Opening (1m)\nThanks for having us today everyone.\n\nThe short version is simple and quick.\n\n# Demo (2m)\nThis is the reports page with three cards.\n\nPick the metrics and watch the chart build itself while I keep talking about it for a while longer.');
const at = (s, text) => `[${String(Math.floor(s / 60)).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}] Me: ${text}`;
test('transcript lines become timed words, Me only', () => {
  assert.deepEqual(spoken('[00:05] Me: hello there\n[00:07] Them: no\n[01:02] Me: again'), [{ms: 5000, text: 'hello there'}, {ms: 62000, text: 'again'}]);
});
test('per_section_times', () => {
  const r = report(script, [at(0, 'thanks for having us today everyone'), at(20, 'the short version is simple and quick'), at(70, 'this is the reports page with three cards')].join('\n'), 150_000);
  assert.equal(r.totalS, 150);
  assert.deepEqual(r.perSection.map(s => [s.plannedS, s.actualS]), [[60, 70], [120, 80]]);
});
test('skipped_line_detected', () => {
  const r = report(script, [at(0, 'thanks for having us today everyone'), at(30, 'this is the reports page with three cards'), at(50, 'pick the metrics and watch the chart build itself')].join('\n'), 60_000);
  assert.deepEqual(r.skipped, [1]);
});
test('a line never reached at the end is not skipped', () => {
  const r = report(script, at(0, 'thanks for having us today everyone'), 10_000);
  assert.deepEqual(r.skipped, []);
});
test('offscript_stretch_over_10s', () => {
  const r = report(script, [at(0, 'thanks for having us today everyone'), at(10, 'by the way our office has a great coffee machine'), at(16, 'and the weather was lovely this morning too'), at(25, 'the short version is simple and quick')].join('\n'), 40_000);
  assert.equal(r.offScript.length, 1);
  assert.deepEqual([r.offScript[0].startMs, r.offScript[0].before], [10_000, 1]);
  assert.ok(r.offScript[0].endMs >= 20_000);
  const short = report(script, [at(0, 'thanks for having us today everyone'), at(10, 'by the way coffee'), at(14, 'the short version is simple and quick')].join('\n'), 20_000);
  assert.equal(short.offScript.length, 0);
});
test('a pause after a word is silence, not off-script', () => {
  const r = report(script, [at(0, 'thanks for having us today everyone'), at(5, 'um'), at(25, 'the short version is simple and quick')].join('\n'), 35_000);
  assert.equal(r.offScript.length, 0);
});
test('fillers the script itself says are not counted', () => {
  const own = parseScript('I basically like this.');
  const r = report(own, at(0, 'I basically like this basically'), 5_000);
  assert.deepEqual(r.fillers, {basically: 1});
});
test('fillers_count_whole_words_only', () => {
  const r = report(script, at(0, 'um thanks for um having us you know basically umbrella likely like'), 10_000);
  assert.deepEqual(r.fillers, {um: 2, 'you know': 1, basically: 1, like: 1});
  const vi = report(parseScript('Xin chào các bạn.'), at(0, 'ờ xin chào kiểu các bạn'), 5_000);
  assert.deepEqual(vi.fillers, {'ờ': 1, 'kiểu': 1});
});
test('long_line_rule', () => {
  // 19 words at 150 wpm is 7.6 s; 1.4 times that is 10.6 s. Spoken over 40 s: long.
  const r = report(script, [at(0, 'thanks for having us today everyone'), at(10, 'the short version is simple and quick'), at(20, 'this is the reports page with three cards'), at(30, 'pick the metrics and watch the chart build itself while I keep talking about it for a while longer')].join('\n'), 70_000);
  assert.deepEqual(r.longLines, [3]);
  assert.equal(r.wpm, Math.round(40 / (70 / 60)));
});
