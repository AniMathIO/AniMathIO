//! Shared pure helpers, drawing and resource adapters.

use super::*;
use crate::style::{self, color as colors};

/// Paint-ready waveform paths, shared by snapshots; bounded independently of PCM.
pub(super) type WavePathCache =
    std::rc::Rc<std::cell::RefCell<BTreeMap<(AssetId, [u64; 11]), gpui::Path<Pixels>>>>;

/// An icon-only button with a tooltip, using a Lucide (or bundled) SVG.
pub(super) fn icon_button(
    id: &'static str,
    icon: gpui_component::Icon,
    tooltip: impl Into<SharedString>,
    listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Button {
    let button = Button::new(id)
        .ghost()
        .compact()
        .icon(icon)
        .on_click(listener);
    let binding: Option<(Box<dyn Action>, &str)> = match id {
        "jump-start" => Some((Box::new(actions::GoToStart), "Viewer")),
        "step-back" => Some((Box::new(actions::PreviousFrame), "Viewer")),
        "play" => Some((Box::new(actions::TogglePlayback), "Viewer")),
        "step-fwd" => Some((Box::new(actions::NextFrame), "Viewer")),
        "jump-end" => Some((Box::new(actions::GoToEnd), "Viewer")),
        "loop" => Some((Box::new(actions::ToggleLoop), "Viewer")),
        "tool-select" => Some((Box::new(actions::SelectTool), "Timeline")),
        "tool-blade" => Some((Box::new(actions::BladeTool), "Timeline")),
        "tool-ripple" => Some((Box::new(actions::RippleTool), "Timeline")),
        "tool-hand" => Some((Box::new(actions::HandTool), "Timeline")),
        "mark-in" => Some((Box::new(actions::MarkIn), "Timeline")),
        "mark-out" => Some((Box::new(actions::MarkOut), "Timeline")),
        "zoom-fit" => Some((Box::new(actions::ZoomToFit), "Timeline")),
        "snap" => Some((Box::new(actions::ToggleSnapping), "Timeline")),
        "zoom-in" => Some((Box::new(actions::ZoomIn), "Timeline")),
        "zoom-out" => Some((Box::new(actions::ZoomOut), "Timeline")),
        "safe-areas" => Some((Box::new(actions::ToggleSafeAreas), "Viewer")),
        _ => None,
    };
    if let Some((action, context)) = binding {
        button.tooltip_with_action(tooltip, action.as_ref(), Some(context))
    } else {
        button.tooltip(tooltip)
    }
}

/// Compact toggle pill for track headers: muted icon when off, tinted with
/// `tint` (a style token) when active.
pub(super) fn header_pill(
    id: (&'static str, u64),
    icon: IconName,
    tooltip: &'static str,
    active: bool,
    tint: u32,
    listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let tint_color = style::hex(tint);
    div()
        .id(id)
        .w(px(22.))
        .h(px(20.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.))
        .cursor_pointer()
        .border_1()
        .border_color(if active {
            style::hexa(tint, 0.55)
        } else {
            gpui::transparent_black()
        })
        .bg(if active {
            style::hexa(tint, 0.20)
        } else {
            gpui::transparent_black()
        })
        .text_color(if active {
            tint_color
        } else {
            style::hex(colors::TEXT_SECONDARY)
        })
        .hover(|s| s.bg(style::hex(colors::HOVER)))
        .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tooltip).build(window, cx))
        .on_click(listener)
        .child(icons::icon(icon).xsmall())
}

pub(super) fn paint_timeline(
    bounds: Bounds<Pixels>,
    snapshot: &TimelineSnapshot,
    zoom: f32,
    scroll: f32,
    window: &mut Window,
    cx: &mut App,
) {
    let width = f32::from(bounds.size.width).max(1.0);
    let layout = TimelineLayout::zoomed(
        width,
        RULER_H,
        snapshot.row_height,
        snapshot.duration,
        zoom,
        scroll,
    );
    let background = style::hex(colors::PANEL);
    let ruler_bg = style::hex(colors::BG);
    let border = style::hex(colors::BORDER);
    let muted = style::hex(colors::TEXT_SECONDARY);
    window.paint_quad(fill(bounds, background));
    let height = f32::from(bounds.size.height);
    let ruler = Bounds::from_corners(
        bounds.origin,
        point(bounds.origin.x + px(width), bounds.origin.y + px(RULER_H)),
    );
    window.paint_quad(fill(ruler, ruler_bg));
    paint_ruler(bounds, &layout, snapshot, width, muted, border, window, cx);
    for row in 0..snapshot.rows {
        let top = bounds.origin.y + px(RULER_H + row as f32 * snapshot.row_height);
        let row_bounds = Bounds::from_corners(
            point(bounds.origin.x, top),
            point(bounds.origin.x + px(width), top + px(snapshot.row_height)),
        );
        window.paint_quad(fill(
            row_bounds,
            if row % 2 == 0 {
                style::hex(colors::RAISED)
            } else {
                background
            },
        ));
    }
    for row in 0..snapshot.rows {
        if !snapshot.overview.iter().any(|(r, _, _)| *r == row) {
            paint_clip_label(
                "Drop media here".into(),
                point(
                    bounds.left() + px(20.),
                    bounds.top() + px(RULER_H + row as f32 * snapshot.row_height + 16.),
                ),
                bounds.right(),
                font(snapshot.theme.font_family.clone()),
                window,
                cx,
            );
        }
    }
    for clip in &snapshot.clips {
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            paint_clip(bounds, &layout, clip, &snapshot.waveform_paths, window, cx);
        });
    }
    for transition in &snapshot.transitions {
        paint_transition(bounds, &layout, transition, window, cx);
    }
    if let Some((left, top, right, bottom)) = snapshot.marquee {
        let band = Bounds::from_corners(
            point(bounds.origin.x + px(left), bounds.origin.y + px(top)),
            point(bounds.origin.x + px(right), bounds.origin.y + px(bottom)),
        );
        window.paint_quad(fill(band, cx.theme().ring.opacity(0.18)));
        window.paint_quad(outline(band, cx.theme().ring, BorderStyle::Solid));
    }
    if snapshot.marks.marked()
        && let Some(range) = snapshot.marks.range(snapshot.duration)
    {
        let x0 = bounds.origin.x + px(timeline::time_to_x(&layout, range.start));
        let x1 = bounds.origin.x + px(timeline::time_to_x(&layout, range.end));
        window.paint_quad(fill(
            Bounds::from_corners(
                point(x0, bounds.origin.y),
                point(x1, bounds.origin.y + px(RULER_H)),
            ),
            style::hex(colors::ACCENT).opacity(0.25),
        ));
        for x in [x0, x1] {
            window.paint_quad(fill(
                Bounds::new(point(x, bounds.origin.y), size(px(1.), px(height))),
                style::hex(colors::ACCENT).opacity(0.5),
            ));
            window.paint_quad(fill(
                Bounds::new(point(x, bounds.origin.y), size(px(8.), px(6.))),
                style::hex(colors::ACCENT),
            ));
        }
    }
    let head_x = bounds.origin.x + px(timeline::time_to_x(&layout, snapshot.position));
    let playhead = style::hex(colors::PLAYHEAD);
    for (width, alpha) in [(7.0, 0.06), (3.0, 0.12), (1.0, 1.0)] {
        window.paint_quad(fill(
            Bounds::new(
                point(head_x - px(width / 2.), bounds.top()),
                size(px(width), px(height)),
            ),
            playhead.opacity(alpha),
        ));
    }
    let mut head = gpui::Path::new(point(head_x - px(6.), bounds.top()));
    for p in [
        point(head_x + px(6.), bounds.top()),
        point(head_x + px(6.), bounds.top() + px(8.)),
        point(head_x, bounds.top() + px(14.)),
        point(head_x - px(6.), bounds.top() + px(8.)),
        point(head_x - px(6.), bounds.top()),
    ] {
        head.line_to(p);
    }
    window.paint_path(head, playhead);
    if snapshot.scrubbing {
        let x = (head_x + px(10.))
            .min(bounds.right() - px(106.))
            .max(bounds.left());
        let pill = Bounds::new(point(x, bounds.top() + px(2.)), size(px(102.), px(20.)));
        window.paint_quad(
            fill(pill, style::hex(colors::RAISED)).corner_radii(px(style::radius::CONTROL)),
        );
        window.paint_quad(
            outline(pill, style::hexa(colors::PLAYHEAD, 0.6), BorderStyle::Solid)
                .corner_radii(px(style::radius::CONTROL)),
        );
        paint_clip_label(
            timecode::format_timecode(snapshot.position, snapshot.frame_rate).into(),
            pill.origin + point(px(7.), px(3.)),
            pill.right(),
            font(snapshot.theme.mono_font_family.clone()),
            window,
            cx,
        );
    }
    if let Some(time) = snapshot.snap_line {
        let x = bounds.origin.x + px(timeline::time_to_x(&layout, time));
        window.paint_quad(fill(
            Bounds::new(point(x, bounds.top()), size(px(1.), px(height))),
            style::hex(colors::ACCENT),
        ));
        paint_icon(
            IconName::Magnet,
            Bounds::new(
                point(x + px(3.), bounds.top() + px(RULER_H + 3.)),
                size(px(14.), px(14.)),
            ),
            style::hex(colors::ACCENT),
            window,
            cx,
        );
    }
}

/// Paint one clip block: kind-coloured body, waveform, fade ramps, outline, trim
/// handles, kind icon, label, lock badge and keyframe diamonds.
#[allow(clippy::too_many_lines)]
pub(super) fn paint_clip(
    bounds: Bounds<Pixels>,
    layout: &TimelineLayout,
    clip: &ClipPaint,
    waveform_paths: &WavePathCache,
    window: &mut Window,
    cx: &mut App,
) {
    let x0 = bounds.origin.x + px(timeline::time_to_x(layout, clip.start));
    let x1 = bounds.origin.x + px(timeline::time_to_x(layout, clip.end));
    let y0 = bounds.origin.y + px(RULER_H + clip.row as f32 * layout.row_height + 3.0);
    let y1 = bounds.origin.y + px(RULER_H + (clip.row as f32 + 1.0) * layout.row_height - 3.0);
    let block = Bounds::from_corners(point(x0, y0), point(x1, y1));
    let kind_color = clip_color(&clip.kind, cx.theme());
    if !timeline::visible_span(layout, clip.start, clip.end) {
        return;
    }
    let dim = if clip.silenced { 0.5 } else { 1.0 };
    let radius = px(style::radius::CLIP);
    if clip.selected {
        for (spread, alpha) in [(4.0, 0.05), (2.0, 0.12)] {
            let glow = Bounds::from_corners(
                point(x0 - px(spread), y0 - px(spread)),
                point(x1 + px(spread), y1 + px(spread)),
            );
            window.paint_quad(
                fill(glow, style::hexa(colors::ACCENT, alpha)).corner_radii(radius + px(spread)),
            );
        }
    }
    window.paint_quad(fill(block, style::hex(colors::RAISED)).corner_radii(radius));
    window.paint_quad(
        fill(
            block,
            kind_color.opacity(if clip.hovered { 0.43 * dim } else { 0.30 * dim }),
        )
        .corner_radii(radius),
    );
    window.paint_quad(
        outline(block, kind_color.opacity(0.45 * dim), BorderStyle::Solid).corner_radii(radius),
    );
    let highlight = Bounds::from_corners(point(x0 + px(5.), y0), point(x1 - px(5.), y0 + px(1.)));
    if x1 - x0 > px(10.) {
        window.paint_quad(fill(highlight, kind_color.opacity(0.65)));
    }
    window.paint_quad(fill(
        Bounds::from_corners(point(x0, y0 + px(5.)), point(x0 + px(3.), y1 - px(5.))),
        kind_color,
    ));
    paint_filmstrip(bounds, clip, (x0, x1, y0, y1), window);
    if let Some(wave) = &clip.waveform {
        paint_waveform(
            bounds,
            layout,
            clip,
            wave,
            (x0, x1, y0, y1),
            waveform_paths,
            window,
            cx,
        );
    }
    paint_fades(clip, (x0, x1, y0, y1), window, cx);
    // Labels stay readable while the clip start scrolls off the left edge.
    let lx = x0.max(bounds.left());
    if !clip.preview.is_empty() && y1 - y0 > px(38.) {
        paint_icon(
            clip.icon,
            Bounds::new(point(lx + px(12.), y0 + px(25.)), size(px(18.), px(18.))),
            kind_color.opacity(0.7),
            window,
            cx,
        );
        paint_clip_label(
            clip.preview.clone(),
            point(lx + px(38.), y0 + px(27.)),
            x1 - px(8.),
            font(cx.theme().font_family.clone()),
            window,
            cx,
        );
    }
    if clip.selected {
        window.paint_quad(
            outline(block, style::hex(colors::ACCENT), BorderStyle::Solid).corner_radii(radius),
        );
        window.paint_quad(
            outline(
                Bounds::from_corners(
                    point(x0 + px(1.0), y0 + px(1.0)),
                    point(x1 - px(1.0), y1 - px(1.0)),
                ),
                style::hex(colors::ACCENT),
                BorderStyle::Solid,
            )
            .corner_radii((radius - px(1.)).max(px(0.))),
        );
    }
    // Trim handles only on the hovered or selected clip.
    if clip.selected || clip.hovered {
        let handle_w = px(3.0);
        for edge_x in [x0 + px(2.), x1 - px(2.)] {
            let handle = Bounds::from_corners(
                point(edge_x - handle_w / 2., (y0 + y1) / 2. - px(5.)),
                point(edge_x + handle_w / 2., (y0 + y1) / 2. + px(5.)),
            );
            window.paint_quad(
                fill(handle, cx.theme().foreground.opacity(0.85)).corner_radii(px(1.5)),
            );
        }
    }
    // Kind icon and label, only when the block is wide enough to hold them.
    if x1 - x0 > px(28.0) {
        let icon_bounds = Bounds::new(point(lx + px(7.0), y0 + px(3.0)), size(px(12.0), px(12.0)));
        paint_icon(clip.icon, icon_bounds, cx.theme().foreground, window, cx);
        if !clip.label.is_empty() {
            paint_clip_label(
                clip.label.clone(),
                point(lx + px(22.0), y0 + px(3.0)),
                x1 - px(4.0),
                font(cx.theme().font_family.clone()),
                window,
                cx,
            );
        }
    }
    if clip.locked && x1 - x0 > px(48.0) {
        let lock = Bounds::new(
            point(x1 - px(18.0), y1 - px(15.0)),
            size(px(12.0), px(12.0)),
        );
        paint_icon(IconName::Lock, lock, cx.theme().foreground, window, cx);
    }
    // Keyframe diamonds: rotated squares with a dark rim so they read over
    // thumbnails and waveforms.
    let diamond = |kx: Pixels, cy: Pixels, half: Pixels| {
        let mut path = gpui::Path::new(point(kx, cy - half));
        path.line_to(point(kx + half, cy));
        path.line_to(point(kx, cy + half));
        path.line_to(point(kx - half, cy));
        path.line_to(point(kx, cy - half));
        path
    };
    for keyframe in &clip.keyframes {
        let kx = bounds.origin.x + px(timeline::time_to_x(layout, keyframe.time));
        let cy = y1 - px(10.0);
        window.paint_path(diamond(kx, cy, px(6.0)), style::hexa(colors::BG, 0.85));
        window.paint_path(
            diamond(kx, cy, px(4.5)),
            if keyframe.selected {
                style::hex(colors::TEXT)
            } else {
                style::hex(colors::ACCENT)
            },
        );
    }
}

/// Content labels are kept here so plugin migration touches one small match.
pub(super) fn timeline_content_preview(clip: &Clip) -> String {
    match &clip.kind {
        ClipKind::Vector(_) => "Vector shape".into(),
        ClipKind::Plugin { plugin_id, .. } => ["text", "source", "latex", "expression"]
            .iter()
            .find_map(|key| {
                clip.params.get(*key).and_then(|value| {
                    if let animathio_core::ParamValue::String(text) = value {
                        Some(
                            text.lines()
                                .next()
                                .unwrap_or("")
                                .chars()
                                .take(100)
                                .collect(),
                        )
                    } else {
                        None
                    }
                })
            })
            .unwrap_or_else(|| {
                if plugin_id.ends_with("shapes") {
                    "Vector shape".into()
                } else {
                    String::new()
                }
            }),
        _ => String::new(),
    }
}

fn paint_filmstrip(
    bounds: Bounds<Pixels>,
    clip: &ClipPaint,
    (x0, x1, y0, y1): (Pixels, Pixels, Pixels, Pixels),
    window: &mut Window,
) {
    if clip.thumbnails.is_empty() || y1 - y0 < px(30.) {
        return;
    }
    let top = y0 + px(20.);
    let height = y1 - top - px(3.);
    let tile_width = (height * (16. / 9.)).max(px(32.));
    let left = x0 + px(5.);
    let first = (f32::from(bounds.left() - left) / f32::from(tile_width))
        .floor()
        .max(0.) as usize;
    let count = (f32::from(bounds.size.width) / f32::from(tile_width)).ceil() as usize + 2;
    let mask = Bounds::from_corners(
        point(left.max(bounds.left()), top),
        point(x1.min(bounds.right()) - px(3.), y1 - px(3.)),
    );
    if mask.size.width <= px(0.) {
        return;
    }
    for index in first..first + count {
        let x = left + tile_width * index as f32;
        if x >= x1 {
            break;
        }
        let fraction = f32::from(x - left) / f32::from(x1 - left).max(1.);
        let slot =
            ((fraction * clip.thumbnails.len() as f32) as usize).min(clip.thumbnails.len() - 1);
        if let Some(image) = &clip.thumbnails[slot] {
            let tile = Bounds::new(point(x, top), size(tile_width - px(1.), height));
            let _ = window.paint_image(mask, tile, Corners::all(px(2.)), image.clone(), 0, false);
            window.paint_quad(fill(tile.intersect(&mask), style::hexa(colors::BG, 0.18)));
        }
    }
}

fn paint_transition(
    bounds: Bounds<Pixels>,
    layout: &TimelineLayout,
    transition: &TransitionPaint,
    window: &mut Window,
    cx: &mut App,
) {
    if !timeline::visible_span(layout, transition.range.start, transition.range.end) {
        return;
    }
    let left = (bounds.left() + px(timeline::time_to_x(layout, transition.range.start)))
        .max(bounds.left());
    let right =
        (bounds.left() + px(timeline::time_to_x(layout, transition.range.end))).min(bounds.right());
    let top = bounds.top() + px(RULER_H + transition.row as f32 * layout.row_height + 3.);
    let bottom = top + px((layout.row_height - 6.).max(8.));
    let area = Bounds::from_corners(point(left, top), point(right, bottom));
    // Outgoing-to-incoming gradient over the overlap, with a soft accent rim.
    window.paint_quad(
        fill(
            area,
            gpui::linear_gradient(
                90.,
                gpui::linear_color_stop(style::hexa(colors::ACCENT_FROM, 0.55), 0.),
                gpui::linear_color_stop(style::hexa(colors::ACCENT_TO, 0.55), 1.),
            ),
        )
        .corner_radii(px(4.)),
    );
    window.paint_quad(
        outline(
            area,
            style::hexa(colors::ACCENT_TO, 0.7),
            BorderStyle::Solid,
        )
        .corner_radii(px(4.)),
    );
    if right - left > px(18.) {
        let icon = Bounds::new(
            point((left + right) / 2. - px(7.), (top + bottom) / 2. - px(7.)),
            size(px(14.), px(14.)),
        );
        window.paint_quad(
            fill(
                Bounds::new(icon.origin - point(px(3.), px(3.)), size(px(20.), px(20.))),
                style::hex(colors::RAISED),
            )
            .corner_radii(px(4.)),
        );
        paint_icon(IconName::Blend, icon, style::hex(colors::TEXT), window, cx);
    }
}

pub(super) fn paint_overview(
    bounds: Bounds<Pixels>,
    clips: &[(usize, TimeRange, Hsla)],
    rows: usize,
    duration: i64,
    zoom: f32,
    scroll: f32,
    window: &mut Window,
) {
    let ppt = f32::from(bounds.size.width) / duration.max(1) as f32;
    let lane = 14. / rows.max(1) as f32;
    for (row, range, color) in clips {
        let area = Bounds::from_corners(
            point(
                bounds.left() + px(range.start.0 as f32 * ppt),
                bounds.top() + px(4. + *row as f32 * lane),
            ),
            point(
                bounds.left() + px(range.end.0 as f32 * ppt),
                bounds.top() + px(4. + (*row + 1) as f32 * lane - 0.5),
            ),
        );
        window.paint_quad(fill(area, color.opacity(0.55)).corner_radii(px(1.)));
    }
    let view = Bounds::new(
        point(bounds.left() + px(scroll / zoom), bounds.top() + px(2.)),
        size(bounds.size.width / zoom, bounds.size.height - px(4.)),
    );
    window.paint_quad(fill(view, style::hexa(colors::ACCENT, 0.10)).corner_radii(px(3.)));
    window.paint_quad(
        outline(view, style::hexa(colors::ACCENT, 0.75), BorderStyle::Solid).corner_radii(px(3.)),
    );
}

/// Paint a Lucide icon (from the registered asset source) into `bounds`.
pub(super) fn paint_icon(
    icon: IconName,
    bounds: Bounds<Pixels>,
    color: Hsla,
    window: &mut Window,
    cx: &mut App,
) {
    let _ = window.paint_svg(
        bounds,
        icon.path(),
        None,
        TransformationMatrix::default(),
        color,
        cx,
    );
}

/// Mirrored peak bars across the visible part of an audio clip, one every two
/// pixels, sampled from the cached downsampled peaks.
pub(super) fn paint_waveform(
    bounds: Bounds<Pixels>,
    layout: &TimelineLayout,
    clip: &ClipPaint,
    wave: &Waveform,
    (x0, x1, y0, y1): (Pixels, Pixels, Pixels, Pixels),
    cache: &WavePathCache,
    window: &mut Window,
    cx: &mut App,
) {
    const STEP: f32 = 3.0;
    let left = f32::from(x0 + px(4.)).max(f32::from(bounds.left()));
    let right = f32::from(x1 - px(2.)).min(f32::from(bounds.right()));
    // Short rows have no room for a header strip: the waveform fills the clip
    // and sits quietly behind the label.
    let compact = y1 - y0 < px(36.);
    let top = if compact { y0 + px(2.) } else { y0 + px(20.) };
    let mid = f32::from(top) + f32::from(y1 - top) / 2.;
    let half_max = (f32::from(y1 - top) - if compact { 6. } else { 4. }) / 2.;
    let alpha = if compact { 0.45 } else { 0.8 };
    if right <= left || half_max <= 1. {
        return;
    }
    let ClipKind::Audio(asset) = &clip.kind else {
        return;
    };
    let key = (
        asset.clone(),
        [
            clip.source_offset as u64,
            clip.start.0 as u64,
            clip.end.0 as u64,
            layout.pixels_per_tick.to_bits(),
            layout.scroll_x.to_bits(),
            u64::from(f32::from(bounds.left()).to_bits()),
            u64::from(f32::from(bounds.top()).to_bits()),
            clip.row as u64,
            u64::from(layout.row_height.to_bits()),
            u64::from(layout.width.to_bits()),
            wave.duration_ticks as u64,
        ],
    );
    if let Some(path) = cache.borrow().get(&key) {
        window.paint_path(
            path.clone(),
            clip_color(&clip.kind, cx.theme()).opacity(alpha),
        );
        return;
    }
    let peak_at = |x: f32| {
        let start =
            (f64::from(x - f32::from(x0)) / layout.pixels_per_tick) as i64 + clip.source_offset;
        let end = start.saturating_add((f64::from(STEP) / layout.pixels_per_tick).ceil() as i64);
        (wave.peak_between(start, end.max(start + 1)) * half_max).max(0.5)
    };
    let steps = ((right - left) / STEP).ceil() as usize;
    // PathBuilder tessellates concave polygons; `gpui::Path::line_to` only fans
    // triangles from the first vertex, which smears a waveform outline.
    let columns: Vec<(f32, f32)> = (0..=steps)
        .map(|index| {
            let x = (left + index as f32 * STEP).min(right);
            (x, peak_at(x))
        })
        .collect();
    let mut points: Vec<Point<Pixels>> = columns
        .iter()
        .map(|(x, half)| point(px(*x), px(mid - half)))
        .collect();
    points.extend(
        columns
            .iter()
            .rev()
            .map(|(x, half)| point(px(*x), px(mid + half))),
    );
    let mut builder = gpui::PathBuilder::fill();
    builder.add_polygon(&points, true);
    let Ok(path) = builder.build() else {
        return;
    };
    let mut paths = cache.borrow_mut();
    if paths.len() >= 128 {
        paths.pop_first();
    }
    paths.insert(key, path.clone());
    window.paint_path(path, clip_color(&clip.kind, cx.theme()).opacity(alpha));
}

/// Polyline samples per fade curve.
const FADE_SAMPLES: usize = 20;

/// Fade ramps (a darkened triangle over the attenuated region) and, on the
/// hovered or selected clip, the circular fade handles.
pub(super) fn paint_fades(
    clip: &ClipPaint,
    (x0, x1, y0, y1): (Pixels, Pixels, Pixels, Pixels),
    window: &mut Window,
    cx: &mut App,
) {
    let width = f32::from(x1 - x0);
    if width < 40.0 {
        return;
    }
    let shade = style::hexa(colors::BG, 0.42);
    let ppt = width / (clip.end.0 - clip.start.0).max(1) as f32;
    let fade_in_px = (clip.fade_in as f32 * ppt).min(width);
    let fade_out_px = (clip.fade_out as f32 * ppt).min(width);
    // Level curve of a fade: a smoothstep from the clip edge (silent) to full
    // level. The attenuated area above it is shaded; the curve itself is stroked.
    // Sampled as a polyline so the fill stays exact (curve segments in GPUI
    // paths only approximate concave fills).
    for (length, left) in [(fade_in_px, true), (fade_out_px, false)] {
        if length <= 1.0 {
            continue;
        }
        let edge = if left { x0 } else { x1 };
        let sign = if left { 1.0 } else { -1.0 };
        let curve: Vec<Point<Pixels>> = (0..=FADE_SAMPLES)
            .map(|i| {
                let t = i as f32 / FADE_SAMPLES as f32;
                let ease = t * t * (3.0 - 2.0 * t);
                point(edge + px(sign * length * t), y1 - (y1 - y0) * ease)
            })
            .collect();
        let mut area = gpui::PathBuilder::fill();
        area.move_to(point(edge, y0));
        area.line_to(point(edge + px(sign * length), y0));
        for p in curve.iter().rev() {
            area.line_to(*p);
        }
        area.close();
        if let Ok(path) = area.build() {
            window.paint_path(path, shade);
        }
        let mut line = gpui::PathBuilder::stroke(px(1.25));
        line.move_to(curve[0]);
        for p in &curve[1..] {
            line.line_to(*p);
        }
        if let Ok(path) = line.build() {
            window.paint_path(path, style::hexa(colors::TEXT, 0.7));
        }
    }
    if clip.selected || clip.hovered {
        let radius = px(4.0);
        for (cx_pos, active) in [
            (x0 + px(fade_in_px + FADE_HANDLE_INSET), clip.fade_in > 0),
            (x1 - px(fade_out_px + FADE_HANDLE_INSET), clip.fade_out > 0),
        ] {
            let handle = Bounds::from_corners(
                point(cx_pos - radius, y0 + px(FADE_HANDLE_INSET) - radius),
                point(cx_pos + radius, y0 + px(FADE_HANDLE_INSET) + radius),
            );
            window.paint_quad(
                fill(
                    handle,
                    if active {
                        style::hex(colors::ACCENT)
                    } else {
                        cx.theme().foreground
                    },
                )
                .corner_radii(radius),
            );
        }
    }
}

/// Paint timecode ticks and labels on the ruler, adapting the interval to the zoom.
#[allow(clippy::too_many_arguments)]
pub(super) fn paint_ruler(
    bounds: Bounds<Pixels>,
    layout: &TimelineLayout,
    snapshot: &TimelineSnapshot,
    width: f32,
    muted: Hsla,
    border: Hsla,
    window: &mut Window,
    cx: &mut App,
) {
    let step = ruler_step(
        snapshot.duration,
        layout.pixels_per_tick,
        snapshot.frame_rate,
    );
    let minor = (step / 4).max(1);
    for tick in timeline::ruler_ticks(layout, minor) {
        let x = bounds.origin.x + px(timeline::time_to_x(layout, Time(tick)));
        let is_major = tick % step == 0;
        let mark_top = bounds.origin.y + px(if is_major { 19.0 } else { 24.0 });
        let mark = Bounds::from_corners(
            point(x, mark_top),
            point(x + px(1.0), bounds.origin.y + px(RULER_H)),
        );
        window.paint_quad(fill(mark, if is_major { muted } else { border }));
        if is_major {
            let label = if step >= TICKS_PER_SECOND {
                let seconds = tick / TICKS_PER_SECOND;
                format!("{:02}:{:02}", seconds / 60, seconds % 60)
            } else {
                timecode::format_timecode(Time(tick), snapshot.frame_rate)
            };
            paint_clip_label(
                label.into(),
                point(x + px(5.0), bounds.origin.y + px(3.0)),
                x + px(width),
                font(snapshot.theme.mono_font_family.clone()),
                window,
                cx,
            );
        }
    }
}

/// Choose a "nice" ruler interval so major ticks stay at least 64px apart.
pub(super) fn ruler_step(duration: i64, pixels_per_tick: f64, rate: FrameRate) -> i64 {
    let tps = TICKS_PER_SECOND as f64;
    let fps = f64::from(timecode::frames_per_second(rate)).max(1.0);
    let frame = (tps / fps).max(1.0);
    let candidates = [
        frame,
        frame * 2.0,
        frame * 5.0,
        frame * 10.0,
        tps / 2.0,
        tps,
        tps * 2.0,
        tps * 5.0,
        tps * 10.0,
        tps * 30.0,
        tps * 60.0,
        tps * 120.0,
        tps * 300.0,
    ];
    let step = candidates
        .into_iter()
        .find(|candidate| candidate * pixels_per_tick >= 96.0)
        .unwrap_or(tps * 300.0);
    (step as i64).clamp(1, duration.max(1))
}

/// A source-kind colour from the design tokens.
pub(super) fn clip_color(kind: &animathio_core::ClipKind, _theme: &gpui_component::Theme) -> Hsla {
    style::hex(match kind {
        ClipKind::Video(_) => colors::CLIP_VIDEO,
        ClipKind::Image(_) => colors::CLIP_IMAGE,
        ClipKind::Audio(_) => colors::CLIP_AUDIO,
        ClipKind::Plugin { plugin_id, .. } => {
            if plugin_id == "org.animathio.text" || plugin_id == "text" {
                colors::CLIP_TEXT
            } else if plugin_id == "org.animathio.shapes" || plugin_id == "shapes" {
                colors::CLIP_SHAPE
            } else if plugin_id.contains("latex") || plugin_id.contains("math") {
                colors::CLIP_LATEX
            } else if plugin_id.contains("graph") {
                colors::CLIP_GRAPH
            } else {
                colors::CLIP_PLUGIN
            }
        }
        ClipKind::Vector(_) => colors::CLIP_SHAPE,
        ClipKind::Error(_) => colors::CLIP_ERROR,
    })
}

/// Paint a single line of clip text, clipped to the right edge.
pub(super) fn paint_clip_label(
    label: SharedString,
    origin: Point<Pixels>,
    right_edge: Pixels,
    label_font: Font,
    window: &mut Window,
    cx: &mut App,
) {
    if label.is_empty() {
        return;
    }
    let run = TextRun {
        len: label.len(),
        font: label_font,
        color: style::hex(colors::TEXT),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window
        .text_system()
        .shape_line(label, px(11.), &[run], None);
    let line_height = px(14.0);
    // Clip the text to the block so long labels never spill into the next clip.
    let mask = ContentMask {
        bounds: Bounds::from_corners(
            origin,
            point(right_edge.max(origin.x), origin.y + line_height),
        ),
    };
    window.with_content_mask(Some(mask), |window| {
        let _ = line.paint(origin, line_height, TextAlign::Left, None, window, cx);
    });
}

pub(super) fn field_value(clip: &Clip, field: &Field) -> f64 {
    match field {
        Field::Transform(property) => match property {
            TransformProperty::X => clip.transform.x,
            TransformProperty::Y => clip.transform.y,
            TransformProperty::ScaleX => clip.transform.scale_x,
            TransformProperty::ScaleY => clip.transform.scale_y,
            TransformProperty::Rotation => clip.transform.rotation,
            TransformProperty::Opacity => clip.transform.opacity,
        },
        Field::Param(name) => clip
            .params
            .get(name)
            .and_then(animathio_core::ParamValue::as_number)
            .unwrap_or(0.0),
    }
}

pub(super) fn with_transform(
    mut transform: Transform,
    property: TransformProperty,
    value: f64,
) -> Transform {
    match property {
        TransformProperty::X => transform.x = value,
        TransformProperty::Y => transform.y = value,
        TransformProperty::ScaleX => transform.scale_x = value,
        TransformProperty::ScaleY => transform.scale_y = value,
        TransformProperty::Rotation => transform.rotation = value,
        TransformProperty::Opacity => transform.opacity = value,
    }
    transform
}

pub(super) fn field_step(field: &Field) -> f64 {
    match field {
        Field::Transform(TransformProperty::Rotation | TransformProperty::Opacity) => 0.05,
        Field::Transform(_) => 0.01,
        Field::Param(_) => 0.1,
    }
}

pub(super) fn frame_rate_label(rate: FrameRate) -> String {
    if rate.denominator() == 1 {
        format!("{} fps", rate.numerator())
    } else {
        format!(
            "{:.3} fps",
            f64::from(rate.numerator()) / f64::from(rate.denominator())
        )
    }
}

pub(super) fn kind_label(kind: &ClipKind) -> String {
    match kind {
        ClipKind::Video(_) => "video".into(),
        ClipKind::Image(_) => "image".into(),
        ClipKind::Audio(_) => "audio".into(),
        ClipKind::Plugin { plugin_id, .. } => match plugin_id.as_str() {
            "org.animathio.text" => "text".into(),
            "org.animathio.shapes" => "shape".into(),
            _ => format!("plugin {plugin_id}"),
        },
        ClipKind::Vector(_) => "vector".into(),
        ClipKind::Error(_) => "error".into(),
    }
}

/// Human-readable media-pool label for an asset's declared media type.
pub(super) fn asset_kind_label(media_type: &str) -> &'static str {
    if media_type.starts_with("video/") {
        "video"
    } else if media_type.starts_with("image/") {
        "image"
    } else if media_type.starts_with("audio/") {
        "audio"
    } else {
        "asset"
    }
}

/// A themed context-menu item.
pub(super) fn context_item(
    icon: IconName,
    label: &str,
    listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut Context<Editor>,
) -> Stateful<Div> {
    div()
        .px_2()
        .py_1()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .cursor_pointer()
        .hover(|this| this.bg(cx.theme().list_hover))
        .child(icons::icon(icon).small())
        .child(label.to_owned())
        .id(SharedString::from(format!("context-item-{label}")))
        .on_click(listener)
}

/// Dim placeholder text used for empty panels and unloaded viewports.
pub(super) fn empty_state(label: &'static str, cx: &App) -> Div {
    div()
        .w_full()
        .py_4()
        .flex()
        .justify_center()
        .text_color(cx.theme().muted_foreground)
        .child(label)
}

/// Paint a subtle checkerboard behind transparent viewer regions.
pub(super) fn paint_checkerboard(bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let cell = 8.0;
    let mut y = 0.0;
    let mut row = 0;
    while y < f32::from(bounds.size.height) {
        let mut x = 0.0;
        let mut col = row;
        while x < f32::from(bounds.size.width) {
            let cell_bounds = Bounds::from_corners(
                point(bounds.origin.x + px(x), bounds.origin.y + px(y)),
                point(
                    bounds.origin.x + px((x + cell).min(f32::from(bounds.size.width))),
                    bounds.origin.y + px((y + cell).min(f32::from(bounds.size.height))),
                ),
            );
            window.paint_quad(fill(
                cell_bounds,
                if col % 2 == 0 {
                    cx.theme().background
                } else {
                    cx.theme().muted
                },
            ));
            x += cell;
            col += 1;
        }
        y += cell;
        row += 1;
    }
}

/// Paint title-safe and action-safe guides over the viewer.
pub(super) fn paint_safe_areas(bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let guide = cx.theme().border;
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    let origin = bounds.origin;
    let mut paint_frame = |inset: f32| {
        let left = origin.x + px(width * inset);
        let top = origin.y + px(height * inset);
        let frame_bounds = Bounds::from_corners(
            point(left, top),
            point(
                origin.x + px(width * (1.0 - inset)),
                origin.y + px(height * (1.0 - inset)),
            ),
        );
        window.paint_quad(outline(frame_bounds, guide, BorderStyle::Solid));
    };
    paint_frame(0.05); // action-safe
    paint_frame(0.10); // title-safe
}

/// The locked track that a clip-mutating command would edit, if any.
pub(super) fn clip_label(project: &Project, clip: &Clip) -> String {
    let kind = match &clip.kind {
        ClipKind::Video(id) | ClipKind::Image(id) | ClipKind::Audio(id) => {
            project.assets().get(id).map(|a| a.name.clone())
        }
        _ => None,
    };
    kind.unwrap_or_else(|| kind_label(&clip.kind))
}

/// Clip title: the asset or kind name, or (on short rows without a body
/// preview) the text snippet itself so text clips stay identifiable.
pub(super) fn clip_title(project: &Project, clip: &Clip, row_height: f32) -> String {
    if row_height <= 44.0 {
        let snippet = timeline_content_preview(clip);
        if !snippet.is_empty() {
            return snippet.chars().take(48).collect();
        }
    }
    clip_label(project, clip)
}

pub(super) fn audio_asset_ids(project: &Project) -> BTreeSet<AssetId> {
    project
        .tracks()
        .iter()
        .flat_map(|t| t.clips().values())
        .filter_map(|c| match &c.kind {
            ClipKind::Audio(id) => Some(id.clone()),
            _ => None,
        })
        .collect()
}

pub(super) fn plugins_cache_dir() -> PathBuf {
    directories::BaseDirs::new().map_or_else(
        || std::env::temp_dir().join("animathio-plugin-cache"),
        |base| base.cache_dir().join("animathio").join("plugin-cache"),
    )
}

/// Load every clip-source plugin from the bundled build output and the user
/// plugins directory. Descriptors are plain data for the UI; the host itself is
/// kept on the UI thread and drives both preview and control rendering.
pub(crate) fn load_plugins() -> (Option<PluginHost>, Vec<PluginDescriptor>, Option<String>) {
    let mut host = match PluginHost::new(plugins_cache_dir()) {
        Ok(host) => host,
        Err(error) => {
            return (
                None,
                Vec::new(),
                Some(format!("plugin host unavailable: {error}")),
            );
        }
    };
    let problems = host.load_discovered_plugins();
    for problem in &problems {
        eprintln!("plugin load problem: {problem}");
    }
    // Preview keeps placeholder boxes for unresolved plugins; say why. Export fails
    // for such clips instead of writing the placeholders.
    let error = problems
        .first()
        .map(|first| format!("plugin load failed: {first} (text and shapes may not render)"));
    let mut descriptors = Vec::new();
    for id in host.loaded_plugin_ids() {
        let Some(plugin) = host.get_plugin(&id) else {
            continue;
        };
        if plugin.manifest.kinds.contains(&PluginKind::ClipSource) {
            descriptors.push(PluginDescriptor {
                id: id.clone(),
                name: plugin.manifest.name.clone(),
                schema: plugin.inspector_schema.clone(),
            });
        }
    }
    descriptors.sort_by(|left, right| left.name.cmp(&right.name));
    (Some(host), descriptors, error)
}

pub(super) fn load_audio(
    pool: &mut AudioPool,
    project: &Project,
    assets: &BTreeMap<AssetId, Vec<u8>>,
) -> Result<(), String> {
    for id in audio_asset_ids(project) {
        if pool.contains(&id) {
            continue;
        }
        let bytes = assets
            .get(&id)
            .ok_or_else(|| format!("audio asset missing: {}", id.as_str()))?;
        if let Ok(source) = SymphoniaSource::from_bytes(bytes.clone()) {
            pool.insert(id, &source).map_err(|e| e.to_string())?;
        } else {
            // An undecodable asset plays silence instead of aborting the mix.
            let silent = Pcm::new(48_000, 1, vec![0.0]).map_err(|e| e.to_string())?;
            pool.insert(id, &silent).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub(super) fn frames_to_ticks(frames: usize, rate: u32) -> i64 {
    let ticks = u128::from(rate.max(1));
    i64::try_from(frames as u128 * TICKS_PER_SECOND as u128 / ticks).unwrap_or(i64::MAX)
}

pub(super) fn parse_fps(value: &str) -> Result<FrameRate, String> {
    if let Some((numerator, denominator)) = value.split_once('/') {
        return FrameRate::new(
            numerator.parse().map_err(|_| "invalid numerator")?,
            denominator.parse().map_err(|_| "invalid denominator")?,
        )
        .map_err(|e| e.0);
    }

    let fps = value
        .parse::<f64>()
        .map_err(|_| "invalid frame rate".to_owned())?;
    if !fps.is_finite() || fps <= 0.0 {
        return Err("invalid frame rate".into());
    }
    // Integer rates keep denominator 1; fractional rates use the NTSC 1001
    // denominator so 23.976/29.97/59.94 round-trip exactly.
    let (numerator, denominator) = if fps.fract() == 0.0 {
        (fps as u32, 1)
    } else {
        ((fps * 1001.0).round() as u32, 1001)
    };
    FrameRate::new(numerator.max(1), denominator).map_err(|e| e.0)
}

pub(crate) fn color_from_json(value: &Value) -> Option<[u8; 4]> {
    if let Some(array) = value.as_array()
        && array.len() >= 3
    {
        return Some([
            array.first().and_then(Value::as_u64).unwrap_or(0) as u8,
            array.get(1).and_then(Value::as_u64).unwrap_or(0) as u8,
            array.get(2).and_then(Value::as_u64).unwrap_or(0) as u8,
            array.get(3).and_then(Value::as_u64).unwrap_or(255) as u8,
        ]);
    }
    if let Some(object) = value.as_object() {
        return Some([
            object.get("r").and_then(Value::as_u64).unwrap_or(0) as u8,
            object.get("g").and_then(Value::as_u64).unwrap_or(0) as u8,
            object.get("b").and_then(Value::as_u64).unwrap_or(0) as u8,
            object.get("a").and_then(Value::as_u64).unwrap_or(255) as u8,
        ]);
    }
    None
}

pub(crate) fn point_from_json(value: &Value) -> Option<[f32; 2]> {
    if let Some(array) = value.as_array()
        && array.len() >= 2
    {
        return Some([
            array.first().and_then(Value::as_f64).unwrap_or(0.0) as f32,
            array.get(1).and_then(Value::as_f64).unwrap_or(0.0) as f32,
        ]);
    }
    if let Some(object) = value.as_object() {
        return Some([
            object.get("x").and_then(Value::as_f64).unwrap_or(0.0) as f32,
            object.get("y").and_then(Value::as_f64).unwrap_or(0.0) as f32,
        ]);
    }
    None
}

pub(super) fn asset_id_from(value: &str) -> AssetId {
    AssetId::try_from(value.to_owned()).unwrap_or_else(|_| AssetId::of(value.as_bytes()))
}

pub(super) fn media_type_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        Some("wav") => "audio/wav",
        Some("mp3") => "audio/mpeg",
        Some("flac") => "audio/flac",
        Some("aac") => "audio/aac",
        Some("m4a") => "audio/mp4",
        Some("ogg") => "audio/ogg",
        Some("mp4") => "video/mp4",
        Some("mov") => "video/quicktime",
        Some("mkv") => "video/x-matroska",
        Some("webm") => "video/webm",
        _ => "application/octet-stream",
    }
}

pub(super) fn project_size(project: &Project) -> (u32, u32) {
    animathio_export::resolution(project)
}

pub(super) fn next_id(project: &Project) -> u64 {
    project
        .tracks()
        .iter()
        .flat_map(|t| t.clips().keys().map(|c| c.0))
        .chain(project.tracks().iter().map(|t| t.id().0))
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

pub(super) fn autosave_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".autosave");
    PathBuf::from(name)
}

pub(super) fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

pub(crate) fn read_project(path: &Path) -> Result<(Project, BTreeMap<AssetId, Vec<u8>>), String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let bundle = animathio_format::read_amio(
        std::io::BufReader::new(file),
        animathio_format::Limits::default(),
    )
    .map_err(|e| e.to_string())?;
    Ok((bundle.project, bundle.assets))
}

/// Read a project, preferring a newer `<file>.amio.autosave` when it exists so a
/// crash mid-edit can be recovered. Returns the recovered flag alongside the data.
pub(crate) fn read_project_with_recovery(path: &Path) -> Result<LoadedProject, String> {
    let autosave = autosave_path(path);
    let use_autosave = match (modified(path), modified(&autosave)) {
        (Some(main), Some(auto)) => auto > main,
        (None, Some(_)) => true,
        _ => false,
    };
    let source = if use_autosave { &autosave } else { path };
    let (project, assets) = read_project(source)?;
    Ok((project, assets, use_autosave))
}

pub(super) fn write_project(
    path: &Path,
    project: &Project,
    assets: &BTreeMap<AssetId, Vec<u8>>,
) -> Result<(), String> {
    // Only persist bytes still referenced by the model so undo of an asset add
    // cannot leave orphaned entries that fail the manifest check.
    let referenced: BTreeMap<AssetId, Vec<u8>> = assets
        .iter()
        .filter(|(id, _)| project.assets().contains_key(id))
        .map(|(id, bytes)| (id.clone(), bytes.clone()))
        .collect();
    animathio_format::save_amio(path, project, &referenced).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::next_id;
    use animathio_core::{
        Clip, ClipId, ClipKind, Command, FrameRate, Project, Time, TimeRange, TrackId,
    };

    /// The allocation `add_media_file` performs for a video with audio: the clip id and
    /// its companion both come from the counter, which is refreshed from the project
    /// after the batch is applied. A second import in the same drop must not collide.
    #[test]
    fn two_video_imports_with_companion_audio_get_distinct_ids() {
        let mut project = Project::new(FrameRate::new(30, 1).unwrap());
        let mut counter = next_id(&project);
        let range = TimeRange::new(Time(0), Time(100)).unwrap();
        for import in 0..3u64 {
            let video = ClipId(counter);
            counter += 1;
            let audio = ClipId(counter);
            counter += 1;
            let video_track = TrackId(100 + import * 2);
            let audio_track = TrackId(101 + import * 2);
            let mut audio_clip = Clip::new(audio, ClipKind::Error("a".into()), range);
            audio_clip.link_group = Some(video);
            let mut video_clip = Clip::new(video, ClipKind::Error("v".into()), range);
            video_clip.link_group = Some(video);
            Command::Batch {
                commands: vec![
                    Command::AddTrack {
                        track: animathio_core::Track::new(video_track, "V"),
                        index: 0,
                    },
                    Command::AddTrack {
                        track: animathio_core::Track::new(audio_track, "A"),
                        index: 1,
                    },
                    Command::AddClip {
                        track: video_track,
                        clip: video_clip,
                    },
                    Command::AddClip {
                        track: audio_track,
                        clip: audio_clip,
                    },
                ],
            }
            .apply(&mut project)
            .unwrap_or_else(|e| panic!("import {import}: {e}"));
            counter = counter.max(next_id(&project));
        }
        let clips: usize = project.tracks().iter().map(|t| t.clips().len()).sum();
        assert_eq!(clips, 6);
    }
}
