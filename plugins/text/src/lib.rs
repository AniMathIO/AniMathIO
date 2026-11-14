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

impl Guest for TextPlugin {
    fn describe() -> Manifest {
        Manifest {
            name: "text".to_string(),
            version: "1.1.0".to_string(),
            description: "Host-shaped text with size, colour and alignment".to_string(),
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
        _time_ticks: i64,
        size: Size,
    ) -> Result<Scene, Diagnostic> {
        let text = revealed(
            &get_string(&params, "text", "AniMathIO"),
            get_number(&params, "reveal", 1.0),
        );
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
