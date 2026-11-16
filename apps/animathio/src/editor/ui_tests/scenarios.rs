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
fn library_import_at_an_occupied_playhead_leaves_the_timeline_alone(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let before = project.clone();
    let mut ui = Ui::open(cx, project, assets);
    let bytes = wav(&[(0.25, 1200)]);
    let path = super::config_dir().join("library-import-occupied.wav");
    std::fs::write(&path, &bytes).unwrap();
    // The Import media picker uses this library-only path, even at time zero.
    ui.update(|editor, _, cx| editor.import_paths(&[path], None, cx));
    ui.read(|editor| {
        assert_eq!(editor.project.tracks(), before.tracks());
        assert!(
            editor
                .project
                .assets()
                .contains_key(&animathio_core::AssetId::of(&bytes))
        );
    });
    let imported = ui.read(|editor| editor.project.clone());
    ui.dispatch(Undo);
    assert_eq!(ui.read(|editor| editor.project.clone()), before);
    ui.dispatch(Redo);
    assert_eq!(ui.read(|editor| editor.project.clone()), imported);
}

#[gpui::test]
fn launcher_library_import_creates_a_drop_target_in_one_undo_step(cx: &mut TestAppContext) {
    let mut ui = Ui::open(cx, Project::default(), BTreeMap::new());
    let before = ui.read(|editor| editor.project.clone());
    let path = super::config_dir().join("library-import-launcher.wav");
    std::fs::write(&path, wav(&[(0.25, 1000)])).unwrap();
    ui.update(|editor, _, cx| editor.import_paths(&[path], None, cx));
    ui.read(|editor| {
        assert!(!editor.is_start_screen());
        assert_eq!(editor.project.assets().len(), 1);
        assert_eq!(editor.project.tracks().len(), 1);
        assert_eq!(clip_count(editor), 0);
    });
    ui.dispatch(Undo);
    assert_eq!(ui.read(|editor| editor.project.clone()), before);
}

#[gpui::test]
fn add_track_button_is_reachable_at_720p_and_undoable(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let before = project.clone();
    let mut ui = Ui::open(cx, project, assets);
    ui.cx
        .simulate_resize(gpui::size(gpui::px(1280.), gpui::px(720.)));
    ui.settle();
    let bounds = ui
        .cx
        .debug_bounds("timeline-add-track")
        .expect("visible Add track button");
    assert!(bounds.bottom() <= gpui::px(720.));
    assert!(bounds.right() <= gpui::px(1280.));
    ui.click(bounds.center(), gpui::Modifiers::default());
    ui.read(|editor| {
        assert_eq!(editor.project.tracks().len(), before.tracks().len() + 1);
        assert!(editor.project.tracks().last().unwrap().clips().is_empty());
        assert_eq!(
            &editor.project.tracks()[..before.tracks().len()],
            before.tracks()
        );
    });
    let added = ui.read(|editor| editor.project.clone());
    ui.dispatch(Undo);
    assert_eq!(ui.read(|editor| editor.project.clone()), before);
    ui.dispatch(Redo);
    assert_eq!(ui.read(|editor| editor.project.clone()), added);
}

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
fn preferences_enter_saves_and_invalid_input_keeps_the_dialog_open(cx: &mut TestAppContext) {
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
    ui.read(|e| {
        assert!(
            timeline::display_tracks(&e.project)[0]
                .clips()
                .contains_key(&e.selected.unwrap())
        );
    });
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

#[gpui::test]
fn rate_stretch_at_the_top_corner_does_not_drag_a_fade(cx: &mut TestAppContext) {
    let (project, assets) = voice_pair();
    let before = project.clone();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|editor, _, cx| editor.select_only(PAIR_FIRST, cx));
    ui.click(ruler_point(&ui, Time(0)), gpui::Modifiers::default());
    ui.press("r");
    let corner = ui.read(|editor| {
        let layout = editor.current_layout();
        let end = editor.find_clip(PAIR_FIRST).unwrap().range.end;
        gpui::point(
            editor.timeline_bounds.origin.x + gpui::px(timeline::time_to_x(&layout, end) - 3.),
            editor.timeline_bounds.origin.y
                + gpui::px(layout.ruler_height + 3. + super::super::FADE_HANDLE_INSET),
        )
    });
    ui.cx
        .simulate_mouse_down(corner, gpui::MouseButton::Left, gpui::Modifiers::default());
    ui.settle();
    assert!(ui.read(|editor| matches!(
        editor.gesture,
        Some(TimelineGesture::Trim(TrimDrag {
            tool: Tool::RateStretch,
            ..
        }))
    )));
    let destination = corner - gpui::point(gpui::px(30.), gpui::px(0.));
    ui.cx.simulate_mouse_move(
        destination,
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    ui.cx.simulate_mouse_up(
        destination,
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    ui.settle();
    ui.read(|editor| {
        let clip = editor.find_clip(PAIR_FIRST).unwrap();
        assert!(clip.time_map.speed.percent() > 100.);
        assert!(clip.animations.is_empty(), "no accidental fade keyframes");
    });
    ui.dispatch(Undo);
    assert_eq!(ui.read(|editor| editor.project.clone()), before);
    ui.dispatch(Redo);
    assert!(
        ui.read(|editor| editor
            .find_clip(PAIR_FIRST)
            .unwrap()
            .time_map
            .speed
            .percent())
            > 100.
    );
}

#[gpui::test]
fn auto_duck_does_not_default_to_a_selected_image_as_the_voice(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let image = project
        .tracks()
        .iter()
        .flat_map(|track| track.clips().values())
        .find(|clip| matches!(clip.kind, animathio_core::ClipKind::Image(_)))
        .unwrap()
        .id;
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|editor, _, cx| editor.select_only(image, cx));
    ui.dispatch(actions::AutoDuck);
    ui.read(|editor| {
        assert_eq!(
            editor.duck_review.as_ref().unwrap().params.voice_clips,
            Vec::<ClipId>::new()
        );
    });
}

#[gpui::test]
fn auto_duck_can_be_scrolled_and_cancelled_at_720p(cx: &mut TestAppContext) {
    let (project, assets) = voice_and_music();
    let mut ui = Ui::open(cx, project, assets);
    ui.cx
        .simulate_resize(gpui::size(gpui::px(1280.), gpui::px(720.)));
    ui.dispatch(actions::AutoDuck);
    let panel = ui.cx.debug_bounds("duck-review").unwrap();
    assert!(panel.bottom() <= gpui::px(720.));
    ui.cx.simulate_event(gpui::ScrollEvent {
        position: panel.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-1600.))),
        ..Default::default()
    });
    ui.settle();
    let cancel = ui.cx.debug_bounds("duck-cancel").unwrap();
    assert!(cancel.top() >= panel.top());
    assert!(cancel.bottom() <= panel.bottom());
    ui.click(cancel.center(), gpui::Modifiers::default());
    assert!(ui.read(|editor| editor.duck_review.is_none()));
}

#[gpui::test]
fn learn_noise_in_out_apply_and_undo(cx: &mut TestAppContext) {
    let (mut project, assets) = voice_and_music();
    Command::AddAudioEffect {
        chain: AudioChain::Clip(MUSIC_CLIP),
        index: 0,
        effect: EffectInstance {
            id: EffectId(777),
            plugin: "org.animathio.audio-basic".into(),
            effect: "denoise".into(),
            params: BTreeMap::new(),
            bypass: false,
        },
    }
    .apply(&mut project)
    .unwrap();
    let before = project.clone();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| {
        e.select_only(MUSIC_CLIP, cx);
        e.marks.set_in(Time(0));
        e.marks.set_out(Time(100 * MS));
    });
    ui.dispatch(actions::LearnNoise);
    ui.read(|e| {
        assert_eq!(
            e.project
                .tracks()
                .iter()
                .flat_map(|t| t.clips().values())
                .find(|c| c.id == MUSIC_CLIP)
                .unwrap()
                .audio_effects[0]
                .params["noise_profile"],
            1.0
        );
    });
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
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
    use animathio_core::{Asset, AssetId, Clip, ClipKind, TICKS_PER_SECOND, TimeRange, TrackId};
    let bytes = include_bytes!("../../../../../crates/control/tests/scenes.mp4").to_vec();
    let asset = AssetId::of(&bytes);
    let mut project = Project::default();
    Command::AddAsset {
        id: asset.clone(),
        asset: Asset {
            name: "scenes.mp4".into(),
            media_type: "video/mp4".into(),
        },
    }
    .apply(&mut project)
    .unwrap();
    Command::AddTrack {
        track: Track::new(TrackId(1), "Video"),
        index: 0,
    }
    .apply(&mut project)
    .unwrap();
    Command::AddClip {
        track: TrackId(1),
        clip: Clip::new(
            ClipId(1),
            ClipKind::Video(asset.clone()),
            TimeRange::new(Time(0), Time(4 * TICKS_PER_SECOND)).unwrap(),
        ),
    }
    .apply(&mut project)
    .unwrap();
    let before = project.clone();
    let mut ui = Ui::open(cx, project, BTreeMap::from([(asset, bytes)]));
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
fn foreground_rows_match_hits_drops_selection_and_track_moves(cx: &mut TestAppContext) {
    let (mut project, assets) = voice_and_music();
    for (index, id) in [3, 4, 5].into_iter().enumerate() {
        Command::AddTrack {
            track: Track::new(animathio_core::TrackId(id), format!("V{id}")),
            index: index + 2,
        }
        .apply(&mut project)
        .unwrap();
        Command::AddClip {
            track: animathio_core::TrackId(id),
            clip: animathio_core::Clip::new(
                ClipId(id + 20),
                animathio_core::ClipKind::Error("visual".into()),
                animathio_core::TimeRange::new(Time(0), Time(1000 * MS)).unwrap(),
            ),
        }
        .apply(&mut project)
        .unwrap();
    }
    let before = project.clone();
    let mut ui = Ui::open(cx, project, assets);
    ui.read(|e| {
        let ids: Vec<_> = timeline::display_tracks(&e.project)
            .iter()
            .map(|t| t.id().0)
            .collect();
        assert_eq!(ids, [5, 4, 3, 1, 2]);
        let layout = e.current_layout();
        let y = layout.ruler_height + layout.row_height / 2.;
        assert_eq!(
            timeline::hit_test(&layout, &e.project, 10., y).map(|hit| hit.clip),
            Some(ClipId(25))
        );
        assert_eq!(
            timeline::drop_placement(&layout, &e.project, 10., y, Time(0), false)
                .unwrap()
                .0,
            animathio_core::TrackId(5)
        );
        assert_eq!(
            crate::selection::marquee(&layout, &e.project, (0., layout.ruler_height), (20., y)),
            BTreeSet::from([ClipId(25)])
        );
    });
    ui.update(|e, _, cx| e.move_track_to_row(animathio_core::TrackId(3), 0, cx));
    assert_eq!(
        ui.read(|e| timeline::display_tracks(&e.project)[0].id().0),
        3
    );
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
    ui.update(|e, _, cx| e.move_track_by(animathio_core::TrackId(5), 1, cx));
    assert_eq!(
        ui.read(|e| timeline::display_tracks(&e.project)[1].id().0),
        5
    );
    ui.dispatch(Undo);
    ui.update(|e, _, cx| e.add_timeline_track(cx));
    assert_eq!(
        ui.read(|e| timeline::display_tracks(&e.project)[0].id().0),
        6
    );
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

#[gpui::test]
fn ruler_and_marker_lane_stay_fixed_when_tracks_scroll(cx: &mut TestAppContext) {
    let mut project = Project::default();
    for id in 1..=12 {
        Command::AddTrack {
            track: Track::new(animathio_core::TrackId(id), format!("V{id}")),
            index: usize::try_from(id - 1).unwrap(),
        }
        .apply(&mut project)
        .unwrap();
        Command::AddClip {
            track: animathio_core::TrackId(id),
            clip: animathio_core::Clip::new(
                ClipId(id + 20),
                animathio_core::ClipKind::Error("visual".into()),
                animathio_core::TimeRange::new(Time(0), Time(2000 * MS)).unwrap(),
            ),
        }
        .apply(&mut project)
        .unwrap();
    }
    let mut ui = Ui::open(cx, project, BTreeMap::new());
    seek(&mut ui, 1000);
    ui.dispatch(AddMarker);
    let marker = ui.read(|e| e.project.markers()[0].id);
    let ruler_before = ruler_point(&ui, Time(1000 * MS));
    let top_before = ui.read(|e| e.timeline_tracks_top);
    ui.update(|e, _, cx| {
        e.timeline_scroll_handle.set_offset(gpui::point(
            gpui::px(0.),
            gpui::px(-e.dock.track_height.pixels() * 3.),
        ));
        cx.notify();
    });
    let ruler_after = ruler_point(&ui, Time(1000 * MS));
    assert_eq!(ruler_before, ruler_after);
    assert!(ui.read(|e| e.timeline_tracks_top) < top_before - 1.);
    ui.click(ruler_after, gpui::Modifiers::default());
    assert_eq!(
        ui.read(|e| e.marker_selection.clone()),
        BTreeSet::from([marker])
    );
    let (point, expected) = ui.read(|e| {
        let layout = e.current_layout();
        let point = gpui::point(
            e.timeline_bounds.origin.x + gpui::px(timeline::time_to_x(&layout, Time(500 * MS))),
            e.timeline_bounds.origin.y + gpui::px(layout.ruler_height + layout.row_height / 2.),
        );
        let row = ((f32::from(point.y) - e.timeline_tracks_top) / layout.row_height) as usize;
        (
            point,
            timeline::display_tracks(&e.project)[row]
                .clips()
                .values()
                .next()
                .unwrap()
                .id,
        )
    });
    assert_eq!(expected, ClipId(29), "three rows scrolled: V9 is now first");
    ui.click(point, gpui::Modifiers::default());
    assert_eq!(ui.read(|e| e.selected), Some(expected));
    ui.read(|e| {
        let (x, y) = e.timeline_relative(point);
        let layout = e.current_layout();
        let hit = timeline::hit_test(&layout, &e.project, x, y).unwrap();
        assert_eq!(hit.clip, expected);
        assert_eq!(
            timeline::drop_placement(&layout, &e.project, x, y, e.position, false)
                .unwrap()
                .0,
            hit.track
        );
    });
}

fn dialog_ready(ui: &mut Ui) {
    ui.advance_clock(std::time::Duration::from_millis(400));
}

fn edit_dialog_field(ui: &mut Ui, selector: &'static str, value: &str) {
    let bounds = ui
        .cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    ui.click(bounds.center(), gpui::Modifiers::default());
    ui.press("secondary-a");
    ui.press("backspace");
    if !value.is_empty() {
        ui.cx.simulate_input(value);
    }
    ui.settle();
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
fn speed_and_hold_dialogs_retain_invalid_input_and_apply_enter(cx: &mut TestAppContext) {
    let (project, assets) = voice_pair();
    let before = project.clone();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, window, cx| {
        e.select_only(PAIR_FIRST, cx);
        e.open_speed_duration(window, cx);
    });
    dialog_ready(&mut ui);
    edit_dialog_field(&mut ui, "speed-percent", "0");
    ui.press("enter");
    assert_eq!(ui.read(|e| e.project.clone()), before);
    assert!(ui.cx.debug_bounds("speed-percent").is_some());
    edit_dialog_field(&mut ui, "speed-percent", "200");
    ui.press("enter");
    dialog_ready(&mut ui);
    assert!(ui.cx.debug_bounds("speed-percent").is_none());
    assert!(
        (ui.read(|e| e.find_clip(PAIR_FIRST).unwrap().time_map.speed.percent()) - 200.).abs()
            < f64::EPSILON
    );
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
    ui.update(|e, window, cx| {
        e.seek_to(Time(500 * MS), cx);
        e.open_frame_hold(window, cx);
    });
    dialog_ready(&mut ui);
    edit_dialog_field(&mut ui, "hold-frames", "0");
    ui.press("enter");
    assert!(ui.cx.debug_bounds("hold-frames").is_some());
    assert_eq!(ui.read(|e| e.project.clone()), before);
    edit_dialog_field(&mut ui, "hold-frames", "15");
    ui.press("enter");
    dialog_ready(&mut ui);
    assert!(ui.cx.debug_bounds("hold-frames").is_none());
    assert_ne!(ui.read(|e| e.project.clone()), before);
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
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

#[gpui::test]
fn write_on_dialog_validates_duration_before_enter_applies(cx: &mut TestAppContext) {
    let mut project = Project::default();
    Command::AddTrack {
        track: Track::new(animathio_core::TrackId(1), "Math"),
        index: 0,
    }
    .apply(&mut project)
    .unwrap();
    Command::AddClip {
        track: animathio_core::TrackId(1),
        clip: animathio_core::Clip::new(
            ClipId(1),
            animathio_core::ClipKind::Plugin {
                plugin_id: "org.animathio.latex".into(),
                params: serde_json::json!({"source":"x"}),
            },
            animathio_core::TimeRange::new(Time(0), Time(2000 * MS)).unwrap(),
        ),
    }
    .apply(&mut project)
    .unwrap();
    let before = project.clone();
    let mut ui = Ui::open(cx, project, BTreeMap::new());
    ui.update(|e, window, cx| {
        e.select_only(ClipId(1), cx);
        e.open_write_on(window, cx);
    });
    dialog_ready(&mut ui);
    edit_dialog_field(&mut ui, "write-duration", "0");
    ui.press("enter");
    assert!(ui.cx.debug_bounds("write-duration").is_some());
    assert_eq!(ui.read(|e| e.project.clone()), before);
    edit_dialog_field(&mut ui, "write-duration", "1");
    ui.press("enter");
    dialog_ready(&mut ui);
    assert!(ui.cx.debug_bounds("write-duration").is_none());
    assert!(!ui.read(|e| e.find_clip(ClipId(1)).unwrap().animations.is_empty()));
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
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

#[gpui::test]
fn denoise_learned_bands_are_collapsed_and_can_be_expanded(cx: &mut TestAppContext) {
    let (mut project, assets) = voice_and_music();
    let mut params: BTreeMap<_, _> = (0..16).map(|i| (format!("noise_{i}"), 0.0001)).collect();
    params.insert("reduction_db".into(), 12.);
    Command::AddAudioEffect {
        chain: AudioChain::Clip(VOICE_CLIP),
        index: 0,
        effect: EffectInstance {
            id: EffectId(777),
            plugin: "org.animathio.audio-basic".into(),
            effect: "denoise".into(),
            bypass: false,
            params,
        },
    }
    .apply(&mut project)
    .unwrap();
    let before = project.clone();
    let mut ui = Ui::open(cx, project, assets);
    ui.cx
        .simulate_resize(gpui::size(gpui::px(2560.), gpui::px(1440.)));
    ui.update(|e, _, cx| {
        e.dock.right_open = true;
        e.closed_sections.insert("speed".into());
        e.select_only(VOICE_CLIP, cx);
        cx.notify();
    });
    assert!(ui.read(|e| e.audio_advanced.is_empty()));
    assert!(
        ui.cx
            .debug_bounds("audio-param-Clip(ClipId(10)):ae777:noise_0")
            .is_none()
    );
    let header = ui
        .cx
        .debug_bounds("audio-advanced-Clip(ClipId(10))-777")
        .unwrap();
    ui.click(header.center(), gpui::Modifiers::default());
    dialog_ready(&mut ui);
    assert!(ui.read(|e| {
        e.audio_advanced
            .contains(&(AudioChain::Clip(VOICE_CLIP), EffectId(777)))
    }));
    assert!(
        ui.cx
            .debug_bounds("audio-param-Clip(ClipId(10)):ae777:noise_0")
            .is_some()
    );
    assert!(
        ui.cx
            .debug_bounds("audio-param-Clip(ClipId(10)):ae777:noise_15")
            .is_some()
    );
    let header = ui
        .cx
        .debug_bounds("audio-advanced-Clip(ClipId(10))-777")
        .unwrap();
    ui.click(header.center(), gpui::Modifiers::default());
    dialog_ready(&mut ui);
    assert!(
        ui.cx
            .debug_bounds("audio-param-Clip(ClipId(10)):ae777:noise_0")
            .is_none()
    );
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

#[gpui::test]
fn auto_duck_clip_targets_show_asset_track_and_time_and_are_clickable(cx: &mut TestAppContext) {
    let (project, assets) = voice_and_music();
    let before = project.clone();
    let track = project
        .tracks()
        .iter()
        .find(|t| t.id() == MUSIC_TRACK)
        .unwrap();
    let label =
        super::super::ducking::duck_clip_label(&project, track, &track.clips()[&MUSIC_CLIP]);
    assert_eq!(label, "music.wav / Music (00:00:00:00)");
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| {
        e.select_only(VOICE_CLIP, cx);
        e.open_duck(cx);
    });
    let target = ui.cx.debug_bounds("duck-target-11").unwrap();
    ui.click(target.center(), gpui::Modifiers::default());
    assert!(ui.read(|e| {
        e.duck_review
            .as_ref()
            .unwrap()
            .params
            .music_clips
            .contains(&MUSIC_CLIP)
    }));
    ui.update(|e, _, cx| {
        e.duck_review = None;
        cx.notify();
    });
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

#[gpui::test]
fn math_dialog_enter_submits_from_the_multiline_source_and_escape_cancels(cx: &mut TestAppContext) {
    if !require_plugins("math_dialog_enter_submits_from_the_multiline_source_and_escape_cancels") {
        return;
    }
    let (project, assets) = sample();
    let before = project.clone();
    let mut ui = Ui::open(cx, project, assets);
    let count = ui.read(clip_count);
    ui.update(|e, window, cx| e.open_math(MathObjectKind::Latex, None, window, cx));
    dialog_ready(&mut ui);
    ui.press("enter");
    dialog_ready(&mut ui);
    assert_eq!(ui.read(clip_count), count + 1);
    assert!(!ui.cx.update(|window, cx| {
        use gpui_component::WindowExt;
        window.has_active_dialog(cx)
    }));
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
    ui.update(|e, window, cx| e.open_math(MathObjectKind::Latex, None, window, cx));
    dialog_ready(&mut ui);
    ui.press("escape");
    dialog_ready(&mut ui);
    assert!(!ui.cx.update(|window, cx| {
        use gpui_component::WindowExt;
        window.has_active_dialog(cx)
    }));
    assert_eq!(ui.read(|e| e.project.clone()), before);
}
