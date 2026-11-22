//! Shared pure helpers, drawing and resource adapters.

use super::*;
use crate::style::{self, color as colors, tokens::TextRole as _};
use std::collections::HashMap;

/// Paint-ready waveform paths, shared by snapshots; bounded independently of PCM.
pub(super) type WavePathCache =
    std::rc::Rc<std::cell::RefCell<BTreeMap<(AssetId, [u64; 12]), gpui::Path<Pixels>>>>;

/// An icon-only button with a tooltip, using a Lucide (or bundled) SVG.
pub(super) fn icon_button(
    id: &'static str,
    icon: gpui_component::Icon,
    tooltip: impl Into<SharedString>,
    listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Button {
    // Panels and toolbars: `Size::Small` (the ROW_SM height, docs/design.md).
    let button = Button::new(id)
        .ghost()
        .small()
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
        "tool-rate-stretch" => Some((Box::new(actions::RateStretchTool), "Timeline")),
        "tool-slip" => Some((Box::new(actions::SlipTool), "Timeline")),
        "tool-slide" => Some((Box::new(actions::SlideTool), "Timeline")),
        "tool-rolling" => Some((Box::new(actions::RollingTool), "Timeline")),
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

/// Which half of the timeline a paint call draws. The ruler and marker lane stay fixed
/// while the track rows scroll vertically, so each half has its own canvas; both use the
/// same geometry (the track half is positioned as if the ruler sat above it).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TimelinePart {
    Ruler,
    Tracks,
}

pub(super) fn paint_timeline(
    part: TimelinePart,
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
        ruler_height(),
        snapshot.row_height,
        snapshot.duration,
        zoom,
        scroll,
    )
    .with_frame_rate(snapshot.frame_rate);
    let background = style::hex(colors::PANEL);
    let ruler_bg = style::hex(colors::BG);
    let border = style::hex(colors::BORDER);
    let muted = style::hex(colors::TEXT_SECONDARY);
    let ruler_part = part == TimelinePart::Ruler;
    window.paint_quad(fill(bounds, background));
    let height = f32::from(bounds.size.height);
    let ruler = Bounds::from_corners(
        bounds.origin,
        point(
            bounds.origin.x + px(width),
            bounds.origin.y + px(ruler_height()),
        ),
    );
    if ruler_part {
        window.paint_quad(fill(ruler, ruler_bg));
        paint_ruler(bounds, &layout, snapshot, width, muted, border, window, cx);
    }
    let rows = if ruler_part { 0 } else { snapshot.rows };
    for row in 0..rows {
        let top = bounds.origin.y + px(ruler_height() + row as f32 * snapshot.row_height);
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
    for row in 0..rows {
        if !snapshot.overview.iter().any(|(r, _, _)| *r == row) {
            // The very first empty row of an empty project says what else can go here.
            let label = if snapshot.overview.is_empty() && row == 0 {
                "Drop media here, or add text, shapes and formulas from Elements"
            } else {
                "Drop media here"
            };
            paint_empty_lane_label(
                label,
                point(
                    bounds.left() + px(20.),
                    bounds.top() + px(ruler_height() + row as f32 * snapshot.row_height + 16.),
                ),
                bounds.right(),
                font(snapshot.theme.font_family.clone()),
                window,
                cx,
            );
        }
    }
    for clip in snapshot.clips.iter().filter(|_| !ruler_part) {
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            paint_clip(bounds, &layout, clip, &snapshot.waveform_paths, window, cx);
        });
    }
    for transition in snapshot.transitions.iter().filter(|_| !ruler_part) {
        paint_transition(bounds, &layout, transition, window, cx);
    }
    if let Some((left, top, right, bottom)) = snapshot.marquee.filter(|_| !ruler_part) {
        let band = Bounds::from_corners(
            point(bounds.origin.x + px(left), bounds.origin.y + px(top)),
            point(bounds.origin.x + px(right), bounds.origin.y + px(bottom)),
        );
        window.paint_quad(fill(
            band,
            style::tint(colors::ACCENT, style::Alpha::Subtle),
        ));
        window.paint_quad(outline(
            band,
            style::hex(colors::ACCENT),
            BorderStyle::Solid,
        ));
    }
    for (range, row) in snapshot.autocuts.iter().filter(|_| !ruler_part) {
        let x0 = timeline::time_to_x(&layout, range.start).clamp(0., layout.width);
        let x1 = timeline::time_to_x(&layout, range.end).clamp(0., layout.width);
        #[allow(clippy::cast_precision_loss)]
        let top = ruler_height() + *row as f32 * snapshot.row_height;
        window.paint_quad(fill(
            Bounds::from_corners(
                point(bounds.origin.x + px(x0), bounds.origin.y + px(top)),
                point(
                    bounds.origin.x + px(x1),
                    bounds.origin.y + px(top + snapshot.row_height),
                ),
            ),
            style::tint(colors::WARNING, style::Alpha::Body),
        ));
    }
    if snapshot.marks.marked()
        && let Some(range) = snapshot.marks.range(snapshot.duration)
    {
        let x0 = bounds.origin.x + px(timeline::time_to_x(&layout, range.start));
        let x1 = bounds.origin.x + px(timeline::time_to_x(&layout, range.end));
        window.paint_quad(fill(
            Bounds::from_corners(
                point(x0, bounds.origin.y),
                point(x1, bounds.origin.y + px(ruler_height())),
            ),
            style::tint(colors::ACCENT, style::Alpha::Wash),
        ));
        for x in [x0, x1] {
            window.paint_quad(fill(
                Bounds::new(point(x, bounds.origin.y), size(px(1.), px(height))),
                style::hex(colors::ACCENT),
            ));
            window.paint_quad(fill(
                Bounds::new(point(x, bounds.origin.y), size(px(8.), px(6.))),
                style::hex(colors::ACCENT),
            ));
        }
    }
    super::markers::paint_markers(bounds, &layout, snapshot, height, window, cx);
    let head_x = bounds.origin.x + px(timeline::time_to_x(&layout, snapshot.position));
    let playhead = style::hex(colors::PLAYHEAD);
    window.paint_quad(fill(
        Bounds::new(
            point(head_x - px(0.5), bounds.top()),
            size(px(1.), px(height)),
        ),
        playhead,
    ));
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
            fill(pill, style::hex(colors::RAISED)).corner_radii(px(style::radius::PILL)),
        );
        window.paint_quad(
            outline(pill, style::hex(colors::BORDER), BorderStyle::Solid)
                .corner_radii(px(style::radius::PILL)),
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
                point(x + px(3.), bounds.top() + px(ruler_height() + 3.)),
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
    let y0 = bounds.origin.y + px(ruler_height() + clip.row as f32 * layout.row_height + 3.0);
    let y1 =
        bounds.origin.y + px(ruler_height() + (clip.row as f32 + 1.0) * layout.row_height - 3.0);
    let block = Bounds::from_corners(point(x0, y0), point(x1, y1));
    let kind_color = clip_color(&clip.kind, cx.theme());
    if !timeline::visible_span(layout, clip.start, clip.end) {
        return;
    }
    let dim = if clip.silenced { 0.5 } else { 1.0 };
    let radius = px(style::radius::CLIP);
    window.paint_quad(fill(block, style::hex(colors::RAISED)).corner_radii(radius));
    let body_alpha = if clip.hovered {
        CLIP_HOVER_ALPHA
    } else {
        style::Alpha::Body.value()
    };
    window.paint_quad(fill(block, kind_color.opacity(body_alpha * dim)).corner_radii(radius));
    window.paint_quad(
        outline(
            block,
            style::tint(colors::BORDER, style::Alpha::Edge),
            BorderStyle::Solid,
        )
        .corner_radii(radius),
    );
    window.paint_quad(fill(
        Bounds::from_corners(point(x0, y0), point(x0 + px(3.), y1)),
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
    paint_speed_ramp(bounds, layout, clip, (x0, x1, y0, y1), window);
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
            outline(block, style::hex(colors::TEXT), BorderStyle::Solid).corner_radii(radius),
        );
        window.paint_quad(
            outline(
                Bounds::from_corners(
                    point(x0 + px(1.0), y0 + px(1.0)),
                    point(x1 - px(1.0), y1 - px(1.0)),
                ),
                style::hex(colors::TEXT),
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
        // Centre the 12 px icon on the Caption text, which `paint_text_line` centres in a
        // Label-height line from `y0 + 3` (centre about `y0 + 11.5`).
        let icon_bounds = Bounds::new(point(lx + px(7.0), y0 + px(5.5)), size(px(12.0), px(12.0)));
        paint_icon(clip.icon, icon_bounds, cx.theme().foreground, window, cx);
        let warned = clip.effect_warning && x1 - x0 > px(64.0);
        if warned {
            // Missing or failed effect plugin: amber badge beside the kind icon.
            let badge = Bounds::new(point(lx + px(23.0), y0 + px(2.0)), size(px(16.0), px(14.0)));
            window.paint_quad(fill(badge, style::hex(colors::WARNING)).corner_radii(px(4.0)));
            paint_icon(
                IconName::TriangleAlert,
                Bounds::new(point(lx + px(25.0), y0 + px(5.5)), size(px(12.0), px(12.0))),
                style::hex(colors::BG),
                window,
                cx,
            );
        }
        if !clip.label.is_empty() {
            let label_x = lx + px(if warned { 44.0 } else { 22.0 });
            let available_w = x1 - label_x - px(4.0);
            paint_clip_name_and_duration(
                &clip.label,
                layout
                    .frame_rate
                    .map(|rate| timecode::short_duration(Time(clip.end.0 - clip.start.0), rate)),
                point(label_x, y0 + px(3.0)),
                available_w,
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
    // Keyframe diamonds: 8 px rotated squares with BG 1 px outline, resting TEXT and selected ACCENT.
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
        window.paint_path(diamond(kx, cy, px(5.0)), style::hex(colors::BG));
        window.paint_path(
            diamond(kx, cy, px(4.0)),
            if keyframe.selected {
                style::hex(colors::ACCENT)
            } else {
                style::hex(colors::TEXT)
            },
        );
    }
}

/// Content labels are kept here so plugin migration touches one small match.
pub(super) fn timeline_content_preview(clip: &Clip) -> String {
    const KEYS: [&str; 4] = ["text", "source", "latex", "expression"];
    let first_line = |text: &str| -> String {
        text.lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(100)
            .collect()
    };
    match &clip.kind {
        ClipKind::Vector(_) => "Vector shape".into(),
        ClipKind::Plugin { plugin_id, params } => KEYS
            .iter()
            .find_map(|key| match clip.params.get(*key) {
                Some(animathio_core::ParamValue::String(text)) => Some(first_line(text)),
                // Plugin sources keep their text in the clip kind's JSON data.
                _ => params
                    .get(*key)
                    .and_then(serde_json::Value::as_str)
                    .map(first_line),
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

/// The filmstrip uses the same physical frame aspect as the rendered clip.
pub(super) fn filmstrip_tile_width(height: f32, canvas: (u32, u32), transform: Transform) -> f32 {
    let ratio = (f64::from(canvas.0) * transform.scale_x).abs()
        / (f64::from(canvas.1.max(1)) * transform.scale_y)
            .abs()
            .max(1e-9);
    (height * ratio as f32).clamp(8., 256.) + 1.
}

/// Keep the physical frame aspect even when an extreme transform caps tile width.
fn filmstrip_frame(
    tile: Bounds<Pixels>,
    canvas: (u32, u32),
    transform: Transform,
) -> Bounds<Pixels> {
    let w = (f64::from(canvas.0) * transform.scale_x).abs().max(1e-9);
    let h = (f64::from(canvas.1) * transform.scale_y).abs().max(1e-9);
    let scale =
        (f64::from(f32::from(tile.size.width)) / w).min(f64::from(f32::from(tile.size.height)) / h);
    let size = size(px((w * scale) as f32), px((h * scale) as f32));
    Bounds::new(
        tile.center() - point(size.width / 2., size.height / 2.),
        size,
    )
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
    let tile_width = px(filmstrip_tile_width(
        f32::from(height),
        clip.canvas,
        clip.transform,
    ));
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
        let slot = index.saturating_sub(clip.thumbnail_first);
        if let Some(Some(image)) = clip.thumbnails.get(slot) {
            let tile = Bounds::new(point(x, top), size(tile_width - px(1.), height));
            let frame = filmstrip_frame(tile, clip.canvas, clip.transform);
            let [fx, fy, fw, fh] = clip
                .fit
                .rect(image.source_size, clip.canvas, clip.transform);
            let fitted = Bounds::new(
                frame.origin + point(frame.size.width * fx as f32, frame.size.height * fy as f32),
                size(frame.size.width * fw as f32, frame.size.height * fh as f32),
            );
            let _ = window.paint_image(
                frame.intersect(&mask),
                fitted,
                Corners::default(),
                image.image.clone(),
                0,
                false,
            );
            window.paint_quad(fill(
                tile.intersect(&mask),
                style::tint(colors::BG, style::Alpha::Wash),
            ));
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
    let top = bounds.top() + px(ruler_height() + transition.row as f32 * layout.row_height + 3.);
    let bottom = top + px((layout.row_height - 6.).max(8.));
    let area = Bounds::from_corners(point(left, top), point(right, bottom));
    // The overlap is a hatched `TEXT` wash with a hairline rim: it reads on every
    // clip colour and carries no accent.
    window.paint_quad(
        fill(area, style::tint(colors::TEXT, style::Alpha::Wash))
            .corner_radii(px(style::radius::CLIP)),
    );
    window.with_content_mask(Some(ContentMask { bounds: area }), |window| {
        let hatch = style::tint(colors::TEXT, style::Alpha::Subtle);
        let (width, height) = (f32::from(area.size.width), f32::from(area.size.height));
        let mut offset = -height;
        while offset < width {
            let mut stroke = gpui::PathBuilder::stroke(px(1.));
            stroke.move_to(point(area.left() + px(offset), area.bottom()));
            stroke.line_to(point(area.left() + px(offset + height), area.top()));
            if let Ok(path) = stroke.build() {
                window.paint_path(path, hatch);
            }
            offset += 8.;
        }
    });
    window.paint_quad(
        outline(
            area,
            style::tint(colors::TEXT, style::Alpha::Subtle),
            BorderStyle::Solid,
        )
        .corner_radii(px(style::radius::CLIP)),
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
            .corner_radii(px(style::radius::CLIP)),
        );
        paint_icon(IconName::Blend, icon, style::hex(colors::TEXT), window, cx);
    }
}

/// The overview strip: every clip as a thin lane block, the visible range as an accent
/// window, inside a `BORDER` frame. It carries no text label.
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
    let inner_h = (f32::from(bounds.size.height) - 2.).max(1.);
    let lane = inner_h / rows.max(1) as f32;
    for (row, range, color) in clips {
        let top = bounds.top() + px(1. + *row as f32 * lane);
        let area = Bounds::from_corners(
            point(bounds.left() + px(range.start.0 as f32 * ppt), top),
            point(
                bounds.left() + px(range.end.0 as f32 * ppt),
                top + px((lane - 0.5).max(1.)),
            ),
        );
        window.paint_quad(fill(area, color.opacity(style::Alpha::Edge.value())));
    }
    let view = Bounds::new(
        point(bounds.left() + px(scroll / zoom), bounds.top()),
        size(bounds.size.width / zoom, bounds.size.height),
    );
    window.paint_quad(fill(
        view,
        style::tint(colors::ACCENT, style::Alpha::Subtle),
    ));
    window.paint_quad(outline(
        bounds,
        style::hex(colors::BORDER),
        BorderStyle::Solid,
    ));
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
    if clip.time_map.hold {
        return;
    }
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
            clip.id.0,
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
        let local = (f64::from(x - f32::from(x0)) / layout.pixels_per_tick) as i64;
        let step = (f64::from(STEP) / layout.pixels_per_tick).ceil() as i64;
        let a = clip
            .time_map
            .source_time(Time(clip.source_offset), Time(local))
            .0;
        let b = clip
            .time_map
            .source_time(Time(clip.source_offset), Time(local.saturating_add(step)))
            .0;
        let (start, end) = (a.min(b), a.max(b));
        (wave.peak_between(start, end.max(start.saturating_add(1))) * half_max).max(0.5)
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

/// The speed curve and its draggable key handles, for the selected clip with speed keys.
/// The curve is the linear ramp the retiming clock integrates: flat before the first key
/// and after the last, with a faint 100% guide behind it.
fn paint_speed_ramp(
    bounds: Bounds<Pixels>,
    layout: &TimelineLayout,
    clip: &ClipPaint,
    (x0, x1, y0, y1): (Pixels, Pixels, Pixels, Pixels),
    window: &mut Window,
) {
    let Some(lane) = crate::speed_ramp::lane(f32::from(y0), f32::from(y1)) else {
        return;
    };
    if !clip.ramp_lane || x1 - x0 < px(super::retime::RAMP_LANE_MIN_WIDTH) {
        return;
    }
    let guide_y = px(lane.y(animathio_core::ClipSpeed::default()));
    window.paint_quad(fill(
        Bounds::from_corners(
            point(x0 + px(4.), guide_y),
            point(x1 - px(4.), guide_y + px(1.)),
        ),
        style::tint(colors::TEXT, style::Alpha::Subtle),
    ));
    if clip.ramp_keys.is_empty() {
        return;
    }
    let key_x =
        |time: Time| (bounds.origin.x + px(timeline::time_to_x(layout, time))).clamp(x0, x1);
    let mut curve = gpui::PathBuilder::stroke(px(1.5));
    let first = &clip.ramp_keys[0];
    curve.move_to(point(x0, px(lane.y(first.speed))));
    for key in &clip.ramp_keys {
        curve.line_to(point(key_x(key.time), px(lane.y(key.speed))));
    }
    if let Some(last) = clip.ramp_keys.last() {
        curve.line_to(point(x1, px(lane.y(last.speed))));
    }
    if let Ok(path) = curve.build() {
        window.paint_path(path, style::hex(colors::TEXT));
    }
    for key in clip.ramp_keys.iter().filter(|key| key.handle) {
        let kx = bounds.origin.x + px(timeline::time_to_x(layout, key.time));
        if kx < x0 || kx > x1 {
            continue;
        }
        let ky = px(lane.y(key.speed));
        let (outer, inner) = (px(5.5), px(4.0));
        window.paint_quad(fill(
            Bounds::from_corners(point(kx - outer, ky - outer), point(kx + outer, ky + outer)),
            style::hex(colors::BG),
        ));
        window.paint_quad(fill(
            Bounds::from_corners(point(kx - inner, ky - inner), point(kx + inner, ky + inner)),
            if key.active {
                style::hex(colors::ACCENT)
            } else {
                style::hex(colors::TEXT)
            },
        ));
    }
}

/// Polyline samples per fade curve.
const FADE_SAMPLES: usize = 32;

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
    let shade = style::tint(colors::BG, style::Alpha::Edge);
    let ppt = width / (clip.end.0 - clip.start.0).max(1) as f32;
    let fade_in_px = (clip.fade_in as f32 * ppt).min(width);
    let fade_out_px = (clip.fade_out as f32 * ppt).min(width);
    // Level curve sampled from the actual keyframes from the edge to full
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
                let ease = clip.fade_levels[usize::from(!left)]
                    .get(i)
                    .copied()
                    .unwrap_or(t)
                    .clamp(0., 1.);
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
            window.paint_path(path, style::tint(colors::TEXT, style::Alpha::Scrim));
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
///
/// The ruler has two rows: marker flags and names (drawn by `markers::paint_markers`) on
/// the top row, tick numbers on the bottom row, so a marker name never covers a number.
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
    let number_font = tabular_font(snapshot.theme.font_family.clone(), FontWeight(400.));
    let line = window.rem_size() * style::Role::Caption.line_height().0;
    let label_top = bounds.origin.y + px(ruler_height()) - line;
    // Minor ticks share the bottom row with the tick numbers: skip the ones that would run
    // through the previous number.
    let mut label_end = bounds.origin.x - px(1.0);
    for tick in timeline::ruler_ticks(layout, minor) {
        let x = bounds.origin.x + px(timeline::time_to_x(layout, Time(tick)));
        let is_major = tick % step == 0;
        if !is_major && x < label_end {
            continue;
        }
        let mark_top = bounds.origin.y
            + px(ruler_height() - (if is_major { 10.0 } else { 5.0 }) * style::ui_scale());
        let mark = Bounds::from_corners(
            point(x, mark_top),
            point(x + px(1.0), bounds.origin.y + px(ruler_height())),
        );
        window.paint_quad(fill(mark, if is_major { muted } else { border }));
        if is_major {
            let label = if step >= TICKS_PER_SECOND {
                let seconds = tick / TICKS_PER_SECOND;
                format!("{:02}:{:02}", seconds / 60, seconds % 60)
            } else {
                timecode::format_timecode(Time(tick), snapshot.frame_rate)
            };
            // Tabular digits: about 0.62 em per character, plus the 5 px inset and a gap.
            let char_width = window.rem_size() * style::Role::Caption.size().0 * 0.62;
            label_end = x + px(8.0) + char_width * label.chars().count() as f32;
            paint_text_line(
                label.into(),
                point(x + px(5.0), label_top),
                x + px(width),
                number_font.clone(),
                TextStyle::new(
                    style::type_size::CAPTION,
                    style::hex(colors::TEXT_SECONDARY),
                ),
                window,
                cx,
            );
        }
    }
}

/// Choose a ruler interval with room for a full timecode at the current UI scale.
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
        .find(|candidate| candidate * pixels_per_tick >= 120.0 * f64::from(style::ui_scale()))
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
            } else if plugin_id == "org.animathio.latex" || plugin_id == "latex" {
                colors::CLIP_LATEX
            } else if plugin_id == "org.animathio.graph" || plugin_id == "graph" {
                colors::CLIP_GRAPH
            } else {
                colors::CLIP_PLUGIN
            }
        }
        ClipKind::Vector(_) => colors::CLIP_SHAPE,
        ClipKind::Error(_) => colors::CLIP_ERROR,
    })
}

/// Size and colour of one line of canvas text (see [`paint_text_line`]). The weight and
/// features live on the `Font` (see [`tabular_font`]).
#[derive(Clone, Copy)]
pub(super) struct TextStyle {
    size: Rems,
    color: Hsla,
}

impl TextStyle {
    pub(super) fn new(size: Rems, color: Hsla) -> Self {
        Self { size, color }
    }
}

/// A canvas font with tabular figures (`tnum`) at `weight`, so timecodes and durations do
/// not jitter while the timeline scrolls.
pub(super) fn tabular_font(family: SharedString, weight: FontWeight) -> Font {
    let mut tabular = font(family);
    tabular.weight = weight;
    tabular.features = FontFeatures(std::sync::Arc::new(vec![("tnum".into(), 1)]));
    tabular
}

/// Hover step of a clip body: the body step plus 0.04. A larger step would push the
/// duration under 4.5:1 on the shape and graph colours (see the contrast test below).
pub(super) const CLIP_HOVER_ALPHA: f32 = 0.36;

/// Width of one line of label text as [`paint_clip_label`] would draw it.
///
/// The ruler measures every marker name plate and every major tick label on every
/// repaint, including every frame of playback, so widths are cached per (font, size,
/// text) in a small table private to the paint thread. The measurement only changes
/// when the label text, the theme fonts or the UI scale change, and each of those
/// changes the key. Cleared wholesale once it outgrows its bound: the visible label
/// set at any one zoom level is small, so the bound is only reached after many edits
/// across zoom levels, where clearing is cheaper than bounding per font.
pub(super) fn label_width(label: &str, label_font: Font, window: &mut Window) -> Pixels {
    text_width(label, label_font, style::type_size::LABEL, window)
}

/// Width of one line of `label` at `size` rems (cached like [`label_width`]).
pub(super) fn text_width(label: &str, label_font: Font, size: Rems, window: &mut Window) -> Pixels {
    use std::cell::RefCell;

    thread_local! {
        static WIDTHS: RefCell<LabelWidths> = RefCell::new(LabelWidths::default());
    }
    if label.is_empty() {
        return px(0.);
    }
    let font_size = window.rem_size() * size.0;
    WIDTHS.with(|widths| {
        widths
            .borrow_mut()
            .get(label, label_font, font_size, window)
    })
}

/// Bounded cache of measured label widths; see [`label_width`].
#[derive(Default)]
struct LabelWidths {
    entries: usize,
    widths: HashMap<(Font, u32), HashMap<String, Pixels>>,
}

impl LabelWidths {
    /// Measured widths kept before the table is cleared wholesale.
    const MAX_ENTRIES: usize = 1024;

    fn get(&mut self, label: &str, font: Font, font_size: Pixels, window: &mut Window) -> Pixels {
        let size = f32::from(font_size).to_bits();
        if let Some(by_text) = self.widths.get(&(font.clone(), size))
            && let Some(width) = by_text.get(label)
        {
            return *width;
        }
        let width = shape_label_width(label, font.clone(), font_size, window);
        if self.entries >= Self::MAX_ENTRIES {
            self.entries = 0;
            self.widths.clear();
        }
        self.entries += 1;
        self.widths
            .entry((font, size))
            .or_default()
            .insert(label.to_owned(), width);
        width
    }
}

/// Shape `label` once at `font_size` and return the width of the line.
fn shape_label_width(
    label: &str,
    label_font: Font,
    font_size: Pixels,
    window: &mut Window,
) -> Pixels {
    let run = TextRun {
        len: label.len(),
        font: label_font,
        color: style::hex(colors::TEXT),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .shape_line(
            SharedString::from(label.to_owned()),
            font_size,
            &[run],
            None,
        )
        .width
}

/// Paint a single line of text in `text_style`, clipped to the right edge.
pub(super) fn paint_text_line(
    label: SharedString,
    origin: Point<Pixels>,
    right_edge: Pixels,
    label_font: Font,
    text_style: TextStyle,
    window: &mut Window,
    cx: &mut App,
) {
    if label.is_empty() {
        return;
    }
    let run = TextRun {
        len: label.len(),
        font: label_font,
        color: text_style.color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let font_size = window.rem_size() * text_style.size.0;
    let line = window
        .text_system()
        .shape_line(label, font_size, &[run], None);
    let line_height = window.rem_size() * style::Role::Label.line_height().0;
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

/// Paint a single line of clip text, clipped to the right edge.
pub(super) fn paint_clip_label(
    label: SharedString,
    origin: Point<Pixels>,
    right_edge: Pixels,
    label_font: Font,
    window: &mut Window,
    cx: &mut App,
) {
    paint_text_line(
        label,
        origin,
        right_edge,
        label_font,
        TextStyle::new(style::type_size::LABEL, style::hex(colors::TEXT)),
        window,
        cx,
    );
}

/// The hint on a track lane that holds no clip (`Role::Label`, `TEXT_MUTED`).
pub(super) fn paint_empty_lane_label(
    label: &str,
    origin: Point<Pixels>,
    right_edge: Pixels,
    label_font: Font,
    window: &mut Window,
    cx: &mut App,
) {
    paint_text_line(
        label.to_owned().into(),
        origin,
        right_edge,
        label_font,
        TextStyle::new(style::type_size::LABEL, style::hex(colors::TEXT_MUTED)),
        window,
        cx,
    );
}

/// A clip's name (`CAPTION`, weight 500, `TEXT`) and, only when both fit in `available`,
/// its duration after it in the muted tint. The duration is never allowed to push the name
/// out: when it does not fit, the name is clipped alone.
pub(super) fn paint_clip_name_and_duration(
    name: &str,
    duration: Option<String>,
    origin: Point<Pixels>,
    available: Pixels,
    window: &mut Window,
    cx: &mut App,
) {
    const GAP: f32 = 8.0;
    let family = cx.theme().font_family.clone();
    let name_font = tabular_font(family.clone(), FontWeight(500.));
    let right = origin.x + available.max(px(0.));
    let size = style::type_size::CAPTION;
    let name_width = text_width(name, name_font.clone(), size, window);
    paint_text_line(
        name.to_owned().into(),
        origin,
        right,
        name_font,
        TextStyle::new(size, style::hex(colors::TEXT)),
        window,
        cx,
    );
    let Some(duration) = duration else {
        return;
    };
    let duration_font = tabular_font(family, FontWeight(400.));
    let duration_width = text_width(&duration, duration_font.clone(), size, window);
    if name_width + px(GAP) + duration_width > available {
        return;
    }
    paint_text_line(
        duration.into(),
        origin + point(name_width + px(GAP), px(0.)),
        right,
        duration_font,
        TextStyle::new(size, style::tint(colors::TEXT, style::Alpha::Scrim)),
        window,
        cx,
    );
}

pub(super) fn field_value(clip: &Clip, field: &Field) -> f64 {
    match field {
        Field::Transform(property) => match property {
            TransformProperty::X => clip.transform.x,
            TransformProperty::Y => clip.transform.y,
            TransformProperty::ScaleX => clip.transform.scale_x,
            TransformProperty::ScaleY => clip.transform.scale_y,
            TransformProperty::Rotation => clip.transform.rotation.to_degrees(),
            TransformProperty::Opacity => clip.transform.opacity,
        },
        Field::Param(name) => clip
            .params
            .get(name)
            .and_then(animathio_core::ParamValue::as_number)
            .unwrap_or_else(|| param_default(name)),
    }
}

/// Title and Lucide icon of the inspector section that holds a clip's own content parameters:
/// named after what the clip is, never "Text" for a shape.
pub(super) fn plugin_section_identity(kind: &ClipKind) -> (&'static str, IconName) {
    let ClipKind::Plugin { plugin_id, .. } = kind else {
        return ("Content", IconName::SlidersHorizontal);
    };
    match plugin_id.as_str() {
        "org.animathio.text" => ("Text", IconName::Type),
        "org.animathio.shapes" => ("Shape", IconName::Shapes),
        "org.animathio.latex" => ("Formula", IconName::Sigma),
        "org.animathio.graph" => ("Plot", IconName::ChartNoAxesColumn),
        _ => ("Content", IconName::SlidersHorizontal),
    }
}

/// Whether the inspector shows plugin parameter `param`. The schema has no conditional
/// visibility, so the editor hides the shapes plugin's raw `path` verbs unless `shape` is
/// `path`; `value_of` reads another parameter's current text value.
pub(super) fn plugin_param_visible(
    plugin_id: &str,
    param: &str,
    value_of: impl Fn(&str) -> Option<String>,
) -> bool {
    if plugin_id == "org.animathio.shapes" && param == "path" {
        return value_of("shape").as_deref() == Some("path");
    }
    true
}

/// The value a builtin clip parameter has when the clip stores none. Playback (`Mixer`) treats a
/// missing `gain` as unity, so the inspector must show and step from 1, never 0.
pub(super) fn param_default(name: &str) -> f64 {
    match name {
        "gain" => 1.0,
        _ => 0.0,
    }
}

/// Linear gain as decibels (`0 dB` = unity), or `-inf` for silence.
pub(super) fn gain_db(gain: f64) -> f64 {
    if gain <= 0.0 {
        f64::NEG_INFINITY
    } else {
        20.0 * gain.log10()
    }
}

/// Short unit hint for a linear gain, for example `0.0 dB` or `-20.0 dB`.
#[cfg(test)]
pub(super) fn gain_db_label(gain: f64) -> String {
    let db = gain_db(gain);
    if db.is_finite() {
        format!("{db:+.1} dB")
    } else {
        "-inf dB".to_owned()
    }
}

pub(super) fn field_step(field: &Field) -> f64 {
    match field {
        Field::Transform(TransformProperty::Rotation) => 1.0,
        Field::Transform(TransformProperty::Opacity) => 0.05,
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
            "org.animathio.latex" => "math".into(),
            "org.animathio.graph" => "graph".into(),
            // Third-party plugins: the last segment of the reverse-DNS id.
            other => other.rsplit('.').next().unwrap_or(other).to_owned(),
        },
        ClipKind::Vector(_) => "vector".into(),
        ClipKind::Error(_) => "error".into(),
    }
}

/// A themed context-menu item. `shortcut` is the action bound to the same command, shown
/// right-aligned exactly like the application menu bar does.
pub(super) fn context_item(
    icon: IconName,
    label: &str,
    shortcut: Option<&dyn Action>,
    listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    window: &Window,
    _cx: &mut Context<Editor>,
) -> Stateful<Div> {
    let binding = shortcut.and_then(|action| {
        gpui_component::kbd::Kbd::binding_for_action(action, Some("Timeline"), window)
    });
    div()
        .px_2()
        .h(crate::style::size::ROW)
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .cursor_pointer()
        .text_role(crate::style::Role::Body)
        .hover(|this| this.bg(style::hex(colors::HOVER)))
        .child(icons::icon(icon).small())
        .child(div().flex_1().child(label.to_owned()))
        .children(binding.map(|kbd| kbd.appearance(false)))
        .id(SharedString::from(format!("context-item-{label}")))
        .on_click(listener)
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

/// Timeline label including the clip's retiming summary.
pub(super) fn clip_label(project: &Project, clip: &Clip) -> String {
    let kind = match &clip.kind {
        ClipKind::Video(id) | ClipKind::Image(id) | ClipKind::Audio(id) => {
            project.assets().get(id).map(|a| a.name.clone())
        }
        _ => None,
    };
    let name = kind.unwrap_or_else(|| kind_label(&clip.kind));
    if clip.time_map.hold {
        format!("{name} [Hold]")
    } else if !clip.time_map.ramps.is_empty() {
        format!(
            "{name} [Ramp{}]",
            if clip.time_map.reverse {
                ", reverse"
            } else {
                ""
            }
        )
    } else if clip.time_map.speed != animathio_core::ClipSpeed::default() || clip.time_map.reverse {
        format!(
            "{name} [{:.2}%{}]",
            clip.time_map.speed.percent(),
            if clip.time_map.reverse {
                ", reverse"
            } else {
                ""
            }
        )
    } else {
        name
    }
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
    crate::editor_env::plugin_cache_dir()
}

/// Load every clip-source plugin from the bundled build output and the user
/// plugins directory. Descriptors are plain data for the UI; the host itself is
/// kept on the UI thread and drives both preview and control rendering.
pub(crate) fn load_plugins() -> (Option<PluginHost>, Vec<PluginDescriptor>, Option<String>) {
    let source = crate::editor_env::plugin_source();
    if source == PluginSource::None {
        return (None, Vec::new(), None);
    }
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
    let problems = match source {
        PluginSource::Dir(path) => {
            let dir = PluginDirectory {
                path,
                allow_reserved_ids: true,
                community: false,
            };
            let mut problems = Vec::new();
            if let Err(error) = host.load_plugin_directory(&dir) {
                problems.push(format!("plugins in {}: {error}", dir.path.display()));
            }
            problems.extend(host.load_failures().iter().cloned());
            problems
        }
        _ => host.load_discovered_plugins_with(crate::editor_env::community_store().as_ref()),
    };
    for problem in &problems {
        eprintln!("plugin load problem: {problem}");
    }
    // Preview keeps placeholder boxes for unresolved plugins; say why. Export fails
    // for such clips instead of writing the placeholders.
    let error = problems
        .first()
        .map(|first| format!("plugin load failed: {first} (text and shapes may not render)"));
    let descriptors = plugin_descriptors(&host);
    (Some(host), descriptors, error)
}

/// The clip-source plugins of `host` as plain UI descriptors, sorted by name.
pub(crate) fn plugin_descriptors(host: &PluginHost) -> Vec<PluginDescriptor> {
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
    descriptors
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

/// "1 media file", "3 media files".
pub(super) fn count_label(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

/// Why a file was refused on import, naming the file and what would have worked.
pub(super) fn unsupported_media_message(path: &Path) -> String {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    format!(
        "{name} is not a supported media file. Use PNG, JPEG, WebP or BMP images, WAV, MP3, \
         FLAC, AAC, M4A or OGG audio, or MP4, MOV, MKV or WebM video."
    )
}

/// One message for every file an import refused: the single reason, or a count plus the first.
pub(super) fn import_error_summary(errors: &[String]) -> Option<String> {
    match errors {
        [] => None,
        [only] => Some(only.clone()),
        [first, rest @ ..] => Some(format!(
            "{} could not be imported. {first}",
            count_label(rest.len() + 1, "file")
        )),
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
    // `save_amio` keeps only manifest-referenced bytes, so undoing an asset
    // removal cannot leave orphaned entries that fail the manifest check.
    animathio_format::save_amio(path, project, assets).map_err(|e| e.to_string())?;
    Ok(())
}

/// Inspector (display) value for a model value: rotation shows in degrees.
pub(super) fn field_display_value(field: &Field, model: f64) -> f64 {
    if matches!(field, Field::Transform(TransformProperty::Rotation)) {
        model.to_degrees()
    } else {
        model
    }
}

/// Model value for an inspector (display) value: rotation is stored in radians.
pub(super) fn field_model_value(field: &Field, value: f64) -> f64 {
    if matches!(field, Field::Transform(TransformProperty::Rotation)) {
        value.to_radians()
    } else {
        value
    }
}

#[cfg(test)]
mod ui_units_tests {
    use super::*;
    #[core::prelude::v1::test]
    fn rotation_degrees_round_trip() {
        let field = Field::Transform(TransformProperty::Rotation);
        let mut clip = Clip::new(
            ClipId(1),
            ClipKind::Image(AssetId::of(b"rotation")),
            animathio_core::TimeRange::new(Time(0), Time(1)).unwrap(),
        );
        clip.transform.rotation = std::f64::consts::FRAC_PI_2;
        assert!((field_value(&clip, &field) - 90.).abs() < 1e-10);
        let changed = inspector::with_transform(
            clip.transform,
            TransformProperty::Rotation,
            field_model_value(&field, -45.),
        );
        assert!((changed.rotation + std::f64::consts::FRAC_PI_4).abs() < 1e-10);
        for degrees in [-360., -45., 0., 90., 180., 720.] {
            assert!((field_model_value(&field, degrees).to_degrees() - degrees).abs() < 1e-10);
        }
        assert!(
            (field_model_value(&Field::Transform(TransformProperty::X), 0.25) - 0.25).abs() < 1e-10
        );
    }
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

#[cfg(test)]
mod clip_paint_tests {
    use super::*;

    fn luminance(rgb: u32) -> f64 {
        let channel = |shift: u32| {
            let value = f64::from((rgb >> shift) & 0xFF) / 255.0;
            if value <= 0.03928 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    }

    fn contrast(a: u32, b: u32) -> f64 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    /// `fg` at `alpha` over `bg`, composited in sRGB space like the framebuffer blend.
    fn composite(fg: u32, alpha: f64, bg: u32) -> u32 {
        (0..3).fold(0u32, |acc, step| {
            let shift = (2 - step) * 8;
            let front = f64::from((fg >> shift) & 0xFF);
            let back = f64::from((bg >> shift) & 0xFF);
            (acc << 8) | ((front * alpha + back * (1. - alpha)).round() as u32 & 0xFF)
        })
    }

    const KINDS: [(&str, u32); 9] = [
        ("video", colors::CLIP_VIDEO),
        ("image", colors::CLIP_IMAGE),
        ("audio", colors::CLIP_AUDIO),
        ("text", colors::CLIP_TEXT),
        ("latex", colors::CLIP_LATEX),
        ("shape", colors::CLIP_SHAPE),
        ("graph", colors::CLIP_GRAPH),
        ("plugin", colors::CLIP_PLUGIN),
        ("error", colors::CLIP_ERROR),
    ];

    /// The name stays at 7:1 on the resting body and at AA on the hovered one, and the
    /// duration (`TEXT` at the scrim step) stays at AA (4.5:1) on both, for every kind.
    #[core::prelude::v1::test]
    fn clip_name_and_duration_are_readable_on_resting_and_hovered_bodies() {
        let duration_alpha = f64::from(style::Alpha::Scrim.value());
        for (kind, token) in KINDS {
            for (state, alpha, name_min) in [
                ("resting", f64::from(style::Alpha::Body.value()), 7.0),
                ("hovered", f64::from(CLIP_HOVER_ALPHA), 6.5),
            ] {
                let body = composite(token, alpha, colors::RAISED);
                let name = contrast(colors::TEXT, body);
                assert!(name >= name_min, "{kind} {state}: name {name:.2}:1");
                let duration_ink = composite(colors::TEXT, duration_alpha, body);
                let duration = contrast(duration_ink, body);
                assert!(duration >= 4.5, "{kind} {state}: duration {duration:.2}:1");
            }
        }
    }

    #[core::prelude::v1::test]
    fn graph_and_latex_clips_have_their_own_colours() {
        let theme = gpui_component::Theme::default();
        let plugin = |id: &str| ClipKind::Plugin {
            plugin_id: id.into(),
            params: serde_json::Value::Null,
        };
        assert_eq!(
            clip_color(&plugin("org.animathio.graph"), &theme),
            style::hex(colors::CLIP_GRAPH)
        );
        assert_eq!(
            clip_color(&plugin("org.animathio.latex"), &theme),
            style::hex(colors::CLIP_LATEX)
        );
        assert_eq!(
            clip_color(&plugin("org.animathio.text"), &theme),
            style::hex(colors::CLIP_TEXT)
        );
        assert_eq!(
            clip_color(&plugin("org.animathio.shapes"), &theme),
            style::hex(colors::CLIP_SHAPE)
        );
        assert_eq!(
            clip_color(&plugin("io.example.thing"), &theme),
            style::hex(colors::CLIP_PLUGIN)
        );
    }
}
