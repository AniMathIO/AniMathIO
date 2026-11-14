//! Chapter lists from timeline markers: a `YouTube`-style description block, `WebVTT`
//! chapters, or an `FFmpeg` metadata file. The MP4 muxer has no chapter support, so chapters
//! ship as sidecars; `ffmpeg -i video.mp4 -i chapters.ffmeta -map_metadata 1 -codec copy
//! out.mp4` embeds the `ffmetadata` form without re-encoding.

use crate::ControlError;
use animathio_core::{Marker, Project, TICKS_PER_SECOND, Time};
use serde::{Deserialize, Serialize};
use std::{fmt::Write as _, path::PathBuf};

/// Sidecar chapter formats.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChapterFormat {
    /// One `M:SS Title` line per chapter, ready for a `YouTube` description.
    #[default]
    Youtube,
    /// `WebVTT` chapter cues (`<track kind="chapters">`).
    Vtt,
    /// `FFmpeg` `;FFMETADATA1` chapter metadata.
    Ffmetadata,
}
impl ChapterFormat {
    /// Parse `youtube`, `vtt` or `ffmetadata`.
    pub fn parse(name: &str) -> Result<Self, ControlError> {
        match name.trim().to_ascii_lowercase().as_str() {
            "youtube" | "txt" => Ok(Self::Youtube),
            "vtt" | "webvtt" => Ok(Self::Vtt),
            "ffmetadata" | "ffmeta" => Ok(Self::Ffmetadata),
            other => Err(ControlError::InvalidParams(format!(
                "unknown chapter format {other:?}: use youtube, vtt or ffmetadata"
            ))),
        }
    }
    /// Lowercase format name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Youtube => "youtube",
            Self::Vtt => "vtt",
            Self::Ffmetadata => "ffmetadata",
        }
    }
    /// Format implied by an output file extension.
    #[must_use]
    pub fn from_path(path: &std::path::Path) -> Option<Self> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "vtt" => Some(Self::Vtt),
            "ffmeta" | "ffmetadata" => Some(Self::Ffmetadata),
            "txt" => Some(Self::Youtube),
            _ => None,
        }
    }
}

/// Arguments for `chapters.export`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChaptersExportParams {
    /// Output format (default `youtube`). With `output`, a `.vtt`, `.ffmeta` or `.txt`
    /// extension picks the format when this is left at its default.
    pub format: ChapterFormat,
    /// Write here instead of returning the text.
    pub output: Option<PathBuf>,
    /// Replace an existing output file.
    pub overwrite: bool,
    /// Title of the synthetic first chapter added when the first marker is after 0:00
    /// (`YouTube` requires a chapter at 0:00). Empty disables it.
    pub intro_title: String,
    /// Shortest allowed chapter in ticks; markers closer than this to the previous
    /// chapter are merged into it (default 10 s, `YouTube`'s minimum). Zero keeps all.
    pub min_length: Time,
    /// Timeline position that becomes 0:00 (an export's `from`); earlier markers are dropped.
    pub start: Time,
    /// Timeline position used as the last chapter's end; defaults to the timeline's end.
    pub end: Option<Time>,
}
impl Default for ChaptersExportParams {
    fn default() -> Self {
        Self {
            format: ChapterFormat::default(),
            output: None,
            overwrite: false,
            intro_title: "Intro".into(),
            min_length: Time(10 * TICKS_PER_SECOND),
            start: Time(0),
            end: None,
        }
    }
}

/// One chapter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chapter {
    /// Start in ticks.
    pub start: Time,
    /// End in ticks (the next chapter's start, or the timeline end).
    pub end: Time,
    /// Chapter title.
    pub title: String,
}

/// Result of `chapters.export`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChaptersExportResult {
    /// Format written.
    pub format: ChapterFormat,
    /// Number of chapters.
    pub chapters: usize,
    /// The text, present when no output path was requested.
    pub text: Option<String>,
    /// Canonical path written, when requested.
    pub path: Option<String>,
}

fn title_of(marker: &Marker, index: usize) -> String {
    let name: String = marker.name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        format!("Chapter {}", index + 1)
    } else {
        name
    }
}

/// Chapters from the project's markers. Each marker starts a chapter that runs to the next
/// marker (or `end`); markers at the same time collapse to the first. Markers closer than
/// `min_length` to the previous chapter, or at/after `end`, are dropped. When the first
/// marker is after zero and `intro_title` is non-empty, a chapter at 0 is prepended.
#[must_use]
pub fn chapters(project: &Project, params: &ChaptersExportParams) -> Vec<Chapter> {
    let end = params
        .end
        .unwrap_or_else(|| crate::content_end(project).max(Time(project.duration().0)));
    let mut starts: Vec<(Time, String)> = Vec::new();
    for (index, marker) in project.markers().iter().enumerate() {
        if marker.time < params.start || (marker.time >= end && end.0 > 0) {
            continue;
        }
        let marker_time = Time(marker.time.0 - params.start.0);
        if starts.last().is_some_and(|(t, _)| *t == marker_time) {
            continue;
        }
        if let Some((last, _)) = starts.last()
            && marker_time.0 - last.0 < params.min_length.0
        {
            continue;
        }
        starts.push((marker_time, title_of(marker, index)));
    }
    if !params.intro_title.trim().is_empty() && starts.first().is_some_and(|(t, _)| t.0 > 0) {
        starts.insert(0, (Time(0), params.intro_title.trim().to_owned()));
    }
    let end = Time((end.0 - params.start.0).max(0));
    let mut out = Vec::with_capacity(starts.len());
    for (i, (start, title)) in starts.iter().enumerate() {
        let next = starts.get(i + 1).map_or(end, |(t, _)| *t);
        out.push(Chapter {
            start: *start,
            end: next.max(*start),
            title: title.clone(),
        });
    }
    out
}

fn millis(time: Time) -> i64 {
    i64::try_from(i128::from(time.0.max(0)) * 1000 / i128::from(TICKS_PER_SECOND))
        .unwrap_or(i64::MAX)
}
fn split(time: Time) -> (i64, i64, i64, i64) {
    let ms = millis(time);
    (ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}
fn youtube_stamp(time: Time) -> String {
    let (h, m, s, _) = split(time);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}
fn vtt_stamp(time: Time) -> String {
    let (h, m, s, ms) = split(time);
    format!("{h:02}:{m:02}:{s:02}.{ms:03}")
}
fn ffmeta_escape(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if matches!(c, '=' | ';' | '#' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Render `chapters` in `format`.
#[must_use]
pub fn format_chapters(chapters: &[Chapter], format: ChapterFormat) -> String {
    let mut out = String::new();
    match format {
        ChapterFormat::Youtube => {
            for chapter in chapters {
                let _ = writeln!(out, "{} {}", youtube_stamp(chapter.start), chapter.title);
            }
        }
        ChapterFormat::Vtt => {
            out.push_str("WEBVTT\n");
            for (i, chapter) in chapters.iter().enumerate() {
                // A cue needs a positive length; a zero-length final chapter gets 1 ms.
                let end = chapter.end.max(Time(
                    chapter.start.0.saturating_add(TICKS_PER_SECOND / 1000),
                ));
                let _ = writeln!(
                    out,
                    "\nchapter-{}\n{} --> {}\n{}",
                    i + 1,
                    vtt_stamp(chapter.start),
                    vtt_stamp(end),
                    chapter.title.replace("-->", "->")
                );
            }
        }
        ChapterFormat::Ffmetadata => {
            out.push_str(";FFMETADATA1\n");
            for chapter in chapters {
                let start = millis(chapter.start);
                let end = millis(chapter.end).max(start + 1);
                let _ = writeln!(
                    out,
                    "\n[CHAPTER]\nTIMEBASE=1/1000\nSTART={start}\nEND={end}\ntitle={}",
                    ffmeta_escape(&chapter.title)
                );
            }
        }
    }
    out
}

/// Export the project's markers as chapters, returning the text or writing a file.
pub fn export_chapters(
    project: &Project,
    params: &ChaptersExportParams,
) -> Result<ChaptersExportResult, ControlError> {
    let format = match (&params.output, params.format) {
        (Some(path), ChapterFormat::Youtube) => {
            ChapterFormat::from_path(path).unwrap_or(ChapterFormat::Youtube)
        }
        (_, format) => format,
    };
    let list = chapters(project, params);
    if list.is_empty() {
        return Err(ControlError::InvalidParams(
            "the project has no markers to use as chapters".into(),
        ));
    }
    let text = format_chapters(&list, format);
    let path = match &params.output {
        None => None,
        Some(output) => {
            let target = crate::validate_output_file(output)?;
            if target.exists() && !params.overwrite {
                return Err(ControlError::AlreadyExists(target));
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&target, &text)?;
            Some(target.display().to_string())
        }
    };
    Ok(ChaptersExportResult {
        format,
        chapters: list.len(),
        text: path.is_none().then_some(text),
        path,
    })
}
