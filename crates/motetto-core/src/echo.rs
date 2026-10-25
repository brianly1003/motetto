//! Text filters shared by the transcript and live captions: microphone text duplicated from speaker
//! playback (SPEC 6.2), whisper's loops, and the subtitle credits whisper invents.
use std::collections::HashSet;

fn words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
}

/// Row mask for one whisper run: a row is a loop when it is faster than anyone speaks (over 8
/// words a second) and at least 80 % of its words were already said in the previous 2 minutes.
/// A loop replays an earlier sentence squeezed into rows of 100 ms to 1 s; real repeats take real
/// time, so they stay.
/// CJK text has no spaces, so a sentence counts as one word and is never dropped.
pub fn loops(rows: &[(u64, u64, String)]) -> Vec<bool> {
    let mut dropped: Vec<bool> = Vec::with_capacity(rows.len());
    for (i, (start, end, text)) in rows.iter().enumerate() {
        let row: HashSet<String> = words(text).collect();
        let fast = words(text).count() as u64 * 125 > end.saturating_sub(*start);
        let looped = fast && !row.is_empty() && {
            // Words of the rows kept in the 2 minutes before this one.
            let heard: HashSet<String> = rows[..i]
                .iter()
                .zip(&dropped)
                .filter(|((s, _, _), gone)| !**gone && s.saturating_add(120_000) >= *start)
                .flat_map(|((_, _, t), _)| words(t))
                .collect();
            row.iter().filter(|w| heard.contains(*w)).count() * 5 >= row.len() * 4
        };
        dropped.push(looped);
    }
    dropped
}

/// Subtitle credits whisper learned from YouTube and writes over music, applause or silence:
/// "Hãy subscribe cho kênh Ghiền Mì Gõ..." came out twice over a song on 2026-10-25, live and
/// after the call, and "...kênh lalaschool..." on another clip. Matched by the outro's shape, so a
/// channel never seen before is caught too: subscribe + channel + video in one row, or the
/// Amara.org subtitle credit. "Like and subscribe" alone is left in: people say it. Live captions
/// drop every match; after the call `transcribe` keeps one the VAD heard spoken.
pub fn credit(text: &str) -> bool {
    // Spaces dropped: whisper writes both "La La School" and "lalaschool".
    let text: String = text.to_lowercase().split_whitespace().collect();
    let has = |any: &[&str]| any.iter().any(|word| text.contains(word));
    (has(&["subscribe", "đăngký", "đăngkí"]) && has(&["kênh", "channel"]) && has(&["video"]))
        || text.contains("amara.org")
}

/// Segment index mask: only microphone rows matching system audio within 1.5 s are removed.
pub fn duplicates(rows: &[(u64, u64, &str, bool)]) -> Vec<bool> {
    let tokens: Vec<HashSet<String>> = rows.iter().map(|row| words(row.2).collect()).collect();
    let mut system: Vec<_> = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| !row.3)
        .map(|(i, _)| i)
        .collect();
    system.sort_by_key(|&i| rows[i].0);
    rows.iter()
        .enumerate()
        .map(|(i, mic)| {
            if !mic.3 || tokens[i].is_empty() {
                return false;
            }
            let end = system.partition_point(|&j| rows[j].0 <= mic.1.saturating_add(1500));
            system[..end].iter().any(|&j| {
                if rows[j].1.saturating_add(1500) < mic.0 {
                    return false;
                }
                let common = tokens[i].intersection(&tokens[j]).count();
                let union = tokens[i].len() + tokens[j].len() - common;
                common * 5 >= union * 4
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drops_echo_but_keeps_distinct_speech_and_system_track() {
        let rows = [
            (0, 1000, "Hello, this is the video.", false),
            (200, 1300, "hello THIS is the video!", true),
            (300, 1400, "I disagree with the video", true),
            (2501, 3000, "Hello this is the video", true),
            (0, 1000, "", true),
        ];
        assert_eq!(duplicates(&rows), [false, true, false, false, false]);
    }
    #[test]
    fn threshold_and_tolerance_are_inclusive() {
        assert_eq!(
            duplicates(&[
                (0, 1000, "one two three four five", false),
                (2500, 3000, "one two three four", true),
                (0, 1000, "one two three", true)
            ]),
            [false, true, false]
        );
    }
    #[test]
    fn order_and_unicode_do_not_change_matching() {
        assert_eq!(
            duplicates(&[
                (100, 900, "Xin chào bạn", true),
                (0, 1000, "XIN CHÀO BẠN!", false)
            ]),
            [true, false]
        );
        assert_eq!(
            duplicates(&[(0, 1000, "same words", true), (0, 1000, "same words", true)]),
            [false, false]
        );
    }
    #[test]
    fn subtitle_credits_are_whisper_not_speech() {
        for outro in [
            "Hãy subscribe cho kênh Ghiền Mì Gõ Để không bỏ lỡ những video hấp dẫn",
            "Mọi người hãy subscribe cho kênh Ghiền Mì Gõ để nhận những video hấp dẫn",
            "Hãy subscribe cho kênh lalaschool Để không bỏ lỡ những video hấp dẫn",
            "Hãy đăng ký kênh Team X để xem thêm nhiều video mới",
            "Please subscribe to my channel for more videos",
            "Subtitles by the Amara.org community",
        ] {
            assert!(credit(outro), "{outro}");
        }
        for said in [
            "Please subscribe to the release notes channel",
            "Don't forget to like and subscribe!",
            "Mình cần thêm người đăng ký kênh bán hàng",
            "Video demo này ai làm vậy?",
            "Cảm ơn các bạn đã theo dõi",
        ] {
            assert!(!credit(said), "{said}");
        }
    }
    #[test]
    fn drops_whisper_loops_but_keeps_real_repeats() {
        let row = |s: u64, e: u64, t: &str| (s, e, t.to_string());
        let said = "Oh, that sounds amazing. But wait. Where would we go? We don't have much time \
                    to plan if we're leaving this Friday.";
        let looped = format!("Hmm. {said}");
        let rows = [
            row(18_980, 32_390, "Hey, Alex. How about you?"),
            row(64_900, 77_040, said),
            row(97_010, 97_110, &looped),
            row(97_010, 97_110, "How about you?"),
            row(100_470, 100_830, &looped),
            row(100_830, 104_000, "How about you?"),
            row(105_000, 105_100, "Brand new words entirely here now"),
            row(300_000, 300_100, said),
        ];
        let got = loops(&rows);
        assert_eq!(got, [false, false, true, true, true, false, false, false]);
    }
}
