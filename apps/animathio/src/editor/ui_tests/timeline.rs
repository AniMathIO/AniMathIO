//! Timeline editing and playback.

use super::fixtures::{MS, sample};
use super::helpers::{clear_error, clip_count, last_error, seek};
use super::{Command, Project, TestAppContext, Time, Track, Ui, actions};
use crate::editor::actions::{Redo, SplitClip, Undo};
use crate::timeline;

#[gpui::test]
fn split_undo_redo_through_actions(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    let before = ui.read(clip_count);
    seek(&mut ui, 1500);
    ui.dispatch(SplitClip);
    let split = ui.read(clip_count);
    assert!(split > before, "split adds clips: {before} -> {split}");
    ui.dispatch(Undo);
    assert_eq!(ui.read(clip_count), before);
    ui.dispatch(Redo);
    assert_eq!(ui.read(clip_count), split);
    // Undo after redo is still a single step.
    ui.dispatch(Undo);
    assert_eq!(ui.read(clip_count), before);
}

#[gpui::test]
fn scrub_play_pause_resume_preserves_playhead(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| {
        let x = timeline::time_to_x(&e.current_layout(), Time(1000 * MS));
        e.scrub_to(x, cx);
    });
    ui.dispatch(actions::TogglePlayback);
    ui.read(|e| {
        assert!(e.playing);
        assert!(e.clock.now() >= Time(1000 * MS));
        assert!(e.position >= Time(1000 * MS));
    });
    ui.dispatch(actions::TogglePlayback);
    let paused = ui.read(|e| {
        assert!(!e.playing);
        e.position
    });
    ui.dispatch(actions::TogglePlayback);
    ui.read(|e| {
        assert!(e.playing);
        assert!(e.clock.now() >= paused);
        assert!(e.position >= paused);
    });
    ui.dispatch(actions::TogglePlayback);
}

#[gpui::test]
fn split_with_nothing_under_the_playhead_explains_itself(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    let before = ui.read(|e| e.project.clone());
    seek(&mut ui, 60_000);
    ui.dispatch(SplitClip);
    assert!(
        last_error(&ui).starts_with("Nothing to split"),
        "split hint"
    );
    assert_eq!(ui.read(|e| e.project.clone()), before, "nothing changed");
    clear_error(&mut ui);
    ui.dispatch(crate::editor::actions::SplitAllTracks);
    assert!(
        last_error(&ui).starts_with("Nothing to split"),
        "split all hint"
    );
}

#[gpui::test]
fn paste_and_duplicate_with_nothing_to_work_on_say_so(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    let before = ui.read(|e| e.project.clone());
    ui.update(|editor, _, cx| editor.clear_selection(cx));
    ui.dispatch(crate::editor::actions::DuplicateClips);
    assert!(last_error(&ui).starts_with("Select a clip to duplicate"));
    clear_error(&mut ui);
    ui.update(|editor, _, cx| {
        editor.clipboard = None;
        editor.marker_clipboard = None;
        cx.write_to_clipboard(gpui::ClipboardItem::new_string("not clips".into()));
    });
    ui.dispatch(crate::editor::actions::PasteClips);
    assert!(last_error(&ui).starts_with("Nothing to paste"));
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

#[test]
fn edit_refusals_are_rewritten_into_instructions() {
    use crate::editor::commands::friendly_edit_error;
    let mut project = Project::default();
    Command::AddTrack {
        track: Track::new(animathio_core::TrackId(7), "Titles"),
        index: 0,
    }
    .apply(&mut project)
    .unwrap();
    assert!(friendly_edit_error(&project, "overlapping clips").contains("overlap another clip"));
    let locked = friendly_edit_error(&project, "track 7 is locked");
    assert!(
        locked.contains("\"Titles\"") && locked.contains("lock"),
        "{locked}"
    );
    assert_eq!(
        friendly_edit_error(&project, "something else"),
        "something else"
    );
}
