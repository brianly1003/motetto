import {test} from 'node:test';
import assert from 'node:assert/strict';
import {noneItem,tidyNone} from '../lib/document.ts';

test('a placeholder "nothing" is None, a real item is not (same list as summarize::none_item)',()=>{
  for(const none of ['None','<none>','&lt;none>','(none)','None.','N/A','*None*','nothing']) assert.ok(noneItem(none),none);
  for(const real of ['None of the options fit','Keep FCM [00:10]','No']) assert.ok(!noneItem(real),real);
});

test('saved notes show every "nothing" bullet as "- None", keeping the line count',()=>{
  const notes='### Action items\n- None\n### Decisions\n- &lt;none>\n### Overview\n- A sound check.';
  const tidy=tidyNone(notes);
  assert.equal(tidy,'### Action items\n- None\n### Decisions\n- None\n### Overview\n- A sound check.');
  assert.equal(tidy.split('\n').length,notes.split('\n').length);
});
