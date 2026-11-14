//! First-party `text` plugin: host-side text shaping with size, colour and alignment.

wit_bindgen::generate!({
    path: "../../crates/plugin-api/wit",
    world: "source-plugin",
});

use animathio::plugin::host;
use animathio::plugin::types::*;

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

// Fitted titles use a real clip-local box, so the editor's transform handles and
// safe-area coordinates describe the visible composition rather than an empty
// full-canvas text source. Ordinary normalized text keeps the wrapped TextRun path.
fn fitted_text(
    text: &str,
    font_size: f32,
    width: f32,
    height: f32,
    color: Color,
    size: Size,
) -> Result<Scene, Diagnostic> {
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
    for (line, text) in lines.iter().enumerate() {
        let contours = host::shape_text(text, font)?;
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
        for contour in contours {
            let path = contour
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
                .collect();
            elements.push(DrawCommand::FillPath((path, color)));
        }
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

fn shape_word(text: &str, font: f32) -> Result<ShapedWord, Diagnostic> {
    let contours = host::shape_text(text, font)?;
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
            self.elements.push(DrawCommand::FillPath((path, color)));
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
        all[index] = shape_word(&words[index], base_font)?;
        all
    } else {
        words
            .iter()
            .map(|word| shape_word(word, base_font))
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
                let pad_x = base_font * 0.22 * k;
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
            version: "1.1.0".to_string(),
            description:
                "Host-shaped text with size, colour, alignment and animated caption styles"
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
                    id: "font_size".to_string(),
                    label: "Font Size".to_string(),
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
        let text = get_string(&params, "text", "AniMathIO");
        let font_size = get_number(&params, "font_size", 48.0) as f32;
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

        if get_string(&params, "units", "pixels") == "normalized" {
            let fit_width = get_number(&params, "fit_width", 0.0) as f32;
            let fit_height = get_number(&params, "fit_height", 0.0) as f32;
            if fit_width > 0.0 && fit_height > 0.0 {
                let style = CaptionStyle::parse(&get_string(&params, "caption_style", "off"));
                if let Some(style) = style.filter(|s| *s != CaptionStyle::Classic) {
                    return caption_scene(
                        style, &params, &text, font_size, fit_width, fit_height, color, time_ticks,
                        size,
                    );
                }
                return fitted_text(&text, font_size, fit_width, fit_height, color, size);
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

        let contours = host::shape_text(&text, font_size)?;

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

        let mut elements = Vec::with_capacity(contours.len());
        for contour in contours {
            let offset: Vec<PathVerb> = contour
                .iter()
                .map(|v| offset_verb(v, x_offset, y_offset))
                .collect();
            elements.push(DrawCommand::FillPath((offset, color)));
        }

        Ok(Scene {
            coordinate_space: CoordinateSpace::Canvas,
            elements,
        })
    }
}

export!(TextPlugin);
