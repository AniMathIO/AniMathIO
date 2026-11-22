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
                                .child(style::panel_header_full(
                                    IconName::Library,
                                    "Library",
                                    Some(self.left_tab.label().into()),
                                    // The + follows the tab: Plugins installs a .amplugin file,
                                    // everything else imports media.
                                    if self.left_tab == LeftTab::Plugins {
                                        Button::new("library-install-plugin")
                                            .debug_selector(|| "library-install-plugin".into())
                                            .ghost()
                                            .small()
                                            .icon(icons::icon(IconName::Plus))
                                            .tooltip("Install plugin from file…")
                                            .disabled(self.community.busy)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.install_community_file(cx);
                                            }))
                                    } else {
                                        Button::new("library-import")
                                            .debug_selector(|| "library-import".into())
                                            .ghost()
                                            .small()
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
                        .child(style::panel_header(IconName::MonitorPlay, "Preview", div()))
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
                                    .small()
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
                            .child(style::panel_header_full(
                                IconName::Film,
                                "Timeline",
                                Some(count_label(self.project.tracks().len(), "track").into()),
                                Button::new("timeline-add-track")
                                    .debug_selector(|| "timeline-add-track".into())
                                    .ghost()
                                    .small()
                                    .icon(icons::icon(IconName::Plus))
                                    .label("Add track")
                                    .tooltip("Add an empty foreground track")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.add_timeline_track(cx);
                                        window.focus(&this.panel_focus[0], cx);
                                    })),
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
            .p(style::gutter() / 2.)
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

/// The Library tab strip: one option per tab (see `style::segmented`).
static LIBRARY_TABS: [style::SegmentedOption; 5] = [
    style::SegmentedOption::new("library-tab-media", "Media", Some(IconName::Images)),
    style::SegmentedOption::new("library-tab-elements", "Elements", Some(IconName::Shapes)),
    style::SegmentedOption::new("library-tab-titles", "Titles", Some(IconName::Type)),
    style::SegmentedOption::new("library-tab-effects", "Effects", Some(IconName::Sparkles)),
    style::SegmentedOption::new("library-tab-plugins", "Plugins", Some(IconName::Puzzle)),
];

impl Editor {
    /// The Library tab strip. The sliding pill only schedules frames for the
    /// 180ms after a tab change.
    pub(super) fn library_tabs(&self, cx: &mut Context<Self>) -> Div {
        // Five labels only fit side by side in a wide dock; below that the tabs are icon-only
        // (each has a tooltip), which covers the default dock width.
        let labelled = self.dock.left_width >= LIBRARY_TABS_LABEL_WIDTH;
        let slide = Some((
            self.previous_left_tab as usize,
            self.tab_changed,
            self.preferences.reduce_motion,
        ));
        let changed = self.tab_changed;
        let reduce_motion = self.preferences.reduce_motion;
        let editor = cx.entity();
        div()
            .relative()
            .child(style::segmented(
                "library-tabs",
                &LIBRARY_TABS,
                &self.media_view.tab_focus,
                self.left_tab as usize,
                slide,
                labelled,
                move |index, window, cx| {
                    editor.update(cx, |this, cx| {
                        this.select_left_tab(LeftTab::ALL[index], cx);
                        if let Some(handle) = this.media_view.tab_focus.get(index) {
                            window.focus(handle, cx);
                        }
                    });
                },
            ))
            .child(
                canvas(
                    |_, _, _| {},
                    move |_, (), window, _| {
                        // Keep painting while the pill slides to the new tab.
                        if !reduce_motion
                            && changed.elapsed().as_secs_f32()
                                < style::motion::BASE_MS as f32 / 1000.
                        {
                            window.request_animation_frame();
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
    }
}
