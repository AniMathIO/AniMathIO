//! The 2.3 timeline, transport and viewer: no overlap in the transport row at common window
//! sizes, one-row track headers, the S/M/L control and clip labels without a repeated duration.

use gpui::{Bounds, Pixels, px, size};

use super::Ui;
use super::fixtures::*;
use crate::layout::TrackHeight;

fn bounds(ui: &mut Ui, id: &'static str) -> Option<Bounds<Pixels>> {
    ui.cx.debug_bounds(id)
}

/// The transport row must show its parts side by side at every window size: the timecode
/// never reaches the step buttons, the duration never reaches them from the other side, and
/// everything stays inside the window. (Regression: at 1024x768 the time readout overlapped
/// the step-back button and the duration.)
#[gpui::test]
fn transport_parts_never_overlap_at_common_window_sizes(cx: &mut gpui::TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    for (width, height) in [(1024., 768.), (1280., 720.), (1366., 768.), (1920., 1080.)] {
        ui.cx.simulate_resize(size(px(width), px(height)));
        ui.settle();
        let label = format!("{width}x{height}");
        let timecode =
            bounds(&mut ui, "timecode-display").unwrap_or_else(|| panic!("{label}: timecode"));
        let centre =
            bounds(&mut ui, "transport-center").unwrap_or_else(|| panic!("{label}: buttons"));
        let right = bounds(&mut ui, "transport-right").unwrap_or_else(|| panic!("{label}: right"));
        assert!(
            timecode.right() <= centre.left(),
            "{label}: the timecode ({timecode:?}) overlaps the buttons ({centre:?})"
        );
        assert!(
            centre.right() <= right.left() + px(0.5) || right.size.width <= px(0.),
            "{label}: the buttons ({centre:?}) overlap the right column ({right:?})"
        );
        if let Some(duration) = bounds(&mut ui, "transport-duration") {
            assert!(
                duration.left() >= centre.right(),
                "{label}: the duration ({duration:?}) overlaps the buttons ({centre:?})"
            );
            assert!(
                duration.right() <= px(width),
                "{label}: the duration is on screen"
            );
        }
        assert!(
            centre.left() >= px(0.) && centre.right() <= px(width),
            "{label}: buttons on screen"
        );
        assert!(
            timecode.left() >= px(0.),
            "{label}: the timecode is on screen"
        );
    }
}

/// A track header is one row: the name and the first control share a vertical centre at the
/// large track height (the 2.2 header put the name at the top and the controls at the bottom).
#[gpui::test]
fn track_header_name_and_pills_share_a_row_at_every_height(cx: &mut gpui::TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.cx.simulate_resize(size(px(1600.), px(2400.)));
    let track = ui.read(|e| e.project.tracks()[0].id());
    for height in [TrackHeight::Small, TrackHeight::Medium, TrackHeight::Large] {
        // Set directly: `set_track_height` saves the dock layout, which would leak into
        // scenarios that expect a first run.
        ui.update(|e, _, _| e.dock.track_height = height);
        ui.settle();
        let name = ui
            .cx
            .debug_bounds(Box::leak(
                format!("track-name-{}", track.0).into_boxed_str(),
            ))
            .expect("the track name");
        let pill = ui
            .cx
            .debug_bounds(Box::leak(
                format!("track-mute-{}", track.0).into_boxed_str(),
            ))
            .expect("the first pill");
        let gap = (name.center().y - pill.center().y).abs();
        assert!(
            gap <= px(2.),
            "{height:?}: the name centre is {gap:?} from the pill centre"
        );
        assert!(
            name.right() <= pill.left(),
            "{height:?}: the name sits left of the pills"
        );
    }
}

/// S, M and L are one segmented control, reachable in the toolbar. (Clicking an option calls
/// `set_track_height`, which saves the dock layout; that is covered by `polish_inspector`.)
#[gpui::test]
fn the_track_height_segments_are_in_the_toolbar(cx: &mut gpui::TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.cx.simulate_resize(size(px(1280.), px(900.)));
    ui.settle();
    let mut previous: Option<Bounds<Pixels>> = None;
    for id in ["track-height-S", "track-height-M", "track-height-L"] {
        let option = bounds(&mut ui, id).unwrap_or_else(|| panic!("{id} is rendered"));
        assert!(
            option.right() <= px(1280.) && option.bottom() <= px(900.),
            "{id}"
        );
        if let Some(previous) = previous {
            assert!(
                option.left() >= previous.right(),
                "{id} follows the previous option"
            );
        }
        previous = Some(option);
    }
}

/// The clip label is the name alone: the duration is painted after it only when it fits.
#[gpui::test]
fn clip_labels_no_longer_repeat_the_duration(cx: &mut gpui::TestAppContext) {
    let (project, assets) = sample();
    let ui = Ui::open(cx, project, assets);
    let labels: Vec<String> = ui.cx.update(|_, cx| {
        ui.editor
            .read(cx)
            .build_timeline_snapshot(cx)
            .clips
            .iter()
            .map(|clip| clip.label.to_string())
            .collect()
    });
    assert_ne!(labels.len(), 0);
    for label in labels {
        assert!(!label.contains("00:00:"), "{label}");
    }
}
