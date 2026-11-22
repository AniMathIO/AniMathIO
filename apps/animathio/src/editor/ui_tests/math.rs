//! Math objects, the Math dialog and Write On.

use crate::timeline;
use animathio_control::{MathObjectDraft, MathObjectKind, math_object_range};
use gpui::AppContext;

use super::fixtures::{MS, sample};
use super::helpers::{clip_count, dialog_ready, edit_dialog_field};
use super::{BTreeMap, ClipId, Command, Project, TestAppContext, Time, Track, Ui, require_plugins};
use crate::editor::actions::{Redo, Undo};
use crate::editor::math::MathEditor;
use crate::editor::support::project_size;

#[gpui::test]
fn latex_on_occupied_track_creates_a_layer(cx: &mut TestAppContext) {
    if !require_plugins("latex_on_occupied_track_creates_a_layer") {
        return;
    }
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    let count = ui.read(clip_count);
    // Fill every layer at the playhead before adding another element.
    ui.update(|e, _, cx| e.add_plugin_clip_by_id("org.animathio.latex", cx));
    assert_eq!(ui.read(clip_count), count + 1);
    let layers = ui.read(|e| e.project.tracks().len());
    ui.update(|e, _, cx| e.add_plugin_clip_by_id("org.animathio.latex", cx));
    assert_eq!(ui.read(|e| e.project.tracks().len()), layers + 1);
    ui.read(|e| {
        assert!(
            timeline::display_tracks(&e.project)[0]
                .clips()
                .contains_key(&e.selected.unwrap())
        );
    });
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.tracks().len()), layers);
}

#[gpui::test]
fn math_dialog_adds_latex_onto_an_occupied_track_as_a_new_layer(cx: &mut TestAppContext) {
    if !require_plugins("math_dialog_adds_latex_onto_an_occupied_track_as_a_new_layer") {
        return;
    }
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    let before = ui.read(|e| e.project.clone());
    let tracks = ui.read(|e| e.project.tracks().len());

    // The Add math object dialog panel, exactly as `Editor::open_math` builds it.
    let panel = ui.update(|e, window, cx| {
        let draft = MathObjectDraft::new(MathObjectKind::Latex);
        let range = math_object_range(e.position).expect("range");
        let editor = cx.entity().downgrade();
        let session = e.project_session;
        let size = project_size(&e.project);
        cx.new(|cx| MathEditor::new(draft, editor, session, range, None, false, size, window, cx))
    });
    // Let the debounced live preview finish so Apply is enabled.
    ui.advance_clock(std::time::Duration::from_millis(400));
    ui.settle();

    ui.cx
        .update(|window, cx| panel.update(cx, |panel, cx| panel.apply(window, cx)));
    ui.settle();
    let added = ui.read(|e| e.selected.expect("added clip selected"));
    ui.read(|e| {
        // The playhead's track is occupied, so the object lands on a new layer.
        assert_eq!(e.project.tracks().len(), tracks + 1);
        let clip = e.find_clip(added).expect("added clip");
        assert!(matches!(
            &clip.kind,
            animathio_core::ClipKind::Plugin { plugin_id, .. } if plugin_id == "org.animathio.latex"
        ));
    });
    // One undo removes object and layer.
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);

    // Editing through the dialog changes only the object's parameters, in place.
    ui.dispatch(Redo);
    let added_state = ui.read(|e| e.project.clone());
    let panel = ui.update(|e, window, cx| {
        let mut draft = MathObjectDraft::new(MathObjectKind::Latex);
        draft.fields.insert("source".into(), "x^2".into());
        let range = e.find_clip(added).expect("clip").range;
        let editor = cx.entity().downgrade();
        let session = e.project_session;
        let size = project_size(&e.project);
        cx.new(|cx| {
            MathEditor::new(
                draft,
                editor,
                session,
                range,
                Some(added),
                false,
                size,
                window,
                cx,
            )
        })
    });
    ui.advance_clock(std::time::Duration::from_millis(400));
    ui.settle();
    ui.cx
        .update(|window, cx| panel.update(cx, |panel, cx| panel.apply(window, cx)));
    ui.settle();
    ui.read(|e| {
        assert_eq!(e.project.tracks().len(), tracks + 1, "edit adds no layer");
        let clip = e.find_clip(added).expect("edited clip");
        let animathio_core::ParamValue::String(source) = clip.params.get("source").unwrap() else {
            panic!("source param");
        };
        assert_eq!(source, "x^2");
    });
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), added_state);
}

#[gpui::test]
fn write_on_dialog_validates_duration_before_enter_applies(cx: &mut TestAppContext) {
    let mut project = Project::default();
    Command::AddTrack {
        track: Track::new(animathio_core::TrackId(1), "Math"),
        index: 0,
    }
    .apply(&mut project)
    .unwrap();
    Command::AddClip {
        track: animathio_core::TrackId(1),
        clip: animathio_core::Clip::new(
            ClipId(1),
            animathio_core::ClipKind::Plugin {
                plugin_id: "org.animathio.latex".into(),
                params: serde_json::json!({"source":"x"}),
            },
            animathio_core::TimeRange::new(Time(0), Time(2000 * MS)).unwrap(),
        ),
    }
    .apply(&mut project)
    .unwrap();
    let before = project.clone();
    let mut ui = Ui::open(cx, project, BTreeMap::new());
    ui.update(|e, window, cx| {
        e.select_only(ClipId(1), cx);
        e.open_write_on(window, cx);
    });
    dialog_ready(&mut ui);
    edit_dialog_field(&mut ui, "write-duration", "0");
    ui.press("enter");
    assert!(ui.cx.debug_bounds("write-duration").is_some());
    assert_eq!(ui.read(|e| e.project.clone()), before);
    edit_dialog_field(&mut ui, "write-duration", "1");
    ui.press("enter");
    dialog_ready(&mut ui);
    assert!(ui.cx.debug_bounds("write-duration").is_none());
    assert!(!ui.read(|e| e.find_clip(ClipId(1)).unwrap().animations.is_empty()));
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

#[gpui::test]
fn math_dialog_enter_submits_from_the_multiline_source_and_escape_cancels(cx: &mut TestAppContext) {
    if !require_plugins("math_dialog_enter_submits_from_the_multiline_source_and_escape_cancels") {
        return;
    }
    let (project, assets) = sample();
    let before = project.clone();
    let mut ui = Ui::open(cx, project, assets);
    let count = ui.read(clip_count);
    ui.update(|e, window, cx| e.open_math(MathObjectKind::Latex, None, window, cx));
    dialog_ready(&mut ui);
    ui.press("enter");
    dialog_ready(&mut ui);
    assert_eq!(ui.read(clip_count), count + 1);
    assert!(!ui.cx.update(|window, cx| {
        use gpui_component::WindowExt;
        window.has_active_dialog(cx)
    }));
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
    ui.update(|e, window, cx| e.open_math(MathObjectKind::Latex, None, window, cx));
    dialog_ready(&mut ui);
    ui.press("escape");
    dialog_ready(&mut ui);
    assert!(!ui.cx.update(|window, cx| {
        use gpui_component::WindowExt;
        window.has_active_dialog(cx)
    }));
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

/// The Add math object dialog at `width` x `height`: every control is laid out inside the dialog
/// form's visible area (not below a scroll) or, for the palette, inside the window.
fn math_dialog_controls(
    cx: &mut TestAppContext,
    width: f32,
    height: f32,
) -> Vec<(&'static str, gpui::Bounds<gpui::Pixels>)> {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.cx
        .simulate_resize(gpui::size(gpui::px(width), gpui::px(height)));
    ui.settle();
    ui.update(|e, window, cx| e.open_math(MathObjectKind::Latex, None, window, cx));
    ui.settle();
    [
        "math-form",
        "math-colour-row",
        "math-write-on",
        "math-palette",
    ]
    .into_iter()
    .map(|selector| {
        (
            selector,
            ui.cx
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} is rendered at {width}x{height}")),
        )
    })
    .collect()
}

#[gpui::test]
fn math_dialog_shows_colour_write_on_and_templates_at_720p(cx: &mut TestAppContext) {
    let controls = math_dialog_controls(cx, 1280., 720.);
    let bounds = |name: &str| controls.iter().find(|(n, _)| *n == name).unwrap().1;
    let form = bounds("math-form");
    // Wide window: the palette sits beside the form, fully inside the window.
    let palette = bounds("math-palette");
    assert!(
        palette.left() >= form.right() - gpui::px(1.),
        "{palette:?} {form:?}"
    );
    assert!(palette.bottom() <= gpui::px(720.));
    assert!(palette.right() <= gpui::px(1280.));
    // Colour and Write on need no scrolling inside the form.
    for name in ["math-colour-row", "math-write-on"] {
        let control = bounds(name);
        assert!(
            control.top() >= form.top() && control.bottom() <= form.bottom() + gpui::px(1.),
            "{name}: {control:?} in {form:?}"
        );
        assert!(control.bottom() <= gpui::px(720.));
    }
}

#[gpui::test]
fn math_dialog_keeps_colour_and_write_on_above_the_palette_in_a_narrow_window(
    cx: &mut TestAppContext,
) {
    let controls = math_dialog_controls(cx, 900., 720.);
    let bounds = |name: &str| controls.iter().find(|(n, _)| *n == name).unwrap().1;
    let form = bounds("math-form");
    for name in ["math-colour-row", "math-write-on"] {
        let control = bounds(name);
        assert!(
            control.bottom() <= form.bottom() + gpui::px(1.),
            "{name}: {control:?} in {form:?}"
        );
        // Before the palette, so the palette is the part that scrolls.
        assert!(control.bottom() <= bounds("math-palette").top());
    }
}

#[test]
fn plugin_errors_lose_their_transport_prefixes() {
    use crate::editor::math::plain_plugin_error;
    assert_eq!(
        plain_plugin_error(
            "plugin produced an invalid scene: service-failed: invalid LaTeX: unbalanced opening brace"
        ),
        "Invalid LaTeX: unbalanced opening brace"
    );
    assert_eq!(plain_plugin_error("boom"), "Boom");
    assert_eq!(plain_plugin_error(""), "");
}
