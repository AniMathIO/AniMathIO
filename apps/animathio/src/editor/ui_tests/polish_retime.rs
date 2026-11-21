//! Speed-ramp editing polish (follow-up to #314): no key edits where no lane is drawn.

use animathio_core::{ClipSpeed, Command, SpeedPoint, Time};

use super::Ui;
use super::fixtures::*;
use crate::timeline::TimelineLayout;

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
