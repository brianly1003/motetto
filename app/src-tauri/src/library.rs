//! Local import folders; source media is copied before the CLI pipeline writes intermediates.

use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use ts_rs::TS;

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct MeetingSummary {
    pub id: String,
    pub title: String,
    #[ts(type = "number")]
    pub created_ms: u64,
    pub status: String,
    #[serde(default)]
    #[ts(type = "number[]")]
    pub marks_ms: Vec<u64>,
    #[serde(default)]
    #[ts(type = "number | null")]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    #[ts(type = "number | null")]
    pub notes_ms: Option<u64>,
    #[serde(default)]
    pub spellings: Vec<String>,
    /// Read-only for review: no note, transcript or title edits, no Write the notes again (SPEC 7.11).
    #[serde(default)]
    pub locked: bool,
    /// Who named the meeting (SPEC 7.6): the notes model replaces a placeholder or its own title,
    /// never one the user typed.
    #[serde(default)]
    pub title_source: TitleSource,
    /// ISO code of the language whisper heard (SPEC 6.4); None before it was recorded.
    #[serde(default)]
    pub language: Option<String>,
    /// ISO code the notes were written in when one was chosen (Settings or Write the notes
    /// again); None for the call's own language or meetings saved before it was kept.
    #[serde(default)]
    pub notes_language: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum TitleSource {
    #[default]
    Placeholder,
    Model,
    User,
}

impl MeetingSummary {
    /// The notes model's title, unless the user named the meeting.
    pub fn titled(&mut self, title: Option<String>) {
        if let Some(title) = title.filter(|_| self.title_source != TitleSource::User) {
            self.title = title;
            self.title_source = TitleSource::Model;
        }
    }
}

#[derive(Serialize, TS)]
pub struct Meeting {
    pub summary: MeetingSummary,
    pub notes_md: String,
    pub transcript: String,
    /// transcript.txt as heard, only when the user edited lines (else empty), for Revert (SPEC 7.11).
    pub transcript_heard: String,
}

/// A search result: the meeting, and the line the words were found in when it isn't only the title.
#[derive(Serialize, TS)]
pub struct SearchHit {
    pub meeting: MeetingSummary,
    pub snippet: Option<Snippet>,
}

#[derive(Serialize, TS)]
pub struct Snippet {
    pub source: SnippetSource,
    /// The matching line without its time, label or Markdown markers, cut around the match.
    pub text: String,
    /// The line's `[mm:ss]` (transcript and My notes lines).
    #[ts(type = "number | null")]
    pub at_ms: Option<u64>,
}

#[derive(Serialize, TS)]
pub enum SnippetSource {
    Notes,
    Transcript,
    MyNotes,
}

pub fn folder(root: &Path, id: &str) -> Result<PathBuf, String> {
    if !is_id(id) {
        return Err("Invalid meeting ID".into());
    }
    let path = root.join(id).canonicalize().map_err(|e| e.to_string())?;
    if path.parent() != Some(root.canonicalize().map_err(|e| e.to_string())?.as_path()) {
        return Err("Meeting is outside the notes folder".into());
    }
    Ok(path)
}

pub(crate) fn text(path: &Path) -> Result<String, String> {
    // The file's name in every error: a bare "No such file or directory" hid which one for a day.
    let name = path
        .file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy();
    let named = |e: std::io::Error| format!("{name}: {e}");
    if path.canonicalize().map_err(named)?.parent()
        != Some(
            path.parent()
                .ok_or("Missing parent folder")?
                .canonicalize()
                .map_err(|e| e.to_string())?
                .as_path(),
        )
    {
        return Err("Meeting file points outside its folder".into());
    }
    let mut result = String::new();
    fs::File::open(path)
        .map_err(named)?
        .take(8 * 1024 * 1024 + 1)
        .read_to_string(&mut result)
        .map_err(named)?;
    if result.len() > 8 * 1024 * 1024 {
        return Err("Meeting text is too large to display".into());
    }
    Ok(result)
}

/// Every meeting folder, newest first. Folders that aren't meetings (no numeric name) are skipped;
/// a meeting whose `meeting.json` can't be read is listed as NeedsAttention instead of hiding the
/// whole library.
pub fn list(root: &Path) -> Result<Vec<MeetingSummary>, String> {
    let mut meetings = Vec::new();
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let Ok(entry) = entry else { continue };
        let Some(id) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !entry.file_type().is_ok_and(|t| t.is_dir()) || !is_id(&id) {
            continue;
        }
        meetings.push(summary(root, &id).unwrap_or_else(|_| unreadable(&id)));
    }
    meetings.sort_by_key(|m| std::cmp::Reverse(m.created_ms));
    Ok(meetings)
}

/// Bytes of the files in every listed meeting folder, for the sidebar footer. `DirEntry`
/// metadata does not follow links, so a linked file is not counted.
pub fn bytes(root: &Path) -> Result<u64, String> {
    let mut total = 0;
    for meeting in list(root)? {
        let Ok(folder) = folder(root, &meeting.id) else {
            continue;
        };
        for entry in fs::read_dir(folder).into_iter().flatten().flatten() {
            if let Ok(meta) = entry.metadata()
                && meta.is_file()
            {
                total += meta.len();
            }
        }
    }
    Ok(total)
}

/// Search locally, including saved text, without loading every meeting into the webview. Each hit
/// carries the first line that matched, so the sidebar can show where.
pub fn search(root: &Path, query: &str) -> Result<Vec<SearchHit>, String> {
    let query = query.trim().to_lowercase();
    Ok(list(root)?
        .into_iter()
        .filter_map(|meeting| {
            let snippet = folder(root, &meeting.id).ok().and_then(|folder| {
                [
                    (SnippetSource::Notes, "notes.md"),
                    (SnippetSource::Transcript, crate::transcript_edits::EDITED),
                    (SnippetSource::Transcript, "transcript.txt"),
                    (SnippetSource::Transcript, "transcript.fixed.txt"),
                    (SnippetSource::MyNotes, "my-notes.md"),
                ]
                .into_iter()
                .find_map(|(source, name)| {
                    let text = text(&folder.join(name)).ok()?;
                    // The notes' folded-away transcript is searched through the transcript files.
                    let text = text.split("<details>").next().unwrap_or_default();
                    let line = text
                        .lines()
                        .find(|line| line.to_lowercase().contains(&query))?;
                    Some(snippet(source, line, &query))
                })
            });
            (snippet.is_some() || meeting.title.to_lowercase().contains(&query))
                .then_some(SearchHit { meeting, snippet })
        })
        .collect())
}

/// Characters kept before the match, and in the whole snippet; the sidebar clamps it to two lines.
const BEFORE: usize = 40;
const LONGEST: usize = 160;

/// `query` is lower case. Cuts count characters, not bytes, so Vietnamese or CJK text can't split
/// inside a character.
fn snippet(source: SnippetSource, line: &str, query: &str) -> Snippet {
    let mut at_ms = None;
    let mut line = line.trim();
    // "[mm:ss] Speaker: text" in transcripts, "[mm:ss] text" in My notes.
    if let Some((stamp, rest)) = line.strip_prefix('[').and_then(|l| l.split_once("] "))
        && let Some((m, s)) = stamp.split_once(':')
        && let (Ok(m), Ok(s)) = (m.parse::<u64>(), s.parse::<u64>())
    {
        at_ms = Some((m * 60 + s) * 1000);
        line = rest;
        if matches!(source, SnippetSource::Transcript)
            && let Some((label, text)) = line.split_once(": ")
            && (matches!(label, "Me" | "Them") || label.strip_prefix("Speaker ").is_some())
        {
            line = text;
        }
    }
    let line = line
        .trim_start_matches(['#', '-', '*', '>', ' '])
        .trim_start_matches("[ ] ")
        .trim_start_matches("[x] ")
        .trim_start_matches("[] ")
        .replace("**", "");
    // Folding can change length (final sigma), so a match found per line may be missed here;
    // the snippet then starts at the line's beginning.
    let at = line
        .char_indices()
        .find(|&(i, _)| {
            let mut folded = line[i..].chars().flat_map(char::to_lowercase);
            query.chars().all(|q| folded.next() == Some(q))
        })
        .map_or(0, |(i, _)| i);
    let mut from = line[..at]
        .char_indices()
        .rev()
        .nth(BEFORE - 1)
        .map_or(0, |(i, _)| i);
    if from > 0
        && let Some(space) = line[from..at].find(' ')
    {
        from += space + 1;
    }
    let to = line[from..]
        .char_indices()
        .nth(LONGEST)
        .map_or(line.len(), |(i, _)| from + i);
    let text = format!(
        "{}{}{}",
        if from > 0 { "…" } else { "" },
        &line[from..to],
        if to < line.len() { "…" } else { "" }
    );
    Snippet {
        source,
        text,
        at_ms,
    }
}

fn is_id(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|v| v.is_ascii_digit())
}

pub fn summary(root: &Path, id: &str) -> Result<MeetingSummary, String> {
    let dir = folder(root, id)?;
    let mut summary: MeetingSummary = serde_json::from_str(&text(&dir.join("meeting.json"))?)
        .map_err(|e| format!("Cannot read meeting {id}: {e}"))?;
    if summary.id != id {
        return Err("Meeting ID does not match its folder".into());
    }
    summary.duration_ms = summary
        .duration_ms
        .or_else(|| crate::meeting_media::duration(root, id).ok().flatten());
    Ok(summary)
}

/// A placeholder for a meeting folder whose details are missing or damaged. Imports name folders
/// by nanoseconds since 1970, so the name still gives the date.
fn unreadable(id: &str) -> MeetingSummary {
    MeetingSummary {
        id: id.into(),
        title: "Meeting that couldn't be read".into(),
        created_ms: id.parse::<u128>().map_or(0, |ns| (ns / 1_000_000) as u64),
        status: "NeedsAttention".into(),
        marks_ms: vec![],
        duration_ms: None,
        notes_ms: None,
        spellings: vec![],
        locked: false,
        title_source: Default::default(),
        language: None,
        notes_language: None,
    }
}

/// Meetings still Recording or Finishing when the app last stopped were cut off by a quit or crash.
/// A recording with saved parts becomes Interrupted, which Home offers to Recover (SPEC 6.1); an
/// import can't be resumed, so it becomes NeedsAttention and doesn't look busy forever. Call once
/// at launch, before any import runs.
pub fn recover_interrupted(root: &Path) -> Result<usize, String> {
    let mut recovered = 0;
    for mut meeting in list(root)? {
        if matches!(
            meeting.status.as_str(),
            "Finishing" | "Recording" | "Stopped"
        ) {
            let recording = folder(root, &meeting.id)?.join("parts.json").exists();
            meeting.status = if recording {
                "Interrupted"
            } else {
                "NeedsAttention"
            }
            .into();
            save(root, &meeting)?;
            recovered += 1;
        }
    }
    Ok(recovered)
}

pub fn read(root: &Path, id: &str) -> Result<Meeting, String> {
    let dir = folder(root, id)?;
    let summary = summary(root, id)?;
    let notes_md = if dir.join("notes.md").exists() {
        text(&dir.join("notes.md"))?
    } else {
        String::new()
    };
    let edited = dir.join(crate::transcript_edits::EDITED).exists();
    let transcript = if edited || dir.join("transcript.txt").exists() {
        crate::transcript_edits::current(&dir)?
    } else {
        String::new()
    };
    let transcript_heard = if edited {
        text(&dir.join("transcript.txt"))?
    } else {
        String::new()
    };
    Ok(Meeting {
        summary,
        notes_md,
        transcript,
        transcript_heard,
    })
}

pub use crate::bookmarks::save;

/// Meetings whose notes still have an unchecked action item, newest first, with their notes,
/// for the Library's Open action items. The UI parses the tasks as the meeting page does.
/// reads every notes.md on each call; an index when libraries reach thousands.
pub fn open_items(root: &Path) -> Result<Vec<(MeetingSummary, String)>, String> {
    let mut open = Vec::new();
    for meeting in list(root)? {
        let Ok(dir) = folder(root, &meeting.id) else {
            continue;
        };
        let Ok(notes) = text(&dir.join("notes.md")) else {
            continue;
        };
        let tasks = notes.split("<details>").next().unwrap_or_default();
        if tasks.lines().any(|line| {
            let line = line.trim_start();
            line.starts_with("- [ ] ") || line.starts_with("* [ ] ")
        }) {
            open.push((meeting, notes));
        }
    }
    Ok(open)
}

/// `None` when Cancel stopped it: the meeting is removed and the source file was never touched.
pub fn import(
    root: &Path,
    source: &Path,
    started: impl FnOnce(&str),
    stage: impl Fn(&str, motetto_core::events::Stage) + Sync,
) -> Result<Option<MeetingSummary>, String> {
    // Checked before a folder exists, so a wrong pick leaves nothing behind.
    let extension = source
        .extension()
        .and_then(|s| s.to_str())
        .filter(|s| s.bytes().all(|b| b.is_ascii_alphanumeric()))
        .ok_or("Choose an audio or video file with an extension")?
        .to_owned();
    let created_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis() as u64;
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos()
        .to_string();
    let mut summary = MeetingSummary {
        id,
        title: source
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Imported meeting")
            .chars()
            .take(120)
            .collect(),
        created_ms,
        status: "Finishing".into(),
        marks_ms: vec![],
        duration_ms: None,
        notes_ms: None,
        spellings: vec![],
        locked: false,
        title_source: Default::default(),
        language: None,
        notes_language: None,
    };
    let dir = root.join(&summary.id);
    fs::create_dir(&dir).map_err(|e| e.to_string())?;
    if let Err(e) = save(root, &summary) {
        // An empty folder would show up as an unreadable meeting.
        let _ = fs::remove_dir(&dir);
        return Err(format!("Couldn't create the meeting folder: {e}"));
    }
    started(&summary.id);
    let meeting_id = summary.id.clone();
    let report = |next| stage(&meeting_id, next);
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let copied = dir.join(format!("source.{extension}"));
        fs::copy(source, &copied)?;
        report(motetto_core::events::Stage::Transcribing { progress: None });
        let heard = motetto_core::transcribe::transcript_parts(
            &[(vec![copied.to_string_lossy().into_owned()], 0)],
            true,
            &|progress| {
                report(motetto_core::events::Stage::Transcribing {
                    progress: Some(progress),
                })
            },
        )?;
        summary.language = heard.language;
        let transcript = heard.text;
        fs::write(dir.join("transcript.txt"), &transcript)?;
        let notes_input = match motetto_core::config::var("FIX_MODEL").filter(|v| !v.is_empty()) {
            Some(model) => {
                let fixed = motetto_core::correct::transcript(&model, &transcript)?;
                fs::write(dir.join("transcript.fixed.txt"), &fixed)?;
                fixed
            }
            None => transcript,
        };
        let started = std::time::Instant::now();
        let spellings = used_spellings();
        report(motetto_core::events::Stage::Summarizing {
            part: 1,
            of: 1,
            progress: None,
        });
        let title = motetto_core::summarize::notes(
            &dir.join("notes").to_string_lossy(),
            &notes_input,
            "",
            summary.language.as_deref(),
            None,
            &report,
        )?;
        summary.titled(title);
        summary.notes_language = motetto_core::config::notes_language();
        summary.notes_ms = Some(started.elapsed().as_millis() as u64);
        summary.spellings = spellings;
        Ok(())
    })();
    if result.is_err() && motetto_core::tools::cancelled() {
        fs::remove_dir_all(&dir)
            .map_err(|e| format!("Import stopped, but its folder couldn't be removed: {e}"))?;
        return Ok(None);
    }
    summary.status = if result.is_ok() {
        "Ready"
    } else {
        "NeedsAttention"
    }
    .into();
    if result.is_ok() {
        report(motetto_core::events::Stage::Saving);
    }
    // The pipeline's error is the one people need; a failed status save is added, not swapped in.
    let saved = save(root, &summary);
    match (result, saved) {
        (Ok(()), Ok(())) => Ok(Some(summary)),
        (Ok(()), Err(e)) => Err(format!(
            "Notes for {} are written, but its status couldn't be saved: {e}",
            summary.title
        )),
        (Err(e), saved) => Err(format!(
            "Couldn't finish {}: {e}. Saved files remain in the notes folder.{}",
            summary.title,
            saved.err().map_or(String::new(), |s| format!(
                " (Its status couldn't be saved either: {s}.)"
            ))
        )),
    }
}

pub(crate) fn used_spellings() -> Vec<String> {
    motetto_core::config::terms()
        .map(|terms| terms.split(", ").map(str::to_owned).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bytes_sum_meeting_files_but_not_links() {
        let root = temp_root();
        fs::create_dir(root.join("123")).unwrap();
        fs::write(
            root.join("123/meeting.json"),
            r#"{"id":"123","title":"A","created_ms":1,"status":"Ready"}"#,
        )
        .unwrap();
        fs::write(root.join("123/part-0000.mic.wav"), vec![0u8; 1000]).unwrap();
        let json = fs::metadata(root.join("123/meeting.json")).unwrap().len();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc/hosts", root.join("123/link.wav")).unwrap();
        assert_eq!(bytes(&root).unwrap(), 1000 + json);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn open_items_lists_only_meetings_with_unchecked_tasks() {
        let root = temp_root();
        for (id, notes) in [
            ("1", "### Action items\n- [x] Done already\n"),
            (
                "2",
                "### Action items\n- [ ] You: Share the design [01:20]\n",
            ),
            ("3", "<details>\n- [ ] Inside the transcript\n</details>"),
        ] {
            fs::create_dir(root.join(id)).unwrap();
            fs::write(
                root.join(id).join("meeting.json"),
                format!(r#"{{"id":"{id}","title":"M{id}","created_ms":{id},"status":"Ready"}}"#),
            )
            .unwrap();
            fs::write(root.join(id).join("notes.md"), notes).unwrap();
        }
        let open = open_items(&root).unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].0.id, "2");
        assert!(open[0].1.contains("Share the design"));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn search_matches_notes_and_transcript_without_exposing_paths() {
        let root = temp_root();
        fs::create_dir(root.join("123")).unwrap();
        fs::write(
            root.join("123/meeting.json"),
            r#"{"id":"123","title":"Design review","created_ms":1,"status":"Ready"}"#,
        )
        .unwrap();
        fs::write(
            root.join("123/transcript.txt"),
            "[00:04] Me: Publish the roadmap",
        )
        .unwrap();
        fs::write(
            root.join("123/my-notes.md"),
            "[00:05] [] Follow up Thursday",
        )
        .unwrap();
        let hits = search(&root, " ROADMAP ").unwrap();
        let snippet = hits[0].snippet.as_ref().unwrap();
        assert!(matches!(snippet.source, SnippetSource::Transcript));
        assert_eq!(
            (snippet.text.as_str(), snippet.at_ms),
            ("Publish the roadmap", Some(4000))
        );
        let hits = search(&root, "Thursday").unwrap();
        let snippet = hits[0].snippet.as_ref().unwrap();
        assert!(matches!(snippet.source, SnippetSource::MyNotes));
        assert_eq!(snippet.text, "Follow up Thursday");
        let hits = search(&root, "design").unwrap();
        assert!(hits[0].snippet.is_none(), "a title-only match has no line");
        assert!(search(&root, "unrelated").unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn snippets_cut_on_characters_around_the_match() {
        let long = "Chúng ta đã thống nhất rằng nhóm thiết kế sẽ hoàn thiện bản mẫu trước thứ Sáu, \
                    sau đó gửi báo cáo cho khách hàng.";
        let cut = snippet(
            SnippetSource::Notes,
            &format!("- **{long}**"),
            "gửi báo cáo",
        );
        assert!(cut.text.starts_with('…') && !cut.text.ends_with('…'));
        assert!(cut.text.contains("gửi báo cáo cho khách hàng."));
        assert!(!cut.text.contains("**"));
        // The cut lands after a space, not inside a word.
        assert!(cut.text[3..].starts_with(|c: char| c.is_alphabetic()));
        let line = format!("[61:05] Speaker 2: {}", "ừ ".repeat(200));
        let cut = snippet(SnippetSource::Transcript, &line, "ừ");
        assert_eq!(cut.at_ms, Some(3_665_000));
        assert_eq!(cut.text.chars().count(), LONGEST + 1);
        let cut = snippet(SnippetSource::Notes, "- [ ] Send the ΣΟΦΟΣ deck", "σοφος");
        assert_eq!(cut.text, "Send the ΣΟΦΟΣ deck");
    }
    #[test]
    fn rejects_paths_outside_library() {
        let root = std::env::temp_dir();
        for id in ["", "..", "../secret", "/tmp", "123/456"] {
            assert!(folder(&root, id).is_err());
        }
    }

    // Cancel is one process-wide flag (motetto_core::tools), so imports run one at a time here too.
    static IMPORTS: std::sync::Mutex<()> = std::sync::Mutex::new(());
    fn temp_root() -> PathBuf {
        // Parallel tests can read the same clock tick; the counter keeps folders apart.
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let root = std::env::temp_dir().join(format!(
            "motetto-library-{}-{}",
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        root
    }
    #[test]
    fn saved_meetings_roundtrip_and_list_newest_first() {
        let root = temp_root();
        for id in ["1", "2"] {
            fs::create_dir(root.join(id)).unwrap();
            save(
                &root,
                &MeetingSummary {
                    id: id.into(),
                    title: format!("Meeting {id}"),
                    created_ms: id.parse().unwrap(),
                    status: "Ready".into(),
                    marks_ms: vec![],
                    duration_ms: None,
                    notes_ms: None,
                    spellings: vec![],
                    locked: false,
                    title_source: Default::default(),
                    language: None,
                    notes_language: None,
                },
            )
            .unwrap();
        }
        fs::write(root.join("2/notes.md"), "### Action items").unwrap();
        fs::write(root.join("2/transcript.txt"), "[00:00] Hello").unwrap();
        assert_eq!(list(&root).unwrap()[0].id, "2");
        let meeting = read(&root, "2").unwrap();
        assert_eq!(meeting.notes_md, "### Action items");
        assert_eq!(meeting.transcript, "[00:00] Hello");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn list_skips_other_folders_and_flags_unreadable_meetings() {
        let root = temp_root();
        fs::create_dir(root.join("Archive")).unwrap();
        fs::write(root.join("notes.txt"), "not a meeting").unwrap();
        fs::create_dir(root.join("1700000000000000000")).unwrap(); // no meeting.json
        fs::create_dir(root.join("2")).unwrap();
        fs::write(root.join("2/meeting.json"), "{ broken").unwrap();
        let meetings = list(&root).unwrap();
        assert_eq!(meetings.len(), 2);
        assert!(meetings.iter().all(|m| m.status == "NeedsAttention"));
        assert_eq!(meetings[0].created_ms, 1_700_000_000_000);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn interrupted_imports_need_attention_after_restart() {
        let root = temp_root();
        for (id, status) in [("1", "Finishing"), ("2", "Ready"), ("3", "Recording")] {
            fs::create_dir(root.join(id)).unwrap();
            let summary = MeetingSummary {
                id: id.into(),
                title: id.into(),
                created_ms: 1,
                status: status.into(),
                marks_ms: vec![],
                duration_ms: None,
                notes_ms: None,
                spellings: vec![],
                locked: false,
                title_source: Default::default(),
                language: None,
                notes_language: None,
            };
            save(&root, &summary).unwrap();
        }
        fs::write(root.join("3/parts.json"), "[]").unwrap();
        assert_eq!(recover_interrupted(&root).unwrap(), 2);
        assert_eq!(read(&root, "1").unwrap().summary.status, "NeedsAttention");
        assert_eq!(read(&root, "2").unwrap().summary.status, "Ready");
        // A recording (it has parts.json) is offered as Recover instead.
        assert_eq!(read(&root, "3").unwrap().summary.status, "Interrupted");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn import_without_extension_leaves_no_folder() {
        let _imports = IMPORTS.lock().unwrap_or_else(|e| e.into_inner());
        let root = temp_root();
        assert!(import(&root, Path::new("/tmp/recording"), |_| {}, |_, _| {}).is_err());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_import_keeps_its_error_and_needs_attention() {
        let _imports = IMPORTS.lock().unwrap_or_else(|e| e.into_inner());
        let root = temp_root();
        let missing = root.join("gone.m4a"); // copy fails: the pipeline error must surface
        // The meeting is announced once its folder exists, so the list can show it writing.
        let mut announced = None;
        let err = import(
            &root,
            &missing,
            |id| {
                announced = Some((id.to_owned(), list(&root).unwrap()[0].status.clone()));
            },
            |_, _| panic!("A failed copy must not announce transcription"),
        )
        .unwrap_err();
        assert!(err.starts_with("Couldn't finish gone:"), "{err}");
        let meetings = list(&root).unwrap();
        assert_eq!(meetings.len(), 1);
        assert_eq!(
            announced,
            Some((meetings[0].id.clone(), "Finishing".into()))
        );
        assert_eq!(meetings[0].status, "NeedsAttention");
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_external_symlinks() {
        let root = temp_root();
        let outside = temp_root();
        fs::write(outside.join("secret"), "private").unwrap();
        fs::create_dir(root.join("1")).unwrap();
        std::os::unix::fs::symlink(outside.join("secret"), root.join("1/notes.md")).unwrap();
        assert!(text(&root.join("1/notes.md")).is_err());
        std::os::unix::fs::symlink(&outside, root.join("2")).unwrap();
        assert!(folder(&root, "2").is_err());
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }
    #[test]
    fn cancelled_import_leaves_no_meeting_and_keeps_the_source() {
        let _imports = IMPORTS.lock().unwrap_or_else(|e| e.into_inner());
        let root = temp_root();
        let source = root.join("call.m4a");
        fs::write(&source, b"not really audio").unwrap();
        motetto_core::tools::cancel();
        let result = import(&root, &source, |_| {}, |_, _| {});
        motetto_core::tools::reset();
        assert!(matches!(result, Ok(None)), "{result:?}");
        assert!(list(&root).unwrap().is_empty());
        assert_eq!(fs::read(&source).unwrap(), b"not really audio");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "requires synthetic speech and installed local models"]
    fn real_import_preserves_source_and_writes_notes() {
        let _imports = IMPORTS.lock().unwrap_or_else(|e| e.into_inner());
        let source = PathBuf::from(std::env::var("MOTETTO_TEST_IMPORT").unwrap());
        let before = fs::read(&source).unwrap();
        let root = temp_root();
        let stages = std::sync::Mutex::new(Vec::new());
        let imported = import(
            &root,
            &source,
            |_| {},
            |id, stage| {
                stages.lock().unwrap().push((id.to_owned(), stage));
            },
        )
        .unwrap()
        .unwrap();
        let stages = stages.into_inner().unwrap();
        assert!(stages.iter().all(|(id, _)| id == &imported.id));
        assert!(matches!(
            stages.first().unwrap().1,
            motetto_core::events::Stage::Transcribing { .. }
        ));
        assert!(
            stages
                .iter()
                .any(|(_, stage)| matches!(stage, motetto_core::events::Stage::Summarizing { .. }))
        );
        assert!(matches!(
            stages.last().unwrap().1,
            motetto_core::events::Stage::Saving
        ));
        let meeting = read(&root, &imported.id).unwrap();
        assert_eq!(imported.status, "Ready");
        assert!(meeting.transcript.to_lowercase().contains("friday"));
        assert!(!meeting.notes_md.is_empty());
        assert_eq!(fs::read(&source).unwrap(), before);
        assert!(!source.with_extension("csv").exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn export_bindings() {
        let config = ts_rs::Config::default().with_out_dir("../../ui/lib/bindings");
        Meeting::export_all(&config).unwrap();
        SearchHit::export_all(&config).unwrap();
        motetto_core::events::Event::export_all(&config).unwrap();
        crate::catalog::Catalog::export_all(&config).unwrap();
    }
}
