//! Place mono float samples on a shared recording clock, retaining silent gaps.

use std::{
    error::Error,
    io::{Seek, Write},
};

pub(super) fn packet<W: Write + Seek>(
    writer: &mut hound::WavWriter<W>,
    written: &mut u64,
    start: u64,
    frames: usize,
    channels: usize,
    bytes: &[u8],
    silent: bool,
) -> Result<(), Box<dyn Error>> {
    let size = frames
        .checked_mul(channels)
        .and_then(|v| v.checked_mul(4))
        .ok_or("audio packet too large")?;
    if channels == 0 || (!silent && bytes.len() != size) {
        return Err("invalid float audio packet".into());
    }
    while *written < start {
        super::wav_room(writer.len())?;
        writer.write_sample(0.0_f32)?;
        *written += 1;
    }
    let skip = written.saturating_sub(start).min(frames as u64) as usize;
    for frame in skip..frames {
        let value = if silent {
            0.0
        } else {
            let mut sum = 0.0_f64;
            for chunk in bytes[frame * channels * 4..(frame + 1) * channels * 4]
                .as_chunks::<4>()
                .0
            {
                let sample = f32::from_le_bytes(*chunk);
                if !sample.is_finite() {
                    return Err("non-finite captured audio".into());
                }
                sum += f64::from(sample);
            }
            (sum / channels as f64) as f32
        };
        super::wav_room(writer.len())?;
        writer.write_sample(value)?;
        *written += 1;
    }
    Ok(())
}

pub(super) fn frame_at(timestamp: u64, epoch: u64, rate: u32) -> Result<u64, Box<dyn Error>> {
    let frames = u128::from(timestamp.saturating_sub(epoch)) * u128::from(rate) / 10_000_000;
    Ok(frames
        .try_into()
        .map_err(|_| "capture timestamp overflow")?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    #[test]
    fn silence_gaps_mixing_and_overlap_keep_timeline() {
        let mut bytes = Cursor::new(Vec::new());
        {
            let mut wav = hound::WavWriter::new(
                &mut bytes,
                hound::WavSpec {
                    channels: 1,
                    sample_rate: 16000,
                    bits_per_sample: 32,
                    sample_format: hound::SampleFormat::Float,
                },
            )
            .unwrap();
            let mut written = 0;
            let samples: Vec<u8> = [0.5_f32, 1.0, 0.0, 0.5]
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect();
            packet(&mut wav, &mut written, 2, 2, 2, &samples, false).unwrap();
            packet(&mut wav, &mut written, 3, 2, 1, &[], true).unwrap();
            assert_eq!(written, 5);
            wav.flush().unwrap();
            wav.finalize().unwrap();
        }
        bytes.set_position(0);
        let samples = hound::WavReader::new(bytes)
            .unwrap()
            .samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(samples, [0.0, 0.0, 0.75, 0.25, 0.0]);
        assert_eq!(frame_at(30_000_000, 10_000_000, 48000).unwrap(), 96000);
        assert_eq!(frame_at(1, 2, 48000).unwrap(), 0);
    }
    #[test]
    fn malformed_packets_are_errors() {
        let mut wav = hound::WavWriter::new(
            Cursor::new(Vec::new()),
            hound::WavSpec {
                channels: 1,
                sample_rate: 16000,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .unwrap();
        assert!(packet(&mut wav, &mut 0, 0, 1, 0, &[], false).is_err());
        assert!(packet(&mut wav, &mut 0, 0, 1, 1, &[0], false).is_err());
        assert!(packet(&mut wav, &mut 0, 0, 1, 1, &f32::NAN.to_le_bytes(), false).is_err());
    }
}

#[cfg(test)]
mod failure_tests {
    use super::*;
    use std::{
        cell::Cell,
        io::{self, Cursor, SeekFrom},
        rc::Rc,
    };
    struct Disk {
        bytes: Cursor<Vec<u8>>,
        fail: Rc<Cell<bool>>,
    }
    impl Write for Disk {
        fn write(&mut self, data: &[u8]) -> io::Result<usize> {
            if self.fail.get() {
                return Err(io::Error::other("disk full"));
            }
            self.bytes.write(data)
        }
        fn flush(&mut self) -> io::Result<()> {
            if self.fail.get() {
                Err(io::Error::other("disk full"))
            } else {
                Ok(())
            }
        }
    }
    impl Seek for Disk {
        fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
            self.bytes.seek(pos)
        }
    }
    #[test]
    fn failed_gap_write_does_not_advance_track() {
        let fail = Rc::new(Cell::new(false));
        let mut wav = hound::WavWriter::new(
            Disk {
                bytes: Cursor::new(Vec::new()),
                fail: fail.clone(),
            },
            hound::WavSpec {
                channels: 1,
                sample_rate: 16000,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .unwrap();
        fail.set(true);
        let mut written = 0;
        assert!(packet(&mut wav, &mut written, 10, 0, 1, &[], true).is_err());
        assert_eq!(written, 0);
        assert!(wav.flush().is_err());
        assert!(wav.finalize().is_err());
    }
}
