//! Headless startup, window state and editor panels.

use super::fixtures::{VOICE_CLIP, sample, voice_and_music, wav};
use super::helpers::{bounds_of, clip_count, search_library};
use super::{
    BTreeMap, Command, LeftTab, Project, TestAppContext, Time, Ui, actions, require_plugins,
};
use crate::editor::actions::{NewProject, ShowEffects, ToggleInspector, Undo};

#[gpui::test]
fn sample_project_opens_on_the_headless_environment(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let ui = Ui::open(cx, project, assets);
    ui.read(|editor| {
        assert!(!editor.is_start_screen());
        assert_ne!(editor.project.tracks(), []);
        // Injected environment: null audio sink and the CPU preview backend.
        assert_eq!(editor.env.audio, crate::editor_env::AudioOutput::Null);
        assert!(editor.clock.mixer_format().is_none());
        assert!(!editor.preview.gpu);
        assert!(!editor.dirty);
        assert!(editor.marker_panel.is_none() && editor.autocut_review.is_none());
    });
}

/// The window state is tracked from real window geometry, debounced-saves through the
/// headless environment's config directory, and reloads with the tracked geometry.
#[gpui::test]
fn window_state_persists_through_the_config_dir(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    // The editor observed the window's bounds when it was built.
    let initial = ui.read(|editor| editor.window_tracker.state());
    assert!(
        initial.width > 0. && initial.height > 0.,
        "observed geometry"
    );
    assert!(
        !initial.maximized,
        "the headless platform window is windowed"
    );

    let dir = super::config_dir().join("window-state-persistence");
    std::fs::remove_dir_all(&dir).ok();
    let path = dir.join(crate::window_state::FILE_NAME);
    ui.update(|editor, _window, _cx| editor.window_tracker.save_path = Some(path.clone()));
    // A geometry change schedules the debounced save; the file only appears once the
    // debounce settles.
    ui.update(|editor, _window, cx| editor.schedule_window_state_save(cx));
    assert!(!path.exists(), "the debounced save has not fired yet");
    ui.advance_clock(crate::window_state::SAVE_DEBOUNCE);
    let saved = crate::window_state::WindowState::read_from(&path)
        .expect("the debounced save wrote the state");
    assert_eq!(saved, initial, "the tracked geometry is what lands on disk");
    std::fs::remove_file(&path).ok();
}

/// An explicitly opened empty project and an empty import still have a canvas.
#[gpui::test]
fn empty_project_keeps_status_summary(cx: &mut TestAppContext) {
    let mut ui = Ui::open(cx, Project::default(), BTreeMap::new());
    let path = super::config_dir().join("empty-status.amio");
    crate::editor::support::write_project(&path, &Project::default(), &BTreeMap::new()).unwrap();
    ui.update(|editor, _, cx| {
        assert!(editor.try_open_path(&path, cx));
        assert!(editor.status_summary_visible());
        editor.new_project(cx);
        editor.try_execute(
            &Command::RemoveTrack {
                id: animathio_core::TrackId(1),
            },
            cx,
        );
        assert_eq!(editor.project.tracks(), []);
        assert!(editor.status_summary_visible());
        editor.new_project_with_chooser(cx);
        assert!(!editor.status_summary_visible());
        editor
            .handle_control_call(
                animathio_control::Call::ProjectOpen { path: path.clone() },
                cx,
            )
            .unwrap();
        assert!(editor.status_summary_visible());
        editor.new_project_with_chooser(cx);
        let v1 = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/import-v1/tests/fixtures/empty.animathio");
        editor.import_control_v1(&v1).unwrap();
        assert!(editor.path.is_none());
        assert!(editor.status_summary_visible());
    });
    std::fs::remove_file(path).ok();
}

#[gpui::test]
fn empty_manim_import_keeps_status_summary(cx: &mut TestAppContext) {
    if !require_plugins("empty_manim_import_keeps_status_summary") {
        return;
    }
    let mut ui = Ui::open(cx, Project::default(), BTreeMap::new());
    ui.update(|editor, _, cx| {
        editor.handle_control_call(animathio_control::Call::ImportManim {
            params: animathio_control::ImportManimParams {
                path: None,
                source: Some("from manim import *\nclass Empty(Scene):\n    def construct(self):\n        pass\n".into()),
                script_dir: None,
                scene: None,
                replace: true,
            },
        }, cx).unwrap();
        assert!(editor.path.is_none());
        assert!(editor.status_summary_visible());
    });
}

#[gpui::test]
fn panel_toggles_change_dock_and_tab_state(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    let open = ui.read(|e| e.dock.right_open);
    ui.dispatch(ToggleInspector);
    assert_eq!(ui.read(|e| e.dock.right_open), !open);
    ui.dispatch(ToggleInspector);
    assert_eq!(ui.read(|e| e.dock.right_open), open);
    ui.dispatch(ShowEffects);
    assert!(ui.read(|e| e.left_tab == LeftTab::Effects && e.dock.left_open));
}

/// The headless test environment never checks for updates: the check is gated on the
/// environment (not on a preference), so no notice appears and no attempt timestamp
/// is written to the harness config directory.
#[gpui::test]
fn headless_environment_never_checks_for_updates(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.settle();
    ui.read(|editor| {
        assert!(
            !editor.env.updates,
            "the headless environment skips the check"
        );
        assert!(
            editor.preferences.check_updates,
            "the gate is the environment, not the preference"
        );
        assert!(editor.notices.is_empty(), "no update notice in tests");
    });
    let state = super::config_dir().join(super::update_check::STATE_FILE);
    assert!(!state.exists(), "the harness never contacts the network");
}

#[gpui::test]
fn select_capture_hooks_pick_the_first_clip_of_a_kind(cx: &mut TestAppContext) {
    let (project, assets) = voice_and_music();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|editor, window, cx| editor.open_screenshot_panel("select-audio", window, cx));
    ui.read(|editor| assert_eq!(editor.selected, Some(VOICE_CLIP)));
    // No LaTeX clip in this project: the hook clears the selection, it does not
    // fall back to another kind.
    ui.update(|editor, window, cx| editor.open_screenshot_panel("select-latex", window, cx));
    ui.read(|editor| assert_eq!(editor.selected, None));
}

#[gpui::test]
fn an_empty_project_says_what_to_do_and_an_edited_one_does_not(cx: &mut TestAppContext) {
    let mut ui = Ui::open(cx, Project::default(), BTreeMap::new());
    ui.dispatch(NewProject);
    ui.press("enter");
    assert_eq!(ui.read(clip_count), 0);
    assert!(
        ui.cx.debug_bounds("viewer-empty-hint").is_some(),
        "a blank canvas explains how to start"
    );
    let path = super::config_dir().join("empty-hint.wav");
    std::fs::write(&path, wav(&[(0.25, 800)])).unwrap();
    let track = ui.read(|e| e.project.tracks()[0].id());
    ui.update(|editor, _, cx| editor.import_paths(&[path], Some((track, Time(0))), cx));
    assert!(ui.read(clip_count) > 0);
    assert!(
        ui.cx.debug_bounds("viewer-empty-hint").is_none(),
        "the hint goes away once something is on the timeline"
    );
}

#[gpui::test]
fn library_elements_plus_buttons_add_latex_text_and_shape_and_undo(cx: &mut TestAppContext) {
    if !require_plugins("library_elements_plus_buttons_add_latex_text_and_shape_and_undo") {
        return;
    }
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.dispatch(actions::FocusLibrary);
    for (query, index, plugin) in [
        ("latex", 0, "latex"),
        ("text", 1, "text"),
        ("circle", 7, "shapes"),
    ] {
        search_library(&mut ui, LeftTab::Elements, query);
        let before = ui.read(|e| e.project.clone());
        let count = ui.read(clip_count);
        let bounds =
            bounds_of(&mut ui, format!("element-default-{index}")).expect("visible add button");
        ui.click(bounds.center(), gpui::Modifiers::default());
        assert_eq!(ui.read(clip_count), count + 1);
        ui.read(|e| {
            let clip = e.find_clip(e.selected.expect("new selection")).unwrap();
            assert!(matches!(&clip.kind, animathio_core::ClipKind::Plugin { plugin_id, .. } if plugin_id == &format!("org.animathio.{plugin}")));
        });
        ui.dispatch(Undo);
        assert_eq!(ui.read(|e| e.project.clone()), before);
    }
    // View destinations reopen a hidden panel, including the existing Effects action.
    ui.dispatch(actions::ToggleMediaPool);
    assert!(!ui.read(|e| e.dock.left_open));
    // The hidden panel took its focus with it; the user is working elsewhere.
    ui.update(|e, window, cx| window.focus(&e.panel_focus[0], cx));
    ui.dispatch(actions::ShowElements);
    assert!(ui.read(|e| e.dock.left_open && e.left_tab == LeftTab::Elements));
    ui.dispatch(actions::ToggleMediaPool);
    assert!(!ui.read(|e| e.dock.left_open));
    // The hidden panel took its focus with it; the user is working elsewhere.
    ui.update(|e, window, cx| window.focus(&e.panel_focus[0], cx));
    ui.dispatch(actions::ShowTitles);
    assert!(ui.read(|e| e.dock.left_open && e.left_tab == LeftTab::Titles));
    ui.dispatch(actions::ShowPlugins);
    assert!(ui.read(|e| e.left_tab == LeftTab::Plugins));
    ui.dispatch(actions::ShowEffects);
    assert!(ui.read(|e| e.left_tab == LeftTab::Effects));
}
