wit_bindgen::generate!({
    path: "../../wit",
    world: "plugin",
});

use std::f32::consts::PI;
use animathio::plugin::types::*;
use animathio::plugin::host;

struct ShapesPlugin;

impl Guest for ShapesPlugin {
    fn describe() -> Manifest {
        host::log("shapes::describe called");
        Manifest {
            name: "shapes".to_string(),
            version: "0.1.0".to_string(),
            description: "Animated circle, regular polygon, and host-shaped text plugin".to_string(),
            params: vec![
                ParamSpec {
                    id: "radius".to_string(),
                    label: "Circle Radius".to_string(),
                    kind: ParamKind::Number(NumberRange {
                        min: 10.0,
                        max: 250.0,
                        default: 70.0,
                        step: 1.0,
                    }),
                },
                ParamSpec {
                    id: "polygon_sides".to_string(),
                    label: "Polygon Sides".to_string(),
                    kind: ParamKind::Number(NumberRange {
                        min: 3.0,
                        max: 12.0,
                        default: 5.0,
                        step: 1.0,
                    }),
                },
                ParamSpec {
                    id: "polygon_radius".to_string(),
                    label: "Polygon Radius".to_string(),
                    kind: ParamKind::Number(NumberRange {
                        min: 20.0,
                        max: 300.0,
                        default: 100.0,
                        step: 1.0,
                    }),
                },
                ParamSpec {
                    id: "circle_color".to_string(),
                    label: "Circle Color".to_string(),
                    kind: ParamKind::Color(ColorParam {
                        default: Color { r: 52, g: 152, b: 219, a: 220 },
                    }),
                },
                ParamSpec {
                    id: "polygon_color".to_string(),
                    label: "Polygon Color".to_string(),
                    kind: ParamKind::Color(ColorParam {
                        default: Color { r: 231, g: 76, b: 60, a: 200 },
                    }),
                },
                ParamSpec {
                    id: "stroke_color".to_string(),
                    label: "Stroke Color".to_string(),
                    kind: ParamKind::Color(ColorParam {
                        default: Color { r: 240, g: 240, b: 240, a: 255 },
                    }),
                },
                ParamSpec {
                    id: "stroke_width".to_string(),
                    label: "Stroke Width".to_string(),
                    kind: ParamKind::Number(NumberRange {
                        min: 1.0,
                        max: 20.0,
                        default: 4.0,
                        step: 0.5,
                    }),
                },
                ParamSpec {
                    id: "label_text".to_string(),
                    label: "Label Text".to_string(),
                    kind: ParamKind::String(StringParam {
                        default: "AniMathIO 2.0".to_string(),
                    }),
                },
                ParamSpec {
                    id: "label_color".to_string(),
                    label: "Label Color".to_string(),
                    kind: ParamKind::Color(ColorParam {
                        default: Color { r: 255, g: 255, b: 255, a: 255 },
                    }),
                },
                ParamSpec {
                    id: "show_label".to_string(),
                    label: "Show Label".to_string(),
                    kind: ParamKind::Bool(BoolParam {
                        default: true,
                    }),
                },
                ParamSpec {
                    id: "animation_mode".to_string(),
                    label: "Animation Mode".to_string(),
                    kind: ParamKind::EnumChoice(EnumParam {
                        options: vec!["smooth".to_string(), "pulse".to_string(), "spin".to_string()],
                        default: "smooth".to_string(),
                    }),
                },
            ],
        }
    }

    fn render(
        params: Vec<ParamEntry>,
        time_ticks: i64,
        size: Size,
    ) -> Scene {
        let mut radius = 70.0f32;
        let mut polygon_sides = 5.0f32;
        let mut polygon_radius = 100.0f32;
        let mut circle_color = Color { r: 52, g: 152, b: 219, a: 220 };
        let mut polygon_color = Color { r: 231, g: 76, b: 60, a: 200 };
        let mut stroke_color = Color { r: 240, g: 240, b: 240, a: 255 };
        let mut stroke_width = 4.0f32;
        let mut label_text = "AniMathIO 2.0".to_string();
        let mut label_color = Color { r: 255, g: 255, b: 255, a: 255 };
        let mut show_label = true;
        let mut _animation_mode = "smooth".to_string();

        for entry in &params {
            match entry.id.as_str() {
                "radius" => {
                    if let ParamValue::Number(n) = entry.value {
                        radius = n as f32;
                    }
                }
                "polygon_sides" => {
                    if let ParamValue::Number(n) = entry.value {
                        polygon_sides = n as f32;
                    }
                }
                "polygon_radius" => {
                    if let ParamValue::Number(n) = entry.value {
                        polygon_radius = n as f32;
                    }
                }
                "circle_color" => {
                    if let ParamValue::Color(c) = entry.value {
                        circle_color = c;
                    }
                }
                "polygon_color" => {
                    if let ParamValue::Color(c) = entry.value {
                        polygon_color = c;
                    }
                }
                "stroke_color" => {
                    if let ParamValue::Color(c) = entry.value {
                        stroke_color = c;
                    }
                }
                "stroke_width" => {
                    if let ParamValue::Number(n) = entry.value {
                        stroke_width = n as f32;
                    }
                }
                "label_text" => {
                    if let ParamValue::String(ref s) = entry.value {
                        label_text = s.clone();
                    }
                }
                "label_color" => {
                    if let ParamValue::Color(c) = entry.value {
                        label_color = c;
                    }
                }
                "show_label" => {
                    if let ParamValue::Bool(b) = entry.value {
                        show_label = b;
                    }
                }
                "animation_mode" => {
                    if let ParamValue::EnumChoice(ref e) = entry.value {
                        _animation_mode = e.clone();
                    }
                }
                _ => {}
            }
        }

        let t = (time_ticks as f32) / 60.0;
        let w = size.width as f32;
        let h = size.height as f32;

        let stroke_style = StrokeStyle {
            width: stroke_width,
            color: stroke_color,
            cap: StrokeCap::Round,
            join: StrokeJoin::Round,
        };

        let mut elements = Vec::new();

        // 1. Animated Circle
        // Circle oscillates in radius and vertical position
        let anim_radius = radius * (1.0 + 0.18 * (t * 2.0 * PI * 0.5).sin());
        let circle_cx = w * 0.32;
        let circle_cy = h * 0.45 + 15.0 * (t * 2.0 * PI * 0.75).cos();

        let circle_path = build_circle_path(circle_cx, circle_cy, anim_radius);
        elements.push(DrawCommand::FillPath((circle_path.clone(), circle_color)));
        elements.push(DrawCommand::StrokePath((circle_path, stroke_style.clone())));

        // 2. Animated Regular Polygon
        let poly_cx = w * 0.68;
        let poly_cy = h * 0.45;
        let sides = (polygon_sides.round() as usize).max(3);
        let rot_angle = t * 1.5; // Rotate 1.5 rad/s

        let poly_path = build_polygon_path(poly_cx, poly_cy, polygon_radius, sides, rot_angle);
        elements.push(DrawCommand::FillPath((poly_path.clone(), polygon_color)));
        elements.push(DrawCommand::StrokePath((poly_path, stroke_style.clone())));

        // 3. Text Label: use host-side shape_text import
        if show_label && !label_text.is_empty() {
            // Call host font shaper
            let shaped_contours = host::shape_text(&label_text, 38.0);
            if !shaped_contours.is_empty() {
                // Position text at center bottom
                let text_x = w * 0.5 - 120.0;
                let text_y = h * 0.82;
                for contour in shaped_contours {
                    let offset_path = offset_path_verbs(&contour, text_x, text_y);
                    elements.push(DrawCommand::FillPath((offset_path, label_color)));
                }
            } else {
                // Fallback to text_run command
                elements.push(DrawCommand::TextRun(TextElement {
                    text: label_text,
                    font_size: 38.0,
                    x: w * 0.5 - 120.0,
                    y: h * 0.82,
                    color: label_color,
                }));
            }
        }

        Scene { elements }
    }
}

fn build_circle_path(cx: f32, cy: f32, r: f32) -> Vec<PathVerb> {
    // 4 cubic Béziers approximating a circle
    let k = 0.55228475 * r;
    vec![
        PathVerb::MoveTo(Point { x: cx + r, y: cy }),
        PathVerb::CubicTo((
            Point { x: cx + r, y: cy + k },
            Point { x: cx + k, y: cy + r },
            Point { x: cx, y: cy + r },
        )),
        PathVerb::CubicTo((
            Point { x: cx - k, y: cy + r },
            Point { x: cx - r, y: cy + k },
            Point { x: cx - r, y: cy },
        )),
        PathVerb::CubicTo((
            Point { x: cx - r, y: cy - k },
            Point { x: cx - k, y: cy - r },
            Point { x: cx, y: cy - r },
        )),
        PathVerb::CubicTo((
            Point { x: cx + k, y: cy - r },
            Point { x: cx + r, y: cy - k },
            Point { x: cx + r, y: cy },
        )),
        PathVerb::Close,
    ]
}

fn build_polygon_path(cx: f32, cy: f32, r: f32, sides: usize, rot: f32) -> Vec<PathVerb> {
    let mut verbs = Vec::with_capacity(sides + 2);
    let step = 2.0 * PI / (sides as f32);

    for i in 0..sides {
        let theta = rot + (i as f32) * step;
        let x = cx + r * theta.cos();
        let y = cy + r * theta.sin();
        if i == 0 {
            verbs.push(PathVerb::MoveTo(Point { x, y }));
        } else {
            verbs.push(PathVerb::LineTo(Point { x, y }));
        }
    }
    verbs.push(PathVerb::Close);
    verbs
}

fn offset_path_verbs(verbs: &[PathVerb], ox: f32, oy: f32) -> Vec<PathVerb> {
    verbs
        .iter()
        .map(|v| match v {
            PathVerb::MoveTo(p) => PathVerb::MoveTo(Point {
                x: p.x + ox,
                y: p.y + oy,
            }),
            PathVerb::LineTo(p) => PathVerb::LineTo(Point {
                x: p.x + ox,
                y: p.y + oy,
            }),
            PathVerb::QuadTo((p1, p2)) => PathVerb::QuadTo((
                Point {
                    x: p1.x + ox,
                    y: p1.y + oy,
                },
                Point {
                    x: p2.x + ox,
                    y: p2.y + oy,
                },
            )),
            PathVerb::CubicTo((p1, p2, p3)) => PathVerb::CubicTo((
                Point {
                    x: p1.x + ox,
                    y: p1.y + oy,
                },
                Point {
                    x: p2.x + ox,
                    y: p2.y + oy,
                },
                Point {
                    x: p3.x + ox,
                    y: p3.y + oy,
                },
            )),
            PathVerb::Close => PathVerb::Close,
        })
        .collect()
}

export!(ShapesPlugin);
