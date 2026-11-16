//! Cue splitting and subtitle formats; no model needed.

use animathio_core::{TICKS_PER_SECOND, Time};
use animathio_transcribe::{
    Cue, Format, Segment, SplitOptions, Word, cues::clean_text, format_cues, parse_cues,
    split_segments,
};
use proptest::prelude::*;

const MS: i64 = TICKS_PER_SECOND / 1000;

fn t(ms: i64) -> Time {
    Time(ms * MS)
}

fn segment(start: i64, end: i64, text: &str) -> Segment {
    Segment {
        start: t(start),
        end: t(end),
        text: text.to_owned(),
        words: Vec::new(),
    }
}

fn assert_legal(cues: &[Cue], options: &SplitOptions) {
    for pair in cues.windows(2) {
        assert!(
            pair[0].end.0 + options.min_gap.0 <= pair[1].start.0,
            "overlap or gap too small: {pair:?}"
        );
    }
    for cue in cues {
        assert!(cue.end.0 > cue.start.0, "empty interval {cue:?}");
        assert!(
            cue.end.0 - cue.start.0 <= options.max_duration.0,
            "too long {cue:?}"
        );
        let lines: Vec<_> = cue.text.split('\n').collect();
        assert!(lines.len() <= options.max_lines, "too many lines {cue:?}");
        for line in lines {
            assert!(
                line.chars().count() <= options.max_chars_per_line,
                "line too long {line:?}"
            );
        }
    }
}

#[test]
fn short_phrase_is_one_single_line_cue() {
    let cues = split_segments(
        &[segment(1000, 2500, "Hello there world")],
        &SplitOptions::default(),
    );
    assert_eq!(cues.len(), 1);
    assert_eq!(cues[0].text, "Hello there world");
    assert_eq!((cues[0].start, cues[0].end), (t(1000), t(2500)));
}

#[test]
fn long_phrase_wraps_into_two_balanced_lines() {
    let text = "The quick brown fox jumps over the lazy dog near the river bank";
    let options = SplitOptions::default();
    let cues = split_segments(&[segment(0, 4000, text)], &options);
    assert_eq!(cues.len(), 1);
    let lines: Vec<_> = cues[0].text.split('\n').collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].len().abs_diff(lines[1].len()) < 12, "{lines:?}");
    assert_legal(&cues, &options);
}

#[test]
fn very_long_text_splits_into_several_cues_with_proportional_times() {
    let words: Vec<String> = (0..60).map(|i| format!("word{i:02}")).collect();
    let options = SplitOptions::default();
    let cues = split_segments(&[segment(0, 12_000, &words.join(" "))], &options);
    assert!(cues.len() >= 4, "{}", cues.len());
    assert_legal(&cues, &options);
    // All words survive in order.
    let joined: Vec<String> = cues
        .iter()
        .flat_map(|c| c.text.split_whitespace().map(str::to_owned))
        .collect();
    assert_eq!(joined, words);
    assert!(cues[0].start.0 >= 0 && cues.last().unwrap().end <= t(12_000 + 800));
}

#[test]
fn sentence_end_and_pause_start_new_cues() {
    let words = |items: &[(&str, i64, i64)]| Segment {
        start: t(items[0].1),
        end: t(items[items.len() - 1].2),
        text: String::new(),
        words: items
            .iter()
            .map(|(w, a, b)| Word {
                text: (*w).into(),
                start: t(*a),
                end: t(*b),
            })
            .collect(),
    };
    let cues = split_segments(
        &[words(&[
            ("Hi.", 0, 500),
            ("How", 600, 900),
            ("are", 900, 1100),
            ("you", 1100, 1400),
            ("later", 5000, 5400),
        ])],
        &SplitOptions::default(),
    );
    let texts: Vec<_> = cues.iter().map(|c| c.text.as_str()).collect();
    assert_eq!(texts, ["Hi.", "How are you", "later"]);
}

#[test]
fn single_line_option_never_makes_two_lines() {
    let options = SplitOptions {
        max_lines: 1,
        max_chars_per_line: 20,
        ..SplitOptions::default()
    };
    let cues = split_segments(
        &[segment(
            0,
            6000,
            "this sentence is definitely longer than twenty characters",
        )],
        &options,
    );
    assert!(cues.len() > 1);
    assert_legal(&cues, &options);
}

#[test]
fn unspaced_text_is_hard_split() {
    let text = "あ".repeat(100);
    let options = SplitOptions::default();
    let cues = split_segments(&[segment(0, 10_000, &text)], &options);
    assert_legal(&cues, &options);
    assert_eq!(
        cues.iter()
            .map(|c| c.text.replace('\n', "").chars().count())
            .sum::<usize>(),
        100
    );
}

#[test]
fn short_cues_extend_into_free_time_but_never_past_the_next_cue() {
    let options = SplitOptions::default();
    let cues = split_segments(
        &[
            segment(0, 100, "Hi."),
            segment(300, 400, "Yes."),
            segment(5000, 5100, "Later."),
        ],
        &options,
    );
    assert_eq!(cues.len(), 3);
    assert_legal(&cues, &options);
    assert!(cues[0].end <= Time(cues[1].start.0 - options.min_gap.0));
    assert_eq!(cues[2].end.0 - cues[2].start.0, options.min_duration.0);
    assert_eq!(cues[1].start, t(300));
}

#[test]
fn overlapping_and_unsorted_segments_become_ordered_cues() {
    let options = SplitOptions::default();
    let cues = split_segments(
        &[
            segment(2000, 4000, "second one"),
            segment(0, 3000, "first one here"),
        ],
        &options,
    );
    assert_legal(&cues, &options);
    assert!(cues[0].text.starts_with("first"));
}

#[test]
fn clean_text_drops_annotations() {
    assert_eq!(clean_text("  [BLANK_AUDIO] "), "");
    assert_eq!(clean_text("(music)"), "");
    assert_eq!(clean_text("\u{266a}\u{266a}"), "");
    assert_eq!(clean_text("  Hello \n world "), "Hello world");
}

#[test]
fn invalid_options_fall_back_to_defaults() {
    let bad = SplitOptions {
        max_chars_per_line: 0,
        ..SplitOptions::default()
    };
    assert!(bad.validate().is_err());
    assert_eq!(
        split_segments(&[segment(0, 1000, "ok then")], &bad).len(),
        1
    );
}

proptest! {
    #[test]
    fn splitting_always_yields_legal_ordered_cues(
        items in proptest::collection::vec((0i64..60_000, 1i64..8_000, "[a-z]{1,12}( [a-z.?!]{1,12}){0,14}"), 0..12),
        chars in 10usize..60,
        lines in 1usize..3,
    ) {
        let segments: Vec<_> = items.iter().map(|(s, d, text)| segment(*s, s + d, text)).collect();
        let options = SplitOptions { max_chars_per_line: chars, max_lines: lines, ..SplitOptions::default() };
        let cues = split_segments(&segments, &options);
        assert_legal(&cues, &options);
    }
}

fn sample() -> Vec<Cue> {
    vec![
        Cue::new(t(0), t(1500), "Hello world"),
        Cue::new(t(61_005), t(3_723_999), "Second line\nthird line"),
    ]
}

#[test]
fn srt_output_matches_the_format() {
    assert_eq!(
        format_cues(&sample(), Format::Srt),
        "1\n00:00:00,000 --> 00:00:01,500\nHello world\n\n2\n00:01:01,005 --> 01:02:03,999\nSecond line\nthird line\n\n"
    );
}

#[test]
fn vtt_output_matches_the_format() {
    assert_eq!(
        format_cues(&sample(), Format::Vtt),
        "WEBVTT\n\n00:00:00.000 --> 00:00:01.500\nHello world\n\n00:01:01.005 --> 01:02:03.999\nSecond line\nthird line\n\n"
    );
}

#[test]
fn both_formats_round_trip() {
    for format in [Format::Srt, Format::Vtt] {
        assert_eq!(
            parse_cues(&format_cues(&sample(), format)).unwrap(),
            sample()
        );
    }
}

#[test]
fn parser_accepts_crlf_bom_tags_comments_and_short_vtt_stamps() {
    let vtt = "\u{feff}WEBVTT - title\r\n\r\nNOTE a comment\r\n\r\nintro\r\n01:02.500 --> 01:04.000 align:start\r\n<v Bob><b>Hi</b> &amp; bye\r\n\r\n";
    let cues = parse_cues(vtt).unwrap();
    assert_eq!(cues.len(), 1);
    assert_eq!(cues[0].text, "Hi & bye");
    assert_eq!((cues[0].start, cues[0].end), (t(62_500), t(64_000)));
    assert_eq!(
        parse_cues("1\r\n00:00:01,000 --> 00:00:02,000\r\nA\r\n\r\n")
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn parser_rejects_malformed_times() {
    assert!(parse_cues("1\n00:00:xx,000 --> 00:00:02,000\nA\n").is_err());
    assert!(parse_cues("1\n00:00:03,000 --> 00:00:02,000\nA\n").is_err());
    assert_eq!(parse_cues("no cues here").unwrap().len(), 0);
}

#[test]
fn format_names_and_paths() {
    assert_eq!(Format::parse("SRT").unwrap(), Format::Srt);
    assert_eq!(Format::parse("webvtt").unwrap(), Format::Vtt);
    assert!(Format::parse("ass").is_err());
    assert_eq!(Format::from_path("a/b.VTT".as_ref()), Format::Vtt);
    assert_eq!(Format::from_path("a/b.txt".as_ref()), Format::Srt);
}

proptest! {
    #[test]
    fn generated_cues_round_trip(
        raw in proptest::collection::vec((0i64..4_000_000, 1i64..20_000, "[A-Za-z0-9,.' ]{1,30}(\n[A-Za-z0-9 ]{1,30}){0,1}"), 0..10)
    ) {
        let cues: Vec<Cue> = raw
            .into_iter()
            .filter(|(_, _, text)| !text.trim().is_empty() && text.lines().all(|l| !l.trim().is_empty()))
            .map(|(s, d, text)| Cue::new(t(s), t(s + d), text.trim()))
            .collect();
        for format in [Format::Srt, Format::Vtt] {
            prop_assert_eq!(parse_cues(&format_cues(&cues, format)).unwrap().len(), cues.len());
        }
    }
}

#[test]
fn parser_rejects_overflowing_timestamps() {
    assert!(
        parse_cues("1\n99999999999999999:00:00,000 --> 99999999999999999:00:01,000\nA\n").is_err()
    );
    assert!(parse_cues("1\n100000:00:00,000 --> 100000:00:01,000\nA\n").is_err());
    assert_eq!(
        parse_cues("1\n99999:59:59,999 --> 99999:59:59,999\nA\n")
            .unwrap()
            .len(),
        1
    );
}
