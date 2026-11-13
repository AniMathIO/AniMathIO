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
    let centre = local_point(t, canvas, 0., 0.);
    let dx = f64::from(f32::from(pointer.x - centre.x));
    let dy = f64::from(f32::from(pointer.y - centre.y));
    let (sin, cos) = t.rotation.sin_cos();
    ((dx * cos + dy * sin).abs()
        <= (t.scale_x * f64::from(f32::from(canvas.size.width)) / 2.).abs()
        && (-dx * sin + dy * cos).abs()
            <= (t.scale_y * f64::from(f32::from(canvas.size.height)) / 2.).abs())
    .then_some(Handle::Move)
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
            if x != 0. {
                t.scale_x = (t.scale_x + local_x / (w * x)).max(0.001);
            }
            if y != 0. {
                t.scale_y = (t.scale_y + local_y / (h * y)).max(0.001);
            }
            if shift {
                let factor = if x == 0. {
                    t.scale_y / drag.transform.scale_y.max(0.001)
                } else {
                    t.scale_x / drag.transform.scale_x.max(0.001)
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
            t.rotation += angle(pointer) - angle(drag.origin);
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
        };
        let (t, guides) = dragged_transform(&drag, frame, point(px(99.), px(0.)), false, true);
        assert!((t.x - 0.4).abs() < 0.0001);
        assert_eq!(guides[0], Some(0.5));
        let drag = GizmoDrag {
            handle: Handle::Scale(0.5, 0.5),
            ..drag
        };
        let (t, _) = dragged_transform(&drag, frame, point(px(50.), px(10.)), true, false);
        assert!((t.scale_y / t.scale_x - 2.).abs() < 0.0001);
    }
}
