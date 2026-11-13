//! Transport.

use super::*;
use crate::style::{self, color, hex, hexa};
use gpui_component::tooltip::Tooltip;

impl Editor {
    pub(super) fn toggle_play(&mut self, cx: &mut Context<Self>) {
        if self.playing {
            if let Err(error) = self.clock.pause() {
                self.error = Some(error);
            }
            self.playing = false;
            self.shuttle = Shuttle::default();
            self.position = self.clock.now();
        } else {
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

    pub(super) fn transport_controls(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let value = timecode::format_timecode(self.position, self.project.frame_rate());
        let mono = cx.theme().mono_font_family.clone();
        let timecode_el = if self.viewer_state.timecode_editing {
            let state = self.input_state("timecode", InputTarget::Timecode, value, 0.0, window, cx);
            if std::mem::take(&mut self.viewer_state.focus_timecode) {
                let handle = state.read(cx).focus_handle(cx);
                window.focus(&handle, cx);
            }
            div()
                .w(px(176.))
                .font_family(mono.clone())
                .child(gpui_component::input::Input::new(&state))
                .into_any_element()
        } else {
            div()
                .id("timecode-display")
                .h(px(36.))
                .px_2()
                .flex()
                .items_center()
                .rounded(px(style::radius::CONTROL))
                .cursor_text()
                .font_family(mono.clone())
                .text_size(px(22.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(hex(color::TEXT))
                .hover(|this| this.bg(hexa(color::HOVER, 0.7)))
                .tooltip(|window, cx| Tooltip::new("Click to type a timecode").build(window, cx))
                .child(value)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.viewer_state.timecode_editing = true;
                    this.viewer_state.focus_timecode = true;
                    cx.notify();
                }))
                .into_any_element()
        };
        let volume =
            self.slider_control("monitor-volume", None, self.viewer_state.volume, window, cx);
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
        div()
            .h(px(60.))
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
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .child(timecode_el),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap_1()
                    .child(icon_button(
                        "prev-edit",
                        icons::icon(IconName::SkipBack),
                        "Previous edit (Up)",
                        cx.listener(|this, _, _, cx| this.step_edit(-1, cx)),
                    ))
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
                                .w(px(42.))
                                .h(px(42.))
                                .shadow_md()
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
                    .child(icon_button(
                        "next-edit",
                        icons::icon(IconName::SkipForward),
                        "Next edit (Down)",
                        cx.listener(|this, _, _, cx| this.step_edit(1, cx)),
                    ))
                    .child(
                        icon_button(
                            "loop",
                            icons::icon(IconName::Repeat),
                            "Loop playback",
                            cx.listener(|this, _, _, cx| this.toggle_loop(cx)),
                        )
                        .selected(self.loop_playback),
                    )
                    .child(
                        div()
                            .id("shuttle-speed")
                            .min_w(px(34.))
                            .h(px(22.))
                            .px_2()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(if speed_active {
                                hexa(color::ACCENT, 0.18)
                            } else {
                                hex(color::RAISED)
                            })
                            .text_size(px(11.))
                            .font_family(mono.clone())
                            .text_color(if speed_active {
                                hex(color::ACCENT)
                            } else {
                                hex(color::TEXT_SECONDARY)
                            })
                            .tooltip(|window, cx| {
                                Tooltip::new("Shuttle speed (J / K / L)").build(window, cx)
                            })
                            .child(speed),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap_2()
                    .child(
                        div()
                            .font_family(mono)
                            .text_size(px(12.))
                            .text_color(hex(color::TEXT_MUTED))
                            .child(duration),
                    )
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
                    .child(div().w(px(84.)).child(volume)),
            )
    }
}
