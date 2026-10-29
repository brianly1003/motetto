import { test } from 'node:test';
import assert from 'node:assert/strict';
import { scriptWriter } from '../lib/scriptWriter.ts';

test('unchanged text and repeated close do not touch the saved version', async () => {
  const calls = [];
  const write = scriptWriter('Demo', 'Hello', 10, async (...args) => { calls.push(args); return 11; });
  assert.equal(await write('Hello'), 10);
  await Promise.all([write('Edited'), write('Edited')]);
  assert.deepEqual(calls, [['Demo', 'Edited', 10]]);
});

test('overlapping saves wait for the preceding version', async () => {
  const calls = [];
  let finish;
  const write = scriptWriter('Demo', 'Hello', 10, async (...args) => {
    calls.push(args);
    if (calls.length === 1) await new Promise(resolve => { finish = resolve; });
    return 10 + calls.length;
  });
  const first = write('One');
  const second = write('Two');
  await Promise.resolve();
  assert.deepEqual(calls, [['Demo', 'One', 10]]);
  finish();
  await Promise.all([first, second]);
  assert.deepEqual(calls, [['Demo', 'One', 10], ['Demo', 'Two', 11]]);
});

test('a real conflict preserves the base and a failed write remains retryable', async () => {
  const calls = [];
  let fail = true;
  const write = scriptWriter('Demo', 'Hello', 10, async (...args) => {
    calls.push(args);
    if (fail) throw Error('Changed in another window');
    return 11;
  });
  await assert.rejects(write('Draft'), /Changed in another window/);
  fail = false;
  assert.equal(await write('Draft'), 11);
  assert.deepEqual(calls, [['Demo', 'Draft', 10], ['Demo', 'Draft', 10]]);
});
