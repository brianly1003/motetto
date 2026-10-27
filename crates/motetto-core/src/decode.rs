//! Streaming audio decode and delay-compensated resampling to 16 kHz mono WAV.

use rubato::{FftFixedIn, Resampler};
use std::{
    error::Error,
    ffi::OsString,
    fs::File,
    io::{BufWriter, ErrorKind},
    path::Path,
    process::Stdio,
};
use symphonia::core::{
    formats::{TrackType, probe::Hint},
    io::MediaSourceStream,
};

type Writer = hound::WavWriter<BufWriter<File>>;

/// Decode one audio track without loading the entire recording into memory.
pub fn to_wav(source: &Path, destination: &Path) -> Result<(), Box<dyn Error>> {
    let result = match convert(source, destination) {
        Err(e) if unsupported(e.as_ref()) => ffmpeg(source, destination, e.as_ref()),
        other => other,
    };
    result.map_err(|e| {
        format!(
            "cannot decode {}: {e}; check the file's audio format and output folder",
            source.display()
        )
        .into()
    })
}

fn convert(source: &Path, destination: &Path) -> Result<(), Box<dyn Error>> {
    if source == destination
        || (destination.exists() && source.canonicalize()? == destination.canonicalize()?)
    {
        return Err("input and output must be different files".into());
    }
    let stream = MediaSourceStream::new(Box::new(File::open(source)?), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = source.extension().and_then(|s| s.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe().probe(
        &hint,
        stream,
        Default::default(),
        Default::default(),
    )?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or("no audio track")?;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or("missing audio parameters")?;
    let mut decoder =
        symphonia::default::get_codecs().make_audio_decoder(params, &Default::default())?;
    let track_id = track.id;
    // Frames decoded out of the track's length, for the steps page; unknown for some formats.
    let (total, mut decoded) = (track.num_frames, 0u64);
    let mut writer = hound::WavWriter::create(
        destination,
        hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )?;
    let mut resample: Option<Mono> = None;
    let mut samples = Vec::<f32>::new();
    while let Some(packet) = format.next_packet()? {
        crate::tools::check()?;
        if packet.track_id != track_id {
            continue;
        }
        // Do not silently drop damaged packets: that shifts all later timestamps.
        let audio = decoder.decode(&packet)?;
        let rate = audio.spec().rate();
        let channels = audio.spec().channels().count();
        if channels == 0 || rate == 0 {
            return Err("invalid audio sample rate or channels".into());
        }
        if resample.is_none() {
            resample = Some(Mono::new(rate)?);
        }
        let mono = resample.as_mut().ok_or("missing resampler")?;
        if mono.rate != rate {
            return Err("sample rate changed within the file".into());
        }
        decoded += audio.frames() as u64;
        if let Some(total) = total {
            crate::progress::step(decoded, total);
        }
        samples.resize(audio.samples_interleaved(), 0.0);
        audio.copy_to_slice_interleaved(&mut samples);
        for frame in samples.chunks_exact(channels) {
            if frame.iter().any(|v| !v.is_finite()) {
                return Err("non-finite audio sample".into());
            }
            mono.push(
                (frame.iter().map(|v| f64::from(*v)).sum::<f64>() / channels as f64) as f32,
                &mut writer,
            )?;
        }
    }
    let mut mono = resample.ok_or("file contains no audio samples")?;
    mono.finish(&mut writer)?;
    writer.finalize()?;
    Ok(())
}

/// Symphonia has no Opus decoder and no WebM/Matroska reader. Only those "unsupported" cases fall
/// back; damaged files still fail here instead of being retried somewhere else.
fn unsupported(error: &(dyn Error + 'static)) -> bool {
    matches!(
        error.downcast_ref::<symphonia::core::errors::Error>(),
        Some(symphonia::core::errors::Error::Unsupported(_))
    )
}

// Opus/WebM imports use the user's own ffmpeg; the desktop app bundles no ffmpeg.
// If those imports become common, add an Opus decoder instead (docs/decisions/audio-decoding.md).
fn ffmpeg(source: &Path, destination: &Path, why: &dyn Error) -> Result<(), Box<dyn Error>> {
    // `file:` and the protocol whitelist keep ffmpeg on local files: a playlist or a name that
    // looks like a URL can't make it fetch anything. Mono f32 can't clip; ffmpeg's stereo
    // downmix is 3 dB louder than ours, which whisper doesn't mind.
    let local = |path: &Path| {
        let mut arg = OsString::from("file:");
        arg.push(path.as_os_str());
        arg
    };
    let status = crate::tools::status(
        crate::tools::command("ffmpeg")
            .args(["-nostdin", "-hide_banner", "-loglevel", "error"])
            .args(["-protocol_whitelist", "file", "-y", "-i"])
            .arg(local(source))
            .args([
                "-vn",
                "-ac",
                "1",
                "-ar",
                "16000",
                "-c:a",
                "pcm_f32le",
                "-f",
                "wav",
            ])
            .arg(local(destination))
            .stdin(Stdio::null()),
    );
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("{why}; ffmpeg could not read it either ({status})").into()),
        Err(e) if e.kind() == ErrorKind::NotFound => Err(format!(
            "{why}; this format (for example Opus or WebM) needs ffmpeg: install it \
             (brew install ffmpeg) or convert the file to WAV, M4A or MP3"
        )
        .into()),
        Err(e) => Err(e.into()),
    }
}

struct Mono {
    rate: u32,
    filter: Option<FftFixedIn<f32>>,
    pending: Vec<f32>,
    skip: usize,
    input: u64,
    written: u64,
}

impl Mono {
    fn new(rate: u32) -> Result<Self, Box<dyn Error>> {
        let filter = if rate == 16_000 {
            None
        } else {
            Some(FftFixedIn::new(rate as usize, 16_000, 1024, 1, 1)?)
        };
        let skip = filter.as_ref().map_or(0, |f| f.output_delay());
        Ok(Self {
            rate,
            filter,
            pending: Vec::with_capacity(1024),
            skip,
            input: 0,
            written: 0,
        })
    }

    fn push(&mut self, sample: f32, writer: &mut Writer) -> Result<(), Box<dyn Error>> {
        self.input += 1;
        if self.filter.is_none() {
            writer.write_sample(sample)?;
            self.written += 1;
        } else {
            self.pending.push(sample);
            if self.pending.len() == 1024 {
                self.block(writer)?;
            }
        }
        Ok(())
    }

    fn block(&mut self, writer: &mut Writer) -> Result<(), Box<dyn Error>> {
        self.pending.resize(1024, 0.0);
        if let Some(filter) = &mut self.filter {
            let output = filter.process(&[&self.pending], None)?;
            let target = self.input * 16_000 / u64::from(self.rate);
            for &sample in &output[0] {
                if self.skip > 0 {
                    self.skip -= 1;
                } else if self.written < target {
                    writer.write_sample(sample)?;
                    self.written += 1;
                }
            }
        }
        self.pending.clear();
        Ok(())
    }

    fn finish(&mut self, writer: &mut Writer) -> Result<(), Box<dyn Error>> {
        if self.input == 0 {
            return Err("file contains no audio samples".into());
        }
        let target = self.input * 16_000 / u64::from(self.rate);
        while self.written < target {
            self.block(writer)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
