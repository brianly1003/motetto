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
const owner = parseScript('Good morning, team. Today I want to demo how our infrastructure handles a high-risk security scenario.');
test('a word still being said moves the highlight onto it',()=>{
 // "infra" is Apple's partial for the word in progress: the highlight is past "infrastructure" now.
 assert.deepEqual(follow(owner.lines, {line: 0, word: 9}, 'demo how our infra'), {line: 0, word: 11, waiting: false});
});
test('a near miss of the next word still counts',()=>{
 assert.deepEqual(follow(owner.lines, {line: 0, word: 10}, 'our infrastructure handle'), {line: 0, word: 12, waiting: false});
});
test('one misheard word at the end does not stop the words before it',()=>{
 assert.deepEqual(follow(owner.lines, {line: 0, word: 9}, 'how our infrastructure handles uh'), {line: 0, word: 12, waiting: false});
});
test('short words still match exactly',()=>{
 // "a" must not be read as the start of "and", nor "our" as "hour".
 assert.deepEqual(follow(owner.lines, {line: 0, word: 7}, 'hour'), {line: 0, word: 7, waiting: true});
});
// The owner's 23:56 rehearsal as Apple heard it: "an employee preparing to" came out as
// "unemployed repairing to", and "data sabotage" as one word.
const real = parseScript('In real-world scenarios, an employee preparing to leave the company might attempt to destroy data sabotage today.');
test('two script words heard as one still count',()=>{
 assert.deepEqual(follow(real.lines, {line: 0, word: 4}, 'real world scenarios unemployed'), {line: 0, word: 6, waiting: false});
 assert.deepEqual(follow(real.lines, {line: 0, word: 14}, 'to destroy datasabotage'), {line: 0, word: 17, waiting: false});
});
test('a long word heard a little wrong still counts',()=>{
 assert.deepEqual(follow(real.lines, {line: 0, word: 4}, 'scenarios unemployed repairing to'), {line: 0, word: 8, waiting: false});
});
test('a different word is not taken for the next one',()=>{
 assert.deepEqual(follow(real.lines, {line: 0, word: 10}, 'banana'), {line: 0, word: 10, waiting: true});
});
// The owner's 01:01 rehearsal: "unalterable log storage. When malicious data loss" came out as
// "not terrible lock storage when Malaysia did last": only two words after "log" lined up.
const logs = parseScript('Exact timestamps in an unalterable log storage. When malicious data loss occurs, our recovery starts.');
test('a misheard word you are on is skipped when the two after it line up',()=>{
 assert.deepEqual(follow(logs.lines, {line: 0, word: 5}, 'not terrible lock storage when'), {line: 0, word: 8, waiting: false});
});
test('one word that lines up past the word you are on is not enough to skip it',()=>{
 assert.deepEqual(follow(logs.lines, {line: 0, word: 5}, 'not terrible banana storage'), {line: 0, word: 5, waiting: true});
});
test('words that sound alike carry on from where you are',()=>{
 // Pronunciation is the report's job: "lock" for "log", "Malaysia did" for "malicious data".
 assert.deepEqual(follow(logs.lines, {line: 0, word: 5}, 'lock storage'), {line: 0, word: 7, waiting: false});
 assert.deepEqual(follow(logs.lines, {line: 0, word: 8}, 'when malaysia did'), {line: 0, word: 10, waiting: false});
});
test('a sound-alike alone, away from where you are, moves nothing',()=>{
 assert.deepEqual(follow(logs.lines, {line: 0, word: 0}, 'malaysia did'), {line: 0, word: 0, waiting: true});
});
test('three words that sound right just past the word you are on skip it',()=>{
 // On "unalterable", Apple heard "not terrible lock storage when".
 assert.deepEqual(follow(logs.lines, {line: 0, word: 4}, 'not terrible lock storage when'), {line: 0, word: 8, waiting: false});
});
// Rehearsing a meeting is not an English lesson: a word read wrong (not even close) must not
// hold you until you say it right; the next script word, said after the one before it, moves on.
test('a word read wrong is passed when you say the next one',()=>{
 assert.deepEqual(follow(logs.lines, {line: 0, word: 5}, 'an unalterable fog storage'), {line: 0, word: 7, waiting: false});
});
test('two words read as one wrong word are passed too',()=>{
 // On "unalterable", Apple heard "an unable storage" for "an unalterable log storage".
 assert.deepEqual(follow(logs.lines, {line: 0, word: 4}, 'in an unable storage'), {line: 0, word: 7, waiting: false});
});
test('passing a word still needs the script word before it, so an ad-lib does not move on',()=>{
 assert.deepEqual(follow(logs.lines, {line: 0, word: 5}, 'we keep fog storage'), {line: 0, word: 5, waiting: true});
});
test('three words that sound right pass two words read wrong',()=>{
 // On "unalterable", Apple heard "in storage when Malaysia": "unalterable log" were lost.
 assert.deepEqual(follow(logs.lines, {line: 0, word: 4}, 'in storage when malaysia'), {line: 0, word: 9, waiting: false});
});
// The owner's 05:17 rehearsal: Apple garbled whole stretches, with a few words right in between.
const garbled = parseScript('Our system handles a critical security challenge: insider threats and data sabotage. In real-world scenarios they might destroy or corrupt critical database records while their access privileges are still active. Our objective today.');
const gw = garbled.lines[0].words, after = w => ({line: 0, word: gw.indexOf(w) + 1, waiting: false});
test('a garbled stretch moves on when 3 words line up in order, as far apart as in the script',()=>{
 assert.deepEqual(follow(garbled.lines, {line: 0, word: gw.indexOf('insider')}, 'address and they sabotage'), after('sabotage'));
 assert.deepEqual(follow(garbled.lines, {line: 0, word: gw.indexOf('database')}, 'wreckers why are their answers were just still active'), after('active'));
});
test('script words far apart in what you say are not taken for reading on',()=>{
 const at = {line: 0, word: gw.indexOf('database')};
 assert.deepEqual(follow(garbled.lines, at, 'records are active'), {...at, waiting: true});
});
test('a garbled start of the script moves on too',()=>{
 assert.deepEqual(follow(garbled.lines, {line: 0, word: 0}, 'hour system and those a critical'), after('critical'));
});
// The owner's rule (2026-10-29): you need not read every word on the strip; it catches up with you.
const skip = parseScript('Our monitoring traced the incident directly to User ID emp_9821. By triggering our recovery workflow, the pipeline restores the exact snapshot.');
const sw = skip.lines[0].words, onEmp = {line: 0, word: sw.indexOf('emp')}, past = w => ({line: 0, word: sw.indexOf(w) + 1, waiting: false});
test('words you skip are passed when 2 after them line up',()=>{
 assert.deepEqual(follow(skip.lines, onEmp, 'by triggering'), past('triggering'));
 assert.deepEqual(follow(skip.lines, onEmp, 'user id by triggering'), past('triggering'));
 assert.deepEqual(follow(skip.lines, onEmp, 'id emp by triggering'), past('triggering'));
});
test('2 words further than 5 ahead do not move it',()=>{
 assert.deepEqual(follow(skip.lines, onEmp, 'the pipeline'), {...onEmp, waiting: true});
});
