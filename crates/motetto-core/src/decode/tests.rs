use super::*;

fn directory() -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "motetto-decode-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir(&path).unwrap();
    path
}

#[test]
fn only_unsupported_formats_fall_back_to_ffmpeg() {
    use symphonia::core::errors::Error as Media;
    let boxed = |e: Media| -> Box<dyn Error> { Box::new(e) };
    assert!(unsupported(
        boxed(Media::Unsupported("core (codec): opus")).as_ref()
    ));
    assert!(!unsupported(
        boxed(Media::DecodeError("bad frame")).as_ref()
    ));
    let io: Box<dyn Error> = Box::new(std::io::Error::other("disk"));
    assert!(!unsupported(io.as_ref()));
    let text: Box<dyn Error> = "no audio track".into();
    assert!(!unsupported(text.as_ref()));
}

#[test]
fn rates_preserve_duration_impulse_position_and_tail() {
    let dir = directory();
    for rate in [8000, 16000, 44100, 48000] {
        let input = dir.join("input.wav");
        let output = dir.join("output.wav");
        let mut writer = hound::WavWriter::create(
            &input,
            hound::WavSpec {
                channels: 2,
                sample_rate: rate,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .unwrap();
        let length = rate + 137;
        for i in 0..length {
            let value = if i == rate / 2 || i == length - 10 {
                1.0_f32
            } else {
                0.0
            };
            writer.write_sample(value).unwrap();
            writer.write_sample(value).unwrap();
        }
        writer.finalize().unwrap();
        to_wav(&input, &output).unwrap();
        let mut reader = hound::WavReader::open(&output).unwrap();
        assert_eq!(reader.spec().sample_rate, 16000);
        assert_eq!(reader.spec().channels, 1);
        let samples: Vec<f32> = reader.samples().collect::<Result<_, _>>().unwrap();
        assert_eq!(
            samples.len(),
            (u64::from(length) * 16000 / u64::from(rate)) as usize
        );
        let peak = samples[..12000]
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .unwrap()
            .0;
        assert!(peak.abs_diff(8000) <= 1, "rate {rate}: peak {peak}");
        assert!(
            samples[samples.len() - 30..].iter().any(|s| s.abs() > 0.1),
            "lost tail at {rate}"
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn mixes_channels_and_rejects_invalid_or_same_file() {
    let dir = directory();
    let input = dir.join("in.wav");
    let output = dir.join("out.wav");
    let mut writer = hound::WavWriter::create(
        &input,
        hound::WavSpec {
            channels: 2,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for _ in 0..100 {
        writer.write_sample(16384_i16).unwrap();
        writer.write_sample(0_i16).unwrap();
    }
    writer.finalize().unwrap();
    to_wav(&input, &output).unwrap();
    let mut reader = hound::WavReader::open(&output).unwrap();
    assert!(
        reader
            .samples::<f32>()
            .all(|s| (s.unwrap() - 0.25).abs() < 0.001)
    );
    let original = std::fs::read(&input).unwrap();
    assert!(to_wav(&input, &input).is_err());
    assert_eq!(std::fs::read(&input).unwrap(), original);
    std::fs::write(&input, b"broken media").unwrap();
    assert!(to_wav(&input, &output).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
#[ignore = "requires generated media fixtures; run scripts/m2-e2e.sh"]
fn imported_formats_decode_without_ffmpeg() {
    let dir = std::path::PathBuf::from(std::env::var("MOTETTO_TEST_MEDIA").unwrap());
    for ext in ["wav", "aiff", "mp3", "m4a", "mp4", "flac"] {
        to_wav(
            &dir.join(format!("speech.{ext}")),
            &dir.join(format!("{ext}.16k.wav")),
        )
        .unwrap();
        let mut reader = hound::WavReader::open(dir.join(format!("{ext}.16k.wav"))).unwrap();
        assert!(reader.duration() > 16000);
        assert!(reader.samples::<f32>().any(|s| s.unwrap().abs() > 0.01));
    }
}

#[test]
fn seconds_come_from_the_header_without_decoding() {
    let dir = std::env::temp_dir().join(format!("motetto-seconds-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tone.wav");
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 8000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut wav = hound::WavWriter::create(&path, spec).unwrap();
    for _ in 0..8000 * 2 * 2 {
        wav.write_sample(0i16).unwrap();
    }
    wav.finalize().unwrap();
    assert_eq!(super::seconds(&path), Some(2.0));
    std::fs::write(&path, b"broken media").unwrap();
    assert_eq!(super::seconds(&path), None);
    std::fs::remove_dir_all(dir).unwrap();
}
