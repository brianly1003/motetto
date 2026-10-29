//! How far a transcription is, for the steps page. Each stage reports its own measured progress
//! (whisper's percent, audio converted, pieces done); stages not yet reached are priced from the
//! time this Mac took for that kind of work, scaled by this run's own speed. Percent is time done
//! out of the expected total, so the percent and the time left always agree.
use crate::events::{NotesProgress, NotesStep, TranscribeProgress as Report, TranscribeStep, Who};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

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

/// Runs `work` while another thread calls `tick` once a second. The ticker stops however `work`
/// ends, a panic included: the scope waits for it, so a flag set only on success would hang the
/// job (and the app's busy flag) for good.
pub(crate) fn ticking<T>(tick: impl Fn() + Sync, work: impl FnOnce() -> T) -> T {
    struct Stop<'a>(&'a AtomicBool);
    impl Drop for Stop<'_> {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
        }
    }
    let finished = AtomicBool::new(false);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            while !finished.load(Ordering::Relaxed) {
                for _ in 0..10 {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    if finished.load(Ordering::Relaxed) {
                        return;
                    }
                }
                tick();
            }
        });
        let _stop = Stop(&finished);
        work()
    })
}

// The notes engine's own counts for the prompt it runs now (one prompt at a time, like whisper):
// prompt tokens read of the total, then words and characters written so far.
static READ: AtomicU32 = AtomicU32::new(0);
static READ_OF: AtomicU32 = AtomicU32::new(0);
static WORDS: AtomicU32 = AtomicU32::new(0);
static WRITTEN: AtomicU32 = AtomicU32::new(0);

/// The thread writing a meeting's notes: only its own prompts count. Read in uses the same engine,
/// one prompt at a time, and a translation waiting or running beside the notes must not move them.
static OWNER: AtomicU64 = AtomicU64::new(0);
static NEXT_THREAD: AtomicU64 = AtomicU64::new(1);
thread_local! {
    static THREAD: u64 = NEXT_THREAD.fetch_add(1, Ordering::Relaxed);
}
fn this_thread() -> u64 {
    THREAD.with(|id| *id)
}
fn counts_here() -> bool {
    OWNER.load(Ordering::Relaxed) == this_thread()
}

/// From the notes engine: `done` of the prompt's `total` tokens read.
pub fn reading(done: u64, total: u64) {
    if !counts_here() {
        return;
    }
    READ.store(done.min(u64::from(u32::MAX)) as u32, Ordering::Relaxed);
    READ_OF.store(total.min(u64::from(u32::MAX)) as u32, Ordering::Relaxed);
}
/// From the notes engine: the answer so far, `words` and `chars` long.
pub fn writing(words: u64, chars: u64) {
    if !counts_here() {
        return;
    }
    WORDS.store(words.min(u64::from(u32::MAX)) as u32, Ordering::Relaxed);
    WRITTEN.store(chars.min(u64::from(u32::MAX)) as u32, Ordering::Relaxed);
}
/// Words in `text`, as the engine streams it (whitespace-separated, so a Vietnamese syllable is
/// one).
pub fn words(text: &str) -> u64 {
    text.split_whitespace().count() as u64
}
fn notes_reset() {
    for counter in [&READ, &READ_OF, &WORDS, &WRITTEN] {
        counter.store(0, Ordering::Relaxed);
    }
}

/// The engine's counts now, for `NotesPlan::report`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NotesCounts {
    pub read: u32,
    pub read_of: u32,
    pub words: u32,
    pub written: u32,
}
pub fn notes_counts() -> NotesCounts {
    NotesCounts {
        read: READ.load(Ordering::Relaxed),
        read_of: READ_OF.load(Ordering::Relaxed),
        words: WORDS.load(Ordering::Relaxed),
        written: WRITTEN.load(Ordering::Relaxed),
    }
}

/// The prompts that write a meeting's notes: each transcript part, then (with two or more) the
/// merge of their notes. Each prompt is read, then written. Reading is timed per prompt character
/// from the engine's token counts. Writing can't be told in advance (the model decides when to
/// stop), so the percent and time left wait until one part has been written and timed.
///
/// Measured on the owner's 100-minute call (6 parts, 2026-10-28): parts read in 12 s and wrote
/// 440 to 690 words at 42 to 51 words a second; the merge wrote 2,696 words, 92 % of the parts'
/// 2,923, at 32 words a second (its longer context), 85 s of the 222.
pub struct NotesPlan {
    /// Characters per prompt; the merge's is known only when it starts (its parts' notes).
    chars: Vec<f64>,
    current: usize,
    started: f64,
    prompt_started: f64,
    /// When the first word of the running prompt came: reading ended.
    writing_from: Option<f64>,
    /// Per finished part: seconds per prompt character read, seconds writing, words written.
    read_costs: Vec<f64>,
    writes: Vec<(f64, f64)>,
    /// Characters written by the finished parts: what the merge will read.
    notes_chars: f64,
}

/// The merge's length, as a share of all the parts' words, and its writing speed, as a share of
/// the parts' (see `NotesPlan`).
const MERGE_WORDS: f64 = 0.92;
const MERGE_SPEED: f64 = 0.75;

impl NotesPlan {
    /// `parts` are the transcript parts' prompt sizes in characters; a merge follows two or more.
    pub fn new(parts: &[usize], now: f64) -> Self {
        notes_reset();
        OWNER.store(this_thread(), Ordering::Relaxed);
        let mut chars: Vec<f64> = parts.iter().map(|&c| c as f64).collect();
        if parts.len() > 1 {
            chars.push(0.0);
        }
        Self {
            chars,
            current: 0,
            started: now,
            prompt_started: now,
            writing_from: None,
            read_costs: vec![],
            writes: vec![],
            notes_chars: 0.0,
        }
    }

    /// Prompt `index` starts with `chars` characters (the merge's are known only now); the
    /// running one is done, with what the engine `counted` for it.
    pub fn begin(&mut self, index: usize, chars: usize, counted: NotesCounts, now: f64) {
        if index > self.current {
            let read = self.chars.get(self.current).copied().unwrap_or(0.0);
            if let Some(from) = self.writing_from
                && read > 0.0
            {
                self.read_costs.push((from - self.prompt_started) / read);
                self.writes.push((now - from, f64::from(counted.words)));
            }
            self.notes_chars += f64::from(counted.written);
        }
        self.current = index;
        if let Some(slot) = self.chars.get_mut(index) {
            *slot = chars as f64;
        }
        self.prompt_started = now;
        self.writing_from = None;
        notes_reset();
    }

    /// The notes are written: nothing counts until the next plan.
    pub fn end(&self) {
        OWNER.store(0, Ordering::Relaxed);
    }

    fn is_merge(&self, index: usize) -> bool {
        self.chars.len() > 1 && index == self.chars.len() - 1
    }

    fn average(values: impl Iterator<Item = f64>) -> Option<f64> {
        let (sum, n) = values.fold((0.0, 0), |(s, n), v| (s + v, n + 1));
        (n > 0).then(|| sum / f64::from(n))
    }

    /// Seconds per prompt character read: finished parts, else the running one from its reports.
    fn read_cost(&self, now: f64, counts: NotesCounts) -> Option<f64> {
        Self::average(self.read_costs.iter().copied()).or_else(|| {
            let chars = self.chars.get(self.current).copied()?;
            let share = f64::from(counts.read) / f64::from(counts.read_of.max(1));
            let until = self.writing_from.unwrap_or(now);
            (share >= 0.1 && chars > 0.0).then(|| (until - self.prompt_started) / (chars * share))
        })
    }

    pub fn report(&mut self, now: f64, counts: NotesCounts) -> NotesProgress {
        if counts.words > 0 && self.writing_from.is_none() {
            self.writing_from = Some(now);
        }
        let step = if counts.words > 0 {
            NotesStep::Writing {
                words: counts.words,
            }
        } else {
            NotesStep::Reading {
                percent: (counts.read_of > 0)
                    .then(|| (counts.read * 100 / counts.read_of).min(100) as u8),
            }
        };
        let left = (|| {
            let write = Self::average(self.writes.iter().map(|(secs, _)| *secs))?;
            let words = Self::average(self.writes.iter().map(|(_, words)| *words))?;
            let speed = self.writes.iter().map(|(_, w)| w).sum::<f64>()
                / self.writes.iter().map(|(s, _)| s).sum::<f64>().max(1.0);
            let read = self.read_cost(now, counts)?;
            let parts = self.chars.len().saturating_sub(1).max(1);
            // The merge writes most of what the parts did: theirs so far, an average part's for
            // each still to come.
            let merge_words = MERGE_WORDS
                * (self.writes.iter().map(|(_, w)| w).sum::<f64>()
                    + words * parts.saturating_sub(self.writes.len()) as f64);
            let writing = |index: usize| {
                if self.is_merge(index) {
                    merge_words / (speed * MERGE_SPEED)
                } else {
                    write
                }
            };
            let chars = self.chars.get(self.current).copied().unwrap_or(0.0);
            let now_left = match self.writing_from {
                // The merge at its own pace once it has written enough to time.
                Some(from) if self.is_merge(self.current) && counts.words >= 50 => {
                    let pace = f64::from(counts.words) / (now - from).max(1.0);
                    (merge_words - f64::from(counts.words)).max(0.0) / pace
                }
                Some(from) => (writing(self.current) - (now - from)).max(0.0),
                None => {
                    let share = f64::from(counts.read) / f64::from(counts.read_of.max(1));
                    read * chars * (1.0 - share) + writing(self.current)
                }
            };
            // The merge reads the parts' notes: the finished parts' average length each.
            let per_part = self.notes_chars / self.writes.len().max(1) as f64;
            let ahead: f64 = ((self.current + 1)..self.chars.len())
                .map(|index| {
                    let c = self.chars[index];
                    let c = if self.is_merge(index) && c == 0.0 {
                        per_part * parts as f64
                    } else {
                        c
                    };
                    read * c + writing(index)
                })
                .sum();
            Some(now_left + ahead)
        })();
        let elapsed = (now - self.started).max(0.0);
        let percent = left.map(|left| {
            let total = elapsed + left;
            if total > 0.0 {
                (elapsed / total * 100.0).floor().min(99.0) as u8
            } else {
                0
            }
        });
        NotesProgress {
            percent,
            eta_s: left
                .filter(|_| elapsed >= 30.0)
                .map(|left| left.round() as u32),
            step,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Notes plans claim the process-wide counters for their thread: tests that make one take turns.
    fn serial() -> std::sync::MutexGuard<'static, ()> {
        static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
        TURN.lock().unwrap_or_else(|e| e.into_inner())
    }

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

    fn counts(read: u32, read_of: u32, words: u32, written: u32) -> NotesCounts {
        NotesCounts {
            read,
            read_of,
            words,
            written,
        }
    }

    #[test]
    fn notes_say_only_what_the_engine_counted_until_a_part_is_written() {
        let _turn = serial();
        // Three parts of 30,000 characters and their merge.
        let mut plan = NotesPlan::new(&[30_000, 30_000, 30_000], 0.0);
        let first = plan.report(10.0, counts(4096, 10_000, 0, 0));
        assert_eq!(first.step, NotesStep::Reading { percent: Some(40) });
        assert_eq!(
            (first.percent, first.eta_s),
            (None, None),
            "writing isn't timed yet"
        );
        let writing = plan.report(25.0, counts(10_000, 10_000, 412, 2500));
        assert_eq!(writing.step, NotesStep::Writing { words: 412 });
        assert_eq!(writing.percent, None);
    }

    #[test]
    fn a_written_part_prices_the_rest_of_the_notes() {
        let _turn = serial();
        let mut plan = NotesPlan::new(&[30_000, 30_000, 30_000], 0.0);
        // Part 1: read for 24 s, the first word at 24 s, done at 60 s: 500 words, 3,000 characters.
        plan.report(24.0, counts(10_000, 10_000, 1, 5));
        plan.begin(1, 30_000, counts(10_000, 10_000, 500, 3000), 60.0);
        let report = plan.report(60.0, counts(0, 10_000, 0, 0));
        // Left: parts 2 and 3 at 24 + 36 s each; the merge reads 9,000 characters of notes (7.2 s)
        // and writes 92 % of 1,500 words at three quarters of 13.9 words a second (132.5 s).
        assert_eq!(report.eta_s, Some(260));
        assert_eq!(report.percent, Some(18)); // 60 of 319.7
        // Halfway through reading part 2: 12 s of its reading gone.
        assert_eq!(
            plan.report(72.0, counts(5000, 10_000, 0, 0)).eta_s,
            Some(248)
        );
    }

    #[test]
    fn the_merge_is_timed_at_its_own_pace_once_it_writes() {
        let _turn = serial();
        let mut plan = NotesPlan::new(&[30_000, 30_000], 0.0);
        plan.report(24.0, counts(10_000, 10_000, 1, 5));
        plan.begin(1, 30_000, counts(10_000, 10_000, 500, 3000), 60.0);
        plan.report(84.0, counts(10_000, 10_000, 1, 5));
        plan.begin(2, 6000, counts(10_000, 10_000, 500, 3000), 120.0);
        plan.report(125.0, counts(2000, 2000, 1, 5));
        // 920 words expected; 600 written in 20 s: 320 more at 30 a second.
        let report = plan.report(145.0, counts(2000, 2000, 600, 3600));
        assert_eq!(report.eta_s, Some(11));
        assert_eq!(report.step, NotesStep::Writing { words: 600 });
    }

    #[test]
    fn one_part_has_no_merge_and_no_percent() {
        let _turn = serial();
        let mut plan = NotesPlan::new(&[12_000], 0.0);
        let report = plan.report(40.0, counts(4000, 4000, 300, 1800));
        assert_eq!((report.percent, report.eta_s), (None, None));
        assert_eq!(report.step, NotesStep::Writing { words: 300 });
    }

    #[test]
    fn only_the_thread_writing_the_notes_moves_their_counts() {
        let _turn = serial();
        let plan = NotesPlan::new(&[1000], 0.0);
        writing(10, 50);
        // A translation on another thread, through the same engine.
        std::thread::spawn(|| writing(999, 9999)).join().unwrap();
        assert_eq!(notes_counts().words, 10);
        plan.end();
        writing(20, 90);
        assert_eq!(notes_counts().words, 10, "nothing counts after the notes");
    }

    #[test]
    fn words_count_vietnamese_syllables() {
        assert_eq!(words("Cập nhật luồng  Optimile\nWeb"), 5);
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
