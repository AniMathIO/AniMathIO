//! Authoring helpers for the first-party text and shapes components.
use animathio_core::ClipKind;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Straight-alpha sRGB RGBA components.
pub type Color = [u8; 4];

/// Wrapped, top-left text; font size is relative to canvas height.
#[must_use]
pub fn text(text: String, font_size: f64, color: Color) -> ClipKind {
    ClipKind::Plugin {
        plugin_id: "org.animathio.text".into(),
        params: json!({
            "text": text, "font_size": font_size, "color": color,
            "units": "normalized", "align": "left",
        }),
    }
}

/// Unit-square vector geometry with optional fill and outline.
#[must_use]
pub fn shape(shape: Shape, fill: Option<Color>, stroke: Option<Stroke>) -> ClipKind {
    let (name, path) = match shape {
        Shape::Rect => ("rect", json!([])),
        Shape::Ellipse => ("ellipse", json!([])),
        Shape::Path(path) => (
            "path",
            serde_json::to_value(path).expect("finite path coordinates"),
        ),
    };
    ClipKind::Plugin {
        plugin_id: "org.animathio.shapes".into(),
        params: json!({
            "units": "normalized", "shape": name, "path": path.to_string(),
            "x": 0.5, "y": 0.5, "width": 1.0, "height": 1.0, "radius": 0.5,
            "rotation": 0.0, "points": 5.0,
            "fill": fill.unwrap_or([0; 4]), "stroke": stroke.as_ref().map_or([0;4], |s| s.color),
            "stroke_width": stroke.as_ref().map_or(0.0, |s| s.width),
        }),
    }
}

/// Basic vector geometry, independent of the rendering library.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Shape {
    /// Full unit rectangle.
    Rect,
    /// Ellipse inscribed in the unit square.
    Ellipse,
    /// Arbitrary line, quadratic and cubic segments in local normalized coordinates.
    Path(Vec<PathElement>),
}

/// Serializable path command. Paths must begin each contour with a move.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PathElement {
    /// Start a contour.
    MoveTo([f64; 2]),
    /// Straight segment.
    LineTo([f64; 2]),
    /// Quadratic control point and endpoint.
    QuadTo([f64; 2], [f64; 2]),
    /// Cubic control points and endpoint.
    CurveTo([f64; 2], [f64; 2], [f64; 2]),
    /// Close the current contour.
    Close,
}

/// Normalized outline, scaled along with its clip.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    /// Outline color.
    pub color: Color,
    /// Width in local unit-square coordinates.
    pub width: f64,
}
