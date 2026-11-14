//! Automatic subtitles: local speech recognition turned into editable caption clips.
//!
//! The pipeline is split so the editor can run the slow part on a worker thread:
//! [`transcribe_project`] reads a project snapshot and returns cues without touching it,
//! [`subtitle_command`] turns cues into one undoable [`Command::Batch`] and
//! [`generate_subtitles`] does both against a live project. Cues use the bundled
//! `caption` title preset (translucent bar plus text, fade in/out), one linked pair per
//! cue, on two new tracks: `Subtitles` (text) above `Subtitles background` (bars).

use crate::{
    CaptionStyleParams, ControlError, TitleAddParams,
    captions::{resolve, style_new_bar_clip, style_new_text_clip, words_param},
    title_preset_command,
};
use animathio_core::{
    AssetId, ClipId, ClipKind, Command, History, Project, TICKS_PER_SECOND, Time, TimeRange,
    TrackId,
};
use animathio_transcribe::{
    Cancel, Cue, Format, ModelSize, SplitOptions, TranscribeOptions, cues::clean_text, format_cues,
    models, parse_cues, prepare::Selection, split_segments,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

/// Name of the text track subtitles are placed on.
pub const SUBTITLE_TRACK: &str = "Subtitles";
const BACKGROUND_TRACK: &str = "Subtitles background";
/// Title preset used for every cue.
const PRESET: &str = "caption";
/// Upper bound on a subtitle file the importer reads.
const MAX_SUBTITLE_FILE: u64 = 8 * 1024 * 1024;
/// Upper bound on the number of cues one import or generation may create.
const MAX_CUES: usize = 5000;
/// Largest import offset magnitude, in ticks (100000 hours).
const MAX_IMPORT_OFFSET: u64 = 100_000 * 3600 * TICKS_PER_SECOND as u64;

fn default_model() -> String {
    "base".to_owned()
}

/// Optional overrides for the line-splitting rules. Times are integer ticks.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtitleSplit {
    /// Longest line in characters (default 42).
    #[serde(default)]
    pub max_chars_per_line: Option<usize>,
    /// Most lines per cue (default 2).
    #[serde(default)]
    pub max_lines: Option<usize>,
    /// Shortest cue (default 800 ms).
    #[serde(default)]
    pub min_duration: Option<Time>,
    /// Longest cue (default 6 s).
    #[serde(default)]
    pub max_duration: Option<Time>,
}

impl SubtitleSplit {
    /// Merge the overrides into the defaults and validate.
    pub fn options(&self) -> Result<SplitOptions, ControlError> {
        let mut options = SplitOptions::default();
        if let Some(v) = self.max_chars_per_line {
            options.max_chars_per_line = v;
        }
        if let Some(v) = self.max_lines {
            options.max_lines = v;
        }
        if let Some(v) = self.min_duration {
            options.min_duration = v;
        }
        if let Some(v) = self.max_duration {
            options.max_duration = v;
        }
        options
            .validate()
            .map_err(|e| ControlError::InvalidParams(e.to_string()))?;
        Ok(options)
    }
}

/// Arguments for `subtitles.generate`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SubtitleGenerateParams {
    /// Model size: `tiny`, `base` (default) or `small`.
    #[serde(default = "default_model")]
    pub model: String,
    /// ISO 639-1 language code; `None` or `auto` detects it.
    #[serde(default)]
    pub language: Option<String>,
    /// Transcribe only this clip's audio.
    #[serde(default)]
    pub clip: Option<ClipId>,
    /// Transcribe only this track's audio.
    #[serde(default)]
    pub track: Option<TrackId>,
    /// Restrict to this timeline interval.
    #[serde(default)]
    pub range: Option<TimeRange>,
    /// Download the model first when it is not installed (explicit consent).
    #[serde(default)]
    pub download_model: bool,
    /// Line-splitting overrides.
    #[serde(default)]
    pub split: SubtitleSplit,
    /// Caption style for the new clips (default `classic`).
    #[serde(default)]
    pub style: CaptionStyleParams,
}

impl Default for SubtitleGenerateParams {
    fn default() -> Self {
        Self {
            model: default_model(),
            language: None,
            clip: None,
            track: None,
            range: None,
            download_model: false,
            split: SubtitleSplit::default(),
            style: CaptionStyleParams::default(),
        }
    }
}

/// Identity of the tracks and clips a subtitle command creates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtitlesAdded {
    /// The `Subtitles` text track.
    pub track_id: TrackId,
    /// The bar track behind it; `None` for styles that draw no bar.
    pub background_track_id: Option<TrackId>,
    /// First text clip.
    pub first_clip: ClipId,
    /// Number of cues created.
    pub cues: usize,
}

/// Result of `subtitles.generate` and `subtitles.import`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SubtitleResult {
    /// Language the recogniser used; empty for imports.
    pub language: String,
    /// Created tracks and clip count; `None` when nothing was recognised.
    pub added: Option<SubtitlesAdded>,
    /// The cues that were created, for display.
    pub cues: Vec<SubtitleCueInfo>,
}

/// One created cue.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtitleCueInfo {
    /// Start tick.
    pub start: Time,
    /// End tick.
    pub end: Time,
    /// Cue text, lines separated by a newline.
    pub text: String,
}

impl From<&Cue> for SubtitleCueInfo {
    fn from(cue: &Cue) -> Self {
        Self {
            start: cue.start,
            end: cue.end,
            text: cue.text.clone(),
        }
    }
}

/// Cues and language produced by [`transcribe_project`], before they touch the project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubtitleDraft {
    /// Recogniser language.
    pub language: String,
    /// Split, timed cues.
    pub cues: Vec<Cue>,
}

/// Progress of a long subtitle job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubtitleProgress {
    /// Downloading a model file.
    Downloading {
        /// Bytes on disk.
        downloaded: u64,
        /// Total bytes.
        total: u64,
    },
    /// Recognising speech, 0..=100.
    Transcribing {
        /// Percent complete.
        percent: u8,
    },
}

/// Thread-safe progress callback.
pub type ProgressFn = Arc<dyn Fn(SubtitleProgress) + Send + Sync>;

impl From<animathio_transcribe::Error> for ControlError {
    fn from(error: animathio_transcribe::Error) -> Self {
        use animathio_transcribe::Error as E;
        match error {
            E::Invalid(message) => Self::InvalidParams(message),
            E::EngineUnavailable => Self::Unsupported(error.to_string()),
            other => Self::Other(other.to_string()),
        }
    }
}

/// Recognise speech in the selected audio and split it into cues. Does not modify the
/// project. Blocking and CPU-heavy: run it on a worker thread. Cancelling through
/// `cancel` returns an error whose message is `cancelled`.
pub fn transcribe_project(
    project: &Project,
    assets: &BTreeMap<AssetId, Vec<u8>>,
    params: &SubtitleGenerateParams,
    cancel: &Cancel,
    progress: &ProgressFn,
) -> Result<SubtitleDraft, ControlError> {
    let size = ModelSize::parse(&params.model)?;
    let split = params.split.options()?;
    // Fail on a bad style before the slow work.
    resolve(project, &params.style)?;
    // Mix first so "no audio" fails before a large download starts.
    let (samples, origin) = animathio_transcribe::mix_selection(
        project,
        assets,
        Selection {
            clip: params.clip,
            track: params.track,
            range: params.range,
        },
    )?;
    if !size.is_installed() {
        if !params.download_model {
            size.require()?;
        }
        let sink = Arc::clone(progress);
        models::download(size, cancel, &mut |p| {
            sink(SubtitleProgress::Downloading {
                downloaded: p.downloaded,
                total: p.total,
            });
        })?;
    }
    let sink = Arc::clone(progress);
    let transcript = animathio_transcribe::transcribe(
        &samples,
        origin,
        &TranscribeOptions {
            model: size,
            language: params.language.clone(),
            threads: None,
        },
        cancel,
        Arc::new(move |percent| sink(SubtitleProgress::Transcribing { percent })),
    )?;
    let cues = split_segments(&transcript.segments, &split);
    Ok(SubtitleDraft {
        language: transcript.language,
        cues,
    })
}

/// Transcribe and add the subtitles as one undo step. A recognition that finds no speech
/// leaves the project and history unchanged and returns `added: None`.
pub fn generate_subtitles(
    project: &mut Project,
    history: &mut History,
    assets: &BTreeMap<AssetId, Vec<u8>>,
    params: &SubtitleGenerateParams,
    cancel: &Cancel,
    progress: &ProgressFn,
) -> Result<SubtitleResult, ControlError> {
    let draft = transcribe_project(project, assets, params, cancel, progress)?;
    apply_cues(project, history, draft.language, &draft.cues, &params.style)
}

/// Add already-known cues (from recognition or an imported file) as one undo step.
pub fn apply_cues(
    project: &mut Project,
    history: &mut History,
    language: String,
    cues: &[Cue],
    style: &CaptionStyleParams,
) -> Result<SubtitleResult, ControlError> {
    let added = if cues.is_empty() {
        None
    } else {
        let (command, added) = subtitle_command(project, cues, style)?;
        history.execute(project, &command)?;
        Some(added)
    };
    Ok(SubtitleResult {
        language,
        added,
        cues: cues.iter().map(Into::into).collect(),
    })
}

#[allow(clippy::too_many_lines)]
/// Build one batch that adds `cues` to two new tracks using the Caption preset. Every cue
/// is a linked bar + text pair with the preset's fade-in/fade-out keyframes, so edits and
/// moves of a pair stay together.
pub fn subtitle_command(
    project: &Project,
    cues: &[Cue],
    style: &CaptionStyleParams,
) -> Result<(Command, SubtitlesAdded), ControlError> {
    let invalid = |m: &str| ControlError::InvalidParams(m.to_owned());
    let resolved = resolve(project, style)?;
    let bars = resolved.info.bar;
    if cues.is_empty() {
        return Err(invalid("no subtitle cues"));
    }
    if cues.len() > MAX_CUES {
        return Err(invalid("too many subtitle cues (limit 5000)"));
    }
    let mut next = project
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
        .checked_add(1)
        .ok_or_else(|| invalid("identity overflow"))?;
    next.checked_add(
        u64::try_from(cues.len())
            .unwrap_or(u64::MAX)
            .saturating_mul(3)
            + 2,
    )
    .ok_or_else(|| invalid("identity overflow"))?;
    let mut take = || {
        let id = next;
        next += 1;
        id
    };
    let background_track = TrackId(take());
    let text_track = TrackId(take());
    let track_names = |wanted: &str| {
        let taken = |name: &str| project.tracks().iter().any(|t| t.name() == name);
        if !taken(wanted) {
            return wanted.to_owned();
        }
        (2..=u32::MAX)
            .map(|n| format!("{wanted} {n}"))
            .find(|name| !taken(name))
            .unwrap_or_else(|| wanted.to_owned())
    };
    let text_name = track_names(SUBTITLE_TRACK);
    let bar_name = if text_name == SUBTITLE_TRACK {
        BACKGROUND_TRACK.to_owned()
    } else {
        format!("{text_name} background")
    };
    let mut commands = Vec::new();
    if bars {
        commands.push(Command::AddTrack {
            track: animathio_core::Track::new(background_track, bar_name),
            index: project.tracks().len(),
        });
    }
    commands.push(Command::AddTrack {
        track: animathio_core::Track::new(text_track, text_name),
        index: project.tracks().len() + usize::from(bars),
    });
    let mut first_clip = None;
    for cue in cues {
        let range = TimeRange::new(cue.start, cue.end)
            .map_err(|e| ControlError::InvalidParams(e.to_string()))?;
        let (batch, _) = title_preset_command(
            project,
            &TitleAddParams {
                preset: PRESET.to_owned(),
                fields: BTreeMap::from([("caption".to_owned(), cue.text.clone())]),
                range,
            },
        )?;
        let Command::Batch { commands: preset } = batch else {
            return Err(ControlError::Other("title preset returned no batch".into()));
        };
        // The preset targets fresh tracks; retarget it onto the shared subtitle tracks and
        // give each clip an id unique across cues.
        let mut tracks = Vec::new();
        let mut clip_ids: BTreeMap<ClipId, ClipId> = BTreeMap::new();
        let group = ClipId(take());
        for command in preset {
            match command {
                Command::AddTrack { track, .. } => tracks.push(track.id()),
                Command::AddClip { track, mut clip } => {
                    let layer = tracks
                        .iter()
                        .position(|t| *t == track)
                        .ok_or_else(|| ControlError::Other("unexpected preset layout".into()))?;
                    let id = if clip_ids.is_empty() {
                        group
                    } else {
                        ClipId(take())
                    };
                    if layer == 0 && !bars {
                        continue;
                    }
                    clip_ids.insert(clip.id, id);
                    clip.id = id;
                    clip.link_group = Some(group);
                    if layer == 1 {
                        first_clip.get_or_insert(id);
                        style_new_text_clip(&mut clip, &resolved, words_param(cue));
                    } else {
                        style_new_bar_clip(&mut clip, &resolved);
                    }
                    commands.push(Command::AddClip {
                        track: if layer == 0 {
                            background_track
                        } else {
                            text_track
                        },
                        clip,
                    });
                }
                Command::AddKeyframe {
                    id,
                    target,
                    time,
                    keyframe,
                } => {
                    let Some(id) = clip_ids.get(&ClipId(id.0)).copied() else {
                        // The bar of a style without bars was dropped.
                        if bars {
                            return Err(ControlError::Other("unexpected preset layout".into()));
                        }
                        continue;
                    };
                    commands.push(Command::AddKeyframe {
                        id,
                        target,
                        time,
                        keyframe,
                    });
                }
                other => commands.push(other),
            }
        }
    }
    Ok((
        Command::Batch { commands },
        SubtitlesAdded {
            track_id: text_track,
            background_track_id: bars.then_some(background_track),
            first_clip: first_clip.ok_or_else(|| invalid("title preset has no text layer"))?,
            cues: cues.len(),
        },
    ))
}

/// Arguments for `subtitles.import`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SubtitleImportParams {
    /// `.srt` or `.vtt` file.
    pub path: PathBuf,
    /// Shift every cue by this many ticks (may be negative; cues ending before zero drop).
    #[serde(default)]
    pub offset: Time,
    /// Caption style for the new clips (default `classic`). Imported cues have no word
    /// timings, so the words are spread evenly over each cue.
    #[serde(default)]
    pub style: CaptionStyleParams,
}

/// Parse an SRT/`WebVTT` file and add its cues as one undo step.
pub fn import_subtitles(
    project: &mut Project,
    history: &mut History,
    params: &SubtitleImportParams,
) -> Result<SubtitleResult, ControlError> {
    let path = crate::validate_existing_file(&params.path)?;
    let bytes = crate::read_bounded(&path, MAX_SUBTITLE_FILE)?;
    let text = String::from_utf8(bytes)
        .map_err(|_| ControlError::InvalidParams("subtitle file is not UTF-8".into()))?;
    let mut cues = parse_cues(&text)?;
    if params.offset.0.unsigned_abs() > MAX_IMPORT_OFFSET {
        return Err(ControlError::InvalidParams(
            "offset must be within 100000 hours".into(),
        ));
    }
    for cue in &mut cues {
        cue.start = Time(cue.start.0.saturating_add(params.offset.0));
        cue.end = Time(cue.end.0.saturating_add(params.offset.0));
    }
    cues.retain(|c| c.end.0 > 0);
    for cue in &mut cues {
        cue.start = Time(cue.start.0.max(0));
        if cue.end.0 <= cue.start.0 {
            cue.end = Time(cue.start.0.saturating_add(TICKS_PER_SECOND / 1000));
        }
    }
    apply_cues(project, history, String::new(), &cues, &params.style)
}

/// Arguments for `subtitles.export`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SubtitleExportParams {
    /// `srt` (default) or `vtt`.
    #[serde(default = "default_format")]
    pub format: String,
    /// Write here instead of returning the text; the format follows `.vtt`/`.srt` when the
    /// format is not given explicitly through a non-default value.
    #[serde(default)]
    pub output: Option<PathBuf>,
    /// Read this track; defaults to the first track named `Subtitles*` that has text clips.
    #[serde(default)]
    pub track: Option<TrackId>,
    /// Replace an existing output file.
    #[serde(default)]
    pub overwrite: bool,
}

fn default_format() -> String {
    "srt".to_owned()
}

/// Result of `subtitles.export`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtitleExportResult {
    /// Format written, `srt` or `vtt`.
    pub format: String,
    /// Track the cues were read from.
    pub track: TrackId,
    /// Number of cues.
    pub cues: usize,
    /// The subtitle text, present when no output path was requested.
    pub text: Option<String>,
    /// Canonical path written, when requested.
    pub path: Option<String>,
}

/// Collect the text cues of a subtitle track, ordered by start time.
pub fn track_cues(
    project: &Project,
    track: Option<TrackId>,
) -> Result<(TrackId, Vec<Cue>), ControlError> {
    let has_text =
        |t: &animathio_core::Track| t.clips().values().any(|c| clip_text(&c.kind).is_some());
    let chosen = match track {
        Some(id) => project
            .tracks()
            .iter()
            .find(|t| t.id() == id)
            .ok_or_else(|| ControlError::InvalidParams(format!("track {} missing", id.0)))?,
        None => project
            .tracks()
            .iter()
            .find(|t| t.name().starts_with(SUBTITLE_TRACK) && has_text(t))
            .ok_or_else(|| {
                ControlError::InvalidParams(
                    "no `Subtitles` track with text clips; pass a track".to_owned(),
                )
            })?,
    };
    let mut cues: Vec<Cue> = chosen
        .clips()
        .values()
        .filter_map(|clip| {
            clip_text(&clip.kind).map(|text| Cue::new(clip.range.start, clip.range.end, text))
        })
        .collect();
    if cues.is_empty() {
        return Err(ControlError::InvalidParams(
            "the track has no text clips".to_owned(),
        ));
    }
    cues.sort_by_key(|c| (c.start, c.end));
    Ok((chosen.id(), cues))
}

fn clip_text(kind: &ClipKind) -> Option<String> {
    match kind {
        ClipKind::Plugin { plugin_id, params } if plugin_id == "org.animathio.text" => {
            let text = params.get("text")?.as_str()?;
            let cleaned = text
                .lines()
                .map(clean_text)
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            (!cleaned.is_empty()).then_some(cleaned)
        }
        _ => None,
    }
}

/// Render a subtitle track as SRT or `WebVTT`, returning the text or writing it to a file.
pub fn export_subtitles(
    project: &Project,
    params: &SubtitleExportParams,
) -> Result<SubtitleExportResult, ControlError> {
    let format = match (&params.output, params.format.as_str()) {
        (Some(path), "srt") if Format::from_path(path) == Format::Vtt => Format::Vtt,
        _ => Format::parse(&params.format)?,
    };
    let (track, cues) = track_cues(project, params.track)?;
    let text = format_cues(&cues, format);
    let name = if format == Format::Vtt { "vtt" } else { "srt" };
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
    Ok(SubtitleExportResult {
        format: name.to_owned(),
        track,
        cues: cues.len(),
        text: path.is_none().then_some(text),
        path,
    })
}
