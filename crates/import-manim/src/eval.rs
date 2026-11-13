//! Evaluation of the small expression fragments Manim scripts use for
//! positioning, colouring and timing (numbers, vectors, colours, string literals).

use crate::Rgba;
use tree_sitter::Node;

/// A resolved expression value, kept deliberately small.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Value {
    /// String literal (quotes and raw-string prefixes stripped).
    Str(String),
    /// Numeric literal or numeric-constant expression.
    Num(f64),
    /// Two-component vector (Manim z is ignored for 2D scenes).
    Vec2 { x: f64, y: f64 },
    /// Resolved colour.
    Color(Rgba),
    /// Unresolved identifier (e.g. a mobject variable name).
    Ident(String),
    /// Python `None`.
    PythonNone,
    /// Anything the importer does not interpret.
    Unknown,
}

/// Evaluate an expression node to a [`Value`].
pub(crate) fn eval(node: &Node<'_>, source: &str) -> Value {
    eval_at_depth(node, source, 0)
}

fn eval_at_depth(node: &Node<'_>, source: &str, depth: usize) -> Value {
    if depth > 64 {
        return Value::Unknown;
    }
    match node.kind() {
        "string" => Value::Str(unquote(node, source)),
        "concatenated_string" => {
            let mut joined = String::new();
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                joined.push_str(&unquote(&child, source));
            }
            Value::Str(joined)
        }
        "integer" | "float" => number_literal(node, source).map_or(Value::Unknown, Value::Num),
        "identifier" => eval_identifier(node, source),
        "list" | "tuple" => eval_sequence(node, source, depth),
        "parenthesized_expression" => node.named_child(0).map_or(Value::Unknown, |inner| {
            eval_at_depth(&inner, source, depth + 1)
        }),
        "unary_operator" => eval_unary(node, source, depth),
        "binary_operator" => eval_binary(node, source, depth),
        "call" => eval_call(node, source, depth),
        "attribute" => eval_attribute(node, source),
        "none" => Value::PythonNone,
        _ => Value::Unknown,
    }
}

/// Extract a number, if the value is one.
pub(crate) fn as_number(value: &Value) -> Option<f64> {
    match value {
        Value::Num(n) => Some(*n),
        _ => None,
    }
}

/// Extract a 2D vector, if the value is one.
pub(crate) fn as_vector(value: &Value) -> Option<(f64, f64)> {
    match value {
        Value::Vec2 { x, y } => Some((*x, *y)),
        _ => None,
    }
}

/// Extract a string, if the value is one.
pub(crate) fn as_string(value: &Value) -> Option<&str> {
    match value {
        Value::Str(s) => Some(s),
        _ => None,
    }
}

/// Extract an identifier, if the value is one.
pub(crate) fn as_ident(value: &Value) -> Option<&str> {
    match value {
        Value::Ident(s) => Some(s),
        _ => None,
    }
}

/// Resolve a colour from a name, hex string or colour value.
pub(crate) fn as_color(value: &Value) -> Option<Rgba> {
    match value {
        Value::Color(c) => Some(*c),
        Value::Str(s) => parse_hex_color(s),
        Value::Ident(name) => manim_color(name),
        _ => None,
    }
}

/// Resolve a Manim colour constant (e.g. `BLUE`) to its RGBA value.
pub(crate) fn manim_color(name: &str) -> Option<Rgba> {
    let hex = match name.to_ascii_uppercase().as_str() {
        "WHITE" => "#FFFFFF",
        "BLACK" => "#000000",
        "GRAY" | "GREY" => "#888888",
        "LIGHT_GRAY" | "LIGHT_GREY" => "#BBBBBB",
        "DARK_GRAY" | "DARK_GREY" => "#444444",
        "RED" => "#FC6255",
        "GREEN" => "#83C167",
        "BLUE" => "#58C4DD",
        "YELLOW" => "#FFFF00",
        "ORANGE" => "#FF862F",
        "PURPLE" => "#9A72AC",
        "PINK" => "#D147BD",
        "TEAL" => "#5CD0B3",
        "GOLD" => "#C7A52C",
        "MAROON" => "#B03060",
        "SILVER" => "#C0C0C0",
        "BROWN" => "#A52A2A",
        _ => return None,
    };
    parse_hex_color(hex)
}

/// Parse `#RGB`, `#RGBA`, `#RRGGBB` or `#RRGGBBAA` into an RGBA colour.
pub(crate) fn parse_hex_color(value: &str) -> Option<Rgba> {
    let hex = value.trim();
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |range: std::ops::Range<usize>| u8::from_str_radix(&hex[range], 16).ok();
    match hex.len() {
        3 => Some([byte(0..1)? * 17, byte(1..2)? * 17, byte(2..3)? * 17, 255]),
        4 => Some([
            byte(0..1)? * 17,
            byte(1..2)? * 17,
            byte(2..3)? * 17,
            byte(3..4)? * 17,
        ]),
        6 => Some([byte(0..2)?, byte(2..4)?, byte(4..6)?, 255]),
        8 => Some([byte(0..2)?, byte(2..4)?, byte(4..6)?, byte(6..8)?]),
        _ => None,
    }
}

/// Source text of a node.
pub(crate) fn text_of(node: &Node<'_>, source: &str) -> String {
    node.utf8_text(source.as_bytes())
        .map_or_default(str::to_owned)
}

/// Strip a Python string literal's quotes and (raw/unicode/bytes) prefix.
/// Escape processing is intentionally skipped: Manim LaTeX sources are raw
/// strings, and this matches the v1 translator's behaviour.
pub(crate) fn unquote(node: &Node<'_>, source: &str) -> String {
    unquote_str(&text_of(node, source))
}

/// String-only variant of [`unquote`], shared with concatenated literals.
pub(crate) fn unquote_str(raw: &str) -> String {
    let s = raw.trim();
    let s = strip_prefixes(s);
    if s.len() >= 6 && s.starts_with("'''") && s.ends_with("'''") {
        return s[3..s.len() - 3].to_owned();
    }
    if s.len() >= 6 && s.starts_with("\"\"\"") && s.ends_with("\"\"\"") {
        return s[3..s.len() - 3].to_owned();
    }
    let bytes = s.as_bytes();
    if s.len() >= 2
        && ((bytes[0] == b'\'' && bytes[s.len() - 1] == b'\'')
            || (bytes[0] == b'"' && bytes[s.len() - 1] == b'"'))
    {
        s[1..s.len() - 1].to_owned()
    } else {
        s.to_owned()
    }
}

fn strip_prefixes(s: &str) -> &str {
    let mut rest = s;
    while rest
        .chars()
        .next()
        .is_some_and(|c| matches!(c, 'r' | 'R' | 'u' | 'U' | 'b' | 'B' | 'f' | 'F'))
        && rest.len() > 1
    {
        rest = &rest[1..];
    }
    rest
}

fn number_literal(node: &Node<'_>, source: &str) -> Option<f64> {
    let text = text_of(node, source);
    let clean: String = text.chars().filter(|c| *c != '_').collect();
    clean.parse::<f64>().ok()
}

fn eval_identifier(node: &Node<'_>, source: &str) -> Value {
    let name = text_of(node, source);
    if let Some(n) = numeric_constant(&name) {
        return Value::Num(n);
    }
    if let Some(v) = direction(&name) {
        return Value::Vec2 { x: v.0, y: v.1 };
    }
    if let Some(c) = manim_color(&name) {
        return Value::Color(c);
    }
    Value::Ident(name)
}

fn numeric_constant(name: &str) -> Option<f64> {
    match name {
        "PI" => Some(std::f64::consts::PI),
        "TAU" => Some(std::f64::consts::TAU),
        "DEGREES" => Some(std::f64::consts::PI / 180.0),
        "E" => Some(std::f64::consts::E),
        _ => None,
    }
}

fn direction(name: &str) -> Option<(f64, f64)> {
    match name {
        "UP" => Some((0.0, 1.0)),
        "DOWN" => Some((0.0, -1.0)),
        "LEFT" => Some((-1.0, 0.0)),
        "RIGHT" => Some((1.0, 0.0)),
        // ORIGIN and the z-only directions all have no 2D effect.
        "ORIGIN" | "OUT" | "IN" => Some((0.0, 0.0)),
        "UL" => Some((-1.0, 1.0)),
        "UR" => Some((1.0, 1.0)),
        "DL" => Some((-1.0, -1.0)),
        "DR" => Some((1.0, -1.0)),
        _ => None,
    }
}

fn eval_sequence(node: &Node<'_>, source: &str, depth: usize) -> Value {
    let mut nums = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match eval_at_depth(&child, source, depth + 1) {
            Value::Num(n) => nums.push(n),
            _ => return Value::Unknown,
        }
    }
    match nums.as_slice() {
        [] => Value::Unknown,
        [x] => Value::Vec2 { x: *x, y: 0.0 },
        [x, y, ..] => Value::Vec2 { x: *x, y: *y },
    }
}

fn eval_unary(node: &Node<'_>, source: &str, depth: usize) -> Value {
    let op = operator_text(node, source);
    let argument = node.child_by_field_name("argument");
    let (Some(op), Some(argument)) = (op, argument) else {
        return Value::Unknown;
    };
    match (op.as_str(), eval_at_depth(&argument, source, depth + 1)) {
        ("-", Value::Num(n)) => Value::Num(-n),
        ("-", Value::Vec2 { x, y }) => Value::Vec2 { x: -x, y: -y },
        ("+", value) => value,
        _ => Value::Unknown,
    }
}

fn eval_binary(node: &Node<'_>, source: &str, depth: usize) -> Value {
    let op = operator_text(node, source);
    let left = node.child_by_field_name("left");
    let right = node.child_by_field_name("right");
    let (Some(op), Some(left), Some(right)) = (op, left, right) else {
        return Value::Unknown;
    };
    apply_binary(
        &op,
        &eval_at_depth(&left, source, depth + 1),
        &eval_at_depth(&right, source, depth + 1),
    )
}

fn apply_binary(op: &str, left: &Value, right: &Value) -> Value {
    use Value::{Num, Vec2};
    match (left, right) {
        (Num(a), Num(b)) => match op {
            "+" => Num(a + b),
            "-" => Num(a - b),
            "*" => Num(a * b),
            "/" => Num(a / b),
            "//" => Num((a / b).floor()),
            "%" => Num(a % b),
            "**" => Num(a.powf(*b)),
            _ => Value::Unknown,
        },
        (Vec2 { x, y }, Num(s)) => match op {
            "*" => Vec2 { x: x * s, y: y * s },
            "/" => Vec2 { x: x / s, y: y / s },
            _ => Value::Unknown,
        },
        (Num(s), Vec2 { x, y }) => match op {
            "*" => Vec2 { x: s * x, y: s * y },
            _ => Value::Unknown,
        },
        (Vec2 { x: ax, y: ay }, Vec2 { x: bx, y: by }) => match op {
            "+" => Vec2 {
                x: ax + bx,
                y: ay + by,
            },
            "-" => Vec2 {
                x: ax - bx,
                y: ay - by,
            },
            _ => Value::Unknown,
        },
        _ => Value::Unknown,
    }
}

fn eval_call(node: &Node<'_>, source: &str, depth: usize) -> Value {
    let Some(function) = node.child_by_field_name("function") else {
        return Value::Unknown;
    };
    if function.kind() == "attribute" {
        let object = function
            .child_by_field_name("object")
            .map_or_default(|o| text_of(&o, source));
        let attribute = function
            .child_by_field_name("attribute")
            .map_or_default(|a| text_of(&a, source));
        if (object == "np" || object == "numpy")
            && attribute == "array"
            && let Some(arguments) = node.child_by_field_name("arguments")
            && let Some(first) = arguments.named_child(0)
        {
            return eval_at_depth(&first, source, depth + 1);
        }
    }
    Value::Unknown
}

fn eval_attribute(node: &Node<'_>, source: &str) -> Value {
    let object = node
        .child_by_field_name("object")
        .map_or_default(|o| text_of(&o, source));
    let attribute = node
        .child_by_field_name("attribute")
        .map_or_default(|a| text_of(&a, source));
    match (object.as_str(), attribute.as_str()) {
        ("np" | "numpy", "pi") => Value::Num(std::f64::consts::PI),
        _ => Value::Unknown,
    }
}

fn operator_text(node: &Node<'_>, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if !child.is_named()
            && let Ok(text) = child.utf8_text(source.as_bytes())
        {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_owned());
            }
        }
    }
    None
}
