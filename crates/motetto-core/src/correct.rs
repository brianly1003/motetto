//! Correct stage (optional): a local model fixes speech-recognition errors, in any language.

use std::error::Error;

use crate::summarize::ask;

const PROMPT: &str = "You fix speech-recognition errors in a meeting transcript. \
First work out the language of each line; a line may mix languages, such as English names inside \
another language. Correct each line in its own language: words misheard as similar-sounding words \
that make no sense in context, spelling, accents and tone marks, capitalised names, punctuation. \
Never translate, summarize, merge, split, add or drop lines. Keep each line's \"[mm:ss] Speaker: \" \
prefix exactly as given. If a word is unclear, leave it. \
Output only the corrected lines, one per input line.\n\nTRANSCRIPT:\n";

// fixed chunk size; size it from the model's context window if long lines overflow it.
const CHUNK_LINES: usize = 40;

/// Corrects the transcript with `llm`, chunk by chunk. A chunk whose answer changes the line
/// count, any `[mm:ss] Speaker:` prefix or most of the words is kept as transcribed, so a weak
/// model can't relabel, drop, invent, translate or rewrite lines.
pub fn transcript(llm: &str, transcript: &str) -> Result<String, Box<dyn Error>> {
    println!("Correcting the transcript with {llm}...");
    let lines: Vec<&str> = transcript.lines().collect();
    let mut fixed = String::new();
    for (i, chunk) in lines.chunks(CHUNK_LINES).enumerate() {
        let answer = ask(llm, &format!("{PROMPT}{}\n", chunk.join("\n")))?;
        match accept(chunk, &answer) {
            Some(lines) => fixed.extend(lines.iter().map(|l| format!("{l}\n"))),
            None => {
                eprintln!(
                    "correct: chunk {} relabelled, dropped or rewrote lines; kept as transcribed",
                    i + 1
                );
                fixed.extend(chunk.iter().map(|l| format!("{l}\n")));
            }
        }
    }
    Ok(fixed)
}

/// The model's lines, if they match the original one for one with the same prefixes and keep
/// most of its words: a correction changes a few words, a translation or rewrite changes most.
fn accept<'a>(original: &[&str], answer: &'a str) -> Option<Vec<&'a str>> {
    let lines: Vec<&str> = answer
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    // word overlap as the any-language "same text" test; per-line diff if it misjudges.
    (lines.len() == original.len()
        && original
            .iter()
            .zip(&lines)
            .all(|(o, l)| l.starts_with(prefix(o)))
        && kept_words(original, &lines) >= 0.5)
        .then_some(lines)
}

/// Share of the answer's words that already appear in the original (case and punctuation aside).
/// Prefixes are left out (they always match), and Chinese, Japanese and Korean characters count
/// one by one, since those languages don't put spaces between words.
pub(crate) fn kept_words(original: &[&str], answer: &[&str]) -> f64 {
    let words = |lines: &[&str]| -> Vec<String> {
        lines
            .iter()
            .flat_map(|l| l[prefix(l).len()..].split_whitespace())
            .flat_map(|w| {
                if w.chars().any(is_cjk) {
                    w.chars()
                        .filter(|c| c.is_alphanumeric())
                        .map(String::from)
                        .collect()
                } else {
                    vec![
                        w.trim_matches(|c: char| !c.is_alphanumeric())
                            .to_lowercase(),
                    ]
                }
            })
            .filter(|w| !w.is_empty())
            .collect()
    };
    let before: std::collections::HashSet<String> = words(original).into_iter().collect();
    let after = words(answer);
    let kept = after.iter().filter(|w| before.contains(*w)).count();
    kept as f64 / after.len().max(1) as f64
}

/// Han, kana and Hangul syllables: scripts written without spaces between words.
fn is_cjk(c: char) -> bool {
    matches!(c, '\u{3040}'..='\u{30FF}' | '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{AC00}'..='\u{D7AF}')
}

/// `[mm:ss] Me: `, `[mm:ss] Speaker 2: ` (or `[mm:ss] ` for unlabelled lines): the part a
/// correction must not touch.
fn prefix(line: &str) -> &str {
    let Some(end) = line.find("] ").map(|i| i + 2) else {
        return "";
    };
    let rest = &line[end..];
    let label = ["Me: ", "Them: "]
        .iter()
        .find(|l| rest.starts_with(*l))
        .map(|l| l.len())
        .or_else(|| {
            let n = rest.strip_prefix("Speaker ")?;
            let digits = n.len() - n.trim_start_matches(|c: char| c.is_ascii_digit()).len();
            (digits > 0 && n[digits..].starts_with(": ")).then_some("Speaker ".len() + digits + 2)
        })
        .unwrap_or(0);
    &line[..end + label]
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGINAL: [&str; 2] = ["[00:04] Me: Trúc hai em", "[00:07] Them: sự nghiệm"];

    #[test]
    fn accept_takes_corrected_lines() {
        let answer = "[00:04] Me: Chúc hai em.  \n\n[00:07] Them: Sự nghiệp.\n";
        assert_eq!(
            accept(&ORIGINAL, answer),
            Some(vec!["[00:04] Me: Chúc hai em.", "[00:07] Them: Sự nghiệp."])
        );
    }

    #[test]
    fn accept_rejects_relabelled_speaker() {
        let answer = "[00:04] Them: Chúc hai em.\n[00:07] Them: Sự nghiệp.";
        assert_eq!(accept(&ORIGINAL, answer), None);
    }

    #[test]
    fn accept_rejects_dropped_or_added_lines() {
        assert_eq!(
            accept(&ORIGINAL, "[00:04] Me: Chúc hai em, sự nghiệp."),
            None
        );
        assert_eq!(
            accept(
                &ORIGINAL,
                "Here is the fix:\n[00:04] Me: a\n[00:07] Them: b"
            ),
            None
        );
    }

    #[test]
    fn accept_takes_chinese_fix_without_spaces() {
        let original = ["[00:02] Them: 我们下个星期五发布第二般。"];
        let answer = "[00:02] Them: 我们下个星期五发布第二版。";
        assert_eq!(accept(&original, answer), Some(vec![answer]));
    }

    #[test]
    fn accept_rejects_chinese_rewrite() {
        let original = ["[00:02] Them: 我们下个星期五发布第二般。"];
        let answer = "[00:02] Them: 会议很成功，大家都很满意。";
        assert_eq!(accept(&original, answer), None);
    }

    #[test]
    fn accept_takes_japanese_fix_and_rejects_its_translation() {
        let original = [
            "[00:05] Me: 来週の金曜日にバージョン2をリリースします。木村さんがログインのバグを直死ます。",
        ];
        let fixed = "[00:05] Me: 来週の金曜日にバージョン2をリリースします。木村さんがログインのバグを直します。";
        assert_eq!(accept(&original, fixed), Some(vec![fixed]));
        let english = "[00:05] Me: We release version 2 next Friday. Kimura fixes the login bug.";
        assert_eq!(accept(&original, english), None);
    }

    #[test]
    fn accept_rejects_translation() {
        let original =
            ["[00:19] Me: Thôi thì tiếc nuối cũng có rất nhiều, chúc hai em thành công."];
        let answer = "[00:19] Me: Well, there's a lot of regret, wishing you both success.";
        assert_eq!(accept(&original, answer), None);
    }

    #[test]
    fn prefix_covers_time_and_label() {
        assert_eq!(prefix("[01:02] Them: hi"), "[01:02] Them: ");
        assert_eq!(prefix("[01:02] hi"), "[01:02] ");
        assert_eq!(prefix("[01:02] Speaker 12: hi"), "[01:02] Speaker 12: ");
        assert_eq!(prefix("[01:02] Speaker said hi"), "[01:02] ");
    }

    #[test]
    fn accept_rejects_renumbered_speaker() {
        let original = ["[00:05] Speaker 2: I can take the login bug."];
        assert_eq!(
            accept(&original, "[00:05] Speaker 3: I can take the login bug."),
            None
        );
    }
}
