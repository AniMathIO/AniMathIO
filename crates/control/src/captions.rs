//! Animated caption styles (CapCut-style) for the clips `subtitles.generate`/`import` create.
//!
//! A caption style is a set of `org.animathio.text` plugin parameters (`caption_style`,
//! `accent`, `outline`, `outline_width`, `max_words`, `words`, `font_size`, `color`, fit box)
//! plus a placement (top, middle or bottom inside the safe area). The plugin renders the
//! active word as a pure function of time; nothing here touches the format schema. Styles
//! are applied at creation ([`caption_fields`] / [`caption_layout`], used by
//! `subtitle_command`) and to existing captions with [`caption_style_command`], one
//! `Command::Batch` so undo is one step.

use crate::{ControlError, TitleAddParams, subtitles::SUBTITLE_TRACK, title_preset_command};
use animathio_core::{
    Clip, ClipId, ClipKind, Command, ParamValue, Project, Time, TimeRange, Track, TrackId,
};
use animathio_transcribe::Cue;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// Plugin that renders caption text.
const TEXT_PLUGIN: &str = "org.animathio.text";
/// Title preset the caption clips come from.
const CAPTION_PRESET: &str = "caption";

/// One bundled caption style.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct CaptionStyleInfo {
    /// Stable id: `classic`, `word-highlight`, `karaoke`, `pop`, `one-word`, `bold-outline`.
    pub id: &'static str,
    /// Display name.
    pub name: &'static str,
    /// One-line description.
    pub description: &'static str,
    /// Default font size, relative to the shorter canvas dimension.
    pub font_size: f64,
    /// Height of the caption box as a canvas-height fraction.
    pub box_height: f64,
    /// Default words per line (0 = fit the box width).
    pub max_words: u32,
    /// Default outline width as a fraction of the font size.
    pub outline_width: f64,
    /// Whether the translucent bar clip behind the text is used.
    pub bar: bool,
}

const STYLES: &[CaptionStyleInfo] = &[
    CaptionStyleInfo {
        id: "classic",
        name: "Classic bar",
        description: "White text on a translucent bar (the default look)",
        font_size: 0.04,
        box_height: 0.10,
        max_words: 0,
        outline_width: 0.0,
        bar: true,
    },
    CaptionStyleInfo {
        id: "word-highlight",
        name: "Word highlight",
        description: "The spoken word sits on an accent-coloured pill",
        font_size: 0.045,
        box_height: 0.14,
        max_words: 0,
        outline_width: 0.0,
        bar: false,
    },
    CaptionStyleInfo {
        id: "karaoke",
        name: "Karaoke fill",
        description: "Words fill with the accent colour as they are spoken",
        font_size: 0.045,
        box_height: 0.14,
        max_words: 0,
        outline_width: 0.0,
        bar: false,
    },
    CaptionStyleInfo {
        id: "pop",
        name: "Pop",
        description: "The spoken word scales up with an ease and takes the accent colour",
        font_size: 0.05,
        box_height: 0.14,
        max_words: 0,
        outline_width: 0.0,
        bar: false,
    },
    CaptionStyleInfo {
        id: "one-word",
        name: "One word at a time",
        description: "Only the spoken word is shown, large, popping in",
        font_size: 0.045,
        box_height: 0.14,
        max_words: 0,
        outline_width: 0.0,
        bar: false,
    },
    CaptionStyleInfo {
        id: "bold-outline",
        name: "Bold outline",
        description: "Big uppercase type with a thick outline, centred for 9:16",
        font_size: 0.06,
        box_height: 0.20,
        max_words: 3,
        outline_width: 0.1,
        bar: false,
    },
];

/// The bundled caption styles, in picker order.
#[must_use]
pub fn caption_styles() -> &'static [CaptionStyleInfo] {
    STYLES
}

/// Vertical placement of a caption inside the safe area.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptionPosition {
    /// Top safe-area edge.
    Top,
    /// Vertical centre.
    Middle,
    /// Bottom safe-area edge (above platform UI on 9:16).
    #[default]
    Bottom,
}

fn default_style() -> String {
    "classic".to_owned()
}

/// A caption style choice with optional overrides. Colours are straight-alpha sRGB bytes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CaptionStyleParams {
    /// Style id from [`caption_styles`] (default `classic`).
    #[serde(default = "default_style")]
    pub style: String,
    /// Font size relative to the shorter canvas dimension (style default when absent).
    #[serde(default)]
    pub font_size: Option<f64>,
    /// Text colour (default white).
    #[serde(default)]
    pub color: Option<[u8; 4]>,
    /// Active-word accent colour (default yellow).
    #[serde(default)]
    pub accent: Option<[u8; 4]>,
    /// Outline colour (default black).
    #[serde(default)]
    pub outline: Option<[u8; 4]>,
    /// Top, middle or bottom (default: bottom, middle for `bold-outline` on portrait).
    #[serde(default)]
    pub position: Option<CaptionPosition>,
    /// Words per line, 0 to fit the box width (style default when absent).
    #[serde(default)]
    pub max_words: Option<u32>,
}

impl Default for CaptionStyleParams {
    fn default() -> Self {
        Self {
            style: default_style(),
            font_size: None,
            color: None,
            accent: None,
            outline: None,
            position: None,
            max_words: None,
        }
    }
}

impl CaptionStyleParams {
    /// A style by id with no overrides.
    #[must_use]
    pub fn named(style: &str) -> Self {
        Self {
            style: style.to_owned(),
            ..Self::default()
        }
    }
}

impl CaptionPosition {
    /// Parse `top`, `middle` or `bottom`.
    pub fn parse(text: &str) -> Result<Self, ControlError> {
        match text.to_ascii_lowercase().as_str() {
            "top" => Ok(Self::Top),
            "middle" | "center" | "centre" => Ok(Self::Middle),
            "bottom" => Ok(Self::Bottom),
            other => Err(ControlError::InvalidParams(format!(
                "unknown caption position `{other}` (top, middle or bottom)"
            ))),
        }
    }
}

/// Parse `#RRGGBB`, `#RRGGBBAA` (or without `#`) into straight-alpha sRGB bytes.
pub fn parse_caption_color(text: &str) -> Result<[u8; 4], ControlError> {
    let hex = text.trim().trim_start_matches('#');
    let bad =
        || ControlError::InvalidParams(format!("colour `{text}` must be #RRGGBB or #RRGGBBAA"));
    if !(hex.len() == 6 || hex.len() == 8) || !hex.is_ascii() {
        return Err(bad());
    }
    let byte = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).map_err(|_| bad());
    Ok([
        byte(0)?,
        byte(2)?,
        byte(4)?,
        if hex.len() == 8 { byte(6)? } else { 255 },
    ])
}

/// Arguments for `subtitles.style`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SubtitleStyleParams {
    /// Restyle the captions on this track; default: every `Subtitles*` track with captions.
    #[serde(default)]
    pub track: Option<TrackId>,
    /// The style and overrides to apply.
    #[serde(flatten)]
    pub style: CaptionStyleParams,
}

/// Result of `subtitles.style`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtitleStyleResult {
    /// Style applied.
    pub style: String,
    /// Number of caption clips restyled.
    pub captions: usize,
}

/// A style resolved against a canvas: effective values and placement.
#[derive(Clone, Debug)]
pub(crate) struct Resolved {
    pub info: &'static CaptionStyleInfo,
    font: f64,
    color: [u8; 4],
    accent: [u8; 4],
    outline: [u8; 4],
    max_words: u32,
    /// Box centre and size as canvas fractions.
    pub y: f64,
    pub height: f64,
}

impl Resolved {
    /// Bar clip placement: the text box plus padding.
    pub fn bar(&self) -> (f64, f64) {
        (self.y, self.height + 0.03)
    }
}

/// Typed value for one plugin parameter.
enum Field {
    Str(String),
    Num(f64),
    Color([u8; 4]),
}

impl Field {
    fn json(&self) -> Value {
        match self {
            Self::Str(s) => json!(s),
            Self::Num(n) => json!(n),
            Self::Color(c) => json!(c),
        }
    }
    fn param(&self, enumeration: bool) -> ParamValue {
        match self {
            Self::Str(s) if enumeration => ParamValue::Enum(s.clone()),
            Self::Str(s) => ParamValue::String(s.clone()),
            Self::Num(n) => ParamValue::Number(*n),
            Self::Color(c) => ParamValue::from_srgba(*c),
        }
    }
}

pub(crate) fn canvas_aspect(project: &Project) -> f64 {
    let (width, height) = animathio_export::resolution(project);
    f64::from(width) / f64::from(height)
}

/// Resolve `params` against the project canvas.
pub(crate) fn resolve(
    project: &Project,
    params: &CaptionStyleParams,
) -> Result<Resolved, ControlError> {
    let invalid = |m: &str| ControlError::InvalidParams(m.to_owned());
    let info = STYLES
        .iter()
        .find(|s| s.id == params.style)
        .ok_or_else(|| {
            invalid(&format!(
                "unknown caption style `{}` (one of: {})",
                params.style,
                STYLES.iter().map(|s| s.id).collect::<Vec<_>>().join(", ")
            ))
        })?;
    if let Some(size) = params.font_size
        && !(0.01..=0.2).contains(&size)
    {
        return Err(invalid("font_size must be between 0.01 and 0.2"));
    }
    if params.max_words.is_some_and(|w| w > 12) {
        return Err(invalid("max_words must be at most 12"));
    }
    let aspect = canvas_aspect(project);
    let portrait = aspect < 1.0;
    let position = params
        .position
        .unwrap_or(if info.id == "bold-outline" && portrait {
            CaptionPosition::Middle
        } else {
            CaptionPosition::Bottom
        });
    let (top, bottom) = if portrait { (0.12, 0.84) } else { (0.08, 0.92) };
    let height = info.box_height;
    let y = match position {
        CaptionPosition::Top => top + height / 2.0,
        CaptionPosition::Middle => 0.5,
        CaptionPosition::Bottom => bottom - height / 2.0,
    };
    Ok(Resolved {
        info,
        font: params.font_size.unwrap_or(info.font_size) * aspect.min(1.0),
        color: params.color.unwrap_or([255; 4]),
        accent: params.accent.unwrap_or([255, 214, 10, 255]),
        outline: params.outline.unwrap_or([0, 0, 0, 255]),
        max_words: params.max_words.unwrap_or(info.max_words),
        y,
        height,
    })
}

fn fields(resolved: &Resolved, words: String) -> Vec<(&'static str, Field, bool)> {
    vec![
        (
            "caption_style",
            Field::Str(resolved.info.id.to_owned()),
            true,
        ),
        ("font_size", Field::Num(resolved.font), false),
        ("color", Field::Color(resolved.color), false),
        ("accent", Field::Color(resolved.accent), false),
        ("outline", Field::Color(resolved.outline), false),
        (
            "outline_width",
            Field::Num(resolved.info.outline_width),
            false,
        ),
        (
            "max_words",
            Field::Num(f64::from(resolved.max_words)),
            false,
        ),
        ("fit_height", Field::Num(resolved.height), false),
        ("words", Field::Str(words), false),
    ]
}

/// Word timings of `cue` as the plugin's `words` parameter: `start:end` ticks per word,
/// relative to the cue start (the caption clip's source offset is zero).
pub(crate) fn words_param(cue: &Cue) -> String {
    cue.timed_words()
        .iter()
        .map(|w| format!("{}:{}", w.start.0 - cue.start.0, w.end.0 - cue.start.0))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Apply a resolved style and word timings to a freshly created caption text clip.
pub(crate) fn style_new_text_clip(clip: &mut Clip, resolved: &Resolved, words: String) {
    clip.transform.y = resolved.y;
    clip.transform.scale_y = resolved.height;
    if let ClipKind::Plugin { params, .. } = &mut clip.kind {
        for (name, field, _) in fields(resolved, words) {
            params[name] = field.json();
        }
    }
}

/// Apply placement to a freshly created caption bar clip.
pub(crate) fn style_new_bar_clip(clip: &mut Clip, resolved: &Resolved) {
    let (y, height) = resolved.bar();
    clip.transform.y = y;
    clip.transform.scale_y = height;
}

fn plugin_marker(clip: &Clip, key: &str) -> Option<String> {
    match &clip.kind {
        ClipKind::Plugin { params, .. } => params.get(key)?.as_str().map(str::to_owned),
        _ => None,
    }
}

fn is_caption_text(clip: &Clip) -> bool {
    matches!(&clip.kind, ClipKind::Plugin { plugin_id, .. } if plugin_id == TEXT_PLUGIN)
        && plugin_marker(clip, "_title_preset").as_deref() == Some(CAPTION_PRESET)
        && plugin_marker(clip, "_title_field").as_deref() == Some("caption")
}

fn is_caption_bar(clip: &Clip) -> bool {
    plugin_marker(clip, "_title_preset").as_deref() == Some(CAPTION_PRESET)
        && plugin_marker(clip, "_title_field").as_deref() == Some("")
}

/// The style id a caption clip currently uses (`classic` when none is stored).
#[must_use]
pub fn clip_caption_style(clip: &Clip) -> Option<String> {
    if !is_caption_text(clip) {
        return None;
    }
    let typed = match clip.params.get("caption_style") {
        Some(ParamValue::Enum(s) | ParamValue::String(s)) => Some(s.clone()),
        _ => None,
    };
    Some(
        typed
            .or_else(|| plugin_marker(clip, "caption_style"))
            .filter(|s| STYLES.iter().any(|i| i.id == *s))
            .unwrap_or_else(default_style),
    )
}

/// Edit a caption's text and re-sync its word timings as one undoable operation.
/// Exact unchanged words retain their timings (longest common subsequence); new words
/// share the available interval between their neighbours, weighted by character count.
/// Returns `None` for ordinary text clips.
pub fn caption_text_edit_command(clip: &Clip, text: &str) -> Option<Command> {
    if !is_caption_text(clip) {
        return None;
    }
    let previous = match clip.params.get("text") {
        Some(ParamValue::String(s)) => s.clone(),
        _ => plugin_marker(clip, "text").unwrap_or_default(),
    };
    let stored = match clip.params.get("words") {
        Some(ParamValue::String(s)) => s.clone(),
        _ => plugin_marker(clip, "words").unwrap_or_default(),
    };
    let timings = resync_words(
        &previous,
        text,
        &stored,
        clip.range.end.0 - clip.range.start.0,
    );
    Some(Command::Batch {
        commands: vec![
            Command::SetPluginParam {
                id: clip.id,
                name: "text".into(),
                value: Some(ParamValue::String(text.into())),
            },
            Command::SetPluginParam {
                id: clip.id,
                name: "words".into(),
                value: Some(ParamValue::String(timings)),
            },
        ],
    })
}

fn resync_words(old: &str, new: &str, stored: &str, duration: i64) -> String {
    let old: Vec<_> = old.split_whitespace().collect();
    let new: Vec<_> = new.split_whitespace().collect();
    let parsed: Option<Vec<(i64, i64)>> = stored
        .split_whitespace()
        .map(|p| {
            let (a, b) = p.split_once(':')?;
            Some((a.parse().ok()?, b.parse().ok()?))
        })
        .collect();
    let valid = parsed.as_ref().is_some_and(|v| {
        v.len() == old.len()
            && v.iter().all(|&(a, b)| a >= 0 && b >= a)
            && v.windows(2).all(|w| w[0].1 <= w[1].0)
    });
    // Source-relative timings can extend beyond the visible window after a trim.
    let duration = if valid {
        parsed
            .as_ref()
            .and_then(|v| v.last())
            .map_or(duration, |v| duration.max(v.1))
    } else {
        duration
    };
    let mut result = vec![None; new.len()];
    // Bound quadratic work for arbitrarily large imported text. Such cues are spread
    // over the clip instead; normal subtitle cues are far below this limit.
    if valid && old.len().saturating_mul(new.len()) <= 262_144 {
        let parsed = parsed.as_ref().expect("validated timings");
        let mut lcs = vec![vec![0usize; new.len() + 1]; old.len() + 1];
        for i in (0..old.len()).rev() {
            for j in (0..new.len()).rev() {
                lcs[i][j] = if old[i] == new[j] {
                    1 + lcs[i + 1][j + 1]
                } else {
                    lcs[i + 1][j].max(lcs[i][j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < old.len() && j < new.len() {
            if old[i] == new[j] {
                result[j] = Some(parsed[i]);
                i += 1;
                j += 1;
            } else if lcs[i + 1][j] >= lcs[i][j + 1] {
                i += 1;
            } else {
                j += 1;
            }
        }
    }
    let mut start = 0;
    while start < new.len() {
        if result[start].is_some() {
            start += 1;
            continue;
        }
        let end = (start..new.len())
            .find(|&i| result[i].is_some())
            .unwrap_or(new.len());
        let from = if start == 0 {
            0
        } else {
            result[start - 1].expect("filled neighbour").1
        };
        let to = if end == new.len() {
            duration.max(from)
        } else {
            result[end].expect("anchor").0.max(from)
        };
        let total: usize = new[start..end].iter().map(|w| w.chars().count()).sum();
        let mut done = 0;
        for i in start..end {
            let a = from + ((to - from) as i128 * done as i128 / total as i128) as i64;
            done += new[i].chars().count();
            let b = from + ((to - from) as i128 * done as i128 / total as i128) as i64;
            result[i] = Some((a, b));
        }
        start = end;
    }
    result
        .into_iter()
        .flatten()
        .map(|(a, b)| format!("{a}:{b}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Caption text clips (and their bar partners) per track, for restyling.
struct CaptionTrack<'a> {
    track: &'a Track,
    texts: Vec<&'a Clip>,
}

fn caption_tracks(
    project: &Project,
    track: Option<TrackId>,
) -> Result<Vec<CaptionTrack<'_>>, ControlError> {
    let tracks: Vec<CaptionTrack<'_>> = project
        .tracks()
        .iter()
        .filter(|t| match track {
            Some(id) => t.id() == id,
            None => t.name().starts_with(SUBTITLE_TRACK),
        })
        .map(|track| CaptionTrack {
            track,
            texts: track
                .clips()
                .values()
                .filter(|c| is_caption_text(c))
                .collect(),
        })
        .filter(|t| !t.texts.is_empty())
        .collect();
    if tracks.is_empty() {
        return Err(ControlError::InvalidParams(
            "no caption clips found (generate or import subtitles first, or pass a track)".into(),
        ));
    }
    Ok(tracks)
}

fn next_id(project: &Project) -> u64 {
    project
        .tracks()
        .iter()
        .flat_map(|t| {
            std::iter::once(t.id().0).chain(
                t.clips()
                    .values()
                    .flat_map(|c| [c.id.0, c.link_group.map_or(0, |g| g.0)]),
            )
        })
        .max()
        .unwrap_or(0)
        + 1
}

/// Build one batch that restyles every caption clip of the selected track(s): plugin
/// parameters, placement and the bar clips (added for `classic`, removed for the others).
/// Returns the batch and the number of captions.
#[allow(clippy::too_many_lines)]
pub fn caption_style_command(
    project: &Project,
    params: &SubtitleStyleParams,
) -> Result<(Command, usize), ControlError> {
    let resolved = resolve(project, &params.style)?;
    let tracks = caption_tracks(project, params.track)?;
    let mut commands = Vec::new();
    let mut next = next_id(project);
    let mut take = || {
        let id = next;
        next += 1;
        id
    };
    let mut count = 0;
    for CaptionTrack { track, texts } in tracks {
        // Existing bar clips of these captions, by link group.
        let bar_track = project.tracks().iter().find(|t| {
            t.clips().values().any(|c| {
                is_caption_bar(c)
                    && texts
                        .iter()
                        .any(|text| text.link_group.is_some() && text.link_group == c.link_group)
            })
        });
        let mut bar_track_id = bar_track.map(Track::id);
        let mut removed_bars = 0usize;
        for text in &texts {
            count += 1;
            let words =
                if text.params.contains_key("words") || plugin_marker(text, "words").is_some() {
                    None
                } else {
                    // Captions made without word timings: spread evenly over the visible span.
                    let content = plugin_marker(text, "text").unwrap_or_default();
                    let mut cue = Cue::new(
                        Time(text.source_offset.0),
                        Time(text.source_offset.0 + text.range.end.0 - text.range.start.0),
                        content,
                    );
                    cue.words.clear();
                    Some(words_param(&cue))
                };
            for (name, field, enumeration) in fields(&resolved, words.clone().unwrap_or_default()) {
                if name == "words" && words.is_none() {
                    continue;
                }
                commands.push(Command::SetPluginParam {
                    id: text.id,
                    name: name.to_owned(),
                    value: Some(field.param(enumeration)),
                });
            }
            let mut transform = text.transform;
            transform.y = resolved.y;
            transform.scale_y = resolved.height;
            commands.push(Command::SetTransform {
                id: text.id,
                transform,
            });
            let bar = bar_track.and_then(|t| {
                t.clips()
                    .values()
                    .find(|c| is_caption_bar(c) && c.link_group == text.link_group)
            });
            match (resolved.info.bar, bar) {
                (true, Some(bar)) => {
                    let (y, height) = resolved.bar();
                    let mut transform = bar.transform;
                    transform.y = y;
                    transform.scale_y = height;
                    commands.push(Command::SetTransform {
                        id: bar.id,
                        transform,
                    });
                }
                (true, None) => {
                    let track_id = *bar_track_id.get_or_insert_with(|| {
                        let id = TrackId(take());
                        commands.push(Command::AddTrack {
                            track: Track::new(id, format!("{} background", track.name())),
                            index: project.tracks().len(),
                        });
                        id
                    });
                    commands.extend(bar_clip_commands(
                        project, text, track_id, &resolved, &mut take,
                    )?);
                }
                (false, Some(bar)) => {
                    removed_bars += 1;
                    // Removing a clip takes its link group along; unlink the bar first.
                    commands.push(Command::SetClipLink {
                        id: bar.id,
                        group: None,
                    });
                    commands.push(Command::RemoveClip { id: bar.id });
                }
                (false, None) => {}
            }
        }
        if let (Some(track), false) = (bar_track, resolved.info.bar)
            && removed_bars > 0
            && track.clips().len() == removed_bars
        {
            commands.push(Command::RemoveTrack { id: track.id() });
        }
    }
    Ok((Command::Batch { commands }, count))
}

/// Restyle the captions as one undo step.
pub fn apply_caption_style(
    project: &mut Project,
    history: &mut animathio_core::History,
    params: &SubtitleStyleParams,
) -> Result<SubtitleStyleResult, ControlError> {
    let (command, captions) = caption_style_command(project, params)?;
    history.execute(project, &command)?;
    Ok(SubtitleStyleResult {
        style: params.style.style.clone(),
        captions,
    })
}

/// Add commands for a caption bar behind `text`, from the bundled Caption preset.
fn bar_clip_commands(
    project: &Project,
    text: &Clip,
    bar_track: TrackId,
    resolved: &Resolved,
    take: &mut impl FnMut() -> u64,
) -> Result<Vec<Command>, ControlError> {
    let (batch, _) = title_preset_command(
        project,
        &TitleAddParams {
            preset: CAPTION_PRESET.to_owned(),
            fields: BTreeMap::new(),
            range: TimeRange::new(text.range.start, text.range.end)?,
        },
    )?;
    let Command::Batch { commands: preset } = batch else {
        return Err(ControlError::Other("title preset returned no batch".into()));
    };
    let mut layer_tracks = Vec::new();
    let mut bar_ids: BTreeSet<ClipId> = BTreeSet::new();
    let mut renamed: BTreeMap<ClipId, ClipId> = BTreeMap::new();
    let mut out = Vec::new();
    for command in preset {
        match command {
            Command::AddTrack { track, .. } => layer_tracks.push(track.id()),
            Command::AddClip { track, mut clip } => {
                if layer_tracks.iter().position(|t| *t == track) != Some(0) {
                    continue;
                }
                let id = ClipId(take());
                renamed.insert(clip.id, id);
                bar_ids.insert(id);
                clip.id = id;
                clip.link_group = text.link_group;
                style_new_bar_clip(&mut clip, resolved);
                out.push(Command::AddClip {
                    track: bar_track,
                    clip,
                });
            }
            Command::AddKeyframe {
                id,
                target,
                time,
                keyframe,
            } => {
                if let Some(id) = renamed.get(&ClipId(id.0)) {
                    out.push(Command::AddKeyframe {
                        id: *id,
                        target,
                        time,
                        keyframe,
                    });
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
mod timing_edit_tests {
    use super::resync_words;

    #[test]
    fn deletions_replacements_duplicates_and_empty_text() {
        assert_eq!(
            resync_words("a b c", "a c", "0:10 20:30 40:50", 60),
            "0:10 40:50"
        );
        assert_eq!(
            resync_words("a b c", "a long x c", "0:10 20:30 40:50", 60),
            "0:10 10:34 34:40 40:50"
        );
        assert_eq!(
            resync_words("a a b", "a b", "0:10 20:30 40:50", 60),
            "0:10 40:50"
        );
        assert_eq!(resync_words("a", "", "0:10", 60), "");
        assert_eq!(resync_words("a b", "a b", "0:20 30:60", 40), "0:20 30:60");
        assert_eq!(resync_words("a", "bb c", "broken", 60), "0:40 40:60");
        assert_eq!(resync_words("a", "a b", "0:60", 60), "0:60 60:60");
    }
}
