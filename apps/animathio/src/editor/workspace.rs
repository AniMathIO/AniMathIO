//! Resizable workspace. Only view state is written to the layout sidecar.

use super::*;
use crate::style::{self, color};
use gpui_component::resizable::{h_resizable, resizable_panel, v_resizable};

impl Editor {
    pub(super) fn workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        // Include visibility in the ID: a reopened panel starts from its saved
        // size rather than inheriting the slot of a different hidden panel.
        let id = SharedString::from(format!(
            "workspace-{}-{}",
            self.dock.left_open, self.dock.right_open
        ));
        let mut middle = h_resizable(id).on_resize(cx.listener(
            |this, state: &Entity<gpui_component::resizable::ResizableState>, _, cx| {
                let sizes = state.read(cx).sizes();
                if this.dock.left_open
                    && let Some(size) = sizes.first()
                {
                    this.dock.left_width = f32::from(*size);
                }
                if this.dock.right_open
                    && let Some(size) = sizes.last()
                {
                    this.dock.right_width = f32::from(*size);
                }
                this.dock.clamp();
                this.dock.save();
            },
        ));
        if self.dock.left_open {
            middle = middle.child(
                resizable_panel()
                    .size(px(self.dock.left_width))
                    .size_range(px(160.)..px(640.))
                    .child(style::panel(
                        div()
                            .size_full()
                            .flex()
                            .flex_col()
                            .child(style::panel_header(
                                IconName::Library,
                                "Library",
                                Button::new("library-import")
                                    .ghost()
                                    .compact()
                                    .icon(icons::icon(IconName::Plus))
                                    .tooltip("Import media")
                                    .on_click(cx.listener(|this, _, _, cx| this.import_media(cx))),
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_h_0()
                                    .child(self.media_pool(window, cx).bg(style::hex(color::PANEL))),
                            ),
                    )),
            );
        }
        middle = middle.child(
            resizable_panel()
                .size_range(px(240.)..px(10000.))
                .child(style::panel(
                    div()
                        .size_full()
                        .flex()
                        .flex_col()
                        .child(style::panel_header(
                            IconName::MonitorPlay,
                            "Preview",
                            div()
                                .text_size(px(11.))
                                .text_color(style::hex(color::TEXT_SECONDARY))
                                .child("PROGRAM"),
                        ))
                        .child(div().flex_1().min_h_0().child(self.viewer(window, cx))),
                )),
        );
        if self.dock.right_open {
            middle = middle.child(
                resizable_panel()
                    .size(px(self.dock.right_width))
                    .size_range(px(200.)..px(720.))
                    .child(style::panel(
                        div()
                            .size_full()
                            .flex()
                            .flex_col()
                            .child(style::panel_header(
                                IconName::SlidersHorizontal,
                                "Inspector",
                                Button::new("inspector-hide")
                                    .ghost()
                                    .compact()
                                    .icon(icons::icon(IconName::PanelRightClose))
                                    .tooltip("Hide inspector")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.dock.right_open = false;
                                        this.dock.save();
                                        cx.notify();
                                    })),
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_h_0()
                                    .child(self.inspector(window, cx).border_l_0()),
                            ),
                    )),
            );
        }
        let workspace = v_resizable(if self.dock.bottom_open {
            "workspace-timeline"
        } else {
            "workspace-viewer"
        })
        .child(
            resizable_panel()
                .size_range(px(140.)..px(10000.))
                .child(middle),
        )
        .when(self.dock.bottom_open, |workspace| {
            workspace.child(
                resizable_panel()
                    .size(px(self.dock.bottom_height))
                    .size_range(px(120.)..px(600.))
                    .child(style::panel(
                        div()
                            .size_full()
                            .flex()
                            .flex_col()
                            .child(style::panel_header(
                                IconName::Film,
                                "Timeline",
                                div()
                                    .text_size(px(11.))
                                    .text_color(style::hex(color::TEXT_SECONDARY))
                                    .child(format!("{} tracks", self.project.tracks().len())),
                            ))
                            .child(div().flex_1().min_h_0().child(self.timeline_panel(cx))),
                    )),
            )
        })
        .on_resize(cx.listener(
            |this, state: &Entity<gpui_component::resizable::ResizableState>, _, cx| {
                if this.dock.bottom_open
                    && let Some(size) = state.read(cx).sizes().last()
                {
                    this.dock.bottom_height = f32::from(*size);
                    this.dock.clamp();
                    this.dock.save();
                }
            },
        ));
        div()
            .flex_1()
            .min_h_0()
            .p(px(style::space::GUTTER / 2.))
            .bg(style::hex(color::BG))
            .child(workspace)
    }
}

impl Editor {
    /// Sliding pill only schedules frames for the 180ms after a tab change.
    pub(super) fn library_tabs(&self, cx: &mut Context<Self>) -> Div {
        let index = |tab| match tab {
            LeftTab::Assets => 0.,
            LeftTab::Plugins => 1.,
            LeftTab::Effects => 2.,
        };
        let from = index(self.previous_left_tab);
        let to = index(self.left_tab);
        let elapsed = self.tab_changed.elapsed().as_secs_f32();
        let progress = if self.preferences.reduce_motion {
            1.
        } else {
            (elapsed / (style::motion::BASE_MS as f32 / 1000.)).min(1.)
        };
        let eased = 1. - (1. - progress).powi(3);
        let indicator = div()
            .absolute()
            .left(relative((from + (to - from) * eased) / 3.))
            .w(relative(1. / 3.))
            .h_full()
            .rounded(px(style::radius::CONTROL))
            .bg(style::hexa(color::ACCENT, 0.16))
            .border_1()
            .border_color(style::hexa(color::ACCENT, 0.28));
        div().p_2().child(
            div()
                .relative()
                .flex()
                .h(px(30.))
                .rounded(px(style::radius::CONTROL))
                .bg(style::hex(color::BG))
                .child(indicator)
                .children(
                    [
                        (LeftTab::Assets, "Assets", IconName::Images),
                        (LeftTab::Plugins, "Plugins", IconName::Puzzle),
                        (LeftTab::Effects, "Effects", IconName::Sparkles),
                    ]
                    .map(|(tab, label, icon)| {
                        let selected = self.left_tab == tab;
                        div()
                            .id(label)
                            .focusable()
                            .tab_index(0)
                            .focus_visible(|this| {
                                this.border_1().border_color(style::hex(color::ACCENT))
                            })
                            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    this.select_left_tab(tab, cx);
                                    cx.stop_propagation();
                                }
                            }))
                            .relative()
                            .flex_1()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap_1()
                            .rounded(px(style::radius::CONTROL))
                            .cursor_pointer()
                            .text_size(px(11.))
                            .text_color(style::hex(if selected {
                                color::TEXT
                            } else {
                                color::TEXT_SECONDARY
                            }))
                            .hover(|this| this.bg(style::hexa(color::HOVER, 0.7)))
                            .child(icons::icon(icon).small())
                            .child(label)
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.select_left_tab(tab, cx)),
                            )
                    }),
                )
                .child(
                    canvas(
                        |_, _, _| {},
                        move |_, (), window, _| {
                            if progress < 1. {
                                window.request_animation_frame();
                            }
                        },
                    )
                    .absolute()
                    .size_full(),
                ),
        )
    }
}
