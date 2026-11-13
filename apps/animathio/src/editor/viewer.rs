//! Canvas presentation and command-based transform gestures.
use super::*;
use crate::style::{color, hex, hexa};

pub(super) struct ViewerState {
    bounds: Bounds<Pixels>,
    pub grid: bool,
    pub checker: bool,
    drag: Option<GizmoDrag>,
    pub volume: f64,
    pub timecode_editing: bool,
    pub focus_timecode: bool,
}

impl Default for ViewerState {
    fn default() -> Self {
        Self {
            bounds: Bounds::default(),
            grid: false,
            checker: false,
            drag: None,
            volume: 1.0,
            timecode_editing: false,
            focus_timecode: false,
        }
    }
}

struct GizmoDrag {
    id: ClipId,
    origin: Point<Pixels>,
    transform: Transform,
    handle: Handle,
    /// Last unwrapped rotation delta of a rotate drag, in radians.
    rotation: std::cell::Cell<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Handle {
    Move,
    Scale(f64, f64),
    Rotate,
}

/// All geometry uses the same canvas mapping as the renderer: rotation happens
/// in display pixels, after normalized scale, around the clip centre.
#[allow(clippy::many_single_char_names)]
fn local_point(t: Transform, canvas: Bounds<Pixels>, x: f64, y: f64) -> Point<Pixels> {
    let w = f64::from(f32::from(canvas.size.width));
    let h = f64::from(f32::from(canvas.size.height));
    let (sin, cos) = t.rotation.sin_cos();
    let dx = x * t.scale_x * w;
    let dy = y * t.scale_y * h;
    canvas.origin
        + point(
            px((t.x * w + dx * cos - dy * sin) as f32),
            px((t.y * h + dx * sin + dy * cos) as f32),
        )
}

fn handles(t: Transform, canvas: Bounds<Pixels>) -> [(Point<Pixels>, Handle); 9] {
    let points = [
        (-0.5, -0.5),
        (0.0, -0.5),
        (0.5, -0.5),
        (0.5, 0.0),
        (0.5, 0.5),
        (0.0, 0.5),
        (-0.5, 0.5),
        (-0.5, 0.0),
    ];
    let mut result = [(Point::default(), Handle::Move); 9];
    for (index, (x, y)) in points.into_iter().enumerate() {
        result[index] = (local_point(t, canvas, x, y), Handle::Scale(x, y));
    }
    let top = local_point(t, canvas, 0.0, -0.5);
    result[8] = (
        top + point(
            px(t.rotation.sin() as f32 * 26.),
            px(-t.rotation.cos() as f32 * 26.),
        ),
        Handle::Rotate,
    );
    result
}

fn hit_handle(t: Transform, canvas: Bounds<Pixels>, pointer: Point<Pixels>) -> Option<Handle> {
    for (position, handle) in handles(t, canvas) {
        if f32::from(position.x - pointer.x).abs() < 9.
            && f32::from(position.y - pointer.y).abs() < 9.
        {
            return Some(handle);
        }
    }
    contains_clip(t, canvas, pointer).then_some(Handle::Move)
}

fn contains_clip(t: Transform, canvas: Bounds<Pixels>, pointer: Point<Pixels>) -> bool {
    let centre = local_point(t, canvas, 0., 0.);
    let dx = f64::from(f32::from(pointer.x - centre.x));
    let dy = f64::from(f32::from(pointer.y - centre.y));
    let (sin, cos) = t.rotation.sin_cos();
    (dx * cos + dy * sin).abs() <= (t.scale_x * f64::from(f32::from(canvas.size.width)) / 2.).abs()
        && (-dx * sin + dy * cos).abs()
            <= (t.scale_y * f64::from(f32::from(canvas.size.height)) / 2.).abs()
}

/// Visible clips under `pointer`, topmost first. Media clips hit anywhere inside
/// their transformed bounds; plugin and vector clips (text, shapes, ...) have a
/// canvas-sized box but only draw part of it, so `has_ink` decides for those.
fn clips_under_pointer(
    project: &Project,
    time: Time,
    canvas: Bounds<Pixels>,
    pointer: Point<Pixels>,
    has_ink: &mut dyn FnMut(&animathio_core::SceneClip) -> bool,
) -> Vec<ClipId> {
    if !canvas.contains(&pointer) {
        return Vec::new();
    }
    evaluate(project, time)
        .clips
        .into_iter()
        .rev()
        .filter(|clip| {
            !matches!(clip.kind, ClipKind::Audio(_))
                && clip.transform.opacity > 0.
                && contains_clip(clip.transform, canvas, pointer)
                && (!matches!(clip.kind, ClipKind::Plugin { .. } | ClipKind::Vector(_))
                    || has_ink(clip))
        })
        .map(|clip| clip.id)
        .collect()
}

/// Whether `clip`, rendered alone at preview-thumbnail size, has visible pixels
/// within a few pixels of `pointer`. Rendering problems count as a hit so a clip
/// is never unselectable.
fn scene_clip_has_ink(
    host: &mut Option<PluginHost>,
    clip: &animathio_core::SceneClip,
    project_size: (u32, u32),
    canvas: Bounds<Pixels>,
    pointer: Point<Pixels>,
) -> bool {
    const LONG_EDGE: f32 = 480.;
    const RADIUS: i64 = 4;
    let (width, height) = project_size;
    let scale = LONG_EDGE / width.max(height).max(1) as f32;
    let size = (
        ((width as f32 * scale).round() as u32).max(1),
        ((height as f32 * scale).round() as u32).max(1),
    );
    let mut single = clip.clone();
    // Effects and opacity must not hide the clip's own shape.
    single.effects.clear();
    single.transform.opacity = 1.;
    let mut scene = Scene {
        clips: vec![single],
    };
    if let Some(host) = host {
        scene = host.resolve(&scene, size);
    }
    let Ok(frame) =
        animathio_render::render(&scene, &mut animathio_render::RenderCtx::default(), size)
    else {
        return true;
    };
    let fx = f32::from(pointer.x - canvas.origin.x) / f32::from(canvas.size.width).max(1.);
    let fy = f32::from(pointer.y - canvas.origin.y) / f32::from(canvas.size.height).max(1.);
    let (cx, cy) = ((fx * size.0 as f32) as i64, (fy * size.1 as f32) as i64);
    (cy - RADIUS..=cy + RADIUS).any(|y| {
        (cx - RADIUS..=cx + RADIUS).any(|x| {
            (0..i64::from(size.0)).contains(&x)
                && (0..i64::from(size.1)).contains(&y)
                && frame.pixels[((y * i64::from(size.0) + x) * 4 + 3) as usize] > 8
        })
    })
}

fn pick_from_stack(hits: &[ClipId], selected: Option<ClipId>, cycle: bool) -> Option<ClipId> {
    let index = if cycle {
        selected
            .and_then(|id| hits.iter().position(|hit| *hit == id))
            .map_or(0, |i| (i + 1) % hits.len().max(1))
    } else {
        0
    };
    hits.get(index).copied()
}

fn canvas_bounds(bounds: Bounds<Pixels>, size: (u32, u32), fit: ViewerFit) -> Bounds<Pixels> {
    let w = (f32::from(bounds.size.width) - 64.).max(1.);
    let h = (f32::from(bounds.size.height) - 72.).max(1.);
    let scale = match fit {
        ViewerFit::Contain => (w / size.0 as f32).min(h / size.1 as f32),
        ViewerFit::Fill => (w / size.0 as f32).max(h / size.1 as f32),
        ViewerFit::Actual => 1.,
    };
    let size = size_of(size, scale);
    Bounds::new(
        bounds.center() - point(size.width / 2., size.height / 2.),
        size,
    )
}
fn size_of((w, h): (u32, u32), scale: f32) -> Size<Pixels> {
    size(px(w as f32 * scale), px(h as f32 * scale))
}

/// Snap a translated box's centre or either edge to canvas centre/edges.
fn snap_axis(value: f64, extent: f64, threshold: f64) -> (f64, Option<f64>) {
    for guide in [0.5, 0., 1.] {
        for offset in [0., -extent / 2., extent / 2.] {
            if (value + offset - guide).abs() < threshold {
                return (guide - offset, Some(guide));
            }
        }
    }
    (value, None)
}

/// Smallest scale magnitude a handle drag may reach; the sign (mirroring) is kept.
const MIN_SCALE: f64 = 0.001;

/// `value` with its magnitude at least [`MIN_SCALE`] and its sign preserved.
fn min_magnitude(value: f64) -> f64 {
    if value.abs() >= MIN_SCALE {
        value
    } else if value.is_sign_negative() {
        -MIN_SCALE
    } else {
        MIN_SCALE
    }
}

/// The angle equal to `raw` modulo a full turn that lies nearest `previous`, so a
/// drag crossing the -x axis continues past pi instead of jumping by 2 pi.
fn unwrap_angle(raw: f64, previous: f64) -> f64 {
    let turn = std::f64::consts::TAU;
    raw + turn * ((previous - raw) / turn).round()
}

#[allow(clippy::many_single_char_names)]
fn dragged_transform(
    drag: &GizmoDrag,
    canvas: Bounds<Pixels>,
    pointer: Point<Pixels>,
    shift: bool,
    snap: bool,
) -> (Transform, [Option<f64>; 2]) {
    let mut t = drag.transform;
    let w = f64::from(f32::from(canvas.size.width)).max(1.);
    let h = f64::from(f32::from(canvas.size.height)).max(1.);
    let dx = f64::from(f32::from(pointer.x - drag.origin.x));
    let dy = f64::from(f32::from(pointer.y - drag.origin.y));
    let mut guides = [None, None];
    match drag.handle {
        Handle::Move => {
            t.x += dx / w;
            t.y += dy / h;
            if snap {
                let (sin, cos) = t.rotation.sin_cos();
                let ex = (t.scale_x * cos).abs() + (t.scale_y * h / w * sin).abs();
                let ey = (t.scale_y * cos).abs() + (t.scale_x * w / h * sin).abs();
                (t.x, guides[0]) = snap_axis(t.x, ex, 6. / w);
                (t.y, guides[1]) = snap_axis(t.y, ey, 6. / h);
            }
        }
        Handle::Scale(x, y) => {
            let (sin, cos) = t.rotation.sin_cos();
            let local_x = dx * cos + dy * sin;
            let local_y = -dx * sin + dy * cos;
            // Scale may be negative (mirrored); only keep it away from zero.
            if x != 0. {
                t.scale_x = min_magnitude(t.scale_x + local_x / (w * x));
            }
            if y != 0. {
                t.scale_y = min_magnitude(t.scale_y + local_y / (h * y));
            }
            if shift {
                let factor = if x == 0. {
                    t.scale_y / min_magnitude(drag.transform.scale_y)
                } else {
                    t.scale_x / min_magnitude(drag.transform.scale_x)
                };
                t.scale_x = drag.transform.scale_x * factor;
                t.scale_y = drag.transform.scale_y * factor;
            }
        }
        Handle::Rotate => {
            let c = local_point(t, canvas, 0., 0.);
            let angle = |p: Point<Pixels>| {
                f64::from(f32::from(p.y - c.y)).atan2(f64::from(f32::from(p.x - c.x)))
            };
            let delta = unwrap_angle(angle(pointer) - angle(drag.origin), drag.rotation.get());
            drag.rotation.set(delta);
            t.rotation += delta;
            if shift {
                t.rotation = (t.rotation / (std::f64::consts::PI / 12.)).round()
                    * (std::f64::consts::PI / 12.);
            }
        }
    }
    (t, guides)
}

impl Editor {
    fn selected_visual(&self) -> Option<(ClipId, Transform)> {
        let id = self.selected?;
        evaluate(&self.project, self.position)
            .clips
            .into_iter()
            .find(|c| c.id == id && !matches!(c.kind, ClipKind::Audio(_)))
            .map(|c| (id, c.transform))
    }

    pub(super) fn move_gizmo(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(drag) = &self.viewer_state.drag else {
            return;
        };
        let (t, _) = dragged_transform(
            drag,
            self.viewer_state.bounds,
            event.position,
            event.modifiers.shift,
            self.snapping,
        );
        let Some(clip) = self.find_clip(drag.id) else {
            return;
        };
        let values: Vec<(TransformProperty, f64)> = match drag.handle {
            Handle::Move => vec![(TransformProperty::X, t.x), (TransformProperty::Y, t.y)],
            Handle::Scale(..) => vec![
                (TransformProperty::ScaleX, t.scale_x),
                (TransformProperty::ScaleY, t.scale_y),
            ],
            Handle::Rotate => vec![(TransformProperty::Rotation, t.rotation)],
        };
        let commands = inspector::transform_edit_commands(clip, &values, self.position);
        self.preview_color(Command::Batch { commands }, cx);
    }

    pub(super) fn end_gizmo(&mut self) {
        self.viewer_state.drag = None;
    }

    pub(super) fn viewer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let image = self.image.clone();
        let fit = self.viewer_fit;
        let safe = self.safe_areas;
        let grid = self.viewer_state.grid;
        let checker = self.viewer_state.checker;
        let selected = self.selected_visual();
        let dragging = self.viewer_state.drag.is_some();
        let weak = cx.entity().downgrade();
        let paint_weak = weak.clone();
        let viewport = div()
            .id("viewer-stage")
            .flex_1()
            .min_h_0()
            .w_full()
            .relative()
            .overflow_hidden()
            .bg(hex(color::VIEWER_STAGE))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    if this.playing {
                        this.seek_to(this.position, cx);
                    }
                    let bounds = this.viewer_state.bounds;
                    let size = project_size(&this.project);
                    let host = &mut this.plugin_host;
                    let hits = clips_under_pointer(
                        &this.project,
                        this.position,
                        bounds,
                        event.position,
                        &mut |clip| scene_clip_has_ink(host, clip, size, bounds, event.position),
                    );
                    let selected_handle = this
                        .selected_visual()
                        .and_then(|(_, t)| hit_handle(t, this.viewer_state.bounds, event.position));
                    if event.modifiers.alt
                        || !matches!(selected_handle, Some(Handle::Scale(..) | Handle::Rotate))
                    {
                        let picked = pick_from_stack(&hits, this.selected, event.modifiers.alt);
                        this.set_selection(picked.into_iter().collect(), picked);
                        cx.notify();
                    }
                    let Some((id, transform)) = this.selected_visual() else {
                        return;
                    };
                    if this.project.tracks().iter().any(|track| {
                        track.is_locked() && track.clips().values().any(|clip| clip.id == id)
                    }) {
                        return;
                    }
                    if let Some(handle) =
                        hit_handle(transform, this.viewer_state.bounds, event.position)
                    {
                        this.viewer_state.drag = Some(GizmoDrag {
                            id,
                            origin: event.position,
                            transform,
                            handle,
                            rotation: std::cell::Cell::new(0.),
                        });
                        this.pointer_down = true;
                        cx.notify();
                    }
                }),
            )
            .child(
                canvas(
                    move |bounds, _, cx| {
                        let frame = canvas_bounds(bounds, (1920, 1080), fit);
                        let _ = weak.update(cx, |this, _| {
                            this.viewer_state.bounds =
                                canvas_bounds(bounds, project_size(&this.project), fit);
                        });
                        frame
                    },
                    move |bounds, _, window, cx| {
                        let frame = paint_weak
                            .update(cx, |this, _| this.viewer_state.bounds)
                            .unwrap_or(bounds);
                        for inset in [12., 8., 4.] {
                            let shadow = Bounds::from_corners(
                                frame.origin - point(px(inset), px(inset - 4.)),
                                frame.bottom_right() + point(px(inset), px(inset + 4.)),
                            );
                            window.paint_quad(fill(shadow, hexa(color::VIEWER_SHADOW, 0.22)));
                        }
                        window.with_content_mask(
                            Some(ContentMask {
                                bounds: frame.intersect(&bounds),
                            }),
                            |window| {
                                window.paint_quad(fill(frame, hex(color::BG)));
                                if checker {
                                    paint_checkerboard(frame, window, cx);
                                }
                                if let Some(image) = image {
                                    let _ = window.paint_image(
                                        frame,
                                        frame,
                                        Corners::default(),
                                        image,
                                        0,
                                        false,
                                    );
                                }
                                if safe {
                                    paint_safe_areas(frame, window, cx);
                                }
                                if grid {
                                    for fraction in [1. / 3., 2. / 3.] {
                                        line(
                                            point(
                                                frame.origin.x + frame.size.width * fraction,
                                                frame.origin.y,
                                            ),
                                            point(
                                                frame.origin.x + frame.size.width * fraction,
                                                frame.bottom_right().y,
                                            ),
                                            hexa(color::TEXT, 0.3),
                                            window,
                                        );
                                        line(
                                            point(
                                                frame.origin.x,
                                                frame.origin.y + frame.size.height * fraction,
                                            ),
                                            point(
                                                frame.bottom_right().x,
                                                frame.origin.y + frame.size.height * fraction,
                                            ),
                                            hexa(color::TEXT, 0.3),
                                            window,
                                        );
                                    }
                                }
                            },
                        );
                        window.paint_quad(outline(frame, hex(color::BORDER), BorderStyle::Solid));
                        if let Some((_, t)) = selected {
                            paint_gizmo(t, frame, dragging, window);
                        }
                    },
                )
                .size_full(),
            );
        let header = div()
            .h(px(38.))
            .px_3()
            .flex()
            .items_center()
            .gap_1()
            .bg(hex(color::PANEL))
            .border_b_1()
            .border_color(hex(color::BORDER))
            .child(
                icons::icon(IconName::Monitor)
                    .small()
                    .text_color(hex(color::TEXT_SECONDARY)),
            )
            .child(
                div()
                    .flex_1()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(px(11.))
                    .text_color(hex(color::TEXT_SECONDARY))
                    .child(format!(
                        "{} x {}  {:.0}%",
                        project_size(&self.project).0,
                        project_size(&self.project).1,
                        f64::from(f32::from(self.viewer_state.bounds.size.width))
                            / f64::from(project_size(&self.project).0.max(1))
                            * 100.
                    )),
            )
            .child(
                Button::new("viewer-fit")
                    .ghost()
                    .compact()
                    .label(match fit {
                        ViewerFit::Contain => "Fit",
                        ViewerFit::Fill => "Fill",
                        ViewerFit::Actual => "100%",
                    })
                    .dropdown_caret(true)
                    .tooltip("Viewer zoom")
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_viewer_menu(cx))),
            )
            .child(
                icon_button(
                    "safe-areas",
                    icons::icon(IconName::SquareDashed),
                    "Action safe / title safe",
                    cx.listener(|this, _, _, cx| this.toggle_safe_areas(cx)),
                )
                .selected(safe),
            )
            .child(
                icon_button(
                    "viewer-grid",
                    icons::icon(IconName::Grid3x3),
                    "Rule of thirds",
                    cx.listener(|this, _, _, cx| {
                        this.viewer_state.grid = !this.viewer_state.grid;
                        cx.notify();
                    }),
                )
                .selected(grid),
            )
            .child(
                icon_button(
                    "viewer-background",
                    icons::icon(IconName::Blend),
                    "Checkerboard / black background",
                    cx.listener(|this, _, _, cx| {
                        this.viewer_state.checker = !this.viewer_state.checker;
                        cx.notify();
                    }),
                )
                .selected(checker),
            );
        let mut root = div()
            .w_full()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .relative()
            .key_context("Viewer")
            .track_focus(&self.panel_focus[1])
            .text_color(hex(color::TEXT))
            .child(header)
            .child(viewport)
            .child(self.transport_controls(window, cx));
        if let Some((_, t)) = selected {
            root = root.child(
                div()
                    .absolute()
                    .bottom(px(66.))
                    .left(px(16.))
                    .px_2()
                    .py_1()
                    .rounded(px(6.))
                    .bg(hexa(color::RAISED, 0.95))
                    .border_1()
                    .border_color(hex(color::BORDER))
                    .text_size(px(11.))
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_color(hex(color::TEXT_SECONDARY))
                    .child(format!(
                        "X {:.3}   Y {:.3}   S {:.2} / {:.2}   R {:.1}°",
                        t.x,
                        t.y,
                        t.scale_x,
                        t.scale_y,
                        t.rotation.to_degrees()
                    )),
            );
        }
        if self.viewer_menu_open {
            root = root.child(self.viewer_menu(cx));
        }
        root
    }

    #[allow(clippy::unused_self)]
    pub(super) fn viewer_menu(&self, cx: &mut Context<Self>) -> Div {
        let mut menu = div()
            .absolute()
            .top(px(38.))
            .right(px(104.))
            .w(px(140.))
            .rounded(px(6.))
            .bg(hex(color::RAISED))
            .border_1()
            .border_color(hex(color::BORDER))
            .p_1()
            .flex()
            .flex_col()
            .shadow_lg();
        for (index, (fit, label)) in [
            (ViewerFit::Contain, "Fit canvas"),
            (ViewerFit::Fill, "Fill viewer"),
            (ViewerFit::Actual, "100% actual size"),
        ]
        .into_iter()
        .enumerate()
        {
            menu = menu.child(
                div()
                    .id(("viewer-fit-item", index))
                    .px_2()
                    .py_1()
                    .cursor_pointer()
                    .hover(|s| s.bg(hex(color::HOVER)))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| this.select_viewer_fit(fit, cx))),
            );
        }
        menu
    }
}

fn line(a: Point<Pixels>, b: Point<Pixels>, color: Hsla, window: &mut Window) {
    let mut path = gpui::PathBuilder::stroke(px(1.));
    path.move_to(a);
    path.line_to(b);
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}
fn paint_gizmo(t: Transform, canvas: Bounds<Pixels>, dragging: bool, window: &mut Window) {
    let points = handles(t, canvas);
    for (a, b) in [(0, 2), (2, 4), (4, 6), (6, 0), (1, 8)] {
        line(points[a].0, points[b].0, hex(color::ACCENT), window);
    }
    for (p, handle) in points {
        let bounds = Bounds::new(p - point(px(3.5), px(3.5)), size(px(7.), px(7.)));
        let quad = fill(bounds, hex(color::TEXT));
        window.paint_quad(if handle == Handle::Rotate {
            quad.corner_radii(px(4.))
        } else {
            quad
        });
        window.paint_quad(outline(bounds, hex(color::ACCENT), BorderStyle::Solid));
    }
    if !dragging {
        return;
    }
    // Accent guides appear when a translated edge/centre is snapped.
    let (sin, cos) = t.rotation.sin_cos();
    let w = f64::from(f32::from(canvas.size.width));
    let h = f64::from(f32::from(canvas.size.height));
    let ex = (t.scale_x * cos).abs() + (t.scale_y * h / w * sin).abs();
    let ey = (t.scale_y * cos).abs() + (t.scale_x * w / h * sin).abs();
    for guide in [0., 0.5, 1.] {
        if [t.x, t.x - ex / 2., t.x + ex / 2.]
            .iter()
            .any(|v| (v - guide).abs() < 0.0001)
        {
            line(
                canvas.origin + point(canvas.size.width * guide as f32, px(0.)),
                canvas.origin + point(canvas.size.width * guide as f32, canvas.size.height),
                hexa(color::ACCENT, 0.6),
                window,
            );
        }
        if [t.y, t.y - ey / 2., t.y + ey / 2.]
            .iter()
            .any(|v| (v - guide).abs() < 0.0001)
        {
            line(
                canvas.origin + point(px(0.), canvas.size.height * guide as f32),
                canvas.origin + point(canvas.size.width, canvas.size.height * guide as f32),
                hexa(color::ACCENT, 0.6),
                window,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[core::prelude::v1::test]
    fn pointer_hits_respect_stacking_bounds_audio_and_time() {
        let mut project = Project::default();
        for (track, clip_id, scale, kind) in [
            (1, 1, 1.0, ClipKind::Image(AssetId::of(b"back"))),
            (2, 2, 0.5, ClipKind::Image(AssetId::of(b"front"))),
            (3, 3, 1.0, ClipKind::Audio(AssetId::of(b"sound"))),
            (
                4,
                4,
                1.0,
                animathio_plugin_api::sources::text("Hi".into(), 0.1, [255; 4]),
            ),
        ] {
            Command::AddTrack {
                track: Track::new(TrackId(track), "T"),
                index: (track - 1) as usize,
            }
            .apply(&mut project)
            .unwrap();
            if let ClipKind::Image(id) | ClipKind::Audio(id) = &kind {
                Command::AddAsset {
                    id: id.clone(),
                    asset: animathio_core::Asset {
                        name: "a".into(),
                        media_type: "application/octet-stream".into(),
                    },
                }
                .apply(&mut project)
                .unwrap();
            }
            let mut clip = Clip::new(
                ClipId(clip_id),
                kind,
                animathio_core::TimeRange::new(Time(0), Time(TICKS_PER_SECOND)).unwrap(),
            );
            clip.transform.scale_x = scale;
            clip.transform.scale_y = scale;
            Command::AddClip {
                track: TrackId(track),
                clip,
            }
            .apply(&mut project)
            .unwrap();
        }
        let stage = Bounds::new(point(px(0.), px(0.)), size(px(800.), px(450.)));
        let ink = std::cell::Cell::new(true);
        let at = |x: f32, y: f32, time: Time| {
            clips_under_pointer(&project, time, stage, point(px(x), px(y)), &mut |_| {
                ink.get()
            })
        };
        // Centre: text on top, then both media clips, upper track first; audio never picks.
        assert_eq!(at(400., 225., Time(0)), [ClipId(4), ClipId(2), ClipId(1)]);
        // Transparent text pixels do not pick the text clip, media still does.
        ink.set(false);
        assert_eq!(at(400., 225., Time(0)), [ClipId(2), ClipId(1)]);
        // Outside the half-size clip only the large one remains.
        assert_eq!(at(50., 50., Time(0)), [ClipId(1)]);
        // Outside the canvas and after the clips end: nothing.
        assert_eq!(at(-5., 10., Time(0)).len(), 0);
        assert_eq!(at(400., 225., Time(2 * TICKS_PER_SECOND)).len(), 0);
    }

    #[core::prelude::v1::test]
    fn picking_selects_frontmost_cycles_and_clears_empty() {
        let hits = [ClipId(3), ClipId(2), ClipId(1)];
        assert_eq!(
            pick_from_stack(&hits, Some(ClipId(1)), false),
            Some(ClipId(3))
        );
        assert_eq!(
            pick_from_stack(&hits, Some(ClipId(3)), true),
            Some(ClipId(2))
        );
        assert_eq!(
            pick_from_stack(&hits, Some(ClipId(1)), true),
            Some(ClipId(3))
        );
        assert_eq!(
            pick_from_stack(&hits, Some(ClipId(99)), true),
            Some(ClipId(3))
        );
        assert_eq!(pick_from_stack(&[], Some(ClipId(1)), true), None);
    }
    #[core::prelude::v1::test]
    fn fit_and_rotated_hit_testing_share_canvas_coordinates() {
        let stage = Bounds::new(point(px(20.), px(30.)), size(px(800.), px(600.)));
        let frame = canvas_bounds(stage, (1920, 1080), ViewerFit::Contain);
        assert_eq!(frame.center(), stage.center());
        let t = Transform {
            scale_x: 0.2,
            scale_y: 0.3,
            rotation: 0.7,
            ..Transform::default()
        };
        assert_eq!(
            hit_handle(t, frame, local_point(t, frame, 0., 0.)),
            Some(Handle::Move)
        );
        assert_eq!(
            hit_handle(t, frame, local_point(t, frame, -0.5, -0.5)),
            Some(Handle::Scale(-0.5, -0.5))
        );
        assert_eq!(hit_handle(t, frame, stage.origin), None);
        assert!(contains_clip(t, frame, local_point(t, frame, 0.49, 0.49)));
        assert!(!contains_clip(t, frame, local_point(t, frame, 0.6, 0.)));
        assert!(contains_clip(
            Transform {
                scale_x: -t.scale_x,
                ..t
            },
            frame,
            frame.center()
        ));
    }
    #[core::prelude::v1::test]
    fn mirrored_scale_keeps_its_sign_and_never_reaches_zero() {
        let frame = Bounds::new(Point::default(), size(px(1000.), px(500.)));
        let drag = GizmoDrag {
            id: ClipId(1),
            origin: Point::default(),
            transform: Transform {
                scale_x: -0.4,
                scale_y: 0.4,
                ..Transform::default()
            },
            handle: Handle::Scale(0.5, 0.),
            rotation: std::cell::Cell::new(0.),
        };
        // The right handle of a mirrored clip sits at -0.2 of the canvas width;
        // dragging it 100 px (0.1) further left grows the mirrored width.
        let (t, _) = dragged_transform(&drag, frame, point(px(-100.), px(0.)), false, false);
        assert!((t.scale_x + 0.6).abs() < 1e-9, "{}", t.scale_x);
        // Dragging it past the centre flips the mirror instead of clamping at +0.001.
        let (t, _) = dragged_transform(&drag, frame, point(px(1000.), px(0.)), false, false);
        assert!((t.scale_x - 1.6).abs() < 1e-9, "{}", t.scale_x);
        // Landing within the dead zone keeps the sign of the dragged value.
        let (t, _) = dragged_transform(&drag, frame, point(px(199.8), px(0.)), false, false);
        assert_eq!(t.scale_x, -MIN_SCALE);
        let (t, _) = dragged_transform(&drag, frame, point(px(200.2), px(0.)), false, false);
        assert_eq!(t.scale_x, MIN_SCALE);
        // Shift keeps the aspect ratio of a mirrored clip, sign included.
        let (t, _) = dragged_transform(&drag, frame, point(px(-100.), px(0.)), true, false);
        assert!((t.scale_x / t.scale_y + 1.).abs() < 1e-9);
        assert!((t.scale_x + 0.6).abs() < 1e-9);
        assert!(min_magnitude(0.) > 0. && min_magnitude(-0.) < 0.);
    }

    #[core::prelude::v1::test]
    fn rotation_unwraps_across_the_negative_x_axis() {
        use std::f64::consts::{PI, TAU};
        assert!((unwrap_angle(0.5, 0.) - 0.5).abs() < 1e-12);
        // Raw atan2 differences jump by 2 pi at the -x axis; unwrapping continues.
        let start = 3. * PI / 4.; // pointer angle at drag start, near the -x axis
        let mut previous = 0.;
        let mut path = Vec::new();
        for step in 0..=24 {
            let true_delta = f64::from(step) * PI / 12.; // 0..2 pi, counter-clockwise
            let now = (start + true_delta).sin().atan2((start + true_delta).cos());
            let raw = now - start;
            previous = unwrap_angle(raw, previous);
            path.push(previous);
            assert!((previous - true_delta).abs() < 1e-9, "{step}: {previous}");
        }
        assert!(path.windows(2).all(|pair| pair[1] > pair[0]));
        assert!((path[24] - TAU).abs() < 1e-9);
    }

    #[core::prelude::v1::test]
    fn move_snaps_edges_and_shift_scale_preserves_aspect() {
        let frame = Bounds::new(Point::default(), size(px(1000.), px(500.)));
        let drag = GizmoDrag {
            id: ClipId(1),
            origin: Point::default(),
            transform: Transform {
                x: 0.3,
                scale_x: 0.2,
                scale_y: 0.4,
                ..Transform::default()
            },
            handle: Handle::Move,
            rotation: std::cell::Cell::new(0.),
        };
        let (t, guides) = dragged_transform(&drag, frame, point(px(99.), px(0.)), false, true);
        assert!((t.x - 0.4).abs() < 0.0001);
        assert_eq!(guides[0], Some(0.5));
        let drag = GizmoDrag {
            handle: Handle::Scale(0.5, 0.5),
            rotation: std::cell::Cell::new(0.),
            ..drag
        };
        let (t, _) = dragged_transform(&drag, frame, point(px(50.), px(10.)), true, false);
        assert!((t.scale_y / t.scale_x - 2.).abs() < 0.0001);
    }
}
