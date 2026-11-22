//! Automatic cuts, scene detection and analysis used by screenshot panels.

use super::fixtures::{
    MUSIC_TRACK, VOICE_CLIP, VOICE_TRACK, sample, scenes_fixture, voice_and_music,
};
use super::helpers::seek;
use super::{ClipId, Editor, TestAppContext, Ui, actions};
use crate::editor::actions::{AutomaticCuts, Redo, SplitClip, Undo};

#[gpui::test]
fn automatic_cuts_scope_picker_and_apply(cx: &mut TestAppContext) {
    let (project, assets) = voice_and_music();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|editor, _, cx| editor.select_only(VOICE_CLIP, cx));
    ui.dispatch(AutomaticCuts);
    ui.settle();

    // The panel opens scoped to the selected clip and analyses it.
    ui.read(|editor| {
        let review = editor.autocut_review.as_ref().expect("panel open");
        assert_eq!(review.params.clip, Some(VOICE_CLIP));
        let proposal = review.proposal.as_ref().expect("analysis finished");
        assert_eq!(proposal.cuts.len(), 1, "the 1 s pause is the only cut");
    });

    // Scope picker: clip -> its track's mix -> whole project -> clip.
    let scope = |ui: &Ui| {
        ui.read(|e| {
            let params = &e.autocut_review.as_ref().expect("panel open").params;
            (params.clip, params.track)
        })
    };
    ui.update(|editor, _, cx| editor.set_autocut_scope(super::super::autocut::Scope::Track, cx));
    assert_eq!(scope(&ui), (None, Some(VOICE_TRACK)));
    ui.update(|editor, _, cx| editor.set_autocut_scope(super::super::autocut::Scope::Project, cx));
    assert_eq!(scope(&ui), (None, None));
    ui.update(|editor, _, cx| editor.set_autocut_scope(super::super::autocut::Scope::Clip, cx));
    assert_eq!(scope(&ui), (Some(VOICE_CLIP), None));
    ui.read(|e| {
        let review = e.autocut_review.as_ref().expect("panel open");
        assert!(review.proposal.is_some(), "re-analysed after the change");
    });

    // Apply: one undo step that cuts the voice and leaves the music alone.
    let voice_clips = |e: &Editor| e.project.tracks()[0].clips().len();
    let music_clips = |e: &Editor| e.project.tracks()[1].clips().len();
    assert_eq!((ui.read(voice_clips), ui.read(music_clips)), (1, 1));
    ui.update(|editor, _, cx| editor.apply_autocut(cx));
    assert!(ui.read(|e| e.autocut_review.is_none()), "panel closes");
    assert_eq!((ui.read(voice_clips), ui.read(music_clips)), (2, 1));
    ui.dispatch(Undo);
    assert_eq!((ui.read(voice_clips), ui.read(music_clips)), (1, 1));
    ui.dispatch(Redo);
    assert_eq!(ui.read(voice_clips), 2);
}

#[gpui::test]
fn editing_invalidates_an_open_autocut_proposal(cx: &mut TestAppContext) {
    let (project, assets) = voice_and_music();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|editor, _, cx| editor.select_only(VOICE_CLIP, cx));
    ui.dispatch(AutomaticCuts);
    ui.settle();
    seek(&mut ui, 500);
    ui.dispatch(SplitClip);
    ui.read(|editor| {
        let review = editor.autocut_review.as_ref().expect("panel stays open");
        assert!(review.proposal.is_none(), "stale proposal dropped");
    });
}

#[gpui::test]
fn scene_menu_requires_video_and_does_not_mutate(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    let before = ui.read(|e| e.project.clone());
    ui.dispatch(actions::SceneCuts);
    ui.read(|editor| {
        assert!(editor.scene_panel.is_none());
        assert_eq!(editor.project, before);
    });
}

#[gpui::test]
fn scene_review_split_and_undo(cx: &mut TestAppContext) {
    let (project, assets) = scenes_fixture();
    let before = project.clone();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| e.select_only(ClipId(1), cx));
    ui.dispatch(actions::SceneCuts);
    ui.settle();
    ui.read(|e| assert!(e.scene_panel.as_ref().is_some_and(|p| !p.busy)));
    ui.update(|e, _, cx| e.apply_scenes(false, cx));
    ui.read(|e| assert_eq!(e.project.tracks()[0].clips().len(), 3));
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

#[gpui::test]
fn screenshot_panels_use_real_analysis(cx: &mut TestAppContext) {
    let (project, assets) = voice_and_music();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|editor, window, cx| editor.open_screenshot_panel("markers", window, cx));
    assert!(ui.read(|editor| editor.marker_panel.is_some()));
    ui.update(|editor, window, cx| editor.open_screenshot_panel("autocut", window, cx));
    ui.settle();
    ui.read(|editor| {
        assert_eq!(editor.selected, Some(VOICE_CLIP));
        let proposal = editor
            .autocut_review
            .as_ref()
            .expect("review")
            .proposal
            .as_ref()
            .expect("analysis completed");
        assert!(
            !proposal.cuts.is_empty(),
            "voice pause detected despite music bed"
        );
    });
    ui.update(|editor, window, cx| editor.open_screenshot_panel("duck", window, cx));
    ui.settle();
    ui.read(|editor| {
        assert_eq!(editor.selected, Some(VOICE_CLIP));
        let review = editor.duck_review.as_ref().expect("duck panel");
        assert_eq!(review.params.music_tracks, vec![MUSIC_TRACK]);
        assert!(
            review
                .proposal
                .as_ref()
                .is_some_and(|p| !p.clips.is_empty())
        );
    });
}
