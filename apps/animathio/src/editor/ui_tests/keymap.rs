//! Timeline shortcuts, editing tools and context menu bindings.

use crate::timeline;
use gpui::{AsKeystroke, MouseUpEvent};

use super::fixtures::{MS, PAIR_FIRST, PAIR_SECOND, sample, voice_pair};
use super::helpers::{ruler_point, seek};
use super::{BTreeMap, ClipId, TestAppContext, Time, Tool, Ui, actions};
use crate::editing;
use crate::editor::actions::{Redo, Undo};
use crate::editor::{TimelineGesture, TrimDrag};
use crate::timeline::TrimEdge;

#[gpui::test]
fn clip_context_menu_renders_with_shortcuts(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, window, cx| {
        let id = e.project.tracks()[0].clips().values().next().unwrap().id;
        e.context_menu = Some(super::super::ContextMenu::Clip { id });
        e.context_menu_pos = Some(gpui::point(gpui::px(100.), gpui::px(100.)));
        assert!(e.context_menu_dropdown(window, cx).is_some());
        assert!(
            gpui_component::kbd::Kbd::binding_for_action(
                &actions::SplitClip,
                Some("Timeline"),
                window
            )
            .is_some()
        );
        // The hints match the Premiere keymap (#240): Ctrl/Cmd+K split, Q/W ripple
        // trims, M marker, V/C/B tools.
        let hint = |action: &dyn gpui::Action| -> (String, bool) {
            let binding = window
                .highest_precedence_binding_for_action_in_context(
                    action,
                    gpui::KeyContext::parse("Timeline").expect("context"),
                )
                .expect("binding exists");
            let stroke = binding.keystrokes()[0].as_keystroke();
            (stroke.key.clone(), stroke.modifiers.secondary(cx))
        };
        assert_eq!(hint(&actions::SplitClip), ("k".into(), true));
        assert_eq!(hint(&actions::RippleTrimPrevious), ("q".into(), false));
        assert_eq!(hint(&actions::RippleTrimNext), ("w".into(), false));
        assert_eq!(hint(&actions::AddMarker), ("m".into(), false));
        assert_eq!(hint(&actions::SelectTool), ("v".into(), false));
        assert_eq!(hint(&actions::BladeTool), ("c".into(), false));
        assert_eq!(hint(&actions::RippleTool), ("b".into(), false));
    });
}

#[gpui::test]
fn premiere_keymap_tools_split_and_ripple_trims(cx: &mut TestAppContext) {
    let (project, assets) = voice_pair();
    let mut ui = Ui::open(cx, project, assets);
    // Focus the timeline by clicking its ruler, like a real editor session.
    ui.click(ruler_point(&ui, Time(0)), gpui::Modifiers::default());

    // Tool keys: V selection, C razor, B ripple edit.
    ui.press("c");
    assert_eq!(ui.read(|e| e.tool), Tool::Blade);
    ui.press("b");
    assert_eq!(ui.read(|e| e.tool), Tool::Ripple);
    ui.press("v");
    assert_eq!(ui.read(|e| e.tool), Tool::Select);

    // Ctrl/Cmd+K splits the clip under the playhead; one undo restores it.
    let before = ui.read(|e| e.project.clone());
    seek(&mut ui, 1000);
    ui.press("secondary-k");
    assert_eq!(ui.read(|e| e.project.tracks()[0].clips().len()), 3);
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);

    // Q ripple-trims the previous edit to the playhead: the first clip's head,
    // the tail shifts left, the source window follows.
    ui.press("q");
    ui.read(|e| {
        let clips = e.project.tracks()[0].clips();
        let head = clips.get(&PAIR_FIRST).expect("first clip");
        assert_eq!(head.range.start, Time(0));
        assert_eq!(head.range.end, Time(1000 * MS));
        assert_eq!(head.source_bounds(), (Time(1000 * MS), Time(2000 * MS)));
        let tail = clips.get(&PAIR_SECOND).expect("second clip");
        assert_eq!(tail.range.start, Time(1000 * MS));
        assert_eq!(tail.range.end, Time(3000 * MS));
    });
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);

    // W ripple-trims the next edit: the first clip's tail, source kept from zero.
    ui.press("w");
    ui.read(|e| {
        let clips = e.project.tracks()[0].clips();
        let head = clips.get(&PAIR_FIRST).expect("first clip");
        assert_eq!(head.range.end, Time(1000 * MS));
        assert_eq!(head.source_bounds(), (Time(0), Time(1000 * MS)));
        let tail = clips.get(&PAIR_SECOND).expect("second clip");
        assert_eq!(tail.range.start, Time(1000 * MS));
        assert_eq!(tail.range.end, Time(3000 * MS));
    });
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

fn additional_tool_scenario(cx: &mut TestAppContext, key: &str, tool: Tool) {
    let before = editing::tool_tests::fixture();
    let mut ui = Ui::open(cx, before.clone(), BTreeMap::new());
    ui.click(ruler_point(&ui, Time(0)), gpui::Modifiers::default());
    ui.press(key);
    assert_eq!(ui.read(|e| e.tool), tool);
    ui.update(|e, window, cx| {
        let c = e.find_clip(ClipId(2)).unwrap().clone();
        let delta = if tool == Tool::RateStretch { -100 } else { 100 };
        e.gesture = Some(TimelineGesture::Trim(TrimDrag {
            tool,
            pointer_origin: c.range.end,
            clip_id: c.id,
            edge: TrimEdge::Right,
            origin_start: c.range.start,
            origin_end: c.range.end,
            origin_source_offset: c.source_offset,
            current: Time(c.range.end.0 + delta),
            ripple: false,
        }));
        assert_ne!(e.edit_tool_feedback(), "");
        e.on_timeline_mouse_up(&MouseUpEvent::default(), window, cx);
    });
    assert_ne!(ui.read(|e| e.project.clone()), before);
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

#[gpui::test]
fn rate_stretch_tool(cx: &mut TestAppContext) {
    additional_tool_scenario(cx, "r", Tool::RateStretch);
}
#[gpui::test]
fn slip_tool(cx: &mut TestAppContext) {
    additional_tool_scenario(cx, "y", Tool::Slip);
}
#[gpui::test]
fn slide_tool(cx: &mut TestAppContext) {
    additional_tool_scenario(cx, "u", Tool::Slide);
}
#[gpui::test]
fn rolling_tool(cx: &mut TestAppContext) {
    additional_tool_scenario(cx, "n", Tool::Rolling);
}

#[gpui::test]
fn rate_stretch_at_the_top_corner_does_not_drag_a_fade(cx: &mut TestAppContext) {
    let (project, assets) = voice_pair();
    let before = project.clone();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|editor, _, cx| editor.select_only(PAIR_FIRST, cx));
    ui.click(ruler_point(&ui, Time(0)), gpui::Modifiers::default());
    ui.press("r");
    let corner = ui.read(|editor| {
        let layout = editor.current_layout();
        let end = editor.find_clip(PAIR_FIRST).unwrap().range.end;
        gpui::point(
            editor.timeline_bounds.origin.x + gpui::px(timeline::time_to_x(&layout, end) - 3.),
            editor.timeline_bounds.origin.y
                + gpui::px(layout.ruler_height + 3. + super::super::FADE_HANDLE_INSET),
        )
    });
    ui.cx
        .simulate_mouse_down(corner, gpui::MouseButton::Left, gpui::Modifiers::default());
    ui.settle();
    assert!(ui.read(|editor| matches!(
        editor.gesture,
        Some(TimelineGesture::Trim(TrimDrag {
            tool: Tool::RateStretch,
            ..
        }))
    )));
    let destination = corner - gpui::point(gpui::px(30.), gpui::px(0.));
    ui.cx.simulate_mouse_move(
        destination,
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    ui.cx.simulate_mouse_up(
        destination,
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    ui.settle();
    ui.read(|editor| {
        let clip = editor.find_clip(PAIR_FIRST).unwrap();
        assert!(clip.time_map.speed.percent() > 100.);
        assert!(clip.animations.is_empty(), "no accidental fade keyframes");
    });
    ui.dispatch(Undo);
    assert_eq!(ui.read(|editor| editor.project.clone()), before);
    ui.dispatch(Redo);
    assert!(
        ui.read(|editor| editor
            .find_clip(PAIR_FIRST)
            .unwrap()
            .time_map
            .speed
            .percent())
            > 100.
    );
}
