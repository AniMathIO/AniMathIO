//! Export submission and validation.

use super::fixtures::sample;
use super::helpers::last_error;
use super::{BTreeMap, Editor, Project, TestAppContext, Time, Ui};

#[gpui::test]
fn export_dialog_refuses_the_open_project_file(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("project.amio");
    std::fs::write(&project_file, b"x").unwrap();
    ui.update(|editor, _window, _cx| editor.path = Some(project_file.clone()));
    // The dialog path and the control-bridge path must both refuse.
    let submitted = ui.update(|editor, _window, cx| {
        editor.submit_export(
            None,
            animathio_export::Format::Mp4,
            "Whole project",
            None,
            None,
            project_file.clone(),
            false,
            cx,
        )
    });
    assert!(matches!(
        submitted,
        crate::editor::dialogs::Submitted::Invalid
    ));
    // The editor turns dialog errors into a toast on the next render, which
    // keeps the message in `last_error` (the transient `error` is taken).
    assert!(ui.read(|editor| {
        editor
            .last_error
            .as_deref()
            .is_some_and(|error| error.contains("the output path is the open project file"))
    }));
    assert!(
        ui.read(|editor| !editor.export_queue.is_busy()),
        "nothing queued"
    );
    assert_eq!(
        std::fs::read(&project_file).unwrap(),
        b"x",
        "project untouched"
    );
    ui.update(|editor, _window, _cx| {
        assert!(editor.control_export_allowed(&project_file).is_err());
        // Any other destination is fine, including next to the project.
        assert!(
            editor
                .control_export_allowed(&dir.path().join("out.mp4"))
                .is_ok()
        );
    });
}

#[gpui::test]
fn export_dialog_still_queues_other_paths_while_a_project_is_open(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("project.amio");
    std::fs::write(&project_file, b"x").unwrap();
    ui.update(|editor, _window, _cx| editor.path = Some(project_file.clone()));
    let submitted = ui.update(|editor, _window, cx| {
        editor.submit_export(
            None,
            animathio_export::Format::Mp4,
            "Whole project",
            None,
            None,
            dir.path().join("out.mp4"),
            false,
            cx,
        )
    });
    assert!(
        matches!(submitted, crate::editor::dialogs::Submitted::Queued),
        "exporting next to the project must still work"
    );
    assert_eq!(
        std::fs::read(&project_file).unwrap(),
        b"x",
        "project untouched"
    );
}

#[gpui::test]
fn export_explains_an_empty_project_and_a_bad_range(cx: &mut TestAppContext) {
    let mut ui = Ui::open(cx, Project::default(), BTreeMap::new());
    ui.update(Editor::open_export_dialog);
    assert!(last_error(&ui).starts_with("Nothing to export yet"));

    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    let out = super::config_dir().join("usability-export.mp4");
    let outcome = ui.update(|editor, _, cx| {
        editor.submit_export(
            None,
            animathio_export::Format::Mp4,
            "Custom",
            Some(Time(10)),
            Some(Time(5)),
            out.clone(),
            false,
            cx,
        )
    });
    assert!(matches!(
        outcome,
        crate::editor::dialogs::Submitted::Invalid
    ));
    let error = last_error(&ui);
    assert!(error.contains("HH:MM:SS:FF"), "custom range hint: {error}");
}
