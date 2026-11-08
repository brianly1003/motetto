import {test} from 'node:test';
import assert from 'node:assert/strict';
import {lengthChip,rateChip,clippedChip,peakLabel,snap,lineRange,wordsInRange,lineInClip,defaultRange,clipFileName,lineTrack} from '../lib/clip.ts';

const lines=[
  {ms:7000,speaker:'You',text:'As you already know, FCM is push.'},
  {ms:16000,speaker:'You',text:'Beams relies on FCM.'},
  {ms:28000,speaker:'Them',text:'So Beams adds reporting?'},
  {ms:null,speaker:'',text:'(untimed note)'},
  {ms:36000,speaker:'Speaker 2',text:'Yes, it does.'},
];

test('length chip thresholds follow SPEC 15.3',()=>{
  assert.deepEqual([2.9,3,4.9,5,10,10.1,12,12.1].map(s=>lengthChip(s).label),['Under 3 s','Under 5 s','Under 5 s','5-10 s · voice sample','5-10 s · voice sample','Over 10 s','Over 10 s','Over 12 s']);
  assert.deepEqual([2,4,7,11,13].map(s=>lengthChip(s).tone),['rec','amber','ok','amber','rec']);
});

test('quality chips',()=>{
  assert.deepEqual(rateChip(48000,32,true),{tone:'ok',label:'48 kHz · 32-bit float'});
  assert.deepEqual(rateChip(44100,16,false),{tone:'ok',label:'44.1 kHz · 16-bit integer'});
  assert.deepEqual(rateChip(16000,32,true),{tone:'amber',label:'16 kHz · sounds like a phone line'});
  assert.deepEqual([clippedChip(0).label,clippedChip(1).label,clippedChip(3).label],['No clipping','Clipped 1 time','Clipped 3 times']);
  assert.deepEqual([peakLabel(-5.84),peakLabel(0.4),peakLabel(null)],['Peak −5.8 dB','Peak 0.4 dB','Silent']);
});

test('snap lands on the quietest 20 ms within 400 ms, else stays',()=>{
  // 1 s close-up from 1000 ms in 100 bins of 10 ms; quiet at bins 40-41 (1400-1420 ms).
  const levels=Array.from({length:100},(_,i)=>i===40||i===41?0.01:0.8);
  assert.equal(snap(levels,1000,2000,1300),1410);
  assert.equal(snap(levels,1000,2000,1900),1900); // no pause in reach: stays
  assert.equal(snap(levels,1000,2000,2500),2500); // outside the close-up
  assert.equal(snap([],0,1000,300),300);
});

test('a line runs to the next timed line; words follow the track',()=>{
  assert.deepEqual(lineRange(lines,2,90000),{from:28000,to:36000});
  assert.deepEqual(lineRange(lines,4,90000),{from:36000,to:90000});
  assert.equal(lineRange(lines,3,90000),null);
  assert.equal(wordsInRange(lines,0,30000,'Me'),'As you already know, FCM is push. Beams relies on FCM.');
  assert.equal(wordsInRange(lines,0,40000,'Them'),'So Beams adds reporting? Yes, it does.');
  assert.equal(wordsInRange(lines,15000,30000,'Both'),'Beams relies on FCM. So Beams adds reporting?');
  assert.equal(lineTrack('Speaker 2'),'Them');
  assert.ok(lineInClip(lines,1,90000,20000,22000,'Me'));
  assert.ok(!lineInClip(lines,2,90000,20000,22000,'Me'));
  assert.ok(!lineInClip(lines,0,90000,20000,22000,'Both'));
});

test('default range and file name',()=>{
  assert.deepEqual(defaultRange(lines,'Me',90000),{from:7000,to:16000});
  assert.deepEqual(defaultRange(lines,'Them',90000),{from:28000,to:36000});
  assert.deepEqual(defaultRange([],'Me',5000),{from:0,to:5000});
  assert.equal(clipFileName('FCM vs Pusher: Beams',74900),'FCM vs Pusher- Beams 01-14.wav');
  assert.equal(clipFileName('  ',0),'Clip 00-00.wav');
});
