//! AVAudioEngine playback of a meeting's tracks: one player node per WAV, all through one
//! time-pitch unit, so Me and Them share a clock, gaps stay silent and speed keeps the pitch.
use objc2::{AllocAnyThread, rc::Retained};
use objc2_avf_audio::{
    AVAudioEngine, AVAudioFile, AVAudioMixerNode, AVAudioNode, AVAudioPlayerNode, AVAudioTime,
    AVAudioUnitTimePitch,
};
use objc2_foundation::{NSString, NSURL};
use std::path::PathBuf;

unsafe extern "C" {
    fn mach_absolute_time() -> u64;
    fn mach_timebase_info(info: *mut [u32; 2]) -> i32;
}

struct Track {
    node: Retained<AVAudioPlayerNode>,
    file: Retained<AVAudioFile>,
    rate: f64,
    offset_ms: f64,
}

pub struct Engine {
    engine: Retained<AVAudioEngine>,
    pitch: Retained<AVAudioUnitTimePitch>,
    tracks: Vec<Track>,
    playing: bool,
    /// Meeting time where the nodes started, or the paused position.
    start_ms: f64,
    rate: f64,
}

impl Engine {
    pub fn open(paths: &[(PathBuf, u64)]) -> Result<Self, String> {
        // SAFETY: every object is created and used only on the playback worker thread. Nodes
        // are attached before they are connected, and connected before the engine starts.
        unsafe {
            let engine = AVAudioEngine::new();
            let mix = AVAudioMixerNode::new();
            let pitch = AVAudioUnitTimePitch::new();
            engine.attachNode(&mix);
            engine.attachNode(&pitch);
            let mut tracks = Vec::new();
            for (path, offset) in paths {
                let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
                let file = AVAudioFile::initForReading_error(AVAudioFile::alloc(), &url)
                    .map_err(|e| e.localizedDescription().to_string())?;
                let format = file.processingFormat();
                let node = AVAudioPlayerNode::new();
                engine.attachNode(&node);
                engine.connect_to_format(&node, &mix, Some(&format));
                tracks.push(Track {
                    rate: format.sampleRate(),
                    node,
                    file,
                    offset_ms: *offset as f64,
                });
            }
            engine.connect_to_format(&mix, &pitch, None);
            engine.connect_to_format(&pitch, &engine.mainMixerNode(), None);
            engine.prepare();
            Ok(Self {
                engine,
                pitch,
                tracks,
                playing: false,
                start_ms: 0.0,
                rate: 1.0,
            })
        }
    }

    /// The meeting position: player time counts source samples, so it already includes speed.
    pub fn position(&self) -> f64 {
        if !self.playing {
            return self.start_ms;
        }
        let Some(track) = self.tracks.first() else {
            return self.start_ms;
        };
        // SAFETY: worker-thread objects; both calls return nil while the node is not rendering.
        let played = unsafe {
            let node: &AVAudioNode = &track.node;
            node.lastRenderTime()
                .and_then(|render| track.node.playerTimeForNodeTime(&render))
                .map_or(0.0, |time| {
                    time.sampleTime().max(0) as f64 / time.sampleRate()
                })
        };
        self.start_ms + played * 1000.0
    }

    /// The position, after starting again where it was if a device change stopped the engine.
    pub fn poll(&mut self) -> Result<f64, String> {
        // SAFETY: worker-thread object.
        if self.playing && unsafe { !self.engine.isRunning() } {
            self.play(self.position())?;
        }
        Ok(self.position())
    }

    pub fn set(&mut self, playing: bool, at_ms: Option<f64>, rate: f64) -> Result<f64, String> {
        let now = self.position();
        // SAFETY: worker-thread objects; rate is checked to 0.5-4, inside the unit's 1/32-32.
        unsafe { self.pitch.setRate(rate as f32) };
        self.rate = rate;
        let from = at_ms.unwrap_or(now);
        // A device change stops the engine; the next command starts it again where it was.
        // SAFETY: worker-thread object.
        let stalled = self.playing && unsafe { !self.engine.isRunning() };
        if !playing {
            self.stop();
            self.start_ms = from;
        } else if !self.playing || at_ms.is_some() || stalled {
            self.play(from)?;
        }
        Ok(self.position())
    }

    fn stop(&mut self) {
        for track in &self.tracks {
            // SAFETY: worker-thread object; stop also drops the scheduled segments.
            unsafe { track.node.stop() };
        }
        self.playing = false;
    }

    fn play(&mut self, from_ms: f64) -> Result<(), String> {
        // An audio device change can stop the engine between any two calls, and starting a node
        // then raises an Objective-C exception, which would abort the app: catch it, start the
        // engine again and retry once.
        let mut result = self.try_play(from_ms);
        if result.is_err() {
            result = self.try_play(from_ms);
        }
        if result.is_err() {
            self.stop();
        }
        result
    }

    fn try_play(&mut self, from_ms: f64) -> Result<(), String> {
        self.stop();
        // SAFETY: worker-thread object.
        if unsafe { !self.engine.isRunning() } {
            // SAFETY: worker-thread object; its nodes are attached and connected.
            unsafe { self.engine.startAndReturnError() }
                .map_err(|e| e.localizedDescription().to_string())?;
        }
        let tracks = &self.tracks;
        // SAFETY: worker-thread objects. Frames stay inside each file (0..length); a later
        // part waits in player time, which follows the speed like the audio does. All nodes
        // start at one host time 50 ms ahead so Me and Them begin on the same sample. A thrown
        // exception leaves nothing half-owned: the nodes are stopped again by the caller.
        objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
            for track in tracks {
                let length = track.file.length();
                let skip = ((from_ms - track.offset_ms) / 1000.0 * track.rate).round() as i64;
                if skip >= length {
                    continue;
                }
                let first = skip.max(0);
                let when =
                    (skip < 0).then(|| AVAudioTime::timeWithSampleTime_atRate(-skip, track.rate));
                track
                    .node
                    .scheduleSegment_startingFrame_frameCount_atTime_completionHandler(
                        &track.file,
                        first,
                        (length - first).min(u32::MAX as i64) as u32,
                        when.as_deref(),
                        std::ptr::null_mut(),
                    );
            }
            let mut base = [0u32; 2];
            mach_timebase_info(&mut base);
            let ahead = 50_000_000 * u64::from(base[1]) / u64::from(base[0].max(1));
            let start = AVAudioTime::timeWithHostTime(mach_absolute_time() + ahead);
            for track in tracks {
                track.node.playAtTime(Some(&start));
            }
        }))
        .map_err(|e| {
            e.map_or("Audio playback failed".to_string(), |e| {
                format!("Audio playback failed: {e:?}")
            })
        })?;
        self.start_ms = from_ms;
        self.playing = true;
        Ok(())
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop();
        // SAFETY: worker-thread object, stopped before it is released.
        unsafe { self.engine.stop() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{thread::sleep, time::Duration};

    fn silent(path: &std::path::Path, rate: u32, seconds: u32) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut wav = hound::WavWriter::create(path, spec).unwrap();
        for _ in 0..rate * seconds {
            wav.write_sample(0.0f32).unwrap();
        }
        wav.finalize().unwrap();
    }

    /// Plays silence on the default output: clock, speed, pause, seek and a later part.
    #[test]
    fn one_clock_follows_speed_pause_and_seek() {
        let dir = std::env::temp_dir().join(format!("motetto-playback-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (me, them, later) = (
            dir.join("a.mic.wav"),
            dir.join("a.system.wav"),
            dir.join("b.mic.wav"),
        );
        silent(&me, 48000, 4);
        silent(&them, 16000, 4);
        silent(&later, 48000, 4);
        let mut engine = Engine::open(&[(me, 0), (them, 0), (later, 6000)]).unwrap();
        let near = |got: f64, want: f64| assert!((got - want).abs() < 120.0, "{got} vs {want}");

        near(engine.set(true, Some(1000.0), 1.0).unwrap(), 1000.0);
        sleep(Duration::from_millis(550));
        near(engine.poll().unwrap(), 1500.0);
        engine.set(true, None, 2.0).unwrap();
        let before = engine.poll().unwrap();
        sleep(Duration::from_millis(500));
        near(engine.poll().unwrap(), before + 1000.0);
        let paused = engine.set(false, None, 2.0).unwrap();
        sleep(Duration::from_millis(200));
        near(engine.poll().unwrap(), paused);
        // A device change stops the engine: the next command starts it again where it was.
        // SAFETY: test thread owns the engine.
        unsafe { engine.engine.stop() };
        near(engine.set(true, Some(3000.0), 1.0).unwrap(), 3000.0);
        sleep(Duration::from_millis(300));
        near(engine.poll().unwrap(), 3300.0);
        // The gap between parts (4-6 s) and the later part keep counting.
        engine.set(true, Some(4500.0), 1.0).unwrap();
        sleep(Duration::from_millis(2050));
        near(engine.poll().unwrap(), 6500.0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// AVFoundation reports misuse by throwing (the harness died that way on an audio device
    /// change, 2026-10-27); `catch` must turn it into an error, never abort the app. A player
    /// node outside any engine throws on play.
    #[test]
    fn objc_exception_is_an_error() {
        // SAFETY: a fresh node on the test thread; the throw is what is tested.
        let thrown = objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
            AVAudioPlayerNode::new().play()
        }));
        assert!(thrown.is_err());
    }
}
