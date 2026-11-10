//! Reduce noise (a microphone mode): RNNoise takes steady background noise (fans, hum, keyboard)
//! out of the mic, on Motetto's own 16 kHz copy, after the call. Whisper does not hear the
//! denoised copy: speech enhancement before Whisper tends to raise its error rate, since Whisper
//! is trained on noisy audio. The denoised copy is played back and tells where the user speaks;
//! `gate` then silences the rest of the mic Whisper hears, so noise alone can't become words.
use std::error::Error;
use std::fs;
use std::path::Path;

use nnnoiseless::DenoiseState;

use crate::decode::{self, Mono};

/// RNNoise works on 10 ms frames of 48 kHz audio scaled like 16-bit samples.
const RATE: u32 = 48_000;
const FRAME: usize = DenoiseState::FRAME_SIZE;
const SCALE: f32 = 32_768.0;
/// Its overlap-add window delays the output by one frame; that much is dropped at the start and
/// flushed at the end, so the copy lines up with the mic sample for sample.
const DELAY: usize = FRAME;

/// Writes `input16` (16 kHz mono float) without steady background noise to `out`, the same
/// length. Streams through a 48 kHz temporary file next to `out` (about 0.7 GB an hour of audio,
/// removed when done); an in-memory resampler chain would avoid it if disk space matters.
pub fn clean(input16: &Path, out: &Path) -> Result<(), Box<dyn Error>> {
    let wide = out.with_extension("48k.tmp");
    let partial = out.with_extension("tmp");
    let result = (|| {
        decode::resample(input16, &wide, RATE)?;
        denoise_to(&wide, &partial)?;
        fs::rename(&partial, out)?;
        Ok(())
    })();
    let _ = fs::remove_file(&wide);
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result
}

fn denoise_to(wide: &Path, out: &Path) -> Result<(), Box<dyn Error>> {
    let mut reader = hound::WavReader::open(wide)?;
    let total = u64::from(reader.duration());
    let mut writer = hound::WavWriter::create(out, spec16())?;
    let mut mono = Mono::new(RATE, 16_000)?;
    let mut state = DenoiseState::new();
    let (mut input, mut output) = ([0.0f32; FRAME], [0.0f32; FRAME]);
    let (mut filled, mut skip, mut emitted, mut read) = (0, DELAY, 0u64, 0u64);
    let mut frame = |input: &[f32; FRAME],
                     output: &mut [f32; FRAME],
                     mono: &mut Mono,
                     writer: &mut decode::Writer,
                     skip: &mut usize,
                     emitted: &mut u64|
     -> Result<(), Box<dyn Error>> {
        state.process_frame(output, input);
        for &sample in output.iter() {
            if *skip > 0 {
                *skip -= 1;
            } else if *emitted < total {
                mono.push(sample / SCALE, writer)?;
                *emitted += 1;
            }
        }
        Ok(())
    };
    for sample in reader.samples::<f32>() {
        input[filled] = sample? * SCALE;
        filled += 1;
        read += 1;
        if filled == FRAME {
            frame(
                &input,
                &mut output,
                &mut mono,
                &mut writer,
                &mut skip,
                &mut emitted,
            )?;
            filled = 0;
            crate::progress::step(read, total);
        }
    }
    // The last partial frame and the delay, padded with silence.
    while emitted < total {
        input[filled..].fill(0.0);
        frame(
            &input,
            &mut output,
            &mut mono,
            &mut writer,
            &mut skip,
            &mut emitted,
        )?;
        filled = 0;
    }
    mono.finish(&mut writer)?;
    writer.finalize()?;
    Ok(())
}

fn spec16() -> hound::WavSpec {
    hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    }
}

/// Speech is kept this long either side of what the speech check found, so word edges stay.
const PAD_MS: u64 = 200;
/// Fade in and out over 10 ms, so a cut does not click.
const FADE: f32 = 160.0;

/// Writes `input16` to `out` with everything outside `speech` (ms spans, sorted) silenced.
pub fn gate(input16: &Path, speech: &[(u64, u64)], out: &Path) -> Result<(), Box<dyn Error>> {
    let mut reader = hound::WavReader::open(input16)?;
    let partial = out.with_extension("tmp");
    let mut writer = hound::WavWriter::create(&partial, spec16())?;
    let spans: Vec<(f32, f32)> = speech
        .iter()
        .map(|&(s, e)| {
            (
                (s.saturating_sub(PAD_MS) * 16) as f32,
                (e.saturating_add(PAD_MS) * 16) as f32,
            )
        })
        .collect();
    let mut next = 0;
    let result = (|| -> Result<(), Box<dyn Error>> {
        for (i, sample) in reader.samples::<f32>().enumerate() {
            let at = i as f32;
            while next < spans.len() && spans[next].1 + FADE <= at {
                next += 1;
            }
            let gain = spans.get(next).map_or(0.0, |&(start, end)| {
                let outside = (start - at).max(at - end).max(0.0);
                (1.0 - outside / FADE).clamp(0.0, 1.0)
            });
            writer.write_sample(sample? * gain)?;
        }
        writer.finalize()?;
        fs::rename(&partial, out)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("motetto-denoise-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }
    fn write(path: &Path, samples: &[f32]) {
        let mut w = hound::WavWriter::create(path, spec16()).unwrap();
        for &s in samples {
            w.write_sample(s).unwrap();
        }
        w.finalize().unwrap();
    }
    fn read(path: &Path) -> Vec<f32> {
        hound::WavReader::open(path)
            .unwrap()
            .samples::<f32>()
            .map(Result::unwrap)
            .collect()
    }
    fn rms(s: &[f32]) -> f32 {
        (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt()
    }
    /// Deterministic white noise in -1..1.
    fn noise(n: usize, seed: &mut u32) -> Vec<f32> {
        (0..n)
            .map(|_| {
                *seed ^= *seed << 13;
                *seed ^= *seed >> 17;
                *seed ^= *seed << 5;
                (*seed as f32 / u32::MAX as f32) * 2.0 - 1.0
            })
            .collect()
    }
    /// A voiced sound: 120 Hz with harmonics, swelling like a syllable four times a second.
    fn voice(n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let t = i as f32 / 16_000.0;
                let syllable = (std::f32::consts::PI * 4.0 * t).sin().abs();
                let tone: f32 = (1..12)
                    .map(|h| (2.0 * std::f32::consts::PI * 120.0 * h as f32 * t).sin() / h as f32)
                    .sum();
                0.2 * syllable * tone
            })
            .collect()
    }

    #[test]
    fn keeps_length() {
        let dir = dir("length");
        let n = 16_000 * 3 + 77;
        write(&dir.join("in.wav"), &voice(n));
        clean(&dir.join("in.wav"), &dir.join("out.wav")).unwrap();
        assert_eq!(read(&dir.join("out.wav")).len(), n);
        let left: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(left.len(), 2, "temporary files left behind: {left:?}");
    }

    #[test]
    fn quiet_noise_gets_quieter() {
        let dir = dir("noise");
        let hiss: Vec<f32> = noise(16_000 * 4, &mut 7).iter().map(|x| x * 0.03).collect();
        write(&dir.join("in.wav"), &hiss);
        clean(&dir.join("in.wav"), &dir.join("out.wav")).unwrap();
        let out = read(&dir.join("out.wav"));
        // Skip the first second, while the filter learns the noise.
        let ratio = rms(&out[16_000..]) / rms(&hiss[16_000..]);
        assert!(ratio < 0.5, "noise only down to {ratio}");
    }

    #[test]
    fn voice_stays_in_place() {
        let dir = dir("aligned");
        let mut seed = 11;
        let speech = voice(16_000 * 3);
        let input: Vec<f32> = speech
            .iter()
            .zip(noise(speech.len(), &mut seed))
            .map(|(v, n)| v + n * 0.01)
            .collect();
        write(&dir.join("in.wav"), &input);
        clean(&dir.join("in.wav"), &dir.join("out.wav")).unwrap();
        let out = read(&dir.join("out.wav"));
        // The best lag between voice and output is 0 (within 1 ms): timings stay valid.
        let corr = |lag: i64| -> f32 {
            (16_000..32_000)
                .map(|i| speech[i] * out[(i as i64 + lag) as usize])
                .sum()
        };
        let best = (-160..=160)
            .max_by(|a, b| corr(*a).total_cmp(&corr(*b)))
            .unwrap();
        assert!(best.abs() <= 16, "denoised voice moved {best} samples");
        assert!(
            rms(&out[16_000..]) > rms(&speech[16_000..]) * 0.5,
            "voice removed"
        );
    }

    #[test]
    fn gate_zeroes_outside_speech() {
        let dir = dir("gate");
        write(&dir.join("in.wav"), &vec![0.5; 16_000 * 6]);
        gate(
            &dir.join("in.wav"),
            &[(1_000, 2_000), (4_000, 4_500)],
            &dir.join("out.wav"),
        )
        .unwrap();
        let out = read(&dir.join("out.wav"));
        assert_eq!(out.len(), 16_000 * 6);
        assert_eq!(out[16_000 / 2], 0.0); // 0.5 s: silence before
        assert_eq!(out[16_000 * 3], 0.0); // 3 s: between spans
        assert_eq!(out[16_000 * 5 + 8_000], 0.0); // 5.5 s: after
        assert_eq!(out[16_000 + 8_000], 0.5); // 1.5 s: speech kept
        assert_eq!(out[(800 * 16) + 8], 0.5); // 0.8 s: inside the 200 ms pad
        assert_eq!(out[4_600 * 16], 0.5); // 4.6 s: pad after
    }
}
