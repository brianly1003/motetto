//! Native fragmented MP4 encoding; serialized access to AVFoundation and locked BGRA pixels.

use crate::video::{self, Preset};
use objc2::{MainThreadMarker, Message, rc::Retained, runtime::AnyObject};
use objc2_av_foundation::*;
use objc2_foundation::{NSCopying, NSDictionary, NSNumber, NSString, NSURL};
use screencapturekit::cm::{CMSampleBuffer, CMSampleBufferExt, CMSampleBufferSCExt, SCFrameStatus};
use screencapturekit::prelude::{SCStreamOutputTrait, SCStreamOutputType};
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
pub(crate) enum Input {
    Video,
    Them,
    Me,
}

struct State {
    writer: Retained<AVAssetWriter>,
    inputs: [Retained<AVAssetWriterInput>; 3],
    origin: Option<f64>,
    finished: bool,
}

// SAFETY: every access to the retained writer and inputs is serialized by Writer's mutex.
unsafe impl Send for State {}

/// An MP4 writer with a video input and separate Them/You AAC inputs.
pub(crate) struct Writer(Mutex<State>);

fn any<T: Message>(value: Retained<T>) -> Retained<AnyObject> {
    // SAFETY: every Objective-C message object is also an AnyObject; ownership is transferred.
    unsafe { Retained::cast_unchecked(value) }
}

fn dict(
    values: Vec<(&NSString, Retained<AnyObject>)>,
) -> Retained<NSDictionary<NSString, AnyObject>> {
    let (keys, values): (Vec<_>, Vec<_>) = values.into_iter().unzip();
    NSDictionary::from_retained_objects(&keys, &values)
}

impl Writer {
    pub(crate) fn create(
        path: &Path,
        width: u32,
        height: u32,
        preset: Preset,
    ) -> Result<Self, String> {
        match path.symlink_metadata() {
            Ok(_) => return Err("a screen recording already exists at this path".into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("cannot check screen recording path: {e}")),
        }
        if width < 2 || height < 2 || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
            return Err("video dimensions must be positive even numbers".into());
        }
        let path = path.to_str().ok_or("video path isn't valid UTF-8")?;
        // SAFETY: typed settings contain the documented NSNumber/NSString values; all objects
        // remain retained while AVFoundation creates and owns its inputs, before writing starts.
        unsafe {
            let writer = AVAssetWriter::assetWriterWithURL_fileType_error(
                &NSURL::fileURLWithPath(&NSString::from_str(path)),
                AVFileTypeMPEG4.ok_or("MPEG-4 encoding isn't available")?,
            )
            .map_err(|e| e.to_string())?;
            writer.setMovieFragmentInterval(objc2_core_media::CMTime::new(2, 1));
            let compression = dict(vec![
                (
                    AVVideoAverageBitRateKey.ok_or("missing bitrate key")?,
                    any(NSNumber::new_i32(preset.video_bps())),
                ),
                (
                    AVVideoMaxKeyFrameIntervalDurationKey.ok_or("missing keyframe key")?,
                    any(NSNumber::new_i32(10)),
                ),
                (
                    AVVideoExpectedSourceFrameRateKey.ok_or("missing frame-rate key")?,
                    any(NSNumber::new_i32(preset.fps())),
                ),
                (
                    AVVideoAllowFrameReorderingKey.ok_or("missing frame-order key")?,
                    any(NSNumber::new_bool(false)),
                ),
                (
                    AVVideoProfileLevelKey.ok_or("missing profile key")?,
                    any(AVVideoProfileLevelH264HighAutoLevel
                        .ok_or("H.264 isn't available")?
                        .copy()),
                ),
            ]);
            let settings = dict(vec![
                (
                    AVVideoCodecKey.ok_or("missing codec key")?,
                    any(AVVideoCodecTypeH264.ok_or("H.264 isn't available")?.copy()),
                ),
                (
                    AVVideoWidthKey.ok_or("missing width key")?,
                    any(NSNumber::new_u32(width)),
                ),
                (
                    AVVideoHeightKey.ok_or("missing height key")?,
                    any(NSNumber::new_u32(height)),
                ),
                (
                    AVVideoCompressionPropertiesKey.ok_or("missing compression key")?,
                    any(compression),
                ),
            ]);
            let video = AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(
                AVMediaTypeVideo.ok_or("missing video type")?,
                Some(&settings),
            );
            let audio_settings = dict(vec![
                (
                    objc2_foundation::ns_string!("AVFormatIDKey"),
                    any(NSNumber::new_u32(0x6161_6320)),
                ),
                (
                    objc2_foundation::ns_string!("AVSampleRateKey"),
                    any(NSNumber::new_i32(48000)),
                ),
                (
                    objc2_foundation::ns_string!("AVNumberOfChannelsKey"),
                    any(NSNumber::new_i32(1)),
                ),
                (
                    objc2_foundation::ns_string!("AVEncoderBitRateKey"),
                    any(NSNumber::new_i32(64000)),
                ),
            ]);
            let audio_type = AVMediaTypeAudio.ok_or("missing audio type")?;
            let audio = || {
                AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(
                    audio_type,
                    Some(&audio_settings),
                )
            };
            let inputs = [video, audio(), audio()];
            for input in &inputs {
                input.setExpectsMediaDataInRealTime(true);
                if !writer.canAddInput(input) {
                    return Err("cannot add video/audio encoder input".into());
                }
                writer.addInput(input);
            }
            Ok(Self(Mutex::new(State {
                writer,
                inputs,
                origin: None,
                finished: false,
            })))
        }
    }

    /// Append on the shared audio host clock used by picture-part WAVs.
    pub(crate) fn append(
        &self,
        input: Input,
        buffer: &CMSampleBuffer,
        origin: Option<f64>,
    ) -> Result<Option<u64>, String> {
        let mut s = self.0.lock().map_err(|_| "video writer lock poisoned")?;
        if s.finished {
            return Ok(None);
        }
        // SAFETY: the SCK buffer owns this CMSampleBuffer for the duration of this call.
        let sample = unsafe { &*(buffer.as_ptr() as *const objc2_core_media::CMSampleBuffer) };
        // SAFETY: the sample is valid and all writer/input access is under the mutex.
        unsafe {
            let time = sample.presentation_time_stamp();
            if time.timescale <= 0 {
                return Err("invalid video/audio timestamp".into());
            }
            let seconds = time.value as f64 / f64::from(time.timescale);
            if s.origin.is_none() {
                if matches!(input, Input::Video) {
                    return Ok(None);
                }
                if !s.writer.startWriting() {
                    return Err(format!(
                        "cannot start video encoder: {:?}",
                        s.writer.error()
                    ));
                }
                let origin = origin.unwrap_or(seconds);
                s.writer
                    .startSessionAtSourceTime(objc2_core_media::CMTime::new(
                        (origin * 1_000_000.0).round() as i64,
                        1_000_000,
                    ));
                s.origin = Some(origin);
            }
            let elapsed = seconds - s.origin.unwrap_or(seconds);
            let retimed;
            let sample = if elapsed < 0.0 && matches!(input, Input::Video) {
                let origin = s.origin.unwrap_or(seconds);
                let timing = objc2_core_media::CMSampleTimingInfo {
                    duration: objc2_core_media::CMTime::new(1, 15),
                    presentationTimeStamp: objc2_core_media::CMTime::new(
                        (origin * 1_000_000.0).round() as i64,
                        1_000_000,
                    ),
                    decodeTimeStamp: objc2_core_media::CMTime::new(0, 0),
                };
                let mut copy = std::ptr::null_mut();
                // SAFETY: one valid timing entry and writable output pointer; the returned +1
                // sample buffer transfers ownership into the SCK wrapper, without changing its input.
                let status = objc2_core_media::CMSampleBuffer::create_copy_with_new_timing(
                    None,
                    sample,
                    1,
                    &timing,
                    std::ptr::NonNull::from(&mut copy),
                );
                if status != 0 {
                    return Err(format!("cannot time the first picture: {status}"));
                }
                retimed = CMSampleBuffer::from_raw(copy.cast()).ok_or("no retimed picture")?;
                &*(retimed.as_ptr() as *const objc2_core_media::CMSampleBuffer)
            } else if elapsed < 0.0 {
                return Ok(None);
            } else {
                sample
            };
            let index = match input {
                Input::Video => 0,
                Input::Them => 1,
                Input::Me => 2,
            };
            let target = &s.inputs[index];
            if !target.isReadyForMoreMediaData() {
                return Ok(None);
            }
            if !target.appendSampleBuffer(sample) {
                return Err(format!(
                    "cannot encode screen recording: {:?}",
                    s.writer.error()
                ));
            }
            Ok(Some((elapsed.max(0.0) * 1000.0) as u64))
        }
    }

    fn started(&self) -> bool {
        self.0.lock().is_ok_and(|s| s.origin.is_some())
    }

    pub(crate) fn finish(&self) -> Result<(), String> {
        let mut s = self.0.lock().map_err(|_| "video writer lock poisoned")?;
        if s.finished {
            return Ok(());
        }
        s.finished = true;
        // SAFETY: capture has stopped, and this mutex excludes any pending buffer append.
        unsafe {
            if s.origin.is_none() {
                s.writer.cancelWriting();
                return Err("no audio reached the video encoder".into());
            }
            if s.writer.status() != AVAssetWriterStatus::Writing {
                return Err(format!("video encoder failed: {:?}", s.writer.error()));
            }
            for input in &s.inputs {
                input.markAsFinished();
            }
            let done = Arc::new(AtomicBool::new(false));
            let completion = done.clone();
            s.writer
                .finishWritingWithCompletionHandler(&block2::RcBlock::new(move || {
                    completion.store(true, Ordering::Release)
                }));
            let start = Instant::now();
            while !done.load(Ordering::Acquire) {
                if start.elapsed() >= Duration::from_secs(5) {
                    s.writer.cancelWriting();
                    return Err("video encoder did not finish within five seconds".into());
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            if s.writer.status() != AVAssetWriterStatus::Completed {
                return Err(format!(
                    "cannot finish screen recording: {:?}",
                    s.writer.error()
                ));
            }
        }
        Ok(())
    }
}

/// Read BGRA while CoreVideo guarantees the locked pixel storage remains valid.
pub(crate) fn grey(buffer: &CMSampleBuffer) -> Option<[u8; video::GREY]> {
    let pixel = buffer.pixel_buffer()?;
    let lock = pixel.lock_read_only().ok()?;
    let address = lock.base_address();
    let size = lock.bytes_per_row().checked_mul(lock.height())?;
    if address.is_null() || size > isize::MAX as usize {
        return None;
    }
    // SAFETY: the non-planar BGRA buffer is read-locked; its storage spans stride * height bytes.
    let bytes = unsafe { std::slice::from_raw_parts(address, size) };
    Some(video::shrink(
        bytes,
        lock.width(),
        lock.height(),
        lock.bytes_per_row(),
    ))
}

/// Display identifiers, local names and backing scales, queried only on AppKit's main thread.
pub fn displays(mtm: MainThreadMarker) -> Vec<(u32, String, f64)> {
    let screens = objc2_app_kit::NSScreen::screens(mtm);
    (0..screens.count())
        .map(|i| screens.objectAtIndex(i))
        .filter_map(|screen| {
            let description = screen.deviceDescription();
            let number =
                description.objectForKey(objc2_foundation::ns_string!("NSScreenNumber"))?;
            let number = number.downcast_ref::<NSNumber>()?;
            Some((
                number.as_u32(),
                screen.localizedName().to_string(),
                screen.backingScaleFactor(),
            ))
        })
        .collect()
}

/// Shared encoder state for audio and picture callbacks; its errors never become WAV errors.
pub(crate) struct Encoding {
    writer: Writer,
    clock: Mutex<Option<f64>>,
    pending: Mutex<Option<CMSampleBuffer>>,
    failed: AtomicBool,
    error: Mutex<Option<String>>,
    moments: Mutex<(video::Moments, Vec<u64>, Option<u64>)>,
}

impl Encoding {
    pub(crate) fn new(writer: Writer) -> Self {
        Self {
            writer,
            clock: Mutex::new(None),
            pending: Mutex::new(None),
            failed: AtomicBool::new(false),
            error: Mutex::new(None),
            moments: Mutex::new((video::Moments::new(), vec![], None)),
        }
    }

    pub(crate) fn fail(&self, reason: String) {
        if let Ok(mut error) = self.error.lock() {
            error.get_or_insert(reason);
        }
        self.failed.store(true, Ordering::Release);
    }

    pub(crate) fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }

    /// Audio and picture share a host-time origin; this survives encoder failure.
    pub(crate) fn audio_time(&self, buffer: &CMSampleBuffer) -> Option<f64> {
        let seconds = buffer.presentation_timestamp().as_seconds()?;
        let mut clock = self.clock.lock().ok()?;
        Some(seconds - *clock.get_or_insert(seconds))
    }

    pub(crate) fn push(&self, input: Input, buffer: &CMSampleBuffer) {
        if self.failed() {
            return;
        }
        let Ok(mut pending) = self.pending.lock() else {
            self.fail("picture queue lock poisoned".into());
            return;
        };
        let origin = self.clock.lock().ok().and_then(|clock| *clock);
        match self.writer.append(input, buffer, origin) {
            Ok(None) if matches!(input, Input::Video) && !self.writer.started() => {
                *pending = Some(buffer.clone());
            }
            Err(error) => self.fail(error),
            Ok(Some(ms)) if matches!(input, Input::Video) => {
                let Ok(mut moments) = self.moments.lock() else {
                    self.fail("screen-change lock poisoned".into());
                    return;
                };
                if moments.2.is_some_and(|last| ms.saturating_sub(last) < 1000) {
                    return;
                }
                moments.2 = Some(ms);
                if let Some(grey) = grey(buffer) {
                    let time = if moments.1.is_empty() { 0 } else { ms };
                    if moments.0.offer(time, &grey) {
                        moments.1.push(time);
                    }
                }
            }
            _ => {}
        }
        let first = if !matches!(input, Input::Video) && self.writer.started() {
            pending.take()
        } else {
            None
        };
        drop(pending);
        if let Some(first) = first {
            self.push(Input::Video, &first);
        }
    }

    pub(crate) fn snapshot(&self) -> (Vec<u64>, Option<String>) {
        let moments = self.moments.lock().map(|m| m.1.clone()).unwrap_or_default();
        let error = self
            .error
            .lock()
            .map(|e| e.clone())
            .unwrap_or_else(|_| Some("video writer lock poisoned".into()));
        (moments, error)
    }

    pub(crate) fn finish(&self) -> (Vec<u64>, Option<String>) {
        if let Err(error) = self.writer.finish() {
            self.fail(error);
        }
        let moments = self.moments.lock().map(|m| m.1.clone()).unwrap_or_default();
        let error = self
            .error
            .lock()
            .map(|e| e.clone())
            .unwrap_or_else(|_| Some("video writer lock poisoned".into()));
        (moments, error)
    }
}

pub(crate) struct Picture(pub Arc<Encoding>);
impl SCStreamOutputTrait for Picture {
    fn did_output_sample_buffer(&self, buffer: CMSampleBuffer, _: SCStreamOutputType) {
        if buffer.frame_status() == Some(SCFrameStatus::Complete) {
            self.0.push(Input::Video, &buffer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoder_error_keeps_the_first_failure_through_finalization() {
        let path = std::env::temp_dir().join(format!(
            "motetto-encoder-{}-{}.mp4",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let encoding = Encoding::new(Writer::create(&path, 320, 240, Preset::Smaller).unwrap());
        encoding.fail("disk full".into());
        encoding.fail("second failure".into());
        assert!(encoding.failed());
        assert_eq!(encoding.finish().1.as_deref(), Some("disk full"));
        if path.exists() {
            std::fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn an_existing_video_is_never_overwritten() {
        let path =
            std::env::temp_dir().join(format!("motetto-existing-video-{}.mp4", std::process::id()));
        std::fs::write(&path, b"existing video").unwrap();
        assert!(Writer::create(&path, 320, 240, Preset::Sharp).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"existing video");
        std::fs::remove_file(path).unwrap();
    }
}
