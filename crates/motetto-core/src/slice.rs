//! Where to cut a track into slices: in the latest pause, so no word is split and a slice
//! usually holds whole sentences that whisper can each detect in their own language.

use std::error::Error;
use std::path::Path;

/// One level per 100 ms of audio.
pub const FRAME_MS: u64 = 100;
/// 300 ms under the quiet level is a pause between words or sentences (calls rarely pause 1 s).
const PAUSE: usize = 3;

/// RMS of each whole 100 ms frame from `start_ms` until `end_ms` or the end of the file,
/// read sample by sample so a long call is never loaded at once.
pub fn levels(path: &Path, start_ms: u64, end_ms: u64) -> Result<Vec<f32>, Box<dyn Error>> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let rate = u64::from(spec.sample_rate);
    reader.seek((start_ms * rate / 1000).try_into()?)?;
    let frame = (rate * FRAME_MS / 1000) as usize * usize::from(spec.channels);
    let frames = (end_ms.saturating_sub(start_ms) / FRAME_MS) as usize;
    let mut levels = Vec::new();
    let (mut sum, mut count) = (0.0_f64, 0);
    for sample in reader.samples::<f32>().take(frame.saturating_mul(frames)) {
        let sample = f64::from(sample?);
        sum += sample * sample;
        count += 1;
        if count == frame {
            levels.push((sum / count as f64).sqrt() as f32);
            (sum, count) = (0.0, 0);
        }
    }
    Ok(levels)
}

/// Length in frames of the next slice, from the levels available after its start (the louder
/// track per frame when tracks are cut together). It ends in the middle of the latest pause
/// between `min` and `max` frames. Without one it waits for more audio, or at `max` ends in the
/// quietest 300 ms. `done` means no more audio will arrive, so a short rest is taken whole.
pub fn cut(levels: &[f32], min: usize, max: usize, done: bool) -> Option<usize> {
    if done && levels.len() <= max {
        return (!levels.is_empty()).then_some(levels.len());
    }
    if levels.len() < min {
        return None;
    }
    let window = &levels[..levels.len().min(max)];
    let mut sorted = window.to_vec();
    sorted.sort_by(f32::total_cmp);
    // energy against the window's median, not a VAD model; steady music or noise
    // under speech hides pauses, and the slice then ends at the quietest 300 ms instead.
    let quiet = (sorted[sorted.len() / 2] * 0.3).max(0.003);
    let mut run = 0_usize;
    let mut pause = None;
    for (i, level) in window.iter().enumerate() {
        run = if *level < quiet { run + 1 } else { 0 };
        let middle = i + 1 - run.div_ceil(2);
        if run >= PAUSE && middle >= min {
            pause = Some(middle);
        }
    }
    if pause.is_some() || window.len() < max {
        return pause;
    }
    let loudness = |end: &usize| window[end - 1] + window[*end] + window[end + 1];
    // The latest of equally quiet spots keeps catch-up slices long.
    (min.max(1)..max - 1)
        .rev()
        .min_by(|a, b| loudness(a).total_cmp(&loudness(b)))
        .or(Some(max))
}

/// Speech intervals in ms that the Silero `model` finds in a WAV, from whisper.cpp's
/// `whisper-vad-speech-segments` (installed with `whisper-server`). Gaps under 300 ms are merged.
pub fn speech(path: &Path, model: &str) -> Result<Vec<(u64, u64)>, Box<dyn Error>> {
    // Not `tools::output`: a Cancel of an after-call job must not stop live captions.
    let mut command = crate::tools::command("whisper-vad-speech-segments");
    command
        .args(["-np", "-vsd", "300", "-vm", model, "-f"])
        .arg(path);
    let output = command
        .output()
        .map_err(|e| crate::tools::explain(&command, e))?;
    if !output.status.success() {
        return Err(format!("whisper-vad-speech-segments failed ({})", output.status).into());
    }
    parse_speech(&String::from_utf8_lossy(&output.stdout))
}

/// `Speech segment 0: start = 0.00, end = 96.00`, in centiseconds.
fn parse_speech(stdout: &str) -> Result<Vec<(u64, u64)>, Box<dyn Error>> {
    let ms = |field: &str| -> Result<u64, Box<dyn Error>> {
        Ok((field.trim().parse::<f64>()? * 10.0).round() as u64)
    };
    stdout
        .lines()
        .filter_map(|line| line.strip_prefix("Speech segment "))
        .map(|line| {
            let (_, times) = line.split_once("start =").ok_or("unexpected VAD output")?;
            let (start, end) = times.split_once(", end =").ok_or("unexpected VAD output")?;
            Ok((ms(start)?, ms(end)?))
        })
        .collect()
}

/// Like `cut`, from speech intervals (ms after the slice start, any tracks, any order) in the
/// first `available` ms: ends in the middle of the latest gap of 300 ms or more without speech
/// that is `min` ms or later. `None` means no such gap yet.
pub fn speech_cut(speech: &[(u64, u64)], available: u64, min: u64) -> Option<u64> {
    let mut spoken = speech.to_vec();
    spoken.sort_unstable();
    let (mut gaps, mut last) = (Vec::new(), 0);
    for (start, end) in spoken {
        if start > last {
            gaps.push((last, start.min(available)));
        }
        last = last.max(end);
    }
    gaps.push((last, available));
    gaps.into_iter()
        .rev()
        .filter(|(start, end)| end.saturating_sub(*start) >= PAUSE as u64 * FRAME_MS)
        .map(|(start, end)| (start + end) / 2)
        .find(|middle| *middle >= min)
}

/// Copies `start_ms..end_ms` (or to the end of the file) of a WAV, keeping its format.
pub fn write(
    source: &Path,
    target: &Path,
    start_ms: u64,
    end_ms: u64,
) -> Result<(), Box<dyn Error>> {
    let mut reader = hound::WavReader::open(source)?;
    let spec = reader.spec();
    let rate = u64::from(spec.sample_rate);
    reader.seek((start_ms * rate / 1000).try_into()?)?;
    let samples = (end_ms.saturating_sub(start_ms).saturating_mul(rate) / 1000) as usize;
    let mut writer = hound::WavWriter::create(target, spec)?;
    for sample in reader
        .samples::<f32>()
        .take(samples.saturating_mul(usize::from(spec.channels)))
    {
        writer.write_sample(sample?)?;
    }
    writer.finalize()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // 1 = speech, 0 = silence, one character per 100 ms frame.
    fn frames(pattern: &str) -> Vec<f32> {
        pattern
            .chars()
            .map(|c| if c == '1' { 0.05 } else { 0.001 })
            .collect()
    }

    #[test]
    fn ends_in_the_latest_pause_after_min() {
        // Pauses at frames 5-7 and 13-16: the later one wins, cut in its middle.
        let levels = frames("11111000111110000111");
        assert_eq!(cut(&levels, 3, 100, false), Some(15));
        // A pause that ends before `min` doesn't count; wait for more audio.
        assert_eq!(cut(&levels[..10], 9, 100, false), None);
        // Two quiet frames are a gap inside a word, not a pause.
        assert_eq!(cut(&frames("1111100111111"), 3, 100, false), None);
    }

    #[test]
    fn waits_for_min_then_falls_back_to_the_quietest_frames_at_max() {
        assert_eq!(cut(&frames("00"), 3, 10, false), None);
        let mut levels = frames("1111111111");
        levels[6] = 0.02;
        levels[7] = 0.01;
        levels[8] = 0.02;
        assert_eq!(cut(&levels, 3, 10, false), Some(7));
        // More audio than `max` only looks at the first `max` frames.
        levels.extend(frames("0000"));
        assert_eq!(cut(&levels, 3, 10, false), Some(7));
    }

    #[test]
    fn at_the_end_takes_the_rest_whole_and_never_returns_empty() {
        assert_eq!(cut(&frames("11"), 3, 10, true), Some(2));
        assert_eq!(cut(&[], 3, 10, true), None);
        // Longer than `max`: still cut at a pause so the next slice gets the rest.
        assert_eq!(cut(&frames("111110001111111"), 3, 10, true), Some(6));
    }

    #[test]
    fn speech_cut_ends_in_the_latest_shared_gap_after_min() {
        // Me speaks 0-3.5 s and 4.5-8 s; Them 3.6-4.0 s, so the only shared gap is 4.0-4.5 s.
        let speech = [(4500, 8000), (0, 3500), (3600, 4000)];
        assert_eq!(speech_cut(&speech, 8000, 3000), Some(4250));
        // A trailing gap counts once it lasts 300 ms; one before `min` never does.
        assert_eq!(speech_cut(&speech, 8200, 3000), Some(4250));
        assert_eq!(speech_cut(&speech, 8400, 3000), Some(8200));
        assert_eq!(speech_cut(&speech, 8000, 5000), None);
        assert_eq!(speech_cut(&[(0, 9000)], 9000, 3000), None);
        assert_eq!(speech_cut(&[], 4000, 3000), None);
        assert_eq!(speech_cut(&[], 6000, 3000), Some(3000));
        // Speech reported past the window does not make a gap.
        assert_eq!(
            speech_cut(&[(0, 2000), (9900, 10_500)], 9800, 3000),
            Some(5900)
        );
    }

    #[test]
    fn parse_speech_reads_centiseconds() {
        let out = "Detected 2 speech segments:\nSpeech segment 0: start = 0.00, end = 96.00\n\
                   Speech segment 1: start = 186.50, end = 493.00\n";
        assert_eq!(parse_speech(out).unwrap(), [(0, 960), (1865, 4930)]);
        assert!(
            parse_speech("Detected 0 speech segments:\n")
                .unwrap()
                .is_empty()
        );
        assert!(parse_speech("Speech segment 0: start = x, end = 1.00").is_err());
    }

    #[test]
    fn silence_is_one_long_pause() {
        assert_eq!(cut(&frames("0000000000"), 3, 10, false), Some(5));
    }

    #[test]
    fn levels_and_write_keep_rate_and_exact_intervals() {
        let root = std::env::temp_dir().join(format!("motetto-slice-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("source.wav");
        let target = root.join("slice.wav");
        let mut writer = hound::WavWriter::create(
            &source,
            hound::WavSpec {
                channels: 1,
                sample_rate: 16000,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .unwrap();
        // 1 s of silence, then 1 s at 0.25 and a partial 50 ms frame.
        for i in 0..32_800 {
            writer
                .write_sample(if i < 16_000 { 0.0_f32 } else { 0.25 })
                .unwrap();
        }
        writer.finalize().unwrap();
        let all = levels(&source, 0, u64::MAX).unwrap();
        assert_eq!(all.len(), 20);
        assert_eq!((all[9], all[10]), (0.0, 0.25));
        assert_eq!(levels(&source, 500, 1500).unwrap().len(), 10);
        write(&source, &target, 500, 1500).unwrap();
        let reader = hound::WavReader::open(&target).unwrap();
        assert_eq!(
            (reader.duration(), reader.spec().sample_rate),
            (16000, 16000)
        );
        write(&source, &target, 2000, u64::MAX).unwrap();
        assert_eq!(hound::WavReader::open(&target).unwrap().duration(), 800);
        std::fs::remove_dir_all(root).unwrap();
    }
}
