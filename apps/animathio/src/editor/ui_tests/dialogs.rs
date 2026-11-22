//! Project settings, Preferences and dialog validation.

use super::fixtures::{sample, voice_and_music};
use super::helpers::{dialog_ready, edit_dialog_field, last_error};
use super::{ClipId, Editor, TestAppContext, Ui};
use crate::editor::actions::Undo;

#[gpui::test]
fn preferences_enter_saves_and_invalid_input_keeps_the_dialog_open(cx: &mut TestAppContext) {
    let _preferences = super::preferences_lock();
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    let original = ui.read(|editor| editor.preferences.clone());
    let edit_autosave = |ui: &mut Ui, text: &str| {
        let bounds = ui
            .cx
            .debug_bounds("preferences-autosave")
            .unwrap_or_else(|| panic!("Autosave input missing before entering {text}"));
        ui.click(bounds.center(), gpui::Modifiers::default());
        ui.press("secondary-a");
        ui.cx.simulate_input(text);
    };
    ui.update(Editor::open_preferences);
    ui.advance_clock(std::time::Duration::from_millis(400));
    edit_autosave(&mut ui, "17");
    ui.press("enter");
    ui.advance_clock(std::time::Duration::from_millis(400));
    assert_eq!(ui.read(|editor| editor.preferences.autosave_seconds), 17);
    assert_eq!(crate::preferences::Preferences::load().autosave_seconds, 17);
    assert!(ui.cx.debug_bounds("preferences-autosave").is_none());

    ui.update(Editor::open_preferences);
    ui.advance_clock(std::time::Duration::from_millis(400));
    edit_autosave(&mut ui, "-1");
    ui.press("enter");
    assert_eq!(ui.read(|editor| editor.preferences.autosave_seconds), 17);
    assert!(ui.cx.debug_bounds("preferences-autosave").is_some());
    edit_autosave(&mut ui, "23");
    let save = ui.cx.debug_bounds("dialog-apply").unwrap();
    ui.click(save.center(), gpui::Modifiers::default());
    assert_eq!(ui.read(|editor| editor.preferences.autosave_seconds), 23);
    assert_eq!(crate::preferences::Preferences::load().autosave_seconds, 23);
    original.save().unwrap();
}

#[gpui::test]
fn project_settings_dialog_enter_validates_applies_and_escape_cancels(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let before = project.clone();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(Editor::open_settings);
    dialog_ready(&mut ui);
    edit_dialog_field(&mut ui, "project-width", "0");
    ui.press("enter");
    assert_eq!(ui.read(|e| e.project.clone()), before);
    assert!(
        ui.cx.update(|w, cx| {
            use gpui_component::WindowExt;
            w.has_active_dialog(cx)
        }),
        "project settings unexpectedly closed"
    );
    dialog_ready(&mut ui);
    assert!(ui.cx.debug_bounds("project-width").is_some());
    assert!(ui.cx.debug_bounds("dialog-validation-error").is_some());
    edit_dialog_field(&mut ui, "project-width", "1280");
    ui.press("enter");
    dialog_ready(&mut ui);
    assert_eq!(ui.read(|e| e.project.width()), 1280);
    assert!(ui.cx.debug_bounds("project-width").is_none());
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
    ui.update(Editor::open_settings);
    dialog_ready(&mut ui);
    assert!(ui.cx.debug_bounds("dialog-validation-error").is_none());
    edit_dialog_field(&mut ui, "project-width", "1440");
    ui.press("escape");
    dialog_ready(&mut ui);
    assert!(ui.cx.debug_bounds("project-width").is_none());
    assert_eq!(ui.read(|e| e.project.clone()), before);
    ui.update(Editor::open_settings);
    dialog_ready(&mut ui);
    edit_dialog_field(&mut ui, "project-width", "960");
    let apply = ui.cx.debug_bounds("dialog-apply").unwrap();
    ui.click(apply.center(), gpui::Modifiers::default());
    assert_eq!(ui.read(|e| e.project.width()), 960);
}

#[gpui::test]
fn invalid_export_and_named_preset_dialogs_stay_open_on_enter(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let before = project.clone();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(Editor::open_export_dialog);
    dialog_ready(&mut ui);
    edit_dialog_field(&mut ui, "export-output", "");
    ui.press("enter");
    assert!(
        ui.cx.update(|w, cx| {
            use gpui_component::WindowExt;
            w.has_active_dialog(cx)
        }),
        "export unexpectedly closed"
    );
    dialog_ready(&mut ui);
    assert!(ui.cx.debug_bounds("export-output").is_some());
    assert!(ui.cx.debug_bounds("dialog-validation-error").is_some());
    ui.press("escape");
    dialog_ready(&mut ui);
    assert!(ui.cx.debug_bounds("export-output").is_none());
    ui.update(Editor::open_save_caption_preset);
    dialog_ready(&mut ui);
    edit_dialog_field(&mut ui, "caption-preset-name", "  ");
    ui.press("enter");
    assert!(ui.cx.debug_bounds("caption-preset-name").is_some());
    edit_dialog_field(&mut ui, "caption-preset-name", "UI keyboard preset");
    ui.press("enter");
    dialog_ready(&mut ui);
    assert!(ui.cx.debug_bounds("caption-preset-name").is_none());
    assert!(ui.read(|e| {
        e.caption_presets
            .iter()
            .any(|p| p.name == "UI keyboard preset")
    }));
    ui.update(|_, window, cx| Editor::save_template_dialog(window, cx));
    ui.settle();
    dialog_ready(&mut ui);
    edit_dialog_field(&mut ui, "template-name", "");
    ui.press("enter");
    assert!(ui.cx.debug_bounds("template-name").is_some());
    ui.press("escape");
    dialog_ready(&mut ui);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

#[gpui::test]
fn unavailable_subtitle_engine_dialog_cannot_be_started_with_enter(cx: &mut TestAppContext) {
    if animathio_transcribe::engine_available() {
        return;
    }
    let (project, assets) = voice_and_music();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(Editor::open_subtitles_dialog);
    dialog_ready(&mut ui);
    ui.press("enter");
    assert!(ui.read(|e| e.subtitle_job.is_none()));
    assert!(ui.cx.debug_bounds("dialog-0").is_some());
    ui.press("escape");
    dialog_ready(&mut ui);
    assert!(ui.cx.debug_bounds("dialog-0").is_none());
}

#[gpui::test]
fn preferences_save_and_every_group_fit_at_720p(cx: &mut TestAppContext) {
    let _preferences = super::preferences_lock();
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.cx
        .simulate_resize(gpui::size(gpui::px(1280.), gpui::px(720.)));
    ui.settle();
    ui.update(Editor::open_preferences);
    ui.advance_clock(std::time::Duration::from_millis(400));
    for selector in [
        "preferences-autosave",
        "preferences-density-compact",
        "preferences-renderer-gpu",
        "preferences-reduce-motion",
        "preferences-check-updates",
        "preferences-community-plugins",
        "dialog-apply",
    ] {
        let bounds = ui
            .cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is rendered"));
        assert!(bounds.top() >= gpui::px(0.), "{selector}: {bounds:?}");
        assert!(bounds.bottom() <= gpui::px(720.), "{selector}: {bounds:?}");
    }
    // Two-state settings are switches and segmented controls, not selects.
    let motion = ui.read(|e| e.preferences.reduce_motion);
    let switch = ui.cx.debug_bounds("preferences-reduce-motion").unwrap();
    ui.click(
        gpui::point(switch.right() - gpui::px(16.), switch.center().y),
        gpui::Modifiers::default(),
    );
    let save = ui.cx.debug_bounds("dialog-apply").unwrap();
    ui.click(save.center(), gpui::Modifiers::default());
    ui.advance_clock(std::time::Duration::from_millis(400));
    assert_ne!(
        ui.read(|e| e.preferences.reduce_motion),
        motion,
        "the switch changed the setting"
    );
    // Put the shared test config back.
    ui.update(|e, _, cx| {
        let mut restored = e.preferences.clone();
        restored.reduce_motion = motion;
        restored.save().unwrap();
        e.preferences = restored;
        cx.set_reduce_motion(motion);
    });
}

#[gpui::test]
fn numbers_that_are_not_numbers_say_what_to_type(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|editor, _, cx| {
        editor.commit_input_value(
            crate::editor::InputTarget::PluginNumber {
                id: ClipId(1),
                name: "size".into(),
            },
            "-".into(),
            cx,
        );
    });
    let error = last_error(&ui);
    assert!(error.starts_with("That is not a number"), "{error}");
    assert!(error.contains("unchanged"), "{error}");
}
