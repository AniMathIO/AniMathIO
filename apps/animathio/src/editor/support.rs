//! Shared pure helpers, drawing and resource adapters.

use super::*;

pub(super) fn button(
    id: &'static str,
    label: impl Into<SharedString>,
    listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Button {
    Button::new(id)
        .secondary()
        .compact()
        .label(label)
        .on_click(listener)
}

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

/// A track-header icon button: the extra-small size keeps two lines in a row.
pub(super) fn header_button(
    id: &'static str,
    icon: gpui_component::Icon,
    tooltip: impl Into<SharedString>,
    listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Button {
    icon_button(id, icon, tooltip, listener).xsmall()
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
    let background = snapshot.theme.background;
    let ruler_bg = snapshot.theme.popover;
    let border = snapshot.theme.border;
    let muted = snapshot.theme.muted_foreground;
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
                background
            } else {
                cx.theme().muted
            },
        ));
    }
    for clip in &snapshot.clips {
        paint_clip(bounds, &layout, clip, window, cx);
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
            snapshot.theme.primary.opacity(0.25),
        ));
        for x in [x0, x1] {
            window.paint_quad(fill(
                Bounds::new(point(x, bounds.origin.y), size(px(2.), px(height))),
                snapshot.theme.primary.opacity(0.65),
            ));
            window.paint_quad(fill(
                Bounds::new(point(x, bounds.origin.y), size(px(8.), px(6.))),
                snapshot.theme.primary,
            ));
        }
    }
    if let Some(time) = snapshot.snap_line {
        let x = bounds.origin.x + px(timeline::time_to_x(&layout, time));
        window.paint_quad(fill(
            Bounds::new(point(x, bounds.origin.y), size(px(2.), px(height))),
            gpui::rgb(0xffd43b),
        ));
    }
    let head_x = bounds.origin.x + px(timeline::time_to_x(&layout, snapshot.position));
    let head = Bounds::from_corners(
        point(head_x, bounds.origin.y),
        point(head_x + px(1.5), bounds.origin.y + px(height)),
    );
    window.paint_quad(fill(head, cx.theme().danger));
    window.paint_quad(fill(
        Bounds::new(
            point(head_x - px(4.), bounds.origin.y),
            size(px(10.), px(8.)),
        ),
        cx.theme().danger,
    ));
}

/// Paint one clip block: kind-coloured body, waveform, fade ramps, outline, trim
/// handles, kind icon, label, lock badge and keyframe diamonds.
#[allow(clippy::too_many_lines)]
pub(super) fn paint_clip(
    bounds: Bounds<Pixels>,
    layout: &TimelineLayout,
    clip: &ClipPaint,
    window: &mut Window,
    cx: &mut App,
) {
    let x0 = bounds.origin.x + px(timeline::time_to_x(layout, clip.start));
    let x1 = bounds.origin.x + px(timeline::time_to_x(layout, clip.end));
    let y0 = bounds.origin.y + px(RULER_H + clip.row as f32 * layout.row_height + 3.0);
    let y1 = bounds.origin.y + px(RULER_H + (clip.row as f32 + 1.0) * layout.row_height - 3.0);
    let block = Bounds::from_corners(point(x0, y0), point(x1, y1));
    let kind_color = clip_color(&clip.kind, cx.theme());
    let dim = if clip.silenced { 0.5 } else { 1.0 };
    window.paint_quad(fill(block, kind_color.opacity(0.45 * dim)));
    window.paint_quad(outline(block, kind_color.opacity(dim), BorderStyle::Solid));

    if let Some(wave) = &clip.waveform {
        paint_waveform(bounds, layout, clip, wave, (x0, x1, y0, y1), window, cx);
    }
    paint_fades(clip, (x0, x1, y0, y1), window, cx);

    if clip.selected {
        window.paint_quad(outline(block, cx.theme().ring, BorderStyle::Solid));
        window.paint_quad(outline(
            Bounds::from_corners(
                point(x0 + px(1.0), y0 + px(1.0)),
                point(x1 - px(1.0), y1 - px(1.0)),
            ),
            cx.theme().ring,
            BorderStyle::Solid,
        ));
    }
    // Trim handles only on the hovered or selected clip.
    if clip.selected || clip.hovered {
        let handle_w = px(3.0);
        for edge_x in [x0, x1] {
            let handle =
                Bounds::from_corners(point(edge_x - handle_w, y0), point(edge_x + handle_w, y1));
            window.paint_quad(
                fill(handle, cx.theme().foreground.opacity(0.85)).corner_radii(px(1.5)),
            );
        }
    }
    // Kind icon and label, only when the block is wide enough to hold them.
    if x1 - x0 > px(28.0) {
        let icon_bounds = Bounds::new(point(x0 + px(5.0), y0 + px(3.0)), size(px(12.0), px(12.0)));
        paint_icon(clip.icon, icon_bounds, cx.theme().foreground, window, cx);
        if !clip.label.is_empty() {
            paint_clip_label(
                clip.label.clone(),
                point(x0 + px(20.0), y0 + px(3.0)),
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
    for keyframe in &clip.keyframes {
        let kx = bounds.origin.x + px(timeline::time_to_x(layout, keyframe.time));
        let cy = y1 - px(10.0);
        let half = px(4.0);
        let mut path = gpui::Path::new(point(kx, cy - half));
        path.line_to(point(kx + half, cy));
        path.line_to(point(kx, cy + half));
        path.line_to(point(kx - half, cy));
        path.line_to(point(kx, cy - half));
        window.paint_path(
            path,
            if keyframe.selected {
                cx.theme().foreground
            } else {
                cx.theme().warning
            },
        );
    }
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
    window: &mut Window,
    cx: &mut App,
) {
    const STEP: f32 = 2.0;
    let left = f32::from(x0).max(f32::from(bounds.origin.x));
    let right = f32::from(x1).min(f32::from(bounds.origin.x) + f32::from(bounds.size.width));
    if right <= left || layout.pixels_per_tick <= 0.0 {
        return;
    }
    let mid = f32::midpoint(f32::from(y0), f32::from(y1)) + 5.0;
    let half_max = (f32::from(y1) - f32::from(y0)) / 2.0 - 8.0;
    if half_max <= 1.0 {
        return;
    }
    let ticks_per_px = 1.0 / layout.pixels_per_tick;
    let color = cx.theme().foreground.opacity(0.6);
    let mut x = left;
    // Bounded by the canvas width: at most `width / STEP` bars.
    while x < right {
        let source_start = ((f64::from(x - f32::from(x0)) * ticks_per_px) as i64)
            .saturating_add(clip.source_offset);
        let source_end =
            source_start.saturating_add((f64::from(STEP) * ticks_per_px).ceil() as i64);
        let peak = wave.peak_between(source_start, source_end.max(source_start + 1));
        let half = (peak * half_max).max(0.5);
        let bar = Bounds::from_corners(
            point(px(x), px(mid - half)),
            point(px((x + 1.0).min(right)), px(mid + half)),
        );
        window.paint_quad(fill(bar, color));
        x += STEP;
    }
}

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
    let shade = cx.theme().background.opacity(0.45);
    let ppt = width / (clip.end.0 - clip.start.0).max(1) as f32;
    let fade_in_px = (clip.fade_in as f32 * ppt).min(width);
    let fade_out_px = (clip.fade_out as f32 * ppt).min(width);
    if fade_in_px > 1.0 {
        let mut path = gpui::Path::new(point(x0, y0));
        path.line_to(point(x0 + px(fade_in_px), y0));
        path.line_to(point(x0, y1));
        path.line_to(point(x0, y0));
        window.paint_path(path, shade);
    }
    if fade_out_px > 1.0 {
        let mut path = gpui::Path::new(point(x1, y0));
        path.line_to(point(x1 - px(fade_out_px), y0));
        path.line_to(point(x1, y1));
        path.line_to(point(x1, y0));
        window.paint_path(path, shade);
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
                        cx.theme().warning
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
    let minor = (step / 2).max(1);
    for tick in timeline::ruler_ticks(layout, minor) {
        let x = bounds.origin.x + px(timeline::time_to_x(layout, Time(tick)));
        let is_major = tick % step == 0;
        let mark_top = bounds.origin.y + px(if is_major { 8.0 } else { 16.0 });
        let mark = Bounds::from_corners(
            point(x, mark_top),
            point(x + px(1.0), bounds.origin.y + px(RULER_H)),
        );
        window.paint_quad(fill(mark, if is_major { muted } else { border }));
        if is_major {
            let label = timecode::format_timecode(Time(tick), snapshot.frame_rate);
            paint_clip_label(
                label.into(),
                point(x + px(3.0), bounds.origin.y + px(12.0)),
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

/// A source-kind colour from the theme's semantic palette.
pub(super) fn clip_color(kind: &animathio_core::ClipKind, theme: &gpui_component::Theme) -> Hsla {
    match kind {
        animathio_core::ClipKind::Video(_) => theme.blue,
        animathio_core::ClipKind::Image(_) => theme.cyan,
        animathio_core::ClipKind::Audio(_) => theme.green,
        animathio_core::ClipKind::Builtin(builtin) => match builtin {
            animathio_core::builtin::Builtin::Text { .. } => theme.magenta,
            animathio_core::builtin::Builtin::Shape { .. } => theme.yellow,
        },
        animathio_core::ClipKind::Plugin { plugin_id, .. } => {
            if plugin_id.contains("latex") || plugin_id.contains("math") {
                theme.primary
            } else if plugin_id.contains("graph") {
                theme.chart_2
            } else {
                theme.accent
            }
        }
        animathio_core::ClipKind::Vector(_) => theme.accent,
        animathio_core::ClipKind::Error(_) => theme.danger,
    }
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
        color: cx.theme().foreground,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window
        .text_system()
        .shape_line(label, cx.theme().font_size, &[run], None);
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
        ClipKind::Builtin(builtin) => match builtin {
            animathio_core::builtin::Builtin::Text { .. } => "text".into(),
            animathio_core::builtin::Builtin::Shape { .. } => "shape".into(),
        },
        ClipKind::Plugin { plugin_id, .. } => format!("plugin {plugin_id}"),
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

/// Every built-in effect, in a stable order for menus and tabs.
pub(super) fn effect_kinds() -> [animathio_core::EffectKind; 8] {
    [
        animathio_core::EffectKind::Grayscale,
        animathio_core::EffectKind::Sepia,
        animathio_core::EffectKind::Invert,
        animathio_core::EffectKind::Saturate,
        animathio_core::EffectKind::Brightness,
        animathio_core::EffectKind::Contrast,
        animathio_core::EffectKind::Blur,
        animathio_core::EffectKind::Opacity,
    ]
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

/// Human-readable effect name for the effects tab and inspector.
pub(super) fn effect_label(kind: animathio_core::EffectKind) -> &'static str {
    match kind {
        animathio_core::EffectKind::Grayscale => "Grayscale",
        animathio_core::EffectKind::Sepia => "Sepia",
        animathio_core::EffectKind::Invert => "Invert",
        animathio_core::EffectKind::Saturate => "Saturate",
        animathio_core::EffectKind::Brightness => "Brightness",
        animathio_core::EffectKind::Contrast => "Contrast",
        animathio_core::EffectKind::Blur => "Blur",
        animathio_core::EffectKind::Opacity => "Opacity",
    }
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
    let mut error = None;
    for dir in animathio_plugin_host::plugin_search_dirs() {
        match host.load_plugin_directory(&dir) {
            Ok(_) => {}
            Err(message) => {
                eprintln!("plugin load failed from {}: {message}", dir.path.display());
                error.get_or_insert_with(|| format!("plugin load failed: {message}"));
            }
        }
    }
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
