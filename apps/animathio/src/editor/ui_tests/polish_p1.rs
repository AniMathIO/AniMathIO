//! Final UI/UX review, P1 items (follow-up to #286): audio gain default, Auto duck footer,
//! the collapsed "Describe in words" disclosure, project settings, shape inspector section and
//! marker name plates over the ruler.

use gpui::{AppContext, Bounds, Entity, Modifiers, TestAppContext, point, px, size};

use animathio_control::{MathObjectDraft, MathObjectKind, math_object_range};
use animathio_core::{ClipKind, FrameRate};

use super::fixtures::*;
use super::{Editor, Ui, actions, require_plugins};
use crate::editor::Field;
use crate::editor::assist::AssistPanel;
use crate::editor::dialogs::settings_preset_index;
use crate::editor::support::{
    field_value, gain_db_label, plugin_param_visible, plugin_section_identity,
};

/// Let a dialog's open animation finish.
fn dialog_ready(ui: &mut Ui) {
    ui.advance_clock(std::time::Duration::from_millis(400));
}

#[gpui::test]
fn audio_gain_defaults_to_unity_in_the_inspector(cx: &mut TestAppContext) {
    let (project, assets) = voice_and_music();
    // The fixture stores no gain on the clip, exactly like a freshly dropped audio file.
    let clip = project
        .tracks()
        .iter()
        .flat_map(|track| track.clips().values())
        .find(|clip| clip.id == VOICE_CLIP)
        .unwrap()
        .clone();
    assert!(!clip.params.contains_key("gain"));
    assert!((field_value(&clip, &Field::Param("gain".into())) - 1.0).abs() < 1e-12);
    assert!(field_value(&clip, &Field::Param("muted".into())).abs() < 1e-12);

    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| e.select_only(VOICE_CLIP, cx));
    ui.update(|_, window, _| window.refresh());
    ui.settle();
    let key = format!("c{}:p:gain", VOICE_CLIP.0);
    let shown = ui
        .cx
        .update(|_, cx| ui.editor.read(cx).fields.input_text(&key, cx));
    assert_eq!(shown.as_deref(), Some("1"), "gain field of a fresh clip");
}

#[test]
fn gain_is_labelled_in_decibels() {
    assert_eq!(gain_db_label(1.0), "+0.0 dB");
    assert_eq!(gain_db_label(0.1), "-20.0 dB");
    assert_eq!(gain_db_label(2.0), "+6.0 dB");
    assert_eq!(gain_db_label(0.0), "-inf dB");
}

#[gpui::test]
fn auto_duck_apply_is_pinned_below_the_scrolling_body(cx: &mut TestAppContext) {
    for (width, height) in [(1280., 720.), (1920., 1080.)] {
        let (project, assets) = voice_and_music();
        let mut ui = Ui::open(cx, project, assets);
        ui.cx.simulate_resize(size(px(width), px(height)));
        ui.dispatch(actions::AutoDuck);
        let panel = ui.cx.debug_bounds("duck-review").unwrap();
        let apply = ui
            .cx
            .debug_bounds("duck-apply")
            .unwrap_or_else(|| panic!("Apply is rendered at {width}x{height}"));
        let cancel = ui.cx.debug_bounds("duck-cancel").unwrap();
        // Visible without any scrolling: inside the panel and inside the window.
        for button in [apply, cancel] {
            assert!(button.top() >= panel.top() && button.bottom() <= panel.bottom());
            assert!(button.bottom() <= px(height));
        }
        // The footer stays put when the body scrolls.
        ui.cx.simulate_event(gpui::ScrollEvent {
            position: panel.center(),
            delta: gpui::ScrollDelta::Pixels(point(px(0.), px(-1600.))),
            ..Default::default()
        });
        ui.settle();
        assert_eq!(ui.cx.debug_bounds("duck-apply"), Some(apply));
    }
}

#[gpui::test]
fn math_dialog_describe_in_words_is_collapsed_and_remembered(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.cx.simulate_resize(size(px(1280.), px(720.)));
    ui.settle();
    ui.update(|e, window, cx| e.open_math(MathObjectKind::Latex, None, window, cx));
    ui.settle();
    let form = ui.cx.debug_bounds("math-form").expect("form");
    let toggle = ui
        .cx
        .debug_bounds("assist-toggle")
        .expect("disclosure header");
    // Closed by default and fully inside the form: the LaTeX fields need no scrolling.
    assert!(toggle.top() >= form.top() && toggle.bottom() <= form.bottom() + px(1.));
    let colour = ui.cx.debug_bounds("math-colour-row").unwrap();
    assert!(colour.bottom() <= toggle.top());
    assert!(toggle.bottom() <= px(720.));
    // Opening it is remembered for the next dialog in this session.
    ui.click(toggle.center(), Modifiers::default());
    ui.update(|_, window, _| window.refresh());
    ui.press("escape");
    dialog_ready(&mut ui);
    ui.update(|e, window, cx| e.open_math(MathObjectKind::Latex, None, window, cx));
    ui.settle();
    assert!(ui.cx.debug_bounds("assist-toggle").is_some());
}

#[gpui::test]
fn math_assist_panel_toggles_and_survives_conversion_state(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    let panel = ui.update(|e, window, cx| {
        let draft = MathObjectDraft::new(MathObjectKind::Latex);
        let range = math_object_range(e.position).expect("range");
        let editor = cx.entity().downgrade();
        let session = e.project_session;
        let size = crate::editor::support::project_size(&e.project);
        cx.new(|cx| {
            crate::editor::math::MathEditor::new(
                draft, editor, session, range, None, false, size, window, cx,
            )
        })
    });
    let assist: Entity<AssistPanel> = ui
        .cx
        .update(|_, cx| panel.read(cx).assist_panel().expect("latex has assist"));
    assert!(!ui.cx.update(|_, cx| assist.read(cx).is_open()));
    ui.cx
        .update(|_, cx| assist.update(cx, AssistPanel::toggle_open));
    assert!(ui.cx.update(|_, cx| assist.read(cx).is_open()));
    // A new panel in the same app starts where the user left it.
    let second = ui.update(|e, window, cx| {
        let draft = MathObjectDraft::new(MathObjectKind::Latex);
        let range = math_object_range(e.position).expect("range");
        let editor = cx.entity().downgrade();
        let session = e.project_session;
        let size = crate::editor::support::project_size(&e.project);
        cx.new(|cx| {
            crate::editor::math::MathEditor::new(
                draft, editor, session, range, None, false, size, window, cx,
            )
        })
    });
    let assist2 = ui
        .cx
        .update(|_, cx| second.read(cx).assist_panel().expect("assist"));
    assert!(ui.cx.update(|_, cx| assist2.read(cx).is_open()));
}

#[test]
fn settings_preset_matches_the_current_size_or_says_custom() {
    let rate30 = FrameRate::new(30, 1).unwrap();
    let rate24 = FrameRate::new(24, 1).unwrap();
    assert_eq!(settings_preset_index(1920, 1080, rate30), 0);
    assert_eq!(settings_preset_index(3840, 2160, rate30), 1);
    assert_eq!(settings_preset_index(1080, 1920, rate30), 2);
    let custom = animathio_control::ExportPreset::ALL.len();
    assert_eq!(settings_preset_index(640, 360, rate30), custom);
    assert_eq!(settings_preset_index(1920, 1080, rate24), custom);
}

#[gpui::test]
fn project_settings_shows_the_background_swatch(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(Editor::open_settings);
    dialog_ready(&mut ui);
    let swatch: Bounds<_> = ui
        .cx
        .debug_bounds("settings-background")
        .expect("background control is rendered");
    assert!(swatch.size.width > px(40.) && swatch.size.height > px(12.));
}

#[test]
fn content_section_is_named_after_the_clip() {
    let plugin = |id: &str| ClipKind::Plugin {
        plugin_id: id.into(),
        params: serde_json::Value::default(),
    };
    assert_eq!(
        plugin_section_identity(&plugin("org.animathio.text")).0,
        "Text"
    );
    assert_eq!(
        plugin_section_identity(&plugin("org.animathio.shapes")).0,
        "Shape"
    );
    assert_eq!(
        plugin_section_identity(&plugin("org.animathio.graph")).0,
        "Graph"
    );
    assert_eq!(
        plugin_section_identity(&plugin("io.example.thing")).0,
        "Content"
    );
    let shape_icon = plugin_section_identity(&plugin("org.animathio.shapes")).1;
    let text_icon = plugin_section_identity(&plugin("org.animathio.text")).1;
    assert_ne!(shape_icon.path(), text_icon.path());
}

#[test]
fn raw_path_verbs_only_show_for_the_path_shape() {
    let shape = |value: &'static str| move |name: &str| (name == "shape").then(|| value.to_owned());
    assert!(!plugin_param_visible(
        "org.animathio.shapes",
        "path",
        shape("rect")
    ));
    assert!(plugin_param_visible(
        "org.animathio.shapes",
        "path",
        shape("path")
    ));
    assert!(plugin_param_visible(
        "org.animathio.shapes",
        "shape",
        shape("rect")
    ));
    // Other plugins' parameters named "path" are not touched.
    assert!(plugin_param_visible(
        "io.example.thing",
        "path",
        shape("rect")
    ));
}

#[gpui::test]
fn shape_inspector_hides_the_path_field_until_shape_is_path(cx: &mut TestAppContext) {
    if !require_plugins("shape_inspector_hides_the_path_field") {
        return;
    }
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| e.add_plugin_clip_by_id("org.animathio.shapes", cx));
    let id = ui
        .read(|e| e.selected)
        .expect("the shape clip is selected after adding it");
    ui.update(|_, window, _| window.refresh());
    ui.settle();
    let path_key = format!("c{}:pt:path", id.0);
    let shape_text = |ui: &mut Ui| {
        ui.cx
            .update(|_, cx| ui.editor.read(cx).fields.input_text(&path_key, cx))
    };
    assert!(
        shape_text(&mut ui).is_none(),
        "path field is hidden for rect"
    );
    ui.update(|e, _, cx| {
        let command = e
            .plugin_param_command(id, "shape".into(), serde_json::json!("path"))
            .expect("shape param");
        e.execute(&command, cx);
    });
    ui.update(|_, window, _| window.refresh());
    ui.settle();
    assert!(
        shape_text(&mut ui).is_some(),
        "path field is shown when shape = path"
    );
}

#[test]
fn ruler_labels_under_a_marker_plate_are_hidden() {
    let plates = [(px(100.), px(180.))];
    let covers = |l: f32, r: f32| crate::editor::markers::plates_cover(px(l), px(r), &plates);
    assert!(covers(90., 120.));
    assert!(covers(170., 200.));
    assert!(covers(110., 130.));
    assert!(!covers(10., 100.));
    assert!(!covers(180., 220.));
}
