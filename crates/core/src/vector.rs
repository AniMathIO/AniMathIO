use crate::{Error, invalid};
use serde::{Deserialize, Serialize};

/// Straight-alpha 8-bit RGBA color.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Color {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
    /// Alpha channel.
    pub a: u8,
}

impl Color {
    /// Construct an RGBA color.
    #[must_use]
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Construct a fully opaque RGB color.
    #[must_use]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }
}

/// 2D point in canvas or local coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    /// Horizontal coordinate.
    pub x: f64,
    /// Vertical coordinate.
    pub y: f64,
}

impl Point {
    /// Construct a point with coordinates.
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub(crate) fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

impl From<(f32, f32)> for Point {
    fn from((x, y): (f32, f32)) -> Self {
        Self {
            x: f64::from(x),
            y: f64::from(y),
        }
    }
}

impl From<(f64, f64)> for Point {
    fn from((x, y): (f64, f64)) -> Self {
        Self { x, y }
    }
}

impl From<Point> for (f64, f64) {
    fn from(p: Point) -> Self {
        (p.x, p.y)
    }
}

/// Line-cap style for stroked paths.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum StrokeCap {
    /// Flat edge at path endpoint.
    #[default]
    Butt,
    /// Semicircular arc at path endpoint.
    Round,
    /// Square extending beyond endpoint by half the stroke width.
    Square,
}

/// Line-join style for stroked paths.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum StrokeJoin {
    /// Sharp corner with miter limit.
    #[default]
    Miter,
    /// Rounded corner.
    Round,
    /// Beveled flat corner.
    Bevel,
}

/// Style parameters for stroked paths.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct StrokeStyle {
    /// Stroke thickness.
    pub width: f64,
    /// Stroke color.
    pub color: Color,
    /// Cap style at line ends.
    pub cap: StrokeCap,
    /// Join style at vertices.
    pub join: StrokeJoin,
}

impl Default for StrokeStyle {
    fn default() -> Self {
        Self {
            width: 1.0,
            color: Color::rgb(255, 255, 255),
            cap: StrokeCap::default(),
            join: StrokeJoin::default(),
        }
    }
}

/// Path command verb matching standard 2D vector primitives.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum PathVerb {
    /// Move cursor to point without drawing.
    MoveTo(Point),
    /// Draw straight line to point.
    LineTo(Point),
    /// Draw quadratic Bézier curve with control point and destination.
    QuadTo(Point, Point),
    /// Draw cubic Bézier curve with two control points and destination.
    CubicTo(Point, Point, Point),
    /// Close the current contour.
    Close,
}

impl PathVerb {
    pub(crate) fn is_finite(&self) -> bool {
        match self {
            Self::MoveTo(p) | Self::LineTo(p) => p.is_finite(),
            Self::QuadTo(p1, p2) => p1.is_finite() && p2.is_finite(),
            Self::CubicTo(p1, p2, p3) => p1.is_finite() && p2.is_finite() && p3.is_finite(),
            Self::Close => true,
        }
    }
}

/// Text run element.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextElement {
    /// Text content.
    pub text: String,
    /// Font size in pixels.
    pub font_size: f64,
    /// Left coordinate.
    pub x: f64,
    /// Baseline coordinate.
    pub y: f64,
    /// Fill color.
    pub color: Color,
}

/// Resolved vector element.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum VectorElement {
    /// Fill a closed path with a solid color.
    FillPath {
        /// Sequence of path verbs.
        path: Vec<PathVerb>,
        /// Fill color.
        color: Color,
    },
    /// Outline a path with a stroke style.
    StrokePath {
        /// Sequence of path verbs.
        path: Vec<PathVerb>,
        /// Stroke parameters.
        style: StrokeStyle,
    },
    /// Render a text run.
    TextRun(TextElement),
}

/// Vector graphics scene composed of vector draw elements.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VectorScene {
    /// Ordered drawing elements.
    pub elements: Vec<VectorElement>,
}

impl VectorScene {
    /// Create an empty vector scene.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Validate coordinates, stroke widths, and font sizes.
    pub fn validate(&self) -> Result<(), Error> {
        for elem in &self.elements {
            match elem {
                VectorElement::FillPath { path, .. } => {
                    for verb in path {
                        if !verb.is_finite() {
                            return Err(invalid("nonfinite path coordinate"));
                        }
                    }
                }
                VectorElement::StrokePath { path, style } => {
                    if !style.width.is_finite() || style.width < 0.0 {
                        return Err(invalid("invalid stroke width"));
                    }
                    for verb in path {
                        if !verb.is_finite() {
                            return Err(invalid("nonfinite path coordinate"));
                        }
                    }
                }
                VectorElement::TextRun(text) => {
                    if !text.font_size.is_finite() || text.font_size <= 0.0 {
                        return Err(invalid("invalid font size"));
                    }
                    if !text.x.is_finite() || !text.y.is_finite() {
                        return Err(invalid("nonfinite text coordinate"));
                    }
                }
            }
        }
        Ok(())
    }
}
