//! Typed control method arguments and results.
//!
//! `Call` is the wire-level dispatch enum: it serializes to
//! `{"method": "...", "params": ...}`, so the JSON-RPC envelope can be built
//! and parsed by splitting and rejoining those two fields.

use animathio_core::{AssetId, ClipId, Command, FrameRate, Project, Time, TrackId};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The currently loaded project plus lightweight session state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectState {
    /// Plain, resource-free project data.
    pub project: Project,
    /// Filesystem path the project was loaded from or last saved to.
    pub path: Option<String>,
    /// Current playhead position in timeline ticks.
    pub playhead: Time,
    /// Whether playback is currently running.
    pub playing: bool,
}

/// Content-addressed metadata for one imported asset.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetInfo {
    /// BLAKE3 content hash.
    pub id: AssetId,
    /// Human-readable filename.
    pub name: String,
    /// Declared media type, inferred from the extension.
    pub media_type: String,
}

/// Arguments for `render.frame`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderFrameParams {
    /// Timeline position to evaluate.
    pub time: Time,
    /// Output width in pixels.
    pub width: u32,
    /// Output height in pixels.
    pub height: u32,
    /// When set, write the PNG here instead of returning base64.
    #[serde(default)]
    pub output: Option<PathBuf>,
}

/// Result of `render.frame`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderFrameResult {
    /// Rendered dimensions.
    pub size: (u32, u32),
    /// Base64-encoded PNG, present when no output path was requested.
    pub base64: Option<String>,
    /// Canonical path the PNG was written to, when requested.
    pub path: Option<String>,
}

/// Offline MP4 export arguments. All times are integer timeline ticks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportParams {
    /// Destination MP4 file.
    pub output: PathBuf,
    /// Inclusive start, default zero.
    #[serde(default)]
    pub from: Time,
    /// Exclusive end, default project duration.
    #[serde(default)]
    pub to: Option<Time>,
    /// Encoding quality and optional rational frame rate.
    #[serde(default)]
    pub settings: animathio_export::Settings,
    /// Replace an existing output file instead of failing with `AlreadyExists`.
    #[serde(default)]
    pub overwrite: bool,
}

/// A read-only summary of the timeline for `timeline.list`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Timeline {
    /// Project frame rate.
    pub frame_rate: FrameRate,
    /// Explicit project duration in ticks; `0` means derived from clips.
    pub duration: Time,
    /// Layers in back-to-front compositing order.
    pub tracks: Vec<TrackInfo>,
}

/// One layer of the timeline summary.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrackInfo {
    /// Layer identity.
    pub id: TrackId,
    /// Layer label.
    pub name: String,
    /// Linear audio gain.
    pub gain: f64,
    /// Clips on this layer.
    pub clips: Vec<ClipInfo>,
}

/// Arguments for `math.add`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MathAddParams {
    /// Math source, validated up front through the typeset engine.
    pub latex: String,
    /// Typeset mode: `latex` or `typst`.
    #[serde(default = "default_math_mode")]
    pub mode: String,
    /// Font size in canvas pixels.
    #[serde(default = "default_math_size")]
    pub size: f64,
    /// Straight-alpha sRGB color; defaults to opaque white.
    pub color: Option<[u8; 4]>,
    /// Horizontal position as a canvas fraction; defaults to centred.
    pub x: Option<f64>,
    /// Vertical position as a canvas fraction; defaults to centred.
    pub y: Option<f64>,
    /// Inclusive timeline start in ticks.
    #[serde(default)]
    pub start: Time,
    /// Clip duration in ticks; `None` derives from the project.
    pub duration: Option<Time>,
    /// Write-style reveal: animate `reveal` from 0 to 1 over this many ticks.
    pub reveal_duration: Option<Time>,
}

fn default_math_mode() -> String {
    "latex".to_owned()
}

fn default_math_size() -> f64 {
    48.0
}

/// Arguments for `graph.add`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GraphAddParams {
    /// Lower x bound.
    pub x_min: Option<f64>,
    /// Upper x bound.
    pub x_max: Option<f64>,
    /// Lower y bound.
    pub y_min: Option<f64>,
    /// Upper y bound.
    pub y_max: Option<f64>,
    /// Newline-separated graph items, e.g. `plot y=x^2`.
    #[serde(default = "default_graph_items")]
    pub items: String,
    /// Draw the coordinate grid.
    pub show_grid: Option<bool>,
    /// Draw the axes.
    pub show_axes: Option<bool>,
    /// Draw numeric labels.
    pub show_labels: Option<bool>,
    /// Straight-alpha sRGB color; defaults to the plugin's blue.
    pub color: Option<[u8; 4]>,
    /// Horizontal position as a canvas fraction; defaults to centred.
    pub x: Option<f64>,
    /// Vertical position as a canvas fraction; defaults to centred.
    pub y: Option<f64>,
    /// Inclusive timeline start in ticks.
    #[serde(default)]
    pub start: Time,
    /// Clip duration in ticks; `None` derives from the project.
    pub duration: Option<Time>,
    /// Draw animation: animate `draw` from 0 to 1 over this many ticks.
    pub draw_duration: Option<Time>,
}

fn default_graph_items() -> String {
    "plot y=x^2".to_owned()
}

/// Identity of a newly added plugin clip.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipAdded {
    /// Destination layer identity.
    pub track_id: TrackId,
    /// New clip identity.
    pub clip_id: ClipId,
}

/// Arguments for `import.manim`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportManimParams {
    /// Script file path; exclusive with `source`.
    pub path: Option<PathBuf>,
    /// Inline script source; exclusive with `path`.
    pub source: Option<String>,
    /// Directory used to resolve embedded files; defaults to the script's parent.
    pub script_dir: Option<PathBuf>,
    /// Scene class name; defaults to the first `Scene` subclass.
    pub scene: Option<String>,
    /// Replace the current project instead of appending to it.
    #[serde(default)]
    pub replace: bool,
}

/// Result of `import.manim`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportManimResult {
    /// Number of imported layers.
    pub tracks: usize,
    /// Number of embedded assets.
    pub assets: usize,
    /// Non-fatal warnings, each formatted as `line:column: message (snippet)`.
    pub warnings: Vec<String>,
    /// Embedded-file and other informational entries.
    pub infos: Vec<String>,
}

/// One clip of the timeline summary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipInfo {
    /// Clip identity.
    pub id: ClipId,
    /// Human-readable source kind label.
    pub kind: String,
    /// Inclusive timeline start.
    pub start: Time,
    /// Exclusive timeline end.
    pub end: Time,
    /// Source offset at the clip's start.
    pub source_offset: Time,
}

/// Every protocol method, tagged with its method name.
// `command.apply` carries a full `animathio_core::Command` by value; the enum
// is a serialization shape, not a hot path, so indirection is unnecessary here.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum Call {
    /// List bundled title compositions and their editable fields.
    #[serde(rename = "titles.list")]
    TitlesList,
    /// Add a linked title composition as one undo step.
    #[serde(rename = "titles.add")]
    TitlesAdd {
        /// Preset, field overrides and timeline range.
        params: crate::TitleAddParams,
    },
    /// Read the project and session state.
    #[serde(rename = "project.get")]
    ProjectGet,
    /// Load a `.amio` archive or `project.json` file.
    #[serde(rename = "project.open")]
    ProjectOpen {
        /// Project path.
        path: PathBuf,
    },
    /// Save the project, optionally to a new path.
    #[serde(rename = "project.save")]
    ProjectSave {
        /// Destination path; `None` overwrites the current path.
        path: Option<PathBuf>,
    },
    /// Apply one serialized command through the shared history.
    #[serde(rename = "command.apply")]
    CommandApply {
        /// The command to apply.
        command: Command,
    },
    /// Undo the last command.
    #[serde(rename = "history.undo")]
    HistoryUndo,
    /// Redo the last undone command.
    #[serde(rename = "history.redo")]
    HistoryRedo,
    /// Render one frame to base64 or a PNG path.
    #[serde(rename = "render.frame")]
    RenderFrame {
        /// Rendering arguments.
        params: RenderFrameParams,
    },
    /// Export a snapshot offline to H.264/AAC MP4; returns its path after completion.
    #[serde(rename = "export")]
    Export {
        /// Destination, range and encoding choices.
        params: ExportParams,
    },
    /// Move the playhead to a timeline position.
    #[serde(rename = "playback.seek")]
    PlaybackSeek {
        /// Target position in ticks.
        time: Time,
    },
    /// Start playback.
    #[serde(rename = "playback.play")]
    PlaybackPlay,
    /// Pause playback.
    #[serde(rename = "playback.pause")]
    PlaybackPause,
    /// Register asset bytes read from a filesystem path.
    #[serde(rename = "assets.import")]
    AssetsImport {
        /// Existing media file.
        path: PathBuf,
    },
    /// Import and place media atomically from a bounded read.
    #[serde(rename = "media.add")]
    MediaAdd {
        /// Existing media file.
        path: PathBuf,
    },
    /// Read the timeline summary.
    #[serde(rename = "timeline.list")]
    TimelineList,
    /// Import a v1 `.animathio` project, replacing the current one.
    #[serde(rename = "project.import_v1")]
    ProjectImportV1 {
        /// v1 project path.
        path: PathBuf,
    },
    /// Add a LaTeX plugin clip.
    #[serde(rename = "math.add")]
    MathAdd {
        /// Clip parameters.
        params: MathAddParams,
    },
    /// Add a graph plugin clip.
    #[serde(rename = "graph.add")]
    GraphAdd {
        /// Clip parameters.
        params: GraphAddParams,
    },
    /// Import a Manim scene, appending to or replacing the current project.
    #[serde(rename = "import.manim")]
    ImportManim {
        /// Import parameters.
        params: ImportManimParams,
    },
    /// List the effects provided by loaded effect plugins.
    #[serde(rename = "effects.list")]
    EffectsList,
    /// Add a plugin effect to a clip's chain with the plugin's default parameters.
    #[serde(rename = "effects.add")]
    EffectsAdd {
        /// Clip, plugin, filter and optional parameter overrides.
        params: crate::EffectAddParams,
    },
    /// List the audio effects provided by loaded audio-effect plugins.
    #[serde(rename = "audio_effects.list")]
    AudioEffectsList,
    /// Add an audio effect to a clip's or a track's chain with the plugin's default parameters.
    #[serde(rename = "audio_effects.add")]
    AudioEffectsAdd {
        /// Clip or track, plugin, filter and optional parameter overrides.
        params: crate::AudioEffectAddParams,
    },
    /// Detect silent audio and ripple-remove it from every track as one undo step.
    #[serde(rename = "timeline.trim_silence")]
    TrimSilence {
        /// Detection parameters.
        params: crate::TrimSilenceParams,
    },
    /// Propose silence and optional speech cuts without editing the project.
    #[serde(rename = "timeline.propose_autocuts")]
    ProposeAutocuts {
        /// Detection settings and selection.
        params: crate::AutocutParams,
    },
    /// Apply enabled review ranges as one undoable ripple batch.
    #[serde(rename = "timeline.apply_autocuts")]
    ApplyAutocuts {
        /// Snapshot-bound proposal with per-cut toggles.
        proposal: crate::AutocutProposal,
    },
    /// List the project's timeline markers.
    #[serde(rename = "markers.list")]
    MarkersList,
    /// Add a timeline marker as one undo step.
    #[serde(rename = "markers.add")]
    MarkersAdd {
        /// Time, name, colour, note and optional span.
        params: crate::MarkerAddParams,
    },
    /// Remove a timeline marker as one undo step.
    #[serde(rename = "markers.remove")]
    MarkersRemove {
        /// Marker identity.
        id: animathio_core::MarkerId,
    },
    /// Analyse audio for tempo and beats without editing the project.
    #[serde(rename = "beats.detect")]
    BeatsDetect {
        /// Audio selection and tempo range.
        params: crate::BeatDetectParams,
    },
    /// Detect beats and add a marker at each (or every Nth, or each downbeat) as one undo step.
    #[serde(rename = "beats.add_markers")]
    BeatsAddMarkers {
        /// Audio selection, beat subset and marker style.
        params: crate::BeatMarkersParams,
    },
    /// Snap the selected clips' cut points to beats and/or markers, or split them there,
    /// as one undo step.
    #[serde(rename = "timeline.sync_cuts_to_beats")]
    SyncCutsToBeats {
        /// Clips, source, mode and tolerances.
        params: crate::SyncCutsParams,
    },
    /// Transcribe audio locally and add the result as caption clips on a new `Subtitles`
    /// track, as one undo step.
    #[serde(rename = "subtitles.generate")]
    SubtitlesGenerate {
        /// Model, language, selection and splitting options.
        params: crate::SubtitleGenerateParams,
    },
    /// Render a subtitle track as SRT or `WebVTT` text or file.
    #[serde(rename = "subtitles.export")]
    SubtitlesExport {
        /// Format, track and optional output path.
        params: crate::SubtitleExportParams,
    },
    /// Restyle the existing caption clips (style, colours, placement, words per line) as one
    /// undo step.
    #[serde(rename = "subtitles.style")]
    SubtitlesStyle {
        /// Style choice with optional overrides.
        params: crate::SubtitleStyleParams,
    },
    /// Add the cues of an SRT or `WebVTT` file as caption clips, as one undo step.
    #[serde(rename = "subtitles.import")]
    SubtitlesImport {
        /// File path and optional time offset.
        params: crate::SubtitleImportParams,
    },
}

/// Build a read-only timeline summary from a project.
#[must_use]
pub fn timeline_of(project: &Project) -> Timeline {
    Timeline {
        frame_rate: project.frame_rate(),
        duration: project.duration(),
        tracks: project
            .tracks()
            .iter()
            .map(|track| TrackInfo {
                id: track.id(),
                name: track.name().to_owned(),
                gain: track.gain(),
                clips: track
                    .clips()
                    .values()
                    .map(|clip| ClipInfo {
                        id: clip.id,
                        kind: clip_kind_label(&clip.kind),
                        start: clip.range.start,
                        end: clip.range.end,
                        source_offset: clip.source_offset,
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// Infer a declared media type from a filename extension; decoders probe the
/// actual bytes regardless of this hint.
#[must_use]
pub fn media_type_of(name: &str) -> &'static str {
    let Some(extension) = name.rsplit('.').next() else {
        return "application/octet-stream";
    };
    match extension.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mov" | "m4v" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "m4a" | "aac" => "audio/aac",
        "flac" => "audio/flac",
        _ => "application/octet-stream",
    }
}

/// Human-readable kind label for a clip source.
#[must_use]
pub fn clip_kind_label(kind: &animathio_core::ClipKind) -> String {
    match kind {
        animathio_core::ClipKind::Video(_) => "video".to_owned(),
        animathio_core::ClipKind::Image(_) => "image".to_owned(),
        animathio_core::ClipKind::Audio(_) => "audio".to_owned(),
        animathio_core::ClipKind::Plugin { plugin_id, .. } => match plugin_id.as_str() {
            "org.animathio.text" => "text".into(),
            "org.animathio.shapes" => "shape".into(),
            _ => format!("plugin {plugin_id}"),
        },
        animathio_core::ClipKind::Vector(_) => "vector".to_owned(),
        animathio_core::ClipKind::Error(_) => "error".to_owned(),
    }
}
