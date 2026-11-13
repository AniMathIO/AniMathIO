//! First-party `latex` plugin: host-typeset math with a Write-like reveal.

wit_bindgen::generate!({
    path: "../../crates/plugin-api/wit",
    world: "plugin",
});

use animathio::plugin::host;
use animathio::plugin::types::*;

struct LatexPlugin;

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

fn offset_verb(v: &PathVerb, ox: f32, oy: f32, scale: f32) -> PathVerb {
    match v {
        PathVerb::MoveTo(p) => PathVerb::MoveTo(Point {
            x: p.x * scale + ox,
            y: p.y * scale + oy,
        }),
        PathVerb::LineTo(p) => PathVerb::LineTo(Point {
            x: p.x * scale + ox,
            y: p.y * scale + oy,
        }),
        PathVerb::QuadTo((c, to)) => PathVerb::QuadTo((
            Point {
                x: c.x * scale + ox,
                y: c.y * scale + oy,
            },
            Point {
                x: to.x * scale + ox,
                y: to.y * scale + oy,
            },
        )),
        PathVerb::CubicTo((c1, c2, to)) => PathVerb::CubicTo((
            Point {
                x: c1.x * scale + ox,
                y: c1.y * scale + oy,
            },
            Point {
                x: c2.x * scale + ox,
                y: c2.y * scale + oy,
            },
            Point {
                x: to.x * scale + ox,
                y: to.y * scale + oy,
            },
        )),
        PathVerb::Close => PathVerb::Close,
    }
}

impl Guest for LatexPlugin {
    fn describe() -> Manifest {
        Manifest {
            name: "latex".to_string(),
            version: "0.1.0".to_string(),
            description: "LaTeX/Typst math typeset by the host into vector outlines".to_string(),
            kinds: vec![PluginKind::ClipSource],
            params: vec![
                ParamSpec {
                    id: "units".into(),
                    label: "Units".into(),
                    kind: ParamKind::EnumChoice(EnumParam {
                        options: vec!["pixels".into(), "normalized".into()],
                        default: "pixels".into(),
                    }),
                },
                ParamSpec {
                    id: "source".to_string(),
                    label: "Source".to_string(),
                    kind: ParamKind::String(StringParam {
                        default: r"\frac{a}{b}".to_string(),
                    }),
                },
                ParamSpec {
                    id: "mode".to_string(),
                    label: "Mode".to_string(),
                    kind: ParamKind::EnumChoice(EnumParam {
                        options: vec!["latex".to_string(), "typst".to_string()],
                        default: "latex".to_string(),
                    }),
                },
                ParamSpec {
                    id: "size".to_string(),
                    label: "Font Size".to_string(),
                    kind: ParamKind::Number(NumberRange {
                        min: 0.0001,
                        max: 400.0,
                        default: 48.0,
                        step: 1.0,
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
            ],
        }
    }

    fn render_clip(params: Vec<ParamEntry>, _time_ticks: i64, size: Size) -> Scene {
        let source = get_string(&params, "source", r"\frac{a}{b}");
        let mode = get_string(&params, "mode", "latex");
        let mut font_size = get_number(&params, "size", 48.0) as f32;
        if get_string(&params, "units", "pixels") == "normalized" {
            font_size *= size.width.min(size.height) as f32;
        }
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
        let reveal = get_number(&params, "reveal", 1.0).clamp(0.0, 1.0) as f32;

        let math_mode = match mode.as_str() {
            "typst" => MathMode::Typst,
            _ => MathMode::Latex,
        };
        let result = host::typeset_math(&source, math_mode, font_size);
        if result.paths.is_empty() {
            return Scene {
                coordinate_space: CoordinateSpace::Canvas,
                elements: Vec::new(),
            };
        }

        let raw_width = result.bbox.max_x - result.bbox.min_x;
        let raw_height = result.bbox.max_y - result.bbox.min_y;
        let scale = if get_string(&params, "units", "pixels") == "normalized" {
            (size.width as f32 * 0.84 / raw_width.max(1.0))
                .min(size.height as f32 * 0.22 / raw_height.max(1.0))
                .min(1.0)
        } else {
            1.0
        };
        let content_width = raw_width * scale;
        let content_height = raw_height * scale;
        let x_offset = match align.as_str() {
            "right" => (size.width as f32 - content_width) - result.bbox.min_x * scale,
            "center" => (size.width as f32 - content_width) / 2.0 - result.bbox.min_x * scale,
            _ => -result.bbox.min_x * scale,
        };
        let y_offset = (size.height as f32 - content_height) / 2.0 - result.bbox.min_y * scale;

        let path_count = result.paths.len();
        let full_count = ((reveal * path_count as f32).floor() as usize).min(path_count);
        let partial = reveal > 0.0 && reveal < 1.0 && full_count < path_count;

        let mut elements = Vec::with_capacity(path_count + usize::from(partial));
        for (index, path) in result.paths.iter().enumerate() {
            if index >= full_count && !(partial && index == full_count) {
                continue;
            }
            let offset: Vec<PathVerb> = path
                .iter()
                .map(|verb| offset_verb(verb, x_offset, y_offset, scale))
                .collect();
            if partial && index == full_count {
                elements.push(DrawCommand::StrokePath((
                    offset,
                    StrokeStyle {
                        width: (font_size * 0.03).max(0.5),
                        color,
                        cap: StrokeCap::Round,
                        join: StrokeJoin::Round,
                    },
                )));
            } else {
                elements.push(DrawCommand::FillPath((offset, color)));
            }
        }

        Scene {
            coordinate_space: CoordinateSpace::Canvas,
            elements,
        }
    }

    fn apply_effect(input: Scene, _params: Vec<ParamEntry>, _t: i64, _size: Size) -> Scene {
        input
    }

    fn evaluate_curve(_t: i64, _duration: i64, _params: Vec<ParamEntry>) -> f64 {
        0.0
    }
}

export!(LatexPlugin);
