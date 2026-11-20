//! Find speakers on a written transcript: the call side is split into voices from the saved audio
//! without transcribing again, so the user's corrections stay. The heard transcript and its edited
//! copy change together: same lines, same prefixes, as the edited copy requires.

use std::error::Error;
use std::path::Path;

use crate::correct::prefix;
use crate::diarize::{self, Turn};

/// A line is taken to end where the next call-side line starts, or this long after its own start.
const LONGEST_LINE_MS: u64 = 30_000;

/// The transcript with speakers, and how many voices were heard. None from `find` when fewer than
/// two voices were heard: there is nothing to change.
#[derive(Debug, PartialEq)]
pub struct Found {
    pub heard: String,
    pub edited: Option<String>,
    pub voices: usize,
}

/// Runs the diarizer on every saved track that is not the microphone (`parts`: sources and their
/// offset on the meeting's timeline, as the transcription takes them) and relabels `heard` and
/// `edited`. `people`: how many other people were on the call, when the user said; it makes the
/// voices far more reliable than the diarizer's own guess. An error when the speaker models are
/// not set up or the diarizer fails.
pub fn find(
    parts: &[(Vec<String>, u64)],
    heard: &str,
    edited: Option<&str>,
    people: Option<usize>,
) -> Result<Option<Found>, Box<dyn Error>> {
    let mic = parts
        .iter()
        .any(|(sources, _)| sources.iter().any(|s| s.ends_with(".mic.wav")));
    let speech = saved_speech(parts);
    if people == Some(1) {
        return Ok(with_turns(
            one_voice(heard, edited, mic),
            heard,
            edited,
            &speech,
        ));
    }
    let mut turns = vec![];
    for (sources, offset) in parts {
        for source in sources {
            if source.ends_with(".mic.wav") {
                continue;
            }
            let found = diarize::turns(Path::new(source), None, people);
            let found = found?.ok_or("the speaker models are not set up")?;
            turns.extend(found.into_iter().map(|t| t.shifted(*offset)));
        }
    }
    Ok(with_turns(
        relabel(heard, edited, &turns, mic),
        heard,
        edited,
        &speech,
    ))
}

/// Speech (ms intervals) heard on the mic and in the call audio.
type Sides = (Vec<(u64, u64)>, Vec<(u64, u64)>);

/// `found` (or the transcript as it was) with long lines cut where the other side took a turn;
/// None when neither changed anything.
fn with_turns(
    found: Option<Found>,
    heard: &str,
    edited: Option<&str>,
    speech: &Option<Sides>,
) -> Option<Found> {
    let Some((me, them)) = speech else {
        return found;
    };
    let (text, copy) = found
        .as_ref()
        .map_or((heard, edited), |f| (f.heard.as_str(), f.edited.as_deref()));
    match interleave(text, copy, me, them) {
        Some((heard, edited)) => Some(Found {
            heard,
            edited,
            voices: found.map_or(1, |f| f.voices),
        }),
        None => found,
    }
}

/// The speech heard on each side (mic, call audio) of a saved recording, on the meeting's
/// timeline, from the copies the transcription left: the mic's denoised or echo-cancelled copy
/// when there is one, so the call heard through loudspeakers doesn't count as the user. None
/// without the speech model, a copy or a microphone track: lines are then not cut.
fn saved_speech(parts: &[(Vec<String>, u64)]) -> Option<Sides> {
    let vad = crate::config::var("WHISPER_VAD_MODEL").filter(|v| !v.is_empty())?;
    let (mut me, mut them) = (vec![], vec![]);
    for (sources, offset) in parts {
        for source in sources {
            let stem = source.strip_suffix(".wav")?;
            let (copies, side) = if source.ends_with(".mic.wav") {
                (vec!["clean", "aec", "16k"], &mut me)
            } else {
                (vec!["16k"], &mut them)
            };
            let copy = copies
                .iter()
                .map(|c| format!("{stem}.{c}.wav"))
                .find(|c| Path::new(c).exists())?;
            let speech = crate::slice::speech(Path::new(&copy), &vad).ok()?;
            side.extend(speech.into_iter().map(|(s, e)| (s + offset, e + offset)));
        }
    }
    (!me.is_empty() && !them.is_empty()).then_some((me, them))
}

/// The transcript with each named voice's `Speaker N: ` label replaced by its name (`names` maps
/// voice numbers to names), for the notes prompt and Copy. Other lines are unchanged.
pub fn named(text: &str, names: &std::collections::BTreeMap<u32, String>) -> String {
    if names.is_empty() {
        return text.to_owned();
    }
    text.split('\n')
        .map(|line| {
            let head = prefix(line);
            let Some(time_end) = head.find("] ").map(|i| i + 2) else {
                return line.to_owned();
            };
            let voice = head[time_end..]
                .strip_prefix("Speaker ")
                .and_then(|rest| rest.strip_suffix(": "))
                .and_then(|n| n.parse::<u32>().ok());
            match voice.and_then(|n| names.get(&n)) {
                Some(name) => format!("{}{name}: {}", &head[..time_end], &line[head.len()..]),
                None => line.to_owned(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every call-side line as one voice, `Speaker 2` (`Speaker 1` in an import without a microphone
/// track), as the user said one other person was on the call: the diarizer's guess often splits one
/// voice over a compressed call into several, and a numbered voice can be given a name. None when
/// nothing changes.
fn one_voice(heard: &str, edited: Option<&str>, mic: bool) -> Option<Found> {
    let label = if mic { "Speaker 2: " } else { "Speaker 1: " };
    let relabel = |text: &str| {
        text.split('\n')
            .map(|line| {
                let head = prefix(line);
                match head.find("] ").map(|i| i + 2) {
                    Some(time_end) if call_side(head) => {
                        format!("{}{label}{}", &head[..time_end], &line[head.len()..])
                    }
                    _ => line.to_owned(),
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let (new_heard, new_edited) = (relabel(heard), edited.map(relabel));
    (new_heard != heard || new_edited.as_deref() != edited).then_some(Found {
        heard: new_heard,
        edited: new_edited,
        voices: 1,
    })
}

/// The call side's lines: `Them`, an earlier run's `Speaker N`, or an unlabelled import's.
fn call_side(head: &str) -> bool {
    !head.is_empty() && !head.ends_with("] Me: ")
}

/// `[mm:ss]` (minutes can pass 99) as milliseconds.
fn millis(head: &str) -> Option<u64> {
    let time = head.strip_prefix('[')?.split(']').next()?;
    let (m, s) = time.split_once(':')?;
    Some((m.parse::<u64>().ok()? * 60 + s.parse::<u64>().ok()?) * 1000)
}

fn stamp(ms: u64) -> String {
    format!("[{:02}:{:02}] ", ms / 60_000, ms / 1000 % 60)
}

/// One call-side line's plan: the pieces it is cut into (one when whole), in time order.
struct Line {
    index: usize,
    pieces: Vec<(u64, u64)>,
    /// Where the cuts fall in its text, as fractions of its speech; empty when whole.
    cuts: Vec<f64>,
}

fn relabel(heard: &str, edited: Option<&str>, turns: &[Turn], mic: bool) -> Option<Found> {
    let heard_lines: Vec<&str> = heard.split('\n').collect();
    let edited_lines: Option<Vec<&str>> = edited
        .map(|e| e.split('\n').collect::<Vec<_>>())
        .filter(|e| e.len() == heard_lines.len());
    let starts: Vec<(usize, u64)> = heard_lines
        .iter()
        .enumerate()
        .filter(|(_, line)| call_side(prefix(line)))
        .filter_map(|(i, line)| Some((i, millis(prefix(line))?)))
        .collect();
    let mut lines = vec![];
    for (n, &(index, start)) in starts.iter().enumerate() {
        let end = starts
            .get(n + 1)
            .map_or(start + LONGEST_LINE_MS, |next| next.1.max(start + 1))
            .min(start + LONGEST_LINE_MS);
        let pieces = diarize::pieces(turns, start, end);
        let cuts = fractions(turns, start, end, &pieces);
        let texts = [
            Some(heard_lines[index]),
            edited_lines.as_ref().map(|e| e[index]),
        ];
        let splits = texts
            .iter()
            .flatten()
            .all(|line| split_text(&line[prefix(line).len()..], &cuts).is_some());
        lines.push(if pieces.len() > 1 && splits {
            Line {
                index,
                pieces,
                cuts,
            }
        } else {
            Line {
                index,
                pieces: vec![(start, end)],
                cuts: vec![],
            }
        });
    }
    let spans: Vec<(u64, u64)> = lines.iter().flat_map(|l| l.pieces.clone()).collect();
    let (ids, voices) = diarize::assign(turns, &spans)?;
    if voices < 2 {
        return None;
    }
    let first = if mic { 2 } else { 1 };
    let mut ids = ids.into_iter();
    let mut plans: Vec<Option<(&Line, Vec<usize>)>> =
        (0..heard_lines.len()).map(|_| None).collect();
    for line in &lines {
        let who = ids.by_ref().take(line.pieces.len()).collect();
        plans[line.index] = Some((line, who));
    }
    let build = |source: &[&str]| -> Vec<String> {
        source
            .iter()
            .zip(&plans)
            .flat_map(|(line, plan)| match plan {
                None => vec![(*line).to_owned()],
                Some((plan, who)) => {
                    let text = &line[prefix(line).len()..];
                    let texts = if plan.cuts.is_empty() {
                        vec![text.to_owned()]
                    } else {
                        split_text(text, &plan.cuts).unwrap_or_else(|| vec![text.to_owned()])
                    };
                    let head = prefix(line);
                    texts
                        .into_iter()
                        .zip(plan.pieces.iter().zip(who))
                        .enumerate()
                        .map(|(i, (text, ((start, _), id)))| {
                            // The first piece keeps the line's own time.
                            let time = if i == 0 {
                                head[..head.find("] ").map_or(0, |e| e + 2)].to_owned()
                            } else {
                                stamp(*start)
                            };
                            format!("{time}Speaker {}: {text}", id + first)
                        })
                        .collect()
                }
            })
            .collect::<Vec<_>>()
    };
    // A split line's later pieces start after lines from the other side spoken meanwhile: both
    // copies are put back in time order with the same moves, so they stay line for line.
    let heard = build(&heard_lines);
    let order = time_order(&heard);
    let arrange = |rows: Vec<String>| {
        order
            .iter()
            .map(|&i| rows[i].clone())
            .collect::<Vec<_>>()
            .join("\n")
    };
    Some(Found {
        edited: edited_lines.as_deref().map(|e| arrange(build(e))),
        heard: arrange(heard),
        voices,
    })
}

/// Speech of the other side at least this long, with gaps under `TURN_GAP_MS` bridged, is a turn
/// that can cut a line; shorter is a backchannel ("ừ") or echo. Each side of a cut also needs this
/// much of the line's own speech.
const TURN_MS: u64 = 1_500;
const TURN_GAP_MS: u64 = 500;
/// A line's own speech ends at its first silence this long (whisper splits there itself).
const LINE_GAP_MS: u64 = 5_000;
/// A line whose speech opens with less than this, then this long a silence, starts after it: an
/// echo or a click that whisper stamped as the line's start. Past `BLIP_LONGEST_GAP_MS` the
/// speech after it is taken for another line's.
const BLIP_MS: u64 = 1_000;
const BLIP_GAP_MS: u64 = LINE_GAP_MS;
const BLIP_LONGEST_GAP_MS: u64 = 15_000;
/// The furthest a line is taken to run.
const LONGEST_TURN_MS: u64 = 120_000;

/// Whisper writes a side's speech until its next pause as one line, so when the two sides take
/// quick turns, one side's 40 s line holds the other's reply in the middle: "[00:00] Them" ran
/// over "[00:18] Me". Each line of one side is cut where the other side took a turn
/// (`me` and `them`: the speech heard on each side, ms on the meeting's timeline) while it was
/// quiet, its text at the same share of its speech, as Find speakers cuts a line between voices;
/// a line that opens with a blip starts after it. Both copies change together, line for line.
/// None when nothing changes.
/// the text share is approximate (a sentence off); word timings from whisper would place it.
pub fn interleave(
    heard: &str,
    edited: Option<&str>,
    me: &[(u64, u64)],
    them: &[(u64, u64)],
) -> Option<(String, Option<String>)> {
    let heard_lines: Vec<&str> = heard.split('\n').collect();
    let edited_lines: Option<Vec<&str>> = edited
        .map(|e| e.split('\n').collect::<Vec<_>>())
        .filter(|e| e.len() == heard_lines.len());
    let mine = |line: &str| prefix(line).ends_with("] Me: ");
    let timed: Vec<(usize, u64, bool)> = heard_lines
        .iter()
        .enumerate()
        .filter(|(_, line)| !prefix(line).is_empty())
        .filter_map(|(i, line)| Some((i, millis(prefix(line))?, mine(line))))
        .collect();
    let (me_turns, them_turns) = (turns(me), turns(them));
    // Per line: (start of each piece, the share of its speech before each cut).
    let mut plans: Vec<Option<(Vec<u64>, Vec<f64>)>> = vec![None; heard_lines.len()];
    for (n, &(index, start, is_me)) in timed.iter().enumerate() {
        let (own, other) = if is_me {
            (me, &them_turns)
        } else {
            (them, &me_turns)
        };
        let limit = timed[n + 1..]
            .iter()
            .find(|t| t.2 == is_me)
            .map_or(start + LONGEST_TURN_MS, |t| t.1.max(start + 1))
            .min(start + LONGEST_TURN_MS);
        let first = opening(own, start, limit);
        let end = speech_end(own, first, limit);
        let mut cuts = vec![];
        let mut from = first;
        for &(turn, _) in other.iter().filter(|t| t.0 > first && t.0 < end) {
            if heard_in(own, from, turn) >= TURN_MS
                && heard_in(own, turn, end) >= TURN_MS
                && heard_in(own, turn, turn + 1_000) < 500
            {
                cuts.push(turn);
                from = turn;
            }
        }
        let total = heard_in(own, first, end).max(1) as f64;
        let shares: Vec<f64> = cuts
            .iter()
            .map(|&c| heard_in(own, first, c) as f64 / total)
            .collect();
        let texts = [
            Some(heard_lines[index]),
            edited_lines.as_ref().map(|e| e[index]),
        ];
        let splits = texts
            .iter()
            .flatten()
            .all(|line| split_text(&line[prefix(line).len()..], &shares).is_some());
        let (cuts, shares) = if splits {
            (cuts, shares)
        } else {
            (vec![], vec![])
        };
        let starts: Vec<u64> = std::iter::once(first)
            .chain(cuts.iter().map(|&c| piece_start(own, c, end)))
            .collect();
        if starts.len() > 1 || stamp(first) != stamp(start) {
            plans[index] = Some((starts, shares));
        }
    }
    if plans.iter().all(Option::is_none) {
        return None;
    }
    let build = |source: &[&str]| -> Vec<String> {
        source
            .iter()
            .zip(&plans)
            .flat_map(|(line, plan)| match plan {
                None => vec![(*line).to_owned()],
                Some((starts, shares)) => {
                    let head = prefix(line);
                    let label = &head[head.find("] ").map_or(0, |e| e + 2)..];
                    let text = &line[head.len()..];
                    split_text(text, shares)
                        .unwrap_or_else(|| vec![text.to_owned()])
                        .into_iter()
                        .zip(starts)
                        .map(|(text, at)| format!("{}{label}{text}", stamp(*at)))
                        .collect()
                }
            })
            .collect()
    };
    let new_heard = build(&heard_lines);
    let order = time_order(&new_heard);
    let arrange = |rows: Vec<String>| {
        order
            .iter()
            .map(|&i| rows[i].clone())
            .collect::<Vec<_>>()
            .join("\n")
    };
    Some((
        arrange(new_heard),
        edited_lines.as_deref().map(|e| arrange(build(e))),
    ))
}

/// Speech regions with gaps under `TURN_GAP_MS` bridged, those of `TURN_MS` or more.
fn turns(speech: &[(u64, u64)]) -> Vec<(u64, u64)> {
    let mut sorted = speech.to_vec();
    sorted.sort_unstable();
    let mut joined: Vec<(u64, u64)> = vec![];
    for (s, e) in sorted {
        match joined.last_mut() {
            Some(last) if s < last.1 + TURN_GAP_MS => last.1 = last.1.max(e),
            _ => joined.push((s, e)),
        }
    }
    joined.retain(|(s, e)| e - s >= TURN_MS);
    joined
}

/// How much of `start..end` is speech.
fn heard_in(speech: &[(u64, u64)], start: u64, end: u64) -> u64 {
    speech
        .iter()
        .map(|&(s, e)| e.min(end).saturating_sub(s.max(start)))
        .sum()
}

/// Where the speech from `start` stops: before its first silence of `LINE_GAP_MS`, or at `limit`.
fn speech_end(speech: &[(u64, u64)], start: u64, limit: u64) -> u64 {
    let mut last: Option<u64> = None;
    let mut sorted: Vec<_> = speech
        .iter()
        .filter(|(s, e)| *e > start && *s < limit)
        .collect();
    sorted.sort_unstable();
    for &(s, e) in sorted {
        if let Some(l) = last.filter(|l| s.saturating_sub(*l) >= LINE_GAP_MS) {
            return l;
        }
        last = Some(last.map_or(e, |l| l.max(e)));
    }
    last.map_or(limit, |l| l.min(limit))
}

/// Where the line's own speech in `start..end` really opens: past blips under `BLIP_MS` followed by
/// `BLIP_GAP_MS` of silence, when speech follows them in the line.
fn opening(speech: &[(u64, u64)], start: u64, end: u64) -> u64 {
    let mut inside: Vec<_> = speech
        .iter()
        .filter(|(s, e)| *e > start && *s < end)
        .copied()
        .collect();
    inside.sort_unstable();
    let mut at = start;
    for pair in inside.windows(2) {
        let ((s, e), (next, _)) = (pair[0], pair[1]);
        if e - s.max(start) >= BLIP_MS
            || !(BLIP_GAP_MS..=BLIP_LONGEST_GAP_MS).contains(&next.saturating_sub(e))
        {
            break;
        }
        at = next;
    }
    at
}

/// A later piece's time: its first stretch of own speech of `BLIP_MS` or more after `cut`, else
/// the cut.
fn piece_start(speech: &[(u64, u64)], cut: u64, end: u64) -> u64 {
    speech
        .iter()
        .filter(|(s, e)| *s >= cut && *s < end && e - s >= BLIP_MS)
        .map(|(s, _)| *s)
        .min()
        .unwrap_or(cut)
}

/// The rows' indices in time order: stable, so rows with the same time keep their order, and a row
/// without a time (a blank line) stays after the row before it.
pub fn time_order<S: AsRef<str>>(rows: &[S]) -> Vec<usize> {
    let mut at = 0;
    let keys: Vec<u64> = rows
        .iter()
        .map(|row| {
            if let Some(ms) = millis(prefix(row.as_ref())) {
                at = ms;
            }
            at
        })
        .collect();
    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_by_key(|&i| keys[i]);
    order
}

/// Where each inner cut of `pieces` falls in the line's speech (from its first heard word to its
/// last), as a fraction: the text is cut at the same share of its length.
fn fractions(turns: &[Turn], start: u64, end: u64, pieces: &[(u64, u64)]) -> Vec<f64> {
    let Some((lo, hi)) = diarize::speech(turns, start, end) else {
        return vec![];
    };
    let span = hi.saturating_sub(lo).max(1) as f64;
    pieces
        .iter()
        .skip(1)
        .map(|(cut, _)| (cut.clamp(&lo, &hi) - lo) as f64 / span)
        .collect()
}

/// `text` cut into `cuts.len() + 1` parts at those fractions of its length, each cut moved to the
/// nearest sentence end within a quarter of the text (else the nearest space). None when there
/// are too few words. Text without spaces (Chinese, Japanese) is never cut: a character cut
/// would need word timings.
fn split_text(text: &str, cuts: &[f64]) -> Option<Vec<String>> {
    if cuts.is_empty() {
        return Some(vec![text.to_owned()]);
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() <= cuts.len() {
        return None;
    }
    // ends[i]: the position (in characters) after word i, for the boundaries between words.
    let mut ends = vec![];
    let mut pos = 0;
    for word in &words[..words.len() - 1] {
        pos += word.chars().count();
        ends.push(pos);
        pos += 1;
    }
    let total = pos + words[words.len() - 1].chars().count();
    let window = (total / 4).max(1);
    let sentence = |i: usize| words[i].ends_with(['.', '?', '!', '…', '。', '？', '！']);
    let mut bounds = vec![];
    let mut lo = 0;
    for (n, f) in cuts.iter().enumerate() {
        let target = (f.clamp(0.0, 1.0) * total as f64).round() as usize;
        // Leave a word for every later piece.
        let hi = ends.len() - (cuts.len() - 1 - n);
        let near = |i: &usize| ends[*i].abs_diff(target);
        let best = (lo..hi)
            .filter(|i| sentence(*i) && near(i) <= window)
            .min_by_key(near)
            .or_else(|| (lo..hi).min_by_key(near))?;
        bounds.push(best);
        lo = best + 1;
    }
    let mut out = vec![];
    let mut from = 0;
    for b in bounds.into_iter().chain(std::iter::once(words.len() - 1)) {
        out.push(words[from..=b].join(" "));
        from = b + 1;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(start_ms: u64, end_ms: u64, speaker: usize) -> Turn {
        Turn::new(start_ms, end_ms, speaker)
    }

    #[test]
    #[ignore = "needs the speaker tool and models; set MOTETTO_TEST_CALL to a .system.wav, \
                MOTETTO_TEST_TRANSCRIPT to its transcript (MOTETTO_TEST_EDITED to an edited \
                copy) and MOTETTO_TEST_OUT to a folder for the results"]
    fn a_real_call_is_split_by_voice() {
        let var = |key| std::env::var(key).ok();
        let read = |path: String| std::fs::read_to_string(path).unwrap();
        let heard = read(var("MOTETTO_TEST_TRANSCRIPT").unwrap());
        let edited = var("MOTETTO_TEST_EDITED").map(read);
        let call = var("MOTETTO_TEST_CALL").unwrap();
        let people = var("MOTETTO_TEST_PEOPLE").map(|n| n.parse().unwrap());
        let found = find(&[(vec![call], 0)], &heard, edited.as_deref(), people)
            .unwrap()
            .unwrap();
        let out = Path::new(&var("MOTETTO_TEST_OUT").unwrap()).to_owned();
        std::fs::write(out.join("transcript.txt"), &found.heard).unwrap();
        if let Some(edited) = &found.edited {
            std::fs::write(out.join("transcript.edited.txt"), edited).unwrap();
            assert_eq!(edited.lines().count(), found.heard.lines().count());
        }
        assert!(found.voices >= 2 && found.heard.lines().count() > heard.lines().count());
    }

    #[test]
    fn named_voices_take_their_names_and_nothing_else_moves() {
        let names = [(2, "Kevin".to_string()), (12, "Linh".to_string())].into();
        let text = "[00:01] Me: hi\n[00:02] Speaker 2: hello\n[00:03] Speaker 3: yo\n\
                    [01:02:03] Speaker 12: Speaker 2: said\n[00:04] Them: ok\nloose";
        assert_eq!(
            named(text, &names),
            "[00:01] Me: hi\n[00:02] Kevin: hello\n[00:03] Speaker 3: yo\n\
             [01:02:03] Linh: Speaker 2: said\n[00:04] Them: ok\nloose"
        );
        assert_eq!(named(text, &Default::default()), text);
    }

    #[test]
    fn text_is_cut_at_sentence_ends_near_the_share() {
        let text = "Đúng rồi anh. Ok rồi sao nữa? Thì nó sẽ đi cược hết 110 này. \
                    Ok khoan từ từ, nó sẽ bet hết 110 là nó đủ rollover đúng không?";
        assert_eq!(
            split_text(text, &[0.3, 0.55]).unwrap(),
            [
                "Đúng rồi anh. Ok rồi sao nữa?",
                "Thì nó sẽ đi cược hết 110 này.",
                "Ok khoan từ từ, nó sẽ bet hết 110 là nó đủ rollover đúng không?"
            ]
        );
        // No sentence end nearby: the nearest space.
        assert_eq!(
            split_text("one two three four", &[0.5]).unwrap(),
            ["one two", "three four"]
        );
        assert_eq!(split_text("one", &[0.5]), None);
        assert_eq!(split_text("你好世界", &[0.5]), None);
    }

    #[test]
    fn split_pieces_go_back_in_time_order_in_both_copies() {
        // A long call-side line split into voices; the user's own line came in between.
        let heard = "[00:10] Them: First part here. Second part there.\n[00:15] Me: Meanwhile I spoke.\n[00:40] Them: Later.";
        let edited = heard.replace("First part", "Edited part");
        let turns = [
            turn(10_000, 14_000, 0),
            turn(20_000, 30_000, 1),
            turn(40_000, 45_000, 0),
        ];
        let found = relabel(heard, Some(&edited), &turns, true).unwrap();
        let times = |text: &str| {
            text.lines()
                .map(|l| millis(prefix(l)).unwrap())
                .collect::<Vec<_>>()
        };
        let heard_times = times(&found.heard);
        assert_eq!(heard_times.len(), 4, "the long line was split");
        assert!(
            heard_times.is_sorted(),
            "rows in time order: {heard_times:?}"
        );
        assert_eq!(times(found.edited.as_deref().unwrap()), times(&found.heard));
        assert!(
            found
                .heard
                .lines()
                .nth(1)
                .unwrap()
                .ends_with("Me: Meanwhile I spoke.")
        );
        assert!(
            found
                .edited
                .unwrap()
                .starts_with("[00:10] Speaker 2: Edited part here.")
        );
        // A row without a time stays after the row before it.
        let rows = ["[00:20] Me: b", "", "[00:10] Me: a"].map(String::from);
        assert_eq!(time_order(&rows), [2, 0, 1]);
    }

    #[test]
    fn a_two_voice_line_is_split_in_both_copies() {
        let heard = "[00:00] Me: hello\n[00:01] Them: Right. So what next? Then it bets all.\n\
                     [00:20] Them: ok";
        let edited = "[00:00] Me: hello\n[00:01] Them: Right. So what next? Then it bets ALL.\n\
                      [00:20] Them: OK";
        // Voice A 1-5 s, voice B 5.3-8 s, voice A again at 20 s.
        let turns = [
            turn(1000, 5000, 0),
            turn(5300, 8000, 1),
            turn(20000, 21000, 0),
        ];
        let found = relabel(heard, Some(edited), &turns, true).unwrap();
        assert_eq!(found.voices, 2);
        assert_eq!(
            found.heard,
            "[00:00] Me: hello\n[00:01] Speaker 2: Right. So what next?\n\
             [00:05] Speaker 3: Then it bets all.\n[00:20] Speaker 2: ok"
        );
        assert_eq!(
            found.edited.unwrap(),
            "[00:00] Me: hello\n[00:01] Speaker 2: Right. So what next?\n\
             [00:05] Speaker 3: Then it bets ALL.\n[00:20] Speaker 2: OK"
        );
    }

    #[test]
    fn one_voice_changes_nothing_and_imports_count_from_one() {
        let heard = "[00:01] Them: one\n[00:05] Them: two";
        assert_eq!(relabel(heard, None, &[turn(0, 9000, 0)], true), None);
        let import = "[00:01] hi there\n[00:05] hello";
        let found = relabel(
            import,
            None,
            &[turn(0, 3000, 0), turn(5000, 9000, 1)],
            false,
        );
        assert_eq!(
            found.unwrap().heard,
            "[00:01] Speaker 1: hi there\n[00:05] Speaker 2: hello"
        );
    }

    #[test]
    fn one_other_person_is_one_numbered_voice() {
        let heard = "[00:00] Me: hi\n[00:01] Speaker 2: a\n[00:05] Speaker 4: b\n\n[00:09] Them: c";
        let edited = heard.replace(": b", ": B");
        let found = one_voice(heard, Some(&edited), true).unwrap();
        assert_eq!(
            found.heard,
            "[00:00] Me: hi\n[00:01] Speaker 2: a\n[00:05] Speaker 2: b\n\n[00:09] Speaker 2: c"
        );
        assert_eq!(found.edited.unwrap(), found.heard.replace(": b", ": B"));
        // Already one voice: nothing to change. An import's one voice is Speaker 1.
        assert_eq!(one_voice("[00:01] Speaker 2: a", None, true), None);
        assert_eq!(
            one_voice("[00:01] Speaker 3: a\n[00:02] b", None, false)
                .unwrap()
                .heard,
            "[00:01] Speaker 1: a\n[00:02] Speaker 1: b"
        );
    }

    #[test]
    fn a_long_line_is_cut_where_the_other_side_took_a_turn() {
        // Them talks 0-18 s, Me 18-22 s, Them again 22.5-40 s; whisper made Them one line and
        // stamped Me at an echo blip at 8 s.
        let heard = "[00:00] Them: One two. Three four. Five six. Seven eight.\n\
                     [00:08] Me: My turn now.\n[01:00] Them: later";
        let edited = heard.replace("Seven", "SEVEN");
        let me = [(8_300, 8_900), (18_000, 22_000)];
        let them = [(0, 17_900), (22_500, 40_000), (60_000, 61_000)];
        let (h, e) = interleave(heard, Some(&edited), &me, &them).unwrap();
        assert_eq!(
            h,
            "[00:00] Them: One two. Three four.\n[00:18] Me: My turn now.\n\
             [00:22] Them: Five six. Seven eight.\n[01:00] Them: later"
        );
        assert_eq!(e.unwrap(), h.replace("Seven", "SEVEN"));
        // Already in turns: nothing changes.
        assert_eq!(interleave(&h, None, &me, &them), None);
        // A backchannel under TURN_MS doesn't cut.
        assert_eq!(
            interleave(
                "[00:00] Them: a b. c d.",
                None,
                &[(9_000, 9_800)],
                &[(0, 8_900), (10_000, 20_000)]
            ),
            None
        );
    }

    #[test]
    fn a_line_too_short_to_cut_keeps_one_label_in_both_copies() {
        // Two voices, but the edited copy has one word left: neither copy is cut.
        let heard = "[00:01] Them: Right then. Bets all.\n[00:30] Them: yes";
        let edited = "[00:01] Them: Right.\n[00:30] Them: yes";
        let turns = [
            turn(1000, 5000, 0),
            turn(5500, 7000, 1),
            turn(30000, 33000, 1),
        ];
        let found = relabel(heard, Some(edited), &turns, true).unwrap();
        assert_eq!(
            found.heard,
            "[00:01] Speaker 2: Right then. Bets all.\n[00:30] Speaker 3: yes"
        );
        assert_eq!(
            found.edited.unwrap(),
            "[00:01] Speaker 2: Right.\n[00:30] Speaker 3: yes"
        );
    }
}
