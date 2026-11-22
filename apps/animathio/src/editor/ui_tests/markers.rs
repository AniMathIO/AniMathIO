//! Timeline marker actions, selection and editing.

use super::fixtures::{MS, sample};
use super::helpers::{dialog_ready, edit_dialog_field, ruler_point, seek};
use super::{BTreeSet, MarkerId, TestAppContext, Time, Ui};
use crate::editor::actions::{
    AddMarker, DeleteSelection, NextMarker, PreviousMarker, ShowMarkers, Undo,
};

#[gpui::test]
fn markers_add_jump_and_panel(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    seek(&mut ui, 1000);
    ui.dispatch(AddMarker);
    seek(&mut ui, 2000);
    ui.dispatch(AddMarker);
    ui.read(|editor| {
        let times: Vec<_> = editor.project.markers().iter().map(|m| m.time).collect();
        assert_eq!(times, [Time(1000 * MS), Time(2000 * MS)]);
    });
    // Repeating the action at the same time does not add a duplicate.
    ui.dispatch(AddMarker);
    ui.read(|editor| {
        assert_eq!(editor.project.markers().len(), 2);
    });

    seek(&mut ui, 0);
    ui.dispatch(NextMarker);
    assert_eq!(ui.read(|e| e.position), Time(1000 * MS));
    ui.dispatch(NextMarker);
    assert_eq!(ui.read(|e| e.position), Time(2000 * MS));
    ui.dispatch(PreviousMarker);
    assert_eq!(ui.read(|e| e.position), Time(1000 * MS));

    assert!(ui.read(|e| e.marker_panel.is_none()));
    ui.dispatch(ShowMarkers);
    assert!(ui.read(|e| e.marker_panel.is_some()));

    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.markers().len()), 1);
}

#[gpui::test]
fn m_key_adds_a_marker_through_the_keymap(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    seek(&mut ui, 500);
    ui.press("m");
    assert_eq!(ui.read(|e| e.project.markers().len()), 1);
}

#[gpui::test]
fn markers_select_by_click_toggle_and_delete(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    seek(&mut ui, 1000);
    ui.dispatch(AddMarker);
    seek(&mut ui, 2000);
    ui.dispatch(AddMarker);
    let ids: Vec<MarkerId> = ui.read(|e| e.project.markers().iter().map(|m| m.id).collect());
    assert_eq!(ids.len(), 2);
    let selection = |ui: &Ui| ui.read(|e| e.marker_selection.clone());

    // Clicking a marker on the ruler selects it and parks the playhead on it.
    ui.click(
        ruler_point(&ui, Time(1000 * MS)),
        gpui::Modifiers::default(),
    );
    assert_eq!(selection(&ui), BTreeSet::from([ids[0]]));
    assert_eq!(ui.read(|e| e.position), Time(1000 * MS));

    // Shift-click adds the second marker to the selection.
    ui.click(
        ruler_point(&ui, Time(2000 * MS)),
        gpui::Modifiers {
            shift: true,
            ..Default::default()
        },
    );
    assert_eq!(selection(&ui), BTreeSet::from([ids[0], ids[1]]));

    // Shift-clicking a selected marker toggles it off.
    ui.click(
        ruler_point(&ui, Time(1000 * MS)),
        gpui::Modifiers {
            shift: true,
            ..Default::default()
        },
    );
    assert_eq!(selection(&ui), BTreeSet::from([ids[1]]));

    // Delete removes exactly the selected marker; undo restores it and the
    // selection is pruned to markers that exist.
    ui.dispatch(DeleteSelection);
    ui.read(|e| {
        let times: Vec<_> = e.project.markers().iter().map(|m| m.time).collect();
        assert_eq!(times, [Time(1000 * MS)]);
        assert!(e.marker_selection.is_empty());
    });
    ui.dispatch(Undo);
    ui.read(|e| {
        assert_eq!(e.project.markers().len(), 2);
        assert!(
            e.marker_selection
                .iter()
                .all(|id| e.project.marker(*id).is_some())
        );
    });
}

#[gpui::test]
fn marker_dialog_enter_saves_escape_cancels(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.dispatch(AddMarker);
    let id = ui.read(|e| e.project.markers()[0].id);
    ui.update(|e, w, cx| e.open_marker_dialog(id, w, cx));
    dialog_ready(&mut ui);
    edit_dialog_field(&mut ui, "marker-name", "Chapter one");
    ui.press("enter");
    dialog_ready(&mut ui);
    assert_eq!(
        ui.read(|e| e.project.marker(id).unwrap().name.clone()),
        "Chapter one"
    );
    assert!(ui.cx.debug_bounds("marker-name").is_none());
    let before = ui.read(|e| e.project.clone());
    ui.update(|e, w, cx| e.open_marker_dialog(id, w, cx));
    dialog_ready(&mut ui);
    edit_dialog_field(&mut ui, "marker-name", "Cancelled");
    ui.press("escape");
    dialog_ready(&mut ui);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}
