//! First-party `graph` plugin: a Mafs-like coordinate plane drawn entirely in
//! the plugin, with function plots, points, segments, circles, polygons and
//! vectors and parametric curves, plus grid/axes/labels from host text shaping.
//!
//! The `items` parameter is a newline-separated list of `key=value` fields.
//! The first field is the item kind (`plot`, `point`, `segment`, `circle`,
//! `polygon`, `vector`, `parametric`); the rest configure it, e.g.
//! `plot y=x^2 color=#3b82f6` or `circle cx=0 cy=0 r=1`.

wit_bindgen::generate!({
    path: "../../crates/plugin-api/wit",
    world: "source-plugin",
});

use animathio::plugin::host;
use animathio::plugin::types::*;

struct GraphPlugin;

const AXIS_COLOR: Color = Color {
    r: 200,
    g: 205,
    b: 220,
    a: 255,
};
const GRID_COLOR: Color = Color {
    r: 120,
    g: 130,
    b: 150,
    a: 90,
};

// ---------------------------------------------------------------------------
// Parameter helpers

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

fn get_bool(params: &[ParamEntry], id: &str, default: bool) -> bool {
    params
        .iter()
        .find(|e| e.id == id)
        .and_then(|e| match &e.value {
            ParamValue::Bool(b) => Some(*b),
            _ => None,
        })
        .unwrap_or(default)
}

// ---------------------------------------------------------------------------
// Expression parser for `y = f(x)`

#[derive(Clone)]
enum Expr {
    Num(f64),
    Var,
    Neg(Box<Expr>),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
    Div(Box<Expr>, Box<Expr>),
    Pow(Box<Expr>, Box<Expr>),
    Func(Func, Box<Expr>),
}

#[derive(Clone, Copy)]
enum Func {
    Sin,
    Cos,
    Tan,
    Exp,
    Log,
    Ln,
    Sqrt,
    Abs,
}

fn eval(expr: &Expr, x: f64) -> f64 {
    match expr {
        Expr::Num(n) => *n,
        Expr::Var => x,
        Expr::Neg(inner) => -eval(inner, x),
        Expr::Add(a, b) => eval(a, x) + eval(b, x),
        Expr::Sub(a, b) => eval(a, x) - eval(b, x),
        Expr::Mul(a, b) => eval(a, x) * eval(b, x),
        Expr::Div(a, b) => eval(a, x) / eval(b, x),
        Expr::Pow(a, b) => eval(a, x).powf(eval(b, x)),
        Expr::Func(func, inner) => {
            let value = eval(inner, x);
            match func {
                Func::Sin => value.sin(),
                Func::Cos => value.cos(),
                Func::Tan => value.tan(),
                Func::Exp => value.exp(),
                Func::Log | Func::Ln => value.ln(),
                Func::Sqrt => value.sqrt(),
                Func::Abs => value.abs(),
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Token {
    Num(f64),
    Var,
    Ident(&'static str),
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    LParen,
    RParen,
}

fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let bytes = input.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        match c {
            ' ' | '\t' | '\r' | '\n' => i += 1,
            '+' => {
                tokens.push(Token::Plus);
                i += 1;
            }
            '-' => {
                tokens.push(Token::Minus);
                i += 1;
            }
            '*' => {
                tokens.push(Token::Star);
                i += 1;
            }
            '/' => {
                tokens.push(Token::Slash);
                i += 1;
            }
            '^' => {
                tokens.push(Token::Caret);
                i += 1;
            }
            '(' => {
                tokens.push(Token::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                i += 1;
            }
            '0'..='9' | '.' => {
                let start = i;
                while i < bytes.len() && matches!(bytes[i] as char, '0'..='9' | '.') {
                    i += 1;
                }
                let text = &input[start..i];
                let value = text
                    .parse::<f64>()
                    .map_err(|_| format!("invalid number `{text}`"))?;
                tokens.push(Token::Num(value));
            }
            'a'..='z' | 'A'..='Z' => {
                let start = i;
                while i < bytes.len() && (bytes[i] as char).is_ascii_alphabetic() {
                    i += 1;
                }
                let text = &input[start..i];
                if text.eq_ignore_ascii_case("x") || text.eq_ignore_ascii_case("t") {
                    tokens.push(Token::Var);
                } else {
                    let func = match text.to_ascii_lowercase().as_str() {
                        "sin" => Func::Sin,
                        "cos" => Func::Cos,
                        "tan" => Func::Tan,
                        "exp" => Func::Exp,
                        "log" => Func::Log,
                        "ln" => Func::Ln,
                        "sqrt" => Func::Sqrt,
                        "abs" => Func::Abs,
                        _ => return Err(format!("unknown function `{text}`")),
                    };
                    // Reuse Ident with a static tag; Func is carried via a
                    // side table lookup in the parser below.
                    tokens.push(match func {
                        Func::Sin => Token::Ident("sin"),
                        Func::Cos => Token::Ident("cos"),
                        Func::Tan => Token::Ident("tan"),
                        Func::Exp => Token::Ident("exp"),
                        Func::Log => Token::Ident("log"),
                        Func::Ln => Token::Ident("ln"),
                        Func::Sqrt => Token::Ident("sqrt"),
                        Func::Abs => Token::Ident("abs"),
                    });
                }
            }
            _ => return Err(format!("unexpected character `{c}`")),
        }
    }
    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<Token> {
        self.tokens.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.peek();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    fn parse_expr(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_term()?;
        loop {
            match self.peek() {
                Some(Token::Plus) => {
                    self.next();
                    let right = self.parse_term()?;
                    left = Expr::Add(Box::new(left), Box::new(right));
                }
                Some(Token::Minus) => {
                    self.next();
                    let right = self.parse_term()?;
                    left = Expr::Sub(Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_term(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_factor()?;
        loop {
            match self.peek() {
                Some(Token::Star) => {
                    self.next();
                    let right = self.parse_factor()?;
                    left = Expr::Mul(Box::new(left), Box::new(right));
                }
                Some(Token::Slash) => {
                    self.next();
                    let right = self.parse_factor()?;
                    left = Expr::Div(Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_factor(&mut self) -> Result<Expr, String> {
        let base = self.parse_unary()?;
        if self.peek() == Some(Token::Caret) {
            self.next();
            let exponent = self.parse_factor()?;
            return Ok(Expr::Pow(Box::new(base), Box::new(exponent)));
        }
        Ok(base)
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Some(Token::Minus) => {
                self.next();
                Ok(Expr::Neg(Box::new(self.parse_unary()?)))
            }
            Some(Token::Plus) => {
                self.next();
                self.parse_unary()
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        match self.next() {
            Some(Token::Num(n)) => Ok(Expr::Num(n)),
            Some(Token::Var) => Ok(Expr::Var),
            Some(Token::Ident(name)) => {
                if self.next() != Some(Token::LParen) {
                    return Err(format!("function `{name}` needs parentheses"));
                }
                let arg = self.parse_expr()?;
                if self.next() != Some(Token::RParen) {
                    return Err(format!("function `{name}` is missing `)`"));
                }
                let func = match name {
                    "sin" => Func::Sin,
                    "cos" => Func::Cos,
                    "tan" => Func::Tan,
                    "exp" => Func::Exp,
                    "log" => Func::Log,
                    "ln" => Func::Ln,
                    "sqrt" => Func::Sqrt,
                    "abs" => Func::Abs,
                    _ => return Err(format!("unknown function `{name}`")),
                };
                Ok(Expr::Func(func, Box::new(arg)))
            }
            Some(Token::LParen) => {
                let inner = self.parse_expr()?;
                if self.next() != Some(Token::RParen) {
                    return Err("missing `)`".to_string());
                }
                Ok(inner)
            }
            Some(other) => Err(format!("unexpected token {other:?}")),
            None => Err("unexpected end of expression".to_string()),
        }
    }
}

fn parse_expression(input: &str) -> Result<Expr, String> {
    let tokens = tokenize(input)?;
    let mut parser = Parser { tokens, pos: 0 };
    let expr = parser.parse_expr()?;
    if parser.peek().is_some() {
        return Err("unexpected trailing input".to_string());
    }
    Ok(expr)
}

// ---------------------------------------------------------------------------
// Graph items

enum GraphItem {
    Parametric {
        x: Expr,
        y: Expr,
        from: f64,
        to: f64,
        color: Color,
    },
    Plot {
        expr: Expr,
        color: Color,
    },
    Point {
        x: f64,
        y: f64,
        color: Color,
    },
    Segment {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        color: Color,
    },
    Circle {
        cx: f64,
        cy: f64,
        r: f64,
        color: Color,
    },
    Polygon {
        points: Vec<(f64, f64)>,
        color: Color,
    },
    Vector {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        color: Color,
    },
}

fn parse_color(value: &str, default: Color) -> Color {
    if let Some(hex) = value.strip_prefix('#') {
        if hex.len() == 6 {
            if let Ok(rgb) = u32::from_str_radix(hex, 16) {
                return Color {
                    r: (rgb >> 16) as u8,
                    g: (rgb >> 8) as u8,
                    b: rgb as u8,
                    a: 255,
                };
            }
        }
        return default;
    }

    let parts: Vec<&str> = value.split(',').collect();
    if parts.len() == 3 || parts.len() == 4 {
        let r = parts[0].trim().parse::<u8>();
        let g = parts[1].trim().parse::<u8>();
        let b = parts[2].trim().parse::<u8>();
        let a = if parts.len() == 4 {
            parts[3].trim().parse::<u8>()
        } else {
            Ok(255)
        };
        if let (Ok(r), Ok(g), Ok(b), Ok(a)) = (r, g, b, a) {
            return Color { r, g, b, a };
        }
    }
    default
}

fn parse_number(value: &str) -> Option<f64> {
    value.trim().parse::<f64>().ok()
}

fn split_assignments(line: &str) -> Vec<(&str, &str)> {
    line.split_whitespace()
        .filter_map(|part| {
            let (key, value) = part.split_once('=')?;
            Some((key, value))
        })
        .collect()
}

fn parse_items(text: &str, default_color: Color) -> Vec<GraphItem> {
    let mut items = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some(kind) = line.split_whitespace().next() else {
            continue;
        };
        let fields = split_assignments(line);
        let color = fields
            .iter()
            .find(|(key, _)| *key == "color")
            .map(|(_, value)| parse_color(value, default_color))
            .unwrap_or(default_color);

        let number = |name: &str, default: f64| -> f64 {
            fields
                .iter()
                .find(|(key, _)| *key == name)
                .and_then(|(_, value)| parse_number(value))
                .unwrap_or(default)
        };

        match kind {
            "parametric" => {
                let expression = |name| {
                    fields
                        .iter()
                        .find(|(key, _)| *key == name)
                        .and_then(|(_, value)| parse_expression(value).ok())
                };
                if let (Some(x), Some(y)) = (expression("x"), expression("y")) {
                    let (from, to) = (number("from", 0.0), number("to", std::f64::consts::TAU));
                    if from.is_finite() && to.is_finite() && to > from {
                        items.push(GraphItem::Parametric {
                            x,
                            y,
                            from,
                            to,
                            color,
                        });
                    }
                }
            }
            "plot" => {
                let y = fields
                    .iter()
                    .find(|(key, _)| *key == "y")
                    .map(|(_, value)| *value)
                    .unwrap_or("0");
                if let Ok(expr) = parse_expression(y) {
                    items.push(GraphItem::Plot { expr, color });
                }
            }
            "point" => {
                items.push(GraphItem::Point {
                    x: number("x", 0.0),
                    y: number("y", 0.0),
                    color,
                });
            }
            "segment" => {
                items.push(GraphItem::Segment {
                    x1: number("x1", 0.0),
                    y1: number("y1", 0.0),
                    x2: number("x2", 1.0),
                    y2: number("y2", 1.0),
                    color,
                });
            }
            "circle" => {
                items.push(GraphItem::Circle {
                    cx: number("cx", 0.0),
                    cy: number("cy", 0.0),
                    r: number("r", 1.0),
                    color,
                });
            }
            "polygon" => {
                let points = fields
                    .iter()
                    .find(|(key, _)| *key == "points")
                    .map(|(_, value)| {
                        value
                            .split(';')
                            .filter_map(|pair| {
                                let (x, y) = pair.split_once(',')?;
                                Some((parse_number(x)?, parse_number(y)?))
                            })
                            .collect::<Vec<(f64, f64)>>()
                    })
                    .unwrap_or_default();
                if points.len() >= 3 {
                    items.push(GraphItem::Polygon { points, color });
                }
            }
            "vector" => {
                items.push(GraphItem::Vector {
                    x1: number("x1", 0.0),
                    y1: number("y1", 0.0),
                    x2: number("x2", 1.0),
                    y2: number("y2", 1.0),
                    color,
                });
            }
            _ => {}
        }
    }
    items
}

// ---------------------------------------------------------------------------
// Geometry helpers

/// Axis tick text: integers without a decimal point, others to one decimal.
fn tick_label(tick: f64) -> String {
    if (tick - tick.round()).abs() < 1e-9 {
        format!("{tick:.0}")
    } else {
        format!("{tick:.1}")
    }
}

fn map_point(x: f64, y: f64, bounds: (f64, f64, f64, f64), size: Size) -> Point {
    let (x_min, x_max, y_min, y_max) = bounds;
    let px = (x - x_min) / (x_max - x_min) * size.width as f64;
    let py = size.height as f64 - (y - y_min) / (y_max - y_min) * size.height as f64;
    Point {
        x: px as f32,
        y: py as f32,
    }
}

fn polyline(points: &[(f64, f64)], bounds: (f64, f64, f64, f64), size: Size) -> Vec<PathVerb> {
    let mut verbs = Vec::with_capacity(points.len() + 1);
    for (index, (x, y)) in points.iter().enumerate() {
        let p = map_point(*x, *y, bounds, size);
        if index == 0 {
            verbs.push(PathVerb::MoveTo(p));
        } else {
            verbs.push(PathVerb::LineTo(p));
        }
    }
    verbs
}

fn circle_verbs(
    cx: f64,
    cy: f64,
    r: f64,
    bounds: (f64, f64, f64, f64),
    size: Size,
) -> Vec<PathVerb> {
    let center = map_point(cx, cy, bounds, size);
    let px = (r / (bounds.1 - bounds.0)) * size.width as f64;
    let py = (r / (bounds.3 - bounds.2)) * size.height as f64;
    ellipse_verbs(center.x, center.y, px as f32, py as f32)
}

fn ellipse_verbs(cx: f32, cy: f32, rx: f32, ry: f32) -> Vec<PathVerb> {
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

fn vector_verbs(
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    bounds: (f64, f64, f64, f64),
    size: Size,
) -> Vec<PathVerb> {
    let tail = map_point(x1, y1, bounds, size);
    let tip = map_point(x2, y2, bounds, size);
    let dx = tip.x - tail.x;
    let dy = tip.y - tail.y;
    let angle = dy.atan2(dx);
    let head_len = 8.0_f32;
    let head_width = 5.0_f32;
    let left = Point {
        x: tip.x - head_len * angle.cos() + head_width * angle.sin(),
        y: tip.y - head_len * angle.sin() - head_width * angle.cos(),
    };
    let right = Point {
        x: tip.x - head_len * angle.cos() - head_width * angle.sin(),
        y: tip.y - head_len * angle.sin() + head_width * angle.cos(),
    };
    vec![
        PathVerb::MoveTo(tail),
        PathVerb::LineTo(tip),
        PathVerb::MoveTo(left),
        PathVerb::LineTo(tip),
        PathVerb::LineTo(right),
    ]
}

fn nice_step(span: f64) -> f64 {
    if span <= 0.0 {
        return 1.0;
    }
    let raw = span / 8.0;
    let magnitude = 10.0_f64.powf(raw.log10().floor());
    let residual = raw / magnitude;
    let factor = if residual < 1.5 {
        1.0
    } else if residual < 3.5 {
        2.0
    } else if residual < 7.5 {
        5.0
    } else {
        10.0
    };
    factor * magnitude
}

// ---------------------------------------------------------------------------
// Plugin implementation

impl Guest for GraphPlugin {
    fn describe() -> Manifest {
        Manifest {
            name: "graph".to_string(),
            version: "1.2.0".to_string(),
            description: "Mafs-like coordinate plane with plots and shapes".to_string(),
            kinds: vec!["clip-source".into()],
            metadata: vec![],
            params: vec![
                ParamSpec {
                    id: "x_min".to_string(),
                    label: "X Min".to_string(),
                    kind: ParamKind::Number(NumberRange {
                        min: -1000.0,
                        max: 1000.0,
                        default: -5.0,
                        step: 0.5,
                    }),
                },
                ParamSpec {
                    id: "x_max".to_string(),
                    label: "X Max".to_string(),
                    kind: ParamKind::Number(NumberRange {
                        min: -1000.0,
                        max: 1000.0,
                        default: 5.0,
                        step: 0.5,
                    }),
                },
                ParamSpec {
                    id: "y_min".to_string(),
                    label: "Y Min".to_string(),
                    kind: ParamKind::Number(NumberRange {
                        min: -1000.0,
                        max: 1000.0,
                        default: -5.0,
                        step: 0.5,
                    }),
                },
                ParamSpec {
                    id: "y_max".to_string(),
                    label: "Y Max".to_string(),
                    kind: ParamKind::Number(NumberRange {
                        min: -1000.0,
                        max: 1000.0,
                        default: 5.0,
                        step: 0.5,
                    }),
                },
                ParamSpec {
                    id: "show_grid".to_string(),
                    label: "Grid".to_string(),
                    kind: ParamKind::Bool(BoolParam { default: true }),
                },
                ParamSpec {
                    id: "show_axes".to_string(),
                    label: "Axes".to_string(),
                    kind: ParamKind::Bool(BoolParam { default: true }),
                },
                ParamSpec {
                    id: "show_labels".to_string(),
                    label: "Labels".to_string(),
                    kind: ParamKind::Bool(BoolParam { default: true }),
                },
                ParamSpec {
                    id: "scale_mode".into(),
                    label: "Coordinate Scale".into(),
                    kind: ParamKind::EnumChoice(EnumParam {
                        options: vec!["uniform".into(), "stretch".into()],
                        default: "uniform".into(),
                    }),
                },
                ParamSpec {
                    id: "draw".to_string(),
                    label: "Draw".to_string(),
                    kind: ParamKind::Number(NumberRange {
                        min: 0.0,
                        max: 1.0,
                        default: 1.0,
                        step: 0.01,
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
                ParamSpec {
                    id: "stroke_width".into(),
                    label: "Plot Stroke Width (540p px)".into(),
                    kind: ParamKind::Number(NumberRange {
                        min: 0.1,
                        max: 64.0,
                        default: 2.0,
                        step: 0.1,
                    }),
                },
                ParamSpec {
                    id: "axis_color".into(),
                    label: "Axis Color".into(),
                    kind: ParamKind::Color(ColorParam {
                        default: AXIS_COLOR,
                    }),
                },
                ParamSpec {
                    id: "grid_color".into(),
                    label: "Grid Color".into(),
                    kind: ParamKind::Color(ColorParam {
                        default: GRID_COLOR,
                    }),
                },
                ParamSpec {
                    id: "items".to_string(),
                    label: "Items".to_string(),
                    kind: ParamKind::String(StringParam {
                        default: "plot y=x^2\ncircle cx=0 cy=0 r=1".to_string(),
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
        let x_min = get_number(&params, "x_min", -5.0);
        let x_max = get_number(&params, "x_max", 5.0);
        let y_min = get_number(&params, "y_min", -5.0);
        let y_max = get_number(&params, "y_max", 5.0);
        let show_grid = get_bool(&params, "show_grid", true);
        let show_axes = get_bool(&params, "show_axes", true);
        let show_labels = get_bool(&params, "show_labels", true);
        let draw = get_number(&params, "draw", 1.0).clamp(0.0, 1.0);
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
        let items_text = get_string(&params, "items", "plot y=x^2");

        if x_max <= x_min || y_max <= y_min {
            return Ok(Scene {
                coordinate_space: CoordinateSpace::Canvas,
                elements: Vec::new(),
            });
        }
        // Uniform is the default: circles stay round. Function charts may explicitly
        // stretch the requested ranges to the viewport for independent axis scales.
        let (width, height) = (f64::from(size.width), f64::from(size.height));
        let (x_min, x_max, y_min, y_max) =
            if get_string(&params, "scale_mode", "uniform") == "stretch" {
                (x_min, x_max, y_min, y_max)
            } else {
                let per_unit = (width / (x_max - x_min)).min(height / (y_max - y_min));
                let (mid_x, mid_y) = ((x_min + x_max) / 2.0, (y_min + y_max) / 2.0);
                let (half_x, half_y) = (width / per_unit / 2.0, height / per_unit / 2.0);
                (
                    mid_x - half_x,
                    mid_x + half_x,
                    mid_y - half_y,
                    mid_y + half_y,
                )
            };
        // Stroke widths and label size follow the shorter canvas edge (1080p: 2x).
        let px = (width.min(height) / 540.0).max(1.0);
        let label_size = (12.0 * px) as f32;
        let bounds = (x_min, x_max, y_min, y_max);

        let mut elements = Vec::new();
        let grid_color = get_color(&params, "grid_color", GRID_COLOR);
        let axis_color = get_color(&params, "axis_color", AXIS_COLOR);
        let stroke_width = (get_number(&params, "stroke_width", 2.0).clamp(0.1, 64.0) * px) as f32;
        let stroke = StrokeStyle {
            width: (1.5 * px) as f32,
            color: axis_color,
            cap: StrokeCap::Round,
            join: StrokeJoin::Round,
        };

        if show_grid {
            let step = nice_step((x_max - x_min).max(y_max - y_min));
            let mut gx = (x_min / step).ceil() * step;
            while gx <= x_max {
                let p1 = map_point(gx, y_min, bounds, size);
                let p2 = map_point(gx, y_max, bounds, size);
                elements.push(DrawCommand::StrokePath((
                    vec![PathVerb::MoveTo(p1), PathVerb::LineTo(p2)],
                    StrokeStyle {
                        width: px as f32,
                        color: grid_color,
                        cap: StrokeCap::Butt,
                        join: StrokeJoin::Miter,
                    },
                )));
                gx += step;
            }
            let mut gy = (y_min / step).ceil() * step;
            while gy <= y_max {
                let p1 = map_point(x_min, gy, bounds, size);
                let p2 = map_point(x_max, gy, bounds, size);
                elements.push(DrawCommand::StrokePath((
                    vec![PathVerb::MoveTo(p1), PathVerb::LineTo(p2)],
                    StrokeStyle {
                        width: px as f32,
                        color: grid_color,
                        cap: StrokeCap::Butt,
                        join: StrokeJoin::Miter,
                    },
                )));
                gy += step;
            }
        }

        if show_axes {
            if (y_min..=y_max).contains(&0.0) {
                let p1 = map_point(x_min, 0.0, bounds, size);
                let p2 = map_point(x_max, 0.0, bounds, size);
                elements.push(DrawCommand::StrokePath((
                    vec![PathVerb::MoveTo(p1), PathVerb::LineTo(p2)],
                    stroke,
                )));
            }
            if (x_min..=x_max).contains(&0.0) {
                let p1 = map_point(0.0, y_min, bounds, size);
                let p2 = map_point(0.0, y_max, bounds, size);
                elements.push(DrawCommand::StrokePath((
                    vec![PathVerb::MoveTo(p1), PathVerb::LineTo(p2)],
                    stroke,
                )));
            }
        }

        if show_labels {
            let step = nice_step((x_max - x_min).max(y_max - y_min));
            let mut tick = (x_min / step).ceil() * step;
            while tick <= x_max {
                if tick.abs() > f64::EPSILON {
                    let p = map_point(tick, 0.0, bounds, size);
                    let label = tick_label(tick);
                    for contour in host::shape_text(&label, label_size)? {
                        let offset: Vec<PathVerb> = contour
                            .iter()
                            .map(|verb| offset_verb(verb, p.x, p.y + (4.0 * px) as f32))
                            .collect();
                        elements.push(DrawCommand::FillPath((offset, axis_color)));
                    }
                }
                tick += step;
            }
            let mut tick = (y_min / step).ceil() * step;
            while tick <= y_max {
                if tick.abs() > f64::EPSILON {
                    let p = map_point(0.0, tick, bounds, size);
                    let label = tick_label(tick);
                    for contour in host::shape_text(&label, label_size)? {
                        let offset: Vec<PathVerb> = contour
                            .iter()
                            .map(|verb| {
                                offset_verb(verb, p.x - (18.0 * px) as f32, p.y - (6.0 * px) as f32)
                            })
                            .collect();
                        elements.push(DrawCommand::FillPath((offset, axis_color)));
                    }
                }
                tick += step;
            }
        }

        let items = parse_items(&items_text, color);
        let item_count = items.len().max(1);
        for (index, item) in items.iter().enumerate() {
            let threshold = index as f64 / item_count as f64;
            if draw <= threshold {
                continue;
            }
            match item {
                GraphItem::Parametric {
                    x,
                    y,
                    from,
                    to,
                    color,
                } => {
                    let samples = 256;
                    let mut path = Vec::with_capacity(samples + 1);
                    let mut connected = false;
                    for i in 0..=samples {
                        let t = from + (to - from) * draw * i as f64 / samples as f64;
                        let (x, y) = (eval(x, t), eval(y, t));
                        if x.is_finite() && y.is_finite() {
                            let point = map_point(x, y, bounds, size);
                            path.push(if connected {
                                PathVerb::LineTo(point)
                            } else {
                                PathVerb::MoveTo(point)
                            });
                            connected = true;
                        } else {
                            connected = false;
                        }
                    }
                    elements.push(DrawCommand::StrokePath((
                        path,
                        StrokeStyle {
                            width: stroke_width,
                            color: *color,
                            cap: StrokeCap::Round,
                            join: StrokeJoin::Round,
                        },
                    )));
                }
                GraphItem::Plot { expr, color } => {
                    let samples = 256;
                    let x_end = x_min + (x_max - x_min) * draw;
                    let mut points = Vec::with_capacity(samples + 1);
                    for i in 0..=samples {
                        let t = i as f64 / samples as f64;
                        let x = x_min + (x_end - x_min) * t;
                        let y = eval(expr, x);
                        if y.is_finite() {
                            points.push((x, y));
                        }
                    }
                    if points.len() >= 2 {
                        elements.push(DrawCommand::StrokePath((
                            polyline(&points, bounds, size),
                            StrokeStyle {
                                width: stroke_width,
                                color: *color,
                                cap: StrokeCap::Round,
                                join: StrokeJoin::Round,
                            },
                        )));
                    }
                }
                GraphItem::Point { x, y, color } => {
                    let p = map_point(*x, *y, bounds, size);
                    elements.push(DrawCommand::FillPath((
                        ellipse_verbs(p.x, p.y, (3.0 * px) as f32, (3.0 * px) as f32),
                        *color,
                    )));
                }
                GraphItem::Segment {
                    x1,
                    y1,
                    x2,
                    y2,
                    color,
                } => {
                    let p1 = map_point(*x1, *y1, bounds, size);
                    let p2 = map_point(*x2, *y2, bounds, size);
                    elements.push(DrawCommand::StrokePath((
                        vec![PathVerb::MoveTo(p1), PathVerb::LineTo(p2)],
                        StrokeStyle {
                            width: stroke_width,
                            color: *color,
                            cap: StrokeCap::Round,
                            join: StrokeJoin::Round,
                        },
                    )));
                }
                GraphItem::Circle { cx, cy, r, color } => {
                    elements.push(DrawCommand::StrokePath((
                        circle_verbs(*cx, *cy, *r, bounds, size),
                        StrokeStyle {
                            width: stroke_width,
                            color: *color,
                            cap: StrokeCap::Round,
                            join: StrokeJoin::Round,
                        },
                    )));
                }
                GraphItem::Polygon { points, color } => {
                    let mapped: Vec<(f64, f64)> = points.clone();
                    if mapped.len() >= 3 {
                        let mut path = polyline(&mapped, bounds, size);
                        path.push(PathVerb::Close);
                        elements.push(DrawCommand::FillPath((path, *color)));
                    }
                }
                GraphItem::Vector {
                    x1,
                    y1,
                    x2,
                    y2,
                    color,
                } => {
                    elements.push(DrawCommand::StrokePath((
                        vector_verbs(*x1, *y1, *x2, *y2, bounds, size),
                        StrokeStyle {
                            width: stroke_width,
                            color: *color,
                            cap: StrokeCap::Round,
                            join: StrokeJoin::Round,
                        },
                    )));
                }
            }
        }

        Ok(Scene {
            coordinate_space: CoordinateSpace::Canvas,
            elements,
        })
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

export!(GraphPlugin);
