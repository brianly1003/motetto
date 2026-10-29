//! Audio buffers, real meters and fallible WAV writes for the macOS stream.

use crate::platform::macos_video::{Encoding, Input};
use screencapturekit::prelude::*;
use std::{
    error::Error,
    fs::File,
    io::{BufWriter, Seek, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
        mpsc,
    },
};

pub(super) type Wav = Arc<Mutex<Option<hound::WavWriter<BufWriter<File>>>>>;

pub(super) struct Track {
    pub(super) path: String,
    pub(super) wav: Wav,
    pub(super) errors: mpsc::Sender<Result<(), String>>,
    pub(super) level: Option<Arc<AtomicU32>>,
    pub(super) video: Option<Arc<Encoding>>,
    pub(super) input: Input,
    /// Mic only: clipped samples since the desktop last looked (`mics::too_loud`).
    pub(super) clips: Option<Arc<AtomicU32>>,
    /// Mic only: the live listener (`mics::Listener`).
    pub(super) listen: Option<crate::mics::Listener>,
}

impl SCStreamOutputTrait for Track {
    fn did_output_sample_buffer(&self, buf: CMSampleBuffer, _: SCStreamOutputType) {
        if let Err(error) = self.write_buffer(&buf) {
            // The receiver may already be gone while capture is stopping.
            let _ = self.errors.send(Err(format!(
                "cannot save {}: {error}; check free disk space and folder permissions",
                self.path
            )));
        } else if let Some(video) = &self.video
            && buf.num_samples() > 0
        {
            video.push(self.input, &buf);
        }
    }
}

impl Track {
    fn write_buffer(&self, buf: &CMSampleBuffer) -> Result<(), Box<dyn Error>> {
        let frames = buf.num_samples() as usize;
        let Ok(list) = buf.audio_buffer_list() else {
            return Ok(());
        };
        let Some(first) = list.get(0) else {
            return Ok(());
        };
        if frames == 0 {
            return Ok(());
        }
        // Built-in mics deliver f32; a Bluetooth headset mic in call mode delivers 16-bit integers.
        let format = buf.format_description();
        let float = format.as_ref().is_none_or(|f| f.audio_is_float());
        let bits = format
            .as_ref()
            .and_then(|f| f.audio_bits_per_channel())
            .unwrap_or(32);
        let samples = to_f32(first.data(), float, bits)
            .ok_or_else(|| format!("unsupported audio format ({bits}-bit, float: {float})"))?;
        // Non-interleaved: buffer 0 is channel 0. Interleaved: take every Nth sample.
        let stride = (samples.len() / frames).max(1);

        if let Some(level) = &self.level {
            level.store(
                rms(samples.iter().step_by(stride).copied()).to_bits(),
                Ordering::Relaxed,
            );
        }
        if let Some(listener) = &self.listen {
            let rate = format
                .as_ref()
                .and_then(|f| f.audio_sample_rate())
                .unwrap_or(48_000.0) as u32;
            send_listen(listener, &channel0(&samples, stride), rate);
        }
        if let Some(clips) = &self.clips {
            let n = samples
                .iter()
                .step_by(stride)
                .filter(|s| s.abs() >= crate::mics::CLIP)
                .count();
            clips.fetch_add(n as u32, Ordering::Relaxed);
        }
        let elapsed = self.video.as_ref().and_then(|video| video.audio_time(buf));
        let mut wav = self.wav.lock().map_err(|_| "audio writer lock poisoned")?;
        if wav.is_none() {
            let rate = buf
                .format_description()
                .and_then(|f| f.audio_sample_rate())
                .unwrap_or(48_000.0) as u32;
            let spec = hound::WavSpec {
                channels: 1,
                sample_rate: rate,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            };
            *wav = Some(hound::WavWriter::create(&self.path, spec)?);
        }
        if let Some(writer) = wav.as_mut() {
            let samples = samples.iter().step_by(stride).copied();
            if let Some(elapsed) = elapsed {
                let (gap, skip) = crate::video::audio_alignment(
                    writer.duration(),
                    elapsed,
                    writer.spec().sample_rate,
                )?;
                write_samples(writer, std::iter::repeat_n(0.0, gap))?;
                write_samples(writer, samples.skip(skip))?;
            } else {
                write_samples(writer, samples)?;
            }
        }
        Ok(())
    }
}

/// The channel the WAV records: every `stride`-th sample of an interleaved buffer.
fn channel0(samples: &[f32], stride: usize) -> Vec<f32> {
    samples.iter().step_by(stride.max(1)).copied().collect()
}

/// Hands `mono` to the live listener without waiting: capture never slows for it.
pub(super) fn send_listen(listener: &crate::mics::Listener, mono: &[f32], rate: u32) {
    if let Ok(slot) = listener.lock()
        && let Some((_, tx)) = slot.as_ref()
    {
        let _ = tx.try_send((mono.to_vec(), rate));
    }
}

fn to_f32(data: &[u8], float: bool, bits: u32) -> Option<Vec<f32>> {
    Some(match (float, bits) {
        (true, 32) => data
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_ne_bytes(*b))
            .collect(),
        (false, 16) => data
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| f32::from(i16::from_ne_bytes(*b)) / 32768.0)
            .collect(),
        (false, 32) => data
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| i32::from_ne_bytes(*b) as f32 / 2_147_483_648.0)
            .collect(),
        _ => return None,
    })
}

fn rms(samples: impl Iterator<Item = f32>) -> f32 {
    let (sum, count) = samples
        .filter(|v| v.is_finite())
        .fold((0.0_f64, 0_u64), |(sum, count), v| {
            (sum + f64::from(v).powi(2), count + 1)
        });
    if count == 0 {
        0.0
    } else {
        ((sum / count as f64).sqrt() as f32).min(1.0)
    }
}

fn write_samples<W: Write + Seek>(
    writer: &mut hound::WavWriter<W>,
    samples: impl Iterator<Item = f32>,
) -> Result<(), hound::Error> {
    for sample in samples {
        writer.write_sample(sample)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{self, Cursor, SeekFrom};
    use std::sync::atomic::AtomicBool;
    #[test]
    fn listener_gets_what_the_wav_gets() {
        // Interleaved stereo: channel 0, as the WAV keeps.
        assert_eq!(channel0(&[0.2, 0.4, -0.2, 0.0], 2), vec![0.2, -0.2]);
        assert_eq!(channel0(&[0.5, 0.25], 1), vec![0.5, 0.25]);
    }

    #[test]
    fn an_old_listener_never_clears_a_new_one() {
        let listener: crate::mics::Listener = Arc::default();
        let (old_tx, _old_rx) = mpsc::sync_channel(1);
        let old = crate::mics::listen(&listener, old_tx);
        let (new_tx, new_rx) = mpsc::sync_channel(1);
        let new = crate::mics::listen(&listener, new_tx);
        crate::mics::unlisten(&listener, old); // the old follower ending
        send_listen(&listener, &[0.1; 4], 48_000);
        assert!(
            new_rx.try_recv().is_ok(),
            "the new follower still hears the mic"
        );
        crate::mics::unlisten(&listener, new);
        assert!(listener.lock().unwrap().is_none());
    }

    #[test]
    fn full_listener_never_blocks() {
        let (tx, rx) = mpsc::sync_channel(1);
        let listener: crate::mics::Listener = Arc::default();
        crate::mics::listen(&listener, tx);
        for _ in 0..3 {
            send_listen(&listener, &[0.1; 480], 48_000);
        }
        assert_eq!(rx.try_recv().unwrap(), (vec![0.1; 480], 48_000));
        assert!(rx.try_recv().is_err());
        drop(rx);
        send_listen(&listener, &[0.1; 480], 48_000); // a gone listener is fine too
        let none: crate::mics::Listener = Arc::default();
        send_listen(&none, &[0.1; 480], 48_000);
    }

    #[test]
    fn meter_uses_real_signal_and_rejects_non_finite_samples() {
        assert_eq!(rms([0.0, 0.0].into_iter()), 0.0);
        assert_eq!(rms([0.5, -0.5].into_iter()), 0.5);
        assert_eq!(rms([f32::NAN, f32::INFINITY].into_iter()), 0.0);
        assert_eq!(rms([4.0].into_iter()), 1.0);
    }

    #[test]
    fn integer_mic_buffers_become_float_samples() {
        let int16: Vec<u8> = [16384_i16, -32768]
            .iter()
            .flat_map(|v| v.to_ne_bytes())
            .collect();
        assert_eq!(to_f32(&int16, false, 16), Some(vec![0.5, -1.0]));
        assert_eq!(to_f32(&0.25_f32.to_ne_bytes(), true, 32), Some(vec![0.25]));
        assert_eq!(
            to_f32(&(i32::MIN).to_ne_bytes(), false, 32),
            Some(vec![-1.0])
        );
        assert_eq!(to_f32(&[0; 3], false, 24), None);
    }
    struct FailingWriter {
        bytes: Cursor<Vec<u8>>,
        fail: Arc<AtomicBool>,
    }

    impl Write for FailingWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.fail.load(Ordering::Relaxed) {
                return Err(io::Error::other("disk full"));
            }
            self.bytes.write(bytes)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Seek for FailingWriter {
        fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
            self.bytes.seek(pos)
        }
    }

    #[test]
    fn sample_write_failure_is_returned() {
        let fail = Arc::new(AtomicBool::new(false));
        let mut writer = hound::WavWriter::new(
            FailingWriter {
                bytes: Cursor::new(Vec::new()),
                fail: fail.clone(),
            },
            hound::WavSpec {
                channels: 1,
                sample_rate: 48000,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .unwrap();
        write_samples(&mut writer, [0.5].into_iter()).unwrap();
        fail.store(true, Ordering::Relaxed);
        assert!(
            write_samples(&mut writer, [0.5].into_iter())
                .unwrap_err()
                .to_string()
                .contains("disk full")
        );
    }
}
