//! Schema-1 compatibility data. Never used by the current project model.
use crate::{Error, invalid};
use serde::{Deserialize, Serialize};

/// Straight-alpha sRGB RGBA components.
pub type Color = [u8; 4];

/// Temporary built-in clip sources. Geometry uses a local unit square [0, 1].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Builtin {
    /// Wrapped, top-left aligned text using the embedded `DejaVu Sans` font.
    Text {
        /// Unicode content, including explicit line breaks.
        text: String,
        /// Font size as a fraction of the unscaled canvas height.
        font_size: f64,
        /// Text fill.
        color: Color,
    },
    /// Vector geometry with optional fill and stroke.
    Shape {
        /// Unit-square geometry.
        shape: Shape,
        /// Nonzero-rule fill; None leaves the interior transparent.
        fill: Option<Color>,
        /// Optional outline.
        stroke: Option<Stroke>,
    },
}

use animathio_plugin_api::sources::{PathElement, Shape, Stroke};

impl Builtin {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        match self {
            Self::Text { font_size, .. } => {
                if !font_size.is_finite() || *font_size <= 0.0 || *font_size > 16.0 {
                    return Err(invalid("builtin font size must be in (0, 16]"));
                }
            }
            Self::Shape { shape, stroke, .. } => {
                if let Some(stroke) = stroke
                    && (!stroke.width.is_finite() || stroke.width < 0.0)
                {
                    return Err(invalid("invalid builtin stroke width"));
                }
                if let Shape::Path(elements) = shape {
                    let mut contour = false;
                    for element in elements {
                        let points: &[[f64; 2]] = match element {
                            PathElement::MoveTo(p) => {
                                contour = true;
                                std::slice::from_ref(p)
                            }
                            PathElement::LineTo(p) if contour => std::slice::from_ref(p),
                            PathElement::QuadTo(a, b) if contour => {
                                if !finite(*a) || !finite(*b) {
                                    return Err(invalid("nonfinite builtin path coordinate"));
                                }
                                &[]
                            }
                            PathElement::CurveTo(a, b, c) if contour => {
                                if !finite(*a) || !finite(*b) || !finite(*c) {
                                    return Err(invalid("nonfinite builtin path coordinate"));
                                }
                                &[]
                            }
                            PathElement::Close if contour => {
                                contour = false;
                                &[]
                            }
                            _ => {
                                return Err(invalid("builtin path contour must start with MoveTo"));
                            }
                        };
                        if points.iter().any(|p| !finite(*p)) {
                            return Err(invalid("nonfinite builtin path coordinate"));
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
fn finite(point: [f64; 2]) -> bool {
    point.iter().all(|v| v.is_finite())
}

impl Builtin {
    pub(super) fn into_plugin(self) -> animathio_core::ClipKind {
        match self {
            Self::Text {
                text,
                font_size,
                color,
            } => animathio_plugin_api::sources::text(text, font_size, color),
            Self::Shape {
                shape,
                fill,
                stroke,
            } => animathio_plugin_api::sources::shape(shape, fill, stroke),
        }
    }
}

pub(super) fn migrate(mut value: serde_json::Value) -> Result<serde_json::Value, Error> {
    if let Some(tracks) = value
        .get_mut("tracks")
        .and_then(serde_json::Value::as_array_mut)
    {
        for track in tracks {
            if let Some(clips) = track
                .get_mut("clips")
                .and_then(serde_json::Value::as_object_mut)
            {
                for clip in clips.values_mut() {
                    if let Some(old) = clip.get("kind").and_then(|k| k.get("Builtin")) {
                        let source: Builtin = serde_json::from_value(old.clone())?;
                        source.validate()?;
                        clip["kind"] = serde_json::to_value(source.into_plugin())?;
                    }
                }
            }
        }
    }
    value["schemaVersion"] = serde_json::json!(2);
    Ok(value)
}

/// Schema 2 stored effects as `{"id", "kind": "Grayscale", "params"}` with a closed enum of
/// builtin filters. Schema 3 names a plugin and a filter id instead; the former builtins now
/// live in `org.animathio.effects-basic` under lower-case ids, with identical parameter names.
pub(super) fn migrate_effects(mut value: serde_json::Value) -> Result<serde_json::Value, Error> {
    if let Some(tracks) = value
        .get_mut("tracks")
        .and_then(serde_json::Value::as_array_mut)
    {
        for track in tracks {
            let Some(clips) = track
                .get_mut("clips")
                .and_then(serde_json::Value::as_object_mut)
            else {
                continue;
            };
            for clip in clips.values_mut() {
                let Some(effects) = clip
                    .get_mut("effects")
                    .and_then(serde_json::Value::as_array_mut)
                else {
                    continue;
                };
                for effect in effects {
                    let object = effect
                        .as_object_mut()
                        .ok_or_else(|| invalid("effect must be an object"))?;
                    let kind = object
                        .remove("kind")
                        .ok_or_else(|| invalid("schema-2 effect has no kind"))?;
                    let id = match kind.as_str() {
                        Some("Grayscale") => "grayscale",
                        Some("Sepia") => "sepia",
                        Some("Invert") => "invert",
                        Some("Saturate") => "saturate",
                        Some("Brightness") => "brightness",
                        Some("Contrast") => "contrast",
                        Some("Blur") => "blur",
                        Some("Opacity") => "opacity",
                        _ => return Err(invalid("unknown builtin effect kind")),
                    };
                    object.insert(
                        "plugin".to_owned(),
                        serde_json::json!(animathio_core::BASIC_EFFECTS_PLUGIN),
                    );
                    object.insert("effect".to_owned(), serde_json::json!(id));
                }
            }
        }
    }
    value["schemaVersion"] = serde_json::json!(3);
    Ok(value)
}

/// Schema 4 adds optional audio-effect chains (`audio_effects` on clips and tracks) and a
/// `bypass` flag on effects. Both are absent-means-default, so a schema-3 project is already
/// valid schema 4; the bump exists so older readers refuse files that may carry audio effects
/// instead of silently dropping them.
pub(super) fn migrate_audio_effects(mut value: serde_json::Value) -> serde_json::Value {
    value["schemaVersion"] = serde_json::json!(4);
    value
}

/// Schema 5 adds project-level timeline markers (`markers`). The key is absent when there are
/// none, so a schema-4 project is already valid schema 5; the bump exists so older readers
/// refuse files that may carry markers instead of silently dropping them.
pub(super) fn migrate_markers(mut value: serde_json::Value) -> serde_json::Value {
    value["schemaVersion"] = serde_json::json!(5);
    value
}
