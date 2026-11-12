//! Viewer.

use super::*;

impl Editor {
    pub(super) fn viewer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let image = self.image.clone();
        let fit = match self.viewer_fit {
            ViewerFit::Contain => ObjectFit::Contain,
            ViewerFit::Fill => ObjectFit::Fill,
            ViewerFit::Actual => ObjectFit::None,
        };
        let safe_areas = self.safe_areas;
        let fit_label = match self.viewer_fit {
            ViewerFit::Contain => "Fit",
            ViewerFit::Fill => "Fill",
            ViewerFit::Actual => "Actual size",
        };
        let viewport = div()
            .flex_1()
            .w_full()
            .flex()
            .items_center()
            .justify_center()
            .relative()
            .bg(cx.theme().background)
            .child(canvas(
                move |_bounds, _window, _cx| {},
                move |bounds, (), window, cx| paint_checkerboard(bounds, window, cx),
            ));
        let viewport = if let Some(image) = image {
            viewport
                .child(img(image).w_full().h_full().object_fit(fit))
                .child(canvas(
                    move |_bounds, _window, _cx| {},
                    move |bounds, (), window, cx| {
                        if safe_areas {
                            paint_safe_areas(bounds, window, cx);
                        }
                    },
                ))
        } else {
            viewport.child(empty_state("rendering…", cx))
        };
        let viewer_root = div()
            .w_full()
            .min_w_0()
            .key_context("Viewer")
            .track_focus(&self.panel_focus[1])
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    if !this.fields.any_focused(window, cx) {
                        window.focus(&this.panel_focus[1], cx);
                    }
                }),
            )
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .relative();
        let viewer_root = viewer_root
            .child(
                div()
                    .h(px(30.))
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .px_1()
                    .bg(cx.theme().background)
                    .child(
                        Button::new("viewer-fit")
                            .ghost()
                            .compact()
                            .label(fit_label)
                            .dropdown_caret(true)
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.toggle_viewer_menu(cx);
                            })),
                    )
                    .child(
                        icon_button(
                            "safe-areas",
                            icons::icon(IconName::SquareDashed),
                            "Toggle safe areas",
                            cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.toggle_safe_areas(cx);
                            }),
                        )
                        .selected(safe_areas)
                        .toggled(safe_areas),
                    ),
            )
            .child(viewport)
            .child(self.transport_controls(window, cx));
        if self.viewer_menu_open {
            viewer_root.child(self.viewer_menu(cx))
        } else {
            viewer_root
        }
    }

    /// Right-click context menu over the timeline, or `None` when closed.
    pub(super) fn viewer_menu(&self, cx: &mut Context<Self>) -> Div {
        let mut menu = div()
            .absolute()
            .top(px(30.))
            .left(px(4.))
            .w(px(140.))
            .bg(cx.theme().popover)
            .border_1()
            .border_color(cx.theme().border)
            .p_1()
            .flex()
            .flex_col()
            .shadow_lg();
        for (fit, label) in [
            (ViewerFit::Contain, "Fit"),
            (ViewerFit::Fill, "Fill"),
            (ViewerFit::Actual, "Actual size"),
        ] {
            let active = self.viewer_fit == fit;
            menu = menu.child(
                div()
                    .px_2()
                    .py_1()
                    .cursor_pointer()
                    .hover(|this| this.bg(cx.theme().muted))
                    .bg(if active {
                        cx.theme().muted
                    } else {
                        cx.theme().popover
                    })
                    .child(label)
                    .id("viewer-fit-item")
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.select_viewer_fit(fit, cx);
                    })),
            );
        }
        menu
    }
}
