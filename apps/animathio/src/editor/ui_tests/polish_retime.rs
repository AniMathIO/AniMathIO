//! Speed-ramp editing polish (follow-up to #314): no key edits where no lane is drawn.

use animathio_core::{ClipSpeed, Command, SpeedPoint, Time};

use super::fixtures::*;
use super::helpers::{dialog_ready, edit_dialog_field};
use super::{ClipId, Editor, TestAppContext, Ui};
use crate::editor::TimelineGesture;
use crate::editor::actions::{Redo, Undo};
use crate::editor::retime;
use crate::timeline::{self, TimelineLayout};

/// A clip block under 24 px wide draws no speed lane, so an Alt-click on it adds no key.
#[gpui::test]
fn alt_click_adds_no_speed_key_on_a_clip_too_narrow_for_the_lane(cx: &mut gpui::TestAppContext) {
    let (project, assets) = voice_pair();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| {
        e.select_only(PAIR_FIRST, cx);
        e.try_execute(
            &Command::SetClipSpeed {
                id: PAIR_FIRST,
                speed: ClipSpeed::default(),
                reverse: false,
                ramps: vec![
                    SpeedPoint {
                        time: Time(0),
                        speed: ClipSpeed::default(),
                    },
                    SpeedPoint {
                        time: Time(500 * MS),
                        speed: ClipSpeed::new(2, 1).unwrap(),
                    },
                ],
                ripple: false,
            },
            cx,
        );
    });
    ui.settle();
    let keys = |ui: &Ui| ui.read(|e| e.find_clip(PAIR_FIRST).unwrap().time_map.ramps.len());
    assert_eq!(keys(&ui), 2);

    // A 40 px canvas for a 4 s project: the 2 s clip is 20 px wide.
    let narrow = ui.read(|e| {
        let wide = e.current_layout();
        TimelineLayout::zoomed(
            40.0,
            wide.ruler_height,
            wide.row_height,
            wide.duration_ticks,
            1.0,
            0.0,
        )
    });
    let y = narrow.ruler_height + narrow.row_height / 2.0;
    assert!(ui.update(|e, _, cx| !e.ramp_press(&narrow, 10.0, y, true, cx)));
    assert_eq!(keys(&ui), 2, "no key was added on the narrow block");
    assert!(ui.read(|e| e.hit_ramp_handle(&narrow, 0.5, y).is_none()));

    // The same Alt-click on a wide block (800 px canvas) still adds one.
    let wide = ui.read(|e| {
        let layout = e.current_layout();
        TimelineLayout::zoomed(
            800.0,
            layout.ruler_height,
            layout.row_height,
            layout.duration_ticks,
            1.0,
            0.0,
        )
    });
    let y = wide.ruler_height + wide.row_height / 2.0;
    let x = crate::timeline::time_to_x(&wide, Time(800 * MS));
    assert!(ui.update(|e, _, cx| e.ramp_press(&wide, x, y, true, cx)));
    assert_eq!(keys(&ui), 3);
}

#[gpui::test]
fn speed_duration_change_ripples_and_undoes(cx: &mut TestAppContext) {
    let (project, assets) = voice_pair();
    let mut ui = Ui::open(cx, project, assets);
    let before = ui.read(|e| e.project.clone());
    ui.update(|e, _, cx| e.select_only(PAIR_FIRST, cx));
    // The Speed/Duration dialog parses its percent field with `retime::percent`
    // and applies one `SetClipSpeed`; drive the same path here.
    let speed = retime::percent("200").expect("valid percent");
    assert_eq!(speed, ClipSpeed::new(2, 1).expect("2x"));
    ui.update(|e, _, cx| {
        // The inspector's Speed section renders for the selection.
        let _ = e.speed_rows(PAIR_FIRST, cx);
        assert!(e.try_execute(
            &Command::SetClipSpeed {
                id: PAIR_FIRST,
                speed,
                reverse: false,
                ramps: Vec::new(),
                ripple: true,
            },
            cx
        ));
    });
    ui.read(|e| {
        let clips = e.project.tracks()[0].clips();
        let head = clips.get(&PAIR_FIRST).expect("first clip");
        // 2x halves the timeline duration and keeps the source interval.
        assert_eq!(head.range.start, Time(0));
        assert_eq!(head.range.end, Time(1000 * MS));
        assert_eq!(head.source_bounds(), (Time(0), Time(2000 * MS)));
        assert_eq!(head.time_map.speed.percent(), 200.);
        // Ripple trailing clips moved the second clip up by the lost second.
        let tail = clips.get(&PAIR_SECOND).expect("second clip");
        assert_eq!(tail.range.start, Time(1000 * MS));
        assert_eq!(tail.range.end, Time(3000 * MS));
    });
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
    ui.dispatch(Redo);
    ui.read(|e| {
        assert_eq!(
            e.project.tracks()[0]
                .clips()
                .get(&PAIR_FIRST)
                .unwrap()
                .range
                .end,
            Time(1000 * MS)
        );
    });
}

#[gpui::test]
fn speed_and_hold_dialogs_retain_invalid_input_and_apply_enter(cx: &mut TestAppContext) {
    let (project, assets) = voice_pair();
    let before = project.clone();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, window, cx| {
        e.select_only(PAIR_FIRST, cx);
        e.open_speed_duration(window, cx);
    });
    dialog_ready(&mut ui);
    edit_dialog_field(&mut ui, "speed-percent", "0");
    ui.press("enter");
    assert_eq!(ui.read(|e| e.project.clone()), before);
    assert!(ui.cx.debug_bounds("speed-percent").is_some());
    edit_dialog_field(&mut ui, "speed-percent", "200");
    ui.press("enter");
    dialog_ready(&mut ui);
    assert!(ui.cx.debug_bounds("speed-percent").is_none());
    assert!(
        (ui.read(|e| e.find_clip(PAIR_FIRST).unwrap().time_map.speed.percent()) - 200.).abs()
            < f64::EPSILON
    );
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
    ui.update(|e, window, cx| {
        e.seek_to(Time(500 * MS), cx);
        e.open_frame_hold(window, cx);
    });
    dialog_ready(&mut ui);
    edit_dialog_field(&mut ui, "hold-frames", "0");
    ui.press("enter");
    assert!(ui.cx.debug_bounds("hold-frames").is_some());
    assert_eq!(ui.read(|e| e.project.clone()), before);
    edit_dialog_field(&mut ui, "hold-frames", "15");
    ui.press("enter");
    dialog_ready(&mut ui);
    assert!(ui.cx.debug_bounds("hold-frames").is_none());
    assert_ne!(ui.read(|e| e.project.clone()), before);
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

#[gpui::test]
fn maintain_pitch_inspector_handler_undoes(cx: &mut TestAppContext) {
    let (project, assets) = voice_pair();
    let mut ui = Ui::open(cx, project, assets);
    let before = ui.read(|e| e.project.clone());
    ui.update(|e, _, cx| {
        e.select_only(PAIR_FIRST, cx);
        let _ = e.speed_rows(PAIR_FIRST, cx);
        e.set_maintain_pitch(PAIR_FIRST, false, cx);
    });
    assert!(!ui.read(|e| e.project.tracks()[0].clips()[&PAIR_FIRST].maintain_pitch));
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
    ui.dispatch(Redo);
    assert!(!ui.read(|e| e.project.tracks()[0].clips()[&PAIR_FIRST].maintain_pitch));
}

/// Window position of speed key `index` of `clip`, or of its clip-local `time` on the
/// 100% line of the lane when `index` is `None`.
fn ramp_point(
    ui: &Ui,
    clip: ClipId,
    index: Option<usize>,
    time: Time,
) -> gpui::Point<gpui::Pixels> {
    ui.read(|e| {
        let layout = e.current_layout();
        let (row, c) = e.ramp_target().expect("selected clip");
        assert_eq!(c.id, clip);
        let lane = Editor::ramp_lane(&layout, row).expect("lane");
        // Keys sit on the ramp's own clock, which runs `phase` ticks ahead of the clip.
        let (local, speed) = index.map_or((time, ClipSpeed::default()), |i| {
            (
                Time(c.time_map.ramps[i].time.0 - c.time_map.phase.0),
                c.time_map.ramps[i].speed,
            )
        });
        gpui::point(
            e.timeline_bounds.origin.x
                + gpui::px(timeline::time_to_x(
                    &layout,
                    Time(c.range.start.0 + local.0),
                )),
            e.timeline_bounds.origin.y + gpui::px(lane.y(speed)),
        )
    })
}

#[gpui::test]
fn speed_ramp_handles_drag_add_and_remove_as_single_undo_steps(cx: &mut TestAppContext) {
    use animathio_core::SpeedPoint;
    let (project, assets) = voice_pair();
    let mut ui = Ui::open(cx, project, assets);
    // A two-key ramp from 100% to 200% on the first clip.
    ui.update(|e, _, cx| {
        e.select_only(PAIR_FIRST, cx);
        e.try_execute(
            &Command::SetClipSpeed {
                id: PAIR_FIRST,
                speed: ClipSpeed::default(),
                reverse: false,
                ramps: vec![
                    SpeedPoint {
                        time: Time(0),
                        speed: ClipSpeed::default(),
                    },
                    SpeedPoint {
                        time: Time(500 * MS),
                        speed: ClipSpeed::new(2, 1).unwrap(),
                    },
                ],
                ripple: false,
            },
            cx,
        );
    });
    ui.settle();
    let ramps = |ui: &Ui| ui.read(|e| e.find_clip(PAIR_FIRST).unwrap().time_map.ramps.clone());
    let original = ramps(&ui);
    assert_eq!(original.len(), 2);
    let before = ui.read(|e| e.project.clone());
    let none = gpui::Modifiers::default();

    // Dragging the second key up and left is one gesture and one undo step.
    let grab = ramp_point(&ui, PAIR_FIRST, Some(1), Time(0));
    ui.cx
        .simulate_mouse_down(grab, gpui::MouseButton::Left, none);
    ui.settle();
    assert!(ui.read(|e| matches!(e.gesture, Some(TimelineGesture::Ramp(_)))));
    for step in 1..=4 {
        let moved = grab + gpui::point(gpui::px(-6. * step as f32), gpui::px(-4. * step as f32));
        ui.cx
            .simulate_mouse_move(moved, gpui::MouseButton::Left, none);
        // Nothing is committed while the pointer is still down.
        assert_eq!(ui.read(|e| e.project.clone()), before);
    }
    assert!(ui.read(Editor::edit_tool_feedback).starts_with("Speed key"));
    let end = grab + gpui::point(gpui::px(-24.), gpui::px(-16.));
    ui.cx.simulate_mouse_up(end, gpui::MouseButton::Left, none);
    ui.settle();
    let dragged = ramps(&ui);
    assert_eq!(dragged.len(), 2);
    assert!(dragged[1].time < original[1].time, "moved earlier");
    assert!(dragged[1].speed.percent() > 200., "moved faster");
    assert_eq!(dragged[0], original[0]);
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
    ui.dispatch(Redo);
    assert_eq!(ramps(&ui), dragged);
    ui.dispatch(Undo);

    // A click that does not move the key changes nothing.
    let grab = ramp_point(&ui, PAIR_FIRST, Some(1), Time(0));
    ui.cx
        .simulate_mouse_down(grab, gpui::MouseButton::Left, none);
    ui.cx.simulate_mouse_up(grab, gpui::MouseButton::Left, none);
    ui.settle();
    assert_eq!(ui.read(|e| e.project.clone()), before);

    // Alt-click adds a key on the curve without changing it; Alt-click a key removes it.
    let alt = gpui::Modifiers {
        alt: true,
        ..Default::default()
    };
    let at = ramp_point(&ui, PAIR_FIRST, None, Time(250 * MS));
    ui.click(at, alt);
    assert_eq!(ramps(&ui).len(), 3);
    // On the 100% -> 200% curve around the 250 ms the pointer landed on (pixel precision).
    assert!((140.0..160.0).contains(&ramps(&ui)[1].speed.percent()));
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
    ui.dispatch(Redo);
    let added = ramp_point(&ui, PAIR_FIRST, Some(1), Time(0));
    ui.click(added, alt);
    assert_eq!(ramps(&ui), original);
    ui.dispatch(Undo);
    assert_eq!(ramps(&ui).len(), 3);
}

/// Undo with the mouse held during a speed-key drag removes the keys the drag points at:
/// the gesture ends, nothing panics and the ramp stays what undo left.
#[gpui::test]
fn undo_during_a_speed_key_drag_ends_the_gesture_without_panicking(cx: &mut TestAppContext) {
    use animathio_core::SpeedPoint;
    let (project, assets) = voice_pair();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| {
        e.select_only(PAIR_FIRST, cx);
        e.try_execute(
            &Command::SetClipSpeed {
                id: PAIR_FIRST,
                speed: ClipSpeed::default(),
                reverse: false,
                ramps: vec![
                    SpeedPoint {
                        time: Time(0),
                        speed: ClipSpeed::default(),
                    },
                    SpeedPoint {
                        time: Time(500 * MS),
                        speed: ClipSpeed::new(2, 1).unwrap(),
                    },
                ],
                ripple: false,
            },
            cx,
        );
    });
    ui.settle();
    let ramps = |ui: &Ui| ui.read(|e| e.find_clip(PAIR_FIRST).unwrap().time_map.ramps.clone());
    assert_eq!(ramps(&ui).len(), 2);
    let none = gpui::Modifiers::default();
    let grab = ramp_point(&ui, PAIR_FIRST, Some(1), Time(0));
    ui.cx
        .simulate_mouse_down(grab, gpui::MouseButton::Left, none);
    ui.settle();
    assert!(ui.read(|e| matches!(e.gesture, Some(TimelineGesture::Ramp(_)))));

    // Ctrl+Z with the button still down: the ramp is gone and so is the gesture.
    ui.dispatch(Undo);
    assert_eq!(ramps(&ui).len(), 0);
    assert!(ui.read(|e| e.gesture.is_none()));
    let moved = grab + gpui::point(gpui::px(-12.), gpui::px(-8.));
    ui.cx
        .simulate_mouse_move(moved, gpui::MouseButton::Left, none);
    ui.cx
        .simulate_mouse_up(moved, gpui::MouseButton::Left, none);
    ui.settle();
    assert_eq!(ramps(&ui).len(), 0, "the stale drag committed nothing");

    // Redo brings the curve back untouched.
    ui.dispatch(Redo);
    assert_eq!(ramps(&ui).len(), 2);
}

/// The first clip of [`voice_pair`] selected, with speed keys 100% at 0, 200% at 600 ms
/// and 100% at 1200 ms.
fn ramp_ui(cx: &mut TestAppContext) -> Ui {
    use animathio_core::SpeedPoint;
    let (project, assets) = voice_pair();
    let mut ui = Ui::open(cx, project, assets);
    let key = |ms: i64, percent: u32| SpeedPoint {
        time: Time(ms * MS),
        speed: ClipSpeed::new(percent, 100).unwrap(),
    };
    ui.update(|e, _, cx| {
        e.select_only(PAIR_FIRST, cx);
        e.try_execute(
            &Command::SetClipSpeed {
                id: PAIR_FIRST,
                speed: ClipSpeed::default(),
                reverse: false,
                ramps: vec![key(0, 100), key(600, 200), key(1200, 100)],
                ripple: false,
            },
            cx,
        );
    });
    ui.settle();
    ui
}

fn first_clip(ui: &Ui) -> animathio_core::Clip {
    ui.read(|e| e.find_clip(PAIR_FIRST).unwrap().clone())
}

/// Speed of the clip's curve at clip-local `time`, in millionths.
fn curve_at(clip: &animathio_core::Clip, local: Time) -> u32 {
    crate::speed_ramp::speed_at(clip, local).millionths()
}

#[gpui::test]
fn nudging_a_key_of_a_head_trimmed_ramp_clip_keeps_the_rest_of_the_curve(cx: &mut TestAppContext) {
    let mut ui = ramp_ui(cx);
    // Cut the first 300 ms off the head: the keys keep their curve time, the clip's
    // clock now runs 300 ms ahead of its first tick.
    ui.update(|e, _, cx| {
        let clip = e.find_clip(PAIR_FIRST).unwrap().clone();
        let cut = Time(300 * MS);
        e.try_execute(
            &Command::TrimClip {
                id: PAIR_FIRST,
                range: animathio_core::TimeRange::new(cut, clip.range.end).unwrap(),
                // Retimed clips take their offset delta in timeline ticks.
                source_offset: Time(clip.source_offset.0 + cut.0),
            },
            cx,
        );
    });
    ui.settle();
    let trimmed = first_clip(&ui);
    assert_eq!(trimmed.time_map.phase, Time(300 * MS));
    let length = trimmed.range.end.0 - trimmed.range.start.0;
    let before: Vec<u32> = (0..=length)
        .step_by(5 * MS as usize)
        .map(|t| curve_at(&trimmed, Time(t)))
        .collect();
    // The handle for the 600 ms key is drawn at local 300 ms (600 - phase), not at 600 ms.
    let handle = ramp_point(&ui, PAIR_FIRST, Some(1), Time(0));
    let expected_x = ui.read(|e| {
        e.timeline_bounds.origin.x
            + gpui::px(timeline::time_to_x(
                &e.current_layout(),
                Time(trimmed.range.start.0 + 300 * MS),
            ))
    });
    assert!((handle.x - expected_x).abs() < gpui::px(0.5));
    assert_eq!(
        ui.read(|e| e.hit_ramp_handle(
            &e.current_layout(),
            f32::from(handle.x - e.timeline_bounds.origin.x),
            f32::from(handle.y - e.timeline_bounds.origin.y)
        )),
        Some((PAIR_FIRST, 1))
    );
    // Nudge it straight up (faster) without moving it sideways.
    let none = gpui::Modifiers::default();
    ui.cx
        .simulate_mouse_down(handle, gpui::MouseButton::Left, none);
    ui.settle();
    assert!(ui.read(|e| matches!(e.gesture, Some(TimelineGesture::Ramp(_)))));
    let up = handle + gpui::point(gpui::px(0.), gpui::px(-8.));
    ui.cx.simulate_mouse_move(up, gpui::MouseButton::Left, none);
    ui.cx.simulate_mouse_up(up, gpui::MouseButton::Left, none);
    ui.settle();
    let nudged = first_clip(&ui);
    // The clock is rebuilt from the clip's own start, and exactly one key changed: the
    // key is still at local 300 ms (its curve time 600 ms minus the phase), and the clip
    // plays the old ramp with only that key's speed different, everywhere along it.
    assert_eq!(nudged.time_map.phase, Time(0));
    let moved = nudged
        .time_map
        .ramps
        .iter()
        .find(|k| k.time == Time(300 * MS))
        .expect("the nudged key stays at its local time");
    assert!(moved.speed.percent() > 200.);
    let mut expected = trimmed.time_map.clone();
    expected.ramps[1].speed = moved.speed;
    let nudged_length = nudged.range.end.0 - nudged.range.start.0;
    for t in (0..=nudged_length).step_by(3 * MS as usize) {
        let want = expected.speed_at_curve(Time(t + 300 * MS)).millionths();
        let got = curve_at(&nudged, Time(t));
        assert!(want.abs_diff(got) <= 2, "tick {t}: {want} vs {got}");
    }
    let last = nudged.time_map.ramps.last().unwrap();
    assert_eq!(last.time, Time(900 * MS), "the 1200 ms key is local 900 ms");
    assert_eq!(last.speed, ClipSpeed::new(1, 1).unwrap());
    assert!(
        nudged.range.end.0 < trimmed.range.end.0,
        "a faster key shortens the clip, nothing else"
    );
    let _ = (length, before);
    ui.dispatch(Undo);
    assert_eq!(first_clip(&ui), trimmed);
}

#[gpui::test]
fn handles_on_a_split_ramp_clip_add_keys_on_the_curve_the_clip_plays(cx: &mut TestAppContext) {
    let mut ui = ramp_ui(cx);
    let right = ClipId(90);
    ui.update(|e, _, cx| {
        let at = Time(e.find_clip(PAIR_FIRST).unwrap().range.start.0 + 700 * MS);
        e.try_execute(
            &Command::SplitClip {
                id: PAIR_FIRST,
                right_id: right,
                at,
            },
            cx,
        );
        e.select_only(right, cx);
    });
    ui.settle();
    let tail = ui.read(|e| e.find_clip(right).unwrap().clone());
    assert_eq!(tail.time_map.phase, Time(700 * MS));
    assert_eq!(tail.time_map.ramps.len(), 3, "the split keeps every key");
    // Only the 1200 ms key (local 500 ms) is inside the right clip: one handle.
    let vertices = crate::speed_ramp::vertices(&tail);
    assert_eq!(
        vertices.iter().filter_map(|v| v.index).collect::<Vec<_>>(),
        [2]
    );
    assert_eq!(
        vertices[0].index, None,
        "the curve enters at an interpolated edge vertex"
    );
    assert_eq!(vertices[0].time, Time(0));
    let before = ui.read(|e| e.project.clone());
    // Alt-click at local 250 ms: the curve there is the one the clip plays (between the
    // 600 ms and 1200 ms keys of the ramp), and the new key leaves it unchanged.
    let layout = ui.read(Editor::current_layout);
    let (x, y) = ui.read(|e| {
        let (row, clip) = e.ramp_target().unwrap();
        let lane = Editor::ramp_lane(&layout, row).unwrap();
        (
            timeline::time_to_x(&layout, Time(clip.range.start.0 + 250 * MS)),
            lane.y(ClipSpeed::default()),
        )
    });
    let at = ui.read(|e| {
        gpui::point(
            e.timeline_bounds.origin.x + gpui::px(x),
            e.timeline_bounds.origin.y + gpui::px(y),
        )
    });
    let alt = gpui::Modifiers {
        alt: true,
        ..Default::default()
    };
    ui.click(at, alt);
    let added = ui.read(|e| e.find_clip(right).unwrap().clone());
    assert_eq!(added.time_map.phase, Time(0));
    let key = added
        .time_map
        .ramps
        .iter()
        .find(|k| (k.time.0 - 250 * MS).abs() <= 33 * MS)
        .expect("a key at the pointer");
    // The ramp at 700 ms + local time lies between 200% (600 ms) and 100% (1200 ms).
    let expected = 200.0 - 100.0 * ((700 * MS + key.time.0 - 600 * MS) as f64 / (600 * MS) as f64);
    assert!(
        (key.speed.percent() - expected).abs() < 0.2,
        "{} vs {expected}",
        key.speed.percent()
    );
    // The edge vertices became real keys at 0 and at the end of the clip's curve.
    assert_eq!(added.time_map.ramps[0].time, Time(0));
    // The clip's length (and so everything after it) did not change by more than rounding.
    // Speeds are rounded to the millionth: a microsecond or so over a second of clip.
    assert!((added.range.end.0 - tail.range.end.0).abs() <= 2_000);
    for t in (0..tail.range.end.0 - tail.range.start.0).step_by(7 * MS as usize) {
        let (a, b) = (curve_at(&tail, Time(t)), curve_at(&added, Time(t)));
        assert!(a.abs_diff(b) <= 2, "tick {t}: {a} vs {b}");
    }
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

#[gpui::test]
fn trim_edges_win_over_speed_handles_and_short_lanes_show_no_keys(cx: &mut TestAppContext) {
    let mut ui = ramp_ui(cx);
    // The first key sits exactly on the clip's left edge: pressing it trims, it does not
    // start a key drag.
    let edge = ramp_point(&ui, PAIR_FIRST, Some(0), Time(0));
    ui.cx
        .simulate_mouse_down(edge, gpui::MouseButton::Left, gpui::Modifiers::default());
    ui.settle();
    assert!(ui.read(|e| matches!(e.gesture, Some(TimelineGesture::Trim(_)))));
    ui.cx
        .simulate_mouse_up(edge, gpui::MouseButton::Left, gpui::Modifiers::default());
    ui.settle();
    // A pointer outside the clip block never grabs a key, even within the grab radius.
    ui.read(|e| {
        let layout = e.current_layout();
        let (row, clip) = e.ramp_target().unwrap();
        let lane = Editor::ramp_lane(&layout, row).unwrap();
        let left = timeline::time_to_x(&layout, clip.range.start) - 3.0;
        assert_eq!(
            e.hit_ramp_handle(&layout, left, lane.y(ClipSpeed::default())),
            None
        );
    });
    // A row too short for the lane has no handles and Alt-click adds no key.
    let before = ui.read(|e| e.project.clone());
    let added = ui.update(|e, _, cx| {
        let mut layout = e.current_layout();
        layout.row_height = 30.0;
        let (row, _) = e.ramp_target().unwrap();
        assert!(Editor::ramp_lane(&layout, row).is_none());
        let (x, y) = (
            timeline::time_to_x(&layout, Time(300 * MS)),
            layout.ruler_height + 15.0,
        );
        e.ramp_press(&layout, x, y, true, cx)
    });
    assert!(!added);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

#[gpui::test]
fn add_speed_key_at_playhead_inserts_a_key_on_the_curve_and_undoes(cx: &mut TestAppContext) {
    let (project, assets) = voice_pair();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| {
        e.select_only(PAIR_FIRST, cx);
        e.seek_to(Time(750 * MS), cx);
    });
    let before = ui.read(|e| e.project.clone());
    // A constant clip: the key takes the clip's speed, so the curve is flat and the clip
    // keeps its length.
    ui.update(|e, _, cx| e.add_speed_key_at_playhead(PAIR_FIRST, cx));
    let clip = first_clip(&ui);
    assert_eq!(clip.time_map.ramps.len(), 1);
    assert_eq!(clip.time_map.ramps[0].time, Time(750 * MS));
    assert_eq!(clip.time_map.ramps[0].speed, ClipSpeed::default());
    assert_eq!(
        clip.range,
        ui.read(|e| e.find_clip(PAIR_SECOND).map(|_| clip.range))
            .unwrap()
    );
    // A second key on the same spot is refused with a message.
    let refused = ui.update(|e, _, cx| {
        e.add_speed_key_at_playhead(PAIR_FIRST, cx);
        e.error.clone()
    });
    assert_eq!(first_clip(&ui).time_map.ramps.len(), 1);
    assert!(refused.is_some());
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}
