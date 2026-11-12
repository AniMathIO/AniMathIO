//! Transport.

use super::*;

impl Editor {
    pub(super) fn toggle_play(&mut self, cx: &mut Context<Self>) {
        if self.playing {
            if let Err(error) = self.clock.pause() {
                self.error = Some(error);
            }
            self.playing = false;
            self.shuttle.0 = 0;
            self.position = self.clock.now();
        } else {
            self.shuttle.0 = 1;
            let _ = self.clock.set_rate(1);
            if let Err(error) = self.clock.seek(self.position) {
                self.error = Some(error);
            }
            if let Err(error) = self.clock.play() {
                self.error = Some(error);
            }
            self.playing = true;
        }
        cx.notify();
    }

    pub(super) fn shuttle(&mut self, direction: i32, cx: &mut Context<Self>) {
        if direction == 0 {
            self.seek_to(self.clock.now(), cx);
            return;
        }
        let rate = self.shuttle.press(direction);
        if let Err(error) = self.clock.set_rate(rate).and_then(|()| self.clock.play()) {
            self.error = Some(error);
            return;
        }
        self.playing = true;
        cx.notify();
    }

    pub(super) fn step_edit(&mut self, direction: i32, cx: &mut Context<Self>) {
        let edges = self
            .project
            .tracks()
            .iter()
            .flat_map(|t| t.clips().values())
            .flat_map(|c| [c.range.start, c.range.end]);
        let time = if direction < 0 {
            edges.filter(|t| *t < self.position).max()
        } else {
            edges.filter(|t| *t > self.position).min()
        };
        if let Some(time) = time {
            self.seek_to(time, cx);
        }
    }

    pub(super) fn seek_to(&mut self, time: Time, cx: &mut Context<Self>) {
        self.playing = false;
        self.shuttle.0 = 0;
        if let Err(error) = self.clock.pause() {
            self.error = Some(error);
        }
        if let Err(error) = self.clock.seek(time) {
            self.error = Some(error);
        }
        self.position = time;
        self.request_render(time);
        cx.notify();
    }

    pub(super) fn scrub_to(&mut self, x: f32, cx: &mut Context<Self>) {
        let layout = self.current_layout();
        let time = timeline::x_to_time(&layout, x);
        self.seek_to(time, cx);
    }

    /// Step the playhead by a whole number of frames.
    pub(super) fn step_frame(&mut self, delta: i64, cx: &mut Context<Self>) {
        let rate = self.project.frame_rate();
        let Some(frame) = rate
            .time_to_frame(self.position, Rounding::Nearest)
            .ok()
            .and_then(|frame| frame.checked_add(delta))
            .map(|frame| frame.max(0))
        else {
            return;
        };
        let Some(time) = rate.frame_to_time(frame, Rounding::Nearest).ok() else {
            return;
        };
        self.seek_to(time, cx);
    }

    pub(super) fn jump_to_start(&mut self, cx: &mut Context<Self>) {
        self.seek_to(Time(0), cx);
    }

    pub(super) fn jump_to_end(&mut self, cx: &mut Context<Self>) {
        let end = timeline::project_duration(&self.project);
        self.seek_to(Time(end), cx);
    }

    pub(super) fn toggle_loop(&mut self, cx: &mut Context<Self>) {
        self.loop_playback = !self.loop_playback;
        cx.notify();
    }

    pub(super) fn toggle_snapping(&mut self, cx: &mut Context<Self>) {
        self.snapping = !self.snapping;
        cx.notify();
    }

    pub(super) fn toggle_panel(&mut self, side: PanelSide, cx: &mut Context<Self>) {
        match side {
            PanelSide::Left => self.dock.left_open = !self.dock.left_open,
            PanelSide::Right => self.dock.right_open = !self.dock.right_open,
            PanelSide::Bottom => self.dock.bottom_open = !self.dock.bottom_open,
        }
        self.dock.save();
        cx.notify();
    }

    pub(super) fn toggle_gpu_preview(&mut self, cx: &mut Context<Self>) {
        self.preview.gpu = !self.preview.gpu;
        self.preview.save();
        self.worker.set_backend(self.preview.backend());
        if let Some(time) = self.last_requested {
            self.request_render(time);
        }
        cx.notify();
    }

    pub(super) fn select_left_tab(&mut self, tab: LeftTab, cx: &mut Context<Self>) {
        self.left_tab = tab;
        cx.notify();
    }

    pub(super) fn toggle_viewer_menu(&mut self, cx: &mut Context<Self>) {
        self.viewer_menu_open = !self.viewer_menu_open;
        cx.notify();
    }

    pub(super) fn select_viewer_fit(&mut self, fit: ViewerFit, cx: &mut Context<Self>) {
        self.viewer_fit = fit;
        self.viewer_menu_open = false;
        cx.notify();
    }

    pub(super) fn toggle_safe_areas(&mut self, cx: &mut Context<Self>) {
        self.safe_areas = !self.safe_areas;
        cx.notify();
    }

    pub(super) fn toggle_section(&mut self, name: &str, cx: &mut Context<Self>) {
        if !self.closed_sections.remove(name) {
            self.closed_sections.insert(name.to_owned());
        }
        cx.notify();
    }

    pub(super) fn nudge_track_gain(&mut self, track: TrackId, delta: f64, cx: &mut Context<Self>) {
        let Some(current) = self
            .project
            .tracks()
            .iter()
            .find(|t| t.id() == track)
            .map(Track::gain)
        else {
            return;
        };
        let gain = (current + delta).clamp(0.0, 2.0);
        self.execute(&Command::SetTrackGain { id: track, gain }, cx);
    }

    /// Place an already-registered asset onto the first track at the playhead.
    pub(super) fn on_play_click(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_play(cx);
    }

    pub(super) fn transport_controls(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let play_icon = if self.playing {
            icons::icon(IconName::Pause)
        } else {
            icons::icon(IconName::Play)
        };
        let play_tooltip = if self.playing { "Pause" } else { "Play" };
        let value = timecode::format_timecode(self.position, self.project.frame_rate());
        let state = self.input_state("timecode", InputTarget::Timecode, value, 0.0, window, cx);
        div()
            .h(px(40.))
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .px_1()
            .bg(cx.theme().background)
            .child(
                div()
                    .w(px(124.))
                    .font_family(cx.theme().mono_font_family.clone())
                    .child(gpui_component::input::Input::new(&state).small()),
            )
            .child(div().flex_1())
            .child(icon_button(
                "jump-start",
                icons::icon(IconName::SkipBack),
                "Go to start",
                cx.listener(|this, _: &ClickEvent, _, cx| this.jump_to_start(cx)),
            ))
            .child(icon_button(
                "step-back",
                icons::icon(IconName::StepBack),
                "Step back one frame",
                cx.listener(|this, _: &ClickEvent, _, cx| this.step_frame(-1, cx)),
            ))
            .child(
                icon_button(
                    "play",
                    play_icon,
                    play_tooltip,
                    cx.listener(Self::on_play_click),
                )
                .when(self.playing, |button| button.primary()),
            )
            .child(icon_button(
                "step-fwd",
                icons::icon(IconName::StepForward),
                "Step forward one frame",
                cx.listener(|this, _: &ClickEvent, _, cx| this.step_frame(1, cx)),
            ))
            .child(icon_button(
                "jump-end",
                icons::icon(IconName::SkipForward),
                "Go to end",
                cx.listener(|this, _: &ClickEvent, _, cx| this.jump_to_end(cx)),
            ))
            .child(
                icon_button(
                    "loop",
                    icons::icon(IconName::Repeat),
                    if self.loop_playback {
                        "Loop: on"
                    } else {
                        "Loop: off"
                    },
                    cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_loop(cx)),
                )
                .selected(self.loop_playback)
                .toggled(self.loop_playback),
            )
            .child(div().flex_1())
            .child(div().text_color(cx.theme().muted_foreground).child(
                if self.playing && self.shuttle.0 != 1 {
                    format!("{}x (silent)", self.shuttle.0)
                } else {
                    timecode::format_timecode(
                        Time(timeline::project_duration(&self.project)),
                        self.project.frame_rate(),
                    )
                },
            ))
    }
}
