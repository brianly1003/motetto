//! The screen setup's mic meter on macOS: an AVAudioEngine input tap on the system default input
//! that only measures. Nothing is written; the engine lives on the preview's own thread.
use block2::RcBlock;
use objc2_avf_audio::{AVAudioEngine, AVAudioPCMBuffer, AVAudioTime};
use std::{
    ptr::NonNull,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
        mpsc,
    },
};

/// Runs the tap until `stop` sends or is dropped; `started` gets the start's result first.
pub fn run(
    level: Arc<AtomicU32>,
    started: mpsc::Sender<Result<(), String>>,
    stop: mpsc::Receiver<()>,
) {
    let store = level.clone();
    let tap = RcBlock::new(
        move |buffer: NonNull<AVAudioPCMBuffer>, _: NonNull<AVAudioTime>| {
            // SAFETY: AVAudioEngine hands the tap a valid buffer for the call; channel 0 of a float
            // (the input node's own) format holds `frameLength` samples, and nothing is kept.
            let samples = unsafe {
                let buffer = buffer.as_ref();
                let channels = buffer.floatChannelData();
                if channels.is_null() {
                    return;
                }
                std::slice::from_raw_parts((*channels).as_ptr(), buffer.frameLength() as usize)
            };
            store.store(
                crate::mic_preview::rms(samples).to_bits(),
                Ordering::Relaxed,
            );
        },
    );
    // SAFETY: the engine and its input node are made and used only on this thread; the tap is
    // installed before the engine starts and removed before it stops, and the block outlives it.
    // With no input device AVAudioEngine throws instead of failing: caught, and the meter hides.
    let made = crate::trace::catch(|| unsafe {
        let engine = AVAudioEngine::new();
        let input = engine.inputNode();
        let format = input.outputFormatForBus(0);
        if format.channelCount() == 0 || format.sampleRate() <= 0.0 {
            return Err("No microphone to listen to.".to_string());
        }
        let block: *const _ = &*tap;
        input.installTapOnBus_bufferSize_format_block(0, 1024, Some(&format), block as *mut _);
        engine.prepare();
        match engine.startAndReturnError() {
            Ok(()) => Ok(engine),
            Err(error) => {
                input.removeTapOnBus(0);
                Err(error.localizedDescription().to_string())
            }
        }
    });
    let engine = match made {
        Ok(Ok(engine)) => engine,
        Ok(Err(error)) => {
            let _ = started.send(Err(error));
            return;
        }
        Err(_) => {
            let _ = started.send(Err("The microphone can't be listened to.".into()));
            return;
        }
    };
    let _ = started.send(Ok(()));
    let _ = stop.recv();
    // SAFETY: the same thread that started the engine removes its tap and stops it; a throw
    // here (the device vanished) is caught so the thread still ends.
    let _ = crate::trace::catch(|| unsafe {
        engine.inputNode().removeTapOnBus(0);
        engine.stop();
    });
    level.store(0, Ordering::Relaxed);
}
