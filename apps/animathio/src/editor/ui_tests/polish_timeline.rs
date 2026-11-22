//! The 2.3 timeline, transport and viewer: no overlap in the transport row at common window
//! sizes, one-row track headers, the S/M/L control and clip labels without a repeated duration.

use gpui::{Bounds, Modifiers, Pixels, point, px, size};

use super::fixtures::*;
use super::helpers::{ruler_point, seek};
use super::{
    BTreeMap, BTreeSet, ClipId, Command, Interpolation, Project, Target, TestAppContext, Time,
    Track, TransformProperty, Ui, private_env, ruler_height,
};
use crate::editor::actions::{AddMarker, Redo, Undo};
use crate::editor::markers::marker_name_bounds;
use crate::editor::support::{
    KEYFRAME_OUTLINE_HALF, clip_block_bounds, clip_label_bounds, keyframe_center_y,
    ruler_tick_label_top,
};
use crate::layout::TrackHeight;
use crate::timeline;

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

#[gpui::test]
fn timecode_input_fits_and_seeks_at_1024x768(cx: &mut gpui::TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.cx.simulate_resize(size(px(1024.), px(768.)));
    ui.settle();
    let display = bounds(&mut ui, "timecode-display").expect("timecode display");
    ui.click(display.center(), Modifiers::default());

    let input = bounds(&mut ui, "timecode-edit").expect("timecode input");
    let left = bounds(&mut ui, "transport-left").expect("left column");
    let center = bounds(&mut ui, "transport-center").expect("transport buttons");
    assert!(input.left() >= left.left() && input.right() <= left.right());
    assert!(input.size.width >= display.size.width && input.right() < center.left());

    ui.press("secondary-a");
    ui.cx.simulate_input("00:00:01:00");
    ui.press("enter");
    assert!(bounds(&mut ui, "timecode-display").is_some());
    assert_eq!(ui.read(|e| e.position), Time(1000 * MS));
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

/// S, M and L are one segmented control, reachable in the toolbar.
#[gpui::test]
fn clicking_the_track_height_segments_resizes_tracks(cx: &mut gpui::TestAppContext) {
    let (project, assets) = sample();
    let env = private_env("track-height-segments");
    let mut ui = Ui::open_with_env(cx, project, assets, env.clone());
    ui.cx.simulate_resize(size(px(1280.), px(900.)));
    ui.settle();
    let track = ui.read(|e| timeline::display_tracks(&e.project)[0].id());
    let header_id = Box::leak(format!("track-header-{}", track.0).into_boxed_str());
    let mut previous: Option<Bounds<Pixels>> = None;
    for (id, height) in [
        ("track-height-S", TrackHeight::Small),
        ("track-height-M", TrackHeight::Medium),
        ("track-height-L", TrackHeight::Large),
    ] {
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
        ui.click(option.center(), Modifiers::default());
        assert_eq!(ui.read(|e| e.dock.track_height), height);
        assert_eq!(crate::layout::DockLayout::load(&env).track_height, height);
        let header = bounds(&mut ui, header_id).expect("track header");
        assert!((header.size.height - px(height.pixels())).abs() <= px(1.));
    }
}

#[gpui::test]
fn compact_clip_label_ends_before_a_keyframe_diamond(cx: &mut gpui::TestAppContext) {
    let (mut project, assets) = sample();
    let image = ClipId(2);
    // At this zoom the extra key falls inside the clip's duration label.
    let time = Time(750 * MS);
    Command::AddKeyframe {
        id: image,
        target: Target::Transform(TransformProperty::Rotation),
        time,
        keyframe: animathio_core::Keyframe {
            value: 0.0.into(),
            interpolation: Interpolation::Linear,
        },
    }
    .apply(&mut project)
    .unwrap();
    let mut ui = Ui::open_sized(
        cx,
        project,
        assets,
        super::test_env(),
        Some(size(px(1280.), px(720.))),
    );
    ui.update(|e, _, cx| {
        e.dock.track_height = TrackHeight::Small;
        e.select_only(image, cx);
    });
    ui.cx.update(|window, cx| {
        let editor = ui.editor.read(cx);
        let layout = editor.current_layout();
        let snapshot = editor.build_timeline_snapshot(cx);
        let clip = snapshot.clips.iter().find(|clip| clip.selected).unwrap();
        assert!(clip.keyframes.iter().any(|key| key.time == time));
        let timeline = Bounds::new(point(px(240.), px(435.)), size(px(layout.width), px(200.)));
        let block = clip_block_bounds(timeline, &layout, clip);
        let left = block.left() + px(22.);
        let diamond_x = timeline.left() + px(timeline::time_to_x(&layout, time));
        let diamond_y = timeline.top() + px(keyframe_center_y(&layout, clip.row));
        assert!(diamond_x + KEYFRAME_OUTLINE_HALF < block.right() - px(4.));
        let label = clip_label_bounds(timeline, &layout, clip, block, left, window.rem_size());
        assert!(label.bottom() > diamond_y - KEYFRAME_OUTLINE_HALF);
        assert!((label.right() - (diamond_x - KEYFRAME_OUTLINE_HALF)).abs() < px(0.05));
    });
}

#[gpui::test]
fn marker_name_plate_stays_above_the_ruler_tick_numbers(cx: &mut gpui::TestAppContext) {
    let (project, assets) = sample();
    let ui = Ui::open(cx, project, assets);
    ui.cx.update(|window, _| {
        let top = px(400.);
        let tick_label_top = ruler_tick_label_top(top + px(ruler_height()), window.rem_size());
        // Give the name the full remainder of the ruler as for a long, last marker.
        let plate = marker_name_bounds(point(px(240.), top), px(1200.), window.rem_size());
        assert!(plate.right() > px(300.));
        assert!(
            plate.bottom() <= tick_label_top,
            "{plate:?} crosses the tick row at {tick_label_top:?}"
        );
    });
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

#[gpui::test]
fn add_track_button_is_reachable_at_720p_and_undoable(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let before = project.clone();
    let mut ui = Ui::open(cx, project, assets);
    ui.cx
        .simulate_resize(gpui::size(gpui::px(1280.), gpui::px(720.)));
    ui.settle();
    let bounds = ui
        .cx
        .debug_bounds("timeline-add-track")
        .expect("visible Add track button");
    assert!(bounds.bottom() <= gpui::px(720.));
    assert!(bounds.right() <= gpui::px(1280.));
    ui.click(bounds.center(), gpui::Modifiers::default());
    ui.read(|editor| {
        assert_eq!(editor.project.tracks().len(), before.tracks().len() + 1);
        assert!(editor.project.tracks().last().unwrap().clips().is_empty());
        assert_eq!(
            &editor.project.tracks()[..before.tracks().len()],
            before.tracks()
        );
    });
    let added = ui.read(|editor| editor.project.clone());
    ui.dispatch(Undo);
    assert_eq!(ui.read(|editor| editor.project.clone()), before);
    ui.dispatch(Redo);
    assert_eq!(ui.read(|editor| editor.project.clone()), added);
}

#[gpui::test]
fn foreground_rows_match_hits_drops_selection_and_track_moves(cx: &mut TestAppContext) {
    let (mut project, assets) = voice_and_music();
    for (index, id) in [3, 4, 5].into_iter().enumerate() {
        Command::AddTrack {
            track: Track::new(animathio_core::TrackId(id), format!("V{id}")),
            index: index + 2,
        }
        .apply(&mut project)
        .unwrap();
        Command::AddClip {
            track: animathio_core::TrackId(id),
            clip: animathio_core::Clip::new(
                ClipId(id + 20),
                animathio_core::ClipKind::Error("visual".into()),
                animathio_core::TimeRange::new(Time(0), Time(1000 * MS)).unwrap(),
            ),
        }
        .apply(&mut project)
        .unwrap();
    }
    let before = project.clone();
    let mut ui = Ui::open(cx, project, assets);
    ui.read(|e| {
        let ids: Vec<_> = timeline::display_tracks(&e.project)
            .iter()
            .map(|t| t.id().0)
            .collect();
        assert_eq!(ids, [5, 4, 3, 1, 2]);
        let layout = e.current_layout();
        let y = layout.ruler_height + layout.row_height / 2.;
        assert_eq!(
            timeline::hit_test(&layout, &e.project, 10., y).map(|hit| hit.clip),
            Some(ClipId(25))
        );
        assert_eq!(
            timeline::drop_placement(&layout, &e.project, 10., y, Time(0), false)
                .unwrap()
                .0,
            animathio_core::TrackId(5)
        );
        assert_eq!(
            crate::selection::marquee(&layout, &e.project, (0., layout.ruler_height), (20., y)),
            BTreeSet::from([ClipId(25)])
        );
    });
    ui.update(|e, _, cx| e.move_track_to_row(animathio_core::TrackId(3), 0, cx));
    assert_eq!(
        ui.read(|e| timeline::display_tracks(&e.project)[0].id().0),
        3
    );
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
    ui.update(|e, _, cx| e.move_track_by(animathio_core::TrackId(5), 1, cx));
    assert_eq!(
        ui.read(|e| timeline::display_tracks(&e.project)[1].id().0),
        5
    );
    ui.dispatch(Undo);
    ui.update(|e, _, cx| e.add_timeline_track(cx));
    assert_eq!(
        ui.read(|e| timeline::display_tracks(&e.project)[0].id().0),
        6
    );
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

#[gpui::test]
fn ruler_and_marker_lane_stay_fixed_when_tracks_scroll(cx: &mut TestAppContext) {
    let mut project = Project::default();
    for id in 1..=12 {
        Command::AddTrack {
            track: Track::new(animathio_core::TrackId(id), format!("V{id}")),
            index: usize::try_from(id - 1).unwrap(),
        }
        .apply(&mut project)
        .unwrap();
        Command::AddClip {
            track: animathio_core::TrackId(id),
            clip: animathio_core::Clip::new(
                ClipId(id + 20),
                animathio_core::ClipKind::Error("visual".into()),
                animathio_core::TimeRange::new(Time(0), Time(2000 * MS)).unwrap(),
            ),
        }
        .apply(&mut project)
        .unwrap();
    }
    let mut ui = Ui::open(cx, project, BTreeMap::new());
    seek(&mut ui, 1000);
    ui.dispatch(AddMarker);
    let marker = ui.read(|e| e.project.markers()[0].id);
    let ruler_before = ruler_point(&ui, Time(1000 * MS));
    let top_before = ui.read(|e| e.timeline_tracks_top);
    ui.update(|e, _, cx| {
        e.timeline_scroll_handle.set_offset(gpui::point(
            gpui::px(0.),
            gpui::px(-e.dock.track_height.pixels() * 3.),
        ));
        cx.notify();
    });
    let ruler_after = ruler_point(&ui, Time(1000 * MS));
    assert_eq!(ruler_before, ruler_after);
    assert!(ui.read(|e| e.timeline_tracks_top) < top_before - 1.);
    ui.click(ruler_after, gpui::Modifiers::default());
    assert_eq!(
        ui.read(|e| e.marker_selection.clone()),
        BTreeSet::from([marker])
    );
    let (point, expected) = ui.read(|e| {
        let layout = e.current_layout();
        let point = gpui::point(
            e.timeline_bounds.origin.x + gpui::px(timeline::time_to_x(&layout, Time(500 * MS))),
            e.timeline_bounds.origin.y + gpui::px(layout.ruler_height + layout.row_height / 2.),
        );
        let row = ((f32::from(point.y) - e.timeline_tracks_top) / layout.row_height) as usize;
        (
            point,
            timeline::display_tracks(&e.project)[row]
                .clips()
                .values()
                .next()
                .unwrap()
                .id,
        )
    });
    assert_eq!(expected, ClipId(29), "three rows scrolled: V9 is now first");
    ui.click(point, gpui::Modifiers::default());
    assert_eq!(ui.read(|e| e.selected), Some(expected));
    ui.read(|e| {
        let (x, y) = e.timeline_relative(point);
        let layout = e.current_layout();
        let hit = timeline::hit_test(&layout, &e.project, x, y).unwrap();
        assert_eq!(hit.clip, expected);
        assert_eq!(
            timeline::drop_placement(&layout, &e.project, x, y, e.position, false)
                .unwrap()
                .0,
            hit.track
        );
    });
}
