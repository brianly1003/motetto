//! Transcribe stage: each track through Rust decoding (16 kHz mono) and whisper-cli, merged by time.
//! With the diarization models set, the call side and unlabelled recordings are split into speakers.

use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use crate::events::{TranscribeProgress, Who};
use crate::progress::{self, Kind};
use crate::{aec, config, decode, diarize, slice};

/// Who a track belongs to, from its file name.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Speaker {
    Me,
    Them,
    Unknown,
    /// A voice the diarizer told apart: "Speaker 2" and up on a call (Me is the first), from
    /// "Speaker 1" in a single-track recording.
    Voice(usize),
}

impl Speaker {
    /// The track a progress step names: You for the mic, Them for the call.
    fn side(self) -> Option<Who> {
        match self {
            Speaker::Me => Some(Who::You),
            Speaker::Them => Some(Who::Them),
            _ => None,
        }
    }
    /// `x.mic.wav` is Me, `x.system.wav` is Them; any other file is unlabelled.
    fn from_path(path: &str) -> Self {
        if path.ends_with(".mic.wav") {
            Speaker::Me
        } else if path.ends_with(".system.wav") {
            Speaker::Them
        } else {
            Speaker::Unknown
        }
    }

    fn label(self) -> String {
        match self {
            Speaker::Me => "Me: ".into(),
            Speaker::Them => "Them: ".into(),
            Speaker::Unknown => String::new(),
            Speaker::Voice(n) => format!("Speaker {n}: "),
        }
    }

    /// The label for the `id`-th voice the diarizer heard on this track (0 = first heard).
    fn voice(self, id: usize) -> Self {
        match self {
            Speaker::Them => Speaker::Voice(id + 2),
            _ => Speaker::Voice(id + 1),
        }
    }
}

/// One whisper segment placed on the call's timeline.
#[derive(Debug, Clone, PartialEq)]
struct Segment {
    start_ms: u64,
    end_ms: u64,
    microphone: bool,
    system: bool,
    speaker: Speaker,
    text: String,
}

/// A whisper row: start and end in ms, text.
type Row = (u64, u64, String);

fn run(cmd: &mut Command) -> Result<(), Box<dyn Error>> {
    let status = crate::tools::status(cmd)?;
    if !status.success() {
        return Err(format!("{cmd:?} failed: {status}").into());
    }
    Ok(())
}

/// The merged transcript and the call's language (SPEC 6.4 `meeting.json` `language`).
pub struct Transcript {
    pub text: String,
    /// ISO codes of the languages spoken, most text first, comma-separated ("en,vi"); a language
    /// needs 5 % of the text to count. None when no words were heard.
    pub language: Option<String>,
}

/// Desktop recording parts use elapsed recording time, excluding the Stop/Resume pause.
/// Quiet mode prevents whisper from printing private transcript text to desktop logs.
/// `report` hears how far the work is about once a second, from another thread.
pub fn transcript_parts(
    parts: &[(Vec<String>, u64)],
    quiet: bool,
    report: &(dyn Fn(TranscribeProgress) + Sync),
) -> Result<Transcript, Box<dyn Error>> {
    let run = Run::new(parts);
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
                if let Some(progress) = run.report() {
                    report(progress);
                }
            }
        });
        let result = transcribe_parts(parts, quiet, &run);
        finished.store(true, Ordering::Relaxed);
        result
    })
}

/// The stages of one transcription, priced up front (SPEC 6.6): per track, converting, echo
/// removal (the mic of a call), the speech check, whisper and the second-pass pieces.
struct Run {
    plan: Mutex<progress::Plan>,
    clock: Instant,
    /// Per track: the index of its first stage.
    first: Vec<usize>,
}
const CONVERT: usize = 0;
const ECHO: usize = 1;
const SPEECH: usize = 2;
const WHISPER: usize = 3;
const CHECK: usize = 4;
const RECHECK: usize = 5;

impl Run {
    fn new(parts: &[(Vec<String>, u64)]) -> Self {
        let vad = config::var("WHISPER_VAD_MODEL").is_some_and(|v| !v.is_empty());
        let mut items = vec![];
        let mut first = vec![];
        for (sources, _) in parts {
            let system = sources.iter().any(|s| s.ends_with(".system.wav"));
            for src in sources {
                let speaker = Speaker::from_path(src);
                // A WAV's length is in its header; another format's is known once converted.
                let audio = hound::WavReader::open(src)
                    .map(|wav| f64::from(wav.duration()) / f64::from(wav.spec().sample_rate))
                    .unwrap_or(0.0);
                let side = speaker.side();
                let echo = speaker == Speaker::Me && system;
                first.push(items.len());
                items.extend([
                    progress::Item::new(Kind::Convert, side, audio),
                    progress::Item::new(Kind::Echo, side, if echo { audio } else { 0.0 }),
                    progress::Item::new(Kind::Speech, side, if vad { audio } else { 0.0 }),
                    // About half a call is speech until the check says.
                    progress::Item::new(
                        Kind::Whisper,
                        side,
                        progress::whisper_units(audio, None, side),
                    ),
                    progress::Item::new(
                        Kind::Check,
                        side,
                        if vad {
                            progress::pieces_expected(audio, side)
                        } else {
                            0.0
                        },
                    ),
                    progress::Item::new(Kind::Check, side, 0.0),
                ]);
            }
        }
        let clock = Instant::now();
        Self {
            plan: Mutex::new(progress::Plan::new(items, 0.0)),
            clock,
            first,
        }
    }
    fn plan(&self) -> std::sync::MutexGuard<'_, progress::Plan> {
        self.plan.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// `track`'s `stage` begins.
    fn begin(&self, track: usize, stage: usize) {
        let now = self.clock.elapsed().as_secs_f64();
        self.plan().begin(self.first[track] + stage, now);
    }
    /// What `track`'s `stage` will take, once measured: seconds of audio or speech, or pieces.
    fn units(&self, track: usize, stage: usize, units: f64) {
        self.plan().items[self.first[track] + stage].units = units;
    }
    fn report(&self) -> Option<TranscribeProgress> {
        let now = self.clock.elapsed().as_secs_f64();
        Some(self.plan().report(now, progress::done()))
    }
}

fn transcribe_parts(
    parts: &[(Vec<String>, u64)],
    quiet: bool,
    run: &Run,
) -> Result<Transcript, Box<dyn Error>> {
    // Transcribe each track on its own: mic = "Me", system audio = "Them". Merge by timestamp.
    // tracks are aligned by start time only (ms-level skew); fine for transcripts.
    let model = config::var("WHISPER_MODEL").ok_or("set WHISPER_MODEL to a ggml model path")?;
    let vad = config::var("WHISPER_VAD_MODEL").filter(|v| !v.is_empty());
    let mut segments = vec![];
    let mut heard: Vec<(String, usize)> = vec![];
    let mut track = 0;
    for (sources, offset_ms) in parts {
        // The mic needs the system track decoded for echo cancellation; each track is decoded once.
        let mut decoded: Vec<String> = Vec::new();
        let mut decode16 = |src: &String| -> Result<String, Box<dyn Error>> {
            let stem = Path::new(src).with_extension("");
            let wav16 = format!("{}.16k.wav", stem.to_string_lossy());
            if !decoded.contains(src) {
                decode::to_wav(Path::new(src), Path::new(&wav16))?;
                decoded.push(src.clone());
            }
            Ok(wav16)
        };
        for src in sources {
            let speaker = Speaker::from_path(src);
            let stem = Path::new(src)
                .with_extension("")
                .to_string_lossy()
                .into_owned();
            run.begin(track, CONVERT);
            let mut wav16 = decode16(src)?;
            let audio = hound::WavReader::open(&wav16)
                .map(|wav| f64::from(wav.duration()) / 16_000.0)
                .unwrap_or(0.0);
            // Through loudspeakers the mic records the other side again: cancel that echo
            // against the system track and transcribe (and play) the cleaned mic instead.
            let system = sources.iter().find(|s| s.ends_with(".system.wav"));
            if let (Speaker::Me, Some(system)) = (speaker, system) {
                run.begin(track, ECHO);
                let system16 = decode16(system)?;
                let cleaned = format!("{stem}.aec.wav");
                let _ = fs::remove_file(&cleaned);
                if let Some(echo) =
                    aec::cancel(Path::new(&wav16), Path::new(&system16), Path::new(&cleaned))
                        .map_err(|e| format!("echo cancellation of {}: {e}", name(src)))?
                {
                    println!(
                        "Removed loudspeaker echo from {src} ({} ms behind, match {:.2})",
                        echo.lag_ms, echo.matched
                    );
                    wav16 = cleaned;
                }
            }
            // The speech whisper will hear, found once: it prices whisper and finds the long
            // silences and invented credits below.
            let speech = vad.as_deref().and_then(|vad| {
                run.begin(track, SPEECH);
                slice::speech(Path::new(&wav16), vad)
                    .map_err(|e| eprintln!("Long-silence check skipped for {wav16}: {e}"))
                    .ok()
            });
            let spoken = speech.as_ref().map(|speech| {
                speech
                    .iter()
                    .map(|(s, e)| e.saturating_sub(*s))
                    .sum::<u64>() as f64
                    / 1000.0
            });
            run.units(
                track,
                WHISPER,
                progress::whisper_units(audio, spoken, speaker.side()),
            );
            run.begin(track, WHISPER);
            println!("Transcribing {src} with {model}...");
            let rows = whisper(&model, &wav16, &stem, quiet, true)?;
            let track_language = fs::read_to_string(format!("{stem}.json"))
                .ok()
                .and_then(|json| detected(&json));
            let _ = fs::remove_file(format!("{stem}.json"));
            let rows = match &speech {
                Some(speech) => {
                    let pieces = split_pieces(&rows, speech);
                    run.units(track, CHECK, pieces as f64);
                    run.begin(track, CHECK);
                    split_joined(&model, &wav16, &stem, rows, speech, pieces, quiet)?
                }
                None => rows, // without VAD whisper keeps the silence and splits there itself
            };
            let spans = track_language
                .as_deref()
                .map_or(vec![], |track| rerun_spans(&rows, track));
            run.units(track, RECHECK, spans.len() as f64);
            if !spans.is_empty() {
                run.begin(track, RECHECK);
            }
            let rows = relanguage(
                &model,
                &wav16,
                &stem,
                rows,
                &spans,
                track_language.as_deref(),
                quiet,
            )?;
            // A short loop can still slip through the 64-token context; drop it here, with
            // invented subtitle credits (after the split, so a credit piece doesn't keep a row
            // whole).
            let loops = crate::echo::loops(&rows);
            let invented = invented_credits(&rows, speech.as_deref());
            let rows: Vec<_> = rows
                .into_iter()
                .zip(loops.into_iter().zip(invented))
                .filter_map(|(row, (looped, invented))| (!looped && !invented).then_some(row))
                .map(|(start, end, text)| {
                    (heard_from(Path::new(&wav16), start, end, &text), end, text)
                })
                .collect();
            let speakers = if speaker == Speaker::Me {
                vec![speaker; rows.len()]
            } else {
                voices(src, Path::new(&wav16), speaker, &rows)
            };
            for (_, _, text) in &rows {
                let language = row_language(text, track_language.as_deref());
                let chars = text.chars().count();
                match heard.iter_mut().find(|(l, _)| *l == language) {
                    Some((_, n)) => *n += chars,
                    None => heard.push((language, chars)),
                }
            }
            for ((start_ms, end_ms, text), label) in rows.into_iter().zip(speakers) {
                segments.push(Segment {
                    start_ms: start_ms.saturating_add(*offset_ms),
                    end_ms: end_ms.saturating_add(*offset_ms),
                    microphone: speaker == Speaker::Me,
                    system: speaker == Speaker::Them,
                    speaker: label,
                    text,
                });
            }
            track += 1;
        }
    }
    Ok(Transcript {
        text: merge(segments),
        language: languages(heard),
    })
}

/// Whisper names one language per track, from its first 30 s: a call that opened with a
/// Vietnamese song and went on in English was saved as "vi" (2026-10-23). So each line is named
/// from its own text: a Vietnamese-only letter, kana or Han says vi, ja or zh; any other line
/// takes the track's language unless that is one of those three, when it is English.
/// other Latin-script languages are told apart only by whisper's per-track guess.
fn row_language(text: &str, track: Option<&str>) -> String {
    match crate::summarize::written_in(text) {
        Some(code) => code.to_string(),
        None => match track {
            Some(code) if !matches!(code, "vi" | "ja" | "zh") => code.to_string(),
            _ => "en".to_string(),
        },
    }
}

/// The languages with at least 5 % of the text, most first, as "en,vi"; None without text.
fn languages(mut heard: Vec<(String, usize)>) -> Option<String> {
    let total: usize = heard.iter().map(|(_, chars)| chars).sum();
    heard.retain(|(_, chars)| *chars > 0 && chars * 100 >= total * 5);
    heard.sort_by_key(|(_, chars)| std::cmp::Reverse(*chars));
    (!heard.is_empty()).then(|| {
        heard
            .into_iter()
            .map(|(code, _)| code)
            .collect::<Vec<_>>()
            .join(",")
    })
}

/// The language whisper-cli `-oj` reports in `result.language`, or None for "auto" or none.
fn detected(json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let language = value["result"]["language"].as_str()?.trim();
    (!language.is_empty() && language != "auto").then(|| language.to_string())
}

/// The file name alone, for errors shown in the app.
fn name(path: &str) -> std::borrow::Cow<'_, str> {
    Path::new(path)
        .file_name()
        .map_or(path.into(), |n| n.to_string_lossy())
}

/// whisper-cli on one 16 kHz WAV: rows written to `<stem>.csv` and returned. With `progress`,
/// its own percent becomes the stage's progress.
fn whisper(
    model: &str,
    wav16: &str,
    stem: &str,
    quiet: bool,
    progress: bool,
) -> Result<Vec<Row>, Box<dyn Error>> {
    let mut whisper = crate::tools::command("whisper-cli");
    whisper.args([
        "-m",
        model,
        "-f",
        wav16,
        "-l",
        config::call_language().as_deref().unwrap_or("auto"),
        "-np",
        "-ocsv",
        "-oj", // only for result.language; the caller deletes it
        "-of",
        stem,
    ]);
    // Greedy decoding: 1.5x faster than the default 5-beam search with the same accuracy
    // (10 min of Vietnamese call: 54 s vs 80 s, differences mostly filler words). Splitting
    // the audio (-p) was faster still but dropped speech at the seams, so it stays off.
    whisper.args(["-bs", "1", "-bo", "1"]);
    // 64 tokens of previous text (2026-10-21): the default full context looped one
    // sentence for a minute on a real call, replacing what was said; none (-mc 0) cut the
    // rest into run-on chunks. 64 kept sentences whole with no loop on both calls and the
    // songs. --carry-initial-prompt keeps TERMS, which a short context would push out.
    whisper.args(["-mc", "64", "--carry-initial-prompt"]);
    // VAD gives real segment start times (else leading silence is folded in) and
    // stops hallucinated lines in the long silent stretches of each track.
    if let Some(vad) = config::var("WHISPER_VAD_MODEL").filter(|v| !v.is_empty()) {
        whisper.args(["--vad", "-vm", &vad]);
    }
    // Names whisper can't know (new products, people, project codes) bias it toward the right
    // spelling; tested not to push English words into a Vietnamese call.
    // whisper keeps only the last ~224 prompt tokens; long lists lose their start.
    if let Some(terms) = config::terms() {
        whisper.args(["--prompt", &terms]);
    }
    if quiet {
        whisper
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
    }
    if progress {
        // `-pp` prints "whisper_print_progress_callback: progress =  5%" to stderr; with -np
        // nothing else goes there, and never the transcript.
        whisper.arg("-pp");
        let status = crate::tools::status_lines(&mut whisper, &|line| match line
            .split_once("progress =")
            .map(|(_, n)| n.trim().trim_end_matches('%'))
        {
            Some(percent) => {
                if let Ok(percent) = percent.parse() {
                    progress::step(percent, 100);
                }
            }
            None if !quiet => eprintln!("{line}"),
            None => {}
        })?;
        if !status.success() {
            return Err(format!("{whisper:?} failed: {status}").into());
        }
    } else {
        run(&mut whisper)?;
    }
    let csv = format!("{stem}.csv");
    let text = fs::read_to_string(&csv).map_err(|e| format!("{}: {e}", name(&csv)))?;
    Ok(parse_csv(&text))
}

/// A silence at least this long inside one row means whisper joined two utterances.
const JOINED_GAP_MS: u64 = 5_000;

/// With VAD, whisper drops the silence between utterances and decodes what is left as one stream,
/// so speech either side of a long silence can land in one row stamped with the first one's
/// start: "Anything else?" at 0:28 and "Hello hello hello" at 2:04 became one 0:25 row, which
/// sorts the later words before the other side's reply (2026-10-23). A row that holds a silence
/// of 5 s or more by Silero (typing and clicks are not speech) is transcribed again in pieces
/// cut in the middle of each silence; the other rows, nearly all, stay as whisper wrote them.
/// Optional like diarizing: if the speech check or a piece fails, or a piece comes back empty,
/// the row stays whole. Cancel still stops the job.
fn split_joined(
    model: &str,
    wav16: &str,
    stem: &str,
    rows: Vec<Row>,
    speech: &[(u64, u64)],
    pieces: usize,
    quiet: bool,
) -> Result<Vec<Row>, Box<dyn Error>> {
    let mut count = Pieces::new(pieces);
    let mut out = Vec::with_capacity(rows.len());
    for (start, end, text) in rows {
        let cuts = cuts(start, end, speech);
        if cuts.is_empty() {
            out.push((start, end, text));
            continue;
        }
        match pieces_of(model, wav16, stem, (start, end), &cuts, quiet, &mut count) {
            Ok(Some(rows)) => out.extend(rows),
            Ok(None) => out.push((start, end, text)),
            Err(e) => {
                crate::tools::check()?;
                eprintln!("Kept a long row whole in {wav16}: {e}");
                out.push((start, end, text));
            }
        }
    }
    Ok(out)
}

/// How many whisper runs `split_joined` will make: each row with long silences, once per piece.
fn split_pieces(rows: &[Row], speech: &[(u64, u64)]) -> usize {
    rows.iter()
        .map(|(start, end, _)| cuts(*start, *end, speech).len())
        .filter(|cuts| *cuts > 0)
        .map(|cuts| cuts + 1)
        .sum()
}

/// Pieces done of a pass's total, for the steps page. A piece that comes back empty ends its row
/// early; the count then jumps past that row's other pieces.
struct Pieces {
    done: u64,
    total: u64,
}
impl Pieces {
    fn new(total: usize) -> Self {
        Self {
            done: 0,
            total: total as u64,
        }
    }
    fn done(&mut self, n: usize) {
        self.done += n as u64;
        progress::step(self.done, self.total);
    }
}

/// Whisper decodes a whole track in the one language it hears in the first 30 s, and decoding in
/// the wrong language translates: a call that opened with a Vietnamese song had its English
/// goodbyes written in Vietnamese ("Cảm ơn David" for "Thanks, David", 2026-10-23). When most of
/// the text is in another language than the track's (only vi, ja and zh can be told from the
/// text), each run of lines written in the track's language is transcribed again on its own with
/// the language detected afresh. The new lines replace the run only if they are mostly in another
/// language, so speech that really was in the track's language (the song) stays as it was.
/// Optional like `split_joined`: a failed piece keeps its lines. Cancel still stops the job.
/// one whisper run per run of lines; a call that switches language every line reruns
/// many short pieces.
fn relanguage(
    model: &str,
    wav16: &str,
    stem: &str,
    rows: Vec<Row>,
    spans: &[(usize, usize)],
    track: Option<&str>,
    quiet: bool,
) -> Result<Vec<Row>, Box<dyn Error>> {
    let Some(track) = track else {
        return Ok(rows);
    };
    let mut count = Pieces::new(spans.len());
    let mut out = Vec::with_capacity(rows.len());
    let mut next = 0;
    for &(first, last) in spans {
        out.extend_from_slice(&rows[next..first]);
        next = last + 1;
        let run = &rows[first..=last];
        // Up to the next line (or 1 s past the last): the run's last row can end mid-word.
        let end = rows
            .get(last + 1)
            .map_or(run[run.len() - 1].1 + 1_000, |next| next.0);
        match pieces_of(model, wav16, stem, (run[0].0, end), &[], quiet, &mut count) {
            Ok(Some(again)) if !mostly_in(&again, track) => out.extend(again),
            Ok(_) => out.extend_from_slice(run),
            Err(e) => {
                crate::tools::check()?;
                eprintln!("Kept lines in the track's language in {wav16}: {e}");
                out.extend_from_slice(run);
            }
        }
    }
    out.extend_from_slice(&rows[next..]);
    Ok(out)
}

/// The runs of lines (first and last index) to transcribe again: those written in the track's
/// language, when that is vi, ja or zh and most of the text is in another language. Else none.
fn rerun_spans(rows: &[Row], track: &str) -> Vec<(usize, usize)> {
    if !matches!(track, "vi" | "ja" | "zh") || rows.is_empty() || mostly_in(rows, track) {
        return vec![];
    }
    let mut spans: Vec<(usize, usize)> = vec![];
    for (i, (_, _, text)) in rows.iter().enumerate() {
        if row_language(text, Some(track)) != track {
            continue;
        }
        match spans.last_mut() {
            Some((_, last)) if *last + 1 == i => *last = i,
            _ => spans.push((i, i)),
        }
    }
    spans
}

/// True when more than half the text is written in `track`'s language.
fn mostly_in(rows: &[Row], track: &str) -> bool {
    let (ours, all) = rows.iter().fold((0, 0), |(ours, all), (_, _, text)| {
        let n = text.chars().count();
        let ours_now = if row_language(text, Some(track)) == track {
            n
        } else {
            0
        };
        (ours + ours_now, all + n)
    });
    ours * 2 > all
}

/// The middle of every silence of `JOINED_GAP_MS` or more between the speech heard in
/// `start..end`.
fn cuts(start: u64, end: u64, speech: &[(u64, u64)]) -> Vec<u64> {
    let mut heard: Vec<_> = speech
        .iter()
        .filter(|(s, e)| *e > start && *s < end)
        .copied()
        .collect();
    heard.sort_unstable();
    let mut cuts = vec![];
    let mut last = None::<u64>;
    for (s, e) in heard {
        if let Some(last) = last.filter(|last| s.saturating_sub(*last) >= JOINED_GAP_MS) {
            let middle = (last + s) / 2;
            if middle > start && middle < end {
                cuts.push(middle);
            }
        }
        last = Some(last.map_or(e, |last| last.max(e)));
    }
    cuts
}

/// Whisper on each piece of `start..end` between `cuts`, times put back on the track. `None`
/// when a piece comes back empty, so no words are lost.
/// one whisper-cli per piece (model loaded each time); rare enough not to matter.
fn pieces_of(
    model: &str,
    wav16: &str,
    stem: &str,
    (start, end): (u64, u64),
    cuts: &[u64],
    quiet: bool,
    count: &mut Pieces,
) -> Result<Option<Vec<Row>>, Box<dyn Error>> {
    let bounds: Vec<u64> = std::iter::once(start)
        .chain(cuts.iter().copied())
        .chain(std::iter::once(end))
        .collect();
    let all = bounds.len() - 1;
    let mut out = vec![];
    for (i, piece) in bounds.windows(2).enumerate() {
        let piece_stem = format!("{stem}.piece{i}");
        let piece_wav = format!("{piece_stem}.16k.wav");
        let rows = slice::write(Path::new(wav16), Path::new(&piece_wav), piece[0], piece[1])
            .and_then(|()| whisper(model, &piece_wav, &piece_stem, quiet, false));
        let _ = fs::remove_file(&piece_wav);
        let _ = fs::remove_file(format!("{piece_stem}.csv"));
        let _ = fs::remove_file(format!("{piece_stem}.json"));
        let rows = match rows {
            Ok(rows) => rows,
            Err(e) => {
                count.done(all - i);
                return Err(e);
            }
        };
        count.done(1);
        if rows.is_empty() {
            count.done(all - i - 1);
            return Ok(None);
        }
        out.extend(
            rows.into_iter()
                .map(|(s, e, text)| (s + piece[0], e.min(piece[1] - piece[0]) + piece[0], text)),
        );
    }
    Ok(Some(out))
}

/// When the track also holds a quiet voice (a video playing near the mic), VAD counts it as
/// speech and whisper starts the next row where that voice began, not where the words did: "Hi
/// Rachel" said at 1:19 was stamped 0:55 (2026-10-22). Neither VAD settings nor a second pass
/// fixed it. A row far longer than its words starts instead at its first 100 ms within 12 dB of
/// its loudest; a row of normal length keeps whisper's start.
/// loudness only; a background voice as loud as the speaker still leaves the early start.
fn heard_from(wav16: &Path, start: u64, end: u64, text: &str) -> u64 {
    let words = text
        .split_whitespace()
        .count()
        .max(text.chars().count() / 6) as u64;
    if end.saturating_sub(start) <= 2_000 + 700 * words {
        return start;
    }
    slice::levels(wav16, start, end).map_or(start, |levels| {
        start + onset(&levels) as u64 * slice::FRAME_MS
    })
}

/// The first frame within 12 dB (a quarter of the level) of the loudest, less one for the attack.
fn onset(levels: &[f32]) -> usize {
    let peak = levels.iter().copied().fold(0.0, f32::max);
    let first = levels.iter().position(|&l| l >= peak / 4.0).unwrap_or(0);
    first.saturating_sub(1)
}

/// One speaker per whisper row: the diarizer's voices when it hears two or more, else the
/// track's own label. Diarizing is optional, so a failure is reported and the track keeps its
/// label instead of losing the meeting.
fn voices(src: &str, wav16: &Path, track: Speaker, rows: &[(u64, u64, String)]) -> Vec<Speaker> {
    let spans: Vec<(u64, u64)> = rows.iter().map(|(s, e, _)| (*s, *e)).collect();
    let turns = match diarize::turns(wav16) {
        Ok(Some(turns)) => turns,
        Ok(None) => return vec![track; rows.len()],
        Err(e) => {
            eprintln!("Speaker labels skipped for {src}: {e}");
            return vec![track; rows.len()];
        }
    };
    match diarize::assign(&turns, &spans) {
        Some((ids, count)) if count >= 2 => {
            println!("Found {count} speakers in {src}");
            ids.into_iter().map(|id| track.voice(id)).collect()
        }
        _ => vec![track; rows.len()],
    }
}

/// Orders segments by start time and renders one `[mm:ss] Me: text` line each. The sort is
/// stable, so segments starting together keep the order their tracks were given in.
fn merge(mut segments: Vec<Segment>) -> String {
    // Unknown single-file imports are not evidence of system-audio echo.
    let indices: Vec<_> = segments
        .iter()
        .enumerate()
        .filter(|(_, s)| s.microphone || s.system)
        .map(|(i, _)| i)
        .collect();
    let rows: Vec<_> = indices
        .iter()
        .map(|&i| {
            let s = &segments[i];
            (s.start_ms, s.end_ms, s.text.as_str(), s.microphone)
        })
        .collect();
    let duplicates = crate::echo::duplicates(&rows);
    let remove: std::collections::HashSet<_> = indices
        .into_iter()
        .zip(duplicates)
        .filter_map(|(i, duplicate)| duplicate.then_some(i))
        .collect();
    let mut index = 0;
    segments.retain(|_| {
        let keep = !remove.contains(&index);
        index += 1;
        keep
    });
    segments.sort_by_key(|s| s.start_ms);
    segments
        .iter()
        .map(|s| {
            format!(
                "[{:02}:{:02}] {}{}\n",
                s.start_ms / 60_000,
                s.start_ms / 1000 % 60,
                s.speaker.label(),
                s.text
            )
        })
        .collect()
}

/// Rows shaped like a YouTube outro (`echo::credit`) that are mostly not speech. whisper writes
/// the outro over music: Silero heard speech in 9 % and 21 % of the two rows on 2026-10-25,
/// while someone reading it aloud is speech nearly all through, so their line stays. Without
/// VAD, or if the check fails, every outro row goes: an invented line is the likelier case.
fn invented_credits(rows: &[Row], speech: Option<&[(u64, u64)]>) -> Vec<bool> {
    let credits: Vec<bool> = rows.iter().map(|row| crate::echo::credit(&row.2)).collect();
    if !credits.contains(&true) {
        return credits;
    }
    let Some(speech) = speech else {
        return credits;
    };
    rows.iter()
        .zip(credits)
        .map(|((start, end, _), credit)| credit && mostly_silent(*start, *end, speech))
        .collect()
}

/// Speech (ms intervals) covers less than half of `start..end`.
fn mostly_silent(start: u64, end: u64, speech: &[(u64, u64)]) -> bool {
    let heard: u64 = speech
        .iter()
        .map(|&(s, e)| e.min(end).saturating_sub(s.max(start)))
        .sum();
    heard * 2 < end.saturating_sub(start)
}

/// whisper-cli `-ocsv` rows: `start_ms,end_ms,"text"` (header row skipped).
fn parse_csv(csv: &str) -> Vec<(u64, u64, String)> {
    csv.lines()
        .filter_map(|line| {
            let mut cols = line.splitn(3, ',');
            let start = cols.next()?.parse().ok()?;
            let end = cols.next()?.parse().ok()?;
            let field = cols.next()?.trim();
            let text = field
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .unwrap_or(field)
                .replace("\"\"", "\"");
            (!text.trim().is_empty()).then(|| (start, end, text.trim().to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_outro_over_music_goes_but_one_read_aloud_stays() {
        // 2026-10-25, Silero on the system track around the 02:59 outro row.
        let speech = [(150_000, 163_000), (181_350, 181_790), (187_070, 189_050)];
        assert!(mostly_silent(179_000, 184_000, &speech));
        assert!(mostly_silent(184_000, 197_000, &speech));
        assert!(!mostly_silent(150_500, 155_300, &speech));
        assert!(!mostly_silent(5_000, 5_000, &speech));
    }
    #[test]
    fn a_row_is_cut_in_each_long_silence_inside_it() {
        // 2026-10-23, Silero on the mic: speech until 30.3 s, typing only, speech from 124.87 s;
        // whisper's row ran 25.72-129.05 s.
        let speech = [
            (20_870, 25_410),
            (25_860, 27_230),
            (28_740, 30_300),
            (124_870, 125_730),
        ];
        assert_eq!(cuts(25_720, 129_050, &speech), vec![77_585]);
        // Short pauses and a silence outside the row don't cut it.
        assert_eq!(cuts(20_000, 30_500, &speech), Vec::<u64>::new());
        assert_eq!(cuts(124_000, 126_000, &speech), Vec::<u64>::new());
        // Overlapping intervals in any order; a silence counts from the latest end before it.
        assert_eq!(
            cuts(0, 30_000, &[(20_000, 21_000), (0, 9_000), (1_000, 3_000)]),
            vec![14_500]
        );
        assert_eq!(cuts(0, 10_000, &[]), Vec::<u64>::new());
    }

    #[test]
    fn a_long_row_starts_where_the_speaker_got_loud() {
        assert_eq!(onset(&[0.02, 0.03, 0.02, 0.3, 0.4, 0.1]), 2);
        assert_eq!(onset(&[0.4, 0.02]), 0);
        assert_eq!(onset(&[]), 0);
        let root = std::env::temp_dir().join(format!("motetto-onset-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let wav = root.join("mic.16k.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::create(&wav, spec).unwrap();
        // A quiet voice for 25 s (a video near the mic), then the speaker for 3 s.
        for i in 0..28 * 16_000 {
            let level = if i < 25 * 16_000 { 0.02 } else { 0.3 };
            writer
                .write_sample(if i % 2 == 0 { level } else { -level })
                .unwrap();
        }
        writer.finalize().unwrap();
        assert_eq!(
            heard_from(&wav, 1_000, 28_000, "Hi Rachel, good morning."),
            24_900
        );
        // A row as long as its words keeps whisper's start, quiet lead or not.
        assert_eq!(
            heard_from(&wav, 24_000, 28_000, "Hi Rachel, good morning."),
            24_000
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn merge_filters_mic_echo_of_diarized_system_but_not_unknown_imports() {
        let mic = Segment {
            start_ms: 100,
            end_ms: 1200,
            microphone: true,
            system: false,
            speaker: Speaker::Me,
            text: "One speaker in the video".into(),
        };
        let system = Segment {
            start_ms: 0,
            end_ms: 1000,
            microphone: false,
            system: true,
            speaker: Speaker::Voice(2),
            text: "One speaker in the video".into(),
        };
        assert_eq!(
            merge(vec![mic.clone(), system.clone()]),
            "[00:00] Speaker 2: One speaker in the video\n"
        );
        let unknown = Segment {
            system: false,
            speaker: Speaker::Unknown,
            ..system
        };
        assert!(merge(vec![mic, unknown]).contains("Me: One speaker"));
    }

    #[test]
    fn each_line_names_its_own_language() {
        // 2026-10-23: a Vietnamese song, then an English call, on a track whisper called "vi".
        assert_eq!(
            row_language("Em viết thiệp hồng tên anh thì đúng", Some("vi")),
            "vi"
        );
        assert_eq!(row_language("Hi Rachel, good morning", Some("vi")), "en");
        assert_eq!(row_language("明日までに資料を送ります", Some("en")), "ja");
        assert_eq!(row_language("Bonjour à tous", Some("fr")), "fr");
        assert_eq!(row_language("Okay", None), "en");
        let heard = vec![("vi".into(), 300), ("en".into(), 2700), ("ja".into(), 40)];
        assert_eq!(languages(heard).as_deref(), Some("en,vi"));
        assert_eq!(languages(vec![("vi".into(), 900)]).as_deref(), Some("vi"));
        assert_eq!(languages(vec![("en".into(), 0)]), None);
        assert_eq!(languages(vec![]), None);
    }

    #[test]
    fn lines_in_a_wrong_track_language_are_picked_for_another_pass() {
        // 2026-10-23: whisper heard "vi" (a song opened the call); the call was in English and its
        // goodbyes came out translated. Both Vietnamese runs are picked; the pass keeps the song.
        let row = |text: &str| (0, 0, text.to_string());
        let rows = vec![
            row("Hello, can you hear me?"),
            row("Muốn mình trong đêm ngồi đêm sao trời"),
            row("Và một nỗi nhớ về trong gì vang"),
            row("Hi Rachel, good morning. Can you hear me and see my screen clearly?"),
            row("It's like if anything urgent pops up before tomorrow, have a"),
            row("Hẹn gặp lại các bạn trong những video tiếp theo."),
            row("Cảm ơn David, YouTube."),
        ];
        assert_eq!(rerun_spans(&rows, "vi"), vec![(1, 2), (5, 6)]);
        // A Vietnamese call, an English track or no words: nothing to redo.
        assert_eq!(rerun_spans(&rows[1..3], "vi"), vec![]);
        assert_eq!(rerun_spans(&rows, "en"), vec![]);
        assert_eq!(rerun_spans(&[], "vi"), vec![]);
        assert!(mostly_in(&rows[5..], "vi") && !mostly_in(&rows, "vi"));
    }

    #[test]
    fn detected_reads_the_language_whisper_heard() {
        assert_eq!(
            detected(r#"{"params":{"language":"auto"},"result":{"language":"vi"}}"#).as_deref(),
            Some("vi")
        );
        assert_eq!(detected(r#"{"result":{"language":"auto"}}"#), None);
        assert_eq!(detected(r#"{"result":{}}"#), None);
        assert_eq!(detected("not json"), None);
    }

    #[test]
    fn parses_whisper_csv() {
        let csv = "start,end,text\n0,2000,\" Hi, all.\"\n2000,4000,\" He said \"\"go\"\".\"\n4000,5000,\" \"\n";
        assert_eq!(
            parse_csv(csv),
            [
                (0, 2000, "Hi, all.".into()),
                (2000, 4000, "He said \"go\".".into())
            ]
        );
    }

    #[test]
    fn parses_quotes_at_field_edges() {
        assert_eq!(
            parse_csv("0,1000,\" Say \"\"go\"\"\"\n1000,2000,\"\"\"yes\"\"\"\n"),
            [
                (0, 1000, "Say \"go\"".into()),
                (1000, 2000, "\"yes\"".into())
            ]
        );
    }

    #[test]
    fn speaker_comes_from_track_name() {
        assert_eq!(Speaker::from_path("call.mic.wav"), Speaker::Me);
        assert_eq!(Speaker::from_path("call.system.wav"), Speaker::Them);
        assert_eq!(Speaker::from_path("call.m4a"), Speaker::Unknown);
    }

    #[test]
    fn voices_number_from_two_on_a_call_and_from_one_alone() {
        assert_eq!(Speaker::Them.voice(0), Speaker::Voice(2));
        assert_eq!(Speaker::Unknown.voice(0), Speaker::Voice(1));
        assert_eq!(Speaker::Voice(3).label(), "Speaker 3: ");
    }

    #[test]
    fn merge_orders_by_start_time_across_tracks() {
        let seg = |start_ms, speaker, text: &str| Segment {
            start_ms,
            end_ms: start_ms + 1000,
            microphone: speaker == Speaker::Me,
            system: speaker == Speaker::Them,
            speaker,
            text: text.into(),
        };
        let merged = merge(vec![
            seg(3_170, Speaker::Me, "I can take it."),
            seg(65_000, Speaker::Me, "Done."),
            seg(0, Speaker::Them, "Who can take the bug?"),
            seg(3_170, Speaker::Them, "Same moment."),
            seg(0, Speaker::Unknown, "No label."),
            seg(3_170, Speaker::Voice(2), "Another voice."),
        ]);
        assert_eq!(
            merged,
            "[00:00] Them: Who can take the bug?\n\
             [00:00] No label.\n\
             [00:03] Me: I can take it.\n\
             [00:03] Them: Same moment.\n\
             [00:03] Speaker 2: Another voice.\n\
             [01:05] Me: Done.\n"
        );
    }
}
