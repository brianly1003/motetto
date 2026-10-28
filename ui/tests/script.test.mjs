import {test} from 'node:test';
import assert from 'node:assert/strict';
import {parseScript, follow, pace, withLine, nameFrom} from '../lib/script.ts';
const demo = parseScript('# Opening (1m)\nThanks for having us today.\n\nThe short version is simple.\n\n# Demo (5m)\nThis is the reports page.\n\n[click New] Make one for sales.\n\nPick the metrics and watch.');
test('sections, cues and lines',()=>{
 const s = parseScript('# Demo (5m)\nThis is it.\n\n[click New] Make one.');
 assert.deepEqual(s.sections, [{title: 'Demo', minutes: 5, first: 0}]);
 assert.equal(s.lines.length, 2);
 assert.deepEqual(s.lines[1].cues, ['click New']);
 assert.deepEqual(s.lines[1].words, ['make', 'one']);
 assert.equal(s.lines[1].text, '[click New] Make one.');
});
test('text before any heading is section "Script"; a heading without minutes has 0',()=>{
 const s = parseScript('Hello there.\n# Next\nAnd more.');
 assert.deepEqual(s.sections.map((x) => [x.title, x.minutes, x.first]), [['Script', 0, 0], ['Next', 0, 1]]);
});
test('advances word by word',()=>{
 assert.deepEqual(follow(demo.lines, {line: 0, word: 0}, 'thanks for'), {line: 0, word: 2, waiting: false});
});
test('moves to the next line at the end of a line',()=>{
 assert.deepEqual(follow(demo.lines, {line: 0, word: 2}, 'having us today the short'), {line: 1, word: 2, waiting: false});
});
test('skips_ahead_on_match',()=>{
 assert.deepEqual(follow(demo.lines, {line: 0, word: 0}, 'this is the reports'), {line: 2, word: 4, waiting: false});
});
test('ignores_repeated_earlier_line',()=>{
 assert.deepEqual(follow(demo.lines, {line: 2, word: 1}, 'thanks for having us today'), {line: 2, word: 1, waiting: true});
});
test('waits_on_adlib',()=>{
 assert.deepEqual(follow(demo.lines, {line: 2, word: 1}, 'and by the way our team loves coffee a lot'), {line: 2, word: 1, waiting: true});
});
test('never jumps further than three lines ahead',()=>{
 assert.deepEqual(follow(demo.lines, {line: 0, word: 0}, 'pick the metrics and watch'), {line: 0, word: 0, waiting: true});
});
test('normalizes case, punctuation, diacritics and numbers written as digits',()=>{
 const s = parseScript('Café, au lait for three people.');
 assert.deepEqual(follow(s.lines, {line: 0, word: 0}, 'CAFE au lait'), {line: 0, word: 3, waiting: false});
 assert.deepEqual(follow(s.lines, {line: 0, word: 3}, 'for 3 people'), {line: 0, word: 6, waiting: false});
});
test('CJK is compared per character',()=>{
 const s = parseScript('我们开始演示');
 assert.deepEqual(s.lines[0].words, ['我', '们', '开', '始', '演', '示']);
 assert.equal(follow(s.lines, {line: 0, word: 0}, '我们开').word, 3);
});
test('pace marks behind after the section budget',()=>{
 assert.deepEqual(pace(demo.sections, 2, 59), {totalS: 360, budgetS: 360, behind: false});
 assert.deepEqual(pace(demo.sections, 0, 61), {totalS: 360, budgetS: 60, behind: true});
});
test('withLine replaces that script line only and keeps its cues', () => {
  const text = '# Demo (2m)\nSay hello.\n\n  [click New] Say hello.\n\nSay hello.';
  assert.equal(withLine(text, 1, 'Hi $& there.'), '# Demo (2m)\nSay hello.\n\n  [click New] Hi $& there.\n\nSay hello.');
  assert.equal(withLine(text, 9, 'x'), text);
});
test('a new script is named from its first line, unique, within the file name rules', () => {
  assert.equal(nameFrom('# Opening (1m)\nHello', new Set(), 'Untitled script'), 'Opening');
  assert.equal(nameFrom('[click] Thanks for having us: today I show you how Team X makes a weekly report', new Set(), 'U'), 'Thanks for having us today I show you');
  assert.equal(nameFrom('# Opening', new Set(['Opening']), 'U'), 'Opening 2');
  assert.equal(nameFrom('???', new Set(), 'Untitled script'), 'Untitled script');
  assert.equal(nameFrom('...hidden', new Set(), 'U'), 'hidden');
});
test('Markdown bullets, numbers and emphasis are not read as words', () => {
  const s = parseScript('- A. Same code, **new** hostname.\n  - Only that `one` route.\n1. First step\n* Star item\n> Quoted');
  assert.deepEqual(s.lines.map(l => l.text), ['A. Same code, new hostname.', 'Only that one route.', 'First step', 'Star item', 'Quoted']);
  assert.deepEqual(s.lines[0].words, ['a', 'same', 'code', 'new', 'hostname']);
});
