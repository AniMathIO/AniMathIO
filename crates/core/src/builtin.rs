//! Temporary generative clip data for the CPU prototype.
//! Text and shapes move to plugins once the component API is available.
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
