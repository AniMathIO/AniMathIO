//! High-level editing helpers shared by the MCP server, headless handler and editor:
//! silence detection, a human-readable timeline description, title presets and
//! export presets. Every mutation is expressed as one core [`Command`] (usually a
//! [`Command::Batch`]) so it is a single undo step.

use std::{collections::BTreeMap, fmt::Write as _};

use animathio_audio::{AudioPool, SymphoniaSource};
use animathio_core::{
    AssetId, Clip, ClipId, ClipKind, Command, FrameRate, History, Project, TICKS_PER_SECOND, Time,
    TimeRange, Track, TrackId, Transform, builtin::Builtin,
};
use serde::{Deserialize, Serialize};

use crate::{ControlError, clip_kind_label};

/// Analysis sample rate. `TICKS_PER_SECOND / 16_000` is an exact integer.
const ANALYSIS_RATE: u32 = 16_000;
/// RMS window length (10 ms).
const WINDOW_FRAMES: usize = 160;
const TICKS_PER_SAMPLE: i64 = TICKS_PER_SECOND / ANALYSIS_RATE as i64;

/// Arguments for `timeline.trim_silence`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrimSilenceParams {
    /// Windows whose RMS level is below this many dBFS count as silent.
    #[serde(default = "default_threshold_db")]
    pub threshold_db: f64,
    /// Only silent runs at least this long are removed.
    #[serde(default = "default_min_silence")]
    pub min_silence: Time,
    /// Silence kept on each side of a removed run, so cuts are not abrupt.
    #[serde(default = "default_padding")]
    pub padding: Time,
    /// Analyse only this track's audio; `None` mixes every audio track.
    #[serde(default)]
    pub track: Option<TrackId>,
}

fn default_threshold_db() -> f64 {
    -40.0
}
fn default_min_silence() -> Time {
    Time(TICKS_PER_SECOND / 1000 * 400)
}
fn default_padding() -> Time {
    Time(TICKS_PER_SECOND / 1000 * 100)
}

impl Default for TrimSilenceParams {
    fn default() -> Self {
        Self {
            threshold_db: default_threshold_db(),
            min_silence: default_min_silence(),
            padding: default_padding(),
            track: None,
        }
    }
}

/// Result of `timeline.trim_silence`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrimSilenceResult {
    /// Removed ranges in the timeline coordinates from before the edit, ascending.
    pub removed: Vec<TimeRange>,
    /// Total removed duration in ticks.
    pub total_removed: Time,
}

/// Detect silent ranges and ripple-remove them from every track as one undo step.
/// A rejected call leaves the project and history unchanged.
pub fn trim_silence(
    project: &mut Project,
    history: &mut History,
    assets: &BTreeMap<AssetId, Vec<u8>>,
    params: &TrimSilenceParams,
) -> Result<TrimSilenceResult, ControlError> {
    let removed = detect_silence(project, assets, params)?;
    let total = removed.iter().map(|r| r.end.0 - r.start.0).sum();
    if !removed.is_empty() {
        // Remove from the end so earlier ranges keep their original coordinates.
        let commands = removed
            .iter()
            .rev()
            .map(|range| Command::RemoveRange {
                range: *range,
                ripple: true,
            })
            .collect();
        history.execute(project, &Command::Batch { commands })?;
    }
    Ok(TrimSilenceResult {
        removed,
        total_removed: Time(total),
    })
}

/// Find silent ranges in the mixed audio of `params.track` (or all tracks), limited to
/// the span covered by audio clips. Muted clips and track gain are honoured.
pub fn detect_silence(
    project: &Project,
    assets: &BTreeMap<AssetId, Vec<u8>>,
    params: &TrimSilenceParams,
) -> Result<Vec<TimeRange>, ControlError> {
    if !params.threshold_db.is_finite() || params.min_silence.0 <= 0 || params.padding.0 < 0 {
        return Err(ControlError::InvalidParams(
            "threshold must be finite, min_silence positive and padding nonnegative".to_owned(),
        ));
    }
    let mut analysis = project.clone();
    if let Some(keep) = params.track {
        if !project.tracks().iter().any(|t| t.id() == keep) {
            return Err(ControlError::InvalidParams(format!(
                "track {} missing",
                keep.0
            )));
        }
        let others: Vec<_> = project
            .tracks()
            .iter()
            .map(Track::id)
            .filter(|id| *id != keep)
            .collect();
        for id in others {
            Command::RemoveTrack { id }.apply(&mut analysis)?;
        }
    }
    let mut pool = AudioPool::default();
    let mut span: Option<(Time, Time)> = None;
    for clip in analysis.tracks().iter().flat_map(|t| t.clips().values()) {
        let ClipKind::Audio(id) = &clip.kind else {
            continue;
        };
        span = Some(span.map_or((clip.range.start, clip.range.end), |(s, e)| {
            (s.min(clip.range.start), e.max(clip.range.end))
        }));
        if pool.contains(id) {
            continue;
        }
        let bytes = assets.get(id).ok_or_else(|| {
            ControlError::Other(format!("audio asset {} not loaded", id.as_str()))
        })?;
        let source = SymphoniaSource::from_bytes(bytes.clone()).map_err(audio_error)?;
        pool.insert(id.clone(), &source).map_err(audio_error)?;
    }
    let Some((start, end)) = span else {
        return Err(ControlError::InvalidParams(
            "no audio clips to analyse".to_owned(),
        ));
    };
    let samples = pool
        .mix(&analysis, TimeRange::new(start, end)?, ANALYSIS_RATE, 1)
        .map_err(audio_error)?;
    // Tick of the first mixed sample (the mixer rounds the range start up).
    // Range starts are nonnegative, so this is a ceiling division.
    let first_tick = (start.0 + TICKS_PER_SAMPLE - 1) / TICKS_PER_SAMPLE * TICKS_PER_SAMPLE;
    let threshold = 10_f64.powf(params.threshold_db / 20.0);
    let window_ticks = TICKS_PER_SAMPLE * i64::try_from(WINDOW_FRAMES).unwrap_or(i64::MAX);

    let mut ranges = Vec::new();
    let mut run_start: Option<i64> = None;
    let windows = samples.chunks(WINDOW_FRAMES);
    let count = windows.len();
    for (index, window) in windows.enumerate() {
        let energy: f64 = window.iter().map(|s| f64::from(*s).powi(2)).sum();
        #[allow(clippy::cast_precision_loss)]
        let rms = (energy / window.len() as f64).sqrt();
        let tick = first_tick + i64::try_from(index).unwrap_or(i64::MAX) * window_ticks;
        if rms < threshold {
            run_start.get_or_insert(tick);
        }
        let last = index + 1 == count;
        if (rms >= threshold || last)
            && let Some(begin) = run_start.take()
        {
            let finish = if rms < threshold { end.0 } else { tick };
            push_run(&mut ranges, begin.max(start.0), finish.min(end.0), params);
        }
    }
    Ok(ranges)
}

fn push_run(ranges: &mut Vec<TimeRange>, begin: i64, finish: i64, params: &TrimSilenceParams) {
    if finish - begin < params.min_silence.0 {
        return;
    }
    let (from, to) = (begin + params.padding.0, finish - params.padding.0);
    if to > from
        && let Ok(range) = TimeRange::new(Time(from), Time(to))
    {
        ranges.push(range);
    }
}

#[allow(clippy::needless_pass_by_value)]
fn audio_error(error: animathio_audio::Error) -> ControlError {
    ControlError::Other(error.to_string())
}

/// Format ticks as `HH:MM:SS.mmm` (rounded down to the millisecond).
#[must_use]
pub fn timecode(time: Time) -> String {
    let sign = if time.0 < 0 { "-" } else { "" };
    let ms = time.0.unsigned_abs() / (TICKS_PER_SECOND as u64 / 1000);
    format!(
        "{sign}{:02}:{:02}:{:02}.{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

/// Longest user-controlled name (in characters) echoed into agent-facing text.
const MAX_NAME_CHARS: usize = 80;

/// Make a user-controlled name (asset, track or clip text) safe to interpolate into
/// text an LLM reads: first line only, control and bidi/zero-width characters
/// stripped, capped at [`MAX_NAME_CHARS`], then quoted and escaped with `{:?}` so it
/// can never be mistaken for surrounding structure or instructions.
fn quote_name(name: &str) -> String {
    let line = name.lines().next().unwrap_or_default();
    let cleaned: String = line
        .chars()
        .filter(|c| {
            !c.is_control()
                && !matches!(
                    c,
                    '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}'
                        | '\u{2066}'..='\u{2069}' | '\u{FEFF}'
                )
        })
        .collect();
    let capped = if cleaned.chars().count() > MAX_NAME_CHARS {
        let mut cut: String = cleaned.chars().take(MAX_NAME_CHARS - 3).collect();
        cut.push_str("...");
        cut
    } else {
        cleaned
    };
    format!("{capped:?}")
}

fn clip_name(project: &Project, clip: &Clip) -> String {
    let shorten = |text: &str| {
        let line = text.lines().next().unwrap_or_default();
        if line.chars().count() > 40 {
            format!("{}...", line.chars().take(40).collect::<String>())
        } else {
            line.to_owned()
        }
    };
    match &clip.kind {
        ClipKind::Video(id) | ClipKind::Image(id) | ClipKind::Audio(id) => project
            .assets()
            .get(id)
            .map_or_else(|| "missing asset".to_owned(), |a| a.name.clone()),
        ClipKind::Builtin(Builtin::Text { text, .. }) => shorten(text),
        ClipKind::Builtin(Builtin::Shape { .. }) => "shape".to_owned(),
        ClipKind::Plugin { params, .. } => params
            .get("source")
            .or_else(|| params.get("text"))
            .or_else(|| params.get("items"))
            .and_then(serde_json::Value::as_str)
            .map(shorten)
            .unwrap_or_default(),
        ClipKind::Vector(_) => String::new(),
        ClipKind::Error(message) => shorten(message),
    }
}

/// End of the last clip on any track.
#[must_use]
pub fn content_end(project: &Project) -> Time {
    project
        .tracks()
        .iter()
        .flat_map(|t| t.clips().values())
        .map(|c| c.range.end)
        .max()
        .unwrap_or_default()
}

/// A concise, human-readable timeline summary for agents: settings, then each track
/// (front-most last) with its clips and gaps in `HH:MM:SS.mmm` timecode.
#[must_use]
pub fn describe_timeline(project: &Project) -> String {
    let mut out = String::new();
    let rate = project.frame_rate();
    let end = content_end(project);
    let (duration, origin) = if project.duration().0 > 0 {
        (project.duration(), "explicit")
    } else {
        (end, "derived from clips")
    };
    let resolution = if project.width() == 0 || project.height() == 0 {
        "default resolution".to_owned()
    } else {
        format!("{}x{}", project.width(), project.height())
    };
    let _ = writeln!(
        out,
        "Project: {resolution} @ {} fps, duration {} ({origin}), {} track(s)",
        fps_label(rate),
        timecode(duration),
        project.tracks().len()
    );
    for (index, track) in project.tracks().iter().enumerate() {
        let _ = writeln!(
            out,
            "Track {} {} (id {}, layer {index}, gain {:.2}): {} clip(s)",
            index + 1,
            quote_name(track.name()),
            track.id().0,
            track.gain(),
            track.clips().len()
        );
        let mut clips: Vec<_> = track.clips().values().collect();
        clips.sort_by_key(|c| c.range.start);
        let mut cursor = Time(0);
        for clip in clips {
            if clip.range.start > cursor {
                let _ = writeln!(
                    out,
                    "  gap   {} - {}",
                    timecode(cursor),
                    timecode(clip.range.start)
                );
            }
            let name = clip_name(project, clip);
            let _ = write!(
                out,
                "  clip {} {} - {} {}",
                clip.id.0,
                timecode(clip.range.start),
                timecode(clip.range.end),
                clip_kind_label(&clip.kind),
            );
            if !name.is_empty() {
                let _ = write!(out, " {}", quote_name(&name));
            }
            if clip.source_offset.0 != 0 {
                let _ = write!(out, " (source from {})", timecode(clip.source_offset));
            }
            out.push('\n');
            cursor = clip.range.end;
        }
        if !track.clips().is_empty() && cursor < end {
            let _ = writeln!(
                out,
                "  gap   {} - {} (end)",
                timecode(cursor),
                timecode(end)
            );
        }
    }
    out
}

fn fps_label(rate: FrameRate) -> String {
    if rate.denominator() == 1 {
        rate.numerator().to_string()
    } else {
        format!(
            "{:.3} ({}/{})",
            f64::from(rate.numerator()) / f64::from(rate.denominator()),
            rate.numerator(),
            rate.denominator()
        )
    }
}

/// On-screen text presets for `add_title`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TitleStyle {
    /// Large centred title.
    Title,
    /// Medium text in the lower-left third.
    LowerThird,
    /// Small subtitle near the bottom edge; consecutive captions share a track.
    Caption,
}

impl TitleStyle {
    /// `(track name, x, y, font size as a fraction of canvas height)`.
    fn preset(self) -> (&'static str, f64, f64, f64) {
        match self {
            Self::Title => ("Titles", 0.5, 0.45, 0.1),
            Self::LowerThird => ("Lower thirds", 0.35, 0.8, 0.055),
            Self::Caption => ("Captions", 0.5, 0.9, 0.045),
        }
    }
}

/// Build one undoable command adding a styled text clip. The clip goes on the
/// front-most track named after the style when that track has room, else on a new
/// front-most track. Returns the command and the new clip's placement.
pub fn title_command(
    project: &Project,
    text: &str,
    range: TimeRange,
    style: TitleStyle,
) -> Result<(Command, TrackId, ClipId), ControlError> {
    if text.trim().is_empty() {
        return Err(ControlError::InvalidParams(
            "title text is empty".to_owned(),
        ));
    }
    let (name, x, y, font_size) = style.preset();
    let max_track = project.tracks().iter().map(|t| t.id().0).max();
    let max_clip = project
        .tracks()
        .iter()
        .flat_map(|t| t.clips().keys())
        .map(|c| c.0)
        .max();
    let next = max_track
        .max(max_clip)
        .map_or(Some(1), |m| m.checked_add(1))
        .ok_or_else(|| ControlError::InvalidParams("identity overflow".to_owned()))?;
    let clip_id = ClipId(next);
    let mut clip = Clip::new(
        clip_id,
        ClipKind::Builtin(Builtin::Text {
            text: text.to_owned(),
            font_size,
            color: [255, 255, 255, 255],
        }),
        range,
    );
    clip.transform = Transform {
        x,
        y,
        ..Transform::default()
    };
    let reusable = project.tracks().iter().rev().find(|t| {
        t.name() == name
            && t.clips()
                .values()
                .all(|c| c.range.end <= range.start || c.range.start >= range.end)
    });
    let mut commands = Vec::new();
    let track_id = if let Some(track) = reusable {
        track.id()
    } else {
        let id = TrackId(next + 1);
        commands.push(Command::AddTrack {
            track: Track::new(id, name),
            index: project.tracks().len(),
        });
        id
    };
    commands.push(Command::AddClip {
        track: track_id,
        clip,
    });
    Ok((Command::Batch { commands }, track_id, clip_id))
}

/// Delivery presets for `set_export_preset`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportPreset {
    /// 1920x1080 at 30 fps.
    #[serde(rename = "youtube_1080p")]
    Youtube1080p,
    /// 3840x2160 at 30 fps.
    #[serde(rename = "youtube_4k")]
    Youtube4k,
    /// 1080x1920 vertical at 30 fps (`YouTube` Shorts, Reels, `TikTok`).
    ShortsVertical,
    /// 1080x1080 at 30 fps.
    Square,
}

impl ExportPreset {
    /// Shared delivery preset table for the editor, CLI and MCP.
    pub const ALL: [Self; 4] = [
        Self::Youtube1080p,
        Self::Youtube4k,
        Self::ShortsVertical,
        Self::Square,
    ];

    /// Stable name used in JSON and user-facing preset selectors.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Youtube1080p => "youtube_1080p",
            Self::Youtube4k => "youtube_4k",
            Self::ShortsVertical => "shorts_vertical",
            Self::Square => "square",
        }
    }

    /// Output `(width, height)`.
    #[must_use]
    pub const fn resolution(self) -> (u32, u32) {
        match self {
            Self::Youtube1080p => (1920, 1080),
            Self::Youtube4k => (3840, 2160),
            Self::ShortsVertical => (1080, 1920),
            Self::Square => (1080, 1080),
        }
    }

    /// Encoder settings for this preset (H.264 CRF 18, the project frame rate).
    #[must_use]
    pub fn export_settings(self) -> animathio_export::Settings {
        animathio_export::Settings {
            frame_rate: None,
            crf: 18.0,
        }
    }

    /// The `SetSettings` command applying this preset's resolution at 30 fps,
    /// keeping the background and explicit duration.
    pub fn command(self, project: &Project) -> Result<Command, ControlError> {
        let (width, height) = self.resolution();
        Ok(Command::SetSettings {
            width,
            height,
            frame_rate: FrameRate::new(30, 1)?,
            background: project.background(),
            duration: project.duration(),
        })
    }
}
