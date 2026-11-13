//! Interpretation of a parsed Manim scene into a validated v2 [`Project`],
//! rebuilt exclusively through [`Command`]s. Positions are mapped from the
//! Manim frame (14.22 × 8 units, origin at the canvas centre) to normalized
//! coordinates; the timeline is rebuilt sequentially from `self.play`/`self.wait`.

use crate::ast::{
    self, Anim, AnimKind, DeclKind, Method, MobjectDecl, MobjectExpr, MobjectExprKind, Stmt,
    StmtKind,
};
use crate::{Asset, ImportError, ManimOptions, Report, Rgba};
use animathio_core::builtin::{Builtin, PathElement, Shape, Stroke};
use animathio_core::{
    Asset as CoreAsset, AssetId, Clip, ClipId, ClipKind, Command, FrameRate, Interpolation,
    Keyframe, Project, TICKS_PER_SECOND, Target, Time, TimeRange, Track, TrackId, Transform,
    TransformProperty,
};
use std::collections::BTreeMap;
use std::path::{Component, Path};

/// Manim frame width in scene units.
const FRAME_WIDTH: f64 = 14.22;
/// Manim frame height in scene units.
const FRAME_HEIGHT: f64 = 8.0;
/// Output canvas resolution, as in Manim's default config.
const CANVAS_WIDTH: u32 = 1920;
/// Output canvas height.
const CANVAS_HEIGHT: u32 = 1080;
/// Pixels per Manim scene unit (1920 / 14.22 ≈ 1080 / 8).
const PX_PER_UNIT: f64 = 135.0;
/// Default output frame rate.
const DEFAULT_FPS: u32 = 30;
/// Default animation run time in seconds.
const DEFAULT_RUN_TIME: f64 = 1.0;
/// Manim's default `next_to` buffer in scene units.
const NEXT_TO_BUFF: f64 = 0.25;
/// Manim's default `to_edge` buffer in scene units.
const TO_EDGE_BUFF: f64 = 0.5;
/// Nominal on-screen stroke width in pixels.
const STROKE_PX: f64 = 4.0;
/// Nominal size of an unresolved image, in scene units.
const IMAGE_SIZE: (f64, f64) = (6.0, 4.0);
/// Reverse-domain plugin id for the `latex` plugin (Tex/MathTex clips).
const LATEX_PLUGIN: &str = "org.animathio.latex";
/// Maximum size of a single embedded image, in bytes.
const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
/// Image file extensions accepted for embedding.
const ALLOWED_IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"];
/// One successfully resolved image: `(name, media type, bytes)`.
type ResolvedImage = (String, String, Vec<u8>);
/// Resolution result: either a validated image or a rejection reason.
type ImageResolution = Result<ResolvedImage, String>;

/// Interpreted, mutable state for one mobject.
#[derive(Clone, Debug)]
struct Mobject {
    kind: Kind,
    center: (f64, f64),
    scale: f64,
    /// Rotation in radians, Manim convention (counter-clockwise, y-up).
    rotation: f64,
    /// Natural extent in scene units, used for `next_to`/`to_edge` layout.
    size: (f64, f64),
    /// Base normalized scale: shapes use `size / frame`, text/latex/images use 1.
    normalized_scale: (f64, f64),
    /// Base opacity.
    opacity: f64,
}

/// Resolved clip source for one mobject.
#[derive(Clone, Debug)]
enum Kind {
    Text {
        text: String,
        font_size_px: f64,
        color: Rgba,
    },
    Tex {
        source: String,
        mode: String,
        size_px: f64,
        color: Rgba,
    },
    Shape {
        geom: ShapeGeom,
        stroke: Option<Rgba>,
        fill: Option<Rgba>,
    },
    Image {
        path: String,
    },
}

/// Shape geometry in local scene units (centred, y-up).
#[derive(Clone, Debug)]
enum ShapeGeom {
    Rect,
    Ellipse,
    Path { verbs: Vec<LocalVerb> },
}

/// Path verb in local, centred scene units.
#[derive(Clone, Debug)]
enum LocalVerb {
    Move((f64, f64)),
    Line((f64, f64)),
    Close,
}

/// One pending keyframe, in absolute scene seconds.
#[derive(Clone, Debug)]
struct KeyEvent {
    clip: String,
    target: Target,
    time: f64,
    value: f64,
    interp: Interpolation,
}

/// Sequential interpretation state.
#[derive(Default)]
struct State {
    mobjects: BTreeMap<String, Mobject>,
    groups: BTreeMap<String, Vec<String>>,
    order: Vec<String>,
    decl_pos: BTreeMap<String, (usize, usize, String)>,
    first_appear: BTreeMap<String, f64>,
    last_end: BTreeMap<String, f64>,
    events: Vec<KeyEvent>,
    cursor: f64,
}

pub(crate) fn translate(
    source: &str,
    options: &ManimOptions,
) -> Result<(Project, Vec<Asset>, Report), ImportError> {
    let mut report = Report::default();
    let (scene, parse_warnings) =
        ast::parse(source, options.scene.as_deref()).map_err(|e| match e {
            ast::ParseError::NoScene => ImportError::NoScene,
            ast::ParseError::SceneNotFound(name) => ImportError::SceneNotFound(name),
        })?;
    report.warnings.extend(parse_warnings);

    let mut state = State::default();

    // Hoist mobject declarations so the timeline pass can reference them in any
    // statement order; declarations do not advance the cursor. Group members are
    // resolved lazily, so `VGroup(a, b)` may reference members declared later.
    for stmt in &scene.statements {
        if let StmtKind::Declare { name, expr } = &stmt.kind {
            if state.mobjects.contains_key(name) || state.groups.contains_key(name) {
                report.warn(
                    stmt.line,
                    stmt.column,
                    format!("mobject `{name}` is declared more than once; the later value wins"),
                    stmt.snippet.clone(),
                );
            }
            define_expr(expr, name, &mut state, &mut report);
            state
                .decl_pos
                .insert(name.clone(), (stmt.line, stmt.column, stmt.snippet.clone()));
        }
    }

    for stmt in &scene.statements {
        interpret(stmt, &mut state, &mut report);
    }

    let scene_end = state.cursor.max(1.0);
    build_project(scene_end, &state, options, report)
}

#[allow(clippy::too_many_lines)] // One arm per statement kind keeps the dispatcher together.
fn interpret(stmt: &Stmt, state: &mut State, report: &mut Report) {
    match &stmt.kind {
        StmtKind::Declare { .. } => {}
        StmtKind::Unsupported { message } => {
            report.warn(
                stmt.line,
                stmt.column,
                message.clone(),
                stmt.snippet.clone(),
            );
        }
        StmtKind::Wait { seconds } => {
            state.cursor += seconds.max(0.0);
        }
        StmtKind::Add { targets } => {
            for target in targets {
                let Some(name) = resolve_expr(target, state, report) else {
                    continue;
                };
                for member in target_members(&name, state) {
                    if state.mobjects.contains_key(&member) {
                        state.first_appear.entry(member).or_insert(state.cursor);
                    } else {
                        report.warn(
                            stmt.line,
                            stmt.column,
                            format!("self.add of unknown mobject `{member}`"),
                            stmt.snippet.clone(),
                        );
                    }
                }
            }
        }
        StmtKind::Remove { targets } => {
            for target in targets {
                let Some(name) = resolve_expr(target, state, report) else {
                    continue;
                };
                for member in target_members(&name, state) {
                    if state.mobjects.contains_key(&member) {
                        state.last_end.insert(member, state.cursor);
                    } else {
                        report.warn(
                            stmt.line,
                            stmt.column,
                            format!("self.remove of unknown mobject `{member}`"),
                            stmt.snippet.clone(),
                        );
                    }
                }
            }
        }
        StmtKind::Mutate {
            target,
            method,
            animated,
        } => {
            if *animated {
                report.warn(
                    stmt.line,
                    stmt.column,
                    format!("`{target}.animate` outside `self.play` ignored"),
                    stmt.snippet.clone(),
                );
                return;
            }
            let Some(mut mob) = state.mobjects.get(target).cloned() else {
                report.warn(
                    stmt.line,
                    stmt.column,
                    format!("method call on unknown mobject `{target}`"),
                    stmt.snippet.clone(),
                );
                return;
            };
            let placement = placement_for(method, &mob, state);
            apply_method(
                &mut mob,
                method,
                placement,
                stmt.line,
                stmt.column,
                &stmt.snippet,
                report,
            );
            state.mobjects.insert(target.clone(), mob);
        }
        StmtKind::MutateExpr { expr, animated } => {
            if *animated {
                report.warn(
                    stmt.line,
                    stmt.column,
                    "`<expr>.animate` outside `self.play` ignored".to_owned(),
                    stmt.snippet.clone(),
                );
                return;
            }
            // Evaluating the expression applies its chained methods as side
            // effects (e.g. `VGroup(a, b).arrange(DOWN)` repositions members).
            let _ = resolve_expr(expr, state, report);
        }
        StmtKind::Play {
            animations,
            run_time,
            rate_func,
        } => {
            interpret_play(
                animations,
                *run_time,
                rate_func.as_deref(),
                state,
                stmt,
                report,
            );
        }
    }
}

fn interpret_play(
    animations: &[Anim],
    play_run_time: Option<f64>,
    rate_func: Option<&str>,
    state: &mut State,
    stmt: &Stmt,
    report: &mut Report,
) {
    let base_run = play_run_time.unwrap_or(DEFAULT_RUN_TIME);
    let mut play_duration = base_run;
    for anim in animations {
        let run = match anim {
            Anim::Simple { run_time, .. } | Anim::Transform { run_time, .. } => {
                run_time.unwrap_or(base_run)
            }
            Anim::Animate { .. } | Anim::Unsupported { .. } => base_run,
        };
        play_duration = play_duration.max(run);
    }
    let interp = match rate_func {
        Some("linear") | None => Interpolation::Linear,
        Some("smooth") => Interpolation::EaseInOut,
        Some(other) => {
            report.warn(
                stmt.line,
                stmt.column,
                format!("unsupported rate_func `{other}`; using linear"),
                stmt.snippet.clone(),
            );
            Interpolation::Linear
        }
    };

    for anim in animations {
        let run = match anim {
            Anim::Simple { run_time, .. } | Anim::Transform { run_time, .. } => {
                run_time.unwrap_or(base_run)
            }
            Anim::Animate { .. } | Anim::Unsupported { .. } => base_run,
        };
        apply_anim(anim, run, interp, state, stmt, report);
    }
    state.cursor += play_duration;
}

#[allow(clippy::too_many_lines)]
fn apply_anim(
    anim: &Anim,
    run: f64,
    interp: Interpolation,
    state: &mut State,
    stmt: &Stmt,
    report: &mut Report,
) {
    match anim {
        Anim::Unsupported { text } => {
            report.warn(
                stmt.line,
                stmt.column,
                format!("unsupported animation: {text}"),
                stmt.snippet.clone(),
            );
        }
        Anim::Transform { source, target, .. } => {
            let Some(source_name) = resolve_expr(source, state, report) else {
                return;
            };
            let Some(target_name) = resolve_expr(target, state, report) else {
                return;
            };
            report.warn(
                stmt.line,
                stmt.column,
                format!("Transform(`{source_name}`, `{target_name}`) approximated by a crossfade"),
                stmt.snippet.clone(),
            );
            for member in target_members(&source_name, state) {
                crossfade(&member, run, interp, state);
            }
            for member in target_members(&target_name, state) {
                crossfade(&member, run, interp, state);
            }
        }
        Anim::Simple { kind, target, .. } => {
            let Some(name) = resolve_expr(target, state, report) else {
                return;
            };
            for member in target_members(&name, state) {
                apply_simple_anim(*kind, &member, run, interp, state, stmt, report);
            }
        }
        Anim::Animate { target, method } => {
            for member in target_members(target, state) {
                let Some(mob) = state.mobjects.get(&member).cloned() else {
                    report.warn(
                        stmt.line,
                        stmt.column,
                        format!("animation targets unknown mobject `{member}`"),
                        stmt.snippet.clone(),
                    );
                    continue;
                };
                state
                    .first_appear
                    .entry(member.clone())
                    .or_insert(state.cursor);
                let placement = placement_for(method, &mob, state);
                apply_animate(
                    &member, &mob, method, placement, run, interp, state, stmt, report,
                );
            }
        }
    }
}

/// Apply one simple animation to a single concrete mobject.
fn apply_simple_anim(
    kind: AnimKind,
    target: &str,
    run: f64,
    interp: Interpolation,
    state: &mut State,
    stmt: &Stmt,
    report: &mut Report,
) {
    let Some(mob) = state.mobjects.get(target).cloned() else {
        report.warn(
            stmt.line,
            stmt.column,
            format!("animation targets unknown mobject `{target}`"),
            stmt.snippet.clone(),
        );
        return;
    };
    state
        .first_appear
        .entry(target.to_owned())
        .or_insert(state.cursor);
    let start = state.cursor;
    let end = start + run;
    match kind {
        AnimKind::Write => {
            if matches!(mob.kind, Kind::Tex { .. }) {
                push_event(
                    state,
                    target,
                    Target::Param("reveal".to_owned()),
                    start,
                    0.0,
                    interp,
                );
                push_event(
                    state,
                    target,
                    Target::Param("reveal".to_owned()),
                    end,
                    1.0,
                    interp,
                );
            } else {
                push_opacity(state, target, start, 0.0, end, 1.0, interp);
            }
        }
        AnimKind::Create | AnimKind::FadeIn => {
            push_opacity(state, target, start, 0.0, end, 1.0, interp);
        }
        AnimKind::GrowFromCenter => {
            push_opacity(state, target, start, 0.0, end, 1.0, interp);
            let full = mob_transform(&mob);
            push_event(
                state,
                target,
                Target::Transform(TransformProperty::ScaleX),
                start,
                0.0,
                interp,
            );
            push_event(
                state,
                target,
                Target::Transform(TransformProperty::ScaleX),
                end,
                full.scale_x,
                interp,
            );
            push_event(
                state,
                target,
                Target::Transform(TransformProperty::ScaleY),
                start,
                0.0,
                interp,
            );
            push_event(
                state,
                target,
                Target::Transform(TransformProperty::ScaleY),
                end,
                full.scale_y,
                interp,
            );
        }
        AnimKind::FadeOut => {
            push_opacity(state, target, start, 1.0, end, 0.0, interp);
            state.last_end.insert(target.to_owned(), end);
        }
        AnimKind::Indicate => {
            report.warn(
                stmt.line,
                stmt.column,
                format!("animation `Indicate` on `{target}` is not supported; skipped"),
                stmt.snippet.clone(),
            );
        }
        AnimKind::Transform | AnimKind::ReplacementTransform => unreachable!(),
    }
}

fn crossfade(target: &str, run: f64, interp: Interpolation, state: &mut State) {
    if !state.mobjects.contains_key(target) {
        return;
    }
    state
        .first_appear
        .entry(target.to_owned())
        .or_insert(state.cursor);
    let start = state.cursor;
    let end = start + run;
    push_opacity(state, target, start, 0.0, end, 1.0, interp);
}

#[allow(clippy::too_many_arguments, clippy::float_cmp, clippy::too_many_lines)] // Exact equality decides whether a component moved.
fn apply_animate(
    target: &str,
    mob: &Mobject,
    method: &Method,
    placement: Option<(f64, f64)>,
    run: f64,
    interp: Interpolation,
    state: &mut State,
    stmt: &Stmt,
    report: &mut Report,
) {
    let mut mob = mob.clone();
    let start = state.cursor;
    let end = start + run;
    let before = mob_transform(&mob);
    match method {
        Method::Shift { delta } => {
            mob.center.0 += delta.0;
            mob.center.1 += delta.1;
        }
        Method::MoveTo { point } => {
            mob.center = *point;
        }
        Method::NextTo { .. } | Method::ToEdge { .. } => {
            if let Some(center) = placement {
                mob.center = center;
            }
        }
        Method::Scale { factor } => {
            mob.scale *= factor;
        }
        Method::Rotate { angle } => {
            mob.rotation += angle;
        }
        Method::Arrange { .. } => {
            report.warn(
                stmt.line,
                stmt.column,
                format!("`.animate.arrange` on `{target}` is not supported; skipped"),
                stmt.snippet.clone(),
            );
            return;
        }
        Method::Unsupported { name } => {
            report.warn(
                stmt.line,
                stmt.column,
                format!("unsupported `.animate` method `{name}` on `{target}`"),
                stmt.snippet.clone(),
            );
            return;
        }
    }
    let after = mob_transform(&mob);
    if before.x != after.x {
        push_event(
            state,
            target,
            Target::Transform(TransformProperty::X),
            start,
            before.x,
            interp,
        );
        push_event(
            state,
            target,
            Target::Transform(TransformProperty::X),
            end,
            after.x,
            interp,
        );
    }
    if before.y != after.y {
        push_event(
            state,
            target,
            Target::Transform(TransformProperty::Y),
            start,
            before.y,
            interp,
        );
        push_event(
            state,
            target,
            Target::Transform(TransformProperty::Y),
            end,
            after.y,
            interp,
        );
    }
    if before.scale_x != after.scale_x {
        push_event(
            state,
            target,
            Target::Transform(TransformProperty::ScaleX),
            start,
            before.scale_x,
            interp,
        );
        push_event(
            state,
            target,
            Target::Transform(TransformProperty::ScaleX),
            end,
            after.scale_x,
            interp,
        );
    }
    if before.scale_y != after.scale_y {
        push_event(
            state,
            target,
            Target::Transform(TransformProperty::ScaleY),
            start,
            before.scale_y,
            interp,
        );
        push_event(
            state,
            target,
            Target::Transform(TransformProperty::ScaleY),
            end,
            after.scale_y,
            interp,
        );
    }
    if before.rotation != after.rotation {
        push_event(
            state,
            target,
            Target::Transform(TransformProperty::Rotation),
            start,
            before.rotation,
            interp,
        );
        push_event(
            state,
            target,
            Target::Transform(TransformProperty::Rotation),
            end,
            after.rotation,
            interp,
        );
    }
    state.mobjects.insert(target.to_owned(), mob);
}

fn apply_method(
    mob: &mut Mobject,
    method: &Method,
    placement: Option<(f64, f64)>,
    line: usize,
    column: usize,
    snippet: &str,
    report: &mut Report,
) {
    match method {
        Method::Shift { delta } => {
            mob.center.0 += delta.0;
            mob.center.1 += delta.1;
        }
        Method::MoveTo { point } => {
            mob.center = *point;
        }
        Method::NextTo { .. } | Method::ToEdge { .. } => {
            if let Some(center) = placement {
                mob.center = center;
            }
        }
        Method::Scale { factor } => {
            mob.scale *= factor;
        }
        Method::Rotate { angle } => {
            mob.rotation += angle;
        }
        Method::Arrange { .. } => {
            // Arranging is a group-level operation; a plain mobject ignores it.
        }
        Method::Unsupported { name } => {
            report.warn(
                line,
                column,
                format!("unsupported method `{name}`"),
                snippet,
            );
        }
    }
}

/// Precompute a neighbour/edge-dependent centre before mutating `state.mobjects`.
fn placement_for(method: &Method, mob: &Mobject, state: &State) -> Option<(f64, f64)> {
    match method {
        Method::NextTo { other, direction } => {
            let neighbour = state.mobjects.get(other)?;
            Some(next_to_center(mob, neighbour, *direction))
        }
        Method::ToEdge { direction } => Some(to_edge_center(mob, *direction)),
        _ => None,
    }
}

fn next_to_center(mob: &Mobject, neighbour: &Mobject, direction: (f64, f64)) -> (f64, f64) {
    let half_width =
        f64::midpoint(mob.size.0 * mob.scale, neighbour.size.0 * neighbour.scale) + NEXT_TO_BUFF;
    let half_height =
        f64::midpoint(mob.size.1 * mob.scale, neighbour.size.1 * neighbour.scale) + NEXT_TO_BUFF;
    (
        neighbour.center.0 + direction.0 * half_width,
        neighbour.center.1 + direction.1 * half_height,
    )
}

fn to_edge_center(mob: &Mobject, direction: (f64, f64)) -> (f64, f64) {
    let half_width = mob.size.0 * mob.scale / 2.0;
    let half_height = mob.size.1 * mob.scale / 2.0;
    let x = if direction.0.abs() > 1e-9 {
        direction.0 * (FRAME_WIDTH / 2.0 - half_width - TO_EDGE_BUFF).max(0.0)
    } else {
        mob.center.0
    };
    let y = if direction.1.abs() > 1e-9 {
        direction.1 * (FRAME_HEIGHT / 2.0 - half_height - TO_EDGE_BUFF).max(0.0)
    } else {
        mob.center.1
    };
    (x, y)
}

fn push_opacity(
    state: &mut State,
    clip: &str,
    start: f64,
    start_value: f64,
    end: f64,
    end_value: f64,
    interp: Interpolation,
) {
    push_event(
        state,
        clip,
        Target::Transform(TransformProperty::Opacity),
        start,
        start_value,
        interp,
    );
    push_event(
        state,
        clip,
        Target::Transform(TransformProperty::Opacity),
        end,
        end_value,
        Interpolation::Linear,
    );
}

fn push_event(
    state: &mut State,
    clip: &str,
    target: Target,
    time: f64,
    value: f64,
    interp: Interpolation,
) {
    state.events.push(KeyEvent {
        clip: clip.to_owned(),
        target,
        time,
        value,
        interp,
    });
}

// ---------- inline mobject expressions ----------

enum Built {
    Mob(Mobject),
    Group(Vec<String>),
}

fn synthetic_name(expr: &MobjectExpr) -> String {
    format!("_anon_{}_{}", expr.line, expr.column)
}

/// Materialize an expression, returning the name it is known by in [`State`].
/// Named references are returned as-is; inline expressions get a synthetic id.
fn resolve_expr(expr: &MobjectExpr, state: &mut State, report: &mut Report) -> Option<String> {
    match &expr.kind {
        MobjectExprKind::Ref(name) => Some(name.clone()),
        MobjectExprKind::Unsupported(text) => {
            report.warn(
                expr.line,
                expr.column,
                format!("unsupported mobject expression: {text}"),
                expr.snippet.clone(),
            );
            None
        }
        _ => {
            let name = synthetic_name(expr);
            define_expr(expr, &name, state, report).then_some(name)
        }
    }
}

/// Define `expr` under `name` in the state (as a mobject or a group).
fn define_expr(expr: &MobjectExpr, name: &str, state: &mut State, report: &mut Report) -> bool {
    match build_expr(expr, state, report) {
        Ok(Built::Mob(mob)) => {
            if !state.mobjects.contains_key(name) {
                state.order.push(name.to_owned());
            }
            state.mobjects.insert(name.to_owned(), mob);
            state
                .decl_pos
                .entry(name.to_owned())
                .or_insert_with(|| (expr.line, expr.column, expr.snippet.clone()));
            true
        }
        Ok(Built::Group(members)) => {
            state.groups.insert(name.to_owned(), members);
            true
        }
        Err(message) => {
            report.warn(expr.line, expr.column, message, expr.snippet.clone());
            false
        }
    }
}

/// Build a resolved mobject or group value without naming it.
fn build_expr(expr: &MobjectExpr, state: &mut State, report: &mut Report) -> Result<Built, String> {
    match &expr.kind {
        MobjectExprKind::Ref(name) => state
            .mobjects
            .get(name)
            .cloned()
            .map(Built::Mob)
            .ok_or_else(|| format!("unknown mobject `{name}`")),
        MobjectExprKind::Factory(decl) => Ok(Built::Mob(build_mobject("", decl))),
        MobjectExprKind::Chain(base, method) => match build_expr(base, state, report)? {
            Built::Mob(mut mob) => {
                let placement = placement_for(method, &mob, state);
                apply_method(
                    &mut mob,
                    method,
                    placement,
                    expr.line,
                    expr.column,
                    &expr.snippet,
                    report,
                );
                Ok(Built::Mob(mob))
            }
            Built::Group(members) => {
                apply_group_method(&members, method, expr, state, report);
                Ok(Built::Group(members))
            }
        },
        MobjectExprKind::Group(members) => {
            let mut names = Vec::new();
            for member in members {
                if let Some(name) = resolve_expr(member, state, report) {
                    names.push(name);
                }
            }
            Ok(Built::Group(names))
        }
        MobjectExprKind::Unsupported(text) => Err(text.clone()),
    }
}

/// The concrete mobject names an expression refers to; groups expand to their
/// members recursively.
fn target_members(name: &str, state: &State) -> Vec<String> {
    let Some(members) = state.groups.get(name) else {
        return vec![name.to_owned()];
    };
    let mut out = Vec::new();
    for member in members {
        out.extend(target_members(member, state));
    }
    out
}

/// Apply a shared transform to every member of a group.
fn apply_group_method(
    members: &[String],
    method: &Method,
    expr: &MobjectExpr,
    state: &mut State,
    report: &mut Report,
) {
    match method {
        Method::Arrange { direction } => arrange_members(members, *direction, state),
        _ => {
            for member in members {
                let Some(mut mob) = state.mobjects.get(member).cloned() else {
                    continue;
                };
                let placement = placement_for(method, &mob, state);
                apply_method(
                    &mut mob,
                    method,
                    placement,
                    expr.line,
                    expr.column,
                    &expr.snippet,
                    report,
                );
                state.mobjects.insert(member.clone(), mob);
            }
        }
    }
}

/// Approximate `VGroup.arrange`: keep the first member where it is, then lay
/// the rest out one after another along `direction`.
fn arrange_members(members: &[String], direction: (f64, f64), state: &mut State) {
    let horizontal = direction.0.abs() > 1e-9;
    let mut previous_center: Option<(f64, f64)> = None;
    let mut previous_size = (0.0, 0.0);
    for name in members {
        let Some(mut mob) = state.mobjects.get(name).cloned() else {
            continue;
        };
        let this_size = (mob.size.0 * mob.scale, mob.size.1 * mob.scale);
        let center = match previous_center {
            None => mob.center,
            Some(prev) => {
                let (gap_x, gap_y) = if horizontal {
                    (
                        previous_size.0 / 2.0 + this_size.0 / 2.0 + NEXT_TO_BUFF,
                        0.0,
                    )
                } else {
                    (
                        0.0,
                        previous_size.1 / 2.0 + this_size.1 / 2.0 + NEXT_TO_BUFF,
                    )
                };
                (prev.0 + direction.0 * gap_x, prev.1 + direction.1 * gap_y)
            }
        };
        mob.center = center;
        state.mobjects.insert(name.clone(), mob);
        previous_center = Some(center);
        previous_size = this_size;
    }
}

// ---------- mobject construction ----------

fn build_mobject(_name: &str, decl: &MobjectDecl) -> Mobject {
    let (kind, size, normalized_scale, natural_center) = mobject_geometry(decl);
    Mobject {
        kind,
        center: decl.center.unwrap_or(natural_center),
        scale: 1.0,
        rotation: 0.0,
        size,
        normalized_scale,
        opacity: 1.0,
    }
}

/// Geometry result of building a mobject from a declaration.
type MobGeometry = (Kind, (f64, f64), (f64, f64), (f64, f64));

#[allow(clippy::too_many_lines)] // One arm per mobject kind keeps the size mapping together.
fn mobject_geometry(decl: &MobjectDecl) -> MobGeometry {
    match &decl.kind {
        DeclKind::Text {
            text,
            font_size,
            color,
        } => (
            Kind::Text {
                text: text.clone(),
                font_size_px: *font_size,
                color: *color,
            },
            estimate_text_size(text, *font_size),
            (1.0, 1.0),
            (0.0, 0.0),
        ),
        DeclKind::Tex {
            source,
            mode,
            size,
            color,
        } => (
            Kind::Tex {
                source: source.clone(),
                mode: mode.clone(),
                size_px: *size,
                color: *color,
            },
            estimate_text_size(source, *size),
            (1.0, 1.0),
            (0.0, 0.0),
        ),
        DeclKind::Circle {
            radius,
            color,
            fill,
        } => {
            let diameter = 2.0 * radius;
            (
                Kind::Shape {
                    geom: ShapeGeom::Ellipse,
                    stroke: Some(*color),
                    fill: *fill,
                },
                (diameter, diameter),
                (diameter / FRAME_WIDTH, diameter / FRAME_HEIGHT),
                (0.0, 0.0),
            )
        }
        DeclKind::Square { side, color, fill } => (
            Kind::Shape {
                geom: ShapeGeom::Rect,
                stroke: Some(*color),
                fill: *fill,
            },
            (*side, *side),
            (side / FRAME_WIDTH, side / FRAME_HEIGHT),
            (0.0, 0.0),
        ),
        DeclKind::Rectangle {
            width,
            height,
            color,
            fill,
        } => (
            Kind::Shape {
                geom: ShapeGeom::Rect,
                stroke: Some(*color),
                fill: *fill,
            },
            (*width, *height),
            (width / FRAME_WIDTH, height / FRAME_HEIGHT),
            (0.0, 0.0),
        ),
        DeclKind::Dot { radius, color } => {
            let diameter = 2.0 * radius;
            (
                Kind::Shape {
                    geom: ShapeGeom::Ellipse,
                    stroke: None,
                    fill: Some(*color),
                },
                (diameter, diameter),
                (diameter / FRAME_WIDTH, diameter / FRAME_HEIGHT),
                (0.0, 0.0),
            )
        }
        DeclKind::Line { start, end, color } => {
            let (verbs, center) = line_geometry(*start, *end);
            let size = verbs_size(&verbs);
            (
                Kind::Shape {
                    geom: ShapeGeom::Path { verbs },
                    stroke: Some(*color),
                    fill: None,
                },
                size,
                (size.0 / FRAME_WIDTH, size.1 / FRAME_HEIGHT),
                center,
            )
        }
        DeclKind::Arrow { start, end, color } => {
            let (verbs, center) = arrow_geometry(*start, *end);
            let size = verbs_size(&verbs);
            (
                Kind::Shape {
                    geom: ShapeGeom::Path { verbs },
                    stroke: Some(*color),
                    fill: None,
                },
                size,
                (size.0 / FRAME_WIDTH, size.1 / FRAME_HEIGHT),
                center,
            )
        }
        DeclKind::Polygon {
            vertices,
            color,
            fill,
        } => {
            let (verbs, center) = polygon_geometry(vertices);
            let size = verbs_size(&verbs);
            (
                Kind::Shape {
                    geom: ShapeGeom::Path { verbs },
                    stroke: Some(*color),
                    fill: *fill,
                },
                size,
                (size.0 / FRAME_WIDTH, size.1 / FRAME_HEIGHT),
                center,
            )
        }
        DeclKind::Image { path } => (
            Kind::Image { path: path.clone() },
            IMAGE_SIZE,
            (1.0, 1.0),
            (0.0, 0.0),
        ),
    }
}

fn estimate_text_size(text: &str, font_size_px: f64) -> (f64, f64) {
    let chars = text.chars().count().max(1) as f64;
    (
        (font_size_px * 0.55 * chars) / PX_PER_UNIT,
        font_size_px / PX_PER_UNIT,
    )
}

fn line_geometry(start: (f64, f64), end: (f64, f64)) -> (Vec<LocalVerb>, (f64, f64)) {
    let center = (f64::midpoint(start.0, end.0), f64::midpoint(start.1, end.1));
    (
        vec![
            LocalVerb::Move((start.0 - center.0, start.1 - center.1)),
            LocalVerb::Line((end.0 - center.0, end.1 - center.1)),
        ],
        center,
    )
}

fn arrow_geometry(start: (f64, f64), end: (f64, f64)) -> (Vec<LocalVerb>, (f64, f64)) {
    let (dx, dy) = (end.0 - start.0, end.1 - start.1);
    let length = (dx * dx + dy * dy).sqrt();
    let (ux, uy) = if length > 1e-12 {
        (dx / length, dy / length)
    } else {
        (1.0, 0.0)
    };
    let tip_len = 0.25_f64.min(length * 0.25);
    let half_width = 0.15_f64.min(length * 0.15);
    let (bx1, by1) = (
        end.0 - ux * tip_len - uy * half_width,
        end.1 - uy * tip_len + ux * half_width,
    );
    let (bx2, by2) = (
        end.0 - ux * tip_len + uy * half_width,
        end.1 - uy * tip_len - ux * half_width,
    );
    let center = bbox_center(&[start, end, (bx1, by1), (bx2, by2)]);
    (
        vec![
            LocalVerb::Move((start.0 - center.0, start.1 - center.1)),
            LocalVerb::Line((end.0 - center.0, end.1 - center.1)),
            LocalVerb::Move((bx1 - center.0, by1 - center.1)),
            LocalVerb::Line((end.0 - center.0, end.1 - center.1)),
            LocalVerb::Line((bx2 - center.0, by2 - center.1)),
        ],
        center,
    )
}

fn polygon_geometry(vertices: &[(f64, f64)]) -> (Vec<LocalVerb>, (f64, f64)) {
    let center = bbox_center(vertices);
    let mut verbs = Vec::with_capacity(vertices.len() + 1);
    for (index, (x, y)) in vertices.iter().enumerate() {
        let point = (x - center.0, y - center.1);
        if index == 0 {
            verbs.push(LocalVerb::Move(point));
        } else {
            verbs.push(LocalVerb::Line(point));
        }
    }
    verbs.push(LocalVerb::Close);
    (verbs, center)
}

fn bbox_center(points: &[(f64, f64)]) -> (f64, f64) {
    let Some((min_x, min_y, max_x, max_y)) = bounds(points) else {
        return (0.0, 0.0);
    };
    (min_x + (max_x - min_x) / 2.0, min_y + (max_y - min_y) / 2.0)
}

fn verbs_size(verbs: &[LocalVerb]) -> (f64, f64) {
    let points: Vec<(f64, f64)> = verbs
        .iter()
        .filter_map(|v| match v {
            LocalVerb::Move(p) | LocalVerb::Line(p) => Some(*p),
            LocalVerb::Close => None,
        })
        .collect();
    let Some((min_x, min_y, max_x, max_y)) = bounds(&points) else {
        return (1.0, 1.0);
    };
    ((max_x - min_x).max(1e-9), (max_y - min_y).max(1e-9))
}

fn bounds(points: &[(f64, f64)]) -> Option<(f64, f64, f64, f64)> {
    let mut iter = points.iter();
    let (first_x, first_y) = *iter.next()?;
    let mut min_x = first_x;
    let mut min_y = first_y;
    let mut max_x = first_x;
    let mut max_y = first_y;
    for (x, y) in iter {
        min_x = min_x.min(*x);
        min_y = min_y.min(*y);
        max_x = max_x.max(*x);
        max_y = max_y.max(*y);
    }
    Some((min_x, min_y, max_x, max_y))
}

// ---------- transform mapping ----------

fn mob_transform(mob: &Mobject) -> Transform {
    Transform {
        x: 0.5 + mob.center.0 / FRAME_WIDTH,
        y: 0.5 - mob.center.1 / FRAME_HEIGHT,
        scale_x: mob.normalized_scale.0 * mob.scale,
        scale_y: mob.normalized_scale.1 * mob.scale,
        // Screen y is down, so negate to preserve Manim's counter-clockwise sense.
        rotation: -mob.rotation,
        opacity: mob.opacity,
    }
}

// ---------- project construction ----------

#[allow(clippy::too_many_lines)] // The rebuild is a single, ordered migration sequence.
fn build_project(
    scene_end: f64,
    state: &State,
    options: &ManimOptions,
    mut report: Report,
) -> Result<(Project, Vec<Asset>, Report), ImportError> {
    let mut project = Project::new(FrameRate::new(DEFAULT_FPS, 1)?);
    Command::SetSettings {
        width: CANVAS_WIDTH,
        height: CANVAS_HEIGHT,
        frame_rate: project.frame_rate(),
        background: [0, 0, 0, 255],
        duration: secs_to_ticks(scene_end),
    }
    .apply(&mut project)?;

    let mut assets: BTreeMap<AssetId, Asset> = BTreeMap::new();
    let mut track_index = 0_u64;
    let mut clip_index = 0_u64;

    for name in &state.order {
        let Some(mob) = state.mobjects.get(name) else {
            continue;
        };
        let pos = state.decl_pos.get(name);
        let Some(appear) = state.first_appear.get(name).copied() else {
            report.warn(
                pos.map_or(0, |p| p.0),
                pos.map_or(0, |p| p.1),
                format!("mobject `{name}` is never added to the scene; skipped"),
                pos.map_or("", |p| p.2.as_str()),
            );
            continue;
        };
        let end = state.last_end.get(name).copied().unwrap_or(scene_end);
        let start_ticks = secs_to_ticks(appear);
        let end_ticks = secs_to_ticks(end);
        if end_ticks <= start_ticks {
            report.warn(
                pos.map_or(0, |p| p.0),
                pos.map_or(0, |p| p.1),
                format!("mobject `{name}` has an empty time range; skipped"),
                pos.map_or("", |p| p.2.as_str()),
            );
            continue;
        }

        // Resolve images once, before building the clip kind.
        let image = match &mob.kind {
            Kind::Image { path } => Some(resolve_image(path, options)),
            _ => None,
        };
        let (kind, params) = build_clip_kind(mob, image.as_ref(), &mut report, pos);
        if let Some(Ok((asset_name, media_type, bytes))) = &image {
            let id = AssetId::of(bytes);
            if !assets.contains_key(&id) {
                Command::AddAsset {
                    id: id.clone(),
                    asset: CoreAsset {
                        name: asset_name.clone(),
                        media_type: media_type.clone(),
                    },
                }
                .apply(&mut project)?;
            }
            assets.entry(id.clone()).or_insert_with(|| Asset {
                id,
                name: asset_name.clone(),
                media_type: media_type.clone(),
                bytes: bytes.clone(),
            });
            report.info(
                pos.map_or(0, |p| p.0),
                pos.map_or(0, |p| p.1),
                format!(
                    "embedded `{asset_name}` ({media_type}, {} bytes)",
                    bytes.len()
                ),
                pos.map_or("", |p| p.2.as_str()),
            );
        }

        track_index += 1;
        clip_index += 1;
        let track_id = TrackId(track_index);
        let clip_id = ClipId(clip_index);
        Command::AddTrack {
            track: Track::new(track_id, name.clone()),
            index: usize::try_from(track_index - 1).unwrap_or(usize::MAX),
        }
        .apply(&mut project)?;
        let clip = Clip {
            link_group: None,
            id: clip_id,
            kind,
            range: TimeRange::new(start_ticks, end_ticks)?,
            source_offset: Time(0),
            transform: mob_transform(mob),
            params: params.into_iter().map(|(k, v)| (k, v.into())).collect(),
            effects: Vec::new(),
            animations: Vec::new(),
        };
        Command::AddClip {
            track: track_id,
            clip,
        }
        .apply(&mut project)?;

        for event in state.events.iter().filter(|e| e.clip == *name) {
            let local_seconds = (event.time - appear).max(0.0);
            Command::AddKeyframe {
                id: clip_id,
                target: event.target.clone(),
                time: secs_to_ticks(local_seconds),
                keyframe: Keyframe {
                    value: event.value.into(),
                    interpolation: event.interp,
                },
            }
            .apply(&mut project)?;
        }
    }

    project.validate()?;
    let assets = assets.into_values().collect();
    Ok((project, assets, report))
}

fn build_clip_kind(
    mob: &Mobject,
    image: Option<&ImageResolution>,
    report: &mut Report,
    pos: Option<&(usize, usize, String)>,
) -> (ClipKind, BTreeMap<String, f64>) {
    match &mob.kind {
        Kind::Text {
            text,
            font_size_px,
            color,
        } => (
            ClipKind::Builtin(Builtin::Text {
                text: text.clone(),
                font_size: (*font_size_px / f64::from(CANVAS_HEIGHT)).clamp(f64::EPSILON, 16.0),
                color: *color,
            }),
            BTreeMap::new(),
        ),
        Kind::Tex {
            source,
            mode,
            size_px,
            color,
        } => {
            let mut params = BTreeMap::new();
            params.insert("reveal".to_owned(), 1.0);
            (
                ClipKind::Plugin {
                    plugin_id: LATEX_PLUGIN.to_owned(),
                    params: serde_json::json!({
                        "source": source,
                        "mode": mode,
                        "size": size_px,
                        "color": hex(*color),
                    }),
                },
                params,
            )
        }
        Kind::Shape { geom, stroke, fill } => {
            let shape = builtin_shape(geom);
            let stroke_width = {
                let scale_x = mob.normalized_scale.0 * mob.scale;
                if scale_x > 0.0 {
                    STROKE_PX / (scale_x * f64::from(CANVAS_WIDTH))
                } else {
                    0.0
                }
            };
            (
                ClipKind::Builtin(Builtin::Shape {
                    shape,
                    fill: *fill,
                    stroke: stroke.map(|color| Stroke {
                        color,
                        width: stroke_width,
                    }),
                }),
                BTreeMap::new(),
            )
        }
        Kind::Image { path } => match image {
            Some(Ok((_, _, bytes))) => (ClipKind::Image(AssetId::of(bytes)), BTreeMap::new()),
            Some(Err(reason)) => {
                report.warn(
                    pos.map_or(0, |p| p.0),
                    pos.map_or(0, |p| p.1),
                    format!("ImageMobject `{path}` rejected: {reason}"),
                    pos.map_or("", |p| p.2.as_str()),
                );
                (
                    ClipKind::Error(format!("unavailable image: {path} ({reason})")),
                    BTreeMap::new(),
                )
            }
            None => {
                report.warn(
                    pos.map_or(0, |p| p.0),
                    pos.map_or(0, |p| p.1),
                    format!("ImageMobject `{path}` not found; using a placeholder"),
                    pos.map_or("", |p| p.2.as_str()),
                );
                (
                    ClipKind::Error(format!("missing image: {path}")),
                    BTreeMap::new(),
                )
            }
        },
    }
}

fn builtin_shape(geom: &ShapeGeom) -> Shape {
    match geom {
        ShapeGeom::Rect => Shape::Rect,
        ShapeGeom::Ellipse => Shape::Ellipse,
        ShapeGeom::Path { verbs } => Shape::Path(builtin_path(verbs)),
    }
}

fn builtin_path(verbs: &[LocalVerb]) -> Vec<PathElement> {
    let points: Vec<(f64, f64)> = verbs
        .iter()
        .filter_map(|v| match v {
            LocalVerb::Move(p) | LocalVerb::Line(p) => Some(*p),
            LocalVerb::Close => None,
        })
        .collect();
    let Some((min_x, min_y, max_x, max_y)) = bounds(&points) else {
        return Vec::new();
    };
    let width = max_x - min_x;
    let height = max_y - min_y;
    let map = |point: (f64, f64)| -> [f64; 2] {
        let u = if width > 0.0 {
            (point.0 - min_x) / width
        } else {
            0.5
        };
        let v = if height > 0.0 {
            (max_y - point.1) / height
        } else {
            0.5
        };
        [u, v]
    };
    verbs
        .iter()
        .map(|verb| match verb {
            LocalVerb::Move(point) => PathElement::MoveTo(map(*point)),
            LocalVerb::Line(point) => PathElement::LineTo(map(*point)),
            LocalVerb::Close => PathElement::Close,
        })
        .collect()
}

fn resolve_image(path: &str, options: &ManimOptions) -> ImageResolution {
    let relative = Path::new(path);

    // Absolute paths and traversal components are never resolved: a malicious
    // scene must not be able to read arbitrary local files into the archive.
    // On Windows, `/image.png` has a root but no drive, so `is_absolute`
    // would miss it even though it must not be joined to the script directory.
    if relative.has_root() {
        return Err(format!("absolute path `{path}` is not allowed"));
    }
    for component in relative.components() {
        match component {
            Component::ParentDir => {
                return Err(format!("`..` path component in `{path}` is not allowed"));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!("unsupported path component in `{path}`"));
            }
            Component::CurDir | Component::Normal(_) => {}
        }
    }

    // Images are resolved relative to the script directory; without one the
    // importer cannot tell where the file is, so it refuses to guess.
    let Some(script_dir) = &options.script_dir else {
        return Err("no script directory was provided".to_owned());
    };
    let base = script_dir.canonicalize().map_err(|error| {
        format!(
            "cannot canonicalize script directory `{}`: {error}",
            script_dir.display()
        )
    })?;

    let candidate = base.join(relative);

    let extension = candidate
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase);
    let Some(extension) = extension else {
        return Err(format!("image `{path}` has no file extension"));
    };
    if !ALLOWED_IMAGE_EXTENSIONS.contains(&extension.as_str()) {
        return Err(format!(
            "image `{path}` has unsupported extension `.{extension}`"
        ));
    }

    // Canonicalize so a symlink pointing outside the script directory is
    // refused rather than followed.
    let candidate = candidate
        .canonicalize()
        .map_err(|error| format!("image `{path}` is not readable: {error}"))?;
    if !candidate.starts_with(&base) {
        return Err(format!(
            "image `{path}` resolves outside the script directory"
        ));
    }

    let metadata = candidate
        .metadata()
        .map_err(|error| format!("cannot stat image `{path}`: {error}"))?;
    if !metadata.is_file() {
        return Err(format!("image `{path}` is not a regular file"));
    }
    if metadata.len() > MAX_IMAGE_BYTES {
        return Err(format!(
            "image `{path}` exceeds the {MAX_IMAGE_BYTES} byte size limit"
        ));
    }

    let bytes = std::fs::read(&candidate)
        .map_err(|error| format!("cannot read image `{path}`: {error}"))?;
    verify_magic(&extension, &bytes)?;

    let media_type = media_type_for(&extension);
    let name = candidate
        .file_name()
        .map_or_else(|| "image".to_owned(), |n| n.to_string_lossy().into_owned());
    Ok((name, media_type, bytes))
}

fn verify_magic(extension: &str, bytes: &[u8]) -> Result<(), String> {
    let valid = match extension {
        "png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "jpg" | "jpeg" => {
            bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF
        }
        "gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "webp" => bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        "bmp" => bytes.starts_with(b"BM"),
        // SVG is XML text, not a raster format; the extension allowlist is the
        // content check for it.
        "svg" => true,
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(format!(
            "image content does not match its `.{extension}` extension"
        ))
    }
}

fn media_type_for(extension: &str) -> String {
    match extension {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        _ => "application/octet-stream",
    }
    .to_owned()
}

fn hex([r, g, b, _]: Rgba) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

fn secs_to_ticks(seconds: f64) -> Time {
    let ticks = (seconds * TICKS_PER_SECOND as f64).round();
    Time(ticks.clamp(i64::MIN as f64, i64::MAX as f64) as i64)
}
