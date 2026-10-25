import {test} from 'node:test';
import assert from 'node:assert/strict';
import {meetingGroup, meetingDate, recordedAt, noteSections, taskLine, timeLabel, durationLabel, notesTime, myNoteTimes, withoutTranscript} from '../lib/document.ts';
test('My notes after the transcript block still show, and their times may seek',()=>{
 const md='### Action items\n- [ ] You: Brush [01:00]\n\n<details><summary>Transcript</summary>\n\n[00:12] Me: hi\n### Not a section\n\n</details>\n\n### My notes\n\n[00:28] > quote\n[01:00] [] Brush\n';
 const sections=noteSections(md);
 assert.deepEqual(sections.map(s=>s.title),['Action items','My notes']);
 assert.equal(sections[1].lines.find(l=>l.text.startsWith('[01:00]')).line,13);
 assert.deepEqual(myNoteTimes(md),[28000,60000]);
 assert.doesNotMatch(withoutTranscript(md),/Me: hi/);
 assert.deepEqual(myNoteTimes('### Overview\n- [00:05] made up'),[]);
});
test('calendar groups use local midnight and Monday week boundaries',()=>{
 const now=new Date(2026,9,21,12);
 assert.equal(meetingGroup(new Date(2026,9,21).getTime(),now),'Today');
 assert.equal(meetingGroup(new Date(2026,9,19).getTime(),now),'This week');
 assert.equal(meetingGroup(new Date(2026,9,18).getTime(),now),'Earlier');
 assert.match(meetingDate(new Date(2026,9,20,13,11).getTime(),now),/^Yesterday · 13:11$/);
 const weekday=new Intl.DateTimeFormat(undefined,{weekday:'short'}).format(new Date(2026,9,15));
 assert.equal(meetingDate(new Date(2026,9,15,9,30).getTime(),now),`${weekday} · 09:30`);
 assert.doesNotMatch(meetingDate(new Date(2026,9,14,9,30).getTime(),now),new RegExp(`^${weekday}`));
 assert.equal(timeLabel(87000),'01:27');
 assert.deepEqual([22900,87000,599999,2580000].map(durationLabel),['22 s','1 min 27 s','9 min 59 s','43 min']);
 // Notes in: 820 s reads as 13 min 40 s; an hour or more drops the seconds.
 assert.deepEqual([48000,819796,3900000].map(notesTime),['48 s','13 min 40 s','1 h 5 min']);
});
test('task positions preserve original Markdown line numbers and owners are explicit',()=>{
 const sections=noteSections('### Action items\n- [ ] You: Send it [01:20]\n\n### Overview\n- Text\n<details><summary>Transcript</summary>\n### Not a section');
 assert.equal(sections.length,2);
 assert.equal(sections[0].lines[0].line,1);
 assert.deepEqual(sections.map(s=>s.line),[0,3]);
 assert.deepEqual(taskLine(sections[0].lines[0].text),{done:false,owner:'You',text:'Send it [01:20]'});
 assert.equal(taskLine('- None'),null);
 assert.equal(taskLine('- [x] Send it').owner,'');
 assert.equal(noteSections('### Topic\n```\n### Code\n```').length,1);
});
test('the Recover banner says when the recording was cut off',()=>{
 const now=new Date(2026,9,21,12);
 assert.equal(recordedAt(new Date(2026,9,21,9,14).getTime(),now),'09:14');
 assert.equal(recordedAt(new Date(2026,9,20,9,14).getTime(),now),'09:14 yesterday');
 const weekday=new Intl.DateTimeFormat(undefined,{weekday:'short'}).format(new Date(2026,9,16));
 assert.equal(recordedAt(new Date(2026,9,16,9,14).getTime(),now),`09:14 on ${weekday}`);
});
