//! Following the voice with Apple's on-device recogniser (SPEC 14.5, macOS): the mic's own
//! buffers stream into `SFSpeechRecognizer`, and its partial results reach the strip about half
//! a second after the words, against ~2.4 s for Whisper windows. On-device only: a locale the Mac
//! cannot recognise locally is never used, so audio never leaves it. One worker thread owns the
//! Speech objects; the result handler runs on its own queue and only sends text back.
use crate::follow::{Heard, restart_due, tail_words};
use block2::RcBlock;
use motetto_core::events::Stop;
use objc2::{AllocAnyThread, rc::Retained};
use objc2_avf_audio::{AVAudioFormat, AVAudioPCMBuffer};
use objc2_foundation::{NSArray, NSError, NSLocale, NSOperationQueue, NSString};
use objc2_speech::{
    SFSpeechAudioBufferRecognitionRequest, SFSpeechRecognitionResult, SFSpeechRecognitionTask,
    SFSpeechRecognitionTaskHint, SFSpeechRecognizer, SFSpeechRecognizerAuthorizationStatus,
};
use std::{
    sync::{atomic::Ordering, mpsc},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Allowed,
    Denied,
    Unknown,
}

/// What macOS says now; never assumed from a request having returned.
pub fn access() -> Access {
    // SAFETY: a class method with no arguments, callable from any thread.
    let status = unsafe { SFSpeechRecognizer::authorizationStatus() };
    if status == SFSpeechRecognizerAuthorizationStatus::Authorized {
        Access::Allowed
    } else if status == SFSpeechRecognizerAuthorizationStatus::NotDetermined {
        Access::Unknown
    } else {
        Access::Denied
    }
}

/// Asks once (the system prompt, with Info.plist's reason), then reads the answer back.
pub fn ask() -> Access {
    if access() != Access::Unknown {
        return access();
    }
    let (tx, rx) = mpsc::channel();
    let handler = RcBlock::new(move |_: SFSpeechRecognizerAuthorizationStatus| {
        let _ = tx.send(());
    });
    // SAFETY: the block only sends on a channel, which is fine from the queue macOS calls it on;
    // Info.plist carries NSSpeechRecognitionUsageDescription, without which this call aborts.
    unsafe { SFSpeechRecognizer::requestAuthorization(&handler) };
    let _ = rx.recv_timeout(Duration::from_secs(60));
    access()
}

fn recognizer(locale: &str) -> Option<Retained<SFSpeechRecognizer>> {
    let locale = NSLocale::initWithLocaleIdentifier(NSLocale::alloc(), &NSString::from_str(locale));
    // SAFETY: a plain initializer; nil (an unsupported locale) comes back as None.
    unsafe { SFSpeechRecognizer::initWithLocale(SFSpeechRecognizer::alloc(), &locale) }
}

/// Locales this Mac recognises without a network ("en-US", "vi-VN").
pub fn on_device_locales() -> Vec<String> {
    // SAFETY: a class method returning an immutable set.
    let all = unsafe { SFSpeechRecognizer::supportedLocales() };
    let mut out: Vec<String> = all
        .iter()
        .map(|l| l.localeIdentifier().to_string().replace('_', "-"))
        // SAFETY: reading a property of a recognizer created here.
        .filter(|id| recognizer(id).is_some_and(|r| unsafe { r.supportsOnDeviceRecognition() }))
        .collect();
    out.sort();
    out
}

/// One result: what was heard so far in this utterance, and whether the task is done. On-device
/// partial results carry no word times (Apple leaves them 0), so none are read.
struct Partial {
    text: String,
    done: bool,
    failed: bool,
}

/// Follows until `stop`: buffers from `audio`, results to `heard`. `hint` gives the next script
/// words when a task starts. Returns Err when the recogniser cannot run (then Whisper follows).
pub fn follow(
    audio: &mpsc::Receiver<(Vec<f32>, u32)>,
    locale: &str,
    hint: &dyn Fn() -> String,
    stop: &Stop,
    heard: &mut dyn FnMut(Heard, &str, bool),
) -> Result<(), String> {
    let recognizer = recognizer(locale).ok_or("This language isn't available")?;
    // SAFETY: reading properties of the recognizer created above.
    if !unsafe { recognizer.supportsOnDeviceRecognition() } {
        return Err("This language isn't on this Mac".into());
    }
    // Results on their own queue: the main thread runs the app.
    // SAFETY: setting the queue before any task starts, as the API asks.
    unsafe { recognizer.setQueue(&NSOperationQueue::new()) };
    let mut fed_ms = 0.0_f64;
    let mut failures = 0_u32;
    while !stop.load(Ordering::SeqCst) {
        let (tx, results) = mpsc::channel::<Partial>();
        let handler = RcBlock::new(
            move |result: *mut SFSpeechRecognitionResult, error: *mut NSError| {
                // SAFETY: macOS passes a valid result or nil, valid for the call.
                let partial = unsafe { result.as_ref() }.map(|r| {
                    let best = unsafe { r.bestTranscription() };
                    Partial {
                        text: unsafe { best.formattedString() }.to_string(),
                        done: unsafe { r.isFinal() },
                        failed: false,
                    }
                });
                let _ = tx.send(partial.unwrap_or(Partial {
                    text: String::new(),
                    done: true,
                    failed: !error.is_null(),
                }));
            },
        );
        // SAFETY: a new request configured before use; every setter takes plain values.
        let request = unsafe { SFSpeechAudioBufferRecognitionRequest::new() };
        unsafe {
            request.setShouldReportPartialResults(true);
            request.setRequiresOnDeviceRecognition(true);
            request.setTaskHint(SFSpeechRecognitionTaskHint::Dictation);
            request.setAddsPunctuation(false);
            let words: Vec<Retained<NSString>> = hint()
                .split_whitespace()
                .take(30)
                .map(NSString::from_str)
                .collect();
            request.setContextualStrings(&NSArray::from_retained_slice(&words));
        }
        // SAFETY: the request and handler live until the task is finished below.
        let task: Retained<SFSpeechRecognitionTask> =
            unsafe { recognizer.recognitionTaskWithRequest_resultHandler(&request, &handler) };
        let started = Instant::now();
        let (mut silent_ms, mut appended, mut ended) = (0.0, Instant::now(), false);
        let mut last_audio = Instant::now();
        let mut last_text = String::new();
        while !stop.load(Ordering::SeqCst) && !ended {
            if let Ok((samples, rate)) = audio.recv_timeout(Duration::from_millis(50)) {
                append(&request, &samples, rate)?;
                appended = Instant::now();
                last_audio = appended;
                let ms = samples.len() as f64 * 1000.0 / f64::from(rate.max(1));
                fed_ms += ms;
                let loud = samples.iter().any(|s| s.abs() > 0.02);
                silent_ms = if loud { 0.0 } else { silent_ms + ms };
            }
            while let Ok(partial) = results.try_recv() {
                ended |= partial.done;
                take(
                    partial,
                    &mut last_text,
                    &mut failures,
                    fed_ms,
                    appended,
                    heard,
                );
            }
            if restart_due(started.elapsed().as_millis() as u64, silent_ms as u64) {
                ended = true;
            }
            if crate::follow::starved(last_audio.elapsed().as_millis() as u64) {
                // SAFETY: cancelling the task this loop owns.
                unsafe { task.cancel() };
                return Err("No microphone audio reached the recogniser".into());
            }
        }
        // SAFETY: ending the audio and finishing the task this loop owns.
        unsafe {
            request.endAudio();
            task.finish();
        }
        // Three failed tasks in a row with nothing heard: the recogniser is not working here.
        if failures >= 3 {
            return Err("Apple recognition keeps failing".into());
        }
    }
    Ok(())
}

/// A result to the strip (when its words changed) and to `heard`'s timing view.
fn take(
    partial: Partial,
    last_text: &mut String,
    failures: &mut u32,
    fed_ms: f64,
    appended: Instant,
    heard: &mut dyn FnMut(Heard, &str, bool),
) {
    if partial.failed {
        *failures += 1;
    }
    if partial.text.is_empty() || (partial.text == *last_text && !partial.done) {
        return;
    }
    *failures = 0;
    last_text.clone_from(&partial.text);
    heard(
        Heard {
            text: tail_words(&partial.text, 12),
            end_ms: fed_ms as u64,
            inference_ms: appended.elapsed().as_millis() as u64,
        },
        &partial.text,
        partial.done,
    );
}

/// One mic buffer as a mono float `AVAudioPCMBuffer` at its own rate.
fn append(
    request: &SFSpeechAudioBufferRecognitionRequest,
    samples: &[f32],
    rate: u32,
) -> Result<(), String> {
    if samples.is_empty() {
        return Ok(());
    }
    // SAFETY: standard float, non-interleaved, one channel; the buffer gets exactly
    // `samples.len()` frames of capacity and channel 0's pointer is valid for that many floats.
    unsafe {
        let format = AVAudioFormat::initStandardFormatWithSampleRate_channels(
            AVAudioFormat::alloc(),
            f64::from(rate),
            1,
        )
        .ok_or("audio format")?;
        let buffer = AVAudioPCMBuffer::initWithPCMFormat_frameCapacity(
            AVAudioPCMBuffer::alloc(),
            &format,
            samples.len() as u32,
        )
        .ok_or("audio buffer")?;
        buffer.setFrameLength(samples.len() as u32);
        let channels = buffer.floatChannelData();
        if channels.is_null() {
            return Err("audio buffer has no float data".into());
        }
        std::ptr::copy_nonoverlapping(samples.as_ptr(), (*channels).as_ptr(), samples.len());
        request.appendAudioPCMBuffer(&buffer);
    }
    Ok(())
}

/// Follows the capture's mic (through `listener`) for the strip until `stop`.
pub fn run(
    app: &tauri::AppHandle,
    listener: &motetto_core::mics::Listener,
    stop: &Stop,
    locale: &str,
) -> Result<(), String> {
    use tauri::{Emitter, Manager};
    let (tx, audio) = mpsc::sync_channel(256);
    let id = motetto_core::mics::listen(listener, tx);
    let hint = || {
        app.state::<crate::follow::State>()
            .hint
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    };
    let result = objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
        // Finals too: at a pause Apple often adds or corrects the last words only there.
        follow(&audio, locale, &hint, stop, &mut |heard, _, _| {
            if let Some(window) = app.get_webview_window("prompter") {
                let _ = window.emit("motetto://follow-heard", heard);
            }
        })
    }))
    .unwrap_or_else(|e| Err(format!("Apple recognition stopped: {e:?}")));
    motetto_core::mics::unlisten(listener, id);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// By hand on a Mac: MOTETTO_FOLLOW_WAV=<a spoken mic WAV> feeds it at real speed, as the
    /// capture does, and prints when each new word reached the strip against when it ended.
    #[test]
    #[ignore = "needs Speech Recognition permission and MOTETTO_FOLLOW_WAV"]
    fn measure() {
        let path = std::env::var("MOTETTO_FOLLOW_WAV").unwrap();
        let locale = std::env::var("MOTETTO_FOLLOW_LOCALE").unwrap_or("en-US".into());
        let hint = std::env::var("MOTETTO_FOLLOW_HINT").unwrap_or_default();
        assert_eq!(
            ask(),
            Access::Allowed,
            "allow Speech Recognition for this terminal"
        );
        let mut reader = hound::WavReader::open(&path).unwrap();
        let spec = reader.spec();
        let samples: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => reader.samples::<f32>().map(Result::unwrap).collect(),
            hound::SampleFormat::Int => reader
                .samples::<i32>()
                .map(|s| s.unwrap() as f32 / (1 << (spec.bits_per_sample - 1)) as f32)
                .collect(),
        };
        let mono: Vec<f32> = samples
            .iter()
            .step_by(spec.channels as usize)
            .copied()
            .collect();
        let rate = spec.sample_rate;
        let (tx, rx) = mpsc::sync_channel(256);
        let stop = Stop::default();
        let feeder_stop = stop.clone();
        let wall = Instant::now();
        let feeder = std::thread::spawn(move || {
            let chunk = (rate / 50) as usize; // 20 ms, about what the capture delivers
            for (n, part) in mono.chunks(chunk).enumerate() {
                let due = Duration::from_millis(n as u64 * 20);
                if let Some(wait) = due.checked_sub(wall.elapsed()) {
                    std::thread::sleep(wait);
                }
                let _ = tx.try_send((part.to_vec(), rate));
            }
            std::thread::sleep(Duration::from_secs(2));
            feeder_stop.store(true, Ordering::SeqCst);
        });
        // Every result with its arrival on the audio clock; word end times come from a separate
        // whisper-cli -ml 1 pass (Apple gives none in partial results), and a script lines them up.
        let started = wall;
        follow(&rx, &locale, &|| hint.clone(), &stop, &mut |_, text, _| {
            println!("SHOW {:.3} {text}", started.elapsed().as_secs_f64());
        })
        .unwrap();
        feeder.join().unwrap();
    }

    #[test]
    fn on_device_locales_are_dashed() {
        assert!(on_device_locales().iter().all(|l| !l.contains('_')));
    }
}
