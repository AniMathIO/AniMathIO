//! Minimal `hello` example plugin: a filled circle with a keyframeable radius
//! and colour, plus a host log line. Use it as a starting point for new plugins.

wit_bindgen::generate!({
    path: "../../../crates/plugin-api/wit",
    world: "plugin",
});

use animathio::plugin::host;
use animathio::plugin::types::*;

struct HelloPlugin;

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

/// A circle as four cubic Béziers (the standard kappa approximation).
fn circle(cx: f32, cy: f32, r: f32) -> Vec<PathVerb> {
    let k = 0.552_284_8 * r;
    vec![
        PathVerb::MoveTo(Point { x: cx + r, y: cy }),
        PathVerb::CubicTo((
            Point {
                x: cx + r,
                y: cy + k,
            },
            Point {
                x: cx + k,
                y: cy + r,
            },
            Point { x: cx, y: cy + r },
        )),
        PathVerb::CubicTo((
            Point {
                x: cx - k,
                y: cy + r,
            },
            Point {
                x: cx - r,
                y: cy + k,
            },
            Point { x: cx - r, y: cy },
        )),
        PathVerb::CubicTo((
            Point {
                x: cx - r,
                y: cy - k,
            },
            Point {
                x: cx - k,
                y: cy - r,
            },
            Point { x: cx, y: cy - r },
        )),
        PathVerb::CubicTo((
            Point {
                x: cx + k,
                y: cy - r,
            },
            Point {
                x: cx + r,
                y: cy - k,
            },
            Point { x: cx + r, y: cy },
        )),
        PathVerb::Close,
    ]
}

impl Guest for HelloPlugin {
    fn describe() -> Manifest {
        Manifest {
            name: "hello".to_string(),
            version: "0.1.0".to_string(),
            description: "Minimal example plugin: a filled circle".to_string(),
            kinds: vec![PluginKind::ClipSource],
            params: vec![
                ParamSpec {
                    id: "radius".to_string(),
                    label: "Radius".to_string(),
                    kind: ParamKind::Number(NumberRange {
                        min: 1.0,
                        max: 512.0,
                        default: 96.0,
                        step: 1.0,
                    }),
                },
                ParamSpec {
                    id: "color".to_string(),
                    label: "Color".to_string(),
                    kind: ParamKind::Color(ColorParam {
                        default: Color {
                            r: 90,
                            g: 170,
                            b: 255,
                            a: 255,
                        },
                    }),
                },
            ],
        }
    }

    fn render_clip(params: Vec<ParamEntry>, _time_ticks: i64, size: Size) -> Scene {
        let radius = get_number(&params, "radius", 96.0) as f32;
        let color = get_color(
            &params,
            "color",
            Color {
                r: 90,
                g: 170,
                b: 255,
                a: 255,
            },
        );
        host::log(&format!(
            "hello: circle radius={radius} at {}x{}",
            size.width, size.height
        ));

        Scene {
            elements: vec![DrawCommand::FillPath((
                circle(size.width as f32 / 2.0, size.height as f32 / 2.0, radius),
                color,
            ))],
        }
    }

    fn apply_effect(input: Scene, _params: Vec<ParamEntry>, _t: i64, _size: Size) -> Scene {
        input
    }

    fn evaluate_curve(_t: i64, _duration: i64, _params: Vec<ParamEntry>) -> f64 {
        0.0
    }
}

export!(HelloPlugin);
