//! Save a clip: a range of one track (or both summed) of a saved recording, written as its own
//! WAV with the recorded samples untouched. The meeting is only read.

use crate::trim::{TrimRange, join_wavs, plan_parts};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub const MIN_MS: u64 = 500;
pub const MAX_MS: u64 = 10 * 60 * 1000;
pub const MAX_BINS: usize = 400;
pub const NOTHING: &str = "Nothing was recorded on this track here.";
pub const CANT_MIX: &str =
    "You and Them were recorded in formats that can't be mixed. Save them one at a time.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Track {
    Me,
    Them,
    Both,
}

impl Track {
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "Me" => Ok(Self::Me),
            "Them" => Ok(Self::Them),
            "Both" => Ok(Self::Both),
            other => Err(format!("Unknown track {other}")),
        }
    }
}

/// One recorded part on the meeting's timeline and its raw track files (None: that track was off).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    pub offset_ms: u64,
    pub duration_ms: u64,
    pub mic: Option<PathBuf>,
    pub system: Option<PathBuf>,
}

pub fn check_range(from_ms: u64, to_ms: u64, total_ms: u64) -> Result<(), String> {
    if to_ms > total_ms {
        return Err("The clip ends after the recording does".into());
    }
    let length = to_ms.saturating_sub(from_ms);
    if length < MIN_MS {
        return Err("Keep at least half a second".into());
    }
    if length > MAX_MS {
        return Err("A clip can be at most 10 minutes".into());
    }
    Ok(())
}

/// Each touched part's span of one track, in that part's own time; parts play one after another
/// as in Trim, so a pause between parts is not in the clip.
fn spans(parts: &[Part], mic: bool, from_ms: u64, to_ms: u64) -> Vec<(Option<PathBuf>, u64, u64)> {
    let timeline: Vec<(u64, u64)> = parts.iter().map(|p| (p.offset_ms, p.duration_ms)).collect();
    plan_parts(&timeline, TrimRange { from_ms, to_ms })
        .into_iter()
        .map(|(index, from, to)| {
            let part = &parts[index];
            (
                (if mic { &part.mic } else { &part.system }).clone(),
                from,
                to,
            )
        })
        .collect()
}

pub fn write(
    parts: &[Part],
    track: Track,
    from_ms: u64,
    to_ms: u64,
    dst: &Path,
) -> Result<(), String> {
    let silent = |mic: bool| {
        spans(parts, mic, from_ms, to_ms)
            .iter()
            .all(|(path, ..)| path.is_none())
    };
    let empty = match track {
        Track::Me => silent(true),
        Track::Them => silent(false),
        Track::Both => silent(true) && silent(false),
    };
    if empty {
        return Err(NOTHING.into());
    }
    let me = || spans(parts, true, from_ms, to_ms);
    let them = || spans(parts, false, from_ms, to_ms);
    match track {
        // Both where one side recorded nothing here is just the other side.
        Track::Me | Track::Both if silent(false) => {
            join_wavs(&me(), dst).map_err(|e| e.to_string())
        }
        Track::Them | Track::Both if silent(true) => {
            join_wavs(&them(), dst).map_err(|e| e.to_string())
        }
        Track::Me => join_wavs(&me(), dst).map_err(|e| e.to_string()),
        Track::Them => join_wavs(&them(), dst).map_err(|e| e.to_string()),
        Track::Both => {
            let (a, b) = (dst.with_extension("me.tmp"), dst.with_extension("them.tmp"));
            let result = join_wavs(&me(), &a)
                .and_then(|()| join_wavs(&them(), &b))
                .map_err(|e| e.to_string())
                .and_then(|()| mix(&a, &b, dst));
            let _ = fs::remove_file(&a);
            let _ = fs::remove_file(&b);
            if result.is_err() {
                let _ = fs::remove_file(dst);
            }
            result
        }
    }
}

/// The two tracks summed frame by frame. A track at a lower rate (a Bluetooth headset mic at
/// 16 or 24 kHz beside 48 kHz call audio) is first resampled to the higher one; a single track
/// is never resampled, so it stays as recorded. Float sums are not clamped: a float WAV holds
/// values over 1.0.
fn mix(a: &Path, b: &Path, dst: &Path) -> Result<(), String> {
    let (rate_a, rate_b) = (open(a)?.spec().sample_rate, open(b)?.spec().sample_rate);
    if rate_a != rate_b {
        let (low, high) = if rate_a < rate_b {
            (a, rate_b)
        } else {
            (b, rate_a)
        };
        let raised = low.with_extension("rate.tmp");
        let result = crate::decode::resample(low, &raised, high)
            .map_err(|_| CANT_MIX.to_string())
            .and_then(|()| fs::rename(&raised, low).map_err(|e| e.to_string()));
        let _ = fs::remove_file(&raised);
        result?;
    }
    let (mut ra, mut rb) = (open(a)?, open(b)?);
    if ra.spec() != rb.spec() {
        return Err(CANT_MIX.into());
    }
    let spec = ra.spec();
    let mut writer = hound::WavWriter::create(dst, spec).map_err(|e| e.to_string())?;
    match spec.sample_format {
        hound::SampleFormat::Float => {
            let mut sb = rb.samples::<f32>();
            for x in ra.samples::<f32>() {
                let y = sb
                    .next()
                    .transpose()
                    .map_err(|e| e.to_string())?
                    .unwrap_or(0.0);
                writer
                    .write_sample(x.map_err(|e| e.to_string())? + y)
                    .map_err(|e| e.to_string())?;
            }
        }
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) - 1;
            let mut sb = rb.samples::<i32>();
            for x in ra.samples::<i32>() {
                let y = sb
                    .next()
                    .transpose()
                    .map_err(|e| e.to_string())?
                    .unwrap_or(0);
                let sum =
                    (i64::from(x.map_err(|e| e.to_string())?) + i64::from(y)).clamp(-max - 1, max);
                writer.write_sample(sum as i32).map_err(|e| e.to_string())?;
            }
        }
    }
    writer.finalize().map_err(|e| e.to_string())
}

fn open(path: &Path) -> Result<hound::WavReader<io::BufReader<fs::File>>, String> {
    hound::WavReader::open(path).map_err(|e| e.to_string())
}

/// What the chips show, measured on the written clip.
#[derive(Debug, Clone, PartialEq)]
pub struct Facts {
    pub rate: u32,
    pub bits: u16,
    pub float: bool,
    pub peak_db: f32,
    pub clipped: u64,
    pub duration_ms: u64,
}

pub fn measure(path: &Path) -> Result<Facts, String> {
    let mut reader = open(path)?;
    let spec = reader.spec();
    let (mut peak, mut clipped) = (0f32, 0u64);
    let mut note = |v: f32| {
        peak = peak.max(v.abs());
        clipped += u64::from(v.abs() >= 1.0);
    };
    match spec.sample_format {
        hound::SampleFormat::Float => {
            for s in reader.samples::<f32>() {
                note(s.map_err(|e| e.to_string())?);
            }
        }
        hound::SampleFormat::Int => {
            let full = (1i64 << (spec.bits_per_sample - 1)) as f32;
            for s in reader.samples::<i32>() {
                // The type's own limit counts as clipped, like 1.0 for float.
                let v = s.map_err(|e| e.to_string())? as f32;
                note(if v >= full - 1.0 || v <= -full {
                    1.0
                } else {
                    v / full
                });
            }
        }
    }
    Ok(Facts {
        rate: spec.sample_rate,
        bits: spec.bits_per_sample,
        float: spec.sample_format == hound::SampleFormat::Float,
        peak_db: if peak > 0.0 {
            20.0 * peak.log10()
        } else {
            f32::NEG_INFINITY
        },
        clipped,
        duration_ms: u64::from(reader.duration()) * 1000 / u64::from(spec.sample_rate.max(1)),
    })
}

/// Peak per bin, 0..1, of the range on one track (Both: the louder of the two), for the close-up.
/// Reads only the range, so a long meeting costs nothing extra.
pub fn levels(
    parts: &[Part],
    track: Track,
    from_ms: u64,
    to_ms: u64,
    bins: usize,
) -> Result<Vec<f32>, String> {
    let bins = bins.clamp(1, MAX_BINS);
    let length = to_ms.saturating_sub(from_ms).max(1);
    let mut out = vec![0f32; bins];
    let tracks: &[bool] = match track {
        Track::Me => &[true],
        Track::Them => &[false],
        Track::Both => &[true, false],
    };
    for &mic in tracks {
        let mut at = 0u64; // ms into the range where this span starts
        for (path, from, to) in spans(parts, mic, from_ms, to_ms) {
            if let Some(path) = path {
                let mut reader = open(&path)?;
                let spec = reader.spec();
                let rate = u64::from(spec.sample_rate.max(1));
                let channels = u64::from(spec.channels.max(1));
                let start = (from * rate / 1000).min(u64::from(reader.duration()));
                reader.seek(start as u32).map_err(|e| e.to_string())?;
                let frames = (to - from) * rate / 1000;
                let full = match spec.sample_format {
                    hound::SampleFormat::Float => 1.0,
                    hound::SampleFormat::Int => (1i64 << (spec.bits_per_sample - 1)) as f32,
                };
                let mut put = |i: u64, v: f32| {
                    let ms = at + i / channels * 1000 / rate;
                    let bin = ((ms * bins as u64) / length).min(bins as u64 - 1) as usize;
                    out[bin] = out[bin].max((v.abs() / full).min(1.0));
                };
                let take = (frames * channels) as usize;
                match spec.sample_format {
                    hound::SampleFormat::Float => {
                        for (i, s) in reader.samples::<f32>().take(take).enumerate() {
                            put(i as u64, s.map_err(|e| e.to_string())?);
                        }
                    }
                    hound::SampleFormat::Int => {
                        for (i, s) in reader.samples::<i32>().take(take).enumerate() {
                            put(i as u64, s.map_err(|e| e.to_string())? as f32);
                        }
                    }
                }
            }
            at += to - from;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("motetto-clip-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A float mono WAV whose frames are `value(frame)`.
    fn wav(path: &Path, rate: u32, frames: u32, value: impl Fn(u32) -> f32) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for f in 0..frames {
            w.write_sample(value(f)).unwrap();
        }
        w.finalize().unwrap();
    }

    fn samples(path: &Path) -> Vec<f32> {
        open(path)
            .unwrap()
            .samples::<f32>()
            .map(Result::unwrap)
            .collect()
    }

    /// Two 1 s parts at 1 kHz; the second has no system track.
    fn two_parts(dir: &Path) -> Vec<Part> {
        let p = |n: &str| dir.join(n);
        wav(&p("0.mic.wav"), 1000, 1000, |f| f as f32 / 1e4);
        wav(&p("0.system.wav"), 1000, 1000, |_| 0.25);
        wav(&p("1.mic.wav"), 1000, 1000, |f| 0.5 + f as f32 / 1e4);
        vec![
            Part {
                offset_ms: 0,
                duration_ms: 1000,
                mic: Some(p("0.mic.wav")),
                system: Some(p("0.system.wav")),
            },
            Part {
                offset_ms: 1000,
                duration_ms: 1000,
                mic: Some(p("1.mic.wav")),
                system: None,
            },
        ]
    }

    #[test]
    fn range_rules() {
        assert!(check_range(0, 499, 10_000).is_err());
        assert!(check_range(0, 500, 10_000).is_ok());
        assert!(check_range(0, MAX_MS + 1, MAX_MS * 2).is_err());
        assert!(check_range(9_000, 10_001, 10_000).is_err());
    }

    #[test]
    fn me_keeps_the_exact_samples_across_parts() {
        let dir = temp("me");
        let parts = two_parts(&dir);
        let dst = dir.join("out.wav");
        write(&parts, Track::Me, 900, 1100, &dst).unwrap();
        let got = samples(&dst);
        assert_eq!(got.len(), 200);
        assert_eq!(got[0], 900.0 / 1e4); // frame 900 of part 0
        assert_eq!(got[100], 0.5); // frame 0 of part 1
        assert_eq!(
            open(&dst).unwrap().spec(),
            open(&parts[0].mic.clone().unwrap()).unwrap().spec()
        );
    }

    #[test]
    fn a_missing_track_in_a_part_is_silence_of_the_same_length() {
        let dir = temp("them");
        let parts = two_parts(&dir);
        let dst = dir.join("out.wav");
        write(&parts, Track::Them, 900, 1100, &dst).unwrap();
        let got = samples(&dst);
        assert_eq!(got.len(), 200);
        assert!(got[..100].iter().all(|v| *v == 0.25));
        assert!(got[100..].iter().all(|v| *v == 0.0));
    }

    #[test]
    fn both_sums_the_tracks_and_leaves_no_temporary_files() {
        let dir = temp("both");
        let parts = two_parts(&dir);
        let dst = dir.join("out.wav");
        write(&parts, Track::Both, 0, 1000, &dst).unwrap();
        let got = samples(&dst);
        assert_eq!(got[10], 10.0 / 1e4 + 0.25);
        let mut names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, ["0.mic.wav", "0.system.wav", "1.mic.wav", "out.wav"]);
    }

    #[test]
    fn both_with_one_silent_side_is_the_other_and_nothing_is_an_error() {
        let dir = temp("oneside");
        let parts = two_parts(&dir);
        let (both, me) = (dir.join("both.wav"), dir.join("me.wav"));
        write(&parts, Track::Both, 1100, 1900, &both).unwrap();
        write(&parts, Track::Me, 1100, 1900, &me).unwrap();
        assert_eq!(samples(&both), samples(&me));
        assert_eq!(
            write(&parts, Track::Them, 1100, 1900, &dir.join("x.wav")),
            Err(NOTHING.to_string())
        );
    }

    #[test]
    fn both_raises_a_lower_rate_mic_to_the_call_rate() {
        let dir = temp("rates");
        // A 24 kHz headset mic beside 48 kHz call audio, 1 s each.
        wav(&dir.join("m.wav"), 24_000, 24_000, |_| 0.25);
        wav(&dir.join("s.wav"), 48_000, 48_000, |_| 0.5);
        let parts = [Part {
            offset_ms: 0,
            duration_ms: 1000,
            mic: Some(dir.join("m.wav")),
            system: Some(dir.join("s.wav")),
        }];
        let dst = dir.join("out.wav");
        write(&parts, Track::Both, 0, 1000, &dst).unwrap();
        let spec = open(&dst).unwrap().spec();
        assert_eq!(spec.sample_rate, 48_000);
        let mixed = samples(&dst);
        assert_eq!(mixed.len(), 48_000);
        // Away from the edges the sum is both levels; the filter only rings at the ends.
        assert!(mixed[1_000..47_000].iter().all(|v| (v - 0.75).abs() < 0.01));
        // The mic alone stays as recorded, and no temporary file is left behind.
        write(&parts, Track::Me, 0, 1000, &dir.join("me.wav")).unwrap();
        assert_eq!(
            open(&dir.join("me.wav")).unwrap().spec().sample_rate,
            24_000
        );
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 4);
    }

    #[test]
    fn measure_finds_peak_and_clipping() {
        let dir = temp("measure");
        let path = dir.join("m.wav");
        wav(&path, 1000, 1000, |f| match f {
            10 | 20 => 1.0,
            30 => -1.2,
            _ => 0.5,
        });
        let facts = measure(&path).unwrap();
        assert_eq!(
            (
                facts.rate,
                facts.bits,
                facts.float,
                facts.clipped,
                facts.duration_ms
            ),
            (1000, 32, true, 3, 1000)
        );
        assert!((facts.peak_db - 20.0 * 1.2f32.log10()).abs() < 1e-4);
    }

    #[test]
    fn levels_are_bounded_and_find_the_loud_moment() {
        let dir = temp("levels");
        let parts = two_parts(&dir);
        let bins = levels(&parts, Track::Me, 0, 2000, 20).unwrap();
        assert_eq!(bins.len(), 20);
        assert!(bins.iter().all(|v| (0.0..=1.0).contains(v)));
        assert!(bins[19] > bins[0]); // part 1 starts at 0.5, part 0 at 0
        assert_eq!(
            levels(&parts, Track::Them, 1000, 2000, 10).unwrap(),
            vec![0.0; 10]
        );
        assert_eq!(
            levels(&parts, Track::Me, 0, 2000, 10_000).unwrap().len(),
            MAX_BINS
        );
    }
}
