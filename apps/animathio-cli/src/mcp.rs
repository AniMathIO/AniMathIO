// The MCP surface maps v1's millisecond/pixel f64 values onto v2's integer
// ticks and normalized fractions, so the lossy-but-bounded casts below are
// intentional.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]

//! MCP stdio server over the control protocol.
//!
//! When `--project` is given it runs headless against that file; otherwise it
//! connects to a running editor's control socket. The tool surface covers
//! v1's 13 tools, adapted to v2 [`animathio_core::Command`]s, plus
//! `render_frame`, `undo`/`redo`, `set_param`, `add_keyframe`, `import_v1`, and the
//! plugin-driven `list_effects`/`add_effect` and `list_audio_effects`/`add_audio_effect`.

use std::{path::PathBuf, sync::Arc};

use animathio_control::{
    Call, ControlClient, ControlHandler, ExportPreset, GraphAddParams, HeadlessHandler,
    ImportManimParams, MathAddParams, ProjectState, RenderFrameParams, TitleStyle,
    TrimSilenceParams, default_socket_spec,
};
use animathio_core::{
    Clip, ClipId, Command, Interpolation, Keyframe, Project, Rounding, TICKS_PER_SECOND, Target,
    Time, TimeRange, Track, TrackId, Transform, TransformProperty, Transition, TransitionId,
    TransitionKind, WipeDirection,
};
use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::wrapper::Parameters,
    model::{
        CallToolResult, ContentBlock, Implementation, ProtocolVersion, ServerCapabilities,
        ServerConfig,
    },
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::Mutex;

/// Shared control backend: a connected editor or a headless project.
pub(crate) type Backend = Box<dyn ControlHandler + Send>;

/// Build the backend: headless on `--project`, else connect to the editor.
pub(crate) fn build_backend(project: Option<PathBuf>) -> Result<Backend, String> {
    if let Some(path) = project {
        return Ok(Box::new(
            HeadlessHandler::load(&path).map_err(|e| e.to_string())?,
        ));
    }
    match ControlClient::connect(&default_socket_spec().map_err(|e| e.to_string())?) {
        Ok(client) => Ok(Box::new(client)),
        Err(error) => Err(format!(
            "no running editor control socket found (pass --project for headless mode): {error}"
        )),
    }
}

/// The MCP server: all tool state is the shared control backend.
pub(crate) struct McpServer {
    backend: Arc<Mutex<Backend>>,
    /// Encoder preset chosen by `set_export_preset`, used by `export_video`.
    export_preset: std::sync::Mutex<Option<ExportPreset>>,
}

impl McpServer {
    pub(crate) fn new(backend: Backend) -> Self {
        Self {
            backend: Arc::new(Mutex::new(backend)),
            export_preset: std::sync::Mutex::new(None),
        }
    }
}

// ---- tool parameter schemas (schemars) -----------------------------------

#[derive(Deserialize, JsonSchema)]
pub(crate) struct AddText {
    pub(crate) text: String,
    #[serde(default = "default_font_size")]
    pub(crate) font_size: f64,
    pub(crate) x: Option<f64>,
    pub(crate) y: Option<f64>,
    pub(crate) color: Option<String>,
    #[serde(default = "default_duration_ms")]
    pub(crate) duration_ms: f64,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct AddMath {
    pub(crate) latex: String,
    pub(crate) mode: Option<String>,
    pub(crate) size: Option<f64>,
    pub(crate) color: Option<String>,
    pub(crate) x: Option<f64>,
    pub(crate) y: Option<f64>,
    pub(crate) start_ms: Option<f64>,
    pub(crate) duration_ms: Option<f64>,
    pub(crate) reveal_duration_ms: Option<f64>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct AddGraph {
    pub(crate) x_min: Option<f64>,
    pub(crate) x_max: Option<f64>,
    pub(crate) y_min: Option<f64>,
    pub(crate) y_max: Option<f64>,
    pub(crate) items: Option<String>,
    pub(crate) show_grid: Option<bool>,
    pub(crate) show_axes: Option<bool>,
    pub(crate) show_labels: Option<bool>,
    pub(crate) color: Option<String>,
    pub(crate) x: Option<f64>,
    pub(crate) y: Option<f64>,
    pub(crate) start_ms: Option<f64>,
    pub(crate) duration_ms: Option<f64>,
    pub(crate) draw_duration_ms: Option<f64>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct AddMedia {
    pub(crate) path: String,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct AddAnimation {
    pub(crate) target_id: String,
    pub(crate) kind: String,
    pub(crate) duration_ms: f64,
    pub(crate) direction: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct UpdateElement {
    pub(crate) id: String,
    pub(crate) placement: Option<Placement>,
    pub(crate) time_frame: Option<TimeFrame>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct Placement {
    pub(crate) x: Option<f64>,
    pub(crate) y: Option<f64>,
    pub(crate) width: Option<f64>,
    pub(crate) height: Option<f64>,
    pub(crate) rotation: Option<f64>,
    pub(crate) scale_x: Option<f64>,
    pub(crate) scale_y: Option<f64>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct TimeFrame {
    pub(crate) start: Option<f64>,
    pub(crate) end: Option<f64>,
}

/// Edit point: either two adjacent clip ids, or a time (plus optional track) that
/// selects the nearest cut.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct AddTransitionArgs {
    /// Track id; optional, narrows the `at_time_ms` search.
    pub(crate) track: Option<String>,
    /// Outgoing clip id (its end is the cut).
    pub(crate) between_clip_a: Option<String>,
    /// Incoming clip id (its start is the cut).
    pub(crate) clip_b: Option<String>,
    /// Pick the edit point nearest to this time instead of naming clips.
    pub(crate) at_time_ms: Option<f64>,
    /// `cross_dissolve` (default), `dip_to_black`, `dip_to_white` or `wipe`.
    pub(crate) kind: Option<String>,
    /// Wipe direction: `left_to_right` (default), `right_to_left`, `top_to_bottom`, `bottom_to_top`.
    pub(crate) direction: Option<String>,
    /// Total duration, centred on the cut.
    pub(crate) duration_ms: f64,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct ElementId {
    pub(crate) id: String,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct SetCanvas {
    pub(crate) width: Option<f64>,
    pub(crate) height: Option<f64>,
    pub(crate) background_color: Option<String>,
    pub(crate) max_time_ms: Option<f64>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct SetParam {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) value: f64,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct AddEffect {
    /// Clip id.
    pub(crate) id: String,
    /// Plugin id from `list_effects`, e.g. `org.animathio.effects-basic`.
    pub(crate) plugin: String,
    /// Effect id from `list_effects`, e.g. `grayscale`.
    pub(crate) effect: String,
    /// Parameter overrides by parameter id; other parameters keep their defaults.
    pub(crate) params: Option<std::collections::BTreeMap<String, f64>>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct AddAudioEffect {
    /// Clip id whose own audio chain to extend (give exactly one of `id` and `track`).
    pub(crate) id: Option<String>,
    /// Track id whose audio chain to extend; the effect then processes the sum of the track's
    /// clips before the track gain.
    pub(crate) track: Option<String>,
    /// Plugin id from `list_audio_effects`, e.g. `org.animathio.audio-basic`.
    pub(crate) plugin: String,
    /// Effect id from `list_audio_effects`, e.g. `compressor`.
    pub(crate) effect: String,
    /// Parameter overrides by parameter id; other parameters keep their defaults.
    pub(crate) params: Option<std::collections::BTreeMap<String, f64>>,
    /// Position in the chain; omit to append.
    pub(crate) index: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct AddKeyframe {
    pub(crate) id: String,
    pub(crate) target: String,
    pub(crate) time_ms: f64,
    pub(crate) value: f64,
    pub(crate) interpolation: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct Seek {
    pub(crate) time_ms: f64,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct SetPlaying {
    pub(crate) playing: bool,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct SaveProject {
    pub(crate) path: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct RenderFrame {
    pub(crate) frame: Option<i64>,
    pub(crate) time_ms: Option<f64>,
    pub(crate) width: Option<u32>,
    pub(crate) height: Option<u32>,
    pub(crate) output: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct ExportVideo {
    pub(crate) path: String,
    pub(crate) format: Option<String>,
    #[serde(default)]
    pub(crate) overwrite: bool,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct ImportV1 {
    pub(crate) path: String,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct ManimScene {
    pub(crate) path: Option<String>,
    pub(crate) source: Option<String>,
    pub(crate) script_dir: Option<String>,
    pub(crate) scene: Option<String>,
    #[serde(default)]
    pub(crate) replace: bool,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct CutAt {
    /// Timeline times in milliseconds. Every clip on every track that strictly
    /// contains one of these times is split there.
    pub(crate) times_ms: Vec<f64>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct RemoveRange {
    /// Inclusive start of the removed range, in timeline milliseconds.
    pub(crate) from_ms: f64,
    /// Exclusive end of the removed range, in timeline milliseconds.
    pub(crate) to_ms: f64,
    /// Shift everything after the range left to close the gap (default true).
    #[serde(default = "default_true")]
    pub(crate) ripple: bool,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct TrimSilence {
    /// Audio quieter than this RMS level (dBFS) counts as silence. Default -40;
    /// use -35 for noisy rooms, -50 for very clean recordings.
    #[serde(default = "default_threshold_db")]
    pub(crate) threshold_db: f64,
    /// Only pauses at least this long (milliseconds) are removed. Default 400.
    #[serde(default = "default_min_silence_ms")]
    pub(crate) min_silence_ms: f64,
    /// Silence kept on each side of every cut (milliseconds). Default 100.
    #[serde(default = "default_padding_ms")]
    pub(crate) padding_ms: f64,
    /// Numeric track id whose audio is analysed; omit to analyse the mix of all tracks.
    pub(crate) track: Option<u64>,
}

#[derive(Deserialize, JsonSchema)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct ProposeAutocuts {
    /// Silence entry threshold in dBFS (default -40).
    silence_db: Option<f64>,
    /// Minimum raw pause length in milliseconds (default 500).
    min_silence_ms: Option<f64>,
    /// Exit hysteresis in dB (default 3).
    hysteresis_db: Option<f64>,
    /// Use peak amplitude instead of RMS.
    #[serde(default)]
    peak: bool,
    /// Preserve pauses shorter than `min_silence_ms` (default true).
    keep_short_pauses: Option<bool>,
    /// Silence kept before speech in milliseconds (default 100).
    padding_before_ms: Option<f64>,
    /// Silence kept after speech in milliseconds (default 100).
    padding_after_ms: Option<f64>,
    /// Analyse a clip (video resolves its linked audio).
    clip: Option<u64>,
    /// Analyse one track's audio mix.
    track: Option<u64>,
    /// Inclusive start of the analysis range in milliseconds (requires `to_ms`).
    from_ms: Option<f64>,
    /// Exclusive end of the analysis range in milliseconds (requires `from_ms`).
    to_ms: Option<f64>,
    /// Cut every unlocked track instead of the selected tracks and companions.
    #[serde(default)]
    all_unlocked_tracks: bool,
    /// Include filler/repeat cuts; requires Whisper.
    #[serde(default)]
    fillers: bool,
    /// Language code; omitted means automatic detection.
    language: Option<String>,
    /// Filler phrase override; empty uses language defaults (English and Hungarian).
    #[serde(default)]
    filler_words: Vec<String>,
    /// Local Whisper model (default base).
    model: Option<String>,
    /// Explicit consent to download a missing model.
    #[serde(default)]
    download_model: bool,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct ApplyAutocuts {
    /// Exact proposal object returned by `propose_autocuts`. Toggle cuts[].enabled
    /// for review; range start/end and `total_saved` are integer timeline ticks.
    proposal: serde_json::Value,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct AutoSubtitles {
    /// Whisper model size: "tiny" (78 MB), "base" (148 MB, default) or "small" (488 MB).
    #[serde(default = "default_subtitle_model")]
    pub(crate) model: String,
    /// ISO 639-1 language code such as "en" or "de"; omit to detect it automatically.
    pub(crate) language: Option<String>,
    /// Numeric clip id: transcribe only this clip's audio.
    pub(crate) clip: Option<u64>,
    /// Numeric track id: transcribe only this track's audio.
    pub(crate) track: Option<u64>,
    /// Start of the range to transcribe, in timeline milliseconds (give with `to_ms`).
    pub(crate) from_ms: Option<f64>,
    /// End of the range to transcribe, in timeline milliseconds (give with `from_ms`).
    pub(crate) to_ms: Option<f64>,
    /// Download the model first if it is not installed. Downloads 78-488 MB once, verifies
    /// its SHA-256 and stores it in the user data dir; only set it with the user's consent.
    #[serde(default)]
    pub(crate) download_model: bool,
    /// Longest subtitle line in characters. Default 42.
    pub(crate) max_chars_per_line: Option<usize>,
    /// Most lines per subtitle. Default 2.
    pub(crate) max_lines: Option<usize>,
    /// Caption style: "classic" (default, white text on a translucent bar), "word-highlight"
    /// (active word on an accent pill), "karaoke" (words fill with the accent colour),
    /// "pop" (active word scales up), "one-word" (one big word at a time) or "bold-outline"
    /// (big uppercase type with a thick outline, centred on 9:16). Word timings from the
    /// recogniser drive the animation.
    pub(crate) style: Option<String>,
    #[serde(flatten)]
    pub(crate) look: CaptionLook,
}

/// Optional overrides of a caption style.
#[derive(Deserialize, JsonSchema, Default)]
pub(crate) struct CaptionLook {
    /// Font size relative to the shorter canvas dimension (0.01-0.2; style default, about 0.04-0.06).
    pub(crate) font_size: Option<f64>,
    /// Text colour as "#RRGGBB" or "#RRGGBBAA". Default white.
    pub(crate) color: Option<String>,
    /// Colour of the active word (pill, fill or pop), "#RRGGBB[AA]". Default yellow.
    pub(crate) accent: Option<String>,
    /// Outline colour, "#RRGGBB[AA]". Default black.
    pub(crate) outline: Option<String>,
    /// "top", "middle" or "bottom" inside the safe area (style default: bottom; bold-outline
    /// on a portrait canvas: middle).
    pub(crate) position: Option<String>,
    /// Words per line, 0 to fit the box width. Style default (bold-outline: 3).
    pub(crate) max_words: Option<u32>,
}

impl CaptionLook {
    fn style_params(
        &self,
        style: Option<&str>,
    ) -> Result<animathio_control::CaptionStyleParams, McpError> {
        let color = |text: &Option<String>| {
            text.as_deref()
                .map(animathio_control::parse_caption_color)
                .transpose()
                .map_err(mcp_error)
        };
        Ok(animathio_control::CaptionStyleParams {
            style: style.unwrap_or("classic").to_owned(),
            font_size: self.font_size,
            color: color(&self.color)?,
            accent: color(&self.accent)?,
            outline: color(&self.outline)?,
            position: self
                .position
                .as_deref()
                .map(animathio_control::CaptionPosition::parse)
                .transpose()
                .map_err(mcp_error)?,
            max_words: self.max_words,
        })
    }
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct SetCaptionStyle {
    /// Style id: "classic", "word-highlight", "karaoke", "pop", "one-word" or "bold-outline".
    pub(crate) style: String,
    /// Numeric id of the track holding the captions; default: every track named "Subtitles*"
    /// that has caption clips.
    pub(crate) track: Option<u64>,
    #[serde(flatten)]
    pub(crate) look: CaptionLook,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct ExportSubtitles {
    /// "srt" (default) or "vtt". A `.vtt` output path also selects `WebVTT`.
    pub(crate) format: Option<String>,
    /// Numeric id of the track holding the subtitle text clips; defaults to the first
    /// track named "Subtitles".
    pub(crate) track: Option<u64>,
    /// Write the file here instead of returning the text.
    pub(crate) path: Option<String>,
    /// Replace an existing file.
    #[serde(default)]
    pub(crate) overwrite: bool,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct ImportSubtitles {
    /// `.srt` or `.vtt` file to add as caption clips.
    pub(crate) path: String,
    /// Shift all cues by this many milliseconds (may be negative). Default 0.
    #[serde(default)]
    pub(crate) offset_ms: f64,
    /// Caption style (see `auto_subtitles`); words are spread evenly over each cue.
    pub(crate) style: Option<String>,
    #[serde(flatten)]
    pub(crate) look: CaptionLook,
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TitleStyleParam {
    /// Large centred title.
    Title,
    /// Medium text in the lower-left third.
    LowerThird,
    /// Small subtitle near the bottom; consecutive captions share one track.
    Caption,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct AddTitle {
    /// Composition id from `list_title_presets`; omitted keeps the legacy style behavior.
    pub(crate) preset: Option<String>,
    /// Named text fields from `list_title_presets`, e.g. name and role.
    #[serde(default)]
    pub(crate) fields: std::collections::BTreeMap<String, String>,
    /// Text to show; use \n for line breaks.
    #[serde(default)]
    pub(crate) text: String,
    /// Timeline start in milliseconds.
    pub(crate) start_ms: f64,
    /// How long the text stays on screen, in milliseconds.
    pub(crate) duration_ms: f64,
    /// Placement and size preset (default `title`).
    pub(crate) style: Option<TitleStyleParam>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct Batch {
    /// Core commands in serde's externally tagged JSON form, applied in order as one
    /// undo step. Times are integer ticks (705600000 per second); ids are numbers.
    /// Examples: `{"RemoveClip":{"id":3}}`, `{"MoveClip":{"id":3,"track":1,"start":0}}`,
    /// `{"SplitAt":{"times":[705600000]}}`,
    /// `{"RemoveRange":{"range":{"start":0,"end":352800000},"ripple":true}}`,
    /// `{"RippleDelete":{"clip":4}}`, `{"SetTrackGain":{"id":1,"gain":0.5}}`,
    /// `{"RenameTrack":{"id":1,"name":"Titles"}}`, `{"MoveTrack":{"id":1,"index":0}}`.
    pub(crate) ops: Vec<serde_json::Value>,
}

/// A schema adapter; the actual names and deserialization come from the shared table.
#[derive(Deserialize)]
#[serde(transparent)]
pub(crate) struct ExportPresetParam(ExportPreset);
impl JsonSchema for ExportPresetParam {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ExportPreset".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type": "string", "enum": ExportPreset::ALL.map(ExportPreset::name)})
    }
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct SetExportPreset {
    /// Delivery preset.
    pub(crate) preset: ExportPresetParam,
}

// ---- helpers --------------------------------------------------------------

fn default_true() -> bool {
    true
}
fn default_subtitle_model() -> String {
    "base".to_owned()
}
fn default_threshold_db() -> f64 {
    -40.0
}
fn default_min_silence_ms() -> f64 {
    400.0
}
fn default_padding_ms() -> f64 {
    100.0
}

fn ticks_to_ms(time: Time) -> f64 {
    time.0 as f64 * 1000.0 / TICKS_PER_SECOND as f64
}

fn subtitle_result_json(result: &animathio_control::SubtitleResult) -> serde_json::Value {
    json!({
        "language": result.language,
        "track": result.added.map(|a| a.track_id.0),
        "background_track": result.added.and_then(|a| a.background_track_id).map(|id| id.0),
        "first_clip": result.added.map(|a| a.first_clip.0),
        "cues": result.cues.iter().map(|c| json!({
            "from_ms": ticks_to_ms(c.start),
            "to_ms": ticks_to_ms(c.end),
            "text": c.text,
        })).collect::<Vec<_>>(),
    })
}

fn range_ms(from_ms: f64, to_ms: f64) -> Result<TimeRange, McpError> {
    TimeRange::new(ms_to_ticks(from_ms)?, ms_to_ticks(to_ms)?)
        .map_err(|_| invalid("range end must be after its start"))
}

fn clip_count(project: &Project) -> usize {
    project.tracks().iter().map(|t| t.clips().len()).sum()
}

fn default_font_size() -> f64 {
    32.0
}
fn default_duration_ms() -> f64 {
    3000.0
}

#[allow(clippy::needless_pass_by_value)] // map_err adapter: to_string() only borrows
fn mcp_error(error: animathio_control::ControlError) -> McpError {
    McpError::internal_error(error.to_string(), None)
}

fn unsupported(message: &str) -> McpError {
    McpError::internal_error(format!("not yet supported: {message}"), None)
}

fn invalid(message: &str) -> McpError {
    McpError::invalid_params(message.to_owned(), None)
}

fn ms_to_ticks(ms: f64) -> Result<Time, McpError> {
    if !ms.is_finite() || ms < 0.0 {
        return Err(invalid(
            "time must be a nonnegative finite number of milliseconds",
        ));
    }
    let ticks = (ms * TICKS_PER_SECOND as f64 / 1000.0).round();
    if ticks > i64::MAX as f64 {
        return Err(invalid("time out of range"));
    }
    Ok(Time(ticks as i64))
}

fn canvas(state: &ProjectState) -> (u32, u32) {
    (
        if state.project.width() == 0 {
            640
        } else {
            state.project.width()
        },
        if state.project.height() == 0 {
            360
        } else {
            state.project.height()
        },
    )
}

fn project_duration(state: &ProjectState) -> Time {
    if state.project.duration().0 > 0 {
        state.project.duration()
    } else {
        Time(TICKS_PER_SECOND * 3)
    }
}

fn next_id(project: &Project) -> u64 {
    let mut max = 0_u64;
    for track in project.tracks() {
        max = max.max(track.id().0);
        for clip in track.clips().keys() {
            max = max.max(clip.0);
        }
    }
    max.saturating_add(1)
}

fn find_clip(project: &Project, id: ClipId) -> Option<&Clip> {
    project
        .tracks()
        .iter()
        .flat_map(|t| t.clips().values())
        .find(|c| c.id == id)
}

fn parse_clip_id(id: &str) -> Result<ClipId, McpError> {
    id.parse::<u64>()
        .map(ClipId)
        .map_err(|_| invalid("id must be a numeric clip id"))
}

fn parse_color(value: &str) -> Option<[u8; 4]> {
    let hex = value.trim().strip_prefix('#')?;
    if !hex.is_ascii() {
        return None;
    }
    let byte = |i: usize, j: usize| u8::from_str_radix(&hex[i..j], 16).ok();
    match hex.len() {
        6 => Some([byte(0, 2)?, byte(2, 4)?, byte(4, 6)?, 255]),
        8 => Some([byte(0, 2)?, byte(2, 4)?, byte(4, 6)?, byte(6, 8)?]),
        _ => None,
    }
}

fn parse_target(target: &str) -> Target {
    match target {
        "x" => Target::Transform(TransformProperty::X),
        "y" => Target::Transform(TransformProperty::Y),
        "scale_x" => Target::Transform(TransformProperty::ScaleX),
        "scale_y" => Target::Transform(TransformProperty::ScaleY),
        "rotation" => Target::Transform(TransformProperty::Rotation),
        "opacity" => Target::Transform(TransformProperty::Opacity),
        other => Target::Param(other.to_owned()),
    }
}

fn parse_interpolation(value: Option<&str>) -> Interpolation {
    match value {
        Some("hold") => Interpolation::Hold,
        Some("ease_in_out") => Interpolation::EaseInOut,
        _ => Interpolation::Linear,
    }
}

fn parse_transition_kind(
    kind: Option<&str>,
    direction: Option<&str>,
) -> Result<TransitionKind, McpError> {
    Ok(match kind.unwrap_or("cross_dissolve") {
        "cross_dissolve" | "dissolve" => TransitionKind::CrossDissolve,
        "dip_to_black" => TransitionKind::DipToBlack,
        "dip_to_white" => TransitionKind::DipToWhite,
        "wipe" => TransitionKind::Wipe {
            direction: match direction.unwrap_or("left_to_right") {
                "left_to_right" => WipeDirection::LeftToRight,
                "right_to_left" => WipeDirection::RightToLeft,
                "top_to_bottom" => WipeDirection::TopToBottom,
                "bottom_to_top" => WipeDirection::BottomToTop,
                _ => return Err(invalid("unknown wipe direction")),
            },
        },
        _ => {
            return Err(invalid(
                "kind must be cross_dissolve, dip_to_black, dip_to_white or wipe",
            ));
        }
    })
}

/// Resolve `(track, outgoing, incoming)` for an edit point.
fn find_edit_point(
    project: &Project,
    args: &AddTransitionArgs,
) -> Result<(TrackId, ClipId, ClipId), McpError> {
    let track_filter = args
        .track
        .as_deref()
        .map(|t| t.parse::<u64>().map(TrackId))
        .transpose()
        .map_err(|_| invalid("track must be a numeric track id"))?;
    let tracks = project
        .tracks()
        .iter()
        .filter(|t| track_filter.is_none_or(|id| t.id() == id));
    if let (Some(a), Some(b)) = (&args.between_clip_a, &args.clip_b) {
        let (a, b) = (parse_clip_id(a)?, parse_clip_id(b)?);
        return tracks
            .into_iter()
            .find(|t| t.clips().contains_key(&a) && t.clips().contains_key(&b))
            .map(|t| (t.id(), a, b))
            .ok_or_else(|| invalid("both clips must be on one track"));
    }
    let at = ms_to_ticks(
        args.at_time_ms
            .ok_or_else(|| invalid("give between_clip_a + clip_b, or at_time_ms"))?,
    )?;
    tracks
        .flat_map(|track| {
            track.clips().values().filter_map(move |a| {
                track
                    .clips()
                    .values()
                    .find(|b| b.range.start == a.range.end)
                    .map(|b| {
                        (
                            track.id(),
                            a.id,
                            b.id,
                            (a.range.end.0 - at.0).unsigned_abs(),
                        )
                    })
            })
        })
        .min_by_key(|candidate| candidate.3)
        .map(|(track, a, b, _)| (track, a, b))
        .ok_or_else(|| invalid("no edit point between adjacent clips found"))
}

fn tool_text(value: impl serde::Serialize) -> Result<CallToolResult, McpError> {
    let text = serde_json::to_string(&value).map_err(|e| mcp_error(e.into()))?;
    Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
}

// ---- tool router ----------------------------------------------------------

#[tool_router]
impl McpServer {
    #[tool(
        description = "Inspect the open project: canvas, tracks, clips, playhead and whether the editor is active."
    )]
    async fn get_project_state(&self) -> Result<CallToolResult, McpError> {
        let mut backend = self.backend.lock().await;
        let state = backend.project_get().map_err(mcp_error)?;
        tool_text(state)
    }

    #[tool(
        description = "Add one plain-text clip. Font size is in pixels; omitted x/y centres the clip; times are milliseconds."
    )]
    async fn add_text(
        &self,
        Parameters(p): Parameters<AddText>,
    ) -> Result<CallToolResult, McpError> {
        let mut backend = self.backend.lock().await;
        let state = backend.project_get().map_err(mcp_error)?;
        let (width, height) = canvas(&state);
        let duration = if p.duration_ms > 0.0 {
            ms_to_ticks(p.duration_ms)?
        } else {
            project_duration(&state)
        };
        let font_size = (p.font_size / f64::from(height)).clamp(1e-6, 16.0);
        let color = p
            .color
            .as_deref()
            .and_then(parse_color)
            .unwrap_or([255, 255, 255, 255]);
        let transform = Transform {
            x: p.x.map_or(0.5, |x| x / f64::from(width)),
            y: p.y.map_or(0.5, |y| y / f64::from(height)),
            ..Transform::default()
        };
        let base = next_id(&state.project);
        let track_id = TrackId(base);
        let clip_id = ClipId(base + 1);
        let mut clip = Clip::new(
            clip_id,
            animathio_plugin_api::sources::text(p.text, font_size, color),
            TimeRange::new(Time(0), duration).map_err(|e| mcp_error(e.into()))?,
        );
        clip.transform = transform;
        backend
            .command_apply(Command::AddTrack {
                track: Track::new(track_id, "Text"),
                index: state.project.tracks().len(),
            })
            .map_err(mcp_error)?;
        backend
            .command_apply(Command::AddClip {
                track: track_id,
                clip,
            })
            .map_err(mcp_error)?;
        tool_text(json!({"track_id": track_id.0, "clip_id": clip_id.0}))
    }

    #[tool(
        description = "Add a LaTeX fragment as an org.animathio.latex plugin clip, with an optional Write-style reveal animation. Coordinates are pixels, times are milliseconds."
    )]
    async fn add_math(
        &self,
        Parameters(p): Parameters<AddMath>,
    ) -> Result<CallToolResult, McpError> {
        let mut backend = self.backend.lock().await;
        let state = backend.project_get().map_err(mcp_error)?;
        let (width, height) = canvas(&state);
        let start = p.start_ms.map(ms_to_ticks).transpose()?.unwrap_or(Time(0));
        let duration = match p.duration_ms {
            Some(ms) => ms_to_ticks(ms)?,
            None => project_duration(&state),
        };
        let reveal_duration = p.reveal_duration_ms.map(ms_to_ticks).transpose()?;
        let result = backend.math_add(MathAddParams {
            latex: p.latex,
            mode: p.mode.unwrap_or_else(|| "latex".to_owned()),
            size: p.size.unwrap_or(48.0),
            color: p
                .color
                .as_deref()
                .map(|color| {
                    parse_color(color).ok_or_else(|| invalid("color must be #RRGGBB or #RRGGBBAA"))
                })
                .transpose()?,
            x: p.x.map(|x| x / f64::from(width)),
            y: p.y.map(|y| y / f64::from(height)),
            start,
            duration: Some(duration),
            reveal_duration,
        });
        match result {
            Ok(result) => tool_text(result),
            Err(error) => Ok(CallToolResult::error(vec![ContentBlock::text(
                error.to_string(),
            )])),
        }
    }

    #[tool(
        description = "Add an org.animathio.graph coordinate plane (bounds, items) with an optional draw animation. Coordinates are pixels, times are milliseconds."
    )]
    async fn add_graph(
        &self,
        Parameters(p): Parameters<AddGraph>,
    ) -> Result<CallToolResult, McpError> {
        let mut backend = self.backend.lock().await;
        let state = backend.project_get().map_err(mcp_error)?;
        let (width, height) = canvas(&state);
        let start = p.start_ms.map(ms_to_ticks).transpose()?.unwrap_or(Time(0));
        let duration = match p.duration_ms {
            Some(ms) => ms_to_ticks(ms)?,
            None => project_duration(&state),
        };
        let draw_duration = p.draw_duration_ms.map(ms_to_ticks).transpose()?;
        let result = backend.graph_add(GraphAddParams {
            x_min: p.x_min,
            x_max: p.x_max,
            y_min: p.y_min,
            y_max: p.y_max,
            items: p.items.unwrap_or_else(|| "plot y=x^2".to_owned()),
            show_grid: p.show_grid,
            show_axes: p.show_axes,
            show_labels: p.show_labels,
            color: p
                .color
                .as_deref()
                .map(|color| {
                    parse_color(color).ok_or_else(|| invalid("color must be #RRGGBB or #RRGGBBAA"))
                })
                .transpose()?,
            x: p.x.map(|x| x / f64::from(width)),
            y: p.y.map(|y| y / f64::from(height)),
            start,
            duration: Some(duration),
            draw_duration,
        });
        match result {
            Ok(result) => tool_text(result),
            Err(error) => Ok(CallToolResult::error(vec![ContentBlock::text(
                error.to_string(),
            )])),
        }
    }

    #[tool(
        description = "Import an image, video, or audio file by absolute path. Video with audio gets linked video and audio clips; the complete import is one undo step."
    )]
    async fn add_media(
        &self,
        Parameters(p): Parameters<AddMedia>,
    ) -> Result<CallToolResult, McpError> {
        let mut backend = self.backend.lock().await;
        tool_text(
            backend
                .handle(Call::MediaAdd {
                    path: PathBuf::from(p.path),
                })
                .map_err(mcp_error)?,
        )
    }

    #[tool(
        description = "Attach an animation to a clip. Supported kinds: fadeIn, fadeOut, slideIn, slideOut."
    )]
    async fn add_animation(
        &self,
        Parameters(p): Parameters<AddAnimation>,
    ) -> Result<CallToolResult, McpError> {
        let mut backend = self.backend.lock().await;
        let state = backend.project_get().map_err(mcp_error)?;
        let clip_id = parse_clip_id(&p.target_id)?;
        let clip =
            find_clip(&state.project, clip_id).ok_or_else(|| invalid("target clip not found"))?;
        let duration = ms_to_ticks(p.duration_ms)?;
        let opacity = clip.transform.opacity;
        match p.kind.as_str() {
            "fadeIn" => {
                backend
                    .command_apply(Command::AddKeyframe {
                        id: clip_id,
                        target: Target::Transform(TransformProperty::Opacity),
                        time: Time(0),
                        keyframe: Keyframe {
                            value: (0.0).into(),
                            interpolation: Interpolation::Linear,
                        },
                    })
                    .map_err(mcp_error)?;
                backend
                    .command_apply(Command::AddKeyframe {
                        id: clip_id,
                        target: Target::Transform(TransformProperty::Opacity),
                        time: duration,
                        keyframe: Keyframe {
                            value: opacity.into(),
                            interpolation: Interpolation::Linear,
                        },
                    })
                    .map_err(mcp_error)?;
            }
            "fadeOut" => {
                backend
                    .command_apply(Command::AddKeyframe {
                        id: clip_id,
                        target: Target::Transform(TransformProperty::Opacity),
                        time: Time(0),
                        keyframe: Keyframe {
                            value: opacity.into(),
                            interpolation: Interpolation::Linear,
                        },
                    })
                    .map_err(mcp_error)?;
                backend
                    .command_apply(Command::AddKeyframe {
                        id: clip_id,
                        target: Target::Transform(TransformProperty::Opacity),
                        time: duration,
                        keyframe: Keyframe {
                            value: (0.0).into(),
                            interpolation: Interpolation::Linear,
                        },
                    })
                    .map_err(mcp_error)?;
            }
            "slideIn" | "slideOut" => {
                let (x, y) = (clip.transform.x, clip.transform.y);
                let direction = p.direction.as_deref().unwrap_or("left");
                let offset = match direction {
                    "left" => (-1.0, 0.0),
                    "right" => (1.0, 0.0),
                    "top" => (0.0, -1.0),
                    "bottom" => (0.0, 1.0),
                    _ => return Err(invalid("direction must be left, right, top or bottom")),
                };
                let (from, to) = if p.kind == "slideIn" {
                    ((x + offset.0, y + offset.1), (x, y))
                } else {
                    ((x, y), (x + offset.0, y + offset.1))
                };
                backend
                    .command_apply(Command::AddKeyframe {
                        id: clip_id,
                        target: Target::Transform(TransformProperty::X),
                        time: Time(0),
                        keyframe: Keyframe {
                            value: from.0.into(),
                            interpolation: Interpolation::Linear,
                        },
                    })
                    .map_err(mcp_error)?;
                backend
                    .command_apply(Command::AddKeyframe {
                        id: clip_id,
                        target: Target::Transform(TransformProperty::X),
                        time: duration,
                        keyframe: Keyframe {
                            value: to.0.into(),
                            interpolation: Interpolation::Linear,
                        },
                    })
                    .map_err(mcp_error)?;
                backend
                    .command_apply(Command::AddKeyframe {
                        id: clip_id,
                        target: Target::Transform(TransformProperty::Y),
                        time: Time(0),
                        keyframe: Keyframe {
                            value: from.1.into(),
                            interpolation: Interpolation::Linear,
                        },
                    })
                    .map_err(mcp_error)?;
                backend
                    .command_apply(Command::AddKeyframe {
                        id: clip_id,
                        target: Target::Transform(TransformProperty::Y),
                        time: duration,
                        keyframe: Keyframe {
                            value: to.1.into(),
                            interpolation: Interpolation::Linear,
                        },
                    })
                    .map_err(mcp_error)?;
            }
            other => {
                return Err(unsupported(&format!(
                    "animation kind '{other}'; only fadeIn/fadeOut/slideIn/slideOut are available"
                )));
            }
        }
        tool_text(json!({"clip_id": clip_id.0}))
    }

    #[tool(
        description = "Update a clip's placement (pixels, origin top-left) and/or visible time frame (milliseconds)."
    )]
    async fn update_element(
        &self,
        Parameters(p): Parameters<UpdateElement>,
    ) -> Result<CallToolResult, McpError> {
        let mut backend = self.backend.lock().await;
        let state = backend.project_get().map_err(mcp_error)?;
        let clip_id = parse_clip_id(&p.id)?;
        let clip = find_clip(&state.project, clip_id).ok_or_else(|| invalid("clip not found"))?;
        let (width, height) = canvas(&state);
        if let Some(placement) = p.placement {
            let mut transform = clip.transform;
            if let Some(x) = placement.x {
                transform.x = x / f64::from(width);
            }
            if let Some(y) = placement.y {
                transform.y = y / f64::from(height);
            }
            if let Some(w) = placement.width {
                transform.scale_x = w / f64::from(width);
            }
            if let Some(h) = placement.height {
                transform.scale_y = h / f64::from(height);
            }
            if let Some(rotation) = placement.rotation {
                transform.rotation = rotation.to_radians();
            }
            if let Some(scale_x) = placement.scale_x {
                transform.scale_x = scale_x;
            }
            if let Some(scale_y) = placement.scale_y {
                transform.scale_y = scale_y;
            }
            backend
                .command_apply(Command::SetTransform {
                    id: clip_id,
                    transform,
                })
                .map_err(mcp_error)?;
        }
        if let Some(time_frame) = p.time_frame {
            let start = time_frame
                .start
                .map(ms_to_ticks)
                .transpose()?
                .unwrap_or(clip.range.start);
            let end = time_frame
                .end
                .map(ms_to_ticks)
                .transpose()?
                .unwrap_or(clip.range.end);
            backend
                .command_apply(Command::TrimClip {
                    id: clip_id,
                    range: TimeRange::new(start, end).map_err(|e| mcp_error(e.into()))?,
                    source_offset: clip.source_offset,
                })
                .map_err(mcp_error)?;
        }
        tool_text(json!({"clip_id": clip_id.0}))
    }

    #[tool(description = "Remove a clip by id.")]
    async fn remove_element(
        &self,
        Parameters(p): Parameters<ElementId>,
    ) -> Result<CallToolResult, McpError> {
        let mut backend = self.backend.lock().await;
        let clip_id = parse_clip_id(&p.id)?;
        backend
            .command_apply(Command::RemoveClip { id: clip_id })
            .map_err(mcp_error)?;
        tool_text(json!({"removed": true, "clip_id": clip_id.0}))
    }

    #[tool(
        description = "Set canvas width/height (pixels), background color (#RRGGBB or #RRGGBBAA) and duration (milliseconds)."
    )]
    async fn set_canvas(
        &self,
        Parameters(p): Parameters<SetCanvas>,
    ) -> Result<CallToolResult, McpError> {
        let mut backend = self.backend.lock().await;
        let state = backend.project_get().map_err(mcp_error)?;
        let width = p.width.map_or(state.project.width(), |w| w as u32);
        let height = p.height.map_or(state.project.height(), |h| h as u32);
        let background = p
            .background_color
            .as_deref()
            .and_then(parse_color)
            .unwrap_or(state.project.background());
        let duration = p
            .max_time_ms
            .map(ms_to_ticks)
            .transpose()?
            .unwrap_or(state.project.duration());
        backend
            .command_apply(Command::SetSettings {
                width,
                height,
                frame_rate: state.project.frame_rate(),
                background,
                duration,
            })
            .map_err(mcp_error)?;
        tool_text(json!({"width": width, "height": height, "duration_ms": p.max_time_ms}))
    }

    #[tool(description = "Set or change a clip's named numeric parameter.")]
    async fn set_param(
        &self,
        Parameters(p): Parameters<SetParam>,
    ) -> Result<CallToolResult, McpError> {
        let mut backend = self.backend.lock().await;
        let clip_id = parse_clip_id(&p.id)?;
        backend
            .command_apply(Command::SetParam {
                id: clip_id,
                name: p.name,
                value: Some(p.value),
            })
            .map_err(mcp_error)?;
        tool_text(json!({"clip_id": clip_id.0}))
    }

    #[tool(
        description = "List the clip effects provided by loaded effect plugins: plugin id, effect id, name, description and numeric parameters (id, min, max, default). Pass the plugin and effect ids to add_effect."
    )]
    async fn list_effects(&self) -> Result<CallToolResult, McpError> {
        let mut backend = self.backend.lock().await;
        let effects = backend.effects_list().map_err(mcp_error)?;
        tool_text(json!({"effects": effects}))
    }

    #[tool(
        description = "Add a pixel effect to a clip as one undo step. Effects come from plugins; call list_effects for the available plugin/effect ids and their parameters. Unspecified parameters keep the plugin default. Effects apply in chain order after the clip is drawn. Returns the effect id."
    )]
    async fn add_effect(
        &self,
        Parameters(p): Parameters<AddEffect>,
    ) -> Result<CallToolResult, McpError> {
        let clip = parse_clip_id(&p.id)?;
        let mut backend = self.backend.lock().await;
        let added = backend
            .effects_add(animathio_control::EffectAddParams {
                clip,
                plugin: p.plugin,
                effect: p.effect,
                params: p.params.unwrap_or_default(),
                index: None,
            })
            .map_err(mcp_error)?;
        tool_text(json!({"clip_id": added.clip.0, "effect_id": added.effect_id.0}))
    }

    #[tool(
        description = "List the audio effects provided by loaded audio-effect plugins: plugin id, effect id, name, description and numeric parameters (id, min, max, default). Pass the plugin and effect ids to add_audio_effect."
    )]
    async fn list_audio_effects(&self) -> Result<CallToolResult, McpError> {
        let mut backend = self.backend.lock().await;
        let effects = backend.audio_effects_list().map_err(mcp_error)?;
        tool_text(json!({"effects": effects}))
    }

    #[tool(
        description = "Add an audio effect to a clip's own chain (id) or to a track's chain (track) as one undo step. Effects come from plugins; call list_audio_effects for the available plugin/effect ids and their parameters in dB, Hz and ms. Unspecified parameters keep the plugin default. Effects apply in chain order: clip chains right after the clip's gain, track chains to the summed track before its gain. Returns the effect id."
    )]
    async fn add_audio_effect(
        &self,
        Parameters(p): Parameters<AddAudioEffect>,
    ) -> Result<CallToolResult, McpError> {
        let clip = p.id.as_deref().map(parse_clip_id).transpose()?;
        let track = p
            .track
            .as_deref()
            .map(|id| {
                id.parse::<u64>()
                    .map(TrackId)
                    .map_err(|_| invalid("track must be a numeric track id"))
            })
            .transpose()?;
        let mut backend = self.backend.lock().await;
        let added = backend
            .audio_effects_add(animathio_control::AudioEffectAddParams {
                clip,
                track,
                plugin: p.plugin,
                effect: p.effect,
                params: p.params.unwrap_or_default(),
                index: p.index,
            })
            .map_err(mcp_error)?;
        let chain = match added.chain {
            animathio_core::AudioChain::Clip(id) => json!({"clip_id": id.0}),
            animathio_core::AudioChain::Track(id) => json!({"track_id": id.0}),
        };
        tool_text(json!({"chain": chain, "effect_id": added.effect_id.0}))
    }

    #[tool(
        description = "Add a keyframe to a clip. The target is a transform property (x, y, scale_x, scale_y, rotation, opacity) or a parameter name."
    )]
    async fn add_keyframe(
        &self,
        Parameters(p): Parameters<AddKeyframe>,
    ) -> Result<CallToolResult, McpError> {
        let mut backend = self.backend.lock().await;
        let clip_id = parse_clip_id(&p.id)?;
        let target = parse_target(&p.target);
        let interpolation = parse_interpolation(p.interpolation.as_deref());
        backend
            .command_apply(Command::AddKeyframe {
                id: clip_id,
                target,
                time: ms_to_ticks(p.time_ms)?,
                keyframe: Keyframe {
                    value: p.value.into(),
                    interpolation,
                },
            })
            .map_err(mcp_error)?;
        tool_text(json!({"clip_id": clip_id.0}))
    }

    #[tool(
        description = "Add a transition (cross_dissolve, dip_to_black, dip_to_white or wipe) on the cut between two adjacent clips, named by clip ids or by the nearest cut to at_time_ms. The duration is centred on the cut."
    )]
    async fn add_transition(
        &self,
        Parameters(p): Parameters<AddTransitionArgs>,
    ) -> Result<CallToolResult, McpError> {
        let mut backend = self.backend.lock().await;
        let state = backend.project_get().map_err(mcp_error)?;
        let kind = parse_transition_kind(p.kind.as_deref(), p.direction.as_deref())?;
        let duration = ms_to_ticks(p.duration_ms)?;
        let (track, from, to) = find_edit_point(&state.project, &p)?;
        let id = TransitionId(
            state
                .project
                .tracks()
                .iter()
                .flat_map(Track::transitions)
                .map(|t| t.id.0.saturating_add(1))
                .max()
                .unwrap_or(0),
        );
        backend
            .command_apply(Command::AddTransition {
                track,
                transition: Transition {
                    id,
                    kind,
                    duration,
                    from,
                    to,
                },
            })
            .map_err(mcp_error)?;
        tool_text(json!({"transition_id": id.0, "track_id": track.0, "from": from.0, "to": to.0}))
    }

    #[tool(
        description = "List every transition with its track, clips, kind, cut and duration (milliseconds)."
    )]
    async fn list_transitions(&self) -> Result<CallToolResult, McpError> {
        let mut backend = self.backend.lock().await;
        let state = backend.project_get().map_err(mcp_error)?;
        let ms = |t: Time| t.0 as f64 * 1000.0 / TICKS_PER_SECOND as f64;
        let list: Vec<_> = state
            .project
            .tracks()
            .iter()
            .flat_map(|track| {
                track.transitions().iter().map(move |t| {
                    let cut = track.clips().get(&t.from).map(|c| c.range.end);
                    json!({
                        "id": t.id.0,
                        "track_id": track.id().0,
                        "from": t.from.0,
                        "to": t.to.0,
                        "kind": t.kind,
                        "duration_ms": ms(t.duration),
                        "cut_ms": cut.map(ms),
                    })
                })
            })
            .collect();
        tool_text(json!({"transitions": list}))
    }

    #[tool(description = "Undo the last edit.")]
    async fn undo(&self) -> Result<CallToolResult, McpError> {
        let mut backend = self.backend.lock().await;
        let undone = backend.history_undo().map_err(mcp_error)?;
        tool_text(json!({"undone": undone}))
    }

    #[tool(description = "Redo the last undone edit.")]
    async fn redo(&self) -> Result<CallToolResult, McpError> {
        let mut backend = self.backend.lock().await;
        let redone = backend.history_redo().map_err(mcp_error)?;
        tool_text(json!({"redone": redone}))
    }

    #[tool(description = "Move the playhead to a timeline time in milliseconds.")]
    async fn seek(&self, Parameters(p): Parameters<Seek>) -> Result<CallToolResult, McpError> {
        let mut backend = self.backend.lock().await;
        backend
            .playback_seek(ms_to_ticks(p.time_ms)?)
            .map_err(mcp_error)?;
        tool_text(json!({"time_ms": p.time_ms}))
    }

    #[tool(description = "Start or pause timeline playback.")]
    async fn set_playing(
        &self,
        Parameters(p): Parameters<SetPlaying>,
    ) -> Result<CallToolResult, McpError> {
        let mut backend = self.backend.lock().await;
        if p.playing {
            backend.playback_play().map_err(mcp_error)?;
        } else {
            backend.playback_pause().map_err(mcp_error)?;
        }
        tool_text(json!({"playing": p.playing}))
    }

    #[tool(description = "Save the project to a path, or overwrite the current file when omitted.")]
    async fn save_project(
        &self,
        Parameters(p): Parameters<SaveProject>,
    ) -> Result<CallToolResult, McpError> {
        let mut backend = self.backend.lock().await;
        let path = backend
            .project_save(p.path.map(PathBuf::from))
            .map_err(mcp_error)?;
        tool_text(json!({"path": path}))
    }

    #[tool(description = "Render one frame to a PNG (returned as base64, or written to a path).")]
    async fn render_frame(
        &self,
        Parameters(p): Parameters<RenderFrame>,
    ) -> Result<CallToolResult, McpError> {
        let mut backend = self.backend.lock().await;
        let state = backend.project_get().map_err(mcp_error)?;
        let (width, height) = canvas(&state);
        let time = if let Some(frame) = p.frame {
            state
                .project
                .frame_rate()
                .frame_to_time(frame, Rounding::Nearest)
                .map_err(|e| mcp_error(e.into()))?
        } else if let Some(time_ms) = p.time_ms {
            ms_to_ticks(time_ms)?
        } else {
            state.playhead
        };
        let result = backend
            .render_frame(RenderFrameParams {
                time,
                width: p.width.unwrap_or(width),
                height: p.height.unwrap_or(height),
                output: p.output.map(PathBuf::from),
            })
            .map_err(mcp_error)?;
        tool_text(json!({
            "size": result.size,
            "base64": result.base64,
            "path": result.path,
        }))
    }

    #[tool(
        description = "Describe the timeline as concise text: resolution, frame rate, duration, then every track with its clips (id, start - end in HH:MM:SS.mmm, kind, name, source offset) and the gaps between them. Call this first, and again after edits, to reason about the timeline. Other tools take milliseconds: 00:01:02.500 is 62500 ms."
    )]
    async fn describe_timeline(&self) -> Result<CallToolResult, McpError> {
        let mut backend = self.backend.lock().await;
        let state = backend.project_get().map_err(mcp_error)?;
        Ok(CallToolResult::success(vec![ContentBlock::text(
            animathio_control::describe_timeline(&state.project),
        )]))
    }

    #[tool(
        description = "Split (razor) every clip on every track at each given time, as one undo step. Clips are only split where a time falls strictly inside them. Use with remove_element, or prefer remove_range to cut a section out directly."
    )]
    async fn cut_at(&self, Parameters(p): Parameters<CutAt>) -> Result<CallToolResult, McpError> {
        let times = p
            .times_ms
            .iter()
            .map(|ms| ms_to_ticks(*ms))
            .collect::<Result<Vec<_>, _>>()?;
        let mut backend = self.backend.lock().await;
        let before = clip_count(&backend.project_get().map_err(mcp_error)?.project);
        backend
            .command_apply(Command::SplitAt { times })
            .map_err(mcp_error)?;
        let after = clip_count(&backend.project_get().map_err(mcp_error)?.project);
        tool_text(json!({"new_clips": after - before}))
    }

    #[tool(
        description = "Remove everything between from_ms and to_ms on all tracks, as one undo step: clips inside are deleted and clips crossing the edges are trimmed or split. With ripple=true (default) later content moves left so no gap is left; with ripple=false a gap remains."
    )]
    async fn remove_range(
        &self,
        Parameters(p): Parameters<RemoveRange>,
    ) -> Result<CallToolResult, McpError> {
        let range = range_ms(p.from_ms, p.to_ms)?;
        let mut backend = self.backend.lock().await;
        backend
            .command_apply(Command::RemoveRange {
                range,
                ripple: p.ripple,
            })
            .map_err(mcp_error)?;
        tool_text(
            json!({"removed_ms": ticks_to_ms(Time(range.end.0 - range.start.0)), "ripple": p.ripple}),
        )
    }

    #[tool(
        description = "Delete one clip and close the gap it leaves on its own track (later clips on that track move left). Other tracks are not shifted; use remove_range to keep tracks in sync."
    )]
    async fn ripple_delete(
        &self,
        Parameters(p): Parameters<ElementId>,
    ) -> Result<CallToolResult, McpError> {
        let clip = parse_clip_id(&p.id)?;
        let mut backend = self.backend.lock().await;
        backend
            .command_apply(Command::RippleDelete { clip })
            .map_err(mcp_error)?;
        tool_text(json!({"deleted": clip.0}))
    }

    #[tool(
        description = "Propose automatic silence cuts and optional Whisper filler/repeat cuts without changing the project. Returns a revision-bound proposal with integer-tick ranges, heuristic confidence and enabled toggles. Review and pass the proposal to apply_autocuts. Selected tracks and every linked companion ripple together; locked companions cause an error."
    )]
    async fn propose_autocuts(
        &self,
        Parameters(p): Parameters<ProposeAutocuts>,
    ) -> Result<CallToolResult, McpError> {
        let range = match (p.from_ms, p.to_ms) {
            (None, None) => None,
            (Some(s), Some(e)) => Some(
                TimeRange::new(ms_to_ticks(s)?, ms_to_ticks(e)?)
                    .map_err(|e| invalid(&e.to_string()))?,
            ),
            _ => return Err(invalid("give both from_ms and to_ms")),
        };
        let params = animathio_control::AutocutParams {
            silence_db: p.silence_db.unwrap_or(-40.0),
            min_silence: ms_to_ticks(p.min_silence_ms.unwrap_or(500.0))?,
            hysteresis_db: p.hysteresis_db.unwrap_or(3.0),
            metric: if p.peak {
                animathio_control::SilenceMetric::Peak
            } else {
                animathio_control::SilenceMetric::Rms
            },
            keep_short_pauses: p.keep_short_pauses.unwrap_or(true),
            padding_before: ms_to_ticks(p.padding_before_ms.unwrap_or(100.0))?,
            padding_after: ms_to_ticks(p.padding_after_ms.unwrap_or(100.0))?,
            clip: p.clip.map(ClipId),
            track: p.track.map(TrackId),
            range,
            all_unlocked_tracks: p.all_unlocked_tracks,
            fillers: p.fillers,
            language: p.language,
            filler_words: p.filler_words,
            model: p.model.unwrap_or_else(|| "base".into()),
            download_model: p.download_model,
        };
        let result = self
            .backend
            .lock()
            .await
            .propose_autocuts(params)
            .map_err(mcp_error)?;
        tool_text(serde_json::to_value(result).map_err(|e| invalid(&e.to_string()))?)
    }

    #[tool(
        description = "Apply a reviewed proposal from propose_autocuts as one undoable ripple batch. Set cuts[].enabled=false to keep a proposed range. Rejects stale project revisions, overlapping ranges and locked tracks; linked video/audio stays aligned. Coordinates remain those of the original project."
    )]
    async fn apply_autocuts(
        &self,
        Parameters(p): Parameters<ApplyAutocuts>,
    ) -> Result<CallToolResult, McpError> {
        let proposal = serde_json::from_value(p.proposal).map_err(|e| invalid(&e.to_string()))?;
        let result = self
            .backend
            .lock()
            .await
            .apply_autocuts(proposal)
            .map_err(mcp_error)?;
        tool_text(serde_json::to_value(result).map_err(|e| invalid(&e.to_string()))?)
    }

    #[tool(
        description = "Cut pauses out of a recording: detect silent stretches in the decoded audio (RMS in 10 ms windows below threshold_db for at least min_silence_ms), keep padding_ms of silence on each side, and ripple-remove them from ALL tracks so video and audio stay in sync. One undo step. Returns the removed ranges in pre-edit timeline milliseconds. Only audio clips are analysed; import the recording's audio as an audio clip."
    )]
    async fn trim_silence(
        &self,
        Parameters(p): Parameters<TrimSilence>,
    ) -> Result<CallToolResult, McpError> {
        if !p.threshold_db.is_finite() {
            return Err(invalid("threshold_db must be finite"));
        }
        let params = TrimSilenceParams {
            threshold_db: p.threshold_db,
            min_silence: ms_to_ticks(p.min_silence_ms)?,
            padding: ms_to_ticks(p.padding_ms)?,
            track: p.track.map(TrackId),
        };
        let mut backend = self.backend.lock().await;
        let result = backend.trim_silence(params).map_err(mcp_error)?;
        let removed: Vec<_> = result
            .removed
            .iter()
            .map(|r| {
                json!({
                    "from_ms": ticks_to_ms(r.start),
                    "to_ms": ticks_to_ms(r.end),
                    "from": animathio_control::timecode(r.start),
                    "to": animathio_control::timecode(r.end),
                })
            })
            .collect();
        tool_text(json!({
            "removed": removed,
            "total_removed_ms": ticks_to_ms(result.total_removed),
        }))
    }

    #[tool(
        description = "Generate subtitles (captions) for the project's audio with a fully local Whisper speech recogniser (no cloud). Mixes the audio of the project, or of one clip/track/range, transcribes it, splits the text into readable cues (max 2 lines, max_chars_per_line chars per line, 0.8-6 s each, no overlaps) and adds them as caption text clips on a new \"Subtitles\" track (with a background bar track for the default \"classic\" style) as one undo step. Pass `style` for an animated CapCut-style caption (word-highlight, karaoke, pop, one-word, bold-outline); the recogniser's per-word timings drive the animation. The model must be installed: if it is not, the call fails with instructions unless download_model is true (one-time 78-488 MB download, SHA-256 verified; ask the user first). Can take from seconds to minutes. Returns the language, the new track ids and the cues (with start/end in ms). Requires an app build with the whisper feature."
    )]
    async fn auto_subtitles(
        &self,
        Parameters(p): Parameters<AutoSubtitles>,
    ) -> Result<CallToolResult, McpError> {
        let range = match (p.from_ms, p.to_ms) {
            (None, None) => None,
            (Some(from), Some(to)) => Some(range_ms(from, to)?),
            _ => return Err(invalid("from_ms and to_ms must be given together")),
        };
        let params = animathio_control::SubtitleGenerateParams {
            model: p.model,
            language: p.language,
            clip: p.clip.map(ClipId),
            track: p.track.map(TrackId),
            range,
            download_model: p.download_model,
            split: animathio_control::SubtitleSplit {
                max_chars_per_line: p.max_chars_per_line,
                max_lines: p.max_lines,
                ..animathio_control::SubtitleSplit::default()
            },
            style: p.look.style_params(p.style.as_deref())?,
        };
        let mut backend = self.backend.lock().await;
        let result = backend.subtitles_generate(params).map_err(mcp_error)?;
        tool_text(subtitle_result_json(&result))
    }

    #[tool(
        description = "Restyle existing caption clips (from auto_subtitles or import_subtitles) as one undo step: style (classic, word-highlight, karaoke, pop, one-word, bold-outline), font_size, colours, position (top, middle, bottom, safe-area aware) and max_words per line. Applies to every caption on the track (default: all \"Subtitles*\" tracks); adds or removes the background bars as the style needs. Returns the style and the number of captions changed."
    )]
    async fn set_caption_style(
        &self,
        Parameters(p): Parameters<SetCaptionStyle>,
    ) -> Result<CallToolResult, McpError> {
        let params = animathio_control::SubtitleStyleParams {
            track: p.track.map(TrackId),
            style: p.look.style_params(Some(&p.style))?,
        };
        let mut backend = self.backend.lock().await;
        let result = backend.subtitles_style(params).map_err(mcp_error)?;
        tool_text(json!({ "style": result.style, "captions": result.captions }))
    }

    #[tool(
        description = "Export a subtitle track as SRT or WebVTT. Returns the subtitle text, or writes it to `path`. Reads the text clips of the given track (default: the first track named \"Subtitles\", as created by auto_subtitles or import_subtitles)."
    )]
    async fn export_subtitles(
        &self,
        Parameters(p): Parameters<ExportSubtitles>,
    ) -> Result<CallToolResult, McpError> {
        let mut backend = self.backend.lock().await;
        let result = backend
            .subtitles_export(animathio_control::SubtitleExportParams {
                format: p.format.unwrap_or_else(|| "srt".to_owned()),
                output: p.path.map(PathBuf::from),
                track: p.track.map(TrackId),
                overwrite: p.overwrite,
            })
            .map_err(mcp_error)?;
        tool_text(json!({
            "format": result.format,
            "track": result.track.0,
            "cues": result.cues,
            "text": result.text,
            "path": result.path,
        }))
    }

    #[tool(
        description = "Import an SRT or WebVTT file as caption clips on a new \"Subtitles\" track (same look and `style` choices as auto_subtitles), as one undo step."
    )]
    async fn import_subtitles(
        &self,
        Parameters(p): Parameters<ImportSubtitles>,
    ) -> Result<CallToolResult, McpError> {
        if !p.offset_ms.is_finite() {
            return Err(invalid("offset_ms must be finite"));
        }
        #[allow(clippy::cast_possible_truncation)]
        let offset = Time((p.offset_ms * TICKS_PER_SECOND as f64 / 1000.0).round() as i64);
        let mut backend = self.backend.lock().await;
        let result = backend
            .subtitles_import(animathio_control::SubtitleImportParams {
                path: PathBuf::from(p.path),
                offset,
                style: p.look.style_params(p.style.as_deref())?,
            })
            .map_err(mcp_error)?;
        tool_text(subtitle_result_json(&result))
    }

    #[tool(
        description = "List the bundled title preset ids, editable field names/defaults, layers and entry/exit animations. Use these ids with add_title preset and fields."
    )]
    async fn list_title_presets(&self) -> Result<CallToolResult, McpError> {
        tool_text(
            self.backend
                .lock()
                .await
                .handle(Call::TitlesList)
                .map_err(mcp_error)?,
        )
    }

    #[tool(
        description = "Add on-screen text from start_ms for duration_ms as one undo step. For linked animated compositions, pass preset from list_title_presets and fields (e.g. name and role for lower_third); text fills the first field unless fields already supplies it. Without preset, use legacy style. style: `title` (large, centred), `lower_third` (medium, lower left, e.g. a speaker name) or `caption` (small subtitle at the bottom; consecutive captions share one track). Returns the clip id for update_element/add_animation."
    )]
    async fn add_title(
        &self,
        Parameters(p): Parameters<AddTitle>,
    ) -> Result<CallToolResult, McpError> {
        let start = ms_to_ticks(p.start_ms)?;
        let end = ms_to_ticks(p.start_ms + p.duration_ms)?;
        let range =
            TimeRange::new(start, end).map_err(|_| invalid("duration_ms must be positive"))?;
        if let Some(preset) = p.preset {
            let definition = animathio_control::title_presets()
                .iter()
                .find(|entry| entry.id == preset)
                .ok_or_else(|| invalid("unknown title preset; call list_title_presets"))?;
            let mut fields = p.fields;
            if !p.text.is_empty() {
                let primary = definition
                    .layers
                    .iter()
                    .find(|layer| !layer.field.is_empty())
                    .expect("preset field");
                fields.entry(primary.field.clone()).or_insert(p.text);
            }
            let result = self
                .backend
                .lock()
                .await
                .handle(Call::TitlesAdd {
                    params: animathio_control::TitleAddParams {
                        preset,
                        fields,
                        range,
                    },
                })
                .map_err(mcp_error)?;
            return tool_text(result);
        }
        let style = match p.style.unwrap_or(TitleStyleParam::Title) {
            TitleStyleParam::Title => TitleStyle::Title,
            TitleStyleParam::LowerThird => TitleStyle::LowerThird,
            TitleStyleParam::Caption => TitleStyle::Caption,
        };
        let mut backend = self.backend.lock().await;
        let state = backend.project_get().map_err(mcp_error)?;
        let (command, track_id, clip_id) =
            animathio_control::title_command(&state.project, &p.text, range, style)
                .map_err(mcp_error)?;
        backend.command_apply(command).map_err(mcp_error)?;
        tool_text(json!({"track_id": track_id.0, "clip_id": clip_id.0}))
    }

    #[tool(
        description = "Apply several low-level core commands atomically as ONE undo step; if any fails, nothing changes. Each op is a core Command in externally tagged JSON, e.g. {\"RemoveClip\":{\"id\":3}} or {\"MoveClip\":{\"id\":3,\"track\":1,\"start\":0}}. Times inside ops are integer ticks (705600000 per second), not milliseconds."
    )]
    async fn batch(&self, Parameters(p): Parameters<Batch>) -> Result<CallToolResult, McpError> {
        let commands = p
            .ops
            .into_iter()
            .enumerate()
            .map(|(index, op)| {
                serde_json::from_value::<Command>(op)
                    .map_err(|error| invalid(&format!("op {index}: {error}")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let count = commands.len();
        let mut backend = self.backend.lock().await;
        backend
            .command_apply(Command::Batch { commands })
            .map_err(mcp_error)?;
        tool_text(json!({"applied": count}))
    }

    #[tool(
        description = "Choose a delivery format: sets the project resolution and 30 fps (one undo step) and makes export_video use high-quality H.264 (CRF 18). Presets: youtube_1080p (1920x1080), youtube_4k (3840x2160), shorts_vertical (1080x1920), square (1080x1080). Positions are canvas fractions, so existing clips keep their relative layout."
    )]
    async fn set_export_preset(
        &self,
        Parameters(p): Parameters<SetExportPreset>,
    ) -> Result<CallToolResult, McpError> {
        let preset = p.preset.0;
        let mut backend = self.backend.lock().await;
        let state = backend.project_get().map_err(mcp_error)?;
        backend
            .command_apply(preset.command(&state.project).map_err(mcp_error)?)
            .map_err(mcp_error)?;
        *self
            .export_preset
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(preset);
        let (width, height) = preset.resolution();
        tool_text(
            json!({"width": width, "height": height, "fps": 30, "crf": preset.export_settings().crf}),
        )
    }

    #[tool(
        description = "Export an offline H.264 + AAC MP4 snapshot of the project. Missing parent directories are created; an existing file is only replaced when overwrite is true."
    )]
    async fn export_video(
        &self,
        Parameters(p): Parameters<ExportVideo>,
    ) -> Result<CallToolResult, McpError> {
        if p.format.as_deref().is_some_and(|format| format != "mp4") {
            return Err(unsupported("only mp4 export is available"));
        }
        let mut backend = self.backend.lock().await;
        let path = backend
            .export_video(animathio_control::ExportParams {
                output: PathBuf::from(p.path),
                from: Time(0),
                to: None,
                settings: self
                    .export_preset
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .map(ExportPreset::export_settings)
                    .unwrap_or_default(),
                overwrite: p.overwrite,
            })
            .map_err(mcp_error)?;
        tool_text(json!({ "path": path }))
    }

    #[tool(description = "Import a v1 `.animathio` project, replacing the current project.")]
    async fn import_v1(
        &self,
        Parameters(p): Parameters<ImportV1>,
    ) -> Result<CallToolResult, McpError> {
        let mut backend = self.backend.lock().await;
        backend
            .project_import_v1(PathBuf::from(&p.path))
            .map_err(mcp_error)?;
        let state = backend.project_get().map_err(mcp_error)?;
        tool_text(json!({"tracks": state.project.tracks().len()}))
    }

    #[tool(
        description = "Import a Manim Community scene from a script path or inline source, appending to (or replacing) the current project. Returns warnings and embedded-file info."
    )]
    async fn import_manim_scene(
        &self,
        Parameters(p): Parameters<ManimScene>,
    ) -> Result<CallToolResult, McpError> {
        let mut backend = self.backend.lock().await;
        let result = backend
            .import_manim(ImportManimParams {
                path: p.path.map(PathBuf::from),
                source: p.source,
                script_dir: p.script_dir.map(PathBuf::from),
                scene: p.scene,
                replace: p.replace,
            })
            .map_err(mcp_error)?;
        tool_text(json!({
            "tracks": result.tracks,
            "assets": result.assets,
            "warnings": result.warnings,
            "infos": result.infos,
        }))
    }
}

#[allow(clippy::unused_async_trait_impl)]
#[tool_handler]
impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::from_build_env())
            .with_protocol_version(ProtocolVersion::V_2024_11_05)
            .with_instructions(
                "AniMathIO v2 control surface over MCP. Mutations are applied as commands with \
                 undo/redo; times are milliseconds and coordinates are canvas pixels (origin \
                 top-left)."
                    .to_owned(),
            )
    }
}

#[cfg(test)]
mod preset_tests {
    use super::*;

    #[test]
    fn schema_and_deserialization_use_the_shared_preset_table() {
        let schema = serde_json::to_value(schemars::schema_for!(ExportPresetParam)).unwrap();
        assert_eq!(
            schema["enum"],
            json!(ExportPreset::ALL.map(ExportPreset::name))
        );
        for preset in ExportPreset::ALL {
            let parsed: ExportPresetParam = serde_json::from_value(json!(preset.name())).unwrap();
            assert_eq!(parsed.0, preset);
        }
    }
}
