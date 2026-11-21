//! Resizable workspace. Only view state is written to the layout sidecar.

use super::*;
use crate::style::{self, color};
use gpui_component::Disableable as _;
use gpui_component::resizable::{h_resizable, resizable_panel, v_resizable};

impl Editor {
    pub(super) fn workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        // Include visibility in the ID: a reopened panel starts from its saved
        // size rather than inheriting the slot of a different hidden panel.
        let id = SharedString::from(format!(
            "workspace-{}-{}-{}",
            self.dock.left_open,
            self.dock.right_open,
            style::ui_scale()
        ));
        let mut middle = h_resizable(id).on_resize(cx.listener(
            |this, state: &Entity<gpui_component::resizable::ResizableState>, _, cx| {
                let sizes = state.read(cx).sizes();
                if this.dock.left_open
                    && let Some(size) = sizes.first()
                {
                    this.dock.left_width = f32::from(*size) / style::ui_scale();
                }
                if this.dock.right_open
                    && let Some(size) = sizes.last()
                {
                    this.dock.right_width = f32::from(*size) / style::ui_scale();
                }
                this.dock.clamp();
                this.dock.save();
            },
        ));
        if self.dock.left_open {
            middle =
                middle.child(
                    resizable_panel()
                        .size(px(self.dock.left_width * style::ui_scale()))
                        .size_range(px(160. * style::ui_scale())..px(640. * style::ui_scale()))
                        .child(style::panel(
                            div()
                                .size_full()
                                .flex()
                                .flex_col()
                                .child(style::panel_header(
                                    IconName::Library,
                                    "Library",
                                    // The + follows the tab: Plugins installs a .amplugin file,
                                    // everything else imports media.
                                    if self.left_tab == LeftTab::Plugins {
                                        Button::new("library-install-plugin")
                                            .debug_selector(|| "library-install-plugin".into())
                                            .ghost()
                                            .compact()
                                            .icon(icons::icon(IconName::Plus))
                                            .tooltip("Install plugin from file...")
                                            .disabled(self.community.busy)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.install_community_file(cx);
                                            }))
                                    } else {
                                        Button::new("library-import")
                                            .debug_selector(|| "library-import".into())
                                            .ghost()
                                            .compact()
                                            .icon(icons::icon(IconName::Plus))
                                            .tooltip("Import media")
                                            .on_click(
                                                cx.listener(|this, _, _, cx| this.import_media(cx)),
                                            )
                                    },
                                ))
                                .child(div().flex_1().min_h_0().child(
                                    self.media_pool(window, cx).bg(style::hex(color::PANEL)),
                                )),
                        )),
                );
        }
        middle = middle.child(
            resizable_panel()
                .size_range(px(240. * style::ui_scale())..px(10000. * style::ui_scale()))
                .child(style::panel(
                    div()
                        .size_full()
                        .flex()
                        .flex_col()
                        .child(style::panel_header(
                            IconName::MonitorPlay,
                            "Preview",
                            div()
                                .text_size(crate::style::type_size::XS)
                                .text_color(style::hex(color::TEXT_SECONDARY))
                                .child("PROGRAM"),
                        ))
                        .child(div().flex_1().min_h_0().child(self.viewer(window, cx))),
                )),
        );
        if self.dock.right_open {
            middle = middle.child(
                resizable_panel()
                    .size(px(self.dock.right_width * style::ui_scale()))
                    .size_range(px(200. * style::ui_scale())..px(720. * style::ui_scale()))
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
                .size_range(px(140. * style::ui_scale())..px(10000. * style::ui_scale()))
                .child(middle),
        )
        .when(self.dock.bottom_open, |workspace| {
            workspace.child(
                resizable_panel()
                    .size(px(self.dock.bottom_height * style::ui_scale()))
                    .size_range(px(120. * style::ui_scale())..px(600. * style::ui_scale()))
                    .child(style::panel(
                        div()
                            .size_full()
                            .flex()
                            .flex_col()
                            .child(style::panel_header(
                                IconName::Film,
                                "Timeline",
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .text_size(crate::style::type_size::XS)
                                    .text_color(style::hex(color::TEXT_SECONDARY))
                                    .child(count_label(self.project.tracks().len(), "track"))
                                    .child(
                                        Button::new("timeline-add-track")
                                            .debug_selector(|| "timeline-add-track".into())
                                            .ghost()
                                            .compact()
                                            .icon(icons::icon(IconName::Plus))
                                            .label("Add track")
                                            .tooltip("Add an empty foreground track")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.add_timeline_track(cx);
                                                window.focus(&this.panel_focus[0], cx);
                                            })),
                                    ),
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
                    this.dock.bottom_height = f32::from(*size) / style::ui_scale();
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
            .flex()
            .flex_col()
            .when(!self.missing_plugin_ids().is_empty(), |body| {
                body.child(self.missing_plugins_notice(cx))
            })
            .child(div().flex_1().min_h_0().child(workspace))
    }
}

/// Dock width (logical px) from which the Library tabs show their labels next to the icons.
pub(super) const LIBRARY_TABS_LABEL_WIDTH: f32 = 400.;

impl Editor {
    /// Sliding pill only schedules frames for the 180ms after a tab change.
    pub(super) fn library_tabs(&self, cx: &mut Context<Self>) -> Div {
        let index = |tab| match tab {
            LeftTab::Media => 0.,
            LeftTab::Plugins => 4.,
            LeftTab::Effects => 3.,
            LeftTab::Titles => 2.,
            LeftTab::Elements => 1.,
        };
        // Five labels only fit side by side in a wide dock; below that the tabs are icon-only
        // (each has a tooltip), which covers the default dock width.
        let labelled = self.dock.left_width >= LIBRARY_TABS_LABEL_WIDTH;
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
            .left(relative((from + (to - from) * eased) / 5.))
            .w(relative(1. / 5.))
            .h_full()
            .rounded(px(style::radius::CONTROL))
            .bg(style::hexa(color::ACCENT, 0.16))
            .border_1()
            .border_color(style::hexa(color::ACCENT, 0.28));
        div().p_2().child(
            div()
                .relative()
                .flex()
                .h(gpui::rems(2.0))
                .rounded(px(style::radius::CONTROL))
                .bg(style::hex(color::BG))
                .child(indicator)
                .children(
                    [
                        (LeftTab::Media, "Media", IconName::Images),
                        (LeftTab::Elements, "Elements", IconName::Shapes),
                        (LeftTab::Titles, "Titles", IconName::Type),
                        (LeftTab::Effects, "Effects", IconName::Sparkles),
                        (LeftTab::Plugins, "Plugins", IconName::Puzzle),
                    ]
                    .map(|(tab, label, icon)| {
                        let selected = self.left_tab == tab;
                        div()
                            .id(label)
                            .debug_selector(move || format!("library-tab-{}", label.to_lowercase()))
                            .track_focus(&self.media_view.tab_focus[tab as usize])
                            .tab_index(0)
                            .focus_visible(|this| {
                                this.border_1().border_color(style::hex(color::ACCENT))
                            })
                            .on_key_down(cx.listener(
                                move |this, event: &KeyDownEvent, window, cx| {
                                    // Arrows move from the focused tab (Tab can focus one that is
                                    // not selected), as in a WAI-ARIA tab list.
                                    let index = tab as usize;
                                    let next = match event.keystroke.key.as_str() {
                                        "enter" | "space" => Some(tab),
                                        "right" => Some(LeftTab::ALL[(index + 1) % 5]),
                                        "left" => Some(LeftTab::ALL[(index + 4) % 5]),
                                        "home" => Some(LeftTab::Media),
                                        "end" => Some(LeftTab::Plugins),
                                        _ => None,
                                    };
                                    if let Some(next) = next {
                                        this.select_left_tab(next, cx);
                                        window.focus(&this.media_view.tab_focus[next as usize], cx);
                                        cx.stop_propagation();
                                    }
                                },
                            ))
                            .relative()
                            .flex_1()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap_1()
                            .rounded(px(style::radius::CONTROL))
                            .cursor_pointer()
                            .text_size(crate::style::type_size::XS)
                            .text_color(style::hex(if selected {
                                color::TEXT
                            } else {
                                color::TEXT_SECONDARY
                            }))
                            .hover(|this| this.bg(style::hexa(color::HOVER, 0.7)))
                            .child(icons::icon(icon).small())
                            .when(labelled, |tab| tab.child(label))
                            .tooltip(move |window, cx| {
                                gpui_component::tooltip::Tooltip::new(label).build(window, cx)
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.select_left_tab(tab, cx);
                                window.focus(&this.media_view.tab_focus[tab as usize], cx);
                            }))
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
