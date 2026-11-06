//! Trimming a saved screen recording at both ends: the kept range, the parts it touches, and the
//! WAVs cut to the same range. The video is joined and cut by the platform (`export`).

use std::{
    io,
    path::{Path, PathBuf},
};

#[cfg(target_os = "macos")]
pub use crate::platform::macos_trim::{duration_ms, export};

/// Joining parts without re-encoding uses AVFoundation; other systems have no trim yet.
#[cfg(not(target_os = "macos"))]
pub fn export(_parts: &[(PathBuf, u64, u64)], _dst: &Path) -> Result<(), String> {
    Err("Trimming a video needs macOS".into())
}
#[cfg(not(target_os = "macos"))]
pub fn duration_ms(_path: &Path) -> Result<u64, String> {
    Err("Trimming a video needs macOS".into())
}

/// The shortest part a trim keeps, as the trim strip's handles allow.
pub const MIN_KEPT_MS: u64 = 1000;

/// The kept range of a recording, in ms on its own timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrimRange {
    pub from_ms: u64,
    pub to_ms: u64,
}

impl TrimRange {
    /// Keeps at least 1 s, ends inside the recording and cuts something.
    pub fn new(from_ms: u64, to_ms: u64, total_ms: u64) -> Result<Self, String> {
        if to_ms > total_ms {
            return Err("The trim ends after the recording does".into());
        }
        if to_ms.saturating_sub(from_ms) < MIN_KEPT_MS {
            return Err("Keep at least 1 second of the recording".into());
        }
        if from_ms == 0 && to_ms == total_ms {
            return Err("Move a handle to cut something first".into());
        }
        Ok(Self { from_ms, to_ms })
    }
}

/// Parts as (offset_ms, duration_ms) on the recording's timeline, to the kept span of each part
/// the range touches: (index, from_ms, to_ms) in that part's own time. Parts outside are skipped.
pub fn plan_parts(parts: &[(u64, u64)], range: TrimRange) -> Vec<(usize, u64, u64)> {
    parts
        .iter()
        .enumerate()
        .filter_map(|(index, &(offset, duration))| {
            let from = range.from_ms.max(offset);
            let to = range.to_ms.min(offset.saturating_add(duration));
            (from < to).then(|| (index, from - offset, to - offset))
        })
        .collect()
}

/// Where a time on the recording's timeline lands after the trim, which plays the kept spans one
/// after another from 0; None when it was cut.
pub fn shift(parts: &[(u64, u64)], range: TrimRange, ms: u64) -> Option<u64> {
    let mut at = 0;
    for (index, from, to) in plan_parts(parts, range) {
        let start = parts[index].0 + from;
        if (start..parts[index].0 + to).contains(&ms) {
            return Some(at + ms - start);
        }
        at += to - from;
    }
    None
}

/// Copies `from_ms..to_ms` of a WAV into a new WAV in the same format (any rate, channels and
/// sample type hound reads). Streams in blocks; never holds the file.
pub fn trim_wav(src: &Path, dst: &Path, from_ms: u64, to_ms: u64) -> io::Result<()> {
    join_wavs(&[(Some(src.to_path_buf()), from_ms, to_ms)], dst)
}

/// Writes spans of WAVs one after another into `dst`: each is `from_ms..to_ms` of its file, or
/// silence for a part that has no file for this track. Each span holds exactly its frames, with
/// silence past a file's end, so later parts stay in step with the video. Every file must share
/// the first one's format; converting between parts is not done.
pub fn join_wavs(spans: &[(Option<PathBuf>, u64, u64)], dst: &Path) -> io::Result<()> {
    let mut readers = vec![];
    for (path, from_ms, to_ms) in spans {
        let reader = path
            .as_deref()
            .map(hound::WavReader::open)
            .transpose()
            .map_err(io_error)?;
        readers.push((reader, *from_ms, *to_ms));
    }
    let spec = readers
        .iter()
        .find_map(|(reader, ..)| reader.as_ref().map(hound::WavReader::spec))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no audio to trim"))?;
    if spec.sample_rate == 0 || spec.channels == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid audio format",
        ));
    }
    if readers
        .iter()
        .any(|(reader, ..)| reader.as_ref().is_some_and(|r| r.spec() != spec))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the parts' audio formats differ, so they can't be joined",
        ));
    }
    let mut writer = hound::WavWriter::create(dst, spec).map_err(io_error)?;
    let frames = |ms: u64| ms.saturating_mul(u64::from(spec.sample_rate)) / 1000;
    for (reader, from_ms, to_ms) in &mut readers {
        let (start, count) = (
            frames(*from_ms),
            frames(*to_ms).saturating_sub(frames(*from_ms)),
        );
        match spec.sample_format {
            hound::SampleFormat::Float => copy::<f32>(reader.as_mut(), &mut writer, start, count),
            hound::SampleFormat::Int => copy::<i32>(reader.as_mut(), &mut writer, start, count),
        }?;
    }
    writer.finalize().map_err(io_error)
}

type Reader = hound::WavReader<io::BufReader<std::fs::File>>;
type Writer = hound::WavWriter<io::BufWriter<std::fs::File>>;

/// `count` frames from frame `start` (hound's buffered reader and writer keep memory bounded),
/// then silence for whatever the file doesn't reach.
fn copy<S: hound::Sample + Default>(
    reader: Option<&mut Reader>,
    writer: &mut Writer,
    start: u64,
    count: u64,
) -> io::Result<()> {
    let channels = u64::from(writer.spec().channels);
    let mut copied = 0;
    if let Some(reader) = reader {
        let start = start.min(u64::from(reader.duration()));
        let start = u32::try_from(start).map_err(io::Error::other)?;
        reader.seek(start)?;
        for sample in reader.samples::<S>().take((count * channels) as usize) {
            writer
                .write_sample(sample.map_err(io_error)?)
                .map_err(io_error)?;
            copied += 1;
        }
    }
    for _ in copied..count * channels {
        writer.write_sample(S::default()).map_err(io_error)?;
    }
    Ok(())
}

fn io_error(error: hound::Error) -> io::Error {
    match error {
        hound::Error::IoError(error) => error,
        error => io::Error::new(io::ErrorKind::InvalidData, error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("motetto-trim-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A WAV of `seconds` whose every frame holds its own index, so a cut shows where it began.
    fn wav(path: &Path, rate: u32, channels: u16, bits: u16, seconds: u32) {
        let format = if bits == 32 {
            hound::SampleFormat::Float
        } else {
            hound::SampleFormat::Int
        };
        let spec = hound::WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: bits,
            sample_format: format,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for frame in 0..rate * seconds {
            for _ in 0..channels {
                if bits == 32 {
                    writer.write_sample(frame as f32 / 1e6).unwrap();
                } else {
                    writer.write_sample((frame % 30_000) as i16).unwrap();
                }
            }
        }
        writer.finalize().unwrap();
    }

    /// RIFF and data chunk sizes as written, and the file's length.
    fn sizes(path: &Path) -> (u32, u32, u64) {
        let bytes = fs::read(path).unwrap();
        let riff = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        let mut at = 12;
        while &bytes[at..at + 4] != b"data" {
            at += 8 + u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        }
        let data = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap());
        (riff, data, bytes.len() as u64)
    }

    fn check_cut(rate: u32, channels: u16, bits: u16) {
        let dir = temp(&format!("{rate}-{channels}-{bits}"));
        let (src, dst) = (dir.join("in.wav"), dir.join("out.wav"));
        wav(&src, rate, channels, bits, 3);
        trim_wav(&src, &dst, 1000, 2000).unwrap();
        let mut reader = hound::WavReader::open(&dst).unwrap();
        assert_eq!(reader.spec(), hound::WavReader::open(&src).unwrap().spec());
        assert_eq!(reader.duration(), rate);
        assert_eq!(reader.len(), rate * u32::from(channels));
        let (riff, data, len) = sizes(&dst);
        assert_eq!(data, rate * u32::from(channels) * u32::from(bits / 8));
        assert_eq!(u64::from(riff), len - 8);
        // The first kept frame is frame `rate` of the source.
        if bits == 32 {
            let first = reader.samples::<f32>().next().unwrap().unwrap();
            assert_eq!(first, rate as f32 / 1e6);
        } else {
            let first = reader.samples::<i32>().next().unwrap().unwrap();
            assert_eq!(first, (rate % 30_000) as i32);
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn range_rules() {
        assert!(TrimRange::new(0, 999, 5000).is_err());
        assert!(TrimRange::new(0, 5000, 5000).is_err());
        assert!(TrimRange::new(100, 6000, 5000).is_err());
        assert!(TrimRange::new(3000, 2000, 5000).is_err());
        assert_eq!(
            TrimRange::new(1600, 4000, 5000),
            Ok(TrimRange {
                from_ms: 1600,
                to_ms: 4000
            })
        );
    }

    #[test]
    fn plan_spans_two_parts() {
        let parts = [(0, 4000), (5000, 3000)];
        let range = TrimRange::new(2000, 6000, 8000).unwrap();
        assert_eq!(
            plan_parts(&parts, range),
            vec![(0, 2000, 4000), (1, 0, 1000)]
        );
        // A range inside the gap between parts and one past the first part skip what's outside.
        let late = TrimRange::new(4500, 8000, 8000).unwrap();
        assert_eq!(plan_parts(&parts, late), vec![(1, 0, 3000)]);
        // Meeting times move to the trimmed timeline; cut ones are gone.
        assert_eq!(shift(&parts, range, 2000), Some(0));
        assert_eq!(shift(&parts, range, 3999), Some(1999));
        assert_eq!(shift(&parts, range, 4500), None);
        assert_eq!(shift(&parts, range, 5500), Some(2500));
        assert_eq!(shift(&parts, range, 6000), None);
        assert_eq!(shift(&parts, range, 1000), None);
    }

    #[test]
    fn trim_wav_keeps_samples() {
        check_cut(16_000, 1, 16);
        check_cut(16_000, 1, 32);
    }

    #[test]
    fn trim_wav_48k_stereo() {
        check_cut(48_000, 2, 16);
        check_cut(48_000, 2, 32);
    }

    #[test]
    fn parts_join_in_order_and_silence_fills_a_missing_track() {
        let dir = temp("join");
        let (a, b, out) = (dir.join("a.wav"), dir.join("b.wav"), dir.join("out.wav"));
        wav(&a, 16_000, 1, 16, 2);
        wav(&b, 16_000, 1, 16, 1);
        join_wavs(
            &[
                (Some(a.clone()), 1500, 2000),
                (None, 0, 250),
                (Some(b.clone()), 0, 500),
                // Past the file's end: silence keeps the length the video has.
                (Some(b.clone()), 900, 1100),
            ],
            &out,
        )
        .unwrap();
        let mut reader = hound::WavReader::open(&out).unwrap();
        assert_eq!(reader.duration(), 8000 + 4000 + 8000 + 3200);
        let samples: Vec<i32> = reader.samples::<i32>().map(Result::unwrap).collect();
        assert_eq!(samples[0], 24_000);
        assert!(samples[8000..12_000].iter().all(|s| *s == 0));
        assert_eq!(samples[12_000], 0);
        assert_eq!(samples[12_001], 1);
        assert_eq!(samples[20_000], 14_400);
        assert!(samples[21_600..].iter().all(|s| *s == 0));
        // Parts recorded in different formats can't be joined without converting.
        let c = dir.join("c.wav");
        wav(&c, 48_000, 1, 16, 1);
        assert!(join_wavs(&[(Some(a), 0, 500), (Some(c), 0, 500)], &out).is_err());
        assert!(join_wavs(&[(None, 0, 500)], &out).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}
