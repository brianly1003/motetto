//! Transcribe again keeps what the user fixed (SPEC 7.11): the names and terms they corrected
//! bias whisper for that run, and their edits are put back on the new transcript.
use motetto_core::correct::prefix;

/// At most this many corrected spellings go to whisper: they share its ~224-token prompt with
/// `TERMS`, and small speech models re-segment around a long prompt (SPEC 6.5). Most meetings
/// have far fewer; a glossary ranking (roadmap item 12) would pick among more.
const MAX_HINTS: usize = 10;
/// A line heard the same way again keeps its edit when it starts within this much of the old one.
const SAME_LINE_MS: u64 = 2_000;

/// What the user changed in one meeting's transcript.
#[derive(Debug, Default, PartialEq)]
pub struct Edits {
    /// `heard → spelling` corrections that look like names or terms.
    fixes: Vec<(String, String)>,
    /// Edited lines: the heard line in full and the user's text (empty = removed).
    lines: Vec<(String, String)>,
}

impl Edits {
    /// Compares the heard transcript with the edited copy, line by line (same count, same
    /// prefixes, SPEC 6.4); a missing or mismatched copy teaches nothing.
    pub fn learn(heard: &str, edited: &str) -> Self {
        let (heard, edited): (Vec<_>, Vec<_>) =
            (heard.split('\n').collect(), edited.split('\n').collect());
        let mut out = Self::default();
        if heard.len() != edited.len() {
            return out;
        }
        for (old, new) in heard.into_iter().zip(edited) {
            let head = prefix(old);
            if head.is_empty() || old == new || !new.starts_with(head) {
                continue;
            }
            let (was, now) = (&old[head.len()..], &new[head.len()..]);
            if let Some(fix) = correction(was, now)
                && !out.fixes.contains(&fix)
            {
                out.fixes.push(fix);
            }
            out.lines.push((old.to_owned(), now.to_owned()));
        }
        out
    }

    /// The corrected spellings, for whisper's prompt.
    pub fn hints(&self) -> Vec<String> {
        let mut out: Vec<String> = vec![];
        for (_, spelling) in &self.fixes {
            if out.len() < MAX_HINTS && !out.contains(spelling) {
                out.push(spelling.clone());
            }
        }
        out
    }

    /// The new transcript with the edits put back, in the edited-copy form: same lines and
    /// prefixes, only text changed. A line heard as before (same speaker, same words, start
    /// within 2 s) takes the user's whole edit; every other line gets the name fixes.
    pub fn apply(&self, transcript: &str) -> String {
        let mut used = vec![false; self.lines.len()];
        transcript
            .split('\n')
            .map(|line| {
                let head = prefix(line);
                if head.is_empty() {
                    return line.to_owned();
                }
                let text = &line[head.len()..];
                let same = self
                    .lines
                    .iter()
                    .enumerate()
                    .position(|(i, (old, _))| !used[i] && same_line(old, head, text));
                match same {
                    Some(i) => {
                        used[i] = true;
                        format!("{head}{}", self.lines[i].1)
                    }
                    None => format!("{head}{}", self.fix(text)),
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn fix(&self, text: &str) -> String {
        self.fixes
            .iter()
            .fold(text.to_owned(), |text, (heard, spelling)| {
                replace_phrase(&text, heard, spelling)
            })
    }
}

/// `old` (a heard line in full) is the line `head` + `text`: same words and speaker, close start.
fn same_line(old: &str, head: &str, text: &str) -> bool {
    let old_head = prefix(old);
    old[old_head.len()..] == *text
        && label(old_head) == label(head)
        && match (millis(old_head), millis(head)) {
            (Some(a), Some(b)) => a.abs_diff(b) <= SAME_LINE_MS,
            _ => false,
        }
}

/// The speaker part of a prefix: `Them: ` in `[01:02] Them: `.
fn label(head: &str) -> &str {
    head.find("] ").map_or("", |i| &head[i + 2..])
}

/// `[mm:ss]` or `[h:mm:ss]` as milliseconds.
fn millis(head: &str) -> Option<u64> {
    let time = head.strip_prefix('[')?.split(']').next()?;
    time.split(':')
        .try_fold(0u64, |total, part| {
            Some(total * 60 + part.parse::<u64>().ok()?)
        })
        .map(|secs| secs * 1000)
}

/// The one phrase a line edit changed, when it looks like fixing a misheard name or term (the
/// Fix all rules, SPEC 7.11): words compared without edge punctuation, the common start and end
/// dropped, 1-4 plain words each side, close in spelling or sharing a consonant skeleton, and the
/// new phrase has a capital or a digit. Grammar fixes and rewrites give nothing.
fn correction(was: &str, now: &str) -> Option<(String, String)> {
    let words = |text: &str| -> Vec<String> {
        text.split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_owned())
            .filter(|w| !w.is_empty())
            .collect()
    };
    let (old, new) = (words(was), words(now));
    let lead = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let tail = old[lead..]
        .iter()
        .rev()
        .zip(new[lead..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let (old, new) = (&old[lead..old.len() - tail], &new[lead..new.len() - tail]);
    let plain = |w: &String| w.chars().all(|c| c.is_alphanumeric() || "'.-".contains(c));
    if !(1..=4).contains(&old.len())
        || !(1..=4).contains(&new.len())
        || !old.iter().chain(new).all(plain)
    {
        return None;
    }
    let (heard, spelling) = (old.join(" "), new.join(" "));
    let squash = |s: &str| -> Vec<char> {
        s.chars()
            .filter(|c| !c.is_whitespace())
            .flat_map(char::to_lowercase)
            .collect()
    };
    let (a, b) = (squash(&heard), squash(&spelling));
    let close = distance(&a, &b) * 10 <= a.len().max(b.len()) * 4;
    let termlike = spelling
        .chars()
        .any(|c| c.is_uppercase() || c.is_ascii_digit());
    (heard != spelling && termlike && (close || skeleton(&a) == skeleton(&b)))
        .then_some((heard, spelling))
}

/// Levenshtein distance between two char slices.
fn distance(a: &[char], b: &[char]) -> usize {
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut last = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let next = (last + usize::from(ca != cb))
                .min(row[j] + 1)
                .min(row[j + 1] + 1);
            last = row[j + 1];
            row[j + 1] = next;
        }
    }
    row[b.len()]
}

/// Consonants with doubles collapsed: `jenleeang` and `jianliang` are both `jnlng`.
fn skeleton(s: &[char]) -> Vec<char> {
    let mut out: Vec<char> = s
        .iter()
        .copied()
        .filter(|c| !"aeiouy".contains(*c))
        .collect();
    out.dedup();
    out
}

/// Every whole-phrase, case-insensitive match of `heard` in `text` replaced by `spelling`; a match
/// inside a longer word is left alone.
fn replace_phrase(text: &str, heard: &str, spelling: &str) -> String {
    let fold = |s: &str| -> Vec<(usize, char)> {
        s.char_indices()
            .flat_map(|(i, c)| c.to_lowercase().map(move |l| (i, l)))
            .collect()
    };
    let (hay, needle) = (fold(text), fold(heard));
    if needle.is_empty() {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let (mut copied, mut i) = (0, 0);
    while i + needle.len() <= hay.len() {
        let matches = hay[i..i + needle.len()]
            .iter()
            .zip(&needle)
            .all(|(a, b)| a.1 == b.1);
        let start = hay[i].0;
        let end = hay.get(i + needle.len()).map_or(text.len(), |c| c.0);
        let edge = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric());
        if matches && edge(text[..start].chars().next_back()) && edge(text[end..].chars().next()) {
            out.push_str(&text[copied..start]);
            out.push_str(spelling);
            copied = end;
            i += needle.len();
        } else {
            i += 1;
        }
    }
    out.push_str(&text[copied..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learns_name_fixes_and_skips_grammar_and_rewrites() {
        let fix = |a, b| correction(a, b);
        assert_eq!(
            fix("we use push beams here.", "we use Pusher Beams here."),
            Some(("push beams".into(), "Pusher Beams".into()))
        );
        assert_eq!(
            fix("ask Jen Lee Ang", "ask Jianliang"),
            Some(("Jen Lee Ang".into(), "Jianliang".into()))
        );
        assert_eq!(
            fix("the IGNB team", "the IGB team"),
            Some(("IGNB".into(), "IGB".into()))
        );
        assert_eq!(fix("their done", "there done"), None); // no capital: grammar
        assert_eq!(fix("the Cost here", "the Code here"), None); // not close: a rewrite
        assert_eq!(fix("a b c d e f", "x y z w v u"), None); // too long
        assert_eq!(fix("same", "same"), None);
    }

    #[test]
    fn learn_keeps_lines_and_fixes_and_hints() {
        let heard = "[00:01] Them: we use push beams\n[00:05] Me: ok\n[00:09] Them: their done";
        let edited = "[00:01] Them: we use Pusher Beams\n[00:05] Me: \n[00:09] Them: there done";
        let edits = Edits::learn(heard, edited);
        assert_eq!(edits.lines.len(), 3);
        assert_eq!(edits.hints(), ["Pusher Beams"]);
        // A different line count (a stale copy) teaches nothing.
        assert_eq!(Edits::learn(heard, "[00:01] Them: x"), Edits::default());
    }

    #[test]
    fn apply_keeps_same_lines_and_spreads_name_fixes() {
        let heard = "[00:01] Them: we use push beams\n[00:05] Me: um ok\n[00:09] Them: their done";
        let edited = "[00:01] Them: we use Pusher Beams\n[00:05] Me: \n[00:09] Them: there done";
        let edits = Edits::learn(heard, edited);
        // Heard again: one line now starts 1 s later, one is new, one moved 5 s (not the same).
        let again = "[00:02] Them: we use push beams\n[00:04] Them: Push Beams works\n\
                     [00:05] Me: um ok\n[00:14] Them: their done";
        assert_eq!(
            edits.apply(again),
            "[00:02] Them: we use Pusher Beams\n[00:04] Them: Pusher Beams works\n\
             [00:05] Me: \n[00:14] Them: their done"
        );
    }

    #[test]
    fn phrases_replace_whole_words_in_any_case_and_script() {
        assert_eq!(
            replace_phrase("pushbeams, push beams.", "push beams", "Pusher Beams"),
            "pushbeams, Pusher Beams."
        );
        assert_eq!(
            replace_phrase("Anh Kenvin nói", "kenvin", "Kevin"),
            "Anh Kevin nói"
        );
        assert_eq!(replace_phrase("Đúng rồi", "đúng", "Đúng"), "Đúng rồi");
        assert_eq!(millis("[1:02:03] Me: "), Some(3_723_000));
    }
}
