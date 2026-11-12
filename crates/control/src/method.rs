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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum Call {
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
    /// Read the timeline summary.
    #[serde(rename = "timeline.list")]
    TimelineList,
    /// Import a v1 `.animathio` project, replacing the current one.
    #[serde(rename = "project.import_v1")]
    ProjectImportV1 {
        /// v1 project path.
        path: PathBuf,
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
        animathio_core::ClipKind::Builtin(builtin) => match builtin {
            animathio_core::builtin::Builtin::Text { .. } => "text".to_owned(),
            animathio_core::builtin::Builtin::Shape { .. } => "shape".to_owned(),
        },
        animathio_core::ClipKind::Plugin { plugin_id, .. } => format!("plugin {plugin_id}"),
        animathio_core::ClipKind::Vector(_) => "vector".to_owned(),
        animathio_core::ClipKind::Error(_) => "error".to_owned(),
    }
}
