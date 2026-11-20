//! Diarize stage (optional): split one track into speakers with sherpa-onnx, on this computer.

use std::error::Error;
use std::fs;
use std::path::Path;

use crate::config;

const TOOL: &str = "sherpa-onnx-offline-speaker-diarization";
/// How far apart two voices must be to count as two people when the count is not given (higher:
/// fewer people). On a real 1 h 54 min call of four or five voices, 0.5 found 204 and 0.8 found
/// 110, most of them a few seconds of one person (folded below to 5); from 0.9 three synthetic
/// voices merged, and 1.1 merged two people of the real call.
const THRESHOLD: f32 = 0.8;
/// Without a count, a voice heard for less than this share of the speech is taken as a stray
/// piece of someone else and joins the voice heard nearest in time. On that call it left five
/// voices that agreed with a run told "4 people" on 82 % of the speech.
const MIN_SHARE: f64 = 0.02;

/// One speaker turn: diarizer cluster `speaker` talked from `start_ms` to `end_ms`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Turn {
    start_ms: u64,
    end_ms: u64,
    speaker: usize,
}

impl Turn {
    #[cfg(test)]
    pub(crate) fn new(start_ms: u64, end_ms: u64, speaker: usize) -> Self {
        Self {
            start_ms,
            end_ms,
            speaker,
        }
    }
    /// The turn moved `offset` ms later: a recording part's place on the meeting's timeline.
    pub(crate) fn shifted(self, offset: u64) -> Self {
        Self {
            start_ms: self.start_ms + offset,
            end_ms: self.end_ms + offset,
            ..self
        }
    }
}

/// From the first to the last voice heard in `start..end`; None when nobody speaks there.
pub(crate) fn speech(turns: &[Turn], start: u64, end: u64) -> Option<(u64, u64)> {
    let inside = turns
        .iter()
        .filter(|t| t.end_ms > start && t.start_ms < end);
    let lo = inside.clone().map(|t| t.start_ms.max(start)).min()?;
    let hi = inside.map(|t| t.end_ms.min(end)).max()?;
    Some((lo, hi))
}

/// A model path setting; unset and empty both mean off.
fn model(key: &str) -> Option<String> {
    config::var(key).filter(|v| !v.is_empty())
}

/// Speaker turns of the saved track `source`, or `None` when diarization is off (both models
/// unset). `people`: how many voices the track holds, when the user said; else the diarizer
/// decides. `wav16`: the decoder's 16 kHz mono WAV of `source` when the caller has one; without
/// it the track is decoded here, and only on a miss.
///
/// The tool takes about a tenth of the call's length (12 minutes for a 2-hour call), so its turns
/// are kept in `.speakers/` beside the track. The after-call transcription fills it, and Find
/// speakers or Transcribe again then reuse it. A kept file counts only when it names the same
/// clustering, both models and the track's size and modification time.
pub(crate) fn turns(
    source: &Path,
    wav16: Option<&Path>,
    people: Option<usize>,
) -> Result<Option<Vec<Turn>>, Box<dyn Error>> {
    let (seg, emb) = match (
        model("DIARIZE_SEGMENTATION_MODEL"),
        model("DIARIZE_EMBEDDING_MODEL"),
    ) {
        (Some(seg), Some(emb)) => (seg, emb),
        (None, None) => return Ok(None),
        _ => {
            return Err(
                "set both DIARIZE_SEGMENTATION_MODEL and DIARIZE_EMBEDDING_MODEL, or neither"
                    .into(),
            );
        }
    };
    let (kept, key) = (kept_path(source, people), key(source, &seg, &emb, people));
    let raw = match (fs::read_to_string(&kept), &key) {
        (Ok(text), Some(key)) if text.lines().next() == Some(key.as_str()) => parse(&text),
        _ => {
            let raw = heard(source, wav16, &seg, &emb, people)?;
            if let Some(key) = &key {
                keep(&kept, key, &raw);
            }
            raw
        }
    };
    Ok(Some(if people.is_some() { raw } else { fold(raw) }))
}

/// Runs the tool on `source` (decoded here unless `wav16` is given).
fn heard(
    source: &Path,
    wav16: Option<&Path>,
    seg: &str,
    emb: &str,
    people: Option<usize>,
) -> Result<Vec<Turn>, Box<dyn Error>> {
    let decoded = std::env::temp_dir().join(format!(
        "motetto-speakers-{}-{}.16k.wav",
        std::process::id(),
        source
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("track")
    ));
    let wav = match wav16 {
        Some(wav) => wav,
        None => {
            crate::decode::to_wav(source, &decoded)?;
            decoded.as_path()
        }
    };
    // The decoder writes 32-bit float for whisper; the diarizer reads only 16-bit PCM.
    let pcm = decoded.with_extension("pcm16.wav");
    let result = to_pcm16(wav, &pcm).and_then(|()| run(&pcm, seg, emb, people));
    let _ = fs::remove_file(&pcm);
    let _ = fs::remove_file(&decoded);
    result
}

/// `<folder>/.speakers/<track file>.<auto|N>.txt`: hidden, so a scan for an import's
/// `source.<ext>` never takes it for audio.
fn kept_path(source: &Path, people: Option<usize>) -> std::path::PathBuf {
    let name = source
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("track");
    let count = people.map_or("auto".to_string(), |n| n.to_string());
    source
        .parent()
        .unwrap_or(Path::new("."))
        .join(".speakers")
        .join(format!("{name}.{count}.txt"))
}

/// The first line of a kept file: what produced it. None when a file can't be measured, so
/// nothing is kept or reused.
fn key(source: &Path, seg: &str, emb: &str, people: Option<usize>) -> Option<String> {
    let size = |path: &Path| fs::metadata(path).ok().map(|m| m.len());
    let meta = fs::metadata(source).ok()?;
    let modified = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis();
    let clustering = people.map_or(format!("threshold {THRESHOLD}"), |n| format!("people {n}"));
    Some(format!(
        "# {TOOL} {clustering}; models {}:{} {}:{}; track {}:{modified}",
        file_name(seg),
        size(Path::new(seg))?,
        file_name(emb),
        size(Path::new(emb))?,
        meta.len()
    ))
}

fn file_name(path: &str) -> &str {
    Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path)
}

/// Writes the raw turns under their key. Keeping them is only a speed-up: a folder that can't be
/// written to (a read-only import) still gets its speakers, the tool just runs again next time.
fn keep(path: &Path, key: &str, turns: &[Turn]) {
    let text: String = std::iter::once(format!("{key}\n"))
        .chain(turns.iter().map(|t| {
            format!(
                "{:.3} -- {:.3} speaker_{:02}\n",
                t.start_ms as f64 / 1000.0,
                t.end_ms as f64 / 1000.0,
                t.speaker
            )
        }))
        .collect();
    let next = path.with_extension("txt.new");
    let _ = path
        .parent()
        .map(fs::create_dir_all)
        .unwrap_or(Ok(()))
        .and_then(|()| fs::write(&next, text))
        .and_then(|()| fs::rename(&next, path));
    let _ = fs::remove_file(&next);
}

fn run(
    wav: &Path,
    seg: &str,
    emb: &str,
    people: Option<usize>,
) -> Result<Vec<Turn>, Box<dyn Error>> {
    let threads = std::thread::available_parallelism().map_or(2, |n| n.get().min(4));
    let clustering = match people {
        Some(n) => format!("--clustering.num-clusters={n}"),
        None => format!("--clustering.cluster-threshold={THRESHOLD}"),
    };
    let out = crate::tools::output(
        crate::tools::command(TOOL)
            .arg(format!("--segmentation.pyannote-model={seg}"))
            .arg(format!("--embedding.model={emb}"))
            .arg(format!("--segmentation.num-threads={threads}"))
            .arg(format!("--embedding.num-threads={threads}"))
            .arg(clustering)
            .arg(wav),
        None,
    )
    .map_err(|e| format!("couldn't run {TOOL} ({e}); put its bin folder on PATH"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let last = stderr.lines().rfind(|l| !l.trim().is_empty()).unwrap_or("");
        return Err(format!("{TOOL} failed ({}): {last}", out.status).into());
    }
    Ok(parse(&String::from_utf8_lossy(&out.stdout)))
}

/// Copies a float WAV to 16-bit PCM, sample by sample, so a long call never sits in memory.
fn to_pcm16(src: &Path, dst: &Path) -> Result<(), Box<dyn Error>> {
    let mut reader = hound::WavReader::open(src)?;
    let spec = hound::WavSpec {
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
        ..reader.spec()
    };
    let mut writer = hound::WavWriter::create(dst, spec)?;
    for sample in reader.samples::<f32>() {
        writer.write_sample(pcm16(sample?))?;
    }
    writer.finalize()?;
    Ok(())
}

fn pcm16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16
}

/// Diarizer stdout rows `1.583 -- 3.406 speaker_00`. Other lines (the echoed config, which also
/// says "speaker", and progress) are skipped.
fn parse(stdout: &str) -> Vec<Turn> {
    stdout
        .lines()
        .filter_map(|line| {
            let mut cols = line.split_whitespace();
            let start_ms = millis(cols.next()?)?;
            (cols.next()? == "--").then_some(())?;
            let end_ms = millis(cols.next()?)?;
            let speaker = cols.next()?.strip_prefix("speaker_")?.parse().ok()?;
            (cols.next().is_none() && end_ms > start_ms).then_some(Turn {
                start_ms,
                end_ms,
                speaker,
            })
        })
        .collect()
}

fn millis(secs: &str) -> Option<u64> {
    let s: f64 = secs.parse().ok()?;
    (s.is_finite() && s >= 0.0).then(|| (s * 1000.0).round() as u64)
}

/// Voices with under `MIN_SHARE` of the speech join the voice of the nearest turn that has more.
fn fold(turns: Vec<Turn>) -> Vec<Turn> {
    let mut heard: Vec<(usize, u64)> = vec![];
    for t in &turns {
        match heard.iter_mut().find(|(s, _)| *s == t.speaker) {
            Some((_, ms)) => *ms += t.end_ms - t.start_ms,
            None => heard.push((t.speaker, t.end_ms - t.start_ms)),
        }
    }
    let total: u64 = heard.iter().map(|(_, ms)| ms).sum();
    let major = |s: usize| {
        heard
            .iter()
            .any(|&(v, ms)| v == s && ms as f64 >= total as f64 * MIN_SHARE)
    };
    let majors: Vec<Turn> = turns.iter().copied().filter(|t| major(t.speaker)).collect();
    if majors.is_empty() {
        return turns;
    }
    turns
        .into_iter()
        .map(|t| {
            if major(t.speaker) {
                return t;
            }
            let gap = |m: &Turn| {
                m.start_ms.saturating_sub(t.end_ms) + t.start_ms.saturating_sub(m.end_ms)
            };
            let nearest = majors
                .iter()
                .min_by_key(|m| gap(m))
                .map_or(t.speaker, |m| m.speaker);
            Turn {
                speaker: nearest,
                ..t
            }
        })
        .collect()
}

/// The shortest stretch of one voice inside a line that counts as a turn; shorter ones are
/// backchannels ("ừ", "ok") or clustering noise and stay in the line around them.
const MIN_TURN_MS: u64 = 1_000;

/// Where to cut the line `start..end` so each piece holds one voice: whisper starts a line only
/// at a pause, so a quick exchange between two people lands in one line. Cuts fall halfway
/// between one voice's last word and the next voice's first. Empty when one voice holds the line.
pub(crate) fn pieces(turns: &[Turn], start: u64, end: u64) -> Vec<(u64, u64)> {
    let mut inside: Vec<_> = turns
        .iter()
        .filter_map(|t| {
            let (s, e) = (t.start_ms.max(start), t.end_ms.min(end));
            (e >= s + MIN_TURN_MS).then_some((s, e, t.speaker))
        })
        .collect();
    inside.sort_unstable();
    let mut runs: Vec<(u64, u64, usize)> = vec![];
    for (s, e, who) in inside {
        match runs.last_mut() {
            Some(last) if last.2 == who => last.1 = last.1.max(e),
            _ => runs.push((s, e, who)),
        }
    }
    let mut bounds = vec![start];
    for pair in runs.windows(2) {
        // Halfway between the end of one voice and the start of the next; when they overlap
        // (both talking), halfway through the overlap.
        let cut = (pair[0].1 + pair[1].0) / 2;
        if cut > bounds[bounds.len() - 1] && cut < end {
            bounds.push(cut);
        }
    }
    if bounds.len() < 2 {
        return vec![];
    }
    bounds.push(end);
    bounds.windows(2).map(|b| (b[0], b[1])).collect()
}

/// For each transcript span (start, end in ms, in time order), the speaker who overlaps it most,
/// or the nearest turn when none overlaps. Speakers are renumbered by first appearance, so the
/// first voice heard is 0. Returns the numbers and how many distinct voices were heard, or
/// `None` when the diarizer found no turns.
pub(crate) fn assign(turns: &[Turn], spans: &[(u64, u64)]) -> Option<(Vec<usize>, usize)> {
    if turns.is_empty() {
        return None;
    }
    let mut order: Vec<usize> = vec![];
    let ids = spans
        .iter()
        .map(|&(start, end)| {
            let best = turns
                .iter()
                .max_by_key(|t| {
                    let overlap = t.end_ms.min(end).saturating_sub(t.start_ms.max(start));
                    let gap = t.start_ms.saturating_sub(end) + start.saturating_sub(t.end_ms);
                    (overlap, std::cmp::Reverse(gap))
                })
                .map_or(0, |t| t.speaker);
            order.iter().position(|&s| s == best).unwrap_or_else(|| {
                order.push(best);
                order.len() - 1
            })
        })
        .collect();
    Some((ids, order.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(start_ms: u64, end_ms: u64, speaker: usize) -> Turn {
        Turn {
            start_ms,
            end_ms,
            speaker,
        }
    }

    #[test]
    fn pcm16_scales_and_clips() {
        assert_eq!(pcm16(0.0), 0);
        assert_eq!(pcm16(0.5), 16384);
        assert_eq!(pcm16(-2.0), -i16::MAX);
        assert_eq!(pcm16(f32::NAN), 0);
    }

    #[test]
    fn parses_turns_and_skips_config_and_progress() {
        let out = "OfflineSpeakerDiarizationConfig(embedding=speaker.onnx)\nStarted\n\
                   1.583 -- 3.406 speaker_00\n9.346 -- 11.472 speaker_01\nprogress 92.86%\n\
                   2.0 -- 1.0 speaker_02\n4.0 -- 5.0 speaker_x\n";
        assert_eq!(parse(out), [turn(1583, 3406, 0), turn(9346, 11472, 1)]);
    }

    #[test]
    fn assign_picks_most_overlap_and_numbers_by_first_appearance() {
        // The diarizer calls the second voice speaker_00; the transcript hears speaker_01 first.
        let turns = [turn(0, 4000, 1), turn(4600, 9000, 0), turn(9500, 12000, 1)];
        let (ids, count) = assign(
            &turns,
            &[(100, 3900), (4500, 8800), (3800, 9600), (9600, 11000)],
        )
        .unwrap();
        assert_eq!(ids, [0, 1, 1, 0]);
        assert_eq!(count, 2);
    }

    #[test]
    fn assign_uses_nearest_turn_when_nothing_overlaps() {
        let turns = [turn(0, 1000, 0), turn(5000, 6000, 1)];
        assert_eq!(
            assign(&turns, &[(4200, 4800), (1100, 1200)]),
            Some((vec![0, 1], 2))
        );
    }

    #[test]
    fn kept_turns_round_trip_and_go_stale_with_the_track() {
        let dir = std::env::temp_dir().join(format!("motetto-kept-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let (track, seg, emb) = (
            dir.join("source.mp4"),
            dir.join("seg.onnx"),
            dir.join("emb.onnx"),
        );
        fs::write(&track, b"audio").unwrap();
        fs::write(&seg, b"s").unwrap();
        fs::write(&emb, b"e").unwrap();
        let (seg, emb) = (seg.to_str().unwrap(), emb.to_str().unwrap());
        // Hidden and not `source.<ext>`, so an import's audio scan never takes it.
        let kept = kept_path(&track, None);
        assert_eq!(kept, dir.join(".speakers/source.mp4.auto.txt"));
        assert_eq!(
            kept_path(&track, Some(4)),
            dir.join(".speakers/source.mp4.4.txt")
        );
        let key = key(&track, seg, emb, None).unwrap();
        assert_ne!(Some(&key), super::key(&track, seg, emb, Some(4)).as_ref());
        let turns = [turn(1583, 3406, 0), turn(9346, 11472, 12)];
        keep(&kept, &key, &turns);
        let text = fs::read_to_string(&kept).unwrap();
        assert_eq!(text.lines().next(), Some(key.as_str()));
        assert_eq!(parse(&text), turns);
        // A changed track (re-saved, so a new size and time) no longer matches.
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(&track, b"other audio").unwrap();
        assert_ne!(super::key(&track, seg, emb, None).unwrap(), key);
        // A missing model can't be measured: nothing is kept or reused.
        assert_eq!(super::key(&track, "/missing.onnx", emb, None), None);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn stray_voices_join_the_nearest_main_voice() {
        // Two people talk for minutes; a third "voice" is two seconds of the second one.
        let turns = vec![
            turn(0, 60_000, 0),
            turn(60_000, 120_000, 1),
            turn(121_000, 123_000, 2),
            turn(130_000, 190_000, 0),
        ];
        let folded = fold(turns);
        assert_eq!(folded[2], turn(121_000, 123_000, 1));
        assert_eq!(folded.iter().filter(|t| t.speaker == 2).count(), 0);
        // Nobody big enough: unchanged.
        assert_eq!(fold(vec![turn(0, 10, 0)]), [turn(0, 10, 0)]);
    }

    #[test]
    fn pieces_cut_a_line_between_voices() {
        // Two voices in one line, a short gap between them, then the first voice again.
        let turns = [turn(0, 3000, 0), turn(3400, 6000, 1), turn(6000, 9000, 0)];
        assert_eq!(
            pieces(&turns, 200, 8800),
            [(200, 3200), (3200, 6000), (6000, 8800)]
        );
        // Overlapping talk is cut halfway through the overlap.
        let turns = [turn(0, 4000, 0), turn(3000, 7000, 1)];
        assert_eq!(pieces(&turns, 0, 7000), [(0, 3500), (3500, 7000)]);
    }

    #[test]
    fn pieces_keep_a_line_of_one_voice_whole() {
        // Same voice in two turns, a backchannel under a second, a voice outside the line.
        let turns = [
            turn(0, 2000, 0),
            turn(2100, 2600, 1),
            turn(2700, 5000, 0),
            turn(5000, 9000, 1),
        ];
        assert!(pieces(&turns, 0, 5300).is_empty());
        assert!(pieces(&[], 0, 5000).is_empty());
    }

    #[test]
    fn assign_without_turns_is_none() {
        assert_eq!(assign(&[], &[(0, 1000)]), None);
    }
}
