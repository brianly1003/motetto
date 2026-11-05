//! Joining and cutting MP4 parts without re-encoding: an AVFoundation composition exported with
//! the passthrough preset.

use objc2::{rc::Retained, runtime::AnyObject};
use objc2_av_foundation::*;
use objc2_core_media::{CMTime, CMTimeRange};
use objc2_foundation::{NSDictionary, NSError, NSNumber, NSString, NSURL};
use std::{
    path::{Path, PathBuf},
    sync::mpsc,
    time::Duration,
};

/// Longest wait for one AVFoundation step; a passthrough export copies samples, so even an
/// hour-long recording is well inside it.
const TIMEOUT: Duration = Duration::from_secs(120);

fn url(path: &Path) -> Result<Retained<NSURL>, String> {
    let path = path.to_str().ok_or("video path isn't valid UTF-8")?;
    Ok(NSURL::fileURLWithPath(&NSString::from_str(path)))
}

/// An asset with exact durations and timing, so a cut lands on the asked time.
fn asset(path: &Path) -> Result<Retained<AVURLAsset>, String> {
    let url = url(path)?;
    // SAFETY: the options dictionary holds the documented NSNumber value for its key, and both
    // stay retained for the call.
    unsafe {
        let precise = NSNumber::numberWithBool(true);
        let options = NSDictionary::<NSString, AnyObject>::from_retained_objects(
            &[AVURLAssetPreferPreciseDurationAndTimingKey],
            &[Retained::into_super(Retained::into_super(precise)).into()],
        );
        Ok(AVURLAsset::URLAssetWithURL_options(&url, Some(&options)))
    }
}

fn ms(value: u64) -> Result<CMTime, String> {
    let value = i64::try_from(value).map_err(|_| "time is out of range")?;
    // SAFETY: CMTimeMake only builds a value; a millisecond timescale is valid.
    Ok(unsafe { CMTime::new(value, 1000) })
}

fn described(error: *mut NSError) -> Option<String> {
    // SAFETY: AVFoundation passes nil or an NSError it keeps alive while the handler runs.
    unsafe { error.as_ref() }.map(|e| e.localizedDescription().to_string())
}

/// A video's length in ms.
pub fn duration_ms(path: &Path) -> Result<u64, String> {
    let asset = asset(path)?;
    // SAFETY: reading the duration of a retained asset; AVFoundation loads it synchronously.
    let seconds = unsafe { asset.duration().seconds() };
    if !seconds.is_finite() || seconds < 0.0 {
        return Err(format!("can't read the length of {}", path.display()));
    }
    Ok((seconds * 1000.0).round() as u64)
}

/// Joins `from_ms..to_ms` of each part, in order, into one MP4 at `dst` without re-encoding.
/// `dst` must not exist; a failed export leaves nothing there.
pub fn export(parts: &[(PathBuf, u64, u64)], dst: &Path) -> Result<(), String> {
    if parts.is_empty() {
        return Err("nothing to export".into());
    }
    if dst.symlink_metadata().is_ok() {
        return Err("a video already exists where the trim goes".into());
    }
    // SAFETY: the composition, assets and session stay retained until the export has finished
    // or been cancelled; the handlers only send on a channel, which is safe from any queue.
    let result = unsafe {
        let composition = AVMutableComposition::composition();
        let mut at = ms(0)?;
        for (path, from, to) in parts {
            let asset = asset(path)?;
            let range = CMTimeRange {
                start: ms(*from)?,
                duration: ms(to.saturating_sub(*from))?,
            };
            let (sent, inserted) = mpsc::channel();
            composition.insertTimeRange_ofAsset_atTime_completionHandler(
                range,
                &asset,
                at,
                &block2::RcBlock::new(move |error: *mut NSError| {
                    let _ = sent.send(described(error));
                }),
            );
            match inserted.recv_timeout(TIMEOUT) {
                Ok(None) => {}
                Ok(Some(error)) => return Err(format!("can't read {}: {error}", path.display())),
                Err(_) => return Err("reading the video took too long".into()),
            }
            at = at.add(range.duration);
        }
        let session = AVAssetExportSession::exportSessionWithAsset_presetName(
            &composition,
            AVAssetExportPresetPassthrough,
        )
        .ok_or("the video can't be exported without re-encoding")?;
        let output = url(dst)?;
        session.setOutputURL(Some(&output));
        session.setOutputFileType(AVFileTypeMPEG4);
        let (sent, done) = mpsc::channel();
        session.exportAsynchronouslyWithCompletionHandler(&block2::RcBlock::new(move || {
            let _ = sent.send(());
        }));
        if done.recv_timeout(TIMEOUT).is_err() {
            session.cancelExport();
            Err("trimming took over two minutes and was stopped".to_string())
        } else if session.status() != AVAssetExportSessionStatus::Completed {
            Err(format!(
                "can't save the trimmed video: {}",
                session
                    .error()
                    .map(|e| e.localizedDescription().to_string())
                    .unwrap_or_else(|| format!("{:?}", session.status()))
            ))
        } else {
            Ok(())
        }
    };
    if result.is_err() {
        let _ = std::fs::remove_file(dst);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::{Tracks, record_with_levels};
    use crate::events::Stop;
    use crate::video::{self, ScreenCapture, ScreenTarget};
    use screencapturekit::prelude::SCShareableContent;
    use std::sync::atomic::Ordering;

    #[test]
    fn export_refuses_an_existing_file_and_an_empty_list() {
        let dir = std::env::temp_dir().join(format!("motetto-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dst = dir.join("out.mp4");
        std::fs::write(&dst, b"keep").unwrap();
        assert!(export(&[(dir.join("in.mp4"), 0, 1000)], &dst).is_err());
        assert_eq!(std::fs::read(&dst).unwrap(), b"keep");
        assert!(export(&[], &dir.join("new.mp4")).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    #[ignore = "needs Screen Recording permission; plays a quiet tone and records it with an area for six seconds"]
    fn trim_export_plays() {
        let display = SCShareableContent::get()
            .unwrap()
            .displays()
            .into_iter()
            .next()
            .unwrap();
        let dir = std::env::temp_dir().join(format!("motetto-trim-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        let base = dir.join("part-0000").to_string_lossy().into_owned();
        let screen = ScreenCapture {
            target: ScreenTarget::Area {
                display: display.display_id(),
                rect: video::AreaRect {
                    x: 0.0,
                    y: 0.0,
                    width: 400.0,
                    height: 300.0,
                },
            },
            preset: video::Preset::Smaller,
            cursor: true,
            clicks: false,
            path: format!("{base}.screen.mp4"),
            scale: 1.0,
            progress: None,
        };
        // A quiet tone gives the part a steady clock: a still area alone yields one frame, and
        // app sounds deliver nothing while the Mac is silent.
        let tone = dir.join("tone.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&tone, spec).unwrap();
        for i in 0..16_000 * 8 {
            let wave = (i as f32 * 440.0 * std::f32::consts::TAU / 16_000.0).sin();
            writer.write_sample((wave * 600.0) as i16).unwrap();
        }
        writer.finalize().unwrap();
        let mut player = std::process::Command::new("/usr/bin/afplay")
            .arg(&tone)
            .spawn()
            .unwrap();
        let stop = Stop::default();
        let timer = stop.clone();
        // Starting the streams can take seconds; the six count from the first audio written.
        let heard = PathBuf::from(format!("{base}.system.wav"));
        let thread = std::thread::spawn(move || {
            let waiting = std::time::Instant::now();
            while !std::fs::metadata(&heard).is_ok_and(|m| m.len() > 4096)
                && waiting.elapsed() < Duration::from_secs(60)
            {
                std::thread::sleep(Duration::from_millis(50));
            }
            std::thread::sleep(Duration::from_secs(6));
            timer.store(true, Ordering::Relaxed);
        });
        let sounds = Tracks {
            mic: false,
            system: true,
        };
        let result = record_with_levels(&base, stop, None, None, Some(&screen), sounds);
        thread.join().unwrap();
        let _ = player.kill();
        let _ = player.wait();
        assert_eq!(result.unwrap().video.unwrap().error, None);
        let source = PathBuf::from(&screen.path);
        assert!(duration_ms(&source).unwrap() >= 5000);
        let trimmed = dir.join("trimmed.mp4");
        export(&[(source, 2000, 4000)], &trimmed).unwrap();
        let kept = duration_ms(&trimmed).unwrap();
        assert!((1900..=2100).contains(&kept), "{kept} ms");
        if std::env::var_os("MOTETTO_KEEP_TRIM").is_none() {
            std::fs::remove_dir_all(dir).unwrap();
        } else {
            eprintln!("kept {}", trimmed.display());
        }
    }
}
