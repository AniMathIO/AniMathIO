use std::f32::consts::PI;

/// 2D Point type used in guest code.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    /// X coordinate.
    pub x: f32,
    /// Y coordinate.
    pub y: f32,
}

impl Point {
    /// Create a new point.
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// 8-bit RGBA color used in guest code.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Color {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
    /// Alpha.
    pub a: u8,
}

impl Color {
    /// Create RGBA color.
    #[must_use]
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Create opaque RGB color.
    #[must_use]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }
}

/// Path command verb matching WIT path-verb.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathVerb {
    /// Move cursor to point without drawing.
    MoveTo(Point),
    /// Draw straight line to point.
    LineTo(Point),
    /// Draw quadratic Bézier curve.
    QuadTo(Point, Point),
    /// Draw cubic Bézier curve.
    CubicTo(Point, Point, Point),
    /// Close the current contour.
    Close,
}

/// Geometry generator helpers for 2D shapes.
pub struct Shapes;

impl Shapes {
    /// Generate a rectangle path.
    #[must_use]
    pub fn rect(x: f32, y: f32, width: f32, height: f32) -> Vec<PathVerb> {
        vec![
            PathVerb::MoveTo(Point::new(x, y)),
            PathVerb::LineTo(Point::new(x + width, y)),
            PathVerb::LineTo(Point::new(x + width, y + height)),
            PathVerb::LineTo(Point::new(x, y + height)),
            PathVerb::Close,
        ]
    }

    /// Generate an axis-aligned ellipse path using 4 cubic Béziers.
    #[must_use]
    pub fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> Vec<PathVerb> {
        let kx = 0.552_284_8 * rx;
        let ky = 0.552_284_8 * ry;
        vec![
            PathVerb::MoveTo(Point::new(cx + rx, cy)),
            PathVerb::CubicTo(
                Point::new(cx + rx, cy + ky),
                Point::new(cx + kx, cy + ry),
                Point::new(cx, cy + ry),
            ),
            PathVerb::CubicTo(
                Point::new(cx - kx, cy + ry),
                Point::new(cx - rx, cy + ky),
                Point::new(cx - rx, cy),
            ),
            PathVerb::CubicTo(
                Point::new(cx - rx, cy - ky),
                Point::new(cx - kx, cy - ry),
                Point::new(cx, cy - ry),
            ),
            PathVerb::CubicTo(
                Point::new(cx + kx, cy - ry),
                Point::new(cx + rx, cy - ky),
                Point::new(cx + rx, cy),
            ),
            PathVerb::Close,
        ]
    }

    /// Generate a circle path.
    #[must_use]
    pub fn circle(cx: f32, cy: f32, r: f32) -> Vec<PathVerb> {
        Self::ellipse(cx, cy, r, r)
    }

    /// Generate a regular polygon with `sides` edges.
    #[must_use]
    pub fn polygon(cx: f32, cy: f32, r: f32, sides: usize, rotation: f32) -> Vec<PathVerb> {
        let n = sides.max(3);
        let step = 2.0 * PI / (n as f32);
        let mut verbs = Vec::with_capacity(n + 1);

        for i in 0..n {
            let theta = rotation + (i as f32) * step;
            let x = cx + r * theta.cos();
            let y = cy + r * theta.sin();
            if i == 0 {
                verbs.push(PathVerb::MoveTo(Point::new(x, y)));
            } else {
                verbs.push(PathVerb::LineTo(Point::new(x, y)));
            }
        }
        verbs.push(PathVerb::Close);
        verbs
    }

    /// Generate a star shape with `points` rays.
    #[must_use]
    pub fn star(
        cx: f32,
        cy: f32,
        r_outer: f32,
        r_inner: f32,
        points: usize,
        rotation: f32,
    ) -> Vec<PathVerb> {
        let n = points.max(3);
        let total_vertices = n * 2;
        let step = 2.0 * PI / (total_vertices as f32);
        let mut verbs = Vec::with_capacity(total_vertices + 1);

        for i in 0..total_vertices {
            let r = if i % 2 == 0 { r_outer } else { r_inner };
            let theta = rotation + (i as f32) * step;
            let x = cx + r * theta.cos();
            let y = cy + r * theta.sin();
            if i == 0 {
                verbs.push(PathVerb::MoveTo(Point::new(x, y)));
            } else {
                verbs.push(PathVerb::LineTo(Point::new(x, y)));
            }
        }
        verbs.push(PathVerb::Close);
        verbs
    }

    /// Generate a straight line.
    #[must_use]
    pub fn line(x1: f32, y1: f32, x2: f32, y2: f32) -> Vec<PathVerb> {
        vec![
            PathVerb::MoveTo(Point::new(x1, y1)),
            PathVerb::LineTo(Point::new(x2, y2)),
        ]
    }

    /// Generate an arrow from (x1, y1) to (x2, y2) with arrow head.
    #[must_use]
    pub fn arrow(
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        head_len: f32,
        head_width: f32,
    ) -> Vec<PathVerb> {
        let dx = x2 - x1;
        let dy = y2 - y1;
        let angle = dy.atan2(dx);

        let half_w = head_width / 2.0;
        let p_head1 = Point::new(
            x2 - head_len * angle.cos() + half_w * angle.sin(),
            y2 - head_len * angle.sin() - half_w * angle.cos(),
        );
        let p_head2 = Point::new(
            x2 - head_len * angle.cos() - half_w * angle.sin(),
            y2 - head_len * angle.sin() + half_w * angle.cos(),
        );

        vec![
            PathVerb::MoveTo(Point::new(x1, y1)),
            PathVerb::LineTo(Point::new(x2, y2)),
            PathVerb::MoveTo(p_head1),
            PathVerb::LineTo(Point::new(x2, y2)),
            PathVerb::LineTo(p_head2),
        ]
    }
}
