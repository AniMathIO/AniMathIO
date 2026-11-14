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
        self.zoom_motion = None;
        let (x, y) = self.timeline_relative(event.position);
        let layout = self.current_layout();
        if y < layout.ruler_height {
            self.selected_keyframe = None;
            if let Some(id) = timeline::hit_marker(&layout, &self.project, x, y)
                && let Some(marker) = self.project.marker(id)
            {
                if event.click_count >= 2 {
                    self.open_marker_dialog(id, window, cx);
                } else {
                    self.gesture = Some(TimelineGesture::Marker(MarkerDrag {
                        id,
                        origin: marker.time,
                        current: marker.time,
                    }));
                    cx.notify();
                }
                return;
            }
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
            self.select_only(clip_id);
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
            self.select_only(selection.clip_id);
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
        let secondary = event.modifiers.secondary(cx);
        let extend = secondary || event.modifiers.shift;
        if !extend
            && let Some((hit, edge)) = timeline::hit_trim_edge(&layout, &self.project, x, y)
            && let Some(clip) = self.find_clip(hit.clip).cloned()
        {
            self.select_only(hit.clip);
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
        let current = self.selected_ids();
        if let Some(hit) = timeline::hit_test(&layout, &self.project, x, y) {
            self.selected_field = None;
            if secondary {
                let toggled = selection::toggle(&current, hit.clip);
                let primary = toggled.contains(&hit.clip).then_some(hit.clip);
                self.set_selection(toggled, primary.or(self.selected));
            } else if event.modifiers.shift {
                let anchor = self.selected.unwrap_or(hit.clip);
                let range = selection::range_select(&self.project, anchor, hit.clip, &current);
                self.set_selection(range, Some(anchor));
            } else {
                // Pressing on a member keeps the group so it can be dragged; a
                // click that does not move collapses to the clicked clip.
                let collapse_to =
                    (current.contains(&hit.clip) && current.len() > 1).then_some(hit.clip);
                if collapse_to.is_some() {
                    self.set_selection(current, Some(hit.clip));
                } else {
                    self.select_only(hit.clip);
                }
                self.begin_clip_drag(&layout, hit, x, collapse_to);
            }
        } else {
            // Empty track space starts a rubber band. Without a modifier a plain
            // click (no drag) deselects and moves the playhead, as before.
            self.gesture = Some(TimelineGesture::Marquee(MarqueeDrag {
                origin: (x + layout.scroll_x as f32, y),
                current: (x, y),
                base: if extend { current } else { BTreeSet::new() },
                moved: false,
            }));
        }
        cx.notify();
    }

    /// Start dragging the (already updated) selection from the clip under the
    /// pointer.
    fn begin_clip_drag(
        &mut self,
        layout: &TimelineLayout,
        hit: timeline::ClipHit,
        x: f32,
        collapse_to: Option<ClipId>,
    ) {
        let Some(clip) = self.find_clip(hit.clip).cloned() else {
            return;
        };
        let members = self.selected_ids();
        let group_start = selection::earliest_start(&self.project, &members).map_or(0, |t| t.0);
        let rows = selection::row_span(&self.project, &members).unwrap_or((0, 0));
        self.gesture = Some(TimelineGesture::Drag(ClipDrag {
            members,
            origin_track: hit.track,
            target_track: hit.track,
            origin_start: clip.range.start,
            group_start,
            rows,
            row_delta: 0,
            grab_offset: timeline::x_to_time(layout, x).0 - clip.range.start.0,
            current_start: clip.range.start,
            collapse_to,
        }));
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
        if y < layout.ruler_height || self.selected_ids().len() > 1 {
            return None;
        }
        let row = ((y - layout.ruler_height) / layout.row_height.max(1.0)) as usize;
        let key_y = layout.ruler_height + (row + 1) as f32 * layout.row_height - 13.0;
        if (y - key_y).abs() > 7.0 {
            return None;
        }
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
                let members: Vec<ClipId> = drag.members.iter().copied().collect();
                let raw = timeline::drag_start(&layout, drag.grab_offset, x);
                let snapped = if self.snapping {
                    timeline::snap_start_among(&layout, &self.project, &members, self.position, raw)
                } else {
                    raw
                };
                // The group stops together at time zero.
                let floor = drag.origin_start.0 - drag.group_start;
                drag.current_start = Time(snapped.0.max(floor));
                self.snap_line = if self.snapping {
                    timeline::snap_target_among(
                        &layout,
                        &self.project,
                        &members,
                        self.position,
                        raw,
                    )
                } else {
                    None
                };
                let rows = self.project.tracks().len();
                let origin_row = self
                    .project
                    .tracks()
                    .iter()
                    .position(|track| track.id() == drag.origin_track)
                    .unwrap_or(0);
                let max_row = rows.saturating_sub(1) as f32;
                let row = ((y - layout.ruler_height) / layout.row_height.max(1.0))
                    .clamp(0.0, max_row) as usize;
                drag.row_delta = selection::row_delta(origin_row, row, rows, drag.rows);
                if let Some(track) = selection::track_at(
                    &self.project,
                    selection::shifted_row(origin_row, drag.row_delta, rows),
                ) {
                    drag.target_track = track;
                }
                cx.notify();
            }
            Some(TimelineGesture::Marquee(band)) => {
                band.current = (x, y);
                let dx = band.current.0 + layout.scroll_x as f32 - band.origin.0;
                let dy = band.current.1 - band.origin.1;
                if !band.moved && dx.hypot(dy) > 3.0 {
                    band.moved = true;
                }
                if band.moved {
                    let origin = (band.origin.0 - layout.scroll_x as f32, band.origin.1);
                    let mut ids = selection::marquee(&layout, &self.project, origin, (x, y));
                    ids.extend(band.base.iter().copied());
                    let primary = self.selected;
                    self.set_selection(ids, primary);
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
            Some(TimelineGesture::Marker(drag)) => {
                let raw = timeline::x_to_time(&layout, x);
                let snapped = if self.snapping {
                    timeline::snap_marker(&layout, &self.project, drag.id, self.position, raw)
                } else {
                    None
                };
                drag.current = snapped.unwrap_or(raw);
                self.snap_line = snapped;
                cx.notify();
            }
            Some(TimelineGesture::Overview) | None => {}
        }
    }

    pub(super) fn on_timeline_mouse_up(
        &mut self,
        event: &MouseUpEvent,
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
                let delta = drag.current_start.0 - drag.origin_start.0;
                if delta == 0 && drag.row_delta == 0 {
                    if let Some(id) = drag.collapse_to {
                        self.select_only(id);
                    }
                    return;
                }
                if let Some(command) =
                    selection::move_command(&self.project, &drag.members, delta, drag.row_delta)
                {
                    self.execute(&command, cx);
                }
            }
            Some(TimelineGesture::Marquee(band)) => {
                if !band.moved && band.base.is_empty() {
                    // A plain click on empty space: deselect and park the playhead.
                    self.clear_selection();
                    let (x, _) = self.timeline_relative(event.position);
                    self.scrub_to(x, cx);
                }
            }
            Some(TimelineGesture::Trim(trim)) => {
                let command = if trim.ripple {
                    ripple::trim_command(
                        &self.project,
                        trim.clip_id,
                        trim.edge,
                        trim.current,
                        self.ripple_scope,
                    )
                } else {
                    editing::trim_command(
                        &self.project,
                        trim.clip_id,
                        trim.edge,
                        trim.current,
                        false,
                    )
                };
                if let Some(command) = command {
                    self.execute(&command, cx);
                }
            }

            Some(TimelineGesture::Marker(drag)) => {
                if drag.current == drag.origin {
                    // A plain click on a marker parks the playhead on it.
                    self.seek_to(drag.origin, cx);
                } else if self
                    .project
                    .markers()
                    .iter()
                    .any(|m| m.id != drag.id && m.time == drag.current)
                {
                    self.error = Some("A marker already exists there".into());
                } else {
                    self.execute(
                        &Command::MoveMarker {
                            id: drag.id,
                            time: drag.current,
                        },
                        cx,
                    );
                }
                cx.notify();
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
            self.select_only(selection.clip_id);
            self.selected_keyframe = Some(selection.clone());
            self.context_menu = Some(ContextMenu::Keyframe(selection));
        } else if let Some(hit) = timeline::hit_test(&layout, &self.project, x, y) {
            // Right-clicking a member keeps the whole selection for the menu.
            let current = self.selected_ids();
            if current.contains(&hit.clip) {
                self.set_selection(current, Some(hit.clip));
            } else {
                self.select_only(hit.clip);
            }
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
            let factor = (dy * 0.002).exp();
            let target = self
                .zoom_motion
                .as_ref()
                .map_or(self.timeline_zoom, |m| m.to.0);
            self.animate_timeline_zoom((target * factor).clamp(1.0, 64.0), x, cx);
        } else if event.modifiers.alt || dx != 0.0 {
            self.zoom_motion = None;
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

    /// Animate zoom and scroll toward `zoom`, keeping the time under `anchor_x` fixed.
    pub(super) fn animate_timeline_zoom(
        &mut self,
        zoom: f32,
        anchor_x: f32,
        cx: &mut Context<Self>,
    ) {
        let layout = self.current_layout();
        let (zoom, scroll) = timeline::zoom_at(&layout, zoom, anchor_x);
        self.zoom_motion = Some(ZoomMotion {
            started: Instant::now(),
            from: (self.timeline_zoom, self.timeline_scroll),
            to: (zoom, scroll),
        });
        cx.notify();
    }

    pub(super) fn advance_timeline_zoom(&mut self, window: &mut Window) {
        if let Some(motion) = &self.zoom_motion {
            let t = (motion.started.elapsed().as_secs_f32() * 1000.0
                / crate::style::motion::BASE_MS as f32)
                .min(1.0);
            let ease = 1.0 - (1.0 - t).powi(3);
            self.timeline_zoom = motion.from.0 + (motion.to.0 - motion.from.0) * ease;
            self.timeline_scroll = motion.from.1 + (motion.to.1 - motion.from.1) * ease;
            if t >= 1.0 {
                self.zoom_motion = None;
            } else {
                window.request_animation_frame();
            }
        }
    }

    fn scroll_overview(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let width = f32::from(self.timeline_bounds.size.width).max(1.0);
        let x = f32::from(position.x - self.timeline_bounds.origin.x);
        self.timeline_scroll =
            (x * self.timeline_zoom - width / 2.0).clamp(0.0, width * (self.timeline_zoom - 1.0));
        cx.notify();
    }

    fn finish_gain_drag(&mut self, cx: &mut Context<Self>) {
        if let Some(drag) = self.gain_drag.take()
            && (drag.current - drag.original).abs() > f64::EPSILON
        {
            self.execute(
                &Command::SetTrackGain {
                    id: drag.id,
                    gain: drag.current,
                },
                cx,
            );
        }
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

    /// The rubber band as a canvas-relative `(left, top, right, bottom)` rectangle.
    fn marquee_rect(&self) -> Option<(f32, f32, f32, f32)> {
        let Some(TimelineGesture::Marquee(band)) = &self.gesture else {
            return None;
        };
        if !band.moved {
            return None;
        }
        let scroll = self.timeline_scroll;
        let origin = (band.origin.0 - scroll, band.origin.1);
        Some((
            origin.0.min(band.current.0),
            origin.1.min(band.current.1),
            origin.0.max(band.current.0),
            origin.1.max(band.current.1),
        ))
    }

    pub(super) fn build_timeline_snapshot(&self, cx: &App) -> TimelineSnapshot {
        let duration = timeline::project_duration(&self.project);
        let mut clips = Vec::new();
        let mut overview = Vec::new();
        let mut transitions = Vec::new();
        let layout = self.current_layout();
        let selected = self.selected_ids();
        let single = selected.len() <= 1;
        let rows = self.project.tracks().len();
        let loaded_effects = self.loaded_effect_ids();
        let any_solo = self.project.tracks().iter().any(Track::is_solo);
        for (row, track) in self.project.tracks().iter().enumerate() {
            let silenced = track.is_muted() || (any_solo && !track.is_solo());
            for transition in track.transitions() {
                if let Some(from) = track.clips().get(&transition.from) {
                    transitions.push(TransitionPaint {
                        row,
                        range: transition.window(from.range.end),
                    });
                }
            }
            for clip in track.clips().values() {
                overview.push((row, clip.range, clip_color(&clip.kind, cx.theme())));
                let mut paint_row = row;
                let (start, end) = if let Some(TimelineGesture::Drag(drag)) = &self.gesture
                    && drag.members.contains(&clip.id)
                {
                    let delta = drag.current_start.0 - drag.origin_start.0;
                    paint_row = selection::shifted_row(row, drag.row_delta, rows);
                    (
                        Time(clip.range.start.0 + delta),
                        Time(clip.range.end.0 + delta),
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
                // Cull before allocating labels, animation markers or requesting media.
                if !timeline::visible_span(&layout, start, end) {
                    continue;
                }
                let mut keyframes = Vec::new();
                if single && self.selected == Some(clip.id) {
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
                let tile_width = ((layout.row_height - 29.) * (16. / 9.)).max(32.);
                let clip_x = timeline::time_to_x(&layout, start) + 5.;
                let thumbnail_first = ((-clip_x / tile_width).floor().max(0.)) as usize;
                let thumbnails = match &clip.kind {
                    ClipKind::Video(asset) | ClipKind::Image(asset)
                        if timeline::time_to_x(&layout, end)
                            - timeline::time_to_x(&layout, start)
                            > 40.0
                            && layout.row_height > 36.0 =>
                    {
                        let still = matches!(clip.kind, ClipKind::Image(_));
                        let height = (layout.row_height - 29.0).clamp(16.0, 96.0) as u32;
                        let count = (layout.width / tile_width).ceil() as usize + 2;
                        (thumbnail_first..thumbnail_first + count)
                            .take_while(|index| {
                                clip_x + *index as f32 * tile_width
                                    < timeline::time_to_x(&layout, end)
                            })
                            .map(|index| {
                                let tick = clip.source_offset.0
                                    + (index as f64 * f64::from(tile_width)
                                        / layout.pixels_per_tick)
                                        as i64;
                                self.thumbnails.borrow_mut().get(
                                    asset,
                                    Time(tick),
                                    height,
                                    still,
                                    &self.assets,
                                )
                            })
                            .collect()
                    }
                    _ => Vec::new(),
                };
                clips.push(ClipPaint {
                    thumbnail_first,
                    effect_warning: self.clip_effect_warning(clip, &loaded_effects),
                    row: paint_row,
                    start,
                    end,
                    selected: selected.contains(&clip.id),
                    hovered: self.hovered_clip == Some(clip.id),
                    label: format!(
                        "{}   {}",
                        clip_title(&self.project, clip, layout.row_height),
                        timecode::format_timecode(Time(end.0 - start.0), self.project.frame_rate())
                    )
                    .into(),
                    preview: timeline_content_preview(clip).into(),
                    thumbnails,
                    kind: clip.kind.clone(),
                    icon: icons::kind_icon(&clip.kind),
                    keyframes,
                    waveform,
                    source_offset: clip.source_offset.0,
                    fade_levels: [FadeEdge::In, FadeEdge::Out].map(|edge| {
                        let length = if edge == FadeEdge::In {
                            fade_in
                        } else {
                            fade_out
                        };
                        fades::levels(clip, edge, length)
                    }),
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
            overview,
            transitions,
            scrubbing: matches!(self.gesture, Some(TimelineGesture::Scrub)),
            waveform_paths: self.waveform_paths.clone(),
            position: self.position,
            frame_rate: self.project.frame_rate(),
            row_height: self.dock.track_height.pixels(),
            marks: self.marks,
            autocuts: [self
                .autocut_review
                .as_ref()
                .and_then(|r| r.proposal.as_ref())
                .map_or_default(|p| {
                    p.cuts
                        .iter()
                        .filter(|c| c.enabled)
                        .flat_map(|c| {
                            self.project
                                .tracks()
                                .iter()
                                .enumerate()
                                .filter(|(_, t)| p.tracks.contains(&t.id()))
                                .map(move |(row, _)| (c.range, row))
                        })
                        .collect::<Vec<_>>()
                })]
            .into_iter()
            .flatten()
            .chain(self.duck_overlay())
            .collect(),
            markers: self.marker_paints(),
            snap_line: self.snap_line,
            marquee: self.marquee_rect(),
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
        let ripple_all = self.ripple_scope == RippleScope::AllTracks;
        let overview = snapshot.overview.clone();
        let overview_rows = snapshot.rows;
        let duration = snapshot.duration;
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
                            this.animate_timeline_zoom(1.0, 0.0, cx);
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
                    .child(
                        icon_button(
                            "ripple-scope",
                            icons::icon(if ripple_all {
                                IconName::Link
                            } else {
                                IconName::Unlink
                            }),
                            self.ripple_scope.label(),
                            cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.toggle_ripple_scope(cx);
                            }),
                        )
                        .selected(ripple_all)
                        .toggled(ripple_all),
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
                            this.zoom_timeline(0.8, cx);
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
                            this.zoom_timeline(1.25, cx);
                        }),
                    )),
            )
            .child(
                div()
                    .h(px(22.))
                    .flex_shrink_0()
                    .flex()
                    .flex_row()
                    .bg(crate::style::hex(crate::style::color::BG))
                    .child(
                        div()
                            .w(px(HEADER_W))
                            .flex_shrink_0()
                            .px_2()
                            .text_xs()
                            .text_color(crate::style::hex(crate::style::color::TEXT_MUTED))
                            .child("OVERVIEW"),
                    )
                    .child(
                        div()
                            .id("timeline-overview")
                            .flex_1()
                            .h_full()
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                    this.zoom_motion = None;
                                    this.gesture = Some(TimelineGesture::Overview);
                                    this.scroll_overview(event.position, cx);
                                }),
                            )
                            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                                if event.dragging()
                                    && matches!(this.gesture, Some(TimelineGesture::Overview))
                                {
                                    this.scroll_overview(event.position, cx);
                                }
                            }))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    this.gesture = None;
                                    cx.notify();
                                }),
                            )
                            .on_mouse_up_out(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    if matches!(this.gesture, Some(TimelineGesture::Overview)) {
                                        this.gesture = None;
                                        cx.notify();
                                    }
                                }),
                            )
                            .child(
                                canvas(
                                    |_, _, _| {},
                                    move |bounds, (), window, _| {
                                        paint_overview(
                                            bounds,
                                            &overview,
                                            overview_rows,
                                            duration,
                                            zoom,
                                            scroll,
                                            window,
                                        );
                                    },
                                )
                                .size_full(),
                            ),
                    ),
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
                                                        let _ = weak.update(cx, |this, cx| {
                                                            if this.timeline_bounds != bounds {
                                                                this.timeline_bounds = bounds;
                                                                cx.notify();
                                                            }
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
            let gain = self
                .gain_drag
                .as_ref()
                .filter(|drag| drag.id == id)
                .map_or(track.gain(), |drag| drag.current);
            let kind = track
                .clips()
                .values()
                .max_by_key(|clip| clip.range.end.0 - clip.range.start.0)
                .map(|clip| &clip.kind);
            let color = kind.map_or(
                crate::style::hex(crate::style::color::CLIP_PLUGIN),
                |kind| clip_color(kind, cx.theme()),
            );
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
                        .id(("track-gain", id.0))
                        .w(px(62.))
                        .h(px(22.))
                        .flex_shrink_0()
                        .cursor_pointer()
                        .tooltip(move |window, cx| {
                            gpui_component::tooltip::Tooltip::new(format!(
                                "Gain {gain:.2}x. Drag to adjust; double-click to reset"
                            ))
                            .build(window, cx)
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                if event.click_count == 2 {
                                    this.gain_drag = None;
                                    this.nudge_track_gain(id, 1.0 - gain, cx);
                                } else {
                                    this.gain_drag = Some(GainDrag {
                                        id,
                                        x: f32::from(event.position.x),
                                        original: gain,
                                        current: gain,
                                    });
                                }
                            }),
                        )
                        .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                            if event.dragging()
                                && let Some(drag) = &mut this.gain_drag
                            {
                                drag.current = timeline::gain_from_drag(
                                    drag.original,
                                    f32::from(event.position.x) - drag.x,
                                );
                                cx.notify();
                            }
                        }))
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| this.finish_gain_drag(cx)),
                        )
                        .on_mouse_up_out(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| this.finish_gain_drag(cx)),
                        )
                        .child(
                            canvas(
                                |_, _, _| {},
                                move |bounds, (), window, _| {
                                    let y = bounds.center().y;
                                    let left = bounds.left() + px(4.);
                                    let right = bounds.right() - px(4.);
                                    window.paint_quad(
                                        fill(
                                            Bounds::from_corners(
                                                point(left, y - px(1.5)),
                                                point(right, y + px(1.5)),
                                            ),
                                            crate::style::hex(crate::style::color::BORDER),
                                        )
                                        .corner_radii(px(2.)),
                                    );
                                    // Unity-gain tick at the midpoint of the 0-2x range.
                                    window.paint_quad(fill(
                                        Bounds::new(
                                            point(
                                                f32::midpoint(f32::from(left), f32::from(right))
                                                    .into(),
                                                y - px(3.5),
                                            ),
                                            size(px(1.), px(7.)),
                                        ),
                                        crate::style::hex(crate::style::color::TEXT_MUTED),
                                    ));
                                    let x = left + (right - left) * (gain as f32 / 2.0);
                                    window.paint_quad(
                                        fill(
                                            Bounds::from_corners(
                                                point(left, y - px(1.5)),
                                                point(x, y + px(1.5)),
                                            ),
                                            color,
                                        )
                                        .corner_radii(px(2.)),
                                    );
                                    window.paint_quad(
                                        fill(
                                            Bounds::new(
                                                point(x - px(3.), y - px(4.)),
                                                size(px(6.), px(8.)),
                                            ),
                                            crate::style::hex(crate::style::color::TEXT),
                                        )
                                        .corner_radii(px(3.)),
                                    );
                                },
                            )
                            .size_full(),
                        ),
                )
                .child(div().flex_1())
                .child(header_pill(
                    ("track-mute", id.0),
                    if muted {
                        IconName::VolumeX
                    } else {
                        IconName::Volume2
                    },
                    if muted { "Unmute track" } else { "Mute track" },
                    muted,
                    crate::style::color::WARNING,
                    cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.execute(&Command::SetTrackMuted { id, muted: !muted }, cx);
                    }),
                ))
                .child(header_pill(
                    ("track-solo", id.0),
                    IconName::Headphones,
                    if solo { "Unsolo track" } else { "Solo track" },
                    solo,
                    crate::style::color::SUCCESS,
                    cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.execute(&Command::SetTrackSolo { id, solo: !solo }, cx);
                    }),
                ))
                .child(header_pill(
                    ("track-lock", id.0),
                    if locked {
                        IconName::Lock
                    } else {
                        IconName::LockOpen
                    },
                    if locked { "Unlock track" } else { "Lock track" },
                    locked,
                    crate::style::color::ACCENT,
                    cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.execute(
                            &Command::SetTrackLocked {
                                id,
                                locked: !locked,
                            },
                            cx,
                        );
                    }),
                ));
            let renaming = self
                .track_rename
                .as_ref()
                .filter(|rename| rename.id == id)
                .map(|rename| rename.input.clone());
            let drag = TrackDrag {
                id,
                name: name.clone().into(),
            };
            let row_index = self
                .project
                .tracks()
                .iter()
                .position(|track| track.id() == id)
                .unwrap_or(0);
            let name_label = if let Some(input) = renaming {
                div()
                    .id(("track-rename", id.0))
                    .w_full()
                    .h(px(22.))
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                        if event.keystroke.key == "escape" {
                            this.finish_track_rename(false, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .child(
                        gpui_component::input::Input::new(&input)
                            .xsmall()
                            .appearance(true),
                    )
            } else {
                div()
                    .id(("track-name", id.0))
                    .w_full()
                    .h(px(14.))
                    .truncate()
                    .text_xs()
                    .text_color(cx.theme().foreground)
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .child(
                        icons::icon(kind.map_or(IconName::Layers, icons::kind_icon))
                            .xsmall()
                            .text_color(color),
                    )
                    .child(name.clone())
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        if event.click_count() == 2 {
                            this.start_track_rename(id, window, cx);
                        }
                    }))
            };
            header = header.child(
                div()
                    // A per-track id scopes the buttons' hover/tooltip state.
                    .id(("track-header", id.0))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            this.context_menu = Some(ContextMenu::Track { id });
                            this.context_menu_pos = Some(event.position);
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .on_drag(drag, |drag, _offset, _window, cx| cx.new(|_| drag.clone()))
                    .drag_over::<TrackDrag>(|style, _, _, cx| {
                        style.border_t_2().border_color(cx.theme().ring)
                    })
                    .on_drop(cx.listener(move |this, drag: &TrackDrag, _, cx| {
                        this.move_track_to(drag.id, row_index, cx);
                    }))
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
                    .border_l(px(3.))
                    .border_color(color)
                    .bg(crate::style::hex(crate::style::color::PANEL))
                    .child(name_label)
                    .child(controls),
            );
        }
        header
    }
}
