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
//! `render_frame`, `undo`/`redo`, `set_param`, `add_keyframe` and `import_v1`.

use std::{path::PathBuf, sync::Arc};

use animathio_control::{
    AssetInfo, ControlClient, ControlHandler, HeadlessHandler, ProjectState, RenderFrameParams,
    default_socket_spec,
};
use animathio_core::{
    Asset, Clip, ClipId, ClipKind, Command, Interpolation, Keyframe, Project, Rounding,
    TICKS_PER_SECOND, Target, Time, TimeRange, Track, TrackId, Transform, TransformProperty,
    builtin::Builtin,
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
}

impl McpServer {
    pub(crate) fn new(backend: Backend) -> Self {
        Self {
            backend: Arc::new(Mutex::new(backend)),
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
#[allow(dead_code)] // schema-only placeholder until the latex plugin lands
pub(crate) struct AddMath {
    pub(crate) latex: String,
    pub(crate) x: Option<f64>,
    pub(crate) y: Option<f64>,
    pub(crate) color: Option<String>,
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
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct ImportV1 {
    pub(crate) path: String,
}

#[derive(Deserialize, JsonSchema)]
#[allow(dead_code)] // schema-only placeholder until manim-import lands
pub(crate) struct ManimScene {
    pub(crate) script: String,
}

// ---- helpers --------------------------------------------------------------

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
            ClipKind::Builtin(Builtin::Text {
                text: p.text,
                font_size,
                color,
            }),
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
        description = "Add a LaTeX fragment. The v2 latex plugin is not implemented yet, so this tool currently fails."
    )]
    async fn add_math(&self, _: Parameters<AddMath>) -> Result<CallToolResult, McpError> {
        Err(unsupported(
            "add_math requires the latex plugin, which is not implemented yet",
        ))
    }

    #[tool(
        description = "Import an image, video, or audio file by absolute path and add a clip for it."
    )]
    async fn add_media(
        &self,
        Parameters(p): Parameters<AddMedia>,
    ) -> Result<CallToolResult, McpError> {
        let mut backend = self.backend.lock().await;
        let info: AssetInfo = backend
            .assets_import(PathBuf::from(&p.path))
            .map_err(mcp_error)?;
        let state = backend.project_get().map_err(mcp_error)?;
        let kind = match media_kind(&info.media_type) {
            Some(MediaKind::Image) => ClipKind::Image(info.id.clone()),
            Some(MediaKind::Video) => ClipKind::Video(info.id.clone()),
            Some(MediaKind::Audio) => ClipKind::Audio(info.id.clone()),
            None => {
                return Err(invalid(&format!("unsupported media type for '{}'", p.path)));
            }
        };
        let base = next_id(&state.project);
        let track_id = TrackId(base);
        let clip_id = ClipId(base + 1);
        backend
            .command_apply(Command::AddAsset {
                id: info.id.clone(),
                asset: Asset {
                    name: info.name.clone(),
                    media_type: info.media_type.clone(),
                },
            })
            .map_err(mcp_error)?;
        let clip = Clip::new(
            clip_id,
            kind,
            TimeRange::new(Time(0), project_duration(&state)).map_err(|e| mcp_error(e.into()))?,
        );
        backend
            .command_apply(Command::AddTrack {
                track: Track::new(track_id, info.name),
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
                            value: 0.0,
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
                            value: opacity,
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
                            value: opacity,
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
                            value: 0.0,
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
                            value: from.0,
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
                            value: to.0,
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
                            value: from.1,
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
                            value: to.1,
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
                    value: p.value,
                    interpolation,
                },
            })
            .map_err(mcp_error)?;
        tool_text(json!({"clip_id": clip_id.0}))
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

    #[tool(description = "Export an offline H.264 + AAC MP4 snapshot of the project.")]
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
                settings: animathio_export::Settings::default(),
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
        description = "Build a whole scene from a Manim Community script. Not implemented yet in v2."
    )]
    async fn import_manim_scene(
        &self,
        _: Parameters<ManimScene>,
    ) -> Result<CallToolResult, McpError> {
        Err(unsupported("manim import is not implemented yet in v2"))
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

#[derive(Clone, Copy)]
enum MediaKind {
    Image,
    Video,
    Audio,
}

fn media_kind(media_type: &str) -> Option<MediaKind> {
    if media_type.starts_with("image/") {
        Some(MediaKind::Image)
    } else if media_type.starts_with("video/") {
        Some(MediaKind::Video)
    } else if media_type.starts_with("audio/") {
        Some(MediaKind::Audio)
    } else {
        None
    }
}
