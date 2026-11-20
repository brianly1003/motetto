import {test} from 'node:test';
import assert from 'node:assert/strict';
import {voiceOf, personOf, voiceColor, speakerLabel, namedTranscript, notesMentions, renameInNotes, saidNames} from '../lib/speakerNames.ts';

test('a named voice reads as its name; voices sharing a name are one person',()=>{
  const names={2:'Kevin',5:'Kevin',3:'Linh'};
  assert.equal(voiceOf('Speaker 12'),12);assert.equal(voiceOf('You'),0);assert.equal(voiceOf('Them'),0);
  assert.equal(speakerLabel('Speaker 3',names),'Linh');assert.equal(speakerLabel('Speaker 4',names),'Speaker 4');assert.equal(speakerLabel('You',names),'You');
  assert.equal(personOf(5,names),2);assert.equal(personOf(2,names),2);assert.equal(personOf(4,names),4);
  assert.equal(voiceColor(2),'var(--voice-them)');assert.equal(voiceColor(3),'var(--voice-3)');assert.equal(voiceColor(7),'var(--voice-3)');
});

test('Copy and the notes take the name; Speaker 21 is not Speaker 2',()=>{
  const text='[00:01] Me: hi\n[00:02] Speaker 2: ask Speaker 2\n[01:02:03] Speaker 21: yes';
  assert.equal(namedTranscript(text,{2:'Kevin'}),'[00:01] Me: hi\n[00:02] Kevin: ask Speaker 2\n[01:02:03] Speaker 21: yes');
  const notes='- [ ] Speaker 2: send it\nSpeaker 21 agreed with Speaker 2.';
  assert.equal(notesMentions(notes,2),2);
  const renamed=renameInNotes(notes,2,'Kevin');
  assert.equal(renamed,'- [ ] Kevin: send it\nSpeaker 21 agreed with Kevin.');
  // The transcript copy at the end of the notes is left alone: a notes save may not change it.
  const withCopy='- Speaker 2 owns it\n<details><summary>Transcript</summary>\n\n[00:02] Speaker 2: hi\n</details>\nSpeaker 2 again';
  assert.equal(notesMentions(withCopy,2),2);
  assert.equal(renameInNotes(withCopy,2,'Holmet'),'- Holmet owns it\n<details><summary>Transcript</summary>\n\n[00:02] Speaker 2: hi\n</details>\nHolmet again');
});

test('names said in the call are suggested, most heard first',()=>{
  assert.deepEqual(saidNames(['Anh Met với chị Kate cái case','Kate ơi, nói team TSM.','And I will ask Linh.','Ok, so','Thanks, Kate']),['Kate','Met','Linh']);
  assert.deepEqual(saidNames(['nothing here']),[]);
});
