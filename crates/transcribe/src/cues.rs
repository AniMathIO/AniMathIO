//! Turning recognised segments into subtitle cues. Pure logic, no model needed.
//!
//! Rules (all configurable through [`SplitOptions`]): at most `max_lines` lines of at most
//! `max_chars_per_line` characters, a cue never outlasts `max_duration` and is never
//! shorter than `min_duration` unless the next cue starts first, and cues never overlap.
//! Cues also break after sentence-ending punctuation and after long pauses.

use animathio_core::{TICKS_PER_SECOND, Time};

/// One recognised word with its timeline-relative interval.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Word {
    /// Word text without surrounding whitespace.
    pub text: String,
    /// Start time.
    pub start: Time,
    /// End time.
    pub end: Time,
}

/// One recognised phrase. `words` may be empty; then the text is spread over the interval.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    /// Start time.
    pub start: Time,
    /// End time.
    pub end: Time,
    /// Recognised text.
    pub text: String,
    /// Word timings, when the engine produced them.
    pub words: Vec<Word>,
}

/// One subtitle: a time interval and one or two lines of text separated by `\n`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cue {
    /// Start time.
    pub start: Time,
    /// Exclusive end time.
    pub end: Time,
    /// Display text; lines are separated by `\n`.
    pub text: String,
    /// Per-word timings (absolute, one entry per whitespace-separated token of `text`).
    /// Empty when the source had none (SRT/VTT imports); see [`Cue::timed_words`].
    pub words: Vec<Word>,
}

impl Cue {
    /// A cue without word timings.
    #[must_use]
    pub fn new(start: Time, end: Time, text: impl Into<String>) -> Self {
        Self {
            start,
            end,
            text: text.into(),
            words: Vec::new(),
        }
    }

    /// One timed word per whitespace-separated token of the text: the recognised timings
    /// when they match the text, otherwise the words evenly spread (by character count)
    /// over the cue interval. Timings are clamped inside the cue and monotonic.
    #[must_use]
    pub fn timed_words(&self) -> Vec<Word> {
        let tokens: Vec<&str> = self.text.split_whitespace().collect();
        let usable = self.words.len() == tokens.len()
            && self
                .words
                .iter()
                .zip(&tokens)
                .all(|(w, t)| w.text.split_whitespace().collect::<String>() == *t);
        if usable {
            let mut floor = self.start.0;
            return self
                .words
                .iter()
                .map(|w| {
                    let start = w.start.0.clamp(floor, self.end.0.max(floor));
                    let end = w.end.0.clamp(start, self.end.0.max(start));
                    floor = start;
                    Word {
                        text: w.text.clone(),
                        start: Time(start),
                        end: Time(end),
                    }
                })
                .collect();
        }
        let total: usize = tokens.iter().map(|t| t.chars().count().max(1)).sum();
        let span = (self.end.0 - self.start.0).max(0);
        let mut done = 0usize;
        tokens
            .iter()
            .map(|token| {
                let from = self.start.0 + proportion(span, done, total);
                done += token.chars().count().max(1);
                let to = self.start.0 + proportion(span, done, total);
                Word {
                    text: (*token).to_owned(),
                    start: Time(from),
                    end: Time(to),
                }
            })
            .collect()
    }
}

/// Line-splitting and timing rules.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SplitOptions {
    /// Longest line, in characters.
    pub max_chars_per_line: usize,
    /// Most lines per cue (1 or 2 are sensible).
    pub max_lines: usize,
    /// Shortest cue on screen (extended into free time after it).
    pub min_duration: Time,
    /// Longest cue on screen.
    pub max_duration: Time,
    /// Silence of at least this length between words starts a new cue.
    pub pause_break: Time,
    /// Minimum gap kept between consecutive cues.
    pub min_gap: Time,
}

const fn ms(value: i64) -> Time {
    Time(TICKS_PER_SECOND / 1000 * value)
}

impl Default for SplitOptions {
    fn default() -> Self {
        Self {
            max_chars_per_line: 42,
            max_lines: 2,
            min_duration: ms(800),
            max_duration: ms(6000),
            pause_break: ms(1000),
            min_gap: ms(40),
        }
    }
}

impl SplitOptions {
    /// Check the options are usable.
    pub fn validate(&self) -> Result<(), crate::Error> {
        let bad = |m: &str| Err(crate::Error::Invalid(m.to_owned()));
        if self.max_chars_per_line < 8 || self.max_chars_per_line > 200 {
            return bad("max_chars_per_line must be between 8 and 200");
        }
        if self.max_lines == 0 || self.max_lines > 4 {
            return bad("max_lines must be between 1 and 4");
        }
        if self.min_duration.0 <= 0
            || self.max_duration.0 < self.min_duration.0
            || self.min_gap.0 < 0
            || self.pause_break.0 <= 0
        {
            return bad("durations must be positive and min_duration <= max_duration");
        }
        Ok(())
    }
}

/// Split recognised segments into cues following `options`.
///
/// The output is sorted, non-overlapping, has no empty cues and every cue satisfies the
/// line and duration limits. `options` that fail [`SplitOptions::validate`] fall back to
/// the defaults rather than panicking.
#[must_use]
pub fn split_segments(segments: &[Segment], options: &SplitOptions) -> Vec<Cue> {
    let options = if options.validate().is_ok() {
        options.clone()
    } else {
        SplitOptions::default()
    };
    let mut words: Vec<Word> = segments
        .iter()
        .flat_map(|segment| segment_words(segment, &options))
        .collect();
    words.sort_by_key(|w| (w.start, w.end));
    let mut cues: Vec<Cue> = Vec::new();
    let mut group: Vec<Word> = Vec::new();
    for word in words {
        if let Some(last) = group.last() {
            let mut candidate: Vec<&str> = group.iter().map(|w| w.text.as_str()).collect();
            candidate.push(&word.text);
            let too_long = wrap(&candidate, &options).is_none();
            let too_slow = word.end.0 - group[0].start.0 > options.max_duration.0;
            let paused = word.start.0 - last.end.0 >= options.pause_break.0;
            let sentence_end = ends_sentence(&last.text);
            if too_long || too_slow || paused || sentence_end {
                cues.extend(finish(&group, &options));
                group.clear();
            }
        }
        group.push(word);
    }
    cues.extend(finish(&group, &options));
    enforce_timing(cues, &options)
}

/// Words of one segment, spreading the text over the interval when timings are absent and
/// hard-splitting tokens that cannot fit on one line (e.g. unspaced CJK text).
fn segment_words(segment: &Segment, options: &SplitOptions) -> Vec<Word> {
    let timed: Vec<Word> = if segment.words.is_empty() {
        let tokens: Vec<&str> = segment.text.split_whitespace().collect();
        let total: usize = tokens.iter().map(|t| t.chars().count().max(1)).sum();
        let span = (segment.end.0 - segment.start.0).max(0);
        let mut done = 0usize;
        tokens
            .iter()
            .map(|token| {
                let from = segment.start.0 + proportion(span, done, total);
                done += token.chars().count().max(1);
                let to = segment.start.0 + proportion(span, done, total);
                Word {
                    text: (*token).to_owned(),
                    start: Time(from),
                    end: Time(to),
                }
            })
            .collect()
    } else {
        segment
            .words
            .iter()
            .filter(|w| !w.text.trim().is_empty())
            .map(|w| Word {
                text: w.text.trim().to_owned(),
                start: w.start,
                end: w.end.max(w.start),
            })
            .collect()
    };
    let limit = options.max_chars_per_line;
    timed
        .into_iter()
        .flat_map(|word| {
            let chars: Vec<char> = word.text.chars().collect();
            if chars.len() <= limit {
                return vec![word];
            }
            let pieces: Vec<&[char]> = chars.chunks(limit).collect();
            let span = word.end.0 - word.start.0;
            let count = pieces.len();
            pieces
                .iter()
                .enumerate()
                .map(|(index, piece)| Word {
                    text: piece.iter().collect(),
                    start: Time(word.start.0 + proportion(span, index, count)),
                    end: Time(word.start.0 + proportion(span, index + 1, count)),
                })
                .collect()
        })
        .collect()
}

#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
fn proportion(span: i64, part: usize, total: usize) -> i64 {
    if total == 0 {
        return 0;
    }
    (i128::from(span) * part as i128 / total as i128) as i64
}

fn ends_sentence(text: &str) -> bool {
    text.trim_end_matches(['"', '\'', ')', ']', '”', '’'])
        .ends_with(['.', '!', '?', '…', '。', '！', '？'])
}

/// Greedy-wrap `words` into at most `max_lines` lines; `None` if they do not fit. Two-line
/// results are rebalanced so lines are close in length.
fn wrap(words: &[&str], options: &SplitOptions) -> Option<Vec<String>> {
    let limit = options.max_chars_per_line;
    let mut lines: Vec<Vec<&str>> = vec![Vec::new()];
    let mut width = 0usize;
    for word in words {
        let len = word.chars().count();
        let line = lines.last_mut()?;
        if line.is_empty() {
            line.push(word);
            width = len;
        } else if width + 1 + len <= limit {
            line.push(word);
            width += 1 + len;
        } else {
            lines.push(vec![word]);
            width = len;
        }
    }
    if lines.len() > options.max_lines
        || lines.iter().any(|l| {
            l.iter().map(|w| w.chars().count()).sum::<usize>() + l.len().saturating_sub(1) > limit
        })
    {
        return None;
    }
    if lines.len() == 2 && options.max_lines >= 2 {
        let line_len = |slice: &[&str]| {
            slice.iter().map(|w| w.chars().count()).sum::<usize>() + slice.len().saturating_sub(1)
        };
        let best = (1..words.len())
            .filter(|&at| line_len(&words[..at]) <= limit && line_len(&words[at..]) <= limit)
            .min_by_key(|&at| line_len(&words[..at]).abs_diff(line_len(&words[at..])))?;
        return Some(vec![words[..best].join(" "), words[best..].join(" ")]);
    }
    Some(lines.into_iter().map(|l| l.join(" ")).collect())
}

fn finish(group: &[Word], options: &SplitOptions) -> Option<Cue> {
    let first = group.first()?;
    let last = group.last()?;
    let texts: Vec<&str> = group.iter().map(|w| w.text.as_str()).collect();
    let lines = wrap(&texts, options)?;
    Some(Cue {
        start: first.start,
        end: last.end.max(first.start),
        text: lines.join("\n"),
        words: group.to_vec(),
    })
}

/// Sort, then make durations and gaps legal without delaying later cues.
fn enforce_timing(mut cues: Vec<Cue>, options: &SplitOptions) -> Vec<Cue> {
    cues.sort_by_key(|c| (c.start, c.end));
    let next_starts: Vec<i64> = cues.iter().skip(1).map(|c| c.start.0).collect();
    let mut out: Vec<Cue> = Vec::with_capacity(cues.len());
    for (index, mut cue) in cues.into_iter().enumerate() {
        if let Some(previous) = out.last() {
            cue.start = Time(cue.start.0.max(previous.end.0 + options.min_gap.0));
        }
        let ceiling = next_starts
            .get(index)
            .map_or(i64::MAX, |next| next - options.min_gap.0);
        // Extend short cues into free time, never into the next cue.
        let wanted = cue.start.0 + options.min_duration.0;
        let mut end = cue.end.0.max(cue.start.0 + 1);
        if end < wanted {
            end = wanted.min(ceiling.max(end));
        }
        end = end.min(cue.start.0 + options.max_duration.0);
        cue.end = Time(end.max(cue.start.0 + 1));
        if !cue.text.trim().is_empty() {
            out.push(cue);
        }
    }
    out
}

/// Drop non-speech annotations (`[BLANK_AUDIO]`, `(music)`, music notes) and normalise
/// whitespace in recognised text. Returns an empty string for pure annotations.
#[must_use]
pub fn clean_text(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim();
    let annotation = (trimmed.starts_with('[') && trimmed.ends_with(']'))
        || (trimmed.starts_with('(') && trimmed.ends_with(')'))
        || trimmed
            .chars()
            .all(|c| matches!(c, '\u{266a}' | '\u{266b}' | ' ' | '-'));
    if annotation {
        String::new()
    } else {
        trimmed.to_owned()
    }
}
