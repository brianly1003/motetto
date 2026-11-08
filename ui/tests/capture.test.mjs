import {test} from 'node:test';
import assert from 'node:assert/strict';
import {capMode,capSet,fromMemory,memoryOf,recordSetup,screenPick} from '../lib/capture.ts';
const memory={mode:'window',mic:{kind:'Off'},app_sounds:false,notes:false,timer:3,clicks:true,area:null,remember:true};
const view={w:1440,h:900};
test('notes always have something to hear',()=>{
 // Audio only writes notes: with no mic and app sounds off, app sounds come back on.
 const cap=fromMemory(memory,null,false,view);
 assert.equal(cap.sys,false);
 const audio=capMode(cap,'audio');
 assert.equal(audio.notes,true);assert.equal(audio.sys,true);
 const opened=fromMemory({...memory,mode:'audio'},null,false,view);
 assert.equal(opened.notes,true);assert.equal(opened.sys,true);
 // A mic is something to hear: app sounds stay as chosen.
 assert.equal(capMode({...cap,mic:{kind:'Default'}},'audio').sys,false);
});
test('an area on no display in particular is on the main display',()=>{
 const cap={...fromMemory(memory,'area',false,view),area:{display:null,x:10,y:20,w:300,h:200}};
 const main={id:1,name:'Built-in',w:2880,h:1800};
 // Picked on a secondary display's overlay (id 2): still the main display's area.
 assert.deepEqual(screenPick(cap,1,2,main).choice,{kind:'Area',display:1,x:10,y:20,width:300,height:200});
});

test('a picture chosen on the setup goes with the recording; none follows Settings', () => {
 const view={w:1400,h:1000};
 const cap=fromMemory(memory,null,false,view);
 assert.equal(cap.picture,null);
 assert.equal(fromMemory({...memory,picture:'huge'},null,false,view).picture,null);
 const high=capSet(cap,'pic:high');
 assert.equal(high.picture,'high');
 assert.equal(capSet(high,'pic:huge').picture,'high');
 const pick={choice:{kind:'Display',id:1},name:'Built-in Display'};
 assert.equal(recordSetup(high,pick).picture,'high');
 assert.equal('picture' in recordSetup(cap,pick),false);
 assert.equal('picture' in recordSetup(high,null),false);
 assert.equal(memoryOf(high).picture,'high');
 assert.equal(fromMemory(memoryOf(high),null,false,view).picture,'high');
});
