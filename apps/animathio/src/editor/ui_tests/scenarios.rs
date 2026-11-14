//! Scenarios that drive real actions and assert on model and view state.

use animathio_control::{caption_styles, clip_caption_style};
use animathio_core::{EffectId, EffectInstance};

use super::fixtures::*;
use super::*;
use crate::editor::actions::{
    AddMarker, AutomaticCuts, NewProject, NextMarker, PreviousMarker, Redo, ShowEffects,
    ShowMarkers, SplitClip, ToggleInspector, Undo,
};
use crate::editor::caption_styles::LookChange;

fn clip_count(editor: &Editor) -> usize {
    editor
        .project
        .tracks()
        .iter()
        .map(|track| track.clips().len())
        .sum()
}

fn seek(ui: &mut Ui, ms: i64) {
    ui.update(|editor, _, cx| editor.seek_to(Time(ms * MS), cx));
}

#[gpui::test]
fn sample_project_opens_on_the_headless_environment(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let ui = Ui::open(cx, project, assets);
    ui.read(|editor| {
        assert!(!editor.is_start_screen());
        assert!(!editor.project.tracks().is_empty());
        // Injected environment: null audio sink and the CPU preview backend.
        assert_eq!(editor.env.audio, crate::editor_env::AudioOutput::Null);
        assert!(editor.clock.mixer_format().is_none());
        assert!(!editor.preview.gpu);
        assert!(!editor.dirty);
        assert!(editor.marker_panel.is_none() && editor.autocut_review.is_none());
    });
}

#[gpui::test]
fn start_screen_then_new_project(cx: &mut TestAppContext) {
    let mut ui = Ui::open(cx, Project::default(), BTreeMap::new());
    assert!(ui.read(Editor::is_start_screen));
    ui.dispatch(NewProject);
    ui.read(|editor| {
        assert!(!editor.is_start_screen());
        assert_eq!(editor.project.tracks().len(), 1);
    });
}

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
    // A second marker at the same time is refused with a message, not added.
    ui.dispatch(AddMarker);
    ui.read(|editor| {
        assert_eq!(editor.project.markers().len(), 2);
        assert!(editor.error.is_some());
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
fn automatic_cuts_scope_picker_and_apply(cx: &mut TestAppContext) {
    let (project, assets) = voice_and_music();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|editor, _, _| editor.select_only(VOICE_CLIP));
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
    ui.update(|editor, _, cx| editor.cycle_autocut_scope(cx));
    assert_eq!(scope(&ui), (None, Some(VOICE_TRACK)));
    ui.update(|editor, _, cx| editor.cycle_autocut_scope(cx));
    assert_eq!(scope(&ui), (None, None));
    ui.update(|editor, _, cx| editor.cycle_autocut_scope(cx));
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
    ui.update(|editor, _, _| editor.select_only(VOICE_CLIP));
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
fn caption_restyle_is_one_undo_step(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let command = caption_command(&project);
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|editor, _, cx| {
        assert!(editor.try_execute(&command, cx));
    });

    // Caption text clips, by id, with the style each carries.
    let styles = |ui: &Ui| -> Vec<(ClipId, String)> {
        ui.read(|e| {
            e.project
                .tracks()
                .iter()
                .flat_map(|t| t.clips().values())
                .filter_map(|c| clip_caption_style(c).map(|s| (c.id, s)))
                .collect()
        })
    };
    let before = styles(&ui);
    assert_eq!(before.len(), 2, "two caption cues");
    let original = before[0].1.clone();
    let track = ui.read(|e| {
        e.project
            .tracks()
            .iter()
            .find(|t| t.name() == "Subtitles")
            .map(Track::id)
            .expect("subtitle track")
    });
    // Selecting a caption makes the remembered look follow its style.
    ui.update(|editor, _, _| editor.select_only(before[0].0));
    assert_eq!(ui.read(|e| e.caption_look.style.clone()), original);

    let target = caption_styles()
        .iter()
        .map(|s| s.id)
        .find(|id| *id != original)
        .expect("another style")
        .to_owned();
    ui.update(|editor, _, cx| {
        editor.apply_look_change(LookChange::Style(target.clone()), Some(track), cx);
    });
    assert!(styles(&ui).iter().all(|(_, style)| *style == target));
    assert_eq!(ui.read(|e| e.caption_look.style.clone()), target);

    ui.dispatch(Undo);
    assert_eq!(styles(&ui), before, "one undo restores every caption");
    ui.dispatch(Redo);
    assert!(styles(&ui).iter().all(|(_, style)| *style == target));
}

#[gpui::test]
fn track_audio_inspector_lists_one_card_per_effect(cx: &mut TestAppContext) {
    let (project, assets) = voice_and_music();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|editor, _, _| editor.audio_inspector_track = Some(VOICE_TRACK));
    let chain = AudioChain::Track(VOICE_TRACK);
    let rows = |ui: &mut Ui| ui.update(|e, window, cx| e.audio_chain_rows(chain, window, cx).len());
    let empty = rows(&mut ui);
    for n in 1..=2_u64 {
        let command = Command::AddAudioEffect {
            chain,
            index: usize::try_from(n - 1).expect("index"),
            effect: EffectInstance {
                id: EffectId(n),
                plugin: "org.animathio.audio-basic".into(),
                effect: "gain".into(),
                params: BTreeMap::new(),
                bypass: false,
            },
        };
        ui.update(|editor, _, cx| {
            editor.try_execute(&command, cx);
        });
        assert_eq!(rows(&mut ui), empty + usize::try_from(n).expect("n"));
    }
    let track = ui.read(|e| e.project.tracks()[0].audio_effects().len());
    assert_eq!(track, 2);
    // A bypassed effect keeps its card.
    ui.update(|editor, _, cx| {
        editor.try_execute(
            &Command::SetAudioEffectBypass {
                chain,
                effect: EffectId(1),
                bypass: true,
            },
            cx,
        );
    });
    assert_eq!(rows(&mut ui), empty + 2);
    if require_plugins("track_audio_inspector_lists_one_card_per_effect") {
        assert!(!ui.read(|e| e.available_audio_effects().is_empty()));
    }
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
