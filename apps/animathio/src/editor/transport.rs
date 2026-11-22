//! Transport.

use super::*;
use crate::style::{self, color, hex, tokens::TextRole as _};
use gpui_component::tooltip::Tooltip;

/// The timecode input while editing and the monitor volume slider, in rems.
const TIMECODE_INPUT_WIDTH: Rems = Rems(11.733);
const VOLUME_SLIDER_WIDTH: Rems = Rems(5.6);

impl Editor {
    pub(super) fn toggle_play(&mut self, cx: &mut Context<Self>) {
        if self.playing {
            if let Err(error) = self.clock.pause() {
                self.error = Some(error);
            }
            self.playing = false;
            self.shuttle = Shuttle::default();
            self.position = self.clock.now();
            self.play_limit = Time(0);
        } else {
            let duration = animathio_export::duration(&self.project);
            if !self.loop_playback {
                self.position = editing::play_start(self.position, duration);
            }
            self.play_limit = editing::play_limit(self.position, duration);
            self.shuttle = Shuttle(1, false);
            let _ = self.clock.set_rate(Speed::NORMAL);
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
        if !self.playing {
            let start = self.clock.now();
            self.play_limit = editing::play_limit(start, animathio_export::duration(&self.project));
        }
        // J/L while K is held is a half-speed jog; otherwise repeat-doubling shuttle.
        if self.k_held {
            self.shuttle.jog(direction);
        } else {
            self.shuttle.press(direction);
        }
        if let Err(error) = self
            .clock
            .set_rate(self.shuttle.speed())
            .and_then(|()| self.clock.play())
        {
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
        self.play_limit = Time(0);
        self.shuttle = Shuttle::default();
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
        let end = animathio_export::duration(&self.project).0;
        self.seek_to(Time(end), cx);
    }

    pub(super) fn toggle_loop(&mut self, cx: &mut Context<Self>) {
        self.loop_playback = !self.loop_playback;
        cx.notify();
    }

    pub(super) fn toggle_snapping(&mut self, cx: &mut Context<Self>) {
        self.snapping = !self.snapping;
        self.snap_line = None;
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
        if self.left_tab != tab {
            self.previous_left_tab = self.left_tab;
            self.tab_changed = Instant::now();
        }
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

    /// A monitor-only snapshot: adjusting listening volume never changes export.
    pub(super) fn monitor_project(&self) -> Project {
        let mut project = self.project.clone();
        let commands = self
            .project
            .tracks()
            .iter()
            .map(|track| Command::SetTrackGain {
                id: track.id(),
                gain: track.gain() * self.viewer_state.volume,
            })
            .collect();
        let _ = Command::Batch { commands }.apply(&mut project);
        project
    }

    pub(super) fn set_monitor_volume(&mut self, value: f64, cx: &mut Context<Self>) {
        self.viewer_state.volume = value.clamp(0., 1.);
        self.clock.set_project(Arc::new(self.monitor_project()));
        cx.notify();
    }

    /// How much of the transport row fits: the full row, one without the volume
    /// slider, or the essentials only. Chosen from the viewer panel's width (the window
    /// minus the open docks and gutters), so the pieces never overlap at 1024 px.
    fn transport_tier(&self, window: &Window) -> TransportTier {
        let scale = style::ui_scale();
        let mut width = f32::from(window.viewport_size().width);
        for (open, dock_width) in [
            (self.dock.left_open, self.dock.left_width),
            (self.dock.right_open, self.dock.right_width),
        ] {
            if open {
                width -= dock_width * scale;
            }
        }
        width -= 4. * f32::from(style::gutter());
        TransportTier::for_rems(width / f32::from(window.rem_size()))
    }

    pub(super) fn transport_controls(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let tier = self.transport_tier(window);
        let value = timecode::format_timecode(self.position, self.project.frame_rate());
        let mono = cx.theme().mono_font_family.clone();
        let timecode_el = if self.viewer_state.timecode_editing {
            let state = self.input_state("timecode", InputTarget::Timecode, value, 0.0, window, cx);
            if std::mem::take(&mut self.viewer_state.focus_timecode) {
                let handle = state.read(cx).focus_handle(cx);
                window.focus(&handle, cx);
            }
            div()
                .w(TIMECODE_INPUT_WIDTH)
                .font_family(mono.clone())
                .child(gpui_component::input::Input::new(&state))
                .into_any_element()
        } else {
            div()
                .id("timecode-display")
                .debug_selector(|| "timecode-display".into())
                .h(style::size::ROW_MD)
                .px_2()
                .flex()
                .items_center()
                .flex_shrink_0()
                .rounded(px(style::radius::CONTROL))
                .cursor_text()
                .font_family(mono.clone())
                .text_size(style::type_size::TIMECODE)
                .text_color(hex(color::TEXT))
                .hover(|this| this.bg(hex(color::HOVER)))
                .tooltip(|window, cx| Tooltip::new("Click to type a timecode").build(window, cx))
                .child(value)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.viewer_state.timecode_editing = true;
                    this.viewer_state.focus_timecode = true;
                    cx.notify();
                }))
                .into_any_element()
        };
        let volume = tier.volume_slider().then(|| {
            self.slider_control("monitor-volume", None, self.viewer_state.volume, window, cx)
        });
        let duration = timecode::format_timecode(
            Time(timeline::project_duration(&self.project)),
            self.project.frame_rate(),
        );
        let speed = if self.playing {
            self.shuttle.label()
        } else {
            "1x".into()
        };
        let speed_active = self.playing && speed != "1x";
        let muted = self.viewer_state.volume == 0.;
        let compact = self.compact_height;
        let play_size = Rems(if compact { 2.0 } else { 2.5 });
        div()
            .h(Rems(if compact { 3.0 } else { 3.6 }))
            .w_full()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .bg(hex(color::PANEL))
            .border_t_1()
            .border_color(hex(color::BORDER))
            .child(
                // The side columns share what the buttons leave and clip rather than
                // overlap them.
                div()
                    .debug_selector(|| "transport-left".into())
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .child(timecode_el),
            )
            .child(
                div()
                    .debug_selector(|| "transport-center".into())
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap_1()
                    .when(tier.edit_jumps(), |row| {
                        row.child(icon_button(
                            "prev-edit",
                            icons::icon(IconName::SkipBack),
                            "Previous edit (Up)",
                            cx.listener(|this, _, _, cx| this.step_edit(-1, cx)),
                        ))
                    })
                    .child(icon_button(
                        "step-back",
                        icons::icon(IconName::StepBack),
                        "Previous frame (Left)",
                        cx.listener(|this, _, _, cx| this.step_frame(-1, cx)),
                    ))
                    .child(
                        div().px_1().child(
                            style::primary_button("play", cx)
                                .rounded_full()
                                .w(play_size)
                                .h(play_size)
                                .icon(
                                    icons::icon(if self.playing {
                                        IconName::Pause
                                    } else {
                                        IconName::Play
                                    })
                                    .size(px(18.)),
                                )
                                .tooltip(if self.playing {
                                    "Pause (Space)"
                                } else {
                                    "Play (Space)"
                                })
                                .on_click(cx.listener(Self::on_play_click)),
                        ),
                    )
                    .child(icon_button(
                        "step-fwd",
                        icons::icon(IconName::StepForward),
                        "Next frame (Right)",
                        cx.listener(|this, _, _, cx| this.step_frame(1, cx)),
                    ))
                    .when(tier.edit_jumps(), |row| {
                        row.child(icon_button(
                            "next-edit",
                            icons::icon(IconName::SkipForward),
                            "Next edit (Down)",
                            cx.listener(|this, _, _, cx| this.step_edit(1, cx)),
                        ))
                    })
                    .child(
                        icon_button(
                            "loop",
                            icons::icon(IconName::Repeat),
                            "Loop playback",
                            cx.listener(|this, _, _, cx| this.toggle_loop(cx)),
                        )
                        .selected(self.loop_playback),
                    )
                    .when(tier.edit_jumps(), |row| {
                        row.child(
                            style::chip("shuttle-speed", None, speed)
                                .tabular()
                                .when(speed_active, |chip| {
                                    chip.bg(style::tint(color::ACCENT, style::Alpha::Subtle))
                                        .text_color(hex(color::ACCENT))
                                })
                                .tooltip(|window, cx| {
                                    Tooltip::new("Shuttle speed (J / K / L)").build(window, cx)
                                }),
                        )
                    }),
            )
            .child(
                div()
                    .debug_selector(|| "transport-right".into())
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap_2()
                    .when(tier.duration(), |row| {
                        row.child(
                            div()
                                .debug_selector(|| "transport-duration".into())
                                .flex_shrink_0()
                                .font_family(mono)
                                .text_role(style::Role::Label)
                                .tabular()
                                .child(duration),
                        )
                    })
                    .child(
                        icon_button(
                            "monitor-mute",
                            icons::icon(if muted {
                                IconName::VolumeX
                            } else {
                                IconName::Volume2
                            }),
                            if muted {
                                "Unmute monitor"
                            } else {
                                "Mute monitor"
                            },
                            cx.listener(move |this, _, _, cx| {
                                this.set_monitor_volume(if muted { 1. } else { 0. }, cx);
                            }),
                        )
                        .selected(muted),
                    )
                    .children(volume.map(|slider| div().w(VOLUME_SLIDER_WIDTH).child(slider))),
            )
    }
}

/// What the transport row shows at a given viewer width (see
/// [`Editor::transport_tier`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransportTier {
    /// Everything: edit jumps, shuttle chip, duration and the volume slider.
    Full,
    /// The volume slider is dropped (the mute button stays).
    NoVolumeSlider,
    /// Step, play, loop, the timecode and mute only.
    Essential,
}

impl TransportTier {
    /// Viewer widths in rems at which the row loses a part. They leave the sum of the
    /// widest parts (timecode, buttons, duration, mute and slider) and the gaps free.
    const FULL_MIN: f32 = 46.;
    const NO_SLIDER_MIN: f32 = 40.;

    fn for_rems(width: f32) -> Self {
        if width >= Self::FULL_MIN {
            Self::Full
        } else if width >= Self::NO_SLIDER_MIN {
            Self::NoVolumeSlider
        } else {
            Self::Essential
        }
    }

    fn volume_slider(self) -> bool {
        self == Self::Full
    }

    fn duration(self) -> bool {
        self != Self::Essential
    }

    fn edit_jumps(self) -> bool {
        self != Self::Essential
    }
}
