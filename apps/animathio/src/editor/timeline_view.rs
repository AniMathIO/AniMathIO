//! Timeline view.

use super::*;

impl Editor {
    pub(super) fn on_timeline_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.panel_focus[0], cx);
        self.context_menu = None;
        let (x, y) = self.timeline_relative(event.position);
        let layout = self.current_layout();
        if y < layout.ruler_height {
            self.selected_keyframe = None;
            self.gesture = Some(TimelineGesture::Scrub);
            self.scrub_to(x, cx);
            return;
        }
        if self.tool == Tool::Hand {
            self.gesture = Some(TimelineGesture::Pan {
                x,
                scroll: self.timeline_scroll,
            });
            return;
        }
        if self.tool == Tool::Blade {
            let at = timeline::drop_start(&layout, &self.project, x, self.position, self.snapping);
            self.snap_line = self
                .snapping
                .then(|| {
                    timeline::snap_target(
                        &layout,
                        &self.project,
                        None,
                        self.position,
                        timeline::x_to_time(&layout, x),
                    )
                })
                .flatten();
            if event.modifiers.shift {
                self.execute(&Command::SplitAt { times: vec![at] }, cx);
            } else if let Some(hit) = timeline::hit_test(&layout, &self.project, x, y) {
                let right_id = self.next_clip_id();
                if let Some(command) = self
                    .find_clip(hit.clip)
                    .and_then(|clip| editing::blade_command(clip, at, right_id))
                {
                    self.execute(&command, cx);
                }
            }
            return;
        }
        if let Some((clip_id, edge)) = self.hit_fade_handle(&layout, x, y) {
            self.selected = Some(clip_id);
            self.selected_field = None;
            self.selected_keyframe = None;
            let current = self
                .find_clip(clip_id)
                .map_or(0, |clip| fades::fade_len(clip, edge));
            self.gesture = Some(TimelineGesture::Fade(FadeDrag {
                clip_id,
                edge,
                current,
            }));
            cx.notify();
            return;
        }
        if let Some(selection) = self.hit_keyframe(&layout, x, y) {
            let clip = self.find_clip(selection.clip_id).cloned();
            self.selected = Some(selection.clip_id);
            self.selected_field = None;
            self.selected_keyframe = Some(selection.clone());
            if let Some(clip) = clip {
                self.keyframe_drag = Some(KeyframeDrag {
                    clip_id: selection.clip_id,
                    target: selection.target.clone(),
                    clip_start: clip.range.start,
                    origin: selection.time,
                    current: selection.time,
                });
            }
            cx.notify();
            return;
        }
        self.selected_keyframe = None;
        if let Some((hit, edge)) = timeline::hit_trim_edge(&layout, &self.project, x, y)
            && let Some(clip) = self.find_clip(hit.clip).cloned()
        {
            self.selected = Some(hit.clip);
            self.selected_field = None;
            self.gesture = Some(TimelineGesture::Trim(TrimDrag {
                clip_id: hit.clip,
                edge,
                origin_start: clip.range.start,
                origin_end: clip.range.end,
                origin_source_offset: clip.source_offset,
                ripple: self.tool == Tool::Ripple || event.modifiers.control,
                current: match edge {
                    TrimEdge::Left => clip.range.start,
                    TrimEdge::Right => clip.range.end,
                },
            }));
            cx.notify();
            return;
        }
        if let Some(hit) = timeline::hit_test(&layout, &self.project, x, y) {
            self.selected = Some(hit.clip);
            self.selected_field = None;
            if let Some(clip) = self.find_clip(hit.clip).cloned() {
                let grab_offset = timeline::x_to_time(&layout, x).0 - clip.range.start.0;
                self.gesture = Some(TimelineGesture::Drag(ClipDrag {
                    clip_id: clip.id,
                    origin_track: hit.track,
                    target_track: hit.track,
                    origin_start: clip.range.start,
                    duration: clip.range.end.0 - clip.range.start.0,
                    grab_offset,
                    current_start: clip.range.start,
                }));
            }
        } else {
            self.selected = None;
            self.selected_field = None;
            self.gesture = Some(TimelineGesture::Scrub);
            self.scrub_to(x, cx);
        }
        cx.notify();
    }

    /// The fade handle under a canvas-relative point, for the selected or hovered
    /// clip only (the same clips that draw handles).
    pub(super) fn hit_fade_handle(
        &self,
        layout: &TimelineLayout,
        x: f32,
        y: f32,
    ) -> Option<(ClipId, FadeEdge)> {
        for (row, track) in self.project.tracks().iter().enumerate() {
            let top = layout.ruler_height + row as f32 * layout.row_height + 3.0;
            if (y - (top + FADE_HANDLE_INSET)).abs() > 7.0 {
                continue;
            }
            for clip in track.clips().values() {
                if self.selected != Some(clip.id) && self.hovered_clip != Some(clip.id) {
                    continue;
                }
                let x0 = timeline::time_to_x(layout, clip.range.start);
                let x1 = timeline::time_to_x(layout, clip.range.end);
                if x1 - x0 < 40.0 {
                    continue;
                }
                let ppt = layout.pixels_per_tick as f32;
                let in_x =
                    x0 + fades::fade_len(clip, FadeEdge::In) as f32 * ppt + FADE_HANDLE_INSET;
                let out_x =
                    x1 - fades::fade_len(clip, FadeEdge::Out) as f32 * ppt - FADE_HANDLE_INSET;
                if (x - in_x).abs() <= 7.0 {
                    return Some((clip.id, FadeEdge::In));
                }
                if (x - out_x).abs() <= 7.0 {
                    return Some((clip.id, FadeEdge::Out));
                }
            }
        }
        None
    }

    pub(super) fn hit_keyframe(
        &self,
        layout: &TimelineLayout,
        x: f32,
        y: f32,
    ) -> Option<KeyframeSelection> {
        let selected = self.selected?;
        if y < layout.ruler_height {
            return None;
        }
        let row = ((y - layout.ruler_height) / layout.row_height.max(1.0)) as usize;
        let mut found = None;
        for (clip_row, clip) in self
            .project
            .tracks()
            .iter()
            .enumerate()
            .flat_map(|(row, track)| track.clips().values().map(move |clip| (row, clip)))
        {
            if clip.id != selected {
                continue;
            }
            if clip_row != row {
                return None;
            }
            for animation in &clip.animations {
                for local in animation.keys.keys().keys() {
                    let time = Time(clip.range.start.0 + local.0);
                    let kx = timeline::time_to_x(layout, time);
                    if (x - kx).abs() <= 6.0 {
                        found = Some(KeyframeSelection {
                            clip_id: selected,
                            target: animation.target.clone(),
                            time: *local,
                        });
                    }
                }
            }
        }
        found
    }

    pub(super) fn on_timeline_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (x, y) = self.timeline_relative(event.position);
        let layout = self.current_layout();
        let hovered = timeline::hit_test(&layout, &self.project, x, y).map(|hit| hit.clip);
        if hovered != self.hovered_clip {
            self.hovered_clip = hovered;
            cx.notify();
        }
        if self.pool_drag.is_some() {
            self.snap_line = if self.snapping {
                timeline::snap_target(
                    &layout,
                    &self.project,
                    None,
                    self.position,
                    timeline::x_to_time(&layout, x),
                )
            } else {
                None
            };
            cx.notify();
            return;
        }
        if self.tool == Tool::Blade && !event.dragging() {
            let raw = timeline::x_to_time(&layout, x);
            self.snap_line = self
                .snapping
                .then(|| timeline::snap_target(&layout, &self.project, None, self.position, raw))
                .flatten();
            cx.notify();
        }
        if !event.dragging() {
            return;
        }
        if let Some(drag) = &mut self.keyframe_drag {
            let raw = timeline::x_to_time(&layout, x);
            drag.current = Time((raw.0 - drag.clip_start.0).max(0));
            cx.notify();
            return;
        }
        self.snap_line = None;
        match &mut self.gesture {
            Some(TimelineGesture::Pan { x: origin, scroll }) => {
                self.timeline_scroll =
                    (*scroll + *origin - x).clamp(0.0, timeline::max_scroll(&layout));
                cx.notify();
            }
            Some(TimelineGesture::Drag(drag)) => {
                let raw = timeline::drag_start(&layout, drag.grab_offset, x);
                drag.current_start = if self.snapping {
                    timeline::snap_start(&layout, &self.project, drag.clip_id, self.position, raw)
                } else {
                    raw
                };
                self.snap_line = if self.snapping {
                    timeline::snap_target(
                        &layout,
                        &self.project,
                        Some(drag.clip_id),
                        self.position,
                        raw,
                    )
                } else {
                    None
                };
                let max_row = self.project.tracks().len().saturating_sub(1) as f32;
                let row = ((y - layout.ruler_height) / layout.row_height.max(1.0))
                    .clamp(0.0, max_row) as usize;
                if let Some(track) = self.project.tracks().get(row) {
                    drag.target_track = track.id();
                }
                cx.notify();
            }
            Some(TimelineGesture::Trim(trim)) => {
                let raw = timeline::x_to_time(&layout, x);
                let snapped = if self.snapping {
                    timeline::snap_start(&layout, &self.project, trim.clip_id, self.position, raw)
                } else {
                    raw
                };
                self.snap_line = if self.snapping {
                    timeline::snap_target(
                        &layout,
                        &self.project,
                        Some(trim.clip_id),
                        self.position,
                        raw,
                    )
                } else {
                    None
                };
                trim.current = match trim.edge {
                    TrimEdge::Left => Time(snapped.0.clamp(
                        (trim.origin_start.0 - trim.origin_source_offset.0).max(0),
                        trim.origin_end.0.saturating_sub(1),
                    )),
                    TrimEdge::Right => Time(
                        snapped
                            .0
                            .clamp(trim.origin_start.0.saturating_add(1), i64::MAX),
                    ),
                };
                if self.snap_line.is_some_and(|target| target != trim.current) {
                    self.snap_line = None;
                }
                cx.notify();
            }
            Some(TimelineGesture::Fade(fade)) => {
                let Some(clip) = self
                    .project
                    .tracks()
                    .iter()
                    .find_map(|track| track.clips().get(&fade.clip_id))
                else {
                    return;
                };
                let pointer = timeline::x_to_time(&layout, x).0;
                let inset = (f64::from(FADE_HANDLE_INSET) / layout.pixels_per_tick) as i64;
                let raw = match fade.edge {
                    FadeEdge::In => pointer - clip.range.start.0 - inset,
                    FadeEdge::Out => clip.range.end.0 - pointer - inset,
                };
                fade.current = raw.clamp(0, fades::max_fade(clip, fade.edge));
                cx.notify();
            }
            Some(TimelineGesture::Scrub) => self.scrub_to(x, cx),
            None => {}
        }
    }

    pub(super) fn on_timeline_mouse_up(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.snap_line = None;
        cx.notify();
        if let Some(drag) = self.keyframe_drag.take() {
            self.selected_keyframe = Some(KeyframeSelection {
                clip_id: drag.clip_id,
                target: drag.target.clone(),
                time: drag.current,
            });
            if drag.current != drag.origin {
                self.execute(
                    &Command::MoveKeyframe {
                        id: drag.clip_id,
                        target: drag.target,
                        from: drag.origin,
                        to: drag.current,
                    },
                    cx,
                );
            }
            cx.notify();
            return;
        }
        match self.gesture.take() {
            Some(TimelineGesture::Drag(drag)) => {
                if drag.target_track == drag.origin_track && drag.current_start == drag.origin_start
                {
                    return;
                }
                self.execute(
                    &Command::MoveClip {
                        id: drag.clip_id,
                        track: drag.target_track,
                        start: drag.current_start,
                    },
                    cx,
                );
            }
            Some(TimelineGesture::Trim(trim)) => {
                if let Some(command) = editing::trim_command(
                    &self.project,
                    trim.clip_id,
                    trim.edge,
                    trim.current,
                    trim.ripple,
                ) {
                    self.execute(&command, cx);
                }
            }

            Some(TimelineGesture::Fade(fade)) => {
                if let Some(command) = self
                    .find_clip(fade.clip_id)
                    .and_then(|clip| fades::fade_command(clip, fade.edge, fade.current))
                {
                    self.execute(&command, cx);
                }
                cx.notify();
            }
            _ => {}
        }
    }

    pub(super) fn on_timeline_context_menu(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (x, y) = self.timeline_relative(event.position);
        let layout = self.current_layout();
        if let Some(selection) = self.hit_keyframe(&layout, x, y) {
            self.selected = Some(selection.clip_id);
            self.selected_keyframe = Some(selection.clone());
            self.context_menu = Some(ContextMenu::Keyframe(selection));
        } else if let Some(hit) = timeline::hit_test(&layout, &self.project, x, y) {
            self.selected = Some(hit.clip);
            self.selected_keyframe = None;
            self.context_menu = Some(ContextMenu::Clip { id: hit.clip });
        } else {
            self.context_menu = None;
        }
        self.context_menu_pos = Some(event.position);
        cx.notify();
    }

    pub(super) fn on_timeline_scroll_wheel(
        &mut self,
        event: &ScrollEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (x, _y) = self.timeline_relative(event.position);
        let (dx, dy) = match event.delta {
            ScrollDelta::Pixels(point) => (f32::from(point.x), f32::from(point.y)),
            ScrollDelta::Lines(point) => (point.x * 20.0, point.y * 20.0),
        };
        if event.modifiers.control || event.modifiers.platform {
            let layout = self.current_layout();
            let anchor = timeline::x_to_time(&layout, x);
            let factor = (dy * 0.002).exp();
            self.timeline_zoom = (self.timeline_zoom * factor).clamp(1.0, 64.0);
            let new_layout = self.current_layout();
            let new_x = timeline::time_to_x(&new_layout, anchor);
            self.timeline_scroll += new_x - x;
        } else if event.modifiers.alt || dx != 0.0 {
            self.timeline_scroll -= dy;
            if dx != 0.0 {
                self.timeline_scroll -= dx;
            }
        }
        if event.modifiers.control || event.modifiers.platform || event.modifiers.alt {
            cx.stop_propagation();
        }
        let layout = self.current_layout();
        self.timeline_scroll = self
            .timeline_scroll
            .clamp(0.0, timeline::max_scroll(&layout));
        cx.notify();
    }

    /// Cached downsampled peaks for a decoded audio asset.
    pub(super) fn waveform_for(&self, id: &AssetId) -> Option<Arc<Waveform>> {
        if let Some(wave) = self.waveforms.borrow().get(id) {
            return Some(wave.clone());
        }
        let wave = Arc::new(self.pool.waveform(id, WAVEFORM_BUCKETS)?);
        self.waveforms.borrow_mut().insert(id.clone(), wave.clone());
        Some(wave)
    }

    pub(super) fn build_timeline_snapshot(&self, cx: &App) -> TimelineSnapshot {
        let duration = timeline::project_duration(&self.project);
        let mut clips = Vec::new();
        let any_solo = self.project.tracks().iter().any(Track::is_solo);
        for (row, track) in self.project.tracks().iter().enumerate() {
            let silenced = track.is_muted() || (any_solo && !track.is_solo());
            for clip in track.clips().values() {
                let (start, end) = if let Some(TimelineGesture::Drag(drag)) = &self.gesture
                    && drag.clip_id == clip.id
                {
                    (
                        drag.current_start,
                        Time(drag.current_start.0 + drag.duration),
                    )
                } else if let Some(TimelineGesture::Trim(trim)) = &self.gesture
                    && trim.clip_id == clip.id
                {
                    match trim.edge {
                        TrimEdge::Left => (trim.current, trim.origin_end),
                        TrimEdge::Right => (trim.origin_start, trim.current),
                    }
                } else {
                    (clip.range.start, clip.range.end)
                };
                let mut keyframes = Vec::new();
                if self.selected == Some(clip.id) {
                    for animation in &clip.animations {
                        for local in animation.keys.keys().keys() {
                            let time = if let Some(drag) = &self.keyframe_drag
                                && drag.clip_id == clip.id
                                && drag.target == animation.target
                                && drag.origin == *local
                            {
                                drag.current
                            } else {
                                *local
                            };
                            keyframes.push(KeyframePaint {
                                time: Time(clip.range.start.0 + time.0),
                                selected: self.selected_keyframe.as_ref().is_some_and(
                                    |selection| {
                                        selection.clip_id == clip.id
                                            && selection.target == animation.target
                                            && selection.time == time
                                    },
                                ),
                            });
                        }
                    }
                }
                let (mut fade_in, mut fade_out) = (
                    fades::fade_len(clip, FadeEdge::In),
                    fades::fade_len(clip, FadeEdge::Out),
                );
                if let Some(TimelineGesture::Fade(drag)) = &self.gesture
                    && drag.clip_id == clip.id
                {
                    match drag.edge {
                        FadeEdge::In => fade_in = drag.current,
                        FadeEdge::Out => fade_out = drag.current,
                    }
                }
                let waveform = match &clip.kind {
                    ClipKind::Audio(asset) => self.waveform_for(asset),
                    _ => None,
                };
                clips.push(ClipPaint {
                    row,
                    start,
                    end,
                    selected: self.selected == Some(clip.id),
                    hovered: self.hovered_clip == Some(clip.id),
                    label: clip_label(&self.project, clip).into(),
                    kind: clip.kind.clone(),
                    icon: icons::kind_icon(&clip.kind),
                    keyframes,
                    waveform,
                    source_offset: clip.source_offset.0,
                    fade_in,
                    fade_out,
                    locked: track.is_locked(),
                    silenced,
                });
            }
        }
        TimelineSnapshot {
            duration,
            rows: self.project.tracks().len(),
            clips,
            position: self.position,
            frame_rate: self.project.frame_rate(),
            row_height: self.dock.track_height.pixels(),
            marks: self.marks,
            snap_line: self.snap_line,
            theme: cx.theme().clone(),
        }
    }

    pub(super) fn timeline_panel(&self, cx: &mut Context<Self>) -> Div {
        let snapshot = self.build_timeline_snapshot(cx);
        let zoom = self.timeline_zoom;
        let scroll = self.timeline_scroll;
        let weak = cx.entity().downgrade();
        let weak_paint = weak.clone();
        let snapping = self.snapping;
        let content_height =
            RULER_H + self.project.tracks().len() as f32 * self.dock.track_height.pixels();
        let pool_dragging = self.pool_drag.is_some();

        div()
            .h_full()
            .key_context("Timeline")
            .track_focus(&self.panel_focus[0])
            .w_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .child(
                div()
                    .h(px(30.))
                    .flex_shrink_0()
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .bg(cx.theme().background)
                    .children(
                        [
                            (
                                Tool::Select,
                                IconName::MousePointer2,
                                "Select",
                                "tool-select",
                            ),
                            (
                                Tool::Blade,
                                IconName::Scissors,
                                "Blade; Shift-click all tracks",
                                "tool-blade",
                            ),
                            (
                                Tool::Ripple,
                                IconName::FoldHorizontal,
                                "Ripple trim",
                                "tool-ripple",
                            ),
                            (Tool::Hand, IconName::Hand, "Pan", "tool-hand"),
                        ]
                        .map(|(tool, icon, label, id)| {
                            icon_button(
                                id,
                                icons::icon(icon),
                                label,
                                cx.listener(move |this, _, _, cx| {
                                    this.tool = tool;
                                    this.snap_line = None;
                                    cx.notify();
                                }),
                            )
                            .selected(self.tool == tool)
                        }),
                    )
                    .child(icon_button(
                        "mark-in",
                        icons::icon(IconName::LogIn),
                        "Set in",
                        cx.listener(|this, _, _, cx| {
                            this.marks.set_in(this.position);
                            cx.notify();
                        }),
                    ))
                    .child(icon_button(
                        "mark-out",
                        icons::icon(IconName::LogOut),
                        "Set out (exclusive)",
                        cx.listener(|this, _, _, cx| {
                            this.marks.set_out(this.position);
                            cx.notify();
                        }),
                    ))
                    .child(icon_button(
                        "zoom-fit",
                        icons::icon(IconName::FoldHorizontal),
                        "Zoom to fit",
                        cx.listener(|this, _, _, cx| {
                            this.timeline_zoom = 1.0;
                            this.timeline_scroll = 0.0;
                            cx.notify();
                        }),
                    ))
                    .child(
                        icon_button(
                            "snap",
                            icons::icon(IconName::Magnet),
                            if snapping {
                                "Snapping: on"
                            } else {
                                "Snapping: off"
                            },
                            cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_snapping(cx)),
                        )
                        .selected(snapping)
                        .toggled(snapping),
                    )
                    .children(
                        [
                            (crate::layout::TrackHeight::Small, "S"),
                            (crate::layout::TrackHeight::Medium, "M"),
                            (crate::layout::TrackHeight::Large, "L"),
                        ]
                        .map(|(height, label)| {
                            Button::new(label)
                                .ghost()
                                .compact()
                                .label(label)
                                .selected(self.dock.track_height == height)
                                .tooltip(format!("Track height: {label}"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.set_track_height(height, cx);
                                }))
                        }),
                    )
                    .child(icon_button(
                        "zoom-out",
                        icons::icon(IconName::ZoomOut),
                        "Zoom out",
                        cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.timeline_zoom = (this.timeline_zoom / 1.25).clamp(1.0, 64.0);
                            cx.notify();
                        }),
                    ))
                    .child(div().text_color(cx.theme().muted_foreground).child(format!(
                        "{:.1} s",
                        timeline::project_duration(&self.project) as f64
                            / TICKS_PER_SECOND as f64
                            / f64::from(zoom)
                    )))
                    .child(icon_button(
                        "zoom-in",
                        icons::icon(IconName::ZoomIn),
                        "Zoom in",
                        cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.timeline_zoom = (this.timeline_zoom * 1.25).clamp(1.0, 64.0);
                            cx.notify();
                        }),
                    )),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .child(
                        div()
                            .id("timeline-track-scroll")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.timeline_scroll_handle)
                            .child(
                                div()
                                    .h(px(content_height))
                                    .flex_shrink_0()
                                    .w_full()
                                    .flex()
                                    .flex_row()
                                    .child(self.track_header(cx))
                                    .child(
                                        div()
                                            .flex_1()
                                            .h_full()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(Self::on_timeline_mouse_down),
                                            )
                                            .on_mouse_down(
                                                MouseButton::Right,
                                                cx.listener(Self::on_timeline_context_menu),
                                            )
                                            .on_mouse_move(
                                                cx.listener(Self::on_timeline_mouse_move),
                                            )
                                            .on_mouse_up(
                                                MouseButton::Left,
                                                cx.listener(Self::on_timeline_mouse_up),
                                            )
                                            .on_scroll(cx.listener(Self::on_timeline_scroll_wheel))
                                            .on_drop::<ExternalPaths>(cx.listener(
                                                |this, paths: &ExternalPaths, window, cx| {
                                                    let position = window.mouse_position();
                                                    let (x, y) = this.timeline_relative(position);
                                                    let layout = this.current_layout();
                                                    let placement = timeline::drop_placement(
                                                        &layout,
                                                        &this.project,
                                                        x,
                                                        y,
                                                        this.position,
                                                        this.snapping,
                                                    );
                                                    if let Some(placement) = placement {
                                                        this.import_paths(
                                                            paths.paths(),
                                                            Some(placement),
                                                            cx,
                                                        );
                                                    }
                                                },
                                            ))
                                            .child(
                                                canvas(
                                                    move |bounds, _window, cx| {
                                                        let _ = weak.update(cx, |this, _| {
                                                            this.timeline_bounds = bounds;
                                                        });
                                                    },
                                                    {
                                                        let weak = weak_paint.clone();
                                                        move |bounds, (), window, cx| {
                                                            paint_timeline(
                                                                bounds, &snapshot, zoom, scroll,
                                                                window, cx,
                                                            );
                                                            if pool_dragging {
                                                                let weak = weak.clone();
                                                                window.on_mouse_event(
                                                move |event: &MouseUpEvent,
                                                      phase: DispatchPhase,
                                                      window,
                                                      cx| {
                                                    if phase == DispatchPhase::Bubble {
                                                        let position = event.position;
                                                        let _ = weak.update(cx, |this, cx| {
                                                            this.drop_on_timeline(position, cx);
                                                        });
                                                    }
                                                    window.refresh();
                                                },
                                            );
                                                                window.request_animation_frame();
                                                            }
                                                        }
                                                    },
                                                )
                                                .size_full(),
                                            ),
                                    ),
                            ),
                    )
                    .child(gpui_component::scroll::Scrollbar::vertical(
                        &self.timeline_scroll_handle,
                    )),
            )
    }

    pub(super) fn track_header(&self, cx: &mut Context<Self>) -> Div {
        let mut header = div()
            .w(px(HEADER_W))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .bg(cx.theme().popover);
        header = header.child(
            div()
                .h(px(RULER_H))
                .flex_shrink_0()
                .w_full()
                .border_b_1()
                .border_color(cx.theme().border),
        );
        for track in self.project.tracks() {
            let id = track.id();
            let name = track.name().to_owned();
            let gain = track.gain();
            let muted = track.is_muted();
            let solo = track.is_solo();
            let locked = track.is_locked();
            let controls = div()
                .w_full()
                .when(
                    self.dock.track_height == crate::layout::TrackHeight::Small,
                    |row| row.w(px(148.)).flex_shrink_0(),
                )
                .h(px(24.))
                .flex()
                .flex_row()
                .items_center()
                .child(
                    div()
                        .w(px(28.))
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("{gain:.1}")),
                )
                .child(header_button(
                    "gain-",
                    icons::icon(IconName::Minus),
                    "Lower track gain",
                    cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.nudge_track_gain(id, -0.1, cx);
                    }),
                ))
                .child(header_button(
                    "gain+",
                    icons::icon(IconName::Plus),
                    "Raise track gain",
                    cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.nudge_track_gain(id, 0.1, cx);
                    }),
                ))
                .child(div().flex_1())
                .child(
                    header_button(
                        "track-mute",
                        icons::icon(if muted {
                            IconName::VolumeX
                        } else {
                            IconName::Volume2
                        }),
                        if muted { "Unmute track" } else { "Mute track" },
                        cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.execute(&Command::SetTrackMuted { id, muted: !muted }, cx);
                        }),
                    )
                    .selected(muted)
                    .toggled(muted),
                )
                .child(
                    header_button(
                        "track-solo",
                        icons::icon(IconName::Headphones),
                        if solo { "Unsolo track" } else { "Solo track" },
                        cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.execute(&Command::SetTrackSolo { id, solo: !solo }, cx);
                        }),
                    )
                    .selected(solo)
                    .toggled(solo),
                )
                .child(
                    header_button(
                        "track-lock",
                        icons::icon(if locked {
                            IconName::Lock
                        } else {
                            IconName::LockOpen
                        }),
                        if locked { "Unlock track" } else { "Lock track" },
                        cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.execute(
                                &Command::SetTrackLocked {
                                    id,
                                    locked: !locked,
                                },
                                cx,
                            );
                        }),
                    )
                    .selected(locked)
                    .toggled(locked),
                );
            header = header.child(
                div()
                    // A per-track id scopes the buttons' hover/tooltip state.
                    .id(("track-header", id.0))
                    .h(px(self.dock.track_height.pixels()))
                    .flex_shrink_0()
                    .w_full()
                    .flex()
                    .when(
                        self.dock.track_height != crate::layout::TrackHeight::Small,
                        gpui::Styled::flex_col,
                    )
                    .items_center()
                    .justify_center()
                    .px_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .w_full()
                            .h(px(14.))
                            .truncate()
                            .text_xs()
                            .text_color(cx.theme().foreground)
                            .child(name),
                    )
                    .child(controls),
            );
        }
        header
    }
}
