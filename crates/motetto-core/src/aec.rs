//! Loudspeaker echo cancellation: on a call played through speakers the mic records the far side
//! again. The system track is exactly what the speakers played, so it is the reference: an NLMS
//! filter takes the echo out of the mic, then what it leaves of the echo is muted (mic gating and
//! AGC make part of the echo nonlinear, and whisper invents sentences from it).
//! Measured on a 35-minute loudspeaker call (2026-10-24): echo -39 to -49 dB, mic-only speech
//! unchanged, 750 mic lines down to 31, all the user's own; an ffmpeg anlms + sidechain chain
//! reached -25 to -31 dB at a quarter of the speed.
use std::error::Error;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

/// 16 kHz mono float, as `decode::to_wav` writes.
const RATE: usize = 16_000;
/// 2 ms frames for the lag search: fine enough for the filter's 40 ms margin.
const FRAME: usize = 32;
/// Lags searched either way: measured lags were -124 to +61 ms.
const MAX_LAG: usize = 250 / 2;
/// Envelope correlation at the best lag that means the mic hears the speakers; headphone calls
/// and separate voices stay near 0.
const MIN_MATCH: f32 = 0.3;
/// 128 ms filter; the echo is placed 40 ms in, so the lag may drift either way within a part.
const TAPS: usize = 2048;
const LEAD: usize = RATE * 40 / 1000;
/// Seconds of the call the filter learns on before the real pass.
const WARMUP: usize = 10 * RATE;
const MU: f32 = 1.0;
/// Regularization against the reference power: smaller diverged where the far side starts
/// talking over the user (the output got louder than the mic).
const EPS: f32 = 11.0;
/// Residual suppression: power levels over ~10 ms; where the filter changed the power by over
/// 3 dB the rest is echo and goes down 30 dB (fading in 5 ms, back in 200 ms). Keyed on what the
/// filter removed, not on how loud the echo was, so speech over the far side stays: ducking on
/// the echo level (as an ffmpeg sidechain compressor does) kept 14 % of it on the test signal.
/// Muting 40 dB with a 50 ms release chopped speech so that whisper looped one sentence over
/// three minutes of a real call; -30 dB and 200 ms did not, and kept the most words.
const LEVEL: f32 = 1.0 / (0.010 * RATE as f32);
const REMOVED: f32 = 2.0;
const MUTE: f32 = 0.03;
const ATTACK: f32 = 1.0 / (0.005 * RATE as f32);
const RELEASE: f32 = 1.0 / (0.200 * RATE as f32);

/// What `cancel` found: how far the mic trails the system track, and how closely it follows it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Echo {
    pub lag_ms: i64,
    pub matched: f32,
}

/// Writes the mic without the loudspeaker echo to `out` and returns what it found, or `None`
/// (and no file) when the mic does not echo the system track. Both inputs are 16 kHz mono float
/// WAVs; they are read sample by sample, so a long call is never held in memory.
pub fn cancel(mic: &Path, system: &Path, out: &Path) -> Result<Option<Echo>, Box<dyn Error>> {
    let (lag, matched) = lag(&envelope(mic)?, &envelope(system)?);
    if matched < MIN_MATCH {
        return Ok(None);
    }
    // The reference is shifted so the echo arrives LEAD samples after it.
    let shift = lag * FRAME as i64 - LEAD as i64;
    let mut canceller = Canceller::new();
    // An untrained filter lets the first second of echo through, and whisper turned that into a
    // made-up line; so the filter learns on the opening seconds first, then starts over.
    pass(mic, system, shift, &mut canceller, WARMUP, None)?;
    canceller.restart();
    let mut writer = hound::WavWriter::create(out, hound::WavReader::open(mic)?.spec())?;
    pass(
        mic,
        system,
        shift,
        &mut canceller,
        usize::MAX,
        Some(&mut writer),
    )?;
    writer.finalize()?;
    Ok(Some(Echo {
        lag_ms: lag * (FRAME * 1000 / RATE) as i64,
        matched,
    }))
}

/// Runs up to `limit` mic samples through the canceller, writing the result if given a writer.
fn pass(
    mic: &Path,
    system: &Path,
    shift: i64,
    canceller: &mut Canceller,
    limit: usize,
    mut out: Option<&mut hound::WavWriter<BufWriter<File>>>,
) -> Result<(), Box<dyn Error>> {
    let mut mic = hound::WavReader::open(mic)?;
    let mut system = hound::WavReader::open(system)?;
    let length = u64::from(mic.duration());
    let delay = std::iter::repeat_with(|| Ok(0.0)).take(shift.max(0) as usize);
    let mut reference = delay
        .chain(system.samples::<f32>().skip((-shift).max(0) as usize))
        .chain(std::iter::repeat_with(|| Ok(0.0)));
    for (i, sample) in mic.samples::<f32>().take(limit).enumerate() {
        if i % RATE == 0 {
            crate::tools::check()?;
            if out.is_some() {
                crate::progress::step(i as u64, length);
            }
        }
        // The chain ends in endless zeros, so `next` always has a sample.
        let reference = reference.next().unwrap_or(Ok(0.0))?;
        let clean = canceller.push(sample?, reference);
        if let Some(out) = out.as_mut() {
            out.write_sample(clean)?;
        }
    }
    Ok(())
}

/// Mean absolute level of each 2 ms frame.
fn envelope(path: &Path) -> Result<Vec<f32>, Box<dyn Error>> {
    let mut reader = hound::WavReader::open(path)?;
    let (mut frames, mut sum, mut count) = (Vec::new(), 0.0, 0);
    for sample in reader.samples::<f32>() {
        sum += sample?.abs();
        count += 1;
        if count == FRAME {
            frames.push(sum / FRAME as f32);
            (sum, count) = (0.0, 0);
        }
    }
    Ok(frames)
}

/// The lag in frames (positive: the mic trails the system track) where the two envelopes
/// correlate best, and that correlation.
fn lag(mic: &[f32], system: &[f32]) -> (i64, f32) {
    let n = mic.len().min(system.len());
    if n <= 2 * MAX_LAG {
        return (0, 0.0);
    }
    let centred = |e: &[f32]| {
        let mean = e[..n].iter().sum::<f32>() / n as f32;
        e[..n].iter().map(|v| v - mean).collect::<Vec<_>>()
    };
    let (m, s) = (centred(mic), centred(system));
    let norm = (m.iter().map(|v| v * v).sum::<f32>() * s.iter().map(|v| v * v).sum::<f32>()).sqrt();
    if norm == 0.0 {
        return (0, 0.0);
    }
    (-(MAX_LAG as i64)..=MAX_LAG as i64)
        .map(|l| {
            let (m, s) = if l >= 0 {
                (&m[l as usize..], &s[..n - l as usize])
            } else {
                (&m[..n - (-l) as usize], &s[(-l) as usize..])
            };
            (l, dot(m, s) / norm)
        })
        .fold((0, 0.0), |best, c| if c.1 > best.1 { c } else { best })
}

/// Sum of products in eight lanes, so it vectorizes: the filter runs it on every sample.
fn dot(a: &[f32], b: &[f32]) -> f32 {
    let ((ca, ta), (cb, tb)) = (a.as_chunks::<8>(), b.as_chunks::<8>());
    let mut lanes = [0.0f32; 8];
    for (x, y) in ca.iter().zip(cb) {
        for k in 0..8 {
            lanes[k] += x[k] * y[k];
        }
    }
    lanes.iter().sum::<f32>() + ta.iter().zip(tb).map(|(x, y)| x * y).sum::<f32>()
}

/// NLMS echo canceller with residual ducking, one sample at a time.
struct Canceller {
    weights: Vec<f32>,
    /// The last TAPS reference samples, written twice so the window is one slice.
    history: Vec<f32>,
    pos: usize,
    power: f64,
    mic_level: f32,
    level: f32,
    gain: f32,
}

impl Canceller {
    fn new() -> Self {
        Self {
            weights: vec![0.0; TAPS],
            history: vec![0.0; 2 * TAPS],
            pos: 0,
            power: 0.0,
            mic_level: 0.0,
            level: 0.0,
            gain: 1.0,
        }
    }

    /// Back to the start of the call, keeping what the filter learned.
    fn restart(&mut self) {
        self.history.fill(0.0);
        (self.pos, self.power) = (0, 0.0);
        (self.mic_level, self.level, self.gain) = (0.0, 0.0, 1.0);
    }

    fn push(&mut self, mic: f32, reference: f32) -> f32 {
        let old = self.history[self.pos];
        self.history[self.pos] = reference;
        self.history[self.pos + TAPS] = reference;
        self.pos = (self.pos + 1) % TAPS;
        self.power =
            (self.power + f64::from(reference * reference) - f64::from(old * old)).max(0.0);
        let window = &self.history[self.pos..self.pos + TAPS];
        let echo = dot(window, &self.weights);
        let residual = mic - echo;
        let step = MU * residual / (EPS + self.power as f32);
        for (w, x) in self.weights.iter_mut().zip(window) {
            *w += step * x;
        }
        // Where you talk, the residual keeps most of the mic's power and passes. Mostly echo where
        // the filter took out over half the power; and where the residual is louder than the mic
        // (the mic gated to silence while the far side talked) the filter would add sound.
        self.mic_level += LEVEL * (mic * mic - self.mic_level);
        self.level += LEVEL * (residual * residual - self.level);
        let yours =
            self.level * REMOVED >= self.mic_level && self.level <= self.mic_level * REMOVED;
        let target = if yours { 1.0 } else { MUTE };
        let rate = if target < self.gain { ATTACK } else { RELEASE };
        self.gain += rate * (target - self.gain);
        residual * self.gain
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Speech-like test signal from a fixed seed: noise in 50 ms pieces, each voiced or not at
    /// random, so two seeds are two unrelated voices.
    fn talker(seed: u64, seconds: usize, on: impl Fn(usize) -> bool) -> Vec<f32> {
        let mut state = seed;
        let mut next = move || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((state >> 33) as f32 / (1u64 << 31) as f32) - 0.5
        };
        let mut voiced = false;
        (0..seconds * RATE)
            .map(|i| {
                if i % (RATE / 20) == 0 {
                    voiced = next() > -0.2;
                }
                let noise = next();
                if on(i / RATE) && voiced {
                    noise * 0.3
                } else {
                    0.0
                }
            })
            .collect()
    }

    /// The far side's sound reaching the mic: delayed, about 8 dB under your voice (as measured on
    /// a loudspeaker call), with one room reflection.
    fn echo_of(far: &[f32], delay_ms: usize) -> Vec<f32> {
        let d = delay_ms * RATE / 1000;
        let r = d + RATE * 12 / 1000;
        (0..far.len())
            .map(|i| {
                let at = |k: usize| if i >= k { far[i - k] } else { 0.0 };
                0.3 * at(d) + 0.1 * at(r)
            })
            .collect()
    }

    fn rms(signal: &[f32], seconds: std::ops::Range<usize>) -> f32 {
        let s = &signal[seconds.start * RATE..seconds.end * RATE];
        (s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt()
    }

    fn db(a: f32, b: f32) -> f32 {
        20.0 * (a / b).log10()
    }

    #[test]
    fn lag_is_found_either_way_and_unrelated_voices_do_not_match() {
        let far = talker(1, 20, |_| true);
        let frames = |s: &[f32]| {
            s.as_chunks::<FRAME>()
                .0
                .iter()
                .map(|c| c.iter().map(|v| v.abs()).sum::<f32>() / FRAME as f32)
                .collect::<Vec<_>>()
        };
        let (l, matched) = lag(&frames(&echo_of(&far, 60)), &frames(&far));
        assert!((l * 2 - 60).abs() <= 2 && matched > 0.5, "{l} {matched}");
        // The mic ahead of the system track (its timestamps lag): a negative lag.
        let mut early = echo_of(&far, 0);
        early.drain(..RATE / 10);
        let (l, _) = lag(&frames(&early), &frames(&far));
        assert!((l * 2 + 100).abs() <= 2, "{l}");
        let other = talker(2, 20, |_| true);
        assert!(lag(&frames(&other), &frames(&far)).1 < MIN_MATCH);
    }

    #[test]
    fn echo_goes_and_the_users_own_speech_stays() {
        // 0-10 s far side only, 10-15 s you only, 15-20 s both at once.
        let far = talker(3, 20, |s| !(10..15).contains(&s));
        let you = talker(4, 20, |s| s >= 10);
        let mic: Vec<f32> = echo_of(&far, 40)
            .iter()
            .zip(&you)
            .map(|(e, y)| e + y)
            .collect();
        let mut canceller = Canceller::new();
        let out: Vec<f32> = mic
            .iter()
            .zip(&far)
            .map(|(m, f)| canceller.push(*m, *f))
            .collect();
        // After a second of adapting, the echo is far below what the mic heard.
        assert!(db(rms(&out, 2..10), rms(&mic, 2..10)) < -40.0);
        // Alone, your speech passes unchanged; over the far side it stays at about -7 dB (the
        // 200 ms release that keeps whisper from looping costs some of it).
        assert!(db(rms(&out, 11..15), rms(&you, 11..15)).abs() < 1.0);
        let kept: f32 = out[16 * RATE..]
            .iter()
            .zip(&you[16 * RATE..])
            .map(|(o, y)| o * y)
            .sum::<f32>()
            / you[16 * RATE..].iter().map(|y| y * y).sum::<f32>();
        assert!(kept > 0.4, "{kept}");
    }

    #[test]
    fn a_mic_gated_to_silence_stays_silent() {
        // Some mics send zeros between your words, also while the far side talks: the filter's
        // echo estimate must not be played into that silence.
        let far = talker(7, 12, |_| true);
        let mic: Vec<f32> = echo_of(&far, 40)
            .iter()
            .enumerate()
            .map(|(i, e)| if (i / RATE) % 4 == 3 { 0.0 } else { *e })
            .collect();
        let mut canceller = Canceller::new();
        let out: Vec<f32> = mic
            .iter()
            .zip(&far)
            .map(|(m, f)| canceller.push(*m, *f))
            .collect();
        for s in [3, 7, 11] {
            assert!(rms(&out, s..s + 1) < 1e-3, "{s}: {}", rms(&out, s..s + 1));
        }
    }

    #[test]
    fn files_are_cleaned_only_when_the_mic_hears_the_speakers() {
        let dir = std::env::temp_dir().join(format!("motetto-aec-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let write = |name: &str, samples: &[f32]| {
            let path = dir.join(name);
            let spec = hound::WavSpec {
                channels: 1,
                sample_rate: RATE as u32,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            };
            let mut w = hound::WavWriter::create(&path, spec).unwrap();
            samples.iter().for_each(|s| w.write_sample(*s).unwrap());
            w.finalize().unwrap();
            path
        };
        let far = talker(5, 12, |_| true);
        let system = write("x.system.16k.wav", &far);
        let out = dir.join("x.mic.aec.wav");
        let echo = cancel(&write("x.mic.16k.wav", &echo_of(&far, 30)), &system, &out)
            .unwrap()
            .unwrap();
        assert!((echo.lag_ms - 30).abs() <= 2, "{echo:?}");
        assert_eq!(
            hound::WavReader::open(&out).unwrap().duration() as usize,
            far.len()
        );
        // Headphones: the mic hears only you, so nothing is written.
        let alone = dir.join("y.mic.aec.wav");
        assert_eq!(
            cancel(
                &write("y.mic.16k.wav", &talker(6, 12, |_| true)),
                &system,
                &alone
            )
            .unwrap(),
            None
        );
        assert!(!alone.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
