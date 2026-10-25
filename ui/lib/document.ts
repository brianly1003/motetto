export function meetingGroup(time: number, now = new Date()) {
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const week = new Date(today);
  week.setDate(today.getDate() - (today.getDay() + 6) % 7);
  return time >= today.getTime() ? 'Today' : time >= week.getTime() ? 'This week' : 'Earlier';
}
// A recording keeps its placeholder until someone names it; show when it started instead.
export function meetingTitle(meeting: {title: string; created_ms: number}) {
  if (meeting.title !== 'Recorded meeting') return meeting.title;
  const time = new Intl.DateTimeFormat(undefined, {hour:'2-digit', minute:'2-digit', hour12:false}).format(new Date(meeting.created_ms));
  return `Meeting at ${time}`;
}
export function meetingDate(time: number, now = new Date()) {
  const date = new Date(time);
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const yesterday = new Date(today); yesterday.setDate(today.getDate() - 1);
  const week = new Date(today); week.setDate(today.getDate() - 6);
  // Mockup: "Today", "Yesterday", then the weekday ("Thu") for the last week, then the date.
  const day = time >= today.getTime() ? 'Today' : time >= yesterday.getTime() ? 'Yesterday' : new Intl.DateTimeFormat(undefined, time >= week.getTime() ? {weekday:'short'} : {month:'short', day:'numeric'}).format(date);
  return `${day} · ${new Intl.DateTimeFormat(undefined, {hour:'2-digit', minute:'2-digit', hour12:false}).format(date)}`;
}
export function timeLabel(ms: number) {
  const seconds = Math.floor(ms / 1000);
  return `${String(Math.floor(seconds / 60)).padStart(2,'0')}:${String(seconds % 60).padStart(2,'0')}`;
}
// Mockup durations: "22 s", "1 min 27 s" for short meetings, whole minutes from 10 min.
// "09:14", "09:14 yesterday", "09:14 on Thu": when a recording was cut off (Recover banner).
export function recordedAt(time: number, now = new Date()) {
  const [day, clock] = meetingDate(time, now).split(' · ');
  return day === 'Today' ? clock : day === 'Yesterday' ? `${clock} yesterday` : `${clock} on ${day}`;
}
export function durationLabel(ms: number) {
  const seconds = Math.floor(ms / 1000), minutes = Math.floor(seconds / 60);
  return seconds < 60 ? `${seconds} s` : minutes < 10 ? `${minutes} min ${seconds % 60} s` : `${minutes} min`;
}
/** How long the notes took: "48 s", "13 min 40 s", "1 h 5 min" (owner 2026-10-25: "820 s" said little). */
export function notesTime(ms: number) {
  const seconds = Math.round(ms / 1000), minutes = Math.floor(seconds / 60);
  return seconds < 60 ? `${seconds} s` : minutes < 60 ? `${minutes} min ${seconds % 60} s` : `${Math.floor(minutes / 60)} h ${minutes % 60} min`;
}
export function fileSize(bytes: number) {
  return bytes >= 1e9 ? `${(bytes/1e9).toFixed(1)} GB` : bytes >= 1e6 ? `${(bytes/1e6).toFixed(1)} MB` : `${(bytes/1e3).toFixed(1)} KB`;
}
// notes.md holds the transcript in a <details> block; My notes may follow it (recording.rs).
export function withoutTranscript(markdown: string) {
  return markdown.replace(/<details><summary>Transcript<\/summary>[\s\S]*?(<\/details>|$)/, '');
}
// Times the user typed in My notes are real call times, so they may seek like transcript times.
export function myNoteTimes(markdown: string) {
  const mine = /^#{2,3} My notes[ \t]*$([\s\S]*)/m.exec(withoutTranscript(markdown))?.[1] ?? '';
  return [...mine.matchAll(/^\s*\[(\d+):(\d{2})\]/gm)].map(match => (Number(match[1])*60+Number(match[2]))*1000);
}
export function noteSections(markdown: string) {
  // `line` is the heading's own line (-1 before the first heading), so a topic title can be edited.
  const sections: {title:string; line:number; lines:{text:string; line:number}[]}[] = [];
  let section = {title:'', line:-1, lines:[] as {text:string; line:number}[]};
  let fence = false, transcript = false;
  // Skip the transcript block but keep line numbers: ticking a task rewrites its original line.
  markdown.split('\n').forEach((text, line) => {
    if (text.includes('<details><summary>Transcript</summary>')) transcript = true;
    if (transcript) { if (text.includes('</details>')) transcript = false; return; }
    if (/^\s*(```|~~~)/.test(text)) fence = !fence;
    const heading = !fence && /^#{1,6}\s+(.+?)\s*#*\s*$/.exec(text);
    if (heading) { if(section.title || section.lines.length) sections.push(section); section = {title:heading[1],line,lines:[]}; }
    else section.lines.push({text,line});
  });
  if(section.title || section.lines.length) sections.push(section);
  return sections.filter(s => s.title || s.lines.some(l=>l.text.trim()));
}
export function taskLine(text: string) {
  const match = /^\s*[-*] \[([ xX])\] (.*)$/.exec(text);
  if (!match) return null;
  const owner = /^(?:\*\*)?([^:*\[\]\n]{1,40})(?:\*\*)?:\s+(.*)$/.exec(match[2]);
  return {done: match[1] !== ' ', owner: owner?.[1] ?? '', text: owner?.[2] ?? match[2]};
}
