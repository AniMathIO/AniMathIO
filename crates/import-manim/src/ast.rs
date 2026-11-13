//! tree-sitter parse of a Manim scene into a small, owned intermediate
//! representation. Expression fragments are evaluated here via [`crate::eval`];
//! no tree-sitter types escape this module.

use crate::eval::{self, Value};
use crate::{Rgba, Warning};
use tree_sitter::Node;

/// The default Manim mobject colour.
const WHITE: Rgba = [255, 255, 255, 255];

/// A parsed scene: one class plus its ordered `construct` statements.
#[derive(Debug)]
pub(crate) struct Scene {
    /// Selected scene class name (exercised by the ported v1 parser tests).
    #[allow(dead_code)]
    pub name: String,
    pub statements: Vec<Stmt>,
}

/// One statement with its source location.
#[derive(Debug)]
pub(crate) struct Stmt {
    pub kind: StmtKind,
    pub line: usize,
    pub column: usize,
    pub snippet: String,
}

/// Interpreted statement kinds.
#[derive(Debug)]
pub(crate) enum StmtKind {
    /// `var = MobjectFactory(...)`, `var = VGroup(...)` or a chained inline
    /// constructor such as `var = Circle().scale(2)`.
    Declare { name: String, expr: MobjectExpr },
    /// `target.method(...)` (possibly through `.animate`, which is rejected here
    /// because `.animate` belongs inside `self.play`).
    Mutate {
        target: String,
        method: Method,
        animated: bool,
    },
    /// A method call on an inline (non-identifier) mobject expression, e.g.
    /// `VGroup(a, b).arrange(DOWN)` or `Circle().scale(2)`.
    MutateExpr { expr: MobjectExpr, animated: bool },
    /// `self.play(...)`.
    Play {
        animations: Vec<Anim>,
        run_time: Option<f64>,
        rate_func: Option<String>,
    },
    /// `self.wait(seconds)`.
    Wait { seconds: f64 },
    /// `self.add(a, b, ...)`.
    Add { targets: Vec<MobjectExpr> },
    /// `self.remove(a, b, ...)`.
    Remove { targets: Vec<MobjectExpr> },
    /// A construct the importer cannot interpret.
    Unsupported { message: String },
}

/// One animation inside `self.play(...)`.
#[derive(Debug, Clone)]
pub(crate) enum Anim {
    /// `Write(x)`, `FadeIn(x)`, etc.
    Simple {
        kind: AnimKind,
        target: MobjectExpr,
        run_time: Option<f64>,
    },
    /// `x.animate.shift(...)` and friends.
    Animate { target: String, method: Method },
    /// `Transform(a, b)` / `ReplacementTransform(a, b)`.
    Transform {
        source: MobjectExpr,
        target: MobjectExpr,
        run_time: Option<f64>,
    },
    /// An animation the importer cannot interpret.
    Unsupported { text: String },
}

/// Recognised Manim animation names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AnimKind {
    Write,
    Create,
    FadeIn,
    FadeOut,
    GrowFromCenter,
    Indicate,
    Transform,
    ReplacementTransform,
}

/// Recognised mobject methods (with or without `.animate`).
#[derive(Debug, Clone)]
pub(crate) enum Method {
    Shift {
        delta: (f64, f64),
    },
    MoveTo {
        point: (f64, f64),
    },
    NextTo {
        other: String,
        direction: (f64, f64),
    },
    ToEdge {
        direction: (f64, f64),
    },
    Scale {
        factor: f64,
    },
    Rotate {
        angle: f64,
    },
    Arrange {
        direction: (f64, f64),
    },
    Unsupported {
        name: String,
    },
}

/// A mobject constructor call.
#[derive(Debug, Clone)]
pub(crate) struct MobjectDecl {
    pub kind: DeclKind,
    /// Constructor-provided centre (`Dot(point=...)`), if any.
    pub center: Option<(f64, f64)>,
}

/// A mobject expression with its source position: a named reference, an inline
/// constructor, a `VGroup` or a chained method call on any of those.
#[derive(Debug, Clone)]
pub(crate) struct MobjectExpr {
    pub line: usize,
    pub column: usize,
    pub snippet: String,
    pub kind: MobjectExprKind,
}

/// The shape of a mobject expression.
#[derive(Debug, Clone)]
pub(crate) enum MobjectExprKind {
    /// `a` — a reference to an already-declared mobject.
    Ref(String),
    /// `Circle()` — an inline mobject constructor.
    Factory(MobjectDecl),
    /// `VGroup(a, b)` — a group of member expressions.
    Group(Vec<MobjectExpr>),
    /// `expr.method(...)`.
    Chain(Box<MobjectExpr>, Method),
    /// An expression that looks like a mobject but is not supported.
    Unsupported(String),
}

/// The parsed mobject constructor arguments.
#[derive(Debug, Clone)]
pub(crate) enum DeclKind {
    Text {
        text: String,
        font_size: f64,
        color: Rgba,
    },
    Tex {
        source: String,
        mode: String,
        size: f64,
        color: Rgba,
    },
    Circle {
        radius: f64,
        color: Rgba,
        fill: Option<Rgba>,
    },
    Square {
        side: f64,
        color: Rgba,
        fill: Option<Rgba>,
    },
    Rectangle {
        width: f64,
        height: f64,
        color: Rgba,
        fill: Option<Rgba>,
    },
    Line {
        start: (f64, f64),
        end: (f64, f64),
        color: Rgba,
    },
    Arrow {
        start: (f64, f64),
        end: (f64, f64),
        color: Rgba,
    },
    Dot {
        radius: f64,
        color: Rgba,
    },
    Polygon {
        vertices: Vec<(f64, f64)>,
        color: Rgba,
        fill: Option<Rgba>,
    },
    Image {
        path: String,
    },
}

/// Why a scene could not be selected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ParseError {
    NoScene,
    SceneNotFound(String),
}

/// Parse `source`, selecting the requested scene (or the first `Scene` subclass).
pub(crate) fn parse(
    source: &str,
    requested: Option<&str>,
) -> Result<(Scene, Vec<Warning>), ParseError> {
    let mut warnings = Vec::new();
    let mut parser = tree_sitter::Parser::new();
    let language: tree_sitter::Language = tree_sitter_python::LANGUAGE.into();
    if parser.set_language(&language).is_err() {
        return Err(ParseError::NoScene);
    }
    let Some(tree) = parser.parse(source, None) else {
        return Err(ParseError::NoScene);
    };
    let root = tree.root_node();
    collect_errors(&root, source, &mut warnings);

    let (class, name) = find_scene_class(&root, source, requested)?;
    let mut statements = Vec::new();
    if let Some(construct) = find_construct(&class, source)
        && let Some(body) = construct.child_by_field_name("body")
    {
        walk_block(&body, source, &mut statements, &mut warnings);
    }
    Ok((Scene { name, statements }, warnings))
}

fn collect_errors(node: &Node<'_>, source: &str, warnings: &mut Vec<Warning>) {
    let mut pending = vec![*node];
    while let Some(node) = pending.pop() {
        if node.is_error() || node.is_missing() {
            let pos = node.start_position();
            warnings.push(Warning {
                line: pos.row + 1,
                column: pos.column + 1,
                message: "parse error".to_owned(),
                snippet: snippet_at(&node, source),
            });
        } else {
            let mut cursor = node.walk();
            pending.extend(node.children(&mut cursor));
        }
    }
}

/// Bound the whole construct before any recursive parsing or evaluation begins.
fn excessive_depth<'tree>(node: &Node<'tree>) -> Option<Node<'tree>> {
    let mut pending = vec![(*node, 0)];
    while let Some((node, depth)) = pending.pop() {
        if depth > 64 {
            return Some(node);
        }
        let mut cursor = node.walk();
        pending.extend(
            node.named_children(&mut cursor)
                .map(|child| (child, depth + 1)),
        );
    }
    None
}

fn find_scene_class<'t>(
    root: &Node<'t>,
    source: &str,
    requested: Option<&str>,
) -> Result<(Node<'t>, String), ParseError> {
    let mut fallback: Option<(Node<'_>, String)> = None;
    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        if child.kind() != "class_definition" || !is_scene_class(&child, source) {
            continue;
        }
        let name = child
            .child_by_field_name("name")
            .map_or_default(|n| eval::text_of(&n, source));
        if let Some(requested) = requested {
            if name == requested {
                return Ok((child, name));
            }
        } else if fallback.is_none() {
            fallback = Some((child, name));
        }
    }
    if requested.is_some() {
        return Err(ParseError::SceneNotFound(
            requested.unwrap_or_default().to_owned(),
        ));
    }
    fallback.ok_or(ParseError::NoScene)
}

fn is_scene_class(class: &Node<'_>, source: &str) -> bool {
    let Some(superclasses) = class.child_by_field_name("superclasses") else {
        return false;
    };
    let mut cursor = superclasses.walk();
    for base in superclasses.named_children(&mut cursor) {
        let name = match base.kind() {
            "identifier" => eval::text_of(&base, source),
            "attribute" => base
                .child_by_field_name("attribute")
                .map_or_default(|a| eval::text_of(&a, source)),
            _ => continue,
        };
        if name == "Scene" || name.ends_with("Scene") {
            return true;
        }
    }
    false
}

fn find_construct<'t>(class: &Node<'t>, source: &str) -> Option<Node<'t>> {
    let body = class.child_by_field_name("body")?;
    let mut cursor = body.walk();
    for stmt in body.named_children(&mut cursor) {
        if stmt.kind() == "function_definition" {
            let name = stmt
                .child_by_field_name("name")
                .map_or_default(|n| eval::text_of(&n, source));
            if name == "construct" {
                return Some(stmt);
            }
        }
    }
    None
}

fn walk_block(block: &Node<'_>, source: &str, out: &mut Vec<Stmt>, warnings: &mut Vec<Warning>) {
    let mut cursor = block.walk();
    for statement in block.named_children(&mut cursor) {
        if let Some(deep) = excessive_depth(&statement) {
            let pos = deep.start_position();
            warnings.push(Warning {
                line: pos.row + 1,
                column: pos.column + 1,
                message: "construct exceeds maximum AST depth of 64; skipped".to_owned(),
                snippet: snippet_at(&statement, source),
            });
            continue;
        }
        if let Some(stmt) = parse_statement(&statement, source) {
            out.push(stmt);
        }
    }
}

fn parse_statement(node: &Node<'_>, source: &str) -> Option<Stmt> {
    let (line, column, snippet) = position(node, source);
    // tree-sitter-python wraps simple statements in an `expression_statement`;
    // unwrap to the statement proper before dispatching.
    let mut inner = *node;
    while inner.kind() == "expression_statement" {
        inner = inner.named_child(0)?;
    }
    let kind = match inner.kind() {
        "call" => parse_call_statement(&inner, source)?,
        "assignment" => parse_assignment(&inner, source),
        "augmented_assignment" => StmtKind::Unsupported {
            message: "unsupported construct: augmented_assignment".to_owned(),
        },
        // Harmless and ubiquitous.
        "pass_statement"
        | "return_statement"
        | "import_statement"
        | "import_from_statement"
        | "future_import_statement" => return None,
        other => StmtKind::Unsupported {
            message: format!("unsupported construct: {other}"),
        },
    };
    Some(Stmt {
        kind,
        line,
        column,
        snippet,
    })
}

fn parse_assignment(node: &Node<'_>, source: &str) -> StmtKind {
    let (Some(left), Some(right)) = (
        node.child_by_field_name("left"),
        node.child_by_field_name("right"),
    ) else {
        return StmtKind::Unsupported {
            message: "unsupported assignment target".to_owned(),
        };
    };
    if left.kind() != "identifier" {
        return StmtKind::Unsupported {
            message: "unsupported assignment target".to_owned(),
        };
    }
    let name = eval::text_of(&left, source);
    // `target.method(...)` on an identifier mutates that named mobject in place.
    if let Some((target, method, animated)) = parse_method_chain(&right, source) {
        return StmtKind::Mutate {
            target,
            method,
            animated,
        };
    }
    // A factory, VGroup or chained inline constructor becomes a declaration.
    // Bare identifier aliases (`x = y`) stay unsupported.
    if let Some(expr) = parse_mobject_expr(&right, source)
        && !matches!(&expr.kind, MobjectExprKind::Ref(_))
    {
        return StmtKind::Declare { name, expr };
    }
    StmtKind::Unsupported {
        message: format!(
            "unsupported assignment: {name} = {}",
            eval::text_of(&right, source)
        ),
    }
}

#[allow(clippy::too_many_lines)] // One arm per supported mobject factory keeps the mapping together.
fn parse_factory(call: &Node<'_>, _name: &str, source: &str) -> Option<MobjectDecl> {
    let function = call.child_by_field_name("function")?;
    if function.kind() != "identifier" {
        return None;
    }
    let factory = eval::text_of(&function, source);
    let (positional, kwargs) = call_arguments(call, source);
    let color = kwarg(&kwargs, "color").and_then(eval::as_color);
    let fill = kwarg(&kwargs, "fill_color").and_then(eval::as_color);
    let center = kwarg(&kwargs, "point").and_then(eval::as_vector);

    let kind = match factory.as_str() {
        "Text" => DeclKind::Text {
            text: positional
                .first()
                .and_then(eval::as_string)
                .unwrap_or_default()
                .to_owned(),
            font_size: kwarg(&kwargs, "font_size")
                .and_then(eval::as_number)
                .unwrap_or(48.0),
            color: color.unwrap_or(WHITE),
        },
        "Tex" | "MathTex" => DeclKind::Tex {
            source: positional
                .first()
                .and_then(eval::as_string)
                .unwrap_or_default()
                .to_owned(),
            mode: if factory == "Tex" { "tex" } else { "mathtex" }.to_owned(),
            size: kwarg(&kwargs, "font_size")
                .or_else(|| kwarg(&kwargs, "size"))
                .and_then(eval::as_number)
                .unwrap_or(48.0),
            color: color.unwrap_or(WHITE),
        },
        "Circle" => DeclKind::Circle {
            radius: kwarg(&kwargs, "radius")
                .and_then(eval::as_number)
                .or_else(|| positional.first().and_then(eval::as_number))
                .unwrap_or(1.0),
            color: color.unwrap_or(WHITE),
            fill,
        },
        "Square" => DeclKind::Square {
            side: kwarg(&kwargs, "side_length")
                .and_then(eval::as_number)
                .or_else(|| positional.first().and_then(eval::as_number))
                .unwrap_or(2.0),
            color: color.unwrap_or(WHITE),
            fill,
        },
        "Rectangle" => DeclKind::Rectangle {
            width: kwarg(&kwargs, "width")
                .and_then(eval::as_number)
                .or_else(|| positional.first().and_then(eval::as_number))
                .unwrap_or(4.0),
            height: kwarg(&kwargs, "height")
                .and_then(eval::as_number)
                .or_else(|| positional.get(1).and_then(eval::as_number))
                .unwrap_or(2.0),
            color: color.unwrap_or(WHITE),
            fill,
        },
        "Line" => DeclKind::Line {
            start: positional
                .first()
                .and_then(eval::as_vector)
                .unwrap_or((-1.0, 0.0)),
            end: positional
                .get(1)
                .and_then(eval::as_vector)
                .unwrap_or((1.0, 0.0)),
            color: color.unwrap_or(WHITE),
        },
        "Arrow" => DeclKind::Arrow {
            start: positional
                .first()
                .and_then(eval::as_vector)
                .unwrap_or((-1.0, 0.0)),
            end: positional
                .get(1)
                .and_then(eval::as_vector)
                .unwrap_or((1.0, 0.0)),
            color: color.unwrap_or(WHITE),
        },
        "Dot" => DeclKind::Dot {
            radius: kwarg(&kwargs, "radius")
                .and_then(eval::as_number)
                .unwrap_or(0.08),
            color: color.unwrap_or(WHITE),
        },
        "Polygon" => DeclKind::Polygon {
            vertices: polygon_vertices(call, source),
            color: color.unwrap_or(WHITE),
            fill,
        },
        "ImageMobject" => DeclKind::Image {
            path: positional
                .first()
                .and_then(eval::as_string)
                .unwrap_or_default()
                .to_owned(),
        },
        _ => return None,
    };
    Some(MobjectDecl { kind, center })
}

fn polygon_vertices(call: &Node<'_>, source: &str) -> Vec<(f64, f64)> {
    let Some(arguments) = call.child_by_field_name("arguments") else {
        return Vec::new();
    };
    let Some(first) = arguments.named_child(0) else {
        return Vec::new();
    };
    let mut vertices = Vec::new();
    if first.kind() == "list" || first.kind() == "tuple" {
        let mut cursor = first.walk();
        for item in first.named_children(&mut cursor) {
            if let Some(point) = vector_of_node(&item, source) {
                vertices.push(point);
            }
        }
    }
    vertices
}

fn vector_of_node(node: &Node<'_>, source: &str) -> Option<(f64, f64)> {
    if node.kind() == "list" || node.kind() == "tuple" {
        let mut nums = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match eval::eval(&child, source) {
                Value::Num(n) => nums.push(n),
                _ => return None,
            }
        }
        return match nums.as_slice() {
            [] => None,
            [x] => Some((*x, 0.0)),
            [x, y, ..] => Some((*x, *y)),
        };
    }
    eval::as_vector(&eval::eval(node, source))
}

fn parse_call_statement(call: &Node<'_>, source: &str) -> Option<StmtKind> {
    let function = call.child_by_field_name("function")?;
    if function.kind() == "attribute" {
        let object = function.child_by_field_name("object")?;
        let attribute = function
            .child_by_field_name("attribute")
            .map_or_default(|a| eval::text_of(&a, source));
        if object.kind() == "identifier" && eval::text_of(&object, source) == "self" {
            return match attribute.as_str() {
                "play" => parse_play(call, source).map(|(animations, run_time, rate_func)| {
                    StmtKind::Play {
                        animations,
                        run_time,
                        rate_func,
                    }
                }),
                "wait" => Some(StmtKind::Wait {
                    seconds: parse_wait(call, source),
                }),
                "add" => Some(StmtKind::Add {
                    targets: parse_targets(call, source),
                }),
                "remove" => Some(StmtKind::Remove {
                    targets: parse_targets(call, source),
                }),
                other => Some(StmtKind::Unsupported {
                    message: format!("unsupported self method: self.{other}"),
                }),
            };
        }
        if let Some((target, method, animated)) = parse_method_chain(call, source) {
            return Some(StmtKind::Mutate {
                target,
                method,
                animated,
            });
        }
        if let Some((expr, animated)) = parse_chain_expr(call, source) {
            return Some(StmtKind::MutateExpr { expr, animated });
        }
    }
    Some(StmtKind::Unsupported {
        message: format!("unsupported call: {}", eval::text_of(call, source)),
    })
}

fn parse_play(call: &Node<'_>, source: &str) -> Option<(Vec<Anim>, Option<f64>, Option<String>)> {
    let arguments = call.child_by_field_name("arguments")?;
    let mut animations = Vec::new();
    let mut run_time = None;
    let mut rate_func = None;
    let mut cursor = arguments.walk();
    for arg in arguments.named_children(&mut cursor) {
        match arg.kind() {
            "keyword_argument" => {
                let name = arg
                    .child_by_field_name("name")
                    .map_or_default(|n| eval::text_of(&n, source));
                let value = arg
                    .child_by_field_name("value")
                    .map(|v| eval::eval(&v, source));
                match name.as_str() {
                    "run_time" => run_time = value.as_ref().and_then(eval::as_number),
                    "rate_func" => {
                        rate_func = value.as_ref().and_then(eval::as_ident).map(str::to_owned);
                    }
                    _ => {}
                }
            }
            _ => {
                if let Some(anim) = parse_anim(&arg, source) {
                    animations.push(anim);
                }
            }
        }
    }
    Some((animations, run_time, rate_func))
}

fn parse_anim(node: &Node<'_>, source: &str) -> Option<Anim> {
    if node.kind() != "call" {
        return Some(Anim::Unsupported {
            text: eval::text_of(node, source),
        });
    }
    let function = node.child_by_field_name("function")?;
    if function.kind() == "identifier" {
        let name = eval::text_of(&function, source);
        let (_, kwargs) = call_arguments(node, source);
        let run_time = kwarg(&kwargs, "run_time").and_then(eval::as_number);
        let kind = match name.as_str() {
            "Write" => AnimKind::Write,
            "Create" => AnimKind::Create,
            "FadeIn" => AnimKind::FadeIn,
            "FadeOut" => AnimKind::FadeOut,
            "GrowFromCenter" => AnimKind::GrowFromCenter,
            "Indicate" => AnimKind::Indicate,
            "Transform" => AnimKind::Transform,
            "ReplacementTransform" => AnimKind::ReplacementTransform,
            _ => {
                return Some(Anim::Unsupported {
                    text: eval::text_of(node, source),
                });
            }
        };
        let args = positional_nodes(node, source);
        if matches!(kind, AnimKind::Transform | AnimKind::ReplacementTransform) {
            let (Some(source_node), Some(target_node)) = (args.first(), args.get(1)) else {
                return Some(Anim::Unsupported {
                    text: eval::text_of(node, source),
                });
            };
            return Some(Anim::Transform {
                source: parse_mobject_target(source_node, source),
                target: parse_mobject_target(target_node, source),
                run_time,
            });
        }
        let Some(target_node) = args.first() else {
            return Some(Anim::Unsupported {
                text: eval::text_of(node, source),
            });
        };
        return Some(Anim::Simple {
            kind,
            target: parse_mobject_target(target_node, source),
            run_time,
        });
    }
    if function.kind() == "attribute"
        && let Some((target, method, animated)) = parse_method_chain(node, source)
        && animated
    {
        return Some(Anim::Animate { target, method });
    }
    Some(Anim::Unsupported {
        text: eval::text_of(node, source),
    })
}

/// Resolve `target.method(args)` or `target.animate.method(args)`.
fn parse_method_chain(call: &Node<'_>, source: &str) -> Option<(String, Method, bool)> {
    let function = call.child_by_field_name("function")?;
    if function.kind() != "attribute" {
        return None;
    }
    let method_name = function
        .child_by_field_name("attribute")
        .map_or_default(|a| eval::text_of(&a, source));
    let mut animated = false;
    let mut object = function.child_by_field_name("object")?;
    while object.kind() == "attribute" {
        let object_attr = object
            .child_by_field_name("attribute")
            .map_or_default(|a| eval::text_of(&a, source));
        if object_attr == "animate" {
            animated = true;
            object = object.child_by_field_name("object")?;
        } else {
            return None;
        }
    }
    if object.kind() != "identifier" {
        return None;
    }
    let target = eval::text_of(&object, source);
    let (positional, _kwargs) = call_arguments(call, source);
    let method = build_method(&method_name, &positional);
    Some((target, method, animated))
}

fn build_method(name: &str, positional: &[Value]) -> Method {
    let vector = positional.first().and_then(eval::as_vector);
    match name {
        "shift" => Method::Shift {
            delta: vector.unwrap_or((0.0, 0.0)),
        },
        "move_to" => Method::MoveTo {
            point: vector.unwrap_or((0.0, 0.0)),
        },
        "next_to" => Method::NextTo {
            other: positional
                .first()
                .and_then(eval::as_ident)
                .unwrap_or_default()
                .to_owned(),
            direction: positional
                .get(1)
                .and_then(eval::as_vector)
                .unwrap_or((0.0, 0.0)),
        },
        "to_edge" => Method::ToEdge {
            direction: vector.unwrap_or((0.0, 0.0)),
        },
        "scale" => Method::Scale {
            factor: positional.first().and_then(eval::as_number).unwrap_or(1.0),
        },
        "rotate" => Method::Rotate {
            angle: positional.first().and_then(eval::as_number).unwrap_or(0.0),
        },
        "arrange" => Method::Arrange {
            direction: vector.unwrap_or((0.0, 0.0)),
        },
        other => Method::Unsupported {
            name: other.to_owned(),
        },
    }
}

fn parse_wait(call: &Node<'_>, source: &str) -> f64 {
    let (positional, _) = call_arguments(call, source);
    positional.first().and_then(eval::as_number).unwrap_or(1.0)
}

/// Parse a mobject expression: a reference, inline constructor, `VGroup` or a
/// chained method call on any of those. Returns `None` when `node` is not a
/// mobject-shaped expression at all.
fn parse_mobject_expr(node: &Node<'_>, source: &str) -> Option<MobjectExpr> {
    let (line, column, snippet) = position(node, source);
    let kind = match node.kind() {
        "identifier" => MobjectExprKind::Ref(eval::text_of(node, source)),
        "call" => {
            let function = node.child_by_field_name("function")?;
            if function.kind() == "identifier" {
                let name = eval::text_of(&function, source);
                if name == "VGroup" {
                    MobjectExprKind::Group(parse_group_members(node, source))
                } else {
                    let decl = parse_factory(node, "", source)?;
                    MobjectExprKind::Factory(decl)
                }
            } else if function.kind() == "attribute" {
                let method_name = function
                    .child_by_field_name("attribute")
                    .map_or_default(|a| eval::text_of(&a, source));
                let object = function.child_by_field_name("object")?;
                let base = parse_mobject_expr(&object, source)?;
                let (positional, _kwargs) = call_arguments(node, source);
                let method = build_method(&method_name, &positional);
                MobjectExprKind::Chain(Box::new(base), method)
            } else {
                return None;
            }
        }
        _ => return None,
    };
    Some(MobjectExpr {
        line,
        column,
        snippet,
        kind,
    })
}

/// Parse a mobject argument, preserving unsupported-looking expressions so the
/// importer can warn about them instead of silently dropping them.
fn parse_mobject_target(node: &Node<'_>, source: &str) -> MobjectExpr {
    let (line, column, snippet) = position(node, source);
    match parse_mobject_expr(node, source) {
        Some(expr) => expr,
        None => MobjectExpr {
            line,
            column,
            snippet,
            kind: MobjectExprKind::Unsupported(eval::text_of(node, source)),
        },
    }
}

fn parse_group_members(call: &Node<'_>, source: &str) -> Vec<MobjectExpr> {
    positional_nodes(call, source)
        .iter()
        .map(|node| parse_mobject_target(node, source))
        .collect()
}

/// Resolve a chained method call whose base is not a plain identifier (e.g.
/// `VGroup(a, b).arrange(DOWN)`). Identifier bases are handled by
/// [`parse_method_chain`].
fn parse_chain_expr(call: &Node<'_>, source: &str) -> Option<(MobjectExpr, bool)> {
    let function = call.child_by_field_name("function")?;
    if function.kind() != "attribute" {
        return None;
    }
    let method_name = function
        .child_by_field_name("attribute")
        .map_or_default(|a| eval::text_of(&a, source));
    let mut object = function.child_by_field_name("object")?;
    let mut animated = false;
    while object.kind() == "attribute" {
        let object_attr = object
            .child_by_field_name("attribute")
            .map_or_default(|a| eval::text_of(&a, source));
        if object_attr == "animate" {
            animated = true;
            object = object.child_by_field_name("object")?;
        } else {
            return None;
        }
    }
    let base = parse_mobject_expr(&object, source)?;
    if matches!(&base.kind, MobjectExprKind::Ref(_)) {
        return None;
    }
    let (positional, _kwargs) = call_arguments(call, source);
    let method = build_method(&method_name, &positional);
    let (line, column, snippet) = position(call, source);
    Some((
        MobjectExpr {
            line,
            column,
            snippet,
            kind: MobjectExprKind::Chain(Box::new(base), method),
        },
        animated,
    ))
}

/// Raw non-keyword argument nodes (values, not evaluated).
fn positional_nodes<'t>(call: &Node<'t>, _source: &str) -> Vec<Node<'t>> {
    let mut nodes = Vec::new();
    let Some(arguments) = call.child_by_field_name("arguments") else {
        return nodes;
    };
    let mut cursor = arguments.walk();
    for arg in arguments.named_children(&mut cursor) {
        if arg.kind() == "keyword_argument" {
            continue;
        }
        nodes.push(arg);
    }
    nodes
}

fn parse_targets(call: &Node<'_>, source: &str) -> Vec<MobjectExpr> {
    positional_nodes(call, source)
        .iter()
        .map(|node| parse_mobject_target(node, source))
        .collect()
}

fn call_arguments(call: &Node<'_>, source: &str) -> (Vec<Value>, Vec<(String, Value)>) {
    let mut positional = Vec::new();
    let mut kwargs = Vec::new();
    let Some(arguments) = call.child_by_field_name("arguments") else {
        return (positional, kwargs);
    };
    let mut cursor = arguments.walk();
    for arg in arguments.named_children(&mut cursor) {
        match arg.kind() {
            "keyword_argument" => {
                let name = arg
                    .child_by_field_name("name")
                    .map_or_default(|n| eval::text_of(&n, source));
                let value = arg
                    .child_by_field_name("value")
                    .map_or(Value::Unknown, |v| eval::eval(&v, source));
                kwargs.push((name, value));
            }
            _ => positional.push(eval::eval(&arg, source)),
        }
    }
    (positional, kwargs)
}

fn kwarg<'a>(kwargs: &'a [(String, Value)], name: &str) -> Option<&'a Value> {
    kwargs.iter().find(|(k, _)| k == name).map(|(_, v)| v)
}

fn position(node: &Node<'_>, source: &str) -> (usize, usize, String) {
    let pos = node.start_position();
    let snippet = source.lines().nth(pos.row).map_or_default(str::trim);
    (pos.row + 1, pos.column + 1, truncate(snippet, 80))
}

fn snippet_at(node: &Node<'_>, source: &str) -> String {
    let pos = node.start_position();
    let snippet = source.lines().nth(pos.row).map_or_default(str::trim);
    truncate(snippet, 80)
}

fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        text.to_owned()
    } else {
        format!("{text}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn parse_ok(source: &str) -> Scene {
        match parse(source, None) {
            Ok((scene, warnings)) => {
                assert!(
                    warnings.is_empty(),
                    "unexpected parse warnings: {warnings:?}"
                );
                scene
            }
            Err(error) => panic!("unexpected parse error: {error:?}"),
        }
    }

    fn plays(scene: &Scene) -> Vec<(Vec<Anim>, Option<f64>, Option<String>)> {
        scene
            .statements
            .iter()
            .filter_map(|s| match &s.kind {
                StmtKind::Play {
                    animations,
                    run_time,
                    rate_func,
                } => Some((animations.clone(), *run_time, rate_func.clone())),
                _ => None,
            })
            .collect()
    }

    fn waits(scene: &Scene) -> Vec<f64> {
        scene
            .statements
            .iter()
            .filter_map(|s| match &s.kind {
                StmtKind::Wait { seconds } => Some(*seconds),
                _ => None,
            })
            .collect()
    }

    fn declares(scene: &Scene) -> BTreeMap<&str, &MobjectDecl> {
        scene
            .statements
            .iter()
            .filter_map(|s| match &s.kind {
                StmtKind::Declare { name, expr } => match &expr.kind {
                    MobjectExprKind::Factory(decl) => Some((name.as_str(), decl)),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    #[test]
    fn extracts_the_class_name() {
        let scene = parse_ok(
            "from manim import *\n\nclass HelloScene(Scene):\n    def construct(self):\n        pass\n",
        );
        assert_eq!(scene.name, "HelloScene");
    }

    #[test]
    fn parses_text_math_tex_and_colours() {
        let scene = parse_ok(
            r#"
from manim import *

class Scene1(Scene):
    def construct(self):
        title = Text("Hello World", font_size=48, color=WHITE)
        formula = MathTex(r"E = mc^2")
"#,
        );
        let decls = declares(&scene);
        let DeclKind::Text {
            text,
            font_size,
            color,
        } = &decls["title"].kind
        else {
            panic!("expected Text");
        };
        assert_eq!(text, "Hello World");
        assert_eq!(*font_size, 48.0);
        assert_eq!(*color, [255, 255, 255, 255]);

        let DeclKind::Tex { source, mode, .. } = &decls["formula"].kind else {
            panic!("expected Tex");
        };
        assert_eq!(source, "E = mc^2");
        assert_eq!(mode, "mathtex");
    }

    #[test]
    fn strips_raw_string_prefixes() {
        let scene = parse_ok(
            r#"
class RawScene(Scene):
    def construct(self):
        greeting = Text(r"Raw text")
        proof = Tex(r"Q.E.D.")
"#,
        );
        let decls = declares(&scene);
        let DeclKind::Text { text, .. } = &decls["greeting"].kind else {
            panic!("expected Text");
        };
        assert_eq!(text, "Raw text");
        let DeclKind::Tex { source, .. } = &decls["proof"].kind else {
            panic!("expected Tex");
        };
        assert_eq!(source, "Q.E.D.");
    }

    #[test]
    fn parses_play_and_wait_statements() {
        let scene = parse_ok(
            r#"
class S(Scene):
    def construct(self):
        a = Text("a")
        b = MathTex(r"b")
        self.play(Write(a))
        self.wait(1)
        self.play(FadeIn(b))
        self.play(FadeOut(a))
        self.wait(2)
"#,
        );
        assert_eq!(plays(&scene).len(), 3);
        assert_eq!(waits(&scene), vec![1.0, 2.0]);
        let first = &plays(&scene)[0].0[0];
        match first {
            Anim::Simple { kind, target, .. } => {
                assert_eq!(*kind, AnimKind::Write);
                assert!(matches!(
                    &target.kind,
                    MobjectExprKind::Ref(name) if name == "a"
                ));
            }
            other => panic!("expected simple animation, got {other:?}"),
        }
    }

    #[test]
    fn parses_circle_rectangle_and_multiple_animations() {
        let scene = parse_ok(
            r#"
class S(Scene):
    def construct(self):
        c = Circle(radius=2, color=BLUE)
        r = Rectangle(width=4, height=2, color=RED)
        img = ImageMobject("photo.png")
        self.play(Create(c), Create(r))
"#,
        );
        let decls = declares(&scene);
        let DeclKind::Circle { radius, color, .. } = &decls["c"].kind else {
            panic!("expected Circle");
        };
        assert_eq!(*radius, 2.0);
        assert_eq!(*color, [88, 196, 221, 255]); // Manim BLUE

        let DeclKind::Rectangle { width, height, .. } = &decls["r"].kind else {
            panic!("expected Rectangle");
        };
        assert_eq!(*width, 4.0);
        assert_eq!(*height, 2.0);

        let DeclKind::Image { path } = &decls["img"].kind else {
            panic!("expected ImageMobject");
        };
        assert_eq!(path, "photo.png");

        assert_eq!(plays(&scene)[0].0.len(), 2);
    }

    #[test]
    fn parses_animate_chains_and_positioning() {
        let scene = parse_ok(
            r"
class S(Scene):
    def construct(self):
        sq = Square(side_length=2)
        sq.shift(UP * 2)
        self.play(sq.animate.move_to(RIGHT * 2), run_time=2, rate_func=smooth)
",
        );
        let mut shifted = false;
        for stmt in &scene.statements {
            if let StmtKind::Mutate {
                target,
                method: Method::Shift { delta },
                ..
            } = &stmt.kind
            {
                assert_eq!(target, "sq");
                assert_eq!(*delta, (0.0, 2.0));
                shifted = true;
            }
        }
        assert!(shifted, "expected a shift mutation");

        let play = &plays(&scene)[0];
        assert_eq!(play.1, Some(2.0));
        assert_eq!(play.2.as_deref(), Some("smooth"));
        match &play.0[0] {
            Anim::Animate {
                target,
                method: Method::MoveTo { point },
            } => {
                assert_eq!(target, "sq");
                assert_eq!(*point, (2.0, 0.0));
            }
            other => panic!("expected animate move_to, got {other:?}"),
        }
    }

    #[test]
    fn reports_unsupported_constructs_as_statements_not_errors() {
        let (scene, warnings) = parse(
            r"
class S(Scene):
    def construct(self):
        for i in range(3):
            self.wait(1)
        self.play(ApplyMethod(x.shift, UP))
",
            None,
        )
        .unwrap();
        assert!(warnings.is_empty(), "parse errors are not expected here");
        let unsupported: Vec<&str> = scene
            .statements
            .iter()
            .filter_map(|s| match &s.kind {
                StmtKind::Unsupported { message } => Some(message.as_str()),
                _ => None,
            })
            .collect();
        assert!(unsupported.iter().any(|m| m.contains("for_statement")));
        let play = &plays(&scene)[0];
        assert!(matches!(&play.0[0], Anim::Unsupported { .. }));
    }
}

#[cfg(test)]
mod depth_tests {
    #[test]
    fn deeply_nested_constructs_are_skipped() {
        for expression in [
            format!("{}1", "-".repeat(100_000)),
            format!("{}1{}", "(".repeat(10_000), ")".repeat(10_000)),
        ] {
            let source = format!(
                "class S(Scene):\n    def construct(self):\n        c = Circle(radius={expression})\n        self.wait(1)\n"
            );
            let (scene, warnings) = super::parse(&source, None).unwrap();
            assert!(
                warnings
                    .iter()
                    .any(|w| w.message.contains("depth") && w.line == 3 && w.column > 0)
            );
            assert_eq!(scene.statements.len(), 1);
        }
    }
}
