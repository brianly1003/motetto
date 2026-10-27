//! How far a transcription is, for the steps page. Each stage reports its own measured progress
//! (whisper's percent, audio converted, pieces done); stages not yet reached are priced from the
//! time this Mac took for that kind of work, scaled by this run's own speed. Percent is time done
//! out of the expected total, so the percent and the time left always agree.
use crate::events::{TranscribeProgress as Report, TranscribeStep, Who};
use std::sync::atomic::{AtomicU32, Ordering};

/// The running stage's progress, parts per million. One transcription runs at a time (the
/// desktop's busy flag), like the Cancel flag in `tools`.
static DONE: AtomicU32 = AtomicU32::new(0);

/// From inside a stage: `done` of `total` units (frames, percent, pieces).
pub fn step(done: u64, total: u64) {
    if total > 0 {
        let share = done.min(total) as f64 / total as f64;
        DONE.store((share * 1e6) as u32, Ordering::Relaxed);
    }
}

pub(crate) fn done() -> f64 {
    f64::from(DONE.load(Ordering::Relaxed)) / 1e6
}

/// Seconds of work per unit, measured on an M-series Mac (2026-10-28) over two runs of a 100-minute
/// call and 30- and 5-minute stretches of it. They only price stages not reached yet; a running
/// stage is timed from its own progress and finished stages of the same kind correct the rest.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Convert,
    Echo,
    Speech,
    Whisper,
    Check,
}
impl Kind {
    fn cost(self) -> f64 {
        match self {
            // Per second of audio: 3.1 s and 1.0 s for 100 and 30 minutes.
            Kind::Convert => 0.0006,
            // 40.6 s and 12.5 s.
            Kind::Echo => 0.0068,
            // 16.5 s and 3.3 s.
            Kind::Speech => 0.0025,
            // Per unit of `whisper_units`: 145 s for 4310 and 44 s for 1483.
            Kind::Whisper => 0.033,
            // Per piece (a whisper-cli start each): 0.6 s to 1.1 s, shorter on the mic.
            Kind::Check => 1.0,
        }
    }
}

/// Whisper's work: the speech it decodes, plus its own speech scan over the whole track (about a
/// tenth of a second of speech per second of audio). Before the speech check, a guess: the mic
/// was 15 % speech and the call 62 % on a 100-minute call (one voice against all the others).
pub fn whisper_units(audio: f64, speech: Option<f64>, side: Option<Who>) -> f64 {
    let share = if side == Some(Who::You) { 0.25 } else { 0.5 };
    speech.unwrap_or(audio * share) + audio / 10.0
}

/// Second-pass pieces expected before whisper's lines are known, per second of audio: the call
/// side had 0.023 to 0.035, the mic 0.012 to 0.021.
pub fn pieces_expected(audio: f64, side: Option<Who>) -> f64 {
    audio * if side == Some(Who::You) { 0.015 } else { 0.028 }
}

#[derive(Clone, Debug)]
pub struct Item {
    pub kind: Kind,
    pub side: Option<Who>,
    /// Units of `kind`'s cost: audio (or speech) seconds, or pieces.
    pub units: f64,
    actual: Option<f64>,
}
impl Item {
    pub fn new(kind: Kind, side: Option<Who>, units: f64) -> Self {
        Self {
            kind,
            side,
            units,
            actual: None,
        }
    }
    fn priced(&self) -> f64 {
        self.units * self.kind.cost()
    }
}

/// A transcription's stages in order, and when the current one began (seconds on the caller's
/// clock, so tests can pass their own).
pub struct Plan {
    pub items: Vec<Item>,
    current: usize,
    started: f64,
    stage_started: f64,
    /// The running stage's first and latest progress reports (time, share done). Its rate is timed
    /// between them, so setup before the first report (whisper's own speech scan, 22 s on a
    /// 100-minute call) doesn't count as progress at 0 %.
    first: Option<(f64, f64)>,
    latest: Option<(f64, f64)>,
}

impl Plan {
    pub fn new(items: Vec<Item>, now: f64) -> Self {
        DONE.store(0, Ordering::Relaxed);
        Self {
            items,
            current: 0,
            started: now,
            stage_started: now,
            first: None,
            latest: None,
        }
    }

    /// Stage `index` begins: the running one is done; any between were skipped and price nothing.
    pub fn begin(&mut self, index: usize, now: f64) {
        if let Some(item) = self.items.get_mut(self.current)
            && self.current < index
        {
            item.actual = Some(now - self.stage_started);
        }
        self.current = index;
        self.stage_started = now;
        (self.first, self.latest) = (None, None);
        DONE.store(0, Ordering::Relaxed);
    }

    /// How much slower (above 1) this run is than the priced cost, from finished stages of the
    /// same kind; 1 when there are none. Across kinds one fast stage misjudged a whole run 4x
    /// (2026-10-28), so other kinds don't count.
    fn speed(&self, kind: Kind) -> f64 {
        let (actual, priced) = self.items[..self.current]
            .iter()
            .filter(|item| item.kind == kind)
            .filter_map(|item| Some((item.actual?, item.priced())))
            .fold((0.0, 0.0), |(a, p), (actual, priced)| {
                (a + actual, p + priced)
            });
        if priced >= 2.0 { actual / priced } else { 1.0 }
    }

    /// The time left in the running stage, `done` (0 to 1) through it.
    fn left_now(&mut self, now: f64, done: f64) -> f64 {
        if done > self.latest.map_or(0.0, |(_, d)| d) {
            self.first.get_or_insert((now, done));
            self.latest = Some((now, done));
        }
        let Some(item) = self.items.get(self.current) else {
            return 0.0;
        };
        let spent = now - self.stage_started;
        match (self.first, self.latest) {
            // Timed from its own reports: the rest at that rate, less what passed since the last.
            (Some((t0, d0)), Some((t1, d1))) if d1 > d0 && t1 > t0 => {
                let rate = (d1 - d0) / (t1 - t0);
                ((1.0 - d1) / rate - (now - t1)).max(0.0)
            }
            // Not moved yet: its price, at this run's speed for the kind.
            _ => (item.priced() * self.speed(item.kind) * (1.0 - done) - spent).max(0.0),
        }
    }

    /// `done` is the running stage's own progress (0 to 1).
    pub fn report(&mut self, now: f64, done: f64) -> Report {
        let elapsed = (now - self.started).max(0.0);
        let left = self.left_now(now, done)
            + self.items[(self.current + 1).min(self.items.len())..]
                .iter()
                .map(|item| item.priced() * self.speed(item.kind))
                .sum::<f64>();
        let total = elapsed + left;
        let percent = if total > 0.0 {
            (elapsed / total * 100.0).floor().min(99.0) as u8
        } else {
            0
        };
        Report {
            percent,
            // Too early to say: the first stages alone don't tell this run's speed.
            eta_s: (elapsed >= 30.0).then(|| left.round() as u32),
            step: self.step(done),
        }
    }

    fn step(&self, done: f64) -> TranscribeStep {
        let Some(item) = self.items.get(self.current) else {
            return TranscribeStep::Transcribing { side: None };
        };
        let side = item.side;
        match item.kind {
            Kind::Convert => TranscribeStep::Converting { side },
            Kind::Echo => TranscribeStep::RemovingEcho,
            Kind::Speech | Kind::Whisper => TranscribeStep::Transcribing { side },
            Kind::Check => {
                let of = item.units.round() as u32;
                TranscribeStep::Checking {
                    side,
                    piece: ((done * f64::from(of)).floor() as u32 + 1).min(of),
                    of,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two whisper stages priced 40 s each.
    fn plan() -> Plan {
        Plan::new(
            vec![
                Item::new(Kind::Whisper, Some(Who::Them), 40.0 / 0.033),
                Item::new(Kind::Whisper, Some(Who::You), 40.0 / 0.033),
            ],
            0.0,
        )
    }

    #[test]
    fn a_stage_without_reports_takes_its_price() {
        let mut plan = plan();
        // 10 s of the first 40 gone, then 40: 80 s in all.
        let report = plan.report(10.0, 0.0);
        assert_eq!(report.percent, 12);
        assert_eq!(report.eta_s, None, "no time left before 30 s");
    }

    #[test]
    fn a_running_stage_is_timed_between_its_own_reports() {
        let mut plan = plan();
        // whisper scans for speech first: nothing for 22 s, then 5 % to 30 % in 10 s.
        plan.report(22.0, 0.05);
        let report = plan.report(32.0, 0.30);
        // 2.5 % a second: 28 s for the rest, then 40 s. Timing from the stage's start instead
        // (32 s for 30 %) would have said 75 s.
        assert_eq!(report.eta_s, Some(68));
        assert_eq!(report.percent, 32); // 32 of 100
        // Between reports the time left counts down.
        assert_eq!(plan.report(36.0, 0.30).eta_s, Some(64));
    }

    #[test]
    fn a_finished_stage_sets_the_speed_of_the_next_of_its_kind_only() {
        let mut plan = Plan::new(
            vec![
                Item::new(Kind::Convert, None, 40.0 / 0.0006), // priced 40 s
                Item::new(Kind::Whisper, None, 40.0 / 0.033),  // priced 40 s
                Item::new(Kind::Whisper, None, 40.0 / 0.033),  // priced 40 s
            ],
            0.0,
        );
        // Converting took 10 s, a quarter of its price: whisper keeps its own.
        plan.begin(1, 10.0);
        assert_eq!(plan.report(10.0, 0.0).eta_s, None);
        assert_eq!(plan.report(30.0, 0.0).eta_s, Some(20 + 40));
        // The first whisper took 80 s, twice its price: the second is priced at 80 s.
        plan.begin(2, 90.0);
        let report = plan.report(90.0, 0.0);
        assert_eq!(report.eta_s, Some(80));
        assert_eq!(report.percent, 52); // 90 of 170
        assert_eq!(report.step, TranscribeStep::Transcribing { side: None });
    }

    #[test]
    fn pieces_count_from_one_and_never_pass_the_total() {
        let mut plan = Plan::new(vec![Item::new(Kind::Check, Some(Who::Them), 148.0)], 0.0);
        let mut piece = |done| match plan.report(5.0, done).step {
            TranscribeStep::Checking { piece, of, .. } => (piece, of),
            other => panic!("{other:?}"),
        };
        assert_eq!(piece(0.0), (1, 148));
        assert_eq!(piece(36.0 / 148.0), (37, 148));
        assert_eq!(piece(1.0), (148, 148));
    }

    #[test]
    fn percent_stays_below_100_until_the_end() {
        let mut plan = plan();
        plan.begin(1, 40.0);
        plan.report(41.0, 0.5);
        // The last stage done: 99 until the transcript is saved.
        let report = plan.report(80.0, 1.0);
        assert_eq!((report.percent, report.eta_s), (99, Some(0)));
    }
}
