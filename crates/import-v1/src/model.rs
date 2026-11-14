//! Lenient serde model of the v1 (`.animathio`) project JSON.
//!
//! v1 never declared a schema version; its files are plain JSON with the shape
//! produced by the `MobX` store. Unknown fields are collected through
//! `#[serde(flatten)]` catch-alls rather than rejected, so noisier or newer v1
//! files still import and the caller can warn about them.

use serde::Deserialize;
use std::collections::BTreeMap;

/// Unknown top-level/object fields, keyed by their JSON name.
pub(crate) type Extra = BTreeMap<String, serde_json::Value>;

fn default_background() -> String {
    "#111111".to_owned()
}
fn default_canvas_width() -> f64 {
    800.0
}
fn default_canvas_height() -> f64 {
    600.0
}
fn default_fps() -> f64 {
    60.0
}
fn default_max_time() -> f64 {
    30_000.0
}
fn default_one() -> f64 {
    1.0
}
fn default_positive_100() -> f64 {
    100.0
}
fn effect_none() -> String {
    "none".to_owned()
}

/// Root object of a serialized v1 project.
#[derive(Debug, Deserialize)]
pub(crate) struct V1Project {
    #[serde(rename = "backgroundColor", default = "default_background")]
    pub(crate) background_color: String,
    #[serde(rename = "editorElements", default)]
    pub(crate) editor_elements: Vec<V1Element>,
    #[serde(rename = "maxTime", default = "default_max_time")]
    pub(crate) max_time: f64,
    #[serde(default)]
    pub(crate) animations: Vec<V1Animation>,
    #[serde(default = "default_fps")]
    pub(crate) fps: f64,
    #[serde(rename = "canvas_width", default = "default_canvas_width")]
    pub(crate) canvas_width: f64,
    #[serde(rename = "canvas_height", default = "default_canvas_height")]
    pub(crate) canvas_height: f64,
    #[serde(rename = "mediaFiles", default)]
    pub(crate) media_files: Vec<V1MediaFile>,
    // Known but unused v1 state; declared so it is not reported as unknown.
    #[serde(rename = "selectedMenuOption", default)]
    pub(crate) _selected_menu_option: Option<String>,
    #[serde(rename = "audios", default)]
    pub(crate) _audios: Vec<serde_json::Value>,
    #[serde(rename = "videos", default)]
    pub(crate) _videos: Vec<serde_json::Value>,
    #[serde(rename = "images", default)]
    pub(crate) _images: Vec<serde_json::Value>,
    #[serde(rename = "currentKeyFrame", default)]
    pub(crate) _current_key_frame: Option<serde_json::Value>,
    #[serde(rename = "selectedVideoFormat", default)]
    pub(crate) _selected_video_format: Option<String>,
    #[serde(flatten)]
    pub(crate) extra: Extra,
}

/// One timeline element (clip) in the v1 editor.
#[derive(Debug, Deserialize)]
pub(crate) struct V1Element {
    #[serde(default)]
    pub(crate) id: String,
    #[serde(rename = "type", default)]
    pub(crate) kind: String,
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) placement: Option<V1Placement>,
    #[serde(rename = "timeFrame", default)]
    pub(crate) time_frame: Option<V1TimeFrame>,
    #[serde(default)]
    pub(crate) properties: V1Properties,
    #[serde(rename = "konvaNode", default)]
    pub(crate) _konva_node: Option<serde_json::Value>,
    #[serde(flatten)]
    pub(crate) extra: Extra,
}

/// Pixel placement of a v1 element (Konva `x`/`y` is the top-left corner).
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct V1Placement {
    #[serde(default)]
    pub(crate) x: f64,
    #[serde(default)]
    pub(crate) y: f64,
    #[serde(default = "default_positive_100")]
    pub(crate) width: f64,
    #[serde(default = "default_positive_100")]
    pub(crate) height: f64,
    #[serde(default)]
    pub(crate) rotation: f64,
    #[serde(rename = "scaleX", alias = "scale_x", default = "default_one")]
    pub(crate) scale_x: f64,
    #[serde(rename = "scaleY", alias = "scale_y", default = "default_one")]
    pub(crate) scale_y: f64,
}

/// Inclusive milliseconds range on the v1 timeline.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct V1TimeFrame {
    #[serde(default)]
    pub(crate) start: f64,
    #[serde(default)]
    pub(crate) end: f64,
}

/// Per-type element properties, modelled as one lenient struct.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct V1Properties {
    #[serde(default)]
    pub(crate) src: Option<String>,
    #[serde(rename = "elementId", default)]
    pub(crate) _element_id: Option<String>,
    #[serde(default)]
    pub(crate) effect: Option<V1Effect>,
    #[serde(default)]
    pub(crate) muted: Option<bool>,
    #[serde(default)]
    pub(crate) volume: Option<f64>,
    #[serde(rename = "masterVolume", default)]
    pub(crate) master_volume: Option<f64>,
    #[serde(default)]
    pub(crate) text: Option<String>,
    #[serde(rename = "fontSize", default)]
    pub(crate) font_size: Option<f64>,
    #[serde(rename = "fontWeight", default)]
    pub(crate) font_weight: Option<f64>,
    #[serde(default)]
    pub(crate) color: Option<String>,
    #[serde(rename = "splittedTexts", default)]
    pub(crate) _splitted_texts: Option<serde_json::Value>,
    #[serde(flatten)]
    pub(crate) extra: Extra,
}

/// Named visual effect; v1 only had the four CSS filter presets plus `none`.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct V1Effect {
    #[serde(rename = "type", default = "effect_none")]
    pub(crate) kind: String,
}

/// A base64 data URL embedded alongside media elements.
#[derive(Debug, Deserialize)]
pub(crate) struct V1MediaFile {
    #[serde(default)]
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) data: String,
    #[serde(rename = "type", default)]
    pub(crate) _kind: String,
    #[serde(flatten)]
    pub(crate) extra: Extra,
}

/// A v1 animation preset, keyed to an element id.
#[derive(Debug, Deserialize)]
pub(crate) struct V1Animation {
    #[serde(default)]
    pub(crate) id: String,
    #[serde(rename = "targetId", default)]
    pub(crate) target_id: String,
    #[serde(default)]
    pub(crate) duration: f64,
    #[serde(rename = "type", default)]
    pub(crate) kind: String,
    #[serde(default)]
    pub(crate) properties: V1AnimationProperties,
    #[serde(flatten)]
    pub(crate) extra: Extra,
}

/// Animation-specific options (direction, per-character text slide, etc.).
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct V1AnimationProperties {
    #[serde(default)]
    pub(crate) direction: Option<String>,
    #[serde(rename = "useClipPath", default)]
    pub(crate) _use_clip_path: Option<bool>,
    #[serde(rename = "textType", default)]
    pub(crate) text_type: Option<String>,
    #[serde(flatten)]
    pub(crate) extra: Extra,
}
