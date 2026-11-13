//! First-party `shapes` plugin: rect, ellipse, polygon, star, arrow and line.

wit_bindgen::generate!({
    path: "../../crates/plugin-api/wit",
    world: "plugin",
});

use animathio::plugin::types::*;

struct ShapesPlugin;

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

fn rect(x: f32, y: f32, w: f32, h: f32) -> Vec<PathVerb> {
    vec![
        PathVerb::MoveTo(Point { x, y }),
        PathVerb::LineTo(Point { x: x + w, y }),
        PathVerb::LineTo(Point { x: x + w, y: y + h }),
        PathVerb::LineTo(Point { x, y: y + h }),
        PathVerb::Close,
    ]
}

fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> Vec<PathVerb> {
    let kx = 0.552_284_8 * rx;
    let ky = 0.552_284_8 * ry;
    vec![
        PathVerb::MoveTo(Point { x: cx + rx, y: cy }),
        PathVerb::CubicTo((
            Point {
                x: cx + rx,
                y: cy + ky,
            },
            Point {
                x: cx + kx,
                y: cy + ry,
            },
            Point { x: cx, y: cy + ry },
        )),
        PathVerb::CubicTo((
            Point {
                x: cx - kx,
                y: cy + ry,
            },
            Point {
                x: cx - rx,
                y: cy + ky,
            },
            Point { x: cx - rx, y: cy },
        )),
        PathVerb::CubicTo((
            Point {
                x: cx - rx,
                y: cy - ky,
            },
            Point {
                x: cx - kx,
                y: cy - ry,
            },
            Point { x: cx, y: cy - ry },
        )),
        PathVerb::CubicTo((
            Point {
                x: cx + kx,
                y: cy - ry,
            },
            Point {
                x: cx + rx,
                y: cy - ky,
            },
            Point { x: cx + rx, y: cy },
        )),
        PathVerb::Close,
    ]
}

fn polygon(cx: f32, cy: f32, r: f32, sides: usize, rotation: f32) -> Vec<PathVerb> {
    let n = sides.max(3);
    let step = std::f32::consts::TAU / n as f32;
    let mut verbs = Vec::with_capacity(n + 1);
    for i in 0..n {
        let theta = rotation + i as f32 * step;
        let p = Point {
            x: cx + r * theta.cos(),
            y: cy + r * theta.sin(),
        };
        if i == 0 {
            verbs.push(PathVerb::MoveTo(p));
        } else {
            verbs.push(PathVerb::LineTo(p));
        }
    }
    verbs.push(PathVerb::Close);
    verbs
}

fn star(
    cx: f32,
    cy: f32,
    r_outer: f32,
    r_inner: f32,
    points: usize,
    rotation: f32,
) -> Vec<PathVerb> {
    let n = points.max(3);
    let total = n * 2;
    let step = std::f32::consts::TAU / total as f32;
    let mut verbs = Vec::with_capacity(total + 1);
    for i in 0..total {
        let r = if i % 2 == 0 { r_outer } else { r_inner };
        let theta = rotation + i as f32 * step;
        let p = Point {
            x: cx + r * theta.cos(),
            y: cy + r * theta.sin(),
        };
        if i == 0 {
            verbs.push(PathVerb::MoveTo(p));
        } else {
            verbs.push(PathVerb::LineTo(p));
        }
    }
    verbs.push(PathVerb::Close);
    verbs
}

fn line(x1: f32, y1: f32, x2: f32, y2: f32) -> Vec<PathVerb> {
    vec![
        PathVerb::MoveTo(Point { x: x1, y: y1 }),
        PathVerb::LineTo(Point { x: x2, y: y2 }),
    ]
}

fn arrow(x1: f32, y1: f32, x2: f32, y2: f32, head_len: f32, head_width: f32) -> Vec<PathVerb> {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let angle = dy.atan2(dx);
    let half_w = head_width / 2.0;
    let tip = Point { x: x2, y: y2 };
    let left = Point {
        x: x2 - head_len * angle.cos() + half_w * angle.sin(),
        y: y2 - head_len * angle.sin() - half_w * angle.cos(),
    };
    let right = Point {
        x: x2 - head_len * angle.cos() - half_w * angle.sin(),
        y: y2 - head_len * angle.sin() + half_w * angle.cos(),
    };
    vec![
        PathVerb::MoveTo(Point { x: x1, y: y1 }),
        PathVerb::LineTo(tip),
        PathVerb::MoveTo(left),
        PathVerb::LineTo(tip),
        PathVerb::LineTo(right),
    ]
}

impl Guest for ShapesPlugin {
    fn describe() -> Manifest {
        Manifest {
            name: "shapes".to_string(),
            version: "0.1.0".to_string(),
            description: "Basic vector shapes: rect, ellipse, polygon, star, arrow, line"
                .to_string(),
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
                    id: "path".into(),
                    label: "Path (JSON verbs)".into(),
                    kind: ParamKind::String(StringParam {
                        default: "[]".into(),
                    }),
                },
                ParamSpec {
                    id: "shape".to_string(),
                    label: "Shape".to_string(),
                    kind: ParamKind::EnumChoice(EnumParam {
                        options: vec![
                            "rect".to_string(),
                            "ellipse".to_string(),
                            "polygon".to_string(),
                            "star".to_string(),
                            "arrow".to_string(),
                            "line".to_string(),
                            "path".to_string(),
                        ],
                        default: "rect".to_string(),
                    }),
                },
                ParamSpec {
                    id: "x".to_string(),
                    label: "Center X".to_string(),
                    kind: ParamKind::Number(NumberRange {
                        min: 0.0,
                        max: 4096.0,
                        default: 640.0,
                        step: 0.001,
                    }),
                },
                ParamSpec {
                    id: "y".to_string(),
                    label: "Center Y".to_string(),
                    kind: ParamKind::Number(NumberRange {
                        min: 0.0,
                        max: 4096.0,
                        default: 360.0,
                        step: 0.001,
                    }),
                },
                ParamSpec {
                    id: "width".to_string(),
                    label: "Width".to_string(),
                    kind: ParamKind::Number(NumberRange {
                        min: 0.0001,
                        max: 4096.0,
                        default: 200.0,
                        step: 0.001,
                    }),
                },
                ParamSpec {
                    id: "height".to_string(),
                    label: "Height".to_string(),
                    kind: ParamKind::Number(NumberRange {
                        min: 0.0001,
                        max: 4096.0,
                        default: 120.0,
                        step: 0.001,
                    }),
                },
                ParamSpec {
                    id: "radius".to_string(),
                    label: "Radius".to_string(),
                    kind: ParamKind::Number(NumberRange {
                        min: 0.0001,
                        max: 4096.0,
                        default: 100.0,
                        step: 0.001,
                    }),
                },
                ParamSpec {
                    id: "points".to_string(),
                    label: "Points / Sides".to_string(),
                    kind: ParamKind::Number(NumberRange {
                        min: 3.0,
                        max: 64.0,
                        default: 5.0,
                        step: 0.001,
                    }),
                },
                ParamSpec {
                    id: "rotation".to_string(),
                    label: "Rotation".to_string(),
                    kind: ParamKind::Number(NumberRange {
                        min: -360.0,
                        max: 360.0,
                        default: 0.0,
                        step: 0.001,
                    }),
                },
                ParamSpec {
                    id: "fill".to_string(),
                    label: "Fill Color".to_string(),
                    kind: ParamKind::Color(ColorParam {
                        default: Color {
                            r: 90,
                            g: 170,
                            b: 255,
                            a: 255,
                        },
                    }),
                },
                ParamSpec {
                    id: "stroke".to_string(),
                    label: "Stroke Color".to_string(),
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
                    id: "stroke_width".to_string(),
                    label: "Stroke Width".to_string(),
                    kind: ParamKind::Number(NumberRange {
                        min: 0.0,
                        max: 64.0,
                        default: 2.0,
                        step: 0.001,
                    }),
                },
            ],
        }
    }

    fn render_clip(params: Vec<ParamEntry>, _time_ticks: i64, size: Size) -> Scene {
        let _ = size;
        let normalized = get_string(&params, "units", "pixels") == "normalized";

        let shape = get_string(&params, "shape", "rect");
        let x = get_number(&params, "x", if normalized { 0.5 } else { 640.0 }) as f32;
        let y = get_number(&params, "y", if normalized { 0.5 } else { 360.0 }) as f32;
        let width = get_number(&params, "width", if normalized { 1.0 } else { 200.0 }) as f32;
        let height = get_number(&params, "height", if normalized { 1.0 } else { 120.0 }) as f32;
        let radius = get_number(&params, "radius", 100.0) as f32;
        let points = get_number(&params, "points", 5.0).clamp(3.0, 64.0) as usize;
        let rotation = get_number(&params, "rotation", 0.0) as f32
            + if normalized {
                0.0
            } else {
                (_time_ticks as f64 / 705_600_000.0) as f32 * 0.5
            };
        let fill = get_color(
            &params,
            "fill",
            Color {
                r: 90,
                g: 170,
                b: 255,
                a: 255,
            },
        );
        let stroke = get_color(
            &params,
            "stroke",
            Color {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
        );
        let stroke_width = get_number(&params, "stroke_width", 2.0) as f32;

        let path = match shape.as_str() {
            "path" => parse_path(&get_string(&params, "path", "[]")),
            "ellipse" if normalized => normalized_ellipse(x, y, width, height),
            "ellipse" => ellipse(x, y, width / 2.0, height / 2.0),
            "polygon" => polygon(x, y, radius, points, rotation),
            "star" => star(x, y, radius, radius * 0.45, points, rotation),
            "arrow" => arrow(x - width / 2.0, y, x + width / 2.0, y, height, height * 0.5),
            "line" => line(x - width / 2.0, y, x + width / 2.0, y),
            _ => rect(x - width / 2.0, y - height / 2.0, width, height),
        };

        let style = StrokeStyle {
            width: stroke_width,
            color: stroke,
            // Match the former renderer's kurbo::Stroke::new defaults.
            cap: StrokeCap::Round,
            join: StrokeJoin::Round,
        };

        let mut elements = Vec::new();
        if fill.a != 0 {
            elements.push(DrawCommand::FillPath((path.clone(), fill)));
        }
        if stroke.a != 0 && stroke_width > 0.0 {
            elements.push(DrawCommand::StrokePath((path, style)));
        }
        Scene {
            coordinate_space: if normalized {
                CoordinateSpace::Normalized
            } else {
                CoordinateSpace::Canvas
            },
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

export!(ShapesPlugin);

fn normalized_ellipse(x: f32, y: f32, width: f32, height: f32) -> Vec<PathVerb> {
    use kurbo::Shape;
    kurbo::Ellipse::new(
        (x as f64, y as f64),
        (width as f64 / 2.0, height as f64 / 2.0),
        0.0,
    )
    .to_path(0.001)
    .elements()
    .iter()
    .map(|e| {
        let p = |v: kurbo::Point| Point {
            x: v.x as f32,
            y: v.y as f32,
        };
        match *e {
            kurbo::PathEl::MoveTo(a) => PathVerb::MoveTo(p(a)),
            kurbo::PathEl::LineTo(a) => PathVerb::LineTo(p(a)),
            kurbo::PathEl::QuadTo(a, b) => PathVerb::QuadTo((p(a), p(b))),
            kurbo::PathEl::CurveTo(a, b, c) => PathVerb::CubicTo((p(a), p(b), p(c))),
            kurbo::PathEl::ClosePath => PathVerb::Close,
        }
    })
    .collect()
}

#[derive(serde::Deserialize)]
enum PathElement {
    MoveTo([f32; 2]),
    LineTo([f32; 2]),
    QuadTo([f32; 2], [f32; 2]),
    CurveTo([f32; 2], [f32; 2], [f32; 2]),
    Close,
}

fn parse_path(json: &str) -> Vec<PathVerb> {
    let Ok(elements) = serde_json::from_str::<Vec<PathElement>>(json) else {
        return Vec::new();
    };
    if elements.len() > 100_000 {
        return Vec::new();
    }
    let p = |v: [f32; 2]| Point { x: v[0], y: v[1] };
    elements
        .into_iter()
        .map(|e| match e {
            PathElement::MoveTo(a) => PathVerb::MoveTo(p(a)),
            PathElement::LineTo(a) => PathVerb::LineTo(p(a)),
            PathElement::QuadTo(a, b) => PathVerb::QuadTo((p(a), p(b))),
            PathElement::CurveTo(a, b, c) => PathVerb::CubicTo((p(a), p(b), p(c))),
            PathElement::Close => PathVerb::Close,
        })
        .collect()
}
