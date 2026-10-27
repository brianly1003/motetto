import type {ScreenOption} from './bindings/ScreenOption';
import type {ScreenState} from './bindings/ScreenState';
import type {AppState} from './bindings/AppState';
import type {VideoPart} from './bindings/VideoPart';
export function defaultPick(options: ScreenOption[], setting: string) {
  return (options.find(o=>o.pick.choice.kind===(setting==='display'?'Display':'CallWindow'))??options[0])?.pick??null;
}
export function pillLabel(screen: ScreenState|null) { return screen?`Recording the screen: ${(screen.while_sharing??screen.pick).name}`:'Record the screen too'; }
export function sizeLabel(preset: string) { return preset==='smaller'?'70 MB':'110 MB'; }
export function shareEnded(previous: AppState|null, next: AppState) { return !!previous?.screen?.while_sharing && !next.sharing; }
export function partAt(videos: VideoPart[], ms: number) { const part=videos.find(p=>ms>=p.offset_ms&&ms<p.offset_ms+p.duration_ms);return part?{part,fileMs:ms-part.offset_ms}:null; }

export function retentionLabel(audio: string, video: string) {
  if(audio==='after_notes')return 'Deleted right after the notes, like the audio.';
  if(video==='7_days')return 'Kept for 7 days after the notes.';
  return audio==='forever'?'Kept until you delete the meeting.':'Kept for 30 days after the notes, like the audio.';
}
