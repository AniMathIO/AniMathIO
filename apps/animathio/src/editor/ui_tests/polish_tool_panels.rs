//! The floating tool panels (Markers and beats, Automatic cuts, Auto duck, Auto reframe) share
//! one frame, stay above the timeline and use labelled controls instead of state-in-label buttons.

use gpui::{Modifiers, TestAppContext, point, px, size};

use super::Ui;
use super::fixtures::*;

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
    assert!(ui.cx.debug_bounds("marker-panel-close").is_none());
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
    // The header X sits at the right end of the header row (it has no debug id of its own).
    let panel = ui.cx.debug_bounds("marker-panel").expect("panel");
    ui.click(
        point(panel.right() - px(22.), panel.top() + px(18.)),
        Modifiers::default(),
    );
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
