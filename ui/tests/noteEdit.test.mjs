import {test} from 'node:test';
import assert from 'node:assert/strict';
import {replaceBlock, splitBlock, assignBlock, cleanName, renameOwner, removeLine} from '../lib/noteEdit.ts';
const md='### Action items\n- [ ] **Kevin**: Check the project [00:56]\n\n### Overview\n- Compared **FCM**.\n- Second\n\nA paragraph\nwrapped\n\n### My notes\n[01:01] Kevin decides.';
test('an edit keeps the parts the user does not type',()=>{
 assert.equal(replaceBlock(md,1,2,'task','Check the 2021 project [00:56]').split('\n')[1],'- [ ] **Kevin**: Check the 2021 project [00:56]');
 assert.equal(replaceBlock(md,4,5,'li','Compared **FCM** twice.').split('\n')[4],'- Compared **FCM** twice.');
 assert.equal(replaceBlock(md,0,1,'heading','Tasks').split('\n')[0],'### Tasks');
 assert.equal(replaceBlock(md,11,12,'jot','Kevin owns it.').split('\n')[11],'[01:01] Kevin owns it.');
 assert.equal(replaceBlock(md,7,9,'p','One  line\n now').split('\n').slice(7,9).join('|'),'One line|now');
});
test('trailing blank lines stay and an emptied block goes',()=>{
 const out=replaceBlock(md,5,7,'li','');
 assert.equal(out.split('\n').length,md.split('\n').length-1);
 assert.ok(!out.includes('- Second'));
 assert.equal(out.split('\n')[5],'');
});
test('shorthands in a paragraph become Markdown',()=>{
 assert.equal(replaceBlock(md,7,9,'p','[] Ship it').split('\n')[7],'- [ ] Ship it');
 assert.equal(replaceBlock(md,7,9,'p','# Risks').split('\n')[7],'### Risks');
});
test('[] at the start of a bullet makes it a task',()=>{
 assert.equal(replaceBlock(md,5,7,'li','[] Work in progress [00:01]').split('\n')[5],'- [ ] Work in progress [00:01]');
 assert.equal(replaceBlock(md,5,7,'li','[ ] Also this').split('\n')[5],'- [ ] Also this');
 assert.equal(replaceBlock(md,5,7,'li','[]not a task').split('\n')[5],'- []not a task');
});
test('the @ picker sets one owner and keeps the file form',()=>{
 assert.equal(assignBlock(md,1,2,'task','Check the project [00:56]','Linh Tran').split('\n')[1],'- [ ] Linh Tran: Check the project [00:56]');
 const done=md.replace('- [ ] **Kevin**','- [x] **Kevin**');
 assert.equal(assignBlock(done,1,2,'task','Check it','You').split('\n')[1],'- [x] You: Check it');
 assert.equal(assignBlock(md,5,7,'li','Second','Kevin').split('\n')[5],'- [ ] Kevin: Second');
 assert.equal(assignBlock(md,7,9,'p','[] A paragraph wrapped','Kevin').split('\n')[7],'- [ ] Kevin: A paragraph wrapped');
 assert.equal(cleanName(' Kev:in [x] '),'Kevin x');
});
test('Enter splits into a block of the same kind',()=>{
 assert.deepEqual(splitBlock(md,1,2,'task','Check the','project [00:56]').split('\n').slice(1,3),['- [ ] **Kevin**: Check the','- [ ] project [00:56]']);
 assert.deepEqual(splitBlock(md,5,7,'li','Second','').split('\n').slice(5,7),['- Second','- ']);
 assert.deepEqual(splitBlock(md,7,9,'p','A paragraph','wrapped').split('\n').slice(7,10),['A paragraph','','wrapped']);
 assert.equal(splitBlock(md,7,9,'p','A paragraph wrapped',''),null);
});
test('Fix this name renames every task of that owner; reassigning changes one',()=>{
 const doc='- [ ] Kenvin: a\n- [x] **Kenvin**: b\n- [ ] Linh: c\n- [ ] no owner';
 assert.equal(renameOwner(doc,'Kenvin','Kevin',null),'- [ ] Kevin: a\n- [x] Kevin: b\n- [ ] Linh: c\n- [ ] no owner');
 assert.equal(renameOwner(doc,'Kenvin','Linh',0),'- [ ] Linh: a\n- [x] **Kenvin**: b\n- [ ] Linh: c\n- [ ] no owner');
 assert.equal(renameOwner('- [ ] Me: x','You','Anna',null),'- [ ] Anna: x');
});
test('Remove takes the line out; an emptied section says None or goes',()=>{
 const notes='### Action items\n- [ ] Kevin: Check it [00:56]\n\n### Decisions\n- Move to the paid plan [01:00]\n\n### Overview\n- A call.\n\n<details><summary>Transcript</summary>\n</details>';
 assert.equal(removeLine(notes,1),notes.replace('- [ ] Kevin: Check it [00:56]','- None'));
 assert.equal(removeLine(notes,4),notes.replace('### Decisions\n- Move to the paid plan [01:00]\n\n',''));
 const two='### Decisions\n- One [00:01]\n- Two [00:02]';
 assert.equal(removeLine(two,1),'### Decisions\n- Two [00:02]');
});
