//! The floating tool panels (Markers and beats, Automatic cuts, Auto duck, Auto reframe) share
//! one frame, stay above the timeline and use labelled controls instead of state-in-label buttons.

use gpui::{AppContext, Modifiers, ParentElement as _, TestAppContext, px, size};

use animathio_control::{MathObjectDraft, MathObjectKind, math_object_range};
use gpui_component::WindowExt;

use super::fixtures::*;
use super::scenarios::scenes_fixture;
use super::{Ui, actions};
use crate::editor::math::MathEditor;
use crate::editor::support::project_size;

const PANELS: [&str; 4] = [
    "marker-panel",
    "autocut-review",
    "duck-review",
    "auto-reframe-panel",
];

#[gpui::test]
fn tool_panels_share_a_frame_and_stay_above_the_timeline(cx: &mut TestAppContext) {
    for (width, height) in [(1280., 720.), (1920., 1080.)] {
        let (project, assets) = voice_and_music();
        let mut ui = Ui::open(cx, project, assets);
        ui.cx.simulate_resize(size(px(width), px(height)));
        ui.settle();
        ui.update(|e, _, cx| {
            e.select_only(VOICE_CLIP, cx);
            e.open_marker_panel(cx);
            e.open_autocut(cx);
            e.open_duck(cx);
            e.open_reframe(cx);
        });
        ui.settle();
        // The timeline header is the first thing in the dock, so a panel that ends above it
        // never covers the clips it works on.
        let timeline = ui
            .cx
            .debug_bounds("timeline-add-track")
            .expect("the timeline is rendered");
        let first = ui.cx.debug_bounds(PANELS[0]).expect("Markers panel");
        for id in PANELS {
            let panel = ui
                .cx
                .debug_bounds(id)
                .unwrap_or_else(|| panic!("{id} is rendered at {width}x{height}"));
            assert_eq!(panel.left(), first.left(), "{id} opens on the same side");
            assert_eq!(panel.top(), first.top(), "{id} starts at the same height");
            assert_eq!(
                panel.size.width, first.size.width,
                "{id} has the same width"
            );
            assert!(
                panel.bottom() <= timeline.top(),
                "{id} covers the timeline at {width}x{height}: {panel:?} vs {timeline:?}"
            );
        }
    }
}

#[gpui::test]
fn markers_panel_uses_segmented_choices_and_the_header_closes_it(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| e.open_marker_panel(cx));
    ui.settle();
    for selector in [
        "beats-scope",
        "beats-every",
        "sync-source",
        "sync-shift",
        "beats-detect",
        "beats-add-markers",
        "marker-add",
    ] {
        assert!(
            ui.cx.debug_bounds(selector).is_some(),
            "{selector} is rendered"
        );
    }
    // No "Close" button in the footer: the header X closes the panel.
    assert!(ui.cx.debug_bounds("marker-close").is_none());
    let every_four = ui.cx.debug_bounds("beats-every-4").expect("Every 4 option");
    ui.click(every_four.center(), Modifiers::default());
    let downbeats = ui
        .cx
        .debug_bounds("beats-downbeats")
        .expect("Downbeats option");
    ui.click(downbeats.center(), Modifiers::default());
    let shift = ui.cx.debug_bounds("sync-shift-500").expect("500 ms option");
    ui.click(shift.center(), Modifiers::default());
    ui.read(|e| {
        let panel = e.marker_panel.as_ref().expect("panel open");
        assert_eq!(panel.settings(), (4, true, 500));
    });
    // The header X is the one close control, found by its own debug id.
    let close = ui
        .cx
        .debug_bounds("tool-panel-close")
        .expect("the header close button");
    let panel = ui.cx.debug_bounds("marker-panel").expect("panel");
    assert!(
        close.right() <= panel.right() && close.top() >= panel.top(),
        "the X sits in the header: {close:?} in {panel:?}"
    );
    ui.click(close.center(), Modifiers::default());
    assert!(ui.read(|e| e.marker_panel.is_none()));
}

#[gpui::test]
fn automatic_cuts_language_is_a_choice_not_a_cycling_button(cx: &mut TestAppContext) {
    let (project, assets) = voice_and_music();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| {
        e.select_only(VOICE_CLIP, cx);
        e.open_autocut(cx);
    });
    ui.settle();
    for selector in [
        "autocut-scope",
        "autocut-language",
        "autocut-short-pauses",
        "autocut-fillers",
        "autocut-analyse",
        "autocut-cancel",
        "autocut-apply",
    ] {
        assert!(
            ui.cx.debug_bounds(selector).is_some(),
            "{selector} is rendered"
        );
    }
    let hungarian = ui
        .cx
        .debug_bounds("autocut-language-hu")
        .expect("Hungarian option");
    ui.click(hungarian.center(), Modifiers::default());
    ui.settle();
    ui.read(|e| {
        let review = e.autocut_review.as_ref().expect("panel open");
        assert_eq!(review.params.language.as_deref(), Some("hu"));
    });
    let auto = ui
        .cx
        .debug_bounds("autocut-language-auto")
        .expect("Auto option");
    ui.click(auto.center(), Modifiers::default());
    ui.settle();
    assert_eq!(
        ui.read(|e| e.autocut_review.as_ref().unwrap().params.language.clone()),
        None
    );
}

#[gpui::test]
fn reframe_panel_picks_the_aspect_with_a_segmented_control(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| e.open_reframe(cx));
    ui.settle();
    let square = ui.cx.debug_bounds("reframe-1:1").expect("1:1 option");
    ui.click(square.center(), Modifiers::default());
    assert_eq!(
        ui.read(|e| e.reframe_panel.as_ref().unwrap().aspect().to_owned()),
        "1:1"
    );
    // The footer has the one primary action and no "Close": the header X closes.
    assert!(ui.cx.debug_bounds("reframe-start").is_some());
    assert!(ui.cx.debug_bounds("reframe-cancel").is_none());
}

/// Both footer buttons of the Markers panel stay inside it at 720p and a 125 % UI scale, and
/// "Add at playhead" is the one that can run until beats have been detected.
#[gpui::test]
fn markers_footer_buttons_stay_inside_the_panel_at_a_larger_ui_scale(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.cx.simulate_resize(size(px(1280.), px(720.)));
    // The window's rem is the theme font size (as Preferences sets it), so this scales the
    // controls and their labels exactly like the 125 % UI scale does.
    ui.update(|_, _, cx| gpui_component::Theme::update(cx, |theme| theme.font_size = px(20.)));
    ui.update(|e, _, cx| e.open_marker_panel(cx));
    ui.settle();
    let panel = ui.cx.debug_bounds("marker-panel").expect("panel");
    for id in ["marker-add", "beats-add-markers"] {
        let button = ui
            .cx
            .debug_bounds(id)
            .unwrap_or_else(|| panic!("{id} is rendered"));
        assert!(
            button.left() >= panel.left()
                && button.right() <= panel.right()
                && button.top() >= panel.top()
                && button.bottom() <= panel.bottom(),
            "{id} is clipped at 125%: {button:?} in {panel:?}"
        );
    }
}

/// "Selected clip" cannot be chosen without a selection: it explains itself and ignores clicks.
#[gpui::test]
fn beats_scope_selected_clip_is_unavailable_without_a_selection(cx: &mut TestAppContext) {
    let (project, assets) = voice_and_music();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| {
        e.selected = None;
        e.selection.clear();
        e.open_marker_panel(cx);
    });
    ui.settle();
    let clip = ui.cx.debug_bounds("beats-scope-clip").expect("option");
    ui.click(clip.center(), Modifiers::default());
    assert!(!ui.read(|e| e.marker_panel.as_ref().unwrap().selected_only()));
    ui.update(|e, _, cx| e.select_only(VOICE_CLIP, cx));
    ui.settle();
    let clip = ui.cx.debug_bounds("beats-scope-clip").expect("option");
    ui.click(clip.center(), Modifiers::default());
    assert!(ui.read(|e| e.marker_panel.as_ref().unwrap().selected_only()));
}

/// The Scenes choices map onto the detection parameters (sensitivity low/medium/high, the
/// minimum shot and the "delete shorter" frames) and a change keeps the panel open.
#[gpui::test]
fn scenes_segmented_choices_set_the_detection_params(cx: &mut TestAppContext) {
    let (project, assets) = scenes_fixture();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| e.select_only(animathio_core::ClipId(1), cx));
    ui.dispatch(actions::SceneCuts);
    ui.settle();
    for (selector, expect) in [
        ("scene-sensitivity-high", (0.75, 6, 0)),
        ("scene-sensitivity-low", (0.25, 6, 0)),
        ("scene-min-12", (0.25, 12, 0)),
        ("scene-delete-24", (0.25, 12, 24)),
        ("scene-delete-0", (0.25, 12, 0)),
    ] {
        let option = ui
            .cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is rendered"));
        ui.click(option.center(), Modifiers::default());
        ui.settle();
        let got = ui.read(|e| {
            let params = e.scene_panel.as_ref().expect("panel stays open").params();
            (
                params.sensitivity,
                params.min_shot_frames,
                params.delete_shorter_than_frames,
            )
        });
        assert_eq!(got, expect, "after {selector}");
    }
}

/// Snippet chips are real buttons: they take keyboard focus and Enter inserts the snippet.
#[gpui::test]
fn math_snippet_chips_are_keyboard_reachable(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    let panel = ui.update(|e, window, cx| {
        let draft = MathObjectDraft::new(MathObjectKind::Latex);
        let range = math_object_range(e.position).expect("range");
        let editor = cx.entity().downgrade();
        let session = e.project_session;
        let size = project_size(&e.project);
        cx.new(|cx| MathEditor::new(draft, editor, session, range, None, true, size, window, cx))
    });
    ui.cx.update(|window, cx| {
        let panel = panel.clone();
        window.open_dialog(cx, move |dialog, _, _| dialog.child(panel.clone()));
    });
    ui.advance_clock(std::time::Duration::from_millis(400));
    assert!(ui.cx.debug_bounds("math-chip-Fraction").is_some());
    let handle = ui
        .cx
        .update(|_, cx| panel.read(cx).chip_focus_for("Fraction"))
        .expect("the Fraction chip has a focus handle");
    ui.cx.update(|window, cx| window.focus(&handle, cx));
    ui.settle();
    assert!(ui.cx.update(|window, _| handle.is_focused(window)));
    ui.press("enter");
    ui.advance_clock(std::time::Duration::from_millis(400));
    let source = ui.cx.update(|_, cx| panel.read(cx).draft_source());
    assert!(source.contains(r"\frac{a}{b}"), "inserted: {source:?}");
}
