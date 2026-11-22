//! Launcher actions, templates and the About dialog.

use super::fixtures::sample;
use super::{BTreeMap, Editor, Project, TestAppContext, Ui};
use crate::editor::actions::{NewProject, Undo};

#[gpui::test]
fn start_screen_then_new_project(cx: &mut TestAppContext) {
    let mut ui = Ui::open(cx, Project::default(), BTreeMap::new());
    assert!(ui.read(Editor::is_start_screen));
    ui.dispatch(NewProject);
    assert!(ui.read(Editor::is_start_screen));
    ui.press("enter");
    ui.read(|editor| {
        assert!(!editor.is_start_screen());
        assert_eq!(editor.project.tracks().len(), 1);
        assert_eq!(
            (editor.project.width(), editor.project.height()),
            (1920, 1080)
        );
        assert!(!editor.dirty);
    });
}

#[gpui::test]
fn templates_launcher_renders_cards(cx: &mut TestAppContext) {
    let mut ui = Ui::open(cx, Project::default(), BTreeMap::new());
    ui.update(|e, _, cx| {
        e.template_cards(cx);
    });
    ui.settle();
    assert!(ui.read(|e| !e.title_library.template_manifests.is_empty()));
}

#[gpui::test]
fn new_project_shortcut_and_menu_reuse_the_launcher_canvas_chooser(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.press("secondary-alt-n");
    assert!(ui.read(Editor::is_start_screen));
    assert!(ui.cx.debug_bounds("canvas-preset-0").is_some());
    ui.press("enter");
    assert_eq!(
        ui.read(|e| (e.project.width(), e.project.height())),
        (1920, 1080)
    );
    ui.dispatch(NewProject);
    assert!(ui.read(Editor::is_start_screen));
    let vertical = ui.cx.debug_bounds("canvas-preset-2").unwrap();
    ui.click(vertical.center(), gpui::Modifiers::default());
    assert_eq!(
        ui.read(|e| (e.project.width(), e.project.height())),
        (1080, 1920)
    );
    ui.read(|e| {
        assert!(!e.dirty);
        assert_eq!(e.project.tracks().len(), 1);
    });
    ui.dispatch(Undo);
    assert_eq!(
        ui.read(|e| (e.project.width(), e.project.height())),
        (1080, 1920)
    );
}

#[test]
fn about_reports_the_running_version() {
    let text = crate::editor::actions::about_text();
    assert!(text.contains(env!("CARGO_PKG_VERSION")), "{text}");
}
