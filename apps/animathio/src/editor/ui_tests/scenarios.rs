//! Scenarios that drive real actions and assert on model and view state.

use crate::editing;
use crate::editor::{TimelineGesture, TrimDrag};
use crate::timeline::TrimEdge;
use gpui::{AppContext, AsKeystroke, MouseUpEvent};

use animathio_control::{
    MathObjectDraft, MathObjectKind, caption_styles, clip_caption_style, math_object_range,
};
use animathio_core::{ClipSpeed, EffectId, EffectInstance};

use super::fixtures::*;
use super::{
    AudioChain, BTreeMap, BTreeSet, ClipId, Command, Editor, LeftTab, MarkerId, Project,
    TestAppContext, Time, Tool, Track, Ui, actions, require_plugins, timeline,
};
use crate::editor::actions::{
    AddMarker, AutomaticCuts, DeleteSelection, NewProject, NextMarker, PreviousMarker, Redo,
    ShowEffects, ShowMarkers, SplitClip, ToggleInspector, Undo,
};
use crate::editor::caption_styles::LookChange;
use crate::editor::math::MathEditor;
use crate::editor::retime;
use crate::editor::support::project_size;

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
        assert_ne!(editor.project.tracks(), []);
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
    ui.update(|editor, _, cx| editor.select_only(before[0].0, cx));
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
fn auto_duck_panel_apply_and_undo(cx: &mut TestAppContext) {
    let (project, assets) = voice_and_music();
    let before = project.clone();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| e.select_only(VOICE_CLIP, cx));
    ui.dispatch(actions::AutoDuck);
    ui.update(|e, _, cx| {
        e.duck_review
            .as_mut()
            .expect("panel open")
            .params
            .music_tracks = vec![MUSIC_TRACK];
        e.analyse_duck(cx);
    });
    assert!(ui.read(|e| e.duck_review.as_ref().unwrap().proposal.is_some()));
    ui.update(|e, _, cx| e.apply_duck(cx));
    assert!(ui.read(|e| e.duck_review.is_none()));
    assert_ne!(ui.read(|e| e.project.clone()), before);
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

#[gpui::test]
fn auto_duck_clip_apply_and_undo(cx: &mut TestAppContext) {
    let (project, assets) = voice_and_music();
    let before = project.clone();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| e.select_only(VOICE_CLIP, cx));
    ui.dispatch(actions::AutoDuck);
    ui.update(|e, _, cx| {
        e.duck_review
            .as_mut()
            .expect("panel open")
            .params
            .music_clips = vec![MUSIC_CLIP];
        e.analyse_duck(cx);
    });
    assert!(ui.read(|e| e.duck_review.as_ref().unwrap().proposal.is_some()));
    ui.update(|e, _, cx| e.apply_duck(cx));
    assert!(ui.read(|e| e.duck_review.is_none()));
    assert_ne!(ui.read(|e| e.project.clone()), before);
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

#[gpui::test]
fn video_effect_card_param_edit_and_undo(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    let id = ui.read(|e| e.project.tracks()[0].clips().values().next().unwrap().id);
    ui.update(|e, _, cx| {
        e.execute(
            &Command::AddEffect {
                id,
                index: 0,
                effect: EffectInstance {
                    id: EffectId(900),
                    plugin: "org.animathio.effects-basic".into(),
                    effect: "blur".into(),
                    params: BTreeMap::new(),
                    bypass: false,
                },
            },
            cx,
        );
    });
    let before = ui.read(|e| e.project.clone());
    ui.update(|e, window, cx| {
        assert!(!e.video_effect_rows(id, window, cx).is_empty());
        let command = e
            .video_effect_param_command(id, EffectId(900), "radius", 5.0)
            .unwrap();
        e.execute(&command, cx);
    });
    assert_ne!(ui.read(|e| e.project.clone()), before);
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

#[gpui::test]
fn animation_preset_apply_and_undo(cx: &mut TestAppContext) {
    if !require_plugins("animation_preset_apply_and_undo") {
        return;
    }
    let (project, assets) = sample();
    let before = project.clone();
    let mut ui = Ui::open(cx, project, assets);
    let id = ui.read(|e| e.project.tracks()[0].clips().values().next().unwrap().id);
    ui.update(|e, _, cx| {
        e.select_only(id, cx);
        e.apply_animation_preset("org.animathio.animations", "fade-in", cx);
    });
    assert_ne!(ui.read(|e| e.project.clone()), before);
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

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
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.tracks().len()), layers);
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
fn media_fit_picker_change_and_undo(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    let id = ui.read(|e| {
        e.project
            .tracks()
            .iter()
            .flat_map(|t| t.clips().values())
            .find(|c| {
                matches!(
                    c.kind,
                    animathio_core::ClipKind::Image(_) | animathio_core::ClipKind::Video(_)
                )
            })
            .expect("sample media clip")
            .id
    });
    let before = ui.read(|e| e.project.clone());
    ui.update(|e, _, cx| {
        e.select_only(id, cx);
        e.set_clip_fit(id, animathio_core::Fit::Cover, cx);
    });
    assert_eq!(
        ui.read(|e| e.find_clip(id).unwrap().fit),
        animathio_core::Fit::Cover
    );
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

#[gpui::test]
fn clip_context_menu_renders_with_shortcuts(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, window, cx| {
        let id = e.project.tracks()[0].clips().values().next().unwrap().id;
        e.context_menu = Some(super::super::ContextMenu::Clip { id });
        e.context_menu_pos = Some(gpui::point(gpui::px(100.), gpui::px(100.)));
        assert!(e.context_menu_dropdown(window, cx).is_some());
        assert!(
            gpui_component::kbd::Kbd::binding_for_action(
                &actions::SplitClip,
                Some("Timeline"),
                window
            )
            .is_some()
        );
        // The hints match the Premiere keymap (#240): Ctrl/Cmd+K split, Q/W ripple
        // trims, M marker, V/C/B tools.
        let hint = |action: &dyn gpui::Action| -> (String, bool) {
            let binding = window
                .highest_precedence_binding_for_action_in_context(
                    action,
                    gpui::KeyContext::parse("Timeline").expect("context"),
                )
                .expect("binding exists");
            let stroke = binding.keystrokes()[0].as_keystroke();
            (stroke.key.clone(), stroke.modifiers.secondary(cx))
        };
        assert_eq!(hint(&actions::SplitClip), ("k".into(), true));
        assert_eq!(hint(&actions::RippleTrimPrevious), ("q".into(), false));
        assert_eq!(hint(&actions::RippleTrimNext), ("w".into(), false));
        assert_eq!(hint(&actions::AddMarker), ("m".into(), false));
        assert_eq!(hint(&actions::SelectTool), ("v".into(), false));
        assert_eq!(hint(&actions::BladeTool), ("c".into(), false));
        assert_eq!(hint(&actions::RippleTool), ("b".into(), false));
    });
}

/// A window-space point on the timeline ruler at `time` (the marker lane).
fn ruler_point(ui: &Ui, time: Time) -> gpui::Point<gpui::Pixels> {
    ui.read(|e| {
        let layout = e.current_layout();
        let x = timeline::time_to_x(&layout, time);
        let y = layout.ruler_height / 2.;
        gpui::point(
            gpui::px(f32::from(e.timeline_bounds.origin.x) + x),
            gpui::px(f32::from(e.timeline_bounds.origin.y) + y),
        )
    })
}

#[gpui::test]
fn premiere_keymap_tools_split_and_ripple_trims(cx: &mut TestAppContext) {
    let (project, assets) = voice_pair();
    let mut ui = Ui::open(cx, project, assets);
    // Focus the timeline by clicking its ruler, like a real editor session.
    ui.click(ruler_point(&ui, Time(0)), gpui::Modifiers::default());

    // Tool keys: V selection, C razor, B ripple edit.
    ui.press("c");
    assert_eq!(ui.read(|e| e.tool), Tool::Blade);
    ui.press("b");
    assert_eq!(ui.read(|e| e.tool), Tool::Ripple);
    ui.press("v");
    assert_eq!(ui.read(|e| e.tool), Tool::Select);

    // Ctrl/Cmd+K splits the clip under the playhead; one undo restores it.
    let before = ui.read(|e| e.project.clone());
    seek(&mut ui, 1000);
    ui.press("secondary-k");
    assert_eq!(ui.read(|e| e.project.tracks()[0].clips().len()), 3);
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);

    // Q ripple-trims the previous edit to the playhead: the first clip's head,
    // the tail shifts left, the source window follows.
    ui.press("q");
    ui.read(|e| {
        let clips = e.project.tracks()[0].clips();
        let head = clips.get(&PAIR_FIRST).expect("first clip");
        assert_eq!(head.range.start, Time(0));
        assert_eq!(head.range.end, Time(1000 * MS));
        assert_eq!(head.source_bounds(), (Time(1000 * MS), Time(2000 * MS)));
        let tail = clips.get(&PAIR_SECOND).expect("second clip");
        assert_eq!(tail.range.start, Time(1000 * MS));
        assert_eq!(tail.range.end, Time(3000 * MS));
    });
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);

    // W ripple-trims the next edit: the first clip's tail, source kept from zero.
    ui.press("w");
    ui.read(|e| {
        let clips = e.project.tracks()[0].clips();
        let head = clips.get(&PAIR_FIRST).expect("first clip");
        assert_eq!(head.range.end, Time(1000 * MS));
        assert_eq!(head.source_bounds(), (Time(0), Time(1000 * MS)));
        let tail = clips.get(&PAIR_SECOND).expect("second clip");
        assert_eq!(tail.range.start, Time(1000 * MS));
        assert_eq!(tail.range.end, Time(3000 * MS));
    });
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

#[gpui::test]
fn speed_duration_change_ripples_and_undoes(cx: &mut TestAppContext) {
    let (project, assets) = voice_pair();
    let mut ui = Ui::open(cx, project, assets);
    let before = ui.read(|e| e.project.clone());
    ui.update(|e, _, cx| e.select_only(PAIR_FIRST, cx));
    // The Speed/Duration dialog parses its percent field with `retime::percent`
    // and applies one `SetClipSpeed`; drive the same path here.
    let speed = retime::percent("200").expect("valid percent");
    assert_eq!(speed, ClipSpeed::new(2, 1).expect("2x"));
    ui.update(|e, _, cx| {
        // The inspector's Speed section renders for the selection.
        let _ = e.speed_rows(PAIR_FIRST, cx);
        assert!(e.try_execute(
            &Command::SetClipSpeed {
                id: PAIR_FIRST,
                speed,
                reverse: false,
                ramps: Vec::new(),
                ripple: true,
            },
            cx
        ));
    });
    ui.read(|e| {
        let clips = e.project.tracks()[0].clips();
        let head = clips.get(&PAIR_FIRST).expect("first clip");
        // 2x halves the timeline duration and keeps the source interval.
        assert_eq!(head.range.start, Time(0));
        assert_eq!(head.range.end, Time(1000 * MS));
        assert_eq!(head.source_bounds(), (Time(0), Time(2000 * MS)));
        assert_eq!(head.time_map.speed.percent(), 200.);
        // Ripple trailing clips moved the second clip up by the lost second.
        let tail = clips.get(&PAIR_SECOND).expect("second clip");
        assert_eq!(tail.range.start, Time(1000 * MS));
        assert_eq!(tail.range.end, Time(3000 * MS));
    });
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
    ui.dispatch(Redo);
    ui.read(|e| {
        assert_eq!(
            e.project.tracks()[0]
                .clips()
                .get(&PAIR_FIRST)
                .unwrap()
                .range
                .end,
            Time(1000 * MS)
        );
    });
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
fn caption_custom_colour_and_preset_undo(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let command = caption_command(&project);
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|editor, _, cx| {
        assert!(editor.try_execute(&command, cx));
    });
    let before = ui.read(|e| e.project.clone());
    let track = before
        .tracks()
        .iter()
        .find(|t| t.name() == "Subtitles")
        .unwrap()
        .id();
    let caption = before
        .tracks()
        .iter()
        .flat_map(|t| t.clips().values())
        .find(|c| clip_caption_style(c).is_some())
        .unwrap()
        .id;
    ui.update(|editor, _, cx| editor.select_only(caption, cx));
    let custom = [17, 83, 149, 255];
    ui.update(|editor, _, cx| {
        editor.apply_look_change(LookChange::Text(custom), Some(track), cx);
    });
    assert_eq!(ui.read(|e| e.caption_look.color), Some(custom));
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
    let mut look = ui.read(|e| e.caption_look.clone());
    look.style = "karaoke".into();
    look.accent = Some(custom);
    ui.update(|editor, _, cx| {
        editor.apply_look_change(LookChange::Preset(look.clone()), Some(track), cx);
    });
    assert_eq!(ui.read(|e| e.caption_look.clone()), look);
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

fn additional_tool_scenario(cx: &mut TestAppContext, key: &str, tool: Tool) {
    let before = editing::tool_tests::fixture();
    let mut ui = Ui::open(cx, before.clone(), BTreeMap::new());
    ui.click(ruler_point(&ui, Time(0)), gpui::Modifiers::default());
    ui.press(key);
    assert_eq!(ui.read(|e| e.tool), tool);
    ui.update(|e, window, cx| {
        let c = e.find_clip(ClipId(2)).unwrap().clone();
        let delta = if tool == Tool::RateStretch { -100 } else { 100 };
        e.gesture = Some(TimelineGesture::Trim(TrimDrag {
            tool,
            pointer_origin: c.range.end,
            clip_id: c.id,
            edge: TrimEdge::Right,
            origin_start: c.range.start,
            origin_end: c.range.end,
            origin_source_offset: c.source_offset,
            current: Time(c.range.end.0 + delta),
            ripple: false,
        }));
        assert_ne!(e.edit_tool_feedback(), "");
        e.on_timeline_mouse_up(&MouseUpEvent::default(), window, cx);
    });
    assert_ne!(ui.read(|e| e.project.clone()), before);
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

#[gpui::test]
fn rate_stretch_tool(cx: &mut TestAppContext) {
    additional_tool_scenario(cx, "r", Tool::RateStretch);
}
#[gpui::test]
fn slip_tool(cx: &mut TestAppContext) {
    additional_tool_scenario(cx, "y", Tool::Slip);
}
#[gpui::test]
fn slide_tool(cx: &mut TestAppContext) {
    additional_tool_scenario(cx, "u", Tool::Slide);
}
#[gpui::test]
fn rolling_tool(cx: &mut TestAppContext) {
    additional_tool_scenario(cx, "n", Tool::Rolling);
}
