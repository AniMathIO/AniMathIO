//! First-party `text` plugin: host-side text shaping with size, colour and alignment.

wit_bindgen::generate!({
    path: "../../crates/plugin-api/wit",
    world: "text-layout-source-plugin",
});

use animathio::plugin::host;
use animathio::plugin::text_layout;
use animathio::plugin::types::*;

mod glyph_slide;

struct TextPlugin;

fn get_string(params: &[ParamEntry], id: &str, default: &str) -> String {
    params
        .iter()
        .find(|e| e.id == id)
        .and_then(|e| match &e.value {
            ParamValue::String(s) | ParamValue::EnumChoice(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_else(|| default.to_string())
}

fn get_number(params: &[ParamEntry], id: &str, default: f64) -> f64 {
    params
        .iter()
        .find(|e| e.id == id)
        .and_then(|e| match &e.value {
            ParamValue::Number(n) => Some(*n),
            _ => None,
        })
        .unwrap_or(default)
}

fn get_color(params: &[ParamEntry], id: &str, default: Color) -> Color {
    params
        .iter()
        .find(|e| e.id == id)
        .and_then(|e| match &e.value {
            ParamValue::Color(c) => Some(*c),
            _ => None,
        })
        .unwrap_or(default)
}

fn verb_x(v: &PathVerb) -> f32 {
    match v {
        PathVerb::MoveTo(p) | PathVerb::LineTo(p) => p.x,
        PathVerb::QuadTo((c, to)) => c.x.max(to.x),
        PathVerb::CubicTo((c1, c2, to)) => c1.x.max(c2.x).max(to.x),
        PathVerb::Close => 0.0,
    }
}

fn offset_verb(v: &PathVerb, ox: f32, oy: f32) -> PathVerb {
    match v {
        PathVerb::MoveTo(p) => PathVerb::MoveTo(Point {
            x: p.x + ox,
            y: p.y + oy,
        }),
        PathVerb::LineTo(p) => PathVerb::LineTo(Point {
            x: p.x + ox,
            y: p.y + oy,
        }),
        PathVerb::QuadTo((c, to)) => PathVerb::QuadTo((
            Point {
                x: c.x + ox,
                y: c.y + oy,
            },
            Point {
                x: to.x + ox,
                y: to.y + oy,
            },
        )),
        PathVerb::CubicTo((c1, c2, to)) => PathVerb::CubicTo((
            Point {
                x: c1.x + ox,
                y: c1.y + oy,
            },
            Point {
                x: c2.x + ox,
                y: c2.y + oy,
            },
            Point {
                x: to.x + ox,
                y: to.y + oy,
            },
        )),
        PathVerb::Close => PathVerb::Close,
    }
}

/// Preserve contour winding and subpath boundaries in one non-zero fill. Filling
/// contours separately paints over counters; this also serves captions and titles.
///
/// The host returns one entry per contour, not per glyph. Within the core's path cap
/// (`animathio_core::MAX_PATH_VERBS`) everything stays in ONE path, exactly as before
/// weighted text existed. Only above it (formerly an error) are contours grouped per
/// glyph (text runs left to right: a contour starting at or past the group's right edge
/// begins a new glyph) and whole glyphs are chunked, so a counter never lands in a
/// different path than its outer contour.
fn fill_contours(
    contours: impl IntoIterator<Item = Vec<PathVerb>>,
    color: Color,
) -> Vec<DrawCommand> {
    let contours: Vec<Vec<PathVerb>> = contours.into_iter().collect();
    let total: usize = contours.iter().map(Vec::len).sum();
    if total <= CORE_MAX_PATH_VERBS {
        let path: Vec<PathVerb> = contours.into_iter().flatten().collect();
        return if path.is_empty() {
            Vec::new()
        } else {
            vec![DrawCommand::FillPath((path, color))]
        };
    }
    let mut groups: Vec<Vec<PathVerb>> = Vec::new();
    let mut right = f32::MIN;
    for contour in contours {
        // `Close` carries no point (`verb_x` gives 0), so it must not count.
        let xs = || {
            contour
                .iter()
                .filter(|v| !matches!(v, PathVerb::Close))
                .map(verb_x)
        };
        let left = xs().fold(f32::MAX, f32::min);
        let edge = xs().fold(f32::MIN, f32::max);
        match groups.last_mut() {
            Some(group) if left < right => {
                group.extend(contour);
                right = right.max(edge);
            }
            _ => {
                groups.push(contour);
                right = edge;
            }
        }
    }
    fill_chunks(groups, color)
}

// Fitted titles use a real clip-local box, so the editor's transform handles and
// safe-area coordinates describe the visible composition rather than an empty
// full-canvas text source. Ordinary normalized text keeps the wrapped TextRun path.
fn fitted_text(
    text: &str,
    font_size: f32,
    face: Typeface,
    width: f32,
    height: f32,
    color: Color,
    size: Size,
    slide: Option<&glyph_slide::Slide>,
) -> Result<Scene, Diagnostic> {
    let slide = slide.filter(|_| text.chars().count() <= glyph_slide::MAX_GLYPHS);
    let box_width = width * size.width as f32;
    let box_height = height * size.height as f32;
    let lines: Vec<_> = text.lines().collect();
    let longest = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(1)
        .max(1) as f32;
    let count = lines.len().max(1) as f32;
    let font = (font_size * size.height as f32)
        .min(box_width / longest)
        .min(box_height / (count * 1.5));
    let line_height = font * 1.5;
    let top = (box_height - count * line_height) / 2.0;
    let mut elements = Vec::new();
    let mut layout = glyph_slide::Layout {
        lines: Vec::new(),
        glyphs: Vec::new(),
    };
    for (line, text) in lines.iter().enumerate() {
        let contours = shape_text(text, font, face)?;
        let band_top = top + line as f32 * line_height;
        layout.lines.push(glyph_slide::Band {
            top: band_top,
            bottom: band_top + line_height,
        });
        let min_x = contours
            .iter()
            .flatten()
            .map(verb_x)
            .fold(f32::MAX, f32::min);
        let max_x = contours
            .iter()
            .flatten()
            .map(verb_x)
            .fold(f32::MIN, f32::max);
        if max_x < min_x {
            continue;
        }
        let ox = (box_width - (max_x - min_x)) / 2.0 - min_x;
        let oy = top + line as f32 * line_height + font;
        let point = |p: Point| Point {
            x: (p.x + ox) / box_width,
            y: (p.y + oy) / box_height,
        };
        if slide.is_some() {
            for unit in slide_units(text, font, face, &contours) {
                let shifted: Vec<Vec<PathVerb>> = unit
                    .contours
                    .iter()
                    .map(|c| c.iter().map(|v| offset_verb(v, ox, oy)).collect())
                    .collect();
                let (left, right) = match unit.cell {
                    Some((left, right)) => (left + ox, right + ox),
                    None => glyph_slide::ink_cell(&shifted),
                };
                layout.glyphs.push(glyph_slide::Glyph {
                    contours: shifted,
                    line,
                    left,
                    right,
                });
            }
            continue;
        }
        let paths = contours.into_iter().map(|contour| {
            contour
                .into_iter()
                .map(|verb| match verb {
                    PathVerb::MoveTo(p) => PathVerb::MoveTo(point(p)),
                    PathVerb::LineTo(p) => PathVerb::LineTo(point(p)),
                    PathVerb::QuadTo((c, p)) => PathVerb::QuadTo((point(c), point(p))),
                    PathVerb::CubicTo((a, b, p)) => {
                        PathVerb::CubicTo((point(a), point(b), point(p)))
                    }
                    PathVerb::Close => PathVerb::Close,
                })
                .collect()
        });
        elements.extend(fill_contours(paths, color));
    }
    if let Some(slide) = slide {
        let point = |p: Point| Point {
            x: p.x / box_width,
            y: p.y / box_height,
        };
        elements = glyph_slide::draw(&layout, slide, color, point);
    }
    Ok(Scene {
        coordinate_space: CoordinateSpace::Normalized,
        elements,
    })
}

// ---------------------------------------------------------------------------
// Animated caption styles.
//
// Rendering is a pure function of the clip-local time passed to `render_clip`
// (`source_time`: the clip's source offset plus the time since its start), so preview and
// export agree. Word timings come from the `words` parameter, a space-separated list of
// `start:end` pairs in ticks (one per whitespace-separated word of `text`, relative to the
// start of the caption's source time). Without usable timings the words are spread evenly
// over the stored span; without any timings no word is active.
// ---------------------------------------------------------------------------

const TICKS_PER_SECOND: f64 = 705_600_000.0;
const MAX_ELEMENTS: usize = 9_000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum CaptionStyle {
    Classic,
    WordHighlight,
    Karaoke,
    Pop,
    OneWord,
    BoldOutline,
}

impl CaptionStyle {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "classic" => Self::Classic,
            "word-highlight" => Self::WordHighlight,
            "karaoke" => Self::Karaoke,
            "pop" => Self::Pop,
            "one-word" => Self::OneWord,
            "bold-outline" => Self::BoldOutline,
            _ => return None,
        })
    }
}

fn parse_word_timings(text: &str) -> Vec<(i64, i64)> {
    text.split_whitespace()
        .filter_map(|pair| {
            let (start, end) = pair.split_once(':')?;
            Some((start.parse().ok()?, end.parse().ok()?))
        })
        .collect()
}

/// One timing per word: the stored ones when they match the word count, otherwise the stored
/// span spread over the words by character count.
fn word_timings(words: &[String], stored: &[(i64, i64)]) -> Vec<(i64, i64)> {
    if stored.len() == words.len() {
        return stored.to_vec();
    }
    let (Some(first), Some(last)) = (stored.first(), stored.last()) else {
        return Vec::new();
    };
    let (from, to) = (first.0, last.1.max(first.0));
    let total: usize = words.iter().map(|w| w.chars().count().max(1)).sum();
    let mut done = 0usize;
    words
        .iter()
        .map(|word| {
            let a = from + ((to - from) as i128 * done as i128 / total.max(1) as i128) as i64;
            done += word.chars().count().max(1);
            let b = from + ((to - from) as i128 * done as i128 / total.max(1) as i128) as i64;
            (a, b)
        })
        .collect()
}

/// The active word at `time`: the last word that has started (the first before any starts),
/// how long it has been active and how far through it we are (0..=1).
fn active_word(timings: &[(i64, i64)], time: i64) -> (usize, f32, f32) {
    let Some(index) = timings.iter().rposition(|(start, _)| *start <= time) else {
        return (0, 0.0, 0.0);
    };
    let (start, end) = timings[index];
    let elapsed = (time - start).max(0) as f64 / TICKS_PER_SECOND;
    let progress = if end > start {
        ((time - start) as f64 / (end - start) as f64).clamp(0.0, 1.0)
    } else {
        1.0
    };
    (index, elapsed as f32, progress as f32)
}

fn ease_out_back(p: f32) -> f32 {
    let p = p.clamp(0.0, 1.0);
    let c1 = 1.4;
    let c3 = c1 + 1.0;
    1.0 + c3 * (p - 1.0).powi(3) + c1 * (p - 1.0).powi(2)
}

fn ease_out_cubic(p: f32) -> f32 {
    1.0 - (1.0 - p.clamp(0.0, 1.0)).powi(3)
}

fn contrast_color(color: Color) -> Color {
    let luma =
        0.2126 * f32::from(color.r) + 0.7152 * f32::from(color.g) + 0.0722 * f32::from(color.b);
    if luma > 150.0 {
        Color {
            r: 16,
            g: 16,
            b: 24,
            a: 255,
        }
    } else {
        Color {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        }
    }
}

struct ShapedWord {
    contours: Vec<Vec<PathVerb>>,
    min_x: f32,
    width: f32,
}

fn shape_word(text: &str, font: f32, face: Typeface) -> Result<ShapedWord, Diagnostic> {
    let contours = shape_text(text, font, face)?;
    let min_x = contours
        .iter()
        .flatten()
        .map(verb_x)
        .fold(f32::MAX, f32::min);
    let max_x = contours
        .iter()
        .flatten()
        .map(verb_x)
        .fold(f32::MIN, f32::max);
    if max_x < min_x {
        return Ok(ShapedWord {
            contours,
            min_x: 0.0,
            width: font * 0.3,
        });
    }
    Ok(ShapedWord {
        contours,
        min_x,
        width: max_x - min_x,
    })
}

/// Placement of one word in box pixels: ink starts at `x`, `baseline` is the glyph origin
/// line, and the word is scaled by `scale` around `(cx, cy)`.
#[derive(Clone, Copy)]
struct Place {
    x: f32,
    baseline: f32,
    k: f32,
    scale: f32,
    cx: f32,
    cy: f32,
}

fn place_contours(word: &ShapedWord, at: Place) -> Vec<Vec<PathVerb>> {
    let map = |p: Point| {
        let x = at.x + (p.x - word.min_x) * at.k;
        let y = at.baseline + p.y * at.k;
        Point {
            x: at.cx + (x - at.cx) * at.scale,
            y: at.cy + (y - at.cy) * at.scale,
        }
    };
    word.contours
        .iter()
        .map(|contour| {
            contour
                .iter()
                .map(|verb| match verb {
                    PathVerb::MoveTo(p) => PathVerb::MoveTo(map(*p)),
                    PathVerb::LineTo(p) => PathVerb::LineTo(map(*p)),
                    PathVerb::QuadTo((c, p)) => PathVerb::QuadTo((map(*c), map(*p))),
                    PathVerb::CubicTo((a, b, p)) => PathVerb::CubicTo((map(*a), map(*b), map(*p))),
                    PathVerb::Close => PathVerb::Close,
                })
                .collect()
        })
        .collect()
}

struct Canvas {
    width: f32,
    height: f32,
    elements: Vec<DrawCommand>,
}

impl Canvas {
    fn norm(&self, p: Point, dx: f32, dy: f32) -> Point {
        Point {
            x: (p.x + dx) / self.width,
            y: (p.y + dy) / self.height,
        }
    }

    /// Fill all contours as one path so counters (the holes of a, o, e) stay open.
    fn fill(&mut self, contours: &[Vec<PathVerb>], color: Color, dx: f32, dy: f32) {
        let path: Vec<PathVerb> = contours
            .iter()
            .flatten()
            .map(|verb| match verb {
                PathVerb::MoveTo(p) => PathVerb::MoveTo(self.norm(*p, dx, dy)),
                PathVerb::LineTo(p) => PathVerb::LineTo(self.norm(*p, dx, dy)),
                PathVerb::QuadTo((c, p)) => {
                    PathVerb::QuadTo((self.norm(*c, dx, dy), self.norm(*p, dx, dy)))
                }
                PathVerb::CubicTo((a, b, p)) => PathVerb::CubicTo((
                    self.norm(*a, dx, dy),
                    self.norm(*b, dx, dy),
                    self.norm(*p, dx, dy),
                )),
                PathVerb::Close => PathVerb::Close,
            })
            .collect();
        if !path.is_empty() {
            self.elements.extend(fill_contours([path], color));
        }
    }

    /// Thick outline: the glyphs repeated around a ring, drawn before the fill.
    fn outline(&mut self, contours: &[Vec<PathVerb>], color: Color, radius: f32) {
        if radius <= 0.0 || color.a == 0 {
            return;
        }
        const STEPS: usize = 12;
        if self.elements.len() + STEPS > MAX_ELEMENTS {
            return;
        }
        for step in 0..STEPS {
            let angle = step as f32 * std::f32::consts::TAU / STEPS as f32;
            self.fill(contours, color, radius * angle.cos(), radius * angle.sin());
        }
    }

    fn pill(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, color: Color) {
        let r = (y1 - y0).min(x1 - x0) * 0.32;
        let q = |cx: f32, cy: f32, x: f32, y: f32| {
            PathVerb::QuadTo((Point { x: cx, y: cy }, Point { x, y }))
        };
        let m = |x: f32, y: f32| Point { x, y };
        let path = vec![
            PathVerb::MoveTo(m(x0 + r, y0)),
            PathVerb::LineTo(m(x1 - r, y0)),
            q(x1, y0, x1, y0 + r),
            PathVerb::LineTo(m(x1, y1 - r)),
            q(x1, y1, x1 - r, y1),
            PathVerb::LineTo(m(x0 + r, y1)),
            q(x0, y1, x0, y1 - r),
            PathVerb::LineTo(m(x0, y0 + r)),
            q(x0, y0, x0 + r, y0),
            PathVerb::Close,
        ];
        self.fill(&[path], color, 0.0, 0.0);
    }
}

fn flatten(contour: &[PathVerb]) -> Vec<(f32, f32)> {
    let mut points: Vec<(f32, f32)> = Vec::new();
    let mut last = (0.0f32, 0.0f32);
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

/// Sutherland-Hodgman clip of a closed polygon to `x <= cut`.
fn clip_left(points: &[(f32, f32)], cut: f32) -> Vec<(f32, f32)> {
    let mut out = Vec::new();
    for i in 0..points.len() {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        let (a_in, b_in) = (a.0 <= cut, b.0 <= cut);
        if a_in {
            out.push(a);
        }
        if a_in != b_in && (b.0 - a.0).abs() > f32::EPSILON {
            let t = (cut - a.0) / (b.0 - a.0);
            out.push((cut, a.1 + t * (b.1 - a.1)));
        }
    }
    out
}

fn clipped_contours(contours: &[Vec<PathVerb>], cut: f32) -> Vec<Vec<PathVerb>> {
    contours
        .iter()
        .filter_map(|contour| {
            let polygon = clip_left(&flatten(contour), cut);
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

#[allow(clippy::too_many_arguments)]
fn caption_scene(
    style: CaptionStyle,
    params: &[ParamEntry],
    text: &str,
    font_size: f32,
    face: Typeface,
    width: f32,
    height: f32,
    color: Color,
    time_ticks: i64,
    size: Size,
) -> Result<Scene, Diagnostic> {
    let box_w = width * size.width as f32;
    let box_h = height * size.height as f32;
    let mut canvas = Canvas {
        width: box_w,
        height: box_h,
        elements: Vec::new(),
    };
    let mut words: Vec<String> = text.split_whitespace().map(str::to_owned).collect();
    if words.is_empty() || box_w <= 0.0 || box_h <= 0.0 {
        return Ok(Scene {
            coordinate_space: CoordinateSpace::Normalized,
            elements: Vec::new(),
        });
    }
    if style == CaptionStyle::BoldOutline {
        for word in &mut words {
            *word = word.to_uppercase();
        }
    }
    let accent = get_color(
        params,
        "accent",
        Color {
            r: 255,
            g: 214,
            b: 10,
            a: 255,
        },
    );
    let outline_color = get_color(
        params,
        "outline",
        Color {
            r: 0,
            g: 0,
            b: 0,
            a: 255,
        },
    );
    let default_outline = if style == CaptionStyle::BoldOutline {
        0.1
    } else {
        0.0
    };
    let outline_width =
        (get_number(params, "outline_width", default_outline) as f32).clamp(0.0, 0.5);
    let max_words = get_number(params, "max_words", 0.0).max(0.0) as usize;
    let timings = word_timings(
        &words,
        &parse_word_timings(&get_string(params, "words", "")),
    );
    let (active, elapsed, progress) = if timings.is_empty() {
        (usize::MAX, 0.0, 0.0)
    } else {
        active_word(&timings, time_ticks)
    };
    let one_word = style == CaptionStyle::OneWord;
    let line_limit = if one_word { 1 } else { 2 };

    let base_font = (font_size * size.height as f32).max(1.0) * if one_word { 1.5 } else { 1.0 };
    // Shape every word once at the reference size; geometry is scaled afterwards.
    let shown = if active == usize::MAX {
        0
    } else {
        active.min(words.len() - 1)
    };
    let shaped: Vec<ShapedWord> = if one_word {
        let index = shown;
        let mut all: Vec<ShapedWord> = (0..words.len())
            .map(|_| ShapedWord {
                contours: Vec::new(),
                min_x: 0.0,
                width: 0.0,
            })
            .collect();
        all[index] = shape_word(&words[index], base_font, face)?;
        all
    } else {
        words
            .iter()
            .map(|word| shape_word(word, base_font, face))
            .collect::<Result<_, _>>()?
    };
    let gap = base_font * 0.28;
    // Greedy line breaking at k = 1 on width and `max_words`.
    let limit = box_w * 0.97;
    let mut lines: Vec<Vec<usize>> = vec![Vec::new()];
    let mut line_width = 0.0f32;
    if one_word {
        lines = vec![vec![shown]];
    } else {
        for (index, word) in shaped.iter().enumerate() {
            let current = lines.last_mut().expect("at least one line");
            let next = if current.is_empty() {
                word.width
            } else {
                line_width + gap + word.width
            };
            let full = max_words > 0 && current.len() >= max_words;
            if !current.is_empty() && (next > limit || full) {
                lines.push(vec![index]);
                line_width = word.width;
            } else {
                current.push(index);
                line_width = next;
            }
        }
    }
    let width_of = |line: &[usize]| -> f32 {
        line.iter().map(|i| shaped[*i].width).sum::<f32>()
            + gap * line.len().saturating_sub(1) as f32
    };
    let widest = lines
        .iter()
        .map(|l| width_of(l))
        .fold(0.0f32, f32::max)
        .max(1.0);
    let shown_lines = lines.len().min(line_limit);
    let line_height = base_font * 1.25;
    let mut k = 1.0f32;
    k = k.min(limit / widest);
    k = k.min(box_h / (shown_lines as f32 * line_height));
    let active_line = lines.iter().position(|l| l.contains(&active)).unwrap_or(0);
    let first = active_line / line_limit * line_limit;
    let top = (box_h - shown_lines as f32 * line_height * k) / 2.0;
    let cap_half = base_font * 0.35 * k;
    // Time-driven pop: the active word eases up, the previous one eases back down.
    let pop_up = |word: usize| -> f32 {
        if style != CaptionStyle::Pop && !one_word {
            return 1.0;
        }
        if word == active {
            if one_word {
                0.78 + 0.22 * ease_out_back(elapsed / 0.12)
            } else {
                1.0 + 0.25 * ease_out_back(elapsed / 0.16)
            }
        } else if style == CaptionStyle::Pop && active != usize::MAX && word + 1 == active {
            1.0 + 0.25 * (1.0 - ease_out_cubic(elapsed / 0.12))
        } else {
            1.0
        }
    };
    for (row, line) in lines.iter().enumerate().skip(first).take(line_limit) {
        let row_width = width_of(line) * k;
        let mut x = (box_w - row_width) / 2.0;
        let baseline =
            top + (row - first) as f32 * line_height * k + line_height * k / 2.0 + cap_half;
        for &index in line {
            let word = &shaped[index];
            let w = word.width * k;
            let is_active = index == active;
            let spoken = active != usize::MAX && index < active;
            let at = Place {
                x,
                baseline,
                k,
                scale: pop_up(index),
                cx: x + w / 2.0,
                cy: baseline - cap_half,
            };
            let placed = place_contours(word, at);
            let (fill, pill) = match style {
                CaptionStyle::WordHighlight if is_active => (contrast_color(accent), true),
                CaptionStyle::Pop if is_active => (accent, false),
                CaptionStyle::BoldOutline if is_active => (accent, false),
                CaptionStyle::Karaoke if spoken => (accent, false),
                _ => (color, false),
            };
            if pill {
                // The pill belongs to the word: it pads the word's own glyphs, but it
                // must not swallow the inter-word spaces. Cap it at half of the gap so
                // a clear space stays visible between the pill and each neighbour.
                let pad_x = gap * 0.5 * k;
                let pad_y = base_font * 0.5 * k;
                canvas.pill(
                    x - pad_x,
                    at.cy - pad_y,
                    x + w + pad_x,
                    at.cy + pad_y,
                    accent,
                );
            }
            canvas.outline(&placed, outline_color, outline_width * base_font * k);
            canvas.fill(&placed, fill, 0.0, 0.0);
            if style == CaptionStyle::Karaoke && is_active && progress > 0.0 {
                let cut = x + w * progress;
                canvas.fill(&clipped_contours(&placed, cut), accent, 0.0, 0.0);
            }
            x += w + gap * k;
        }
    }
    Ok(Scene {
        coordinate_space: CoordinateSpace::Normalized,
        elements: canvas.elements,
    })
}

impl Guest for TextPlugin {
    fn describe() -> Manifest {
        Manifest {
            name: "text".to_string(),
            version: "1.4.1".to_string(),
            description:
                "Host-shaped text with size, colour, alignment, animated caption styles and per-character slides"
                    .to_string(),
            kinds: vec!["clip-source".into()],
            metadata: vec![],
            params: vec![
                ParamSpec {
                    id: "fit_width".into(),
                    label: "Fit width".into(),
                    kind: ParamKind::Number(NumberRange {
                        min: 0.0,
                        max: 1.0,
                        default: 0.0,
                        step: 0.01,
                    }),
                },
                ParamSpec {
                    id: "fit_height".into(),
                    label: "Fit height".into(),
                    kind: ParamKind::Number(NumberRange {
                        min: 0.0,
                        max: 1.0,
                        default: 0.0,
                        step: 0.01,
                    }),
                },
                ParamSpec {
                    id: "units".into(),
                    label: "Units".into(),
                    kind: ParamKind::EnumChoice(EnumParam {
                        options: vec!["pixels".into(), "normalized".into()],
                        default: "pixels".into(),
                    }),
                },
                ParamSpec {
                    id: "text".to_string(),
                    label: "Text".to_string(),
                    kind: ParamKind::String(StringParam {
                        default: "AniMathIO".to_string(),
                    }),
                },
                ParamSpec {
                    id: "font_weight".into(),
                    label: "Font weight".into(),
                    kind: ParamKind::Number(NumberRange {
                        min: 100.0,
                        max: 900.0,
                        default: 400.0,
                        step: 100.0,
                    }),
                },
                ParamSpec {
                    id: "font".into(),
                    label: "Font".into(),
                    kind: ParamKind::EnumChoice(EnumParam {
                        options: vec!["dejavu".into(), "inter".into()],
                        default: "dejavu".into(),
                    }),
                },
                ParamSpec {
                    id: "font_size".to_string(),
                    label: "Font size".to_string(),
                    kind: ParamKind::Number(NumberRange {
                        min: 0.0001,
                        max: 4096.0,
                        default: 48.0,
                        step: 0.001,
                    }),
                },
                ParamSpec {
                    id: "color".to_string(),
                    label: "Color".to_string(),
                    kind: ParamKind::Color(ColorParam {
                        default: Color {
                            r: 255,
                            g: 255,
                            b: 255,
                            a: 255,
                        },
                    }),
                },
                ParamSpec {
                    id: "caption_style".into(),
                    label: "Caption style".into(),
                    kind: ParamKind::EnumChoice(EnumParam {
                        options: vec![
                            "off".into(),
                            "classic".into(),
                            "word-highlight".into(),
                            "karaoke".into(),
                            "pop".into(),
                            "one-word".into(),
                            "bold-outline".into(),
                        ],
                        default: "off".into(),
                    }),
                },
                ParamSpec {
                    id: "words".into(),
                    label: "Word timings".into(),
                    kind: ParamKind::String(StringParam {
                        default: String::new(),
                    }),
                },
                ParamSpec {
                    id: "accent".into(),
                    label: "Accent".into(),
                    kind: ParamKind::Color(ColorParam {
                        default: Color {
                            r: 255,
                            g: 214,
                            b: 10,
                            a: 255,
                        },
                    }),
                },
                ParamSpec {
                    id: "outline".into(),
                    label: "Outline".into(),
                    kind: ParamKind::Color(ColorParam {
                        default: Color {
                            r: 0,
                            g: 0,
                            b: 0,
                            a: 255,
                        },
                    }),
                },
                ParamSpec {
                    id: "outline_width".into(),
                    label: "Outline width".into(),
                    kind: ParamKind::Number(NumberRange {
                        min: 0.0,
                        max: 0.5,
                        default: 0.0,
                        step: 0.01,
                    }),
                },
                ParamSpec {
                    id: "max_words".into(),
                    label: "Words per line".into(),
                    kind: ParamKind::Number(NumberRange {
                        min: 0.0,
                        max: 12.0,
                        default: 0.0,
                        step: 1.0,
                    }),
                },
                ParamSpec {
                    id: "reveal".to_string(),
                    label: "Reveal".to_string(),
                    kind: ParamKind::Number(NumberRange {
                        min: 0.0,
                        max: 1.0,
                        default: 1.0,
                        step: 0.01,
                    }),
                },
                ParamSpec {
                    id: "char_slide".into(),
                    label: "Character slide (1 is at rest)".into(),
                    kind: ParamKind::Number(NumberRange {
                        min: 0.0,
                        max: 1.0,
                        default: 1.0,
                        step: 0.01,
                    }),
                },
                ParamSpec {
                    id: "char_slide_direction".into(),
                    label: "Character slide direction (1 left, 2 right, 3 top, 4 bottom)".into(),
                    kind: ParamKind::Number(NumberRange {
                        min: 1.0,
                        max: 4.0,
                        default: 1.0,
                        step: 1.0,
                    }),
                },
                ParamSpec {
                    id: "char_slide_stagger".into(),
                    label: "Character slide stagger (negative starts with the last)".into(),
                    kind: ParamKind::Number(NumberRange {
                        min: -1.0,
                        max: 1.0,
                        default: 0.5,
                        step: 0.01,
                    }),
                },
                ParamSpec {
                    id: "char_slide_clip".into(),
                    label: "Character slide mask (0 off, 1 on)".into(),
                    kind: ParamKind::Number(NumberRange {
                        min: 0.0,
                        max: 1.0,
                        default: 1.0,
                        step: 1.0,
                    }),
                },
                ParamSpec {
                    id: "char_slide_fade".into(),
                    label: "Character slide fade".into(),
                    kind: ParamKind::Number(NumberRange {
                        min: 0.0,
                        max: 1.0,
                        default: 0.0,
                        step: 0.01,
                    }),
                },
                ParamSpec {
                    id: "align".to_string(),
                    label: "Alignment".to_string(),
                    kind: ParamKind::EnumChoice(EnumParam {
                        options: vec![
                            "left".to_string(),
                            "center".to_string(),
                            "right".to_string(),
                        ],
                        default: "center".to_string(),
                    }),
                },
            ],
        }
    }

    fn render_clip(
        params: Vec<ParamEntry>,
        time_ticks: i64,
        size: Size,
    ) -> Result<Scene, Diagnostic> {
        let text = revealed(
            &get_string(&params, "text", "AniMathIO"),
            get_number(&params, "reveal", 1.0),
        );
        let font_size = get_number(&params, "font_size", 48.0) as f32;
        let face = Typeface::from_params(&params);
        let color = get_color(
            &params,
            "color",
            Color {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
        );
        let align = get_string(&params, "align", "center");
        // A caption style (Classic included) owns the clip's motion, so the character
        // slide parameters are ignored whenever one is set.
        let styled = CaptionStyle::parse(&get_string(&params, "caption_style", "off")).is_some();
        let slide = glyph_slide::Slide::from_params(
            get_number(&params, "char_slide", 1.0),
            get_number(&params, "char_slide_direction", 1.0),
            get_number(&params, "char_slide_stagger", 0.5),
            get_number(&params, "char_slide_clip", 1.0),
            get_number(&params, "char_slide_fade", 0.0),
        )
        .filter(|_| !styled && text.chars().count() <= glyph_slide::MAX_GLYPHS);

        if get_string(&params, "units", "pixels") == "normalized" {
            let fit_width = get_number(&params, "fit_width", 0.0) as f32;
            let fit_height = get_number(&params, "fit_height", 0.0) as f32;
            if fit_width > 0.0 && fit_height > 0.0 {
                let style = CaptionStyle::parse(&get_string(&params, "caption_style", "off"));
                if let Some(style) = style.filter(|s| *s != CaptionStyle::Classic) {
                    return caption_scene(
                        style, &params, &text, font_size, face, fit_width, fit_height, color,
                        time_ticks, size,
                    );
                }
                return fitted_text(
                    &text,
                    font_size,
                    face,
                    fit_width,
                    fit_height,
                    color,
                    size,
                    slide.as_ref(),
                );
            }
            if let Some(slide) = &slide {
                return if face.outlined() {
                    weighted_slide(&text, font_size, face, &align, color, size, slide)
                } else {
                    wrapped_slide(&text, font_size, &align, color, size, slide)
                };
            }
            if face.outlined() {
                return weighted_text_run(&text, font_size, face, &align, color, size);
            }
            // Generic TextRun uses the host's wrapped, embedded-font layout.
            // Keep coordinates and font size relative to the unscaled canvas.
            return Ok(Scene {
                coordinate_space: CoordinateSpace::Normalized,
                elements: vec![DrawCommand::TextRun(TextElement {
                    align: match align.as_str() {
                        "center" => TextAlignment::Center,
                        "right" => TextAlignment::Right,
                        _ => TextAlignment::Left,
                    },
                    text,
                    font_size,
                    x: 0.0,
                    y: 0.0,
                    color,
                })],
            });
        }

        let contours = shape_text(&text, font_size, face)?;

        let mut min_x = f32::MAX;
        let mut max_x = f32::MIN;
        for contour in &contours {
            for verb in contour {
                let x = verb_x(verb);
                min_x = min_x.min(x);
                max_x = max_x.max(x);
            }
        }
        let text_width = if max_x >= min_x { max_x - min_x } else { 0.0 };
        let x_offset = match align.as_str() {
            "right" => size.width as f32 - text_width,
            "center" => (size.width as f32 - text_width) / 2.0,
            _ => 0.0,
        } - min_x;
        let y_offset = size.height as f32 * 0.5;

        if let Some(slide) = &slide {
            return Ok(Scene {
                coordinate_space: CoordinateSpace::Canvas,
                elements: pixel_slide(
                    &text,
                    font_size,
                    face,
                    &contours,
                    (x_offset, y_offset),
                    color,
                    slide,
                )?,
            });
        }
        let paths = contours.into_iter().map(|contour| {
            contour
                .iter()
                .map(|v| offset_verb(v, x_offset, y_offset))
                .collect()
        });
        let elements = fill_contours(paths, color);

        Ok(Scene {
            coordinate_space: CoordinateSpace::Canvas,
            elements,
        })
    }
}

/// DejaVu Sans line box (ascent and descent) in em, for the single-line pixel layout.
const ASCENT: f32 = 0.93;
const DESCENT: f32 = 0.24;

/// Per-glyph slide of a wrapped, normalized text: the glyph units come from the host's
/// layout (the same one a `text-run` is drawn with), so glyphs sit where the plain text
/// puts them.
fn wrapped_slide(
    text: &str,
    font_size: f32,
    align: &str,
    color: Color,
    size: Size,
    slide: &glyph_slide::Slide,
) -> Result<Scene, Diagnostic> {
    let pixels = font_size * size.height as f32;
    let lines = text_layout::layout_text(
        text,
        pixels,
        size,
        match align {
            "center" => TextAlignment::Center,
            "right" => TextAlignment::Right,
            _ => TextAlignment::Left,
        },
    )?;
    let mut layout = glyph_slide::Layout {
        lines: Vec::new(),
        glyphs: Vec::new(),
    };
    for (line, row) in lines.into_iter().enumerate() {
        layout.lines.push(glyph_slide::Band {
            top: row.top,
            bottom: row.bottom,
        });
        for cluster in row.clusters {
            if cluster.outline.is_empty() {
                continue;
            }
            layout.glyphs.push(glyph_slide::Glyph {
                contours: cluster.outline,
                line,
                left: cluster.left,
                right: cluster.right,
            });
        }
    }
    let (width, height) = (size.width as f32, size.height as f32);
    let elements = glyph_slide::draw(&layout, slide, color, |p| Point {
        x: p.x / width,
        y: p.y / height,
    });
    Ok(Scene {
        coordinate_space: CoordinateSpace::Normalized,
        elements,
    })
}

/// A slide unit of one line before it is placed: its contours in the line's coordinates and
/// its advance cell (`None`: use the ink, as the host layout gives no advances).
struct SlideUnit {
    contours: Vec<Vec<PathVerb>>,
    cell: Option<(f32, f32)>,
}

/// The glyph units of one line whose whole outline is `whole`. Outlined (weighted or Inter)
/// text takes them from the Inter shaper that drew `whole`; the host's text splits its
/// `shape-text` contours per character: one host call per `char` of `text`, bounded by
/// [`glyph_slide::MAX_GLYPHS`] over the whole text (the slide is dropped above that many
/// characters) and only mid-animation.
fn slide_units(
    text: &str,
    size: f32,
    face: Typeface,
    whole: &[Vec<PathVerb>],
) -> Vec<SlideUnit> {
    if face.outlined() {
        if let Ok(shaper) = Shaper::get(face.weight) {
            return shaper
                .units(text, size)
                .into_iter()
                .map(|unit| SlideUnit {
                    contours: unit.contours,
                    cell: Some((unit.left, unit.right)),
                })
                .collect();
        }
    }
    glyph_slide::split_contours(text, whole, |s| host::shape_text(s, size))
        .unwrap_or_else(|| vec![whole.to_vec()])
        .into_iter()
        .map(|contours| SlideUnit {
            contours,
            cell: None,
        })
        .collect()
}

/// Per-glyph slide of the single-line pixel layout.
fn pixel_slide(
    text: &str,
    font_size: f32,
    face: Typeface,
    contours: &[Vec<PathVerb>],
    (ox, oy): (f32, f32),
    color: Color,
    slide: &glyph_slide::Slide,
) -> Result<Vec<DrawCommand>, Diagnostic> {
    // The line box of the typeface: Inter's own metrics for outlined text, DejaVu's otherwise.
    let (above, below) = if face.outlined() {
        let metrics = Shaper::get(face.weight)?.line_metrics(font_size);
        (metrics.ascent, metrics.line_height - metrics.ascent)
    } else {
        (ASCENT * font_size, DESCENT * font_size)
    };
    let layout = glyph_slide::Layout {
        lines: vec![glyph_slide::Band {
            top: oy - above,
            bottom: oy + below,
        }],
        glyphs: slide_units(text, font_size, face, contours)
            .into_iter()
            .map(|unit| {
                let contours: Vec<Vec<PathVerb>> = unit
                    .contours
                    .iter()
                    .map(|c| c.iter().map(|v| offset_verb(v, ox, oy)).collect())
                    .collect();
                let (left, right) = match unit.cell {
                    Some((left, right)) => (left + ox, right + ox),
                    None => glyph_slide::ink_cell(&contours),
                };
                glyph_slide::Glyph {
                    contours,
                    line: 0,
                    left,
                    right,
                }
            })
            .collect(),
    };
    Ok(glyph_slide::draw(&layout, slide, color, |p| p))
}

/// The first `reveal` fraction of the characters (typewriter presets keyframe it).
fn revealed(text: &str, reveal: f64) -> String {
    if !(reveal < 1.0) {
        return text.to_owned();
    }
    let count = text.chars().count() as f64;
    let shown = (reveal.max(0.0) * count + 1e-9).floor() as usize;
    text.chars().take(shown).collect()
}

export!(TextPlugin);

/// Bundled Inter variable font: outlines every weight other than regular, and regular too
/// when the clip's `font` is `inter`.
const INTER: &[u8] = include_bytes!("../../../apps/animathio/assets/fonts/InterVariable.ttf");
/// Bundled DejaVu Sans (the host's own face): fallback for characters Inter lacks
/// (maths operators such as `∀ ∈ ℝ`, Hebrew, ...), so weighted text never shows `.notdef` boxes.
const DEJAVU: &[u8] = include_bytes!("../../../assets/fonts/DejaVuSans.ttf");

/// Same limits as the host `shape-text` import and renderer, so weighted text is no looser.
const MAX_TEXT_CHARS: usize = 10_000;
const MAX_TEXT_VERBS: usize = 200_000;
const MAX_FONT_PX: f32 = 1_048_560.0;
/// The core caps one path at 100,000 verbs; stay well below it by splitting into elements.
const MAX_VERBS_PER_ELEMENT: usize = 40_000;
/// Mirror of `animathio_core::MAX_PATH_VERBS` (the plugin does not link the core).
const CORE_MAX_PATH_VERBS: usize = 100_000;
/// A tab advances 0.6 em: what the host's regular text measures (its tab maps to DejaVu's
/// `.notdef`, 0.6 em wide). The host also draws that box; weighted text only advances.
const TAB_ADVANCE_EM: f32 = 0.6;

/// Typeface selection of one text clip: the `font_weight` and `font` parameters.
///
/// `font = dejavu` (the default, and what every project saved before the parameter existed
/// uses) keeps the host's DejaVu Sans `TextRun`/`shape-text` at weight 400 and outlines Inter
/// at any other weight. `font = inter` outlines every weight, 400 included, from Inter, so
/// keyframing the weight never switches typeface.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Typeface {
    weight: f32,
    inter: bool,
}

impl Typeface {
    fn from_params(params: &[ParamEntry]) -> Self {
        Self {
            weight: get_number(params, "font_weight", 400.0).clamp(100.0, 900.0) as f32,
            inter: get_string(params, "font", "dejavu") == "inter",
        }
    }

    /// True when the plugin outlines the text itself instead of using the host's shaping.
    fn outlined(self) -> bool {
        self.inter || self.weight != 400.0
    }
}

fn invalid(message: &str) -> Diagnostic {
    Diagnostic {
        code: "text.invalid".into(),
        message: message.into(),
        metadata: vec![],
    }
}

fn check_size(px: f32) -> Result<(), Diagnostic> {
    if px.is_finite() && px > 0.0 && px <= MAX_FONT_PX {
        Ok(())
    } else {
        Err(invalid("invalid font size"))
    }
}

fn check_text(text: &str) -> Result<(), Diagnostic> {
    // Counting stops early: a longer string is rejected the same way as the host does.
    if text.chars().nth(MAX_TEXT_CHARS).is_some() {
        Err(invalid("text is too long (max 10000 characters)"))
    } else {
        Ok(())
    }
}

fn check_verbs(contours: &[Vec<PathVerb>]) -> Result<(), Diagnostic> {
    if contours.iter().map(Vec::len).sum::<usize>() > MAX_TEXT_VERBS {
        Err(invalid("text has too many path verbs"))
    } else {
        Ok(())
    }
}

fn shape_text(text: &str, size: f32, face: Typeface) -> Result<Vec<Vec<PathVerb>>, Diagnostic> {
    if !face.outlined() {
        return host::shape_text(text, size);
    }
    check_size(size)?;
    check_text(text)?;
    let contours = Shaper::get(face.weight)?.outline(text, size);
    check_verbs(&contours)?;
    Ok(contours)
}

/// Glyph data in em units (size 1, y down, baseline at 0), cached per character.
struct CachedGlyph {
    advance: f32,
    contours: Vec<Vec<PathVerb>>,
    /// True when the glyph comes from the DejaVu fallback face.
    fallback: bool,
}

/// Inter instanced at one `wght` value with DejaVu Sans as per-character fallback. Glyphs are
/// laid out left to right by their advances (no kerning or ligatures), which is deterministic
/// and needs no host import. Outlines and advances are cached per character, so a long
/// paragraph pays for each distinct letter once, which keeps it inside the fuel budget.
///
/// The DejaVu fallback is the regular face at every weight (no synthetic emboldening): the
/// characters that need it (maths operators, Hebrew, ...) keep the host's look.
struct Shaper {
    inter: skrifa::FontRef<'static>,
    dejavu: skrifa::FontRef<'static>,
    location: skrifa::instance::Location,
    glyphs: std::cell::RefCell<std::collections::HashMap<char, std::rc::Rc<CachedGlyph>>>,
}

struct LineMetrics {
    ascent: f32,
    line_height: f32,
}

thread_local! {
    /// Shapers by weight bits; one render call usually needs a single weight.
    static SHAPERS: std::cell::RefCell<Vec<(u32, std::rc::Rc<Shaper>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

impl Shaper {
    fn new(weight: f32) -> Result<Self, Diagnostic> {
        use skrifa::MetadataProvider;
        let inter = skrifa::FontRef::new(INTER).map_err(|_| invalid("bundled font is invalid"))?;
        let dejavu =
            skrifa::FontRef::new(DEJAVU).map_err(|_| invalid("bundled font is invalid"))?;
        let location = inter.axes().location([(skrifa::Tag::new(b"wght"), weight)]);
        Ok(Self {
            inter,
            dejavu,
            location,
            glyphs: Default::default(),
        })
    }

    fn get(weight: f32) -> Result<std::rc::Rc<Self>, Diagnostic> {
        SHAPERS.with(|cell| {
            let mut shapers = cell.borrow_mut();
            if let Some((_, shaper)) = shapers.iter().find(|(bits, _)| *bits == weight.to_bits()) {
                return Ok(shaper.clone());
            }
            let shaper = std::rc::Rc::new(Self::new(weight)?);
            shapers.push((weight.to_bits(), shaper.clone()));
            Ok(shaper)
        })
    }

    fn glyph(&self, ch: char) -> std::rc::Rc<CachedGlyph> {
        if let Some(glyph) = self.glyphs.borrow().get(&ch) {
            return glyph.clone();
        }
        let glyph = std::rc::Rc::new(self.build(ch));
        self.glyphs.borrow_mut().insert(ch, glyph.clone());
        glyph
    }

    fn build(&self, ch: char) -> CachedGlyph {
        use skrifa::MetadataProvider;
        let inter_id = self
            .inter
            .charmap()
            .map(ch)
            .filter(|id| *id != skrifa::GlyphId::NOTDEF);
        // Inter when it has the character; otherwise DejaVu (its own `.notdef` box when
        // neither face has it, like the host's regular text).
        let (font, location, id, fallback) = match inter_id {
            Some(id) => (
                &self.inter,
                skrifa::instance::LocationRef::from(&self.location),
                id,
                false,
            ),
            None => (
                &self.dejavu,
                skrifa::instance::LocationRef::default(),
                self.dejavu.charmap().map(ch).unwrap_or_default(),
                true,
            ),
        };
        let unscaled = skrifa::instance::Size::unscaled();
        let per_em = f32::from(font.metrics(unscaled, location).units_per_em)
            .max(1.0)
            .recip();
        let advance = font
            .glyph_metrics(unscaled, location)
            .advance_width(id)
            .map_or(0.6, |units| units * per_em);
        let mut pen = OutlinePen {
            per_em,
            ..OutlinePen::default()
        };
        if let Some(glyph) = font.outline_glyphs().get(id) {
            let _ = glyph.draw(
                skrifa::outline::DrawSettings::unhinted(unscaled, location),
                &mut pen,
            );
        }
        CachedGlyph {
            advance,
            contours: pen.finish(),
            fallback,
        }
    }

    /// Whether `ch` is drawn from the fallback face (or is `.notdef` in both).
    #[cfg(test)]
    fn uses_fallback(&self, ch: char) -> bool {
        self.glyph(ch).fallback
    }

    fn advance(&self, ch: char, size: f32) -> f32 {
        match ch {
            '\t' => TAB_ADVANCE_EM * size,
            c if c.is_control() => 0.0,
            c => self.glyph(c).advance * size,
        }
    }

    fn width(&self, text: &str, size: f32) -> f32 {
        text.chars().map(|ch| self.advance(ch, size)).sum()
    }

    fn line_metrics(&self, size: f32) -> LineMetrics {
        use skrifa::MetadataProvider;
        let metrics = self
            .inter
            .metrics(skrifa::instance::Size::new(size), &self.location);
        LineMetrics {
            ascent: metrics.ascent,
            line_height: metrics.ascent - metrics.descent + metrics.leading,
        }
    }

    /// Outlines for one line with the baseline at y = 0 and the pen at x = 0
    /// (y grows downwards, like the host's `shape-text`), one path per glyph. Control
    /// characters draw nothing.
    fn outline(&self, text: &str, size: f32) -> Vec<Vec<PathVerb>> {
        let mut paths = Vec::new();
        let mut cursor = 0.0;
        for ch in text.chars() {
            if !ch.is_control() {
                let glyph = self.glyph(ch);
                // One path per glyph: its contours (outer shape and counters) stay together
                // when the scene is split into several fills, so a counter is never filled.
                let path: Vec<PathVerb> = glyph
                    .contours
                    .iter()
                    .flatten()
                    .map(|verb| {
                        map_verb(verb, |p| Point {
                            x: cursor + p.x * size,
                            y: p.y * size,
                        })
                    })
                    .collect();
                if !path.is_empty() {
                    paths.push(path);
                }
            }
            cursor += self.advance(ch, size);
        }
        paths
    }

    /// The slide units of one line, laid out exactly like [`Shaper::outline`] (same pen
    /// positions, same glyphs): one per character with ink, a combining mark joining the
    /// character before it (a dot or accent never separates from its letter), each with its
    /// advance cell. Blanks and control characters make no unit.
    fn units(&self, text: &str, size: f32) -> Vec<GlyphUnit> {
        let mut units: Vec<GlyphUnit> = Vec::new();
        let mut cursor = 0.0;
        for ch in text.chars() {
            let advance = self.advance(ch, size);
            let ink: Vec<Vec<PathVerb>> = if ch.is_control() {
                Vec::new()
            } else {
                self.glyph(ch)
                    .contours
                    .iter()
                    .map(|contour| {
                        contour
                            .iter()
                            .map(|verb| {
                                map_verb(verb, |p| Point {
                                    x: cursor + p.x * size,
                                    y: p.y * size,
                                })
                            })
                            .collect()
                    })
                    .collect()
            };
            match units.last_mut() {
                Some(unit) if glyph_slide::is_mark(ch) => {
                    unit.contours.extend(ink);
                    unit.right = unit.right.max(cursor + advance);
                }
                _ => units.push(GlyphUnit {
                    contours: ink,
                    left: cursor,
                    right: cursor + advance,
                }),
            }
            cursor += advance;
        }
        units.retain(|unit| !unit.contours.is_empty());
        units
    }
}

/// One glyph slide unit of outlined text: its contours (separate subpaths, so counters stay
/// open) and its advance cell along x, in the line's own coordinates.
struct GlyphUnit {
    contours: Vec<Vec<PathVerb>>,
    left: f32,
    right: f32,
}

fn map_verb(verb: &PathVerb, f: impl Fn(Point) -> Point) -> PathVerb {
    match verb {
        PathVerb::MoveTo(p) => PathVerb::MoveTo(f(*p)),
        PathVerb::LineTo(p) => PathVerb::LineTo(f(*p)),
        PathVerb::QuadTo((c, p)) => PathVerb::QuadTo((f(*c), f(*p))),
        PathVerb::CubicTo((a, b, p)) => PathVerb::CubicTo((f(*a), f(*b), f(*p))),
        PathVerb::Close => PathVerb::Close,
    }
}

/// Collects contours in em units (`per_em` = 1 / units-per-em), y flipped to point down.
#[derive(Default)]
struct OutlinePen {
    paths: Vec<Vec<PathVerb>>,
    current: Vec<PathVerb>,
    per_em: f32,
}

impl OutlinePen {
    fn point(&self, x: f32, y: f32) -> Point {
        Point {
            x: x * self.per_em,
            y: -y * self.per_em,
        }
    }

    fn flush(&mut self) {
        if !self.current.is_empty() {
            self.paths.push(std::mem::take(&mut self.current));
        }
    }

    fn finish(mut self) -> Vec<Vec<PathVerb>> {
        self.flush();
        self.paths
    }
}

impl skrifa::outline::OutlinePen for OutlinePen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.flush();
        let p = self.point(x, y);
        self.current.push(PathVerb::MoveTo(p));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.point(x, y);
        self.current.push(PathVerb::LineTo(p));
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let (c, p) = (self.point(cx0, cy0), self.point(x, y));
        self.current.push(PathVerb::QuadTo((c, p)));
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let (a, b, p) = (self.point(cx0, cy0), self.point(cx1, cy1), self.point(x, y));
        self.current.push(PathVerb::CubicTo((a, b, p)));
    }

    fn close(&mut self) {
        self.current.push(PathVerb::Close);
        self.flush();
    }
}

fn is_blank(ch: char) -> bool {
    ch == ' ' || ch == '\t'
}

/// Greedy line breaking like the host's layout: breaks happen after blanks, runs of
/// blanks (including leading ones) and tabs keep their width, blanks that end a wrapped
/// line hang past the edge instead of forcing a break, an over-long word overflows
/// rather than splitting, and every `\n` (also a trailing one) starts a new line.
fn wrap_lines(shaper: &Shaper, text: &str, px: f32, max_width: f32) -> Vec<Line> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = Vec::new();
    for paragraph in normalized.split('\n') {
        let mut line = String::new();
        let mut line_width = 0.0_f32;
        let mut rest = paragraph;
        while !rest.is_empty() {
            let word_end = rest.find(is_blank).unwrap_or(rest.len());
            let (word, after) = rest.split_at(word_end);
            let blank_end = after.find(|ch| !is_blank(ch)).unwrap_or(after.len());
            let (blanks, next) = after.split_at(blank_end);
            let word_width = shaper.width(word, px);
            if !line.is_empty() && !word.is_empty() && line_width + word_width > max_width {
                lines.push(Line {
                    text: std::mem::take(&mut line),
                    soft_wrapped: true,
                });
                line_width = 0.0;
            }
            line.push_str(word);
            line.push_str(blanks);
            line_width += word_width + shaper.width(blanks, px);
            rest = next;
        }
        lines.push(Line {
            text: line,
            soft_wrapped: false,
        });
    }
    lines
}

/// One laid-out line. Blanks that end a soft-wrapped line hang past the edge and do not
/// count towards alignment; blanks that end a line at a newline or the end of the text do,
/// as in the host (so `"ab  "` centres including its trailing blanks).
struct Line {
    text: String,
    soft_wrapped: bool,
}

impl Line {
    fn aligned_width(&self, shaper: &Shaper, px: f32) -> f32 {
        if self.soft_wrapped {
            shaper.width(self.text.trim_end_matches(is_blank), px)
        } else {
            shaper.width(&self.text, px)
        }
    }
}

/// Fill commands for `contours`, split so no single path nears the core's verb cap.
fn fill_chunks(contours: Vec<Vec<PathVerb>>, color: Color) -> Vec<DrawCommand> {
    let mut commands = Vec::new();
    let mut chunk: Vec<PathVerb> = Vec::new();
    for contour in contours {
        if !chunk.is_empty() && chunk.len() + contour.len() > MAX_VERBS_PER_ELEMENT {
            commands.push(DrawCommand::FillPath((std::mem::take(&mut chunk), color)));
        }
        chunk.extend(contour);
    }
    if !chunk.is_empty() {
        commands.push(DrawCommand::FillPath((chunk, color)));
    }
    commands
}

/// One laid-out line of an outlined text block: its text, left edge and baseline in pixels.
struct PlacedLine {
    text: String,
    x: f32,
    baseline: f32,
}

/// The wrapped block of an outlined normalized text: shared by the plain weighted render
/// and its per-character slide, so the glyphs sit where the plain text puts them.
struct Block {
    shaper: std::rc::Rc<Shaper>,
    lines: Vec<PlacedLine>,
    /// Top of the first line and the distance between lines.
    top: f32,
    line_height: f32,
    /// The font size in pixels.
    px: f32,
}

/// Wrapped at the canvas width, lines aligned inside the block and the block centred in the
/// clip box, like the host's normalized `TextRun`, so switching weight does not move the text.
fn place_block(
    text: &str,
    font_size: f32,
    face: Typeface,
    align: &str,
    size: Size,
) -> Result<Block, Diagnostic> {
    let px = font_size * size.height as f32;
    check_size(px)?;
    check_text(text)?;
    let (width, height) = (size.width as f32, size.height as f32);
    let shaper = Shaper::get(face.weight)?;
    let metrics = shaper.line_metrics(px);
    let lines = wrap_lines(&shaper, text, px, width);
    let widths: Vec<f32> = lines
        .iter()
        .map(|line| line.aligned_width(&shaper, px))
        .collect();
    let block_width = widths.iter().copied().fold(0.0, f32::max);
    let top = (height - metrics.line_height * lines.len() as f32) / 2.0;
    let left = (width - block_width) / 2.0;
    let placed = lines
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            let remaining = block_width - widths[index];
            PlacedLine {
                text: line.text,
                x: left
                    + match align {
                        "left" => 0.0,
                        "right" => remaining,
                        _ => remaining / 2.0,
                    },
                baseline: top + index as f32 * metrics.line_height + metrics.ascent,
            }
        })
        .collect();
    Ok(Block {
        shaper,
        lines: placed,
        top,
        line_height: metrics.line_height,
        px,
    })
}

/// Weighted counterpart of the host's normalized `TextRun` (see [`place_block`]).
fn weighted_text_run(
    text: &str,
    font_size: f32,
    face: Typeface,
    align: &str,
    color: Color,
    size: Size,
) -> Result<Scene, Diagnostic> {
    let (width, height) = (size.width as f32, size.height as f32);
    let block = place_block(text, font_size, face, align, size)?;
    let mut contours = Vec::new();
    for line in &block.lines {
        let scale = |p: Point| Point {
            x: (p.x + line.x) / width,
            y: (p.y + line.baseline) / height,
        };
        for contour in block.shaper.outline(&line.text, block.px) {
            contours.push(contour.iter().map(|verb| map_verb(verb, scale)).collect());
        }
    }
    check_verbs(&contours)?;
    Ok(Scene {
        coordinate_space: CoordinateSpace::Normalized,
        elements: fill_chunks(contours, color),
    })
}

/// Per-glyph slide of an outlined (weighted or Inter) normalized text: the units are the
/// Inter shaper's own glyphs on the lines of [`place_block`], never the host's DejaVu layout,
/// so the typeface does not change while the text animates.
fn weighted_slide(
    text: &str,
    font_size: f32,
    face: Typeface,
    align: &str,
    color: Color,
    size: Size,
    slide: &glyph_slide::Slide,
) -> Result<Scene, Diagnostic> {
    let (width, height) = (size.width as f32, size.height as f32);
    let block = place_block(text, font_size, face, align, size)?;
    let mut layout = glyph_slide::Layout {
        lines: Vec::new(),
        glyphs: Vec::new(),
    };
    for (index, line) in block.lines.iter().enumerate() {
        let band_top = block.top + index as f32 * block.line_height;
        layout.lines.push(glyph_slide::Band {
            top: band_top,
            bottom: band_top + block.line_height,
        });
        for unit in block.shaper.units(&line.text, block.px) {
            layout.glyphs.push(glyph_slide::Glyph {
                contours: unit
                    .contours
                    .iter()
                    .map(|c| c.iter().map(|v| offset_verb(v, line.x, line.baseline)).collect())
                    .collect(),
                line: index,
                left: unit.left + line.x,
                right: unit.right + line.x,
            });
        }
    }
    check_verbs(
        &layout
            .glyphs
            .iter()
            .flat_map(|glyph| glyph.contours.iter().cloned())
            .collect::<Vec<_>>(),
    )?;
    let elements = glyph_slide::draw(&layout, slide, color, |p| Point {
        x: p.x / width,
        y: p.y / height,
    });
    Ok(Scene {
        coordinate_space: CoordinateSpace::Normalized,
        elements,
    })
}

#[cfg(test)]
mod weight_tests {
    use super::*;

    fn outline(weight: f32, text: &str) -> Vec<Vec<PathVerb>> {
        Shaper::get(weight).unwrap().outline(text, 48.0)
    }

    fn width(contours: &[Vec<PathVerb>]) -> f32 {
        let xs = contours.iter().flatten().map(verb_x);
        xs.clone().fold(f32::MIN, f32::max) - xs.fold(f32::MAX, f32::min)
    }

    /// The word-highlight pill wraps the active word's own glyphs. It must leave part
    /// of the inter-word gap visible on both sides, so a caption never reads as
    /// `third[harmonic]at` instead of `third [harmonic] at`.
    #[test]
    fn the_word_highlight_pill_leaves_the_inter_word_spaces_visible() {
        let accent = Color {
            r: 255,
            g: 214,
            b: 10,
            a: 255,
        };
        let ms = 705_600i64;
        let timings = (0..4)
            .map(|i| format!("{}:{}", i * 500 * ms, (i + 1) * 500 * ms))
            .collect::<Vec<_>>()
            .join(" ");
        let params = vec![
            ParamEntry {
                id: "caption_style".into(),
                value: ParamValue::String("word-highlight".into()),
            },
            ParamEntry {
                id: "words".into(),
                value: ParamValue::String(timings),
            },
            ParamEntry {
                id: "accent".into(),
                value: ParamValue::Color(accent),
            },
        ];
        let size = Size {
            width: 960,
            height: 540,
        };
        let (box_w, box_h) = (0.9 * size.width as f32, 0.9 * size.height as f32);
        let scene = caption_scene(
            CaptionStyle::WordHighlight,
            &params,
            "Math is beautiful today",
            0.09,
            // An outlined face so shaping runs natively (the regular face shapes
            // through the host, which only exists inside the component).
            Typeface {
                weight: 700.0,
                inter: true,
            },
            0.9,
            0.9,
            Color {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
            700 * ms, // the second word, "is", is active
            size,
        )
        .unwrap();
        // Normalized ink bounding box of one fill: (x0, x1, y0, y1).
        let span = |verbs: &[PathVerb]| -> (f32, f32, f32, f32) {
            let mut acc = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
            for verb in verbs {
                let pts: Vec<Point> = match verb {
                    PathVerb::MoveTo(p) | PathVerb::LineTo(p) => vec![*p],
                    PathVerb::QuadTo((c, p)) => vec![*c, *p],
                    PathVerb::CubicTo((a, b, p)) => vec![*a, *b, *p],
                    PathVerb::Close => continue,
                };
                for p in pts {
                    acc.0 = acc.0.min(p.x);
                    acc.1 = acc.1.max(p.x);
                    acc.2 = acc.2.min(p.y);
                    acc.3 = acc.3.max(p.y);
                }
            }
            acc
        };
        let fills: Vec<_> = scene.elements.iter().filter_map(|element| match element {
            DrawCommand::FillPath((verbs, color)) => Some((span(verbs), *color)),
            _ => None,
        }).collect();
        // The pill is the only accent-coloured fill: the active word's glyphs are
        // drawn in the contrast colour on top of it, the other words in `color`.
        let same = |a: &Color, b: &Color| (a.r, a.g, a.b, a.a) == (b.r, b.g, b.b, b.a);
        let pill_index = fills
            .iter()
            .position(|(_, color)| same(color, &accent))
            .expect("the pill fill");
        let pill = fills[pill_index].0;
        let mut before = (f32::MAX, f32::MIN);
        let mut after = (f32::MAX, f32::MIN);
        for (index, ((x0, x1, _, _), _)) in fills.iter().enumerate() {
            if index == pill_index {
                continue;
            }
            if *x1 <= pill.0 {
                before = (before.0.min(*x0), before.1.max(*x1));
            } else if *x0 >= pill.1 {
                after = (after.0.min(*x0), after.1.max(*x1));
            }
        }
        assert!(
            before.1 > f32::MIN && after.0 < f32::MAX,
            "a word must sit on each side of the highlighted one"
        );
        // The pill is exactly one em tall (half an em of padding above and below
        // the line), and the caption's inter-word gap is 0.28 em at the same scale.
        let em = (pill.3 - pill.2) * box_h;
        let gap = 0.28 * em;
        let left = (pill.0 - before.1) * box_w;
        let right = (after.0 - pill.1) * box_w;
        // Most of each space must stay visible around the pill (the old 0.22 em
        // padding ate all but 0.06 em of the 0.28 em gap, so the caption read
        // `Math[is]beautiful` with the spaces swallowed).
        assert!(
            left >= 0.4 * gap,
            "left space swallowed: {left:.3} px, gap {gap:.3} px"
        );
        assert!(
            right >= 0.4 * gap,
            "right space swallowed: {right:.3} px, gap {gap:.3} px"
        );
    }

    #[test]
    fn weight_schema_keeps_regular_as_default() {
        let manifest = TextPlugin::describe();
        assert_eq!(manifest.version, "1.4.1");
        let param = manifest
            .params
            .iter()
            .find(|param| param.id == "font_weight")
            .unwrap();
        let ParamKind::Number(range) = &param.kind else {
            panic!("numeric weight required")
        };
        assert_eq!(
            (range.min, range.max, range.default, range.step),
            (100.0, 900.0, 400.0, 100.0)
        );
    }

    #[test]
    fn font_is_an_enum_that_defaults_to_dejavu() {
        let manifest = TextPlugin::describe();
        let param = manifest.params.iter().find(|p| p.id == "font").unwrap();
        let ParamKind::EnumChoice(choice) = &param.kind else {
            panic!("enum font required")
        };
        assert_eq!(choice.options, ["dejavu", "inter"]);
        assert_eq!(choice.default, "dejavu");
        let params = |font: Option<&str>, weight: f64| {
            let mut entries = vec![ParamEntry {
                id: "font_weight".into(),
                value: ParamValue::Number(weight),
            }];
            if let Some(font) = font {
                entries.push(ParamEntry {
                    id: "font".into(),
                    value: ParamValue::String(font.into()),
                });
            }
            Typeface::from_params(&entries)
        };
        // Absent `font` is DejaVu: regular is host text, anything else is outlined Inter.
        assert!(!params(None, 400.0).outlined());
        assert!(params(None, 700.0).outlined());
        // Inter outlines regular too, so weight never decides the typeface.
        assert!(params(Some("inter"), 400.0).outlined());
        assert!(!params(Some("dejavu"), 400.0).outlined());
    }

    #[test]
    fn slide_units_are_the_outline_glyph_for_glyph() {
        let shaper = Shaper::get(700.0).unwrap();
        for text in ["Hamburgefonts", "in", "a b", "∀x ∈ ℝ", "e\u{301}x"] {
            let units = shaper.units(text, 40.0);
            let whole: Vec<Vec<PathVerb>> = units
                .iter()
                .map(|unit| unit.contours.iter().flatten().cloned().collect())
                .collect();
            // The units regroup the very same verbs as the plain outline (marks merged).
            let count = |paths: &[Vec<PathVerb>]| paths.iter().map(Vec::len).sum::<usize>();
            assert_eq!(count(&whole), count(&shaper.outline(text, 40.0)), "{text:?}");
        }
    }

    #[test]
    fn slide_units_merge_marks_and_skip_blanks() {
        let shaper = Shaper::get(700.0).unwrap();
        // The i carries its dot as a second contour; the space makes no unit.
        let units = shaper.units("i n", 40.0);
        assert_eq!(units.len(), 2);
        assert_eq!(units[0].contours.len(), 2);
        // A combining accent joins the letter before it.
        assert_eq!(shaper.units("e\u{301}", 40.0).len(), 1);
        assert_eq!(shaper.units("ae\u{301}", 40.0).len(), 2);
        // Cells follow the pen: left to right, each as wide as its advance.
        let units = shaper.units("Hi", 40.0);
        assert_eq!(units[0].left, 0.0);
        assert!((units[0].right - shaper.advance('H', 40.0)).abs() < 1e-3);
        assert_eq!(units[1].left, units[0].right);
    }

    #[test]
    fn variable_font_has_distinct_semibold_bold_and_heavy_outlines() {
        let outlines: Vec<_> = [300.0, 600.0, 700.0, 800.0]
            .into_iter()
            .map(|weight| format!("{:?}", outline(weight, "Heading")))
            .collect();
        assert!(outlines.iter().all(|outline| outline.len() > 100));
        for pair in outlines.windows(2) {
            assert_ne!(pair[0], pair[1]);
        }
        // Heavier faces are wider, so text never overlaps its neighbours.
        let widths: Vec<_> = [300.0, 600.0, 800.0]
            .into_iter()
            .map(|weight| width(&outline(weight, "Heading")))
            .collect();
        assert!(
            widths.windows(2).all(|pair| pair[0] < pair[1]),
            "{widths:?}"
        );
    }

    #[test]
    fn outlines_sit_on_the_baseline_and_ignore_control_characters() {
        let a = outline(700.0, "Hi");
        let b = outline(700.0, "H\ni\t");
        assert_eq!(format!("{a:?}"), format!("{b:?}"));
        let ys = a.iter().flatten().filter_map(|verb| match verb {
            PathVerb::MoveTo(p) | PathVerb::LineTo(p) => Some(p.y),
            _ => None,
        });
        // y grows downwards: ink is above the baseline (negative), never far below it.
        assert!(ys.clone().fold(f32::MAX, f32::min) < -20.0);
        assert!(ys.fold(f32::MIN, f32::max) < 1.0);
    }

    #[test]
    fn characters_missing_from_inter_fall_back_to_dejavu_instead_of_notdef() {
        let shaper = Shaper::get(700.0).unwrap();
        for ch in "∀∃∈∉⊂⊆∪∩∇∮ℝℕℤℚא".chars() {
            assert!(shaper.uses_fallback(ch), "{ch} should come from DejaVu");
            let glyph = shaper.glyph(ch);
            assert!(!glyph.contours.is_empty(), "{ch} has an outline");
            let notdef = shaper.glyph('\u{10FFFE}');
            assert_ne!(
                format!("{:?}", glyph.contours),
                format!("{:?}", notdef.contours),
                "{ch} must not be the .notdef box"
            );
        }
        // Latin stays Inter.
        assert!(!shaper.uses_fallback('x'));
        assert!(!outline(700.0, "∀x ∈ ℝ").is_empty());
    }

    #[test]
    fn wrapping_keeps_blanks_and_trailing_newlines_like_the_host() {
        let shaper = Shaper::get(400.0).unwrap();
        let lines = |text: &str, width: f32| -> Vec<String> {
            wrap_lines(&shaper, text, 20.0, width)
                .into_iter()
                .map(|line| line.text)
                .collect()
        };
        // Repeated and leading blanks are preserved.
        assert_eq!(lines("a  b", 1e6), ["a  b"]);
        assert_eq!(lines("  lead", 1e6), ["  lead"]);
        // A tab is whitespace with width, not dropped.
        assert_eq!(lines("a\tb", 1e6), ["a\tb"]);
        assert!(shaper.width("a\tb", 20.0) > shaper.width("ab", 20.0));
        // Every newline starts a line, including a trailing one; CRLF is one break.
        assert_eq!(lines("a\n", 1e6), ["a", ""]);
        assert_eq!(lines("a\r\nb", 1e6), ["a", "b"]);
        // Blanks at a wrap point hang on the earlier line; an over-long word overflows.
        let narrow = shaper.width("aa", 20.0) * 1.5;
        assert_eq!(lines("aa  aa", narrow), ["aa  ", "aa"]);
        assert_eq!(lines("aaaaaaaaaa", narrow), ["aaaaaaaaaa"]);
        let wrapped = wrap_lines(&shaper, "aa  aa  ", 20.0, 1e6);
        assert!(!wrapped[0].soft_wrapped);
        let wrapped = wrap_lines(&shaper, "aa  aa  ", 20.0, narrow);
        assert!(wrapped[0].soft_wrapped && !wrapped[1].soft_wrapped);
        // Hanging blanks stay out of the alignment width; a final line keeps them.
        assert_eq!(
            wrapped[0].aligned_width(&shaper, 20.0),
            shaper.width("aa", 20.0)
        );
        assert_eq!(
            wrapped[1].aligned_width(&shaper, 20.0),
            shaper.width("aa  ", 20.0)
        );
        assert_eq!(shaper.advance('\t', 20.0), 12.0);
    }

    #[test]
    fn invalid_sizes_and_long_text_are_rejected_like_the_host() {
        let face = Typeface {
            weight: 700.0,
            inter: false,
        };
        for size in [0.0, -4.0, f32::NAN, f32::INFINITY, 2.0e6] {
            assert!(shape_text("x", size, face).is_err(), "size {size}");
        }
        assert!(shape_text(&".".repeat(MAX_TEXT_CHARS), 12.0, face).is_ok());
        assert!(shape_text(&".".repeat(MAX_TEXT_CHARS + 1), 12.0, face).is_err());
    }

    #[test]
    fn a_long_paragraph_is_split_into_paths_below_the_core_verb_cap() {
        let text = "The quick brown fox jumps over the lazy dog. ".repeat(46);
        assert!(text.chars().count() > 2000);
        let face = Typeface {
            weight: 700.0,
            inter: true,
        };
        let scene = weighted_text_run(
            &text,
            0.03,
            face,
            "left",
            Color {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
            Size {
                width: 1920,
                height: 1080,
            },
        )
        .unwrap();
        assert!(scene.elements.len() > 1, "split across several paths");
        for element in &scene.elements {
            let DrawCommand::FillPath((verbs, _)) = element else {
                panic!("paths expected")
            };
            assert!(
                verbs.len() <= MAX_VERBS_PER_ELEMENT + 200,
                "{}",
                verbs.len()
            );
        }
    }

    /// Host text arrives as one contour per outline. Above the core's path cap the
    /// contours are regrouped per glyph, so a counter always shares a path with its
    /// outer contour (otherwise the non-zero fill paints the counter solid).
    #[test]
    fn long_host_text_keeps_counters_with_their_glyph() {
        let pt = |x: f32, y: f32| Point { x, y };
        let ring = |x: f32, size: f32, y: f32| {
            let mut v = vec![PathVerb::MoveTo(pt(x, y))];
            for i in 1..=29 {
                v.push(PathVerb::LineTo(pt(x + size * (i % 2) as f32, y + size)));
            }
            v.push(PathVerb::Close);
            v
        };
        // 2,000 "o": outer contour then counter, 31 verbs each (124k verbs total).
        let mut contours = Vec::new();
        for i in 0..2_000 {
            let x = i as f32 * 10.0;
            contours.push(ring(x, 8.0, 0.0));
            contours.push(ring(x + 2.0, 4.0, 2.0));
        }
        let color = Color { r: 0, g: 0, b: 0, a: 255 };
        let elements = fill_contours(contours.clone(), color);
        assert!(elements.len() > 1, "above the cap the fill is chunked");
        let mut glyph_contours = 0;
        for element in &elements {
            let DrawCommand::FillPath((verbs, _)) = element else {
                panic!("paths expected")
            };
            assert!(verbs.len() <= MAX_VERBS_PER_ELEMENT + 200);
            let closes = verbs.iter().filter(|v| matches!(v, PathVerb::Close)).count();
            assert_eq!(closes % 2, 0, "outer contour and counter split apart");
            glyph_contours += closes;
        }
        assert_eq!(glyph_contours, 4_000);

        // Within the cap: one path, exactly the pre-weight behaviour.
        let short = fill_contours(contours[..400].to_vec(), color);
        assert_eq!(short.len(), 1);
    }
}
