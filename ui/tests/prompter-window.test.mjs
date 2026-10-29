import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readingWindow} from '../lib/prompter.ts';
// Five rendered rows of one paragraph, 30 px apart, starting at words 0, 8, 16, 24, 32.
const rows = [0, 8, 16, 24, 32].map((word, i) => ({word, top: i * 30}));
const view = (word, start, per = 2) => {
  const v = readingWindow(rows, per, word, 30, start);
  return {start: v.start, top: v.top, to: v.to, last: v.last};
};
test('the paragraph opens on its first rows',()=>{
 assert.deepEqual(view(3), {start: 0, top: 0, to: 16, last: false});
});
test('the text stays still while you read the bottom row',()=>{
 assert.deepEqual(view(10, 0), {start: 0, top: 0, to: 16, last: false});
});
test('near the end of the bottom row it rolls one row, so the next line is ready',()=>{
 // 3 words left on row 1 ("…preparing to"): row 2 comes into view below it.
 assert.deepEqual(view(13, 0), {start: 8, top: 30, to: 24, last: false});
});
test('the highlight entering the row you already read does not move the text',()=>{
 assert.deepEqual(view(17, 8), {start: 8, top: 30, to: 24, last: false});
});
test('going back shows the row you went back to',()=>{
 assert.deepEqual(view(3, 16), {start: 0, top: 0, to: 16, last: false});
});
test('the paragraph end in view says so, so the next paragraph can preview',()=>{
 assert.deepEqual(view(29, 16), {start: 24, top: 90, to: Infinity, last: true});
});
test('one row at a time when only one fits',()=>{
 assert.deepEqual(view(10, 0, 1), {start: 8, top: 30, to: 16, last: false});
});
test('the page counter follows the row being read',()=>{
 const v = readingWindow(rows, 2, 17, 30, 8);
 assert.deepEqual([v.page, v.total], [1, 3]);
});
