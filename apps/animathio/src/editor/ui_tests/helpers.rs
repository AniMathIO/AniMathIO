//! Shared actions and assertions for editor UI tests.

use super::fixtures::MS;
use super::{Editor, LeftTab, Time, Ui};
use crate::timeline;

pub(super) fn clip_count(editor: &Editor) -> usize {
    editor
        .project
        .tracks()
        .iter()
        .map(|track| track.clips().len())
        .sum()
}

pub(super) fn seek(ui: &mut Ui, ms: i64) {
    ui.update(|editor, _, cx| editor.seek_to(Time(ms * MS), cx));
}

/// A window-space point on the timeline ruler at `time` (the marker lane).
pub(super) fn ruler_point(ui: &Ui, time: Time) -> gpui::Point<gpui::Pixels> {
    ui.read(|e| {
        let layout = e.current_layout();
        let x = timeline::time_to_x(&layout, time);
        let y = layout.ruler_height / 2.;
        gpui::point(
            gpui::px(f32::from(e.timeline_bounds.origin.x) + x),
            gpui::px(f32::from(e.timeline_bounds.origin.y) + y),
        )
    })
}

pub(super) fn dialog_ready(ui: &mut Ui) {
    ui.advance_clock(std::time::Duration::from_millis(400));
}

pub(super) fn edit_dialog_field(ui: &mut Ui, selector: &'static str, value: &str) {
    let bounds = ui
        .cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    ui.click(bounds.center(), gpui::Modifiers::default());
    ui.press("secondary-a");
    ui.press("backspace");
    if !value.is_empty() {
        ui.cx.simulate_input(value);
    }
    ui.settle();
}

/// `debug_bounds` wants a `'static` selector; the leak is a few bytes in a test.
pub(super) fn bounds_of(ui: &mut Ui, selector: String) -> Option<gpui::Bounds<gpui::Pixels>> {
    ui.cx.debug_bounds(Box::leak(selector.into_boxed_str()))
}

pub(super) fn clear_error(ui: &mut Ui) {
    ui.update(|editor, _, _| editor.last_error = None);
}

pub(super) fn last_error(ui: &Ui) -> String {
    ui.read(|editor| editor.last_error.clone().unwrap_or_default())
}

pub(super) fn search_library(ui: &mut Ui, tab: LeftTab, query: &str) {
    ui.update(|e, _, cx| e.select_left_tab(tab, cx));
    ui.update(|e, window, cx| {
        let input = e.media_view.searches[tab as usize]
            .clone()
            .expect("rendered search");
        input.update(cx, |state, cx| state.set_value(query, window, cx));
        // `set_value` does not emit `InputEvent::Change`; mirror what the subscription stores.
        e.media_view.queries[tab as usize] = query.to_owned();
        cx.notify();
    });
}
