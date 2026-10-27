import {test} from 'node:test';
import assert from 'node:assert/strict';
import {defaultPick,pillLabel,sizeLabel,shareEnded,partAt,retentionLabel} from '../lib/screen.ts';
const window={choice:{kind:'CallWindow',app_id:'zoom'},name:'Zoom window'},display={choice:{kind:'Display',id:1},name:'Display'};
test('screen defaults, labels and share-end transitions',()=>{
 const options=[{pick:window,sub:''},{pick:display,sub:''}];
 assert.deepEqual(defaultPick(options,'call_window'),window);assert.deepEqual(defaultPick(options,'display'),display);assert.equal(defaultPick([],'display'),null);
 assert.equal(pillLabel(null),'Record the screen too');assert.equal(pillLabel({pick:window,while_sharing:null,waiting:false}),'Recording the screen: Zoom window');assert.equal(sizeLabel('smaller'),'70 MB');
 assert.ok(shareEnded({screen:{while_sharing:display}},{sharing:null}));assert.ok(!shareEnded({screen:{while_sharing:null}},{sharing:null}));
});
test('video mapping uses start-inclusive/end-exclusive part intervals and preserves gaps',()=>{
 const videos=[{offset_ms:0,duration_ms:30000},{offset_ms:72000,duration_ms:15000}];
 assert.equal(partAt(videos,10000).fileMs,10000);assert.equal(partAt(videos,30000),null);assert.equal(partAt(videos,50000),null);assert.equal(partAt(videos,80000).fileMs,8000);assert.equal(partAt(videos,87000),null);
});

test('video retention always respects the audio limit',()=>{
 assert.match(retentionLabel('after_notes','forever'),/right after/);
 assert.match(retentionLabel('30_days','forever'),/30 days/);
 assert.match(retentionLabel('30_days','7_days'),/7 days/);
 assert.match(retentionLabel('forever','same_as_audio'),/until you delete/);
});
