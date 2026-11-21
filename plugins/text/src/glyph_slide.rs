//! Per-glyph slide ("character slide"): every glyph travels a short distance into place,
//! staggered in visual order and optionally masked to its own cell.
//!
//! The text plugin sees the shaped glyphs, so the units are exact (no pixel splitting):
//! a glyph with its dot or accent is one unit, a ligature is one unit, each CJK character
//! is one unit and lines stay separate. Units come from the host `text-layout` import for
//! wrapped text (the renderer's own layout) and from per-character `shape-text` calls for
//! the single-line and fitted layouts; in both the order is logical (left to right) for
//! left-to-right text and the visual order of the line for right-to-left text.
//!
//! Everything is a pure function of the parameters, in normalized positions: the stagger
//! follows the glyph's index among all glyphs divided by the glyph count, and the travel
//! distance is the line height, so any resolution produces the same timing and geometry
//! (up to the pixel scale).
//!
//! Parameters (all keyframeable numbers, so the presets animate them):
//! `char_slide` progress (1 at rest, which renders the ordinary text), `char_slide_direction`
//! (1 left, 2 right, 3 top, 4 bottom: the side the glyphs come from), `char_slide_stagger`
//! (the fraction of the progress spent staggering; negative starts with the last glyph),
//! `char_slide_clip` (mask each glyph to its own cell along the travel axis) and
//! `char_slide_fade` (amount of alpha ramp, 0 to 1).

use crate::animathio::plugin::types::{Color, DrawCommand, PathVerb, Point};

/// More glyphs than this render as the plain text instead of animating.
pub const MAX_GLYPHS: usize = 4000;

/// One glyph unit in pixel coordinates.
pub struct Glyph {
    pub contours: Vec<Vec<PathVerb>>,
    /// Index into [`Layout::lines`].
    pub line: usize,
    /// Advance cell along x; widened to the ink when the ink overhangs it.
    pub left: f32,
    pub right: f32,
}

/// Vertical extent of one text line in pixels.
pub struct Band {
    pub top: f32,
    pub bottom: f32,
}

/// Glyph units of a whole text in visual (left to right) order, plus the line bands.
pub struct Layout {
    pub lines: Vec<Band>,
    pub glyphs: Vec<Glyph>,
}

/// The slide parameters at one instant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slide {
    pub progress: f64,
    pub direction: u8,
    pub stagger: f64,
    pub clip: bool,
    pub fade: f64,
}

impl Slide {
    /// `None` when the glyphs are at rest (or the progress is not a number).
    pub fn from_params(progress: f64, direction: f64, stagger: f64, clip: f64, fade: f64) -> Option<Self> {
        if !(progress < 1.0) {
            return None;
        }
        Some(Self {
            progress: progress.max(0.0),
            direction: if direction.is_finite() {
                direction.round().clamp(1.0, 4.0) as u8
            } else {
                1
            },
            stagger: if stagger.is_finite() {
                stagger.clamp(-1.0, 1.0)
            } else {
                0.0
            },
            clip: clip > 0.5,
            fade: if fade.is_finite() {
                fade.clamp(0.0, 1.0)
            } else {
                0.0
            },
        })
    }
}

/// The progress of glyph `index` of `count`: `0` hidden, `1` in place.
///
/// With `stagger` s the glyph starts after `|s| * u` of the progress, where `u` is its
/// normalized position (`index / (count - 1)`, mirrored for a negative `s`), and then
/// takes the remaining `1 - |s|`.
pub fn local_progress(slide: &Slide, index: usize, count: usize) -> f64 {
    let u = if count > 1 {
        index as f64 / (count - 1) as f64
    } else {
        0.0
    };
    let magnitude = slide.stagger.abs().min(0.95);
    let u = if slide.stagger < 0.0 { 1.0 - u } else { u };
    let start = magnitude * u;
    ((slide.progress - start) / (1.0 - magnitude)).clamp(0.0, 1.0)
}

/// Draw commands for every glyph at `slide.progress`, one filled path per glyph so counters
/// stay open. `map` converts pixel points to scene coordinates.
pub fn draw(
    layout: &Layout,
    slide: &Slide,
    color: Color,
    map: impl Fn(Point) -> Point,
) -> Vec<DrawCommand> {
    let count = layout.glyphs.len();
    let vertical = slide.direction >= 3;
    let sign = if slide.direction % 2 == 1 { -1.0_f32 } else { 1.0 };
    let mut out = Vec::with_capacity(count);
    for (index, glyph) in layout.glyphs.iter().enumerate() {
        let local = local_progress(slide, index, count) as f32;
        let band = &layout.lines[glyph.line];
        let distance = (band.bottom - band.top).max(0.0);
        let offset = sign * (1.0 - local) * distance;
        let (dx, dy) = if vertical { (0.0, offset) } else { (offset, 0.0) };
        let alpha = 1.0 - slide.fade as f32 * (1.0 - local);
        let alpha = (f32::from(color.a) * alpha).round().clamp(0.0, 255.0) as u8;
        if alpha == 0 {
            continue;
        }
        let ink = bounds(&glyph.contours);
        let Some(ink) = ink else { continue };
        // The mask: the glyph's own cell on the travel axis, never narrower than its ink, so
        // a glyph in place is never trimmed.
        let (lo, hi) = if vertical {
            (band.top.min(ink.1), band.bottom.max(ink.3))
        } else {
            (glyph.left.min(ink.0), glyph.right.max(ink.2))
        };
        let moved = |p: Point| Point {
            x: p.x + dx,
            y: p.y + dy,
        };
        let shifted: Vec<Vec<PathVerb>> = glyph
            .contours
            .iter()
            .map(|contour| contour.iter().map(|verb| map_verb(verb, &moved)).collect())
            .collect();
        let contours = if slide.clip && offset != 0.0 {
            let (a, b) = if vertical {
                (ink.1 + dy, ink.3 + dy)
            } else {
                (ink.0 + dx, ink.2 + dx)
            };
            if b <= lo || a >= hi {
                continue; // Entirely behind the mask.
            }
            if a >= lo && b <= hi {
                shifted
            } else {
                clipped(&shifted, vertical, lo, hi)
            }
        } else {
            shifted
        };
        let path: Vec<PathVerb> = contours
            .iter()
            .flatten()
            .map(|verb| map_verb(verb, &map))
            .collect();
        if !path.is_empty() {
            out.push(DrawCommand::FillPath((path, Color { a: alpha, ..color })));
        }
    }
    out
}

fn map_verb(verb: &PathVerb, f: &impl Fn(Point) -> Point) -> PathVerb {
    match verb {
        PathVerb::MoveTo(p) => PathVerb::MoveTo(f(*p)),
        PathVerb::LineTo(p) => PathVerb::LineTo(f(*p)),
        PathVerb::QuadTo((c, p)) => PathVerb::QuadTo((f(*c), f(*p))),
        PathVerb::CubicTo((a, b, p)) => PathVerb::CubicTo((f(*a), f(*b), f(*p))),
        PathVerb::Close => PathVerb::Close,
    }
}

/// `(min_x, min_y, max_x, max_y)` over every control point.
fn bounds(contours: &[Vec<PathVerb>]) -> Option<(f32, f32, f32, f32)> {
    let mut out: Option<(f32, f32, f32, f32)> = None;
    let mut add = |p: &Point| {
        out = Some(match out {
            None => (p.x, p.y, p.x, p.y),
            Some((x0, y0, x1, y1)) => (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y)),
        });
    };
    for verb in contours.iter().flatten() {
        match verb {
            PathVerb::MoveTo(p) | PathVerb::LineTo(p) => add(p),
            PathVerb::QuadTo((c, p)) => {
                add(c);
                add(p);
            }
            PathVerb::CubicTo((a, b, p)) => {
                add(a);
                add(b);
                add(p);
            }
            PathVerb::Close => {}
        }
    }
    out
}

/// Sutherland-Hodgman clip of a closed polygon to `lo <= axis <= hi` (`axis` is y when
/// `vertical`).
fn clip_polygon(points: &[(f32, f32)], vertical: bool, lo: f32, hi: f32) -> Vec<(f32, f32)> {
    let coord = |p: (f32, f32)| if vertical { p.1 } else { p.0 };
    let mut current = points.to_vec();
    for (bound, keep_above) in [(lo, true), (hi, false)] {
        let inside = |p: (f32, f32)| {
            if keep_above {
                coord(p) >= bound
            } else {
                coord(p) <= bound
            }
        };
        let mut next = Vec::with_capacity(current.len() + 2);
        for i in 0..current.len() {
            let a = current[i];
            let b = current[(i + 1) % current.len()];
            if inside(a) {
                next.push(a);
            }
            if inside(a) != inside(b) && (coord(b) - coord(a)).abs() > f32::EPSILON {
                let t = (bound - coord(a)) / (coord(b) - coord(a));
                next.push((a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1)));
            }
        }
        current = next;
        if current.len() < 3 {
            return Vec::new();
        }
    }
    current
}

/// Contours clipped to the interval; curves become short segments only where a glyph
/// straddles the mask.
fn clipped(contours: &[Vec<PathVerb>], vertical: bool, lo: f32, hi: f32) -> Vec<Vec<PathVerb>> {
    contours
        .iter()
        .filter_map(|contour| {
            let polygon = clip_polygon(&flatten(contour), vertical, lo, hi);
            if polygon.len() < 3 {
                return None;
            }
            let mut path: Vec<PathVerb> = polygon
                .iter()
                .enumerate()
                .map(|(i, (x, y))| {
                    let p = Point { x: *x, y: *y };
                    if i == 0 {
                        PathVerb::MoveTo(p)
                    } else {
                        PathVerb::LineTo(p)
                    }
                })
                .collect();
            path.push(PathVerb::Close);
            Some(path)
        })
        .collect()
}

fn flatten(contour: &[PathVerb]) -> Vec<(f32, f32)> {
    let mut points: Vec<(f32, f32)> = Vec::new();
    let mut last = (0.0_f32, 0.0_f32);
    for verb in contour {
        match verb {
            PathVerb::MoveTo(p) | PathVerb::LineTo(p) => {
                last = (p.x, p.y);
                points.push(last);
            }
            PathVerb::QuadTo((c, p)) => {
                for i in 1..=8 {
                    let t = i as f32 / 8.0;
                    let u = 1.0 - t;
                    points.push((
                        u * u * last.0 + 2.0 * u * t * c.x + t * t * p.x,
                        u * u * last.1 + 2.0 * u * t * c.y + t * t * p.y,
                    ));
                }
                last = (p.x, p.y);
            }
            PathVerb::CubicTo((a, b, p)) => {
                for i in 1..=10 {
                    let t = i as f32 / 10.0;
                    let u = 1.0 - t;
                    points.push((
                        u * u * u * last.0
                            + 3.0 * u * u * t * a.x
                            + 3.0 * u * t * t * b.x
                            + t * t * t * p.x,
                        u * u * u * last.1
                            + 3.0 * u * u * t * a.y
                            + 3.0 * u * t * t * b.y
                            + t * t * t * p.y,
                    ));
                }
                last = (p.x, p.y);
            }
            PathVerb::Close => {}
        }
    }
    points
}

/// Group the contours of `text` shaped by `shape` (one call per character, then once for
/// the whole text) into glyph units. The host call count is bounded: `shape` runs once per
/// `char` of `text`, and the callers only get here when the whole text has at most
/// [`MAX_GLYPHS`] characters (4000), so one
/// frame makes at most that many extra `shape-text` calls, and none while the clip is at rest.
/// The grouping is: the contours of consecutive characters, merging a
/// combining mark into the character before it. `None` when the per-character contour
/// counts do not add up to the whole text (the host merged glyphs), so the caller can fall
/// back to the plain text.
pub fn split_contours<E>(
    text: &str,
    whole: &[Vec<PathVerb>],
    mut shape: impl FnMut(&str) -> Result<Vec<Vec<PathVerb>>, E>,
) -> Option<Vec<Vec<Vec<PathVerb>>>> {
    let mut counts: Vec<usize> = Vec::new();
    let mut buffer = [0_u8; 4];
    for ch in text.chars() {
        if is_mark(ch) && !counts.is_empty() {
            let n = shape(ch.encode_utf8(&mut buffer)).ok()?.len();
            *counts.last_mut()? += n;
            continue;
        }
        counts.push(shape(ch.encode_utf8(&mut buffer)).ok()?.len());
    }
    if counts.iter().sum::<usize>() != whole.len() {
        return None;
    }
    let mut at = 0;
    let mut units = Vec::new();
    for n in counts {
        if n > 0 {
            units.push(whole[at..at + n].to_vec());
        }
        at += n;
    }
    Some(units)
}

/// Combining marks and joiners attach to the character before them.
fn is_mark(ch: char) -> bool {
    matches!(u32::from(ch),
        0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF
        | 0xFE00..=0xFE0F | 0xFE20..=0xFE2F | 0x200C | 0x200D)
}

/// Ink extent of a glyph unit along x, used as its cell when no advance box is known.
pub fn ink_cell(contours: &[Vec<PathVerb>]) -> (f32, f32) {
    bounds(contours).map_or((0.0, 0.0), |b| (b.0, b.2))
}
