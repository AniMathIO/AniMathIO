//! Shared components (see `docs/design.md` "Components"): one implementation of
//! each — buttons, panel headers, the segmented control, the floating tool
//! panel, form rows, the empty state, chips and the toast styling.
//!
//! Surface PRs (UI rework, PRs 2-6) adopt these one file at a time; until then
//! some are unused (the crate allows dead code in this module).

use gpui::{
    AnyElement, InteractiveElement as _, IntoElement as _, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, prelude::FluentBuilder as _,
};
use gpui_component::{
    Sizable as _,
    button::{Button, ButtonVariants as _},
    switch::Switch,
    tooltip::Tooltip,
};

use super::tokens::{Alpha, Role, TextRole as _};
use super::{color, hex, radius, size, tint};
use crate::icons::{IconName, icon};

// --- Buttons ---------------------------------------------------------------

/// The primary button: solid accent fill, `ON_ACCENT` text, weight 600, hover
/// and pressed states (see `docs/design.md` "Buttons"). At most one per
/// dialog, tool panel or view.
///
/// Built on the kit's `Primary` variant, which reads `primary`, `primary_hover`,
/// `primary_active` and `primary_foreground` from the theme (`apply_palette`).
/// The kit's `Custom` variant is not usable here: it fills the button with a
/// 20% wash of its colour, not the solid colour.
pub fn primary_button(id: impl Into<gpui::ElementId>, _cx: &gpui::App) -> Button {
    Button::new(id)
        .primary()
        .font_weight(gpui::FontWeight::SEMIBOLD)
}

/// The secondary button (a filled raised button with a strong border):
/// "Cancel", "Browse…", every other button with a label. The theme maps the
/// kit's `Secondary` variant to `RAISED`, `HOVER`, `PRESSED` and `TEXT`.
pub fn secondary_button(id: impl Into<gpui::ElementId>, _cx: &gpui::App) -> Button {
    Button::new(id)
        .secondary()
        .border_color(hex(color::BORDER_STRONG))
}

/// The destructive button. Filled (danger fill, `ON_ACCENT` text) in a confirm
/// dialog whose purpose is destruction; otherwise a secondary button with
/// danger text (see `docs/design.md` "Buttons").
pub fn destructive_button(id: impl Into<gpui::ElementId>, filled: bool, cx: &gpui::App) -> Button {
    if filled {
        Button::new(id).danger()
    } else {
        secondary_button(id, cx).text_color(hex(color::DANGER))
    }
}

// --- Docked panels ---------------------------------------------------------

/// A floating panel, leaving half a gutter on each shared edge.
pub fn panel(content: impl gpui::IntoElement) -> gpui::Div {
    let inset = super::gutter() / 2.;
    gpui::div().size_full().p(inset).child(
        gpui::div()
            .size_full()
            .overflow_hidden()
            .rounded(gpui::px(radius::PANEL))
            .border_1()
            .border_color(hex(color::BORDER))
            .bg(hex(color::PANEL))
            .child(content),
    )
}

/// A docked panel header (see `docs/design.md` "Docked panels"): `HEADER`
/// high, panel fill, a hairline below, the icon (small, `TEXT_SECONDARY`),
/// the title in `Heading`, then the actions. Legacy three-argument form for
/// callers without a context line (PR 2 switches them to
/// [`panel_header_full`]).
pub fn panel_header(
    icon_name: IconName,
    title: &'static str,
    actions: impl gpui::IntoElement,
) -> gpui::Div {
    panel_header_full(icon_name, title, None, actions)
}

/// A docked panel header with the optional context (the active tab, the track
/// count): `HEADER` high, panel fill, a hairline below, icon (small,
/// `TEXT_SECONDARY`), title in `Heading`, context in `Label` `TEXT_SECONDARY`,
/// then the actions (at most three ghost icon buttons; more belong in an
/// overflow menu).
pub fn panel_header_full(
    icon_name: IconName,
    title: &'static str,
    context: Option<gpui::SharedString>,
    actions: impl gpui::IntoElement,
) -> gpui::Div {
    gpui::div()
        .flex()
        .items_center()
        .h(size::HEADER)
        .flex_shrink_0()
        .px_3()
        .gap_2()
        .bg(hex(color::PANEL))
        .border_b_1()
        .border_color(hex(color::BORDER))
        .child(
            icon(icon_name)
                .small()
                .text_color(hex(color::TEXT_SECONDARY)),
        )
        .child(gpui::div().flex_1().text_role(Role::Heading).child(title))
        .children(context.map(|context| {
            gpui::div()
                .text_role(Role::Label)
                .text_color(hex(color::TEXT_SECONDARY))
                .child(context)
        }))
        .child(actions)
}

// --- Tab strips and segmented controls -------------------------------------

/// One option of a [`segmented`] control (or tab strip).
#[derive(Clone, Copy)]
pub struct SegmentedOption {
    /// The label (shown when the control is labelled; the tooltip otherwise).
    pub label: &'static str,
    /// The (optional) icon. Shown always; the only thing shown when
    /// icon-only.
    pub icon: Option<IconName>,
    /// The element/debug id for this option (for example `library-tab-media`).
    pub id: &'static str,
}

impl SegmentedOption {
    pub fn new(id: &'static str, label: &'static str, icon: Option<IconName>) -> Self {
        Self { label, icon, id }
    }
}

/// A segmented control (see `docs/design.md` "Tab strips and segmented
/// controls"): a `BG` well, [`size::ROW`] high, control radius, options sharing
/// the width, the active one on an accent subtle fill with an accent edge
/// border and weight 500 (inactive: `TEXT_SECONDARY`, hover fill). Arrow keys,
/// Home and End move the selection (Space or Enter activates the focused
/// option); icon-only options get tooltips.
///
/// `slide` is `(from index, when the selection changed, reduce motion)`; pass
/// it (and keep requesting animation frames for
/// [`super::motion::BASE_MS`]) to animate the sliding pill — `workspace.rs`
/// (PR 2) shows the caller side.
pub fn segmented(
    id: &'static str,
    options: &[SegmentedOption],
    selected: usize,
    slide: Option<(usize, std::time::Instant, bool)>,
    labelled: bool,
    on_select: impl Fn(usize, &mut gpui::Window, &mut gpui::App) + Clone + 'static,
) -> gpui::Stateful<gpui::Div> {
    let count = options.len().max(1) as f32;
    let (from, eased) = match slide {
        Some((from, changed, reduce_motion)) => {
            let progress = if reduce_motion {
                1.
            } else {
                (changed.elapsed().as_secs_f32() / (super::motion::BASE_MS as f32 / 1000.))
                    .clamp(0., 1.)
            };
            (from as f32, 1. - (1. - progress).powi(3))
        }
        None => (selected as f32, 1.),
    };
    let indicator = gpui::div()
        .absolute()
        .left(gpui::relative(
            (from + (eased * (selected as f32 - from))) / count,
        ))
        .w(gpui::relative(1. / count))
        .h_full()
        .rounded(gpui::px(radius::CONTROL))
        .bg(tint(color::ACCENT, Alpha::Subtle))
        .border_1()
        .border_color(tint(color::ACCENT, Alpha::Edge));
    gpui::div().id(id).p_2().child(
        gpui::div()
            .relative()
            .flex()
            .h(size::ROW)
            .rounded(gpui::px(radius::CONTROL))
            .bg(hex(color::BG))
            .child(indicator)
            .children(options.iter().enumerate().map(|(position, option)| {
                let (label, option_id, option_icon) = (option.label, option.id, option.icon);
                let option_count = options.len();
                let selected_option = position == selected;
                let on_select = on_select.clone();
                let on_key_select = on_select.clone();
                gpui::div()
                    .id(option_id)
                    .debug_selector(|| option_id.to_string())
                    .tab_index(0)
                    .focus_visible(|styled| styled.border_1().border_color(hex(color::ACCENT)))
                    .on_key_down(move |event: &gpui::KeyDownEvent, window, cx| {
                        // Arrows move from the focused option (Tab can focus one
                        // that is not selected), as in a WAI-ARIA tab list.
                        let (back, forward) = (
                            (position + option_count - 1) % option_count,
                            (position + 1) % option_count,
                        );
                        let next = match event.keystroke.key.as_str() {
                            "enter" | "space" => Some(position),
                            "right" => Some(forward),
                            "left" => Some(back),
                            "home" => Some(0),
                            "end" => Some(option_count - 1),
                            _ => None,
                        };
                        if let Some(next) = next {
                            on_key_select(next, window, cx);
                            cx.stop_propagation();
                        }
                    })
                    .relative()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap_1()
                    .cursor_pointer()
                    .text_color(if selected_option {
                        hex(color::TEXT)
                    } else {
                        hex(color::TEXT_SECONDARY)
                    })
                    .text_role(Role::Label)
                    .when(selected_option, |this| {
                        this.font_weight(gpui::FontWeight(500.))
                    })
                    .when(!selected_option, |this| {
                        this.hover(|styled| styled.bg(hex(color::HOVER)))
                    })
                    .when(!labelled, |this| {
                        this.tooltip(move |window, cx| Tooltip::new(label).build(window, cx))
                    })
                    .on_click(move |_event, window, cx| on_select(position, window, cx))
                    .children(option_icon.map(|icon_name| icon(icon_name).xsmall()))
                    .when(labelled, |this| this.child(label))
            })),
    )
}

// --- Floating tool panels --------------------------------------------------

/// A header close handler (the header (X) button).
pub type OnClose = Box<dyn Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static>;

/// The shared frame for non-modal panels over the workspace (Markers and
/// beats, Automatic cuts, Auto duck, Scenes, Auto reframe, the export queue;
/// see `docs/design.md` "Floating tool panels"): anchored at the top left of
/// the viewer area, 24rem wide, at most the space above the timeline, overlay
/// radius and shadow, a scrolling body with a visible scrollbar and a pinned
/// footer.
///
/// ```ignore
/// style::ToolPanel::new("markers-panel", IconName::Flag, "Markers and beats")
///     .bottom_height(editor.dock.bottom_height)
///     .on_close(cx.listener(|this, _, _, cx| this.close_marker_panel(cx)))
///     .body(scroll.clone(), markers_body(editor, cx))
///     .footer(primary_button("markers-add", cx).label("Add at playhead"))
///     .render(window)
/// ```
pub struct ToolPanel {
    id: &'static str,
    icon: IconName,
    title: &'static str,
    /// Height (in scale units) of the docked area below the viewer; the panel
    /// never grows into it (`ducking.rs` shows the caller side).
    bottom_height: f32,
    on_close: Option<OnClose>,
    body: Option<AnyElement>,
    footer: Option<AnyElement>,
    /// Scroll handle for the scrolling body (owned by the caller's view state).
    scroll: Option<gpui::ScrollHandle>,
}

impl ToolPanel {
    pub fn new(id: &'static str, icon: IconName, title: &'static str) -> Self {
        Self {
            id,
            icon,
            title,
            bottom_height: 0.,
            on_close: None,
            body: None,
            footer: None,
            scroll: None,
        }
    }

    /// The dock height (in scale units, as `dock.bottom_height` reports it)
    /// the panel must stay above.
    pub fn bottom_height(mut self, bottom_height: f32) -> Self {
        self.bottom_height = bottom_height;
        self
    }

    /// The header close (X) button's handler.
    pub fn on_close(
        mut self,
        on_close: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
    ) -> Self {
        self.on_close = Some(Box::new(on_close));
        self
    }

    /// The scrolling body. Pass the view's own `ScrollHandle` so the panel
    /// remembers its scroll position across renders.
    pub fn body(mut self, scroll: gpui::ScrollHandle, body: impl gpui::IntoElement) -> Self {
        self.scroll = Some(scroll);
        self.body = Some(body.into_any_element());
        self
    }

    /// The pinned footer (the primary action; there is no separate close
    /// button here — the header X closes).
    pub fn footer(mut self, footer: impl gpui::IntoElement) -> Self {
        self.footer = Some(footer.into_any_element());
        self
    }

    /// Build the frame (see the struct docs for the geometry).
    #[allow(clippy::too_many_lines)]
    pub fn render(self, window: &gpui::Window) -> gpui::Stateful<gpui::Div> {
        use gpui::{div, px};
        let scale = super::ui_scale();
        // The tool panels sit below the title bar (56px) and end above the
        // timeline dock and the status line, so their tools stay usable.
        let max_height = (window.viewport_size().height
            - px(56. + 32. + self.bottom_height * scale))
        .max(px(160.));
        let Self {
            id,
            icon: icon_name,
            title,
            on_close,
            body,
            footer,
            scroll,
            ..
        } = self;
        gpui::div()
            .id(id)
            .debug_selector(move || id.to_string())
            .absolute()
            .left(gpui::rems(0.75))
            .top(px(56. * scale))
            .w(gpui::rems(24.))
            .max_h(max_height)
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(hex(color::RAISED))
            .text_color(hex(color::TEXT))
            .border_1()
            .border_color(hex(color::BORDER_STRONG))
            .rounded(gpui::px(radius::OVERLAY))
            .shadow_lg()
            .occlude()
            // Header: HEADER high, title in TITLE, close (X) at the right.
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .h(size::HEADER)
                    .flex_shrink_0()
                    .px_3()
                    .child(
                        icon(icon_name)
                            .small()
                            .text_color(hex(color::TEXT_SECONDARY)),
                    )
                    .child(div().flex_1().text_role(Role::Title).child(title))
                    .when_some(on_close, |this, on_close| {
                        this.child(
                            Button::new("tool-panel-close")
                                .ghost()
                                .xsmall()
                                .icon(icon(IconName::X).xsmall())
                                .tooltip("Close")
                                .on_click(on_close),
                        )
                    }),
            )
            // Scrolling body with a visible scrollbar (the kit always-on mode).
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .when_some(scroll, |this, scroll| {
                        let body = body.unwrap_or_else(|| div().into_any_element());
                        this.child(
                            div()
                                .id("tool-panel-body")
                                .flex_1()
                                .min_h_0()
                                .flex()
                                .flex_col()
                                .track_scroll(&scroll)
                                .overflow_y_scroll()
                                .child(body),
                        )
                        .child(gpui_component::scroll::Scrollbar::vertical(&scroll))
                    }),
            )
            // Pinned footer with the primary action.
            .when_some(footer, |this, footer| {
                this.child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap_2()
                        .border_t_1()
                        .border_color(hex(color::BORDER))
                        .px_3()
                        .py_2()
                        .child(footer),
                )
            })
    }
}

// --- Forms -----------------------------------------------------------------

/// A form row (see `docs/design.md` "Dialogs"): the label in a fixed 8.5rem
/// left column (`Label`, `TEXT_SECONDARY`), the control filling the rest.
pub fn form_row(label: &'static str, control: impl gpui::IntoElement) -> gpui::Div {
    gpui::div()
        .flex()
        .items_center()
        .gap_2()
        .min_h(size::ROW)
        .child(
            gpui::div()
                .w(gpui::rems(8.5))
                .flex_shrink_0()
                .text_role(Role::Label)
                .text_color(hex(color::TEXT_SECONDARY))
                .child(label),
        )
        .child(control)
}

/// A form group (see `docs/design.md` "Dialogs"): a `Heading` group title over
/// its rows (dialogs separate groups by a 4-step gap).
pub fn form_group(title: &'static str, rows: impl IntoIterator<Item = AnyElement>) -> gpui::Div {
    gpui::div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            gpui::div()
                .text_role(Role::Heading)
                .text_color(hex(color::TEXT))
                .child(title),
        )
        .children(rows)
}

/// A labelled switch row (see `docs/design.md` "Fields, sliders, toggles and
/// selects"): label to the left on the same row, the switch at the right edge,
/// an optional one-line description below in `Label` `TEXT_MUTED`. The
/// handler receives the (possibly new) checked state.
pub fn switch_row(
    id: &'static str,
    label: &'static str,
    description: Option<&'static str>,
    checked: bool,
    on_change: impl Fn(&bool, &mut gpui::Window, &mut gpui::App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    gpui::div()
        .id(id)
        .debug_selector(move || id.to_string())
        .flex()
        .flex_col()
        .min_h(size::ROW)
        .py_1()
        .child(
            gpui::div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    gpui::div()
                        .text_role(Role::Label)
                        .text_color(hex(color::TEXT_SECONDARY))
                        .child(label),
                )
                .child(Switch::new(id).checked(checked).on_click(on_change)),
        )
        .children(description.map(|description| {
            gpui::div()
                .text_role(Role::Label)
                .text_color(hex(color::TEXT_MUTED))
                .child(description)
        }))
}

// --- Empty states, chips, toggles ------------------------------------------

/// The one empty state (see `docs/design.md` "Empty states"): centred in its
/// space, an icon (1.5rem, `TEXT_MUTED`), one line (Body,
/// `TEXT_SECONDARY`) saying what will be here, and one action that adds it.
pub fn empty_state(
    icon_name: Option<IconName>,
    line: &'static str,
    action: Option<AnyElement>,
) -> gpui::Div {
    gpui::div()
        .w_full()
        .py_4()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_2()
        .text_color(hex(color::TEXT_SECONDARY))
        .children(icon_name.map(|icon_name| {
            icon(icon_name)
                .w(gpui::rems(1.5))
                .h(gpui::rems(1.5))
                .text_color(hex(color::TEXT_MUTED))
        }))
        .child(gpui::div().text_role(Role::Body).child(line))
        .children(action)
}

/// A status chip (see `docs/design.md` "Window frame and status bar"):
/// `ROW_XS` high, pill radius, raised fill, `Label` `TEXT_SECONDARY`, an
/// optional small icon. Chain `.tabular()` for sizes and durations (the digits
/// then do not jitter).
pub fn chip(
    id: &'static str,
    icon_name: Option<IconName>,
    label: impl Into<gpui::SharedString>,
) -> gpui::Stateful<gpui::Div> {
    gpui::div()
        .id(id)
        .debug_selector(move || id.to_string())
        .flex()
        .items_center()
        .gap_1()
        .h(size::ROW_XS)
        .px_2()
        .flex_shrink_0()
        .rounded_full()
        .bg(hex(color::RAISED))
        .text_role(Role::Label)
        .text_color(hex(color::TEXT_SECONDARY))
        .children(icon_name.map(|icon_name| icon(icon_name).xsmall()))
        .child(label.into())
}

/// A small toggle pill (track-header mute/solo/lock, timeline toolbar
/// toggles): `ROW_XS` high, control radius, muted icon when off; when on, the
/// `tint` token at the subtle fill and edge border steps (see `docs/design.md`
/// "Timeline" and "Interaction states").
pub fn icon_toggle(
    id: String,
    icon_name: IconName,
    tooltip: &'static str,
    active: bool,
    tint_token: u32,
    listener: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let id_string = id.clone();
    gpui::div()
        .id(id)
        .debug_selector(move || id_string.clone())
        .h(size::ROW_XS)
        .min_w(size::ROW_XS)
        .px_1()
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(gpui::px(radius::CONTROL))
        .cursor_pointer()
        .border_1()
        .border_color(if active {
            tint(tint_token, Alpha::Edge)
        } else {
            gpui::transparent_black()
        })
        .bg(if active {
            tint(tint_token, Alpha::Subtle)
        } else {
            gpui::transparent_black()
        })
        .text_color(if active {
            hex(tint_token)
        } else {
            hex(color::TEXT_SECONDARY)
        })
        .hover(|styled| styled.bg(hex(color::HOVER)))
        .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tooltip).build(window, cx))
        .on_click(listener)
        .child(icon(icon_name).xsmall())
}

// --- Toasts ----------------------------------------------------------------

/// The kit owns toast placement, dismissal and its slide animation; the app
/// adds the semantic left stripe (see `docs/design.md` "Toasts") before
/// handing it to the kit.
pub struct Toast;
impl Toast {
    fn styled(
        message: impl Into<gpui::SharedString>,
        kind: gpui_component::notification::NotificationType,
        color: u32,
    ) -> gpui_component::notification::Notification {
        gpui_component::notification::Notification::new()
            .message(message)
            .with_type(kind)
            .border_0()
            .border_l(gpui::px(3.))
            .border_color(hex(color))
            .rounded(gpui::px(radius::OVERLAY))
    }
    pub fn info(
        message: impl Into<gpui::SharedString>,
    ) -> gpui_component::notification::Notification {
        Self::styled(
            message,
            gpui_component::notification::NotificationType::Info,
            color::ACCENT,
        )
    }
    pub fn warning(
        message: impl Into<gpui::SharedString>,
    ) -> gpui_component::notification::Notification {
        Self::styled(
            message,
            gpui_component::notification::NotificationType::Warning,
            color::WARNING,
        )
    }
    pub fn success(
        message: impl Into<gpui::SharedString>,
    ) -> gpui_component::notification::Notification {
        Self::styled(
            message,
            gpui_component::notification::NotificationType::Success,
            color::SUCCESS,
        )
    }
    pub fn error(
        message: impl Into<gpui::SharedString>,
    ) -> gpui_component::notification::Notification {
        Self::styled(
            message,
            gpui_component::notification::NotificationType::Error,
            color::DANGER,
        )
    }
}
