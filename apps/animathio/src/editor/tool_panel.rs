//! Pieces shared by the floating tool panels (Markers and beats, Automatic cuts, Auto duck,
//! Scenes, Auto reframe): the body padding, section headings, status lines, the segmented
//! choice, a slider row with its value and the footer. Each panel builds its frame with
//! `style::ToolPanel`; these helpers keep the content the same everywhere.

use super::*;
use crate::style::{self, Role, TextRole as _, color, hex};
use gpui::Rems;
use gpui_component::slider::{Slider, SliderState};

/// Width of a slider's value readout.
const VALUE_WIDTH: Rems = Rems(5.);
/// Width of the check slot at the start of a [`check_row`].
const CHECK_SLOT: Rems = Rems(1.);

/// The scrolling body of a tool panel: padded, groups four steps apart.
pub(super) fn body() -> Div {
    // A wider right edge keeps values clear of the scrollbar that overlays it.
    div().flex().flex_col().gap_4().py_3().pl_3().pr_4()
}

/// A group of rows under a `Heading` title (the title may be dynamic, "Markers (3)").
pub(super) fn section(
    title: impl Into<SharedString>,
    rows: impl IntoIterator<Item = AnyElement>,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .text_role(Role::Heading)
                .text_color(hex(color::TEXT))
                .child(title.into()),
        )
        .children(rows)
}

/// A status or help line (`Label`, `TEXT_SECONDARY`).
pub(super) fn note(text: impl Into<SharedString>) -> Div {
    div()
        .text_role(Role::Label)
        .text_color(hex(color::TEXT_SECONDARY))
        .child(text.into())
}

/// An error line: an alert icon and the message in `DANGER`.
pub(super) fn problem(text: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .items_start()
        .gap_1()
        .text_role(Role::Label)
        .text_color(hex(color::DANGER))
        .child(
            div().pt_px().child(
                icons::icon(IconName::CircleAlert)
                    .xsmall()
                    .text_color(hex(color::DANGER)),
            ),
        )
        .child(div().flex_1().min_w_0().child(text.into()))
}

/// One `FocusHandle` per option of a segmented control (kept in the panel state, so the
/// keyboard can reach every option).
pub(super) fn focus_handles(count: usize, cx: &mut Context<Editor>) -> Vec<FocusHandle> {
    (0..count).map(|_| cx.focus_handle()).collect()
}

/// A labelled segmented choice (`style::segmented` without its outer padding, with the
/// control's debug id on the strip).
pub(super) fn choice(
    id: &'static str,
    options: &[style::SegmentedOption],
    focus: &[FocusHandle],
    selected: usize,
    on_select: impl Fn(usize, &mut Window, &mut App) + Clone + 'static,
) -> Stateful<Div> {
    style::segmented(id, options, focus, selected, None, true, on_select)
        .p_0()
        .debug_selector(move || id.to_string())
}

/// A slider with its exact value at the right (`Label`, tabular, right-aligned).
pub(super) fn slider_row(
    label: &'static str,
    state: &Entity<SliderState>,
    value: impl Into<SharedString>,
) -> Div {
    style::form_row(
        label,
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Slider::new(state).horizontal()),
            )
            .child(
                div()
                    .w(VALUE_WIDTH)
                    .flex_shrink_0()
                    .flex()
                    .justify_end()
                    .text_role(Role::Label)
                    .text_color(hex(color::TEXT_SECONDARY))
                    .tabular()
                    .child(value.into()),
            ),
    )
}

/// A selectable row with a leading check box (`Check` when on, an empty outline when off; never a
/// minus), used for "include this track" chips and proposed cuts. `ROW` high.
pub(super) fn check_row(
    id: impl Into<ElementId>,
    selector: Option<String>,
    on: bool,
    label: impl Into<SharedString>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let row = div()
        .id(id)
        .flex()
        .items_center()
        .gap_2()
        .min_h(style::size::ROW)
        .px_2()
        .rounded(px(style::radius::CONTROL))
        .cursor_pointer()
        .text_role(Role::Label)
        .text_color(hex(if on {
            color::TEXT
        } else {
            color::TEXT_SECONDARY
        }))
        .when(on, |row| {
            row.bg(style::tint(color::ACCENT, style::Alpha::Wash))
        })
        .hover(|row| row.bg(hex(color::HOVER)))
        .on_click(on_click)
        // An empty outline box when off, so an unchecked row never reads as plain text.
        .child(
            div()
                .w(CHECK_SLOT)
                .h(CHECK_SLOT)
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(style::radius::CONTROL))
                .border_1()
                .border_color(hex(if on {
                    color::ACCENT
                } else {
                    color::BORDER_STRONG
                }))
                .when(on, |slot| {
                    slot.child(
                        icons::icon(IconName::Check)
                            .xsmall()
                            .text_color(hex(color::ACCENT)),
                    )
                }),
        )
        .child(div().flex_1().min_w_0().truncate().child(label.into()));
    match selector {
        Some(selector) => row.debug_selector(move || selector.clone()),
        None => row,
    }
}

/// A duration as seconds with `decimals` places ("0.50 s"), for slider values.
#[allow(clippy::cast_precision_loss)]
pub(super) fn seconds_label(time: Time, decimals: usize) -> String {
    format!("{:.decimals$} s", time.0 as f64 / TICKS_PER_SECOND as f64)
}

/// A label over a full-width control, for controls too wide for the label column
/// (segmented choices with long options).
pub(super) fn stacked(label: &'static str, control: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_role(Role::Label)
                .text_color(hex(color::TEXT_SECONDARY))
                .child(label),
        )
        .child(control)
}
