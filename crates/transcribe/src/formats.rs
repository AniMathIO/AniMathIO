//! SRT and `WebVTT` reading and writing. Timestamps are millisecond precision, so cues that
//! start on whole milliseconds round-trip exactly.

use crate::{Cue, Error};
use animathio_core::{TICKS_PER_SECOND, Time};
use std::fmt::Write as _;

/// A subtitle file format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// `SubRip`: numbered cues, `HH:MM:SS,mmm --> HH:MM:SS,mmm`.
    Srt,
    /// `WebVTT`: `WEBVTT` header, `HH:MM:SS.mmm --> HH:MM:SS.mmm`.
    Vtt,
}

impl Format {
    /// Parse `srt` or `vtt` (case-insensitive).
    pub fn parse(name: &str) -> Result<Self, Error> {
        match name.to_ascii_lowercase().as_str() {
            "srt" => Ok(Self::Srt),
            "vtt" | "webvtt" => Ok(Self::Vtt),
            other => Err(Error::Invalid(format!(
                "unknown subtitle format `{other}` (use srt or vtt)"
            ))),
        }
    }

    /// Guess from a file extension, defaulting to SRT.
    #[must_use]
    pub fn from_path(path: &std::path::Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some(e) if e.eq_ignore_ascii_case("vtt") => Self::Vtt,
            _ => Self::Srt,
        }
    }
}

const TICKS_PER_MS: i64 = TICKS_PER_SECOND / 1000;

fn to_ms(time: Time) -> i64 {
    (time.0.max(0) + TICKS_PER_MS / 2) / TICKS_PER_MS
}

fn stamp(time: Time, separator: char) -> String {
    let total = to_ms(time);
    format!(
        "{:02}:{:02}:{:02}{separator}{:03}",
        total / 3_600_000,
        total / 60_000 % 60,
        total / 1000 % 60,
        total % 1000
    )
}

/// Render cues in the given format.
#[must_use]
pub fn format_cues(cues: &[Cue], format: Format) -> String {
    let mut out = String::new();
    if format == Format::Vtt {
        out.push_str("WEBVTT\n\n");
    }
    for (index, cue) in cues.iter().enumerate() {
        let separator = if format == Format::Srt { ',' } else { '.' };
        if format == Format::Srt {
            let _ = writeln!(out, "{}", index + 1);
        }
        let _ = writeln!(
            out,
            "{} --> {}",
            stamp(cue.start, separator),
            stamp(cue.end, separator)
        );
        // A blank line would end the cue early; keep the text contiguous.
        let text = cue
            .text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        let _ = write!(out, "{text}\n\n");
    }
    out
}

/// Parse SRT or `WebVTT` text; the format is detected from the `WEBVTT` header.
pub fn parse_cues(text: &str) -> Result<Vec<Cue>, Error> {
    let text = text
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let vtt = text.trim_start().starts_with("WEBVTT");
    let mut cues = Vec::new();
    for block in text.split("\n\n") {
        let lines: Vec<&str> = block.lines().map(str::trim_end).collect();
        let Some(timing) = lines.iter().position(|l| l.contains("-->")) else {
            continue;
        };
        if vtt && lines[0].starts_with("NOTE")
            || lines[0].starts_with("STYLE")
            || lines[0].starts_with("REGION")
        {
            continue;
        }
        let (from, rest) = lines[timing]
            .split_once("-->")
            .ok_or_else(|| Error::Invalid("malformed cue timing".into()))?;
        let to = rest.split_whitespace().next().unwrap_or("");
        let start = parse_stamp(from.trim())?;
        let end = parse_stamp(to)?;
        if end.0 < start.0 {
            return Err(Error::Invalid(format!(
                "cue ends before it starts: {}",
                lines[timing]
            )));
        }
        let body = lines[timing + 1..]
            .iter()
            .map(|l| strip_tags(l))
            .collect::<Vec<_>>()
            .join("\n");
        if body.trim().is_empty() {
            continue;
        }
        cues.push(Cue {
            start,
            end,
            text: body.trim().to_owned(),
        });
    }
    Ok(cues)
}

fn parse_stamp(text: &str) -> Result<Time, Error> {
    let bad = || Error::Invalid(format!("malformed timestamp `{text}`"));
    let normalized = text.replace(',', ".");
    let (clock, fraction) = normalized.split_once('.').ok_or_else(bad)?;
    let parts: Vec<&str> = clock.split(':').collect();
    let number = |s: &str| s.parse::<i64>().map_err(|_| bad());
    let (hours, minutes, seconds) = match parts.as_slice() {
        [h, m, s] => (number(h)?, number(m)?, number(s)?),
        [m, s] => (0, number(m)?, number(s)?),
        _ => return Err(bad()),
    };
    if fraction.is_empty() || fraction.len() > 9 || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad());
    }
    let digits: String = fraction.chars().chain("000".chars()).take(3).collect();
    let millis = number(&digits)?;
    if !(0..60).contains(&minutes) || !(0..60).contains(&seconds) || hours < 0 {
        return Err(bad());
    }
    let total = ((hours * 60 + minutes) * 60 + seconds) * 1000 + millis;
    total.checked_mul(TICKS_PER_MS).map(Time).ok_or_else(bad)
}

/// Remove `<b>`-style markup and unescape the basic `WebVTT` entities.
fn strip_tags(line: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in line.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}
