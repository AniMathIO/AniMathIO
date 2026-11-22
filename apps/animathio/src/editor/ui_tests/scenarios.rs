//! Scenarios that drive real actions and assert on model and view state.

use crate::editing;
use crate::editor::{TimelineGesture, TrimDrag};
use crate::timeline::TrimEdge;
use gpui::{AppContext, AsKeystroke, Entity, Focusable as _, MouseUpEvent};

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
use crate::editor::assist::AssistPanel;
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

/// The launcher shows no canvas yet, so the status bar hides the placeholder
/// resolution/duration chips until a project exists.
#[gpui::test]
fn launcher_status_bar_hides_placeholder_resolution_and_duration(cx: &mut TestAppContext) {
    let mut ui = Ui::open(cx, Project::default(), BTreeMap::new());
    assert!(ui.read(Editor::is_start_screen));
    ui.read(|editor| assert!(!editor.status_summary_visible()));
    ui.dispatch(NewProject);
    ui.press("enter");
    ui.read(|editor| {
        assert!(!editor.is_start_screen());
        assert!(editor.status_summary_visible());
        assert_eq!(editor.project.tracks().len(), 1);
    });
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
fn clipped_slide_options_choose_the_edge_and_apply_as_one_undoable_batch(cx: &mut TestAppContext) {
    if !require_plugins("clipped_slide_options") {
        return;
    }
    let (project, assets) = sample();
    let before = project.clone();
    let mut ui = Ui::open(cx, project, assets);
    let id = ui.read(|e| e.project.tracks()[0].clips().values().next().unwrap().id);
    ui.update(|e, _, cx| {
        e.select_only(id, cx);
        // The inspector and the Effects tab both render the shared option buttons.
        assert!(!e.animation_rows(id, cx).is_empty());
        assert!(!e.animation_library_rows(cx).is_empty());
        let presets = e.available_animation_presets();
        let fitting: Vec<_> = presets.iter().collect();
        let controls = crate::editor::animations::option_controls(&fitting);
        assert_eq!(controls[0].id(), "direction");
        // default -> left -> right -> top
        let direction = controls[0].clone();
        assert!(e.option_label(&direction).ends_with("Default"));
        e.cycle_preset_option(&direction, cx);
        assert!(e.option_label(&direction).ends_with("Left"));
        e.cycle_preset_option(&direction, cx);
        e.cycle_preset_option(&direction, cx);
        assert!(e.option_label(&direction).ends_with("Top"));
        e.apply_animation_preset("org.animathio.animations", "clipped-slide-in", cx);
    });
    let direction_keys = ui.read(|e| {
        let clip = e.find_clip(id).unwrap();
        let effect = clip
            .effects
            .iter()
            .find(|f| f.effect == "slide")
            .expect("slide effect added")
            .id;
        clip.animations
            .iter()
            .find(|a| {
                a.target
                    == animathio_core::Target::Effect {
                        effect,
                        param: "direction".into(),
                    }
            })
            .unwrap()
            .keys
            .keys()
            .values()
            .map(|k| k.value.as_number().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(direction_keys, [3.0, 3.0], "direction top");
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

fn music_with_denoise() -> (Project, BTreeMap<animathio_core::AssetId, Vec<u8>>) {
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
    (project, assets)
}

fn denoise_params(editor: &Editor) -> BTreeMap<String, f64> {
    editor
        .project
        .tracks()
        .iter()
        .flat_map(|t| t.clips().values())
        .find(|c| c.id == MUSIC_CLIP)
        .unwrap()
        .audio_effects[0]
        .params
        .clone()
}

fn range_marker(id: u64, start_ms: i64, length_ms: i64) -> Command {
    let mut marker = animathio_core::Marker::new(MarkerId(id), Time(start_ms * MS));
    marker.duration = Some(Time(length_ms * MS));
    Command::AddMarker { marker }
}

#[gpui::test]
fn learn_noise_uses_a_selected_range_marker_trimmed_to_the_clip(cx: &mut TestAppContext) {
    let (project, assets) = music_with_denoise();
    let mut ui = Ui::open(cx, project, assets);
    // The marker reaches 500 ms past the 3 s clip: only the part inside is analysed.
    ui.update(|e, _, cx| {
        e.execute(&range_marker(5, 2000, 1500), cx);
        e.select_only(MUSIC_CLIP, cx);
        e.marker_selection = BTreeSet::from([MarkerId(5)]);
    });
    let before = ui.read(|e| e.project.clone());
    ui.dispatch(actions::LearnNoise);
    let params = ui.read(denoise_params);
    assert_eq!(params["noise_profile"], 1.0);
    assert!((0..16).all(|i| params.contains_key(&format!("noise_{i}"))));
    // One undo step restores the adaptive (profile-less) denoise.
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

#[gpui::test]
fn learn_noise_prefers_in_out_over_a_marker_and_needs_a_clean_selection(cx: &mut TestAppContext) {
    let (project, assets) = music_with_denoise();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| {
        e.execute(&range_marker(5, 0, 1000), cx);
        e.execute(&range_marker(6, 1000, 1000), cx);
        e.select_only(MUSIC_CLIP, cx);
        // Two range markers selected is ambiguous: nothing to learn from.
        e.marker_selection = BTreeSet::from([MarkerId(5), MarkerId(6)]);
    });
    let before = ui.read(|e| e.project.clone());
    ui.dispatch(actions::LearnNoise);
    assert_eq!(ui.read(|e| e.project.clone()), before);
    assert!(ui.read(|e| {
        e.last_error
            .as_deref()
            .is_some_and(|error| error.contains("Set In and Out"))
    }));
    // In/Out wins over the marker selection.
    ui.update(|e, _, _| {
        e.marks.set_in(Time(0));
        e.marks.set_out(Time(100 * MS));
    });
    ui.dispatch(actions::LearnNoise);
    assert_eq!(ui.read(denoise_params)["noise_profile"], 1.0);
}

#[gpui::test]
fn learn_noise_outside_the_clip_or_without_denoise_changes_nothing(cx: &mut TestAppContext) {
    let (mut project, assets) = music_with_denoise();
    // The music clip now spans 2 s..5 s; the selection below is in the project, not the clip.
    Command::MoveClip {
        id: MUSIC_CLIP,
        track: MUSIC_TRACK,
        start: Time(2000 * MS),
    }
    .apply(&mut project)
    .unwrap();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| {
        e.select_only(VOICE_CLIP, cx);
        e.marks.set_in(Time(0));
        e.marks.set_out(Time(100 * MS));
    });
    let before = ui.read(|e| e.project.clone());
    ui.dispatch(actions::LearnNoise);
    assert_eq!(ui.read(|e| e.project.clone()), before, "no denoise effect");
    assert!(ui.read(|e| e.last_error.as_deref() == Some("Add a denoise effect first")));
    ui.update(|e, _, cx| {
        e.select_only(MUSIC_CLIP, cx);
        e.marks.set_in(Time(0));
        e.marks.set_out(Time(1000 * MS));
        e.last_error = None;
    });
    ui.dispatch(actions::LearnNoise);
    assert_eq!(
        ui.read(|e| e.project.clone()),
        before,
        "selection outside clip"
    );
    assert_eq!(
        ui.read(|e| e.last_error.clone()).as_deref(),
        Some("The selection is outside the clip")
    );
}

#[gpui::test]
fn denoise_card_learn_button_applies_the_in_out_selection(cx: &mut TestAppContext) {
    let (project, assets) = music_with_denoise();
    let mut ui = Ui::open(cx, project, assets);
    ui.cx
        .simulate_resize(gpui::size(gpui::px(2560.), gpui::px(1440.)));
    ui.update(|e, _, cx| {
        e.dock.right_open = true;
        e.closed_sections.insert("speed".into());
        e.select_only(MUSIC_CLIP, cx);
        e.marks.set_in(Time(0));
        e.marks.set_out(Time(200 * MS));
        cx.notify();
    });
    let button = ui
        .cx
        .debug_bounds("audio-learn-777")
        .expect("denoise card shows the learn button");
    ui.click(button.center(), gpui::Modifiers::default());
    assert_eq!(ui.read(denoise_params)["noise_profile"], 1.0);
}

#[gpui::test]
fn auto_duck_suggests_the_other_audio_tracks_and_previews_before_applying(cx: &mut TestAppContext) {
    let (project, assets) = voice_and_music();
    let before = project.clone();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| {
        e.select_only(VOICE_CLIP, cx);
        e.open_duck(cx);
    });
    ui.settle();
    ui.read(|e| {
        let review = e.duck_review.as_ref().unwrap();
        // The voice clip's own track is never suggested as music.
        assert_eq!(review.params.voice_clips, vec![VOICE_CLIP]);
        assert_eq!(review.params.music_tracks, vec![MUSIC_TRACK]);
        let proposal = review.proposal.as_ref().expect("analysed on open");
        let summary = super::super::ducking::duck_summary(&review.params, proposal, &e.project);
        assert!(
            summary.contains("under 2 voice regions") && summary.contains("one undo step"),
            "{summary}"
        );
    });
    assert!(ui.cx.debug_bounds("duck-summary").is_some());
    // Attack, release, gap and threshold sit behind one toggle so the preview stays in view.
    assert!(!ui.read(|e| e.duck_review.as_ref().unwrap().timing_open));
    let timing = ui.cx.debug_bounds("duck-timing").unwrap();
    ui.click(timing.center(), gpui::Modifiers::default());
    assert!(ui.read(|e| e.duck_review.as_ref().unwrap().timing_open));
    // Nothing is written until Apply, and a single Undo reverts it.
    assert_eq!(ui.read(|e| e.project.clone()), before);
    ui.update(|e, _, cx| e.apply_duck(cx));
    assert_ne!(ui.read(|e| e.project.clone()), before);
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

#[gpui::test]
fn auto_duck_does_not_suggest_locked_music_tracks(cx: &mut TestAppContext) {
    let (mut project, assets) = voice_and_music();
    Command::SetTrackLocked {
        id: MUSIC_TRACK,
        locked: true,
    }
    .apply(&mut project)
    .unwrap();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| {
        e.select_only(VOICE_CLIP, cx);
        e.open_duck(cx);
    });
    ui.read(|e| {
        let review = e.duck_review.as_ref().unwrap();
        assert_eq!(
            review.params.music_tracks,
            Vec::<animathio_core::TrackId>::new()
        );
        assert!(review.proposal.is_none());
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

/// A project with one four-second video clip of the three-shot test movie, and its bytes.
pub(super) fn scenes_fixture() -> (Project, BTreeMap<animathio_core::AssetId, Vec<u8>>) {
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
    (project, BTreeMap::from([(asset, bytes)]))
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

/// Exercise the real rendered Library and its scoped input subscriptions.
#[gpui::test]
fn library_tabs_have_distinct_contents_and_scoped_search(cx: &mut TestAppContext) {
    if !require_plugins("library_tabs_have_distinct_contents_and_scoped_search") {
        return;
    }
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.dispatch(actions::FocusLibrary);
    for (tab, selector) in [
        (LeftTab::Media, "media-kind-filters"),
        (LeftTab::Elements, "element-card-0"),
        (LeftTab::Titles, "title-centered_title"),
        (LeftTab::Effects, "library-write-on"),
        (LeftTab::Plugins, "plugin-inventory"),
    ] {
        ui.update(|e, _, cx| e.select_left_tab(tab, cx));
        assert!(
            ui.cx.debug_bounds(selector).is_some(),
            "missing content for {}",
            tab.label()
        );
        assert_eq!(
            ui.cx.debug_bounds("media-kind-filters").is_some(),
            tab == LeftTab::Media
        );
        assert_eq!(
            ui.cx.debug_bounds("element-card-0").is_some(),
            tab == LeftTab::Elements
        );
        assert_eq!(
            ui.cx.debug_bounds("title-centered_title").is_some(),
            tab == LeftTab::Titles
        );
        assert_eq!(
            ui.cx.debug_bounds("plugin-inventory").is_some(),
            tab == LeftTab::Plugins
        );
        assert_eq!(
            ui.cx.debug_bounds("library-write-on").is_some(),
            tab == LeftTab::Effects
        );
    }
    search_library(&mut ui, LeftTab::Elements, "latex");
    assert!(ui.cx.debug_bounds("element-card-0").is_some());
    assert!(ui.cx.debug_bounds("element-card-1").is_none());
    assert_eq!(ui.read(|e| e.element_kinds().len()), 1);
    search_library(&mut ui, LeftTab::Titles, "caption");
    assert!(ui.cx.debug_bounds("title-caption").is_some());
    assert!(ui.cx.debug_bounds("title-centered_title").is_none());
    // Transitions live in Effects and are filtered by its search like everything else.
    ui.update(|e, _, cx| e.select_left_tab(LeftTab::Effects, cx));
    for key in ["video", "audio", "both"] {
        assert!(bounds_of(&mut ui, format!("library-transition-{key}")).is_some());
    }
    search_library(&mut ui, LeftTab::Effects, "invert");
    assert!(ui.cx.debug_bounds("library-transition-video").is_none());
    assert!(
        ui.cx
            .debug_bounds("effect-item-org.animathio.effects-basic-invert")
            .is_some()
    );
    assert!(ui.cx.debug_bounds("library-write-on").is_none());
    assert!(ui.cx.debug_bounds("effects-empty").is_none());
    // A search that hides everything says so instead of claiming no plugins are loaded.
    search_library(&mut ui, LeftTab::Effects, "no such effect");
    assert!(ui.cx.debug_bounds("effects-empty").is_some());
    assert!(ui.cx.debug_bounds("library-transition-video").is_none());
    search_library(&mut ui, LeftTab::Effects, "write on");
    assert!(ui.cx.debug_bounds("library-write-on").is_some());
    assert!(ui.cx.debug_bounds("effects-empty").is_none());
    search_library(&mut ui, LeftTab::Effects, "invert");
    search_library(&mut ui, LeftTab::Plugins, "import-manim");
    assert!(
        ui.cx
            .debug_bounds("installed-org.animathio.import-manim")
            .is_some()
    );
    assert!(ui.cx.debug_bounds("installed-org.animathio.text").is_none());
    search_library(&mut ui, LeftTab::Media, "independent media query");
    ui.update(|e, _, cx| e.select_left_tab(LeftTab::Elements, cx));
    assert_eq!(
        ui.read(|e| e.media_view.queries[LeftTab::Elements as usize].clone()),
        "latex"
    );
    assert!(ui.cx.debug_bounds("element-card-0").is_some());
    assert!(ui.cx.debug_bounds("element-card-1").is_none());
    assert!(ui.cx.debug_bounds("media-kind-filters").is_none());
}

/// `debug_bounds` wants a `'static` selector; the leak is a few bytes in a test.
fn bounds_of(ui: &mut Ui, selector: String) -> Option<gpui::Bounds<gpui::Pixels>> {
    ui.cx.debug_bounds(Box::leak(selector.into_boxed_str()))
}

fn search_library(ui: &mut Ui, tab: LeftTab, query: &str) {
    ui.update(|e, _, cx| e.select_left_tab(tab, cx));
    ui.update(|e, window, cx| {
        let input = e.media_view.searches[tab as usize]
            .clone()
            .expect("rendered search");
        input.update(cx, |state, cx| state.set_value(query, window, cx));
        // `set_value` does not emit `InputEvent::Change`; mirror what the subscription stores.
        e.media_view.queries[tab as usize] = query.to_owned();
        cx.notify();
    });
}

#[gpui::test]
fn library_plugins_lists_all_eight_kinds_without_duplicate_element_sources(
    cx: &mut TestAppContext,
) {
    if !require_plugins("library_plugins_lists_all_eight_kinds_without_duplicate_element_sources") {
        return;
    }
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.dispatch(actions::FocusLibrary);
    ui.dispatch(actions::ShowPlugins);
    let ids = ui.read(|e| e.plugin_host.as_ref().unwrap().loaded_plugin_ids());
    let expected: Vec<_> = [
        "animations",
        "audio-basic",
        "effects-basic",
        "graph",
        "import-manim",
        "latex",
        "shapes",
        "text",
    ]
    .into_iter()
    .map(|id| format!("org.animathio.{id}"))
    .collect();
    assert_eq!(ids, expected);
    for id in ids {
        search_library(&mut ui, LeftTab::Plugins, &id);
        assert!(
            bounds_of(&mut ui, format!("installed-{id}")).is_some(),
            "missing {id}"
        );
        assert!(ui.read(|e| {
            e.plugin_host
                .as_ref()
                .unwrap()
                .get_plugin(&id)
                .unwrap()
                .source_path
                .is_some()
        }));
    }
    search_library(&mut ui, LeftTab::Elements, "");
    ui.read(|e| {
        assert_eq!(e.element_kinds().len(), 11);
        let sources: Vec<_> = e
            .element_plugin_sources()
            .iter()
            .map(|p| p.id.as_str())
            .collect();
        assert_eq!(sources, ["org.animathio.shapes"]);
    });
    for id in ["text", "latex", "graph"] {
        assert!(bounds_of(&mut ui, format!("element-plugin-org.animathio.{id}")).is_none());
    }
    // A third-party clip source belongs to Elements, not to Titles or Effects.
    ui.update(|e, _, cx| {
        let mut descriptor = e
            .plugins
            .iter()
            .find(|p| p.id == "org.animathio.text")
            .unwrap()
            .clone();
        descriptor.id = "example.generated".into();
        descriptor.name = "Example generator".into();
        e.plugins.push(descriptor);
        cx.notify();
    });
    search_library(&mut ui, LeftTab::Elements, "example");
    assert!(
        ui.cx
            .debug_bounds("element-plugin-example.generated")
            .is_some()
    );
    assert_eq!(ui.read(|e| e.element_plugin_sources().len()), 1);
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

#[gpui::test]
fn library_tab_arrows_move_selection_and_focus(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.dispatch(actions::FocusLibrary);
    ui.update(|e, window, cx| {
        e.select_left_tab(LeftTab::Media, cx);
        window.focus(&e.media_view.tab_focus[0], cx);
    });
    ui.press("right");
    assert!(ui.read(|e| e.left_tab == LeftTab::Elements));
    ui.press("right");
    assert!(ui.read(|e| e.left_tab == LeftTab::Titles));
    ui.press("end");
    assert!(ui.read(|e| e.left_tab == LeftTab::Plugins));
    ui.press("right");
    assert!(ui.read(|e| e.left_tab == LeftTab::Media));
    ui.press("left");
    assert!(ui.read(|e| e.left_tab == LeftTab::Plugins));
    ui.press("home");
    assert!(ui.read(|e| e.left_tab == LeftTab::Media));
    // Tab can focus a tab header without selecting it; arrows start from the focused one.
    ui.update(|e, window, cx| window.focus(&e.media_view.tab_focus[LeftTab::Effects as usize], cx));
    ui.press("right");
    assert!(ui.read(|e| e.left_tab == LeftTab::Plugins));
    ui.update(|e, window, cx| window.focus(&e.media_view.tab_focus[LeftTab::Titles as usize], cx));
    ui.press("left");
    assert!(ui.read(|e| e.left_tab == LeftTab::Elements));
}

/// Community plugins start off. Switching them on and enabling one loads it into the running
/// editor (its clip source appears in the plugin list); turning the switch off unloads it
/// again. No restart, and the headless harness uses a private plugin folder and settings.
#[gpui::test]
fn community_plugins_load_and_unload_without_restart(cx: &mut TestAppContext) {
    const HELLO: &[u8] =
        include_bytes!("../../../../../crates/plugin-host/tests/fixtures/hello-1.0.0.amplugin");
    const ID: &str = "com.example.hello";
    if !require_plugins("community_plugins_load_and_unload_without_restart") {
        return;
    }
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.settle();
    // A private store of this test: the shared one in the test config directory also holds the
    // `community-plugins.json` that other scenarios (Preferences) write concurrently.
    let root = tempfile::tempdir().unwrap();
    let store = animathio_plugin_host::CommunityStore::new(
        root.path().join("plugins"),
        root.path().join("community-plugins.json"),
    );
    ui.update(|editor, _, _| editor.community.store = Some(store.clone()));
    let has_hello = |ui: &Ui| ui.read(|editor| editor.plugins.iter().any(|plugin| plugin.id == ID));
    assert!(!store.settings().enabled, "community plugins start off");
    assert!(!has_hello(&ui));

    // Installed plugins start disabled, so even a switched-on editor does not load them.
    store.install_bytes(HELLO, None, false).unwrap();
    ui.update(|editor, _, cx| editor.set_community_plugins_enabled(true, cx).unwrap());
    assert!(!has_hello(&ui), "installed but not enabled");

    // Enabling the plugin (the CLI or a future Browse UI) and re-syncing loads it live.
    store.set_plugin_enabled(ID, true).unwrap();
    ui.update(|editor, _, cx| editor.sync_community_plugins(cx));
    assert!(has_hello(&ui), "enabled plugin is loaded without a restart");

    // Switching the global switch off unloads it, and the project is unaffected.
    let before = ui.read(|editor| editor.project.clone());
    ui.update(|editor, _, cx| editor.set_community_plugins_enabled(false, cx).unwrap());
    assert!(!has_hello(&ui), "switch off unloads community plugins");
    assert_eq!(ui.read(|editor| editor.project.clone()), before);
    ui.read(|editor| {
        let host = editor.plugin_host.as_ref().expect("plugin host");
        assert!(host.get_plugin(ID).is_none());
    });

    store.uninstall(ID).unwrap();
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
fn maintain_pitch_inspector_handler_undoes(cx: &mut TestAppContext) {
    let (project, assets) = voice_pair();
    let mut ui = Ui::open(cx, project, assets);
    let before = ui.read(|e| e.project.clone());
    ui.update(|e, _, cx| {
        e.select_only(PAIR_FIRST, cx);
        let _ = e.speed_rows(PAIR_FIRST, cx);
        e.set_maintain_pitch(PAIR_FIRST, false, cx);
    });
    assert!(!ui.read(|e| e.project.tracks()[0].clips()[&PAIR_FIRST].maintain_pitch));
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
    ui.dispatch(Redo);
    assert!(!ui.read(|e| e.project.tracks()[0].clips()[&PAIR_FIRST].maintain_pitch));
}

/// Window position of speed key `index` of `clip`, or of its clip-local `time` on the
/// 100% line of the lane when `index` is `None`.
fn ramp_point(
    ui: &Ui,
    clip: ClipId,
    index: Option<usize>,
    time: Time,
) -> gpui::Point<gpui::Pixels> {
    ui.read(|e| {
        let layout = e.current_layout();
        let (row, c) = e.ramp_target().expect("selected clip");
        assert_eq!(c.id, clip);
        let lane = Editor::ramp_lane(&layout, row).expect("lane");
        // Keys sit on the ramp's own clock, which runs `phase` ticks ahead of the clip.
        let (local, speed) = index.map_or((time, ClipSpeed::default()), |i| {
            (
                Time(c.time_map.ramps[i].time.0 - c.time_map.phase.0),
                c.time_map.ramps[i].speed,
            )
        });
        gpui::point(
            e.timeline_bounds.origin.x
                + gpui::px(timeline::time_to_x(
                    &layout,
                    Time(c.range.start.0 + local.0),
                )),
            e.timeline_bounds.origin.y + gpui::px(lane.y(speed)),
        )
    })
}

#[gpui::test]
fn speed_ramp_handles_drag_add_and_remove_as_single_undo_steps(cx: &mut TestAppContext) {
    use animathio_core::SpeedPoint;
    let (project, assets) = voice_pair();
    let mut ui = Ui::open(cx, project, assets);
    // A two-key ramp from 100% to 200% on the first clip.
    ui.update(|e, _, cx| {
        e.select_only(PAIR_FIRST, cx);
        e.try_execute(
            &Command::SetClipSpeed {
                id: PAIR_FIRST,
                speed: ClipSpeed::default(),
                reverse: false,
                ramps: vec![
                    SpeedPoint {
                        time: Time(0),
                        speed: ClipSpeed::default(),
                    },
                    SpeedPoint {
                        time: Time(500 * MS),
                        speed: ClipSpeed::new(2, 1).unwrap(),
                    },
                ],
                ripple: false,
            },
            cx,
        );
    });
    ui.settle();
    let ramps = |ui: &Ui| ui.read(|e| e.find_clip(PAIR_FIRST).unwrap().time_map.ramps.clone());
    let original = ramps(&ui);
    assert_eq!(original.len(), 2);
    let before = ui.read(|e| e.project.clone());
    let none = gpui::Modifiers::default();

    // Dragging the second key up and left is one gesture and one undo step.
    let grab = ramp_point(&ui, PAIR_FIRST, Some(1), Time(0));
    ui.cx
        .simulate_mouse_down(grab, gpui::MouseButton::Left, none);
    ui.settle();
    assert!(ui.read(|e| matches!(e.gesture, Some(TimelineGesture::Ramp(_)))));
    for step in 1..=4 {
        let moved = grab + gpui::point(gpui::px(-6. * step as f32), gpui::px(-4. * step as f32));
        ui.cx
            .simulate_mouse_move(moved, gpui::MouseButton::Left, none);
        // Nothing is committed while the pointer is still down.
        assert_eq!(ui.read(|e| e.project.clone()), before);
    }
    assert!(ui.read(Editor::edit_tool_feedback).starts_with("Speed key"));
    let end = grab + gpui::point(gpui::px(-24.), gpui::px(-16.));
    ui.cx.simulate_mouse_up(end, gpui::MouseButton::Left, none);
    ui.settle();
    let dragged = ramps(&ui);
    assert_eq!(dragged.len(), 2);
    assert!(dragged[1].time < original[1].time, "moved earlier");
    assert!(dragged[1].speed.percent() > 200., "moved faster");
    assert_eq!(dragged[0], original[0]);
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
    ui.dispatch(Redo);
    assert_eq!(ramps(&ui), dragged);
    ui.dispatch(Undo);

    // A click that does not move the key changes nothing.
    let grab = ramp_point(&ui, PAIR_FIRST, Some(1), Time(0));
    ui.cx
        .simulate_mouse_down(grab, gpui::MouseButton::Left, none);
    ui.cx.simulate_mouse_up(grab, gpui::MouseButton::Left, none);
    ui.settle();
    assert_eq!(ui.read(|e| e.project.clone()), before);

    // Alt-click adds a key on the curve without changing it; Alt-click a key removes it.
    let alt = gpui::Modifiers {
        alt: true,
        ..Default::default()
    };
    let at = ramp_point(&ui, PAIR_FIRST, None, Time(250 * MS));
    ui.click(at, alt);
    assert_eq!(ramps(&ui).len(), 3);
    // On the 100% -> 200% curve around the 250 ms the pointer landed on (pixel precision).
    assert!((140.0..160.0).contains(&ramps(&ui)[1].speed.percent()));
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
    ui.dispatch(Redo);
    let added = ramp_point(&ui, PAIR_FIRST, Some(1), Time(0));
    ui.click(added, alt);
    assert_eq!(ramps(&ui), original);
    ui.dispatch(Undo);
    assert_eq!(ramps(&ui).len(), 3);
}

/// The first clip of [`voice_pair`] selected, with speed keys 100% at 0, 200% at 600 ms
/// and 100% at 1200 ms.
fn ramp_ui(cx: &mut TestAppContext) -> Ui {
    use animathio_core::SpeedPoint;
    let (project, assets) = voice_pair();
    let mut ui = Ui::open(cx, project, assets);
    let key = |ms: i64, percent: u32| SpeedPoint {
        time: Time(ms * MS),
        speed: ClipSpeed::new(percent, 100).unwrap(),
    };
    ui.update(|e, _, cx| {
        e.select_only(PAIR_FIRST, cx);
        e.try_execute(
            &Command::SetClipSpeed {
                id: PAIR_FIRST,
                speed: ClipSpeed::default(),
                reverse: false,
                ramps: vec![key(0, 100), key(600, 200), key(1200, 100)],
                ripple: false,
            },
            cx,
        );
    });
    ui.settle();
    ui
}

fn first_clip(ui: &Ui) -> animathio_core::Clip {
    ui.read(|e| e.find_clip(PAIR_FIRST).unwrap().clone())
}

/// Speed of the clip's curve at clip-local `time`, in millionths.
fn curve_at(clip: &animathio_core::Clip, local: Time) -> u32 {
    crate::speed_ramp::speed_at(clip, local).millionths()
}

#[gpui::test]
fn nudging_a_key_of_a_head_trimmed_ramp_clip_keeps_the_rest_of_the_curve(cx: &mut TestAppContext) {
    let mut ui = ramp_ui(cx);
    // Cut the first 300 ms off the head: the keys keep their curve time, the clip's
    // clock now runs 300 ms ahead of its first tick.
    ui.update(|e, _, cx| {
        let clip = e.find_clip(PAIR_FIRST).unwrap().clone();
        let cut = Time(300 * MS);
        e.try_execute(
            &Command::TrimClip {
                id: PAIR_FIRST,
                range: animathio_core::TimeRange::new(cut, clip.range.end).unwrap(),
                // Retimed clips take their offset delta in timeline ticks.
                source_offset: Time(clip.source_offset.0 + cut.0),
            },
            cx,
        );
    });
    ui.settle();
    let trimmed = first_clip(&ui);
    assert_eq!(trimmed.time_map.phase, Time(300 * MS));
    let length = trimmed.range.end.0 - trimmed.range.start.0;
    let before: Vec<u32> = (0..=length)
        .step_by(5 * MS as usize)
        .map(|t| curve_at(&trimmed, Time(t)))
        .collect();
    // The handle for the 600 ms key is drawn at local 300 ms (600 - phase), not at 600 ms.
    let handle = ramp_point(&ui, PAIR_FIRST, Some(1), Time(0));
    let expected_x = ui.read(|e| {
        e.timeline_bounds.origin.x
            + gpui::px(timeline::time_to_x(
                &e.current_layout(),
                Time(trimmed.range.start.0 + 300 * MS),
            ))
    });
    assert!((handle.x - expected_x).abs() < gpui::px(0.5));
    assert_eq!(
        ui.read(|e| e.hit_ramp_handle(
            &e.current_layout(),
            f32::from(handle.x - e.timeline_bounds.origin.x),
            f32::from(handle.y - e.timeline_bounds.origin.y)
        )),
        Some((PAIR_FIRST, 1))
    );
    // Nudge it straight up (faster) without moving it sideways.
    let none = gpui::Modifiers::default();
    ui.cx
        .simulate_mouse_down(handle, gpui::MouseButton::Left, none);
    ui.settle();
    assert!(ui.read(|e| matches!(e.gesture, Some(TimelineGesture::Ramp(_)))));
    let up = handle + gpui::point(gpui::px(0.), gpui::px(-8.));
    ui.cx.simulate_mouse_move(up, gpui::MouseButton::Left, none);
    ui.cx.simulate_mouse_up(up, gpui::MouseButton::Left, none);
    ui.settle();
    let nudged = first_clip(&ui);
    // The clock is rebuilt from the clip's own start, and exactly one key changed: the
    // key is still at local 300 ms (its curve time 600 ms minus the phase), and the clip
    // plays the old ramp with only that key's speed different, everywhere along it.
    assert_eq!(nudged.time_map.phase, Time(0));
    let moved = nudged
        .time_map
        .ramps
        .iter()
        .find(|k| k.time == Time(300 * MS))
        .expect("the nudged key stays at its local time");
    assert!(moved.speed.percent() > 200.);
    let mut expected = trimmed.time_map.clone();
    expected.ramps[1].speed = moved.speed;
    let nudged_length = nudged.range.end.0 - nudged.range.start.0;
    for t in (0..=nudged_length).step_by(3 * MS as usize) {
        let want = expected.speed_at_curve(Time(t + 300 * MS)).millionths();
        let got = curve_at(&nudged, Time(t));
        assert!(want.abs_diff(got) <= 2, "tick {t}: {want} vs {got}");
    }
    let last = nudged.time_map.ramps.last().unwrap();
    assert_eq!(last.time, Time(900 * MS), "the 1200 ms key is local 900 ms");
    assert_eq!(last.speed, ClipSpeed::new(1, 1).unwrap());
    assert!(
        nudged.range.end.0 < trimmed.range.end.0,
        "a faster key shortens the clip, nothing else"
    );
    let _ = (length, before);
    ui.dispatch(Undo);
    assert_eq!(first_clip(&ui), trimmed);
}

#[gpui::test]
fn handles_on_a_split_ramp_clip_add_keys_on_the_curve_the_clip_plays(cx: &mut TestAppContext) {
    let mut ui = ramp_ui(cx);
    let right = ClipId(90);
    ui.update(|e, _, cx| {
        let at = Time(e.find_clip(PAIR_FIRST).unwrap().range.start.0 + 700 * MS);
        e.try_execute(
            &Command::SplitClip {
                id: PAIR_FIRST,
                right_id: right,
                at,
            },
            cx,
        );
        e.select_only(right, cx);
    });
    ui.settle();
    let tail = ui.read(|e| e.find_clip(right).unwrap().clone());
    assert_eq!(tail.time_map.phase, Time(700 * MS));
    assert_eq!(tail.time_map.ramps.len(), 3, "the split keeps every key");
    // Only the 1200 ms key (local 500 ms) is inside the right clip: one handle.
    let vertices = crate::speed_ramp::vertices(&tail);
    assert_eq!(
        vertices.iter().filter_map(|v| v.index).collect::<Vec<_>>(),
        [2]
    );
    assert_eq!(
        vertices[0].index, None,
        "the curve enters at an interpolated edge vertex"
    );
    assert_eq!(vertices[0].time, Time(0));
    let before = ui.read(|e| e.project.clone());
    // Alt-click at local 250 ms: the curve there is the one the clip plays (between the
    // 600 ms and 1200 ms keys of the ramp), and the new key leaves it unchanged.
    let layout = ui.read(Editor::current_layout);
    let (x, y) = ui.read(|e| {
        let (row, clip) = e.ramp_target().unwrap();
        let lane = Editor::ramp_lane(&layout, row).unwrap();
        (
            timeline::time_to_x(&layout, Time(clip.range.start.0 + 250 * MS)),
            lane.y(ClipSpeed::default()),
        )
    });
    let at = ui.read(|e| {
        gpui::point(
            e.timeline_bounds.origin.x + gpui::px(x),
            e.timeline_bounds.origin.y + gpui::px(y),
        )
    });
    let alt = gpui::Modifiers {
        alt: true,
        ..Default::default()
    };
    ui.click(at, alt);
    let added = ui.read(|e| e.find_clip(right).unwrap().clone());
    assert_eq!(added.time_map.phase, Time(0));
    let key = added
        .time_map
        .ramps
        .iter()
        .find(|k| (k.time.0 - 250 * MS).abs() <= 33 * MS)
        .expect("a key at the pointer");
    // The ramp at 700 ms + local time lies between 200% (600 ms) and 100% (1200 ms).
    let expected = 200.0 - 100.0 * ((700 * MS + key.time.0 - 600 * MS) as f64 / (600 * MS) as f64);
    assert!(
        (key.speed.percent() - expected).abs() < 0.2,
        "{} vs {expected}",
        key.speed.percent()
    );
    // The edge vertices became real keys at 0 and at the end of the clip's curve.
    assert_eq!(added.time_map.ramps[0].time, Time(0));
    // The clip's length (and so everything after it) did not change by more than rounding.
    // Speeds are rounded to the millionth: a microsecond or so over a second of clip.
    assert!((added.range.end.0 - tail.range.end.0).abs() <= 2_000);
    for t in (0..tail.range.end.0 - tail.range.start.0).step_by(7 * MS as usize) {
        let (a, b) = (curve_at(&tail, Time(t)), curve_at(&added, Time(t)));
        assert!(a.abs_diff(b) <= 2, "tick {t}: {a} vs {b}");
    }
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

#[gpui::test]
fn trim_edges_win_over_speed_handles_and_short_lanes_show_no_keys(cx: &mut TestAppContext) {
    let mut ui = ramp_ui(cx);
    // The first key sits exactly on the clip's left edge: pressing it trims, it does not
    // start a key drag.
    let edge = ramp_point(&ui, PAIR_FIRST, Some(0), Time(0));
    ui.cx
        .simulate_mouse_down(edge, gpui::MouseButton::Left, gpui::Modifiers::default());
    ui.settle();
    assert!(ui.read(|e| matches!(e.gesture, Some(TimelineGesture::Trim(_)))));
    ui.cx
        .simulate_mouse_up(edge, gpui::MouseButton::Left, gpui::Modifiers::default());
    ui.settle();
    // A pointer outside the clip block never grabs a key, even within the grab radius.
    ui.read(|e| {
        let layout = e.current_layout();
        let (row, clip) = e.ramp_target().unwrap();
        let lane = Editor::ramp_lane(&layout, row).unwrap();
        let left = timeline::time_to_x(&layout, clip.range.start) - 3.0;
        assert_eq!(
            e.hit_ramp_handle(&layout, left, lane.y(ClipSpeed::default())),
            None
        );
    });
    // A row too short for the lane has no handles and Alt-click adds no key.
    let before = ui.read(|e| e.project.clone());
    let added = ui.update(|e, _, cx| {
        let mut layout = e.current_layout();
        layout.row_height = 30.0;
        let (row, _) = e.ramp_target().unwrap();
        assert!(Editor::ramp_lane(&layout, row).is_none());
        let (x, y) = (
            timeline::time_to_x(&layout, Time(300 * MS)),
            layout.ruler_height + 15.0,
        );
        e.ramp_press(&layout, x, y, true, cx)
    });
    assert!(!added);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

#[gpui::test]
fn add_speed_key_at_playhead_inserts_a_key_on_the_curve_and_undoes(cx: &mut TestAppContext) {
    let (project, assets) = voice_pair();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| {
        e.select_only(PAIR_FIRST, cx);
        e.seek_to(Time(750 * MS), cx);
    });
    let before = ui.read(|e| e.project.clone());
    // A constant clip: the key takes the clip's speed, so the curve is flat and the clip
    // keeps its length.
    ui.update(|e, _, cx| e.add_speed_key_at_playhead(PAIR_FIRST, cx));
    let clip = first_clip(&ui);
    assert_eq!(clip.time_map.ramps.len(), 1);
    assert_eq!(clip.time_map.ramps[0].time, Time(750 * MS));
    assert_eq!(clip.time_map.ramps[0].speed, ClipSpeed::default());
    assert_eq!(
        clip.range,
        ui.read(|e| e.find_clip(PAIR_SECOND).map(|_| clip.range))
            .unwrap()
    );
    // A second key on the same spot is refused with a message.
    let refused = ui.update(|e, _, cx| {
        e.add_speed_key_at_playhead(PAIR_FIRST, cx);
        e.error.clone()
    });
    assert_eq!(first_clip(&ui).time_map.ramps.len(), 1);
    assert!(refused.is_some());
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

/// The ten 1.x text styles: listed under Typography, one undo step each, exact size/weight.
#[gpui::test]
fn typography_presets_add_with_their_weight_in_one_undo_step(cx: &mut TestAppContext) {
    if !require_plugins("typography_presets_add_with_their_weight_in_one_undo_step") {
        return;
    }
    // 1.x sizes in px against its 360-high canvas, with the legacy weights.
    let expected = [
        ("typography_title", 28.0, 600.0),
        ("typography_subtitle", 16.0, 600.0),
        ("typography_body", 14.0, 400.0),
        ("typography_caption", 12.0, 400.0),
        ("typography_heading_1", 24.0, 800.0),
        ("typography_heading_2", 20.0, 800.0),
        ("typography_heading_3", 18.0, 800.0),
        ("typography_heading_4", 16.0, 800.0),
        ("typography_heading_5", 14.0, 800.0),
        ("typography_heading_6", 12.0, 800.0),
    ];
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.dispatch(actions::FocusLibrary);
    ui.update(|e, _, cx| e.select_left_tab(LeftTab::Titles, cx));
    for (id, _, _) in expected {
        assert!(
            bounds_of(&mut ui, format!("title-{id}")).is_some(),
            "{id} card is listed"
        );
    }
    for (id, px, weight) in expected {
        let before = ui.read(|e| e.project.clone());
        let clips_before = ui.read(clip_count);
        ui.update(|e, _, cx| e.add_title_preset(id, Time(0), cx));
        let added = ui.read(|e| e.selected.expect("the added text clip is selected"));
        ui.read(|e| {
            assert_eq!(clip_count(e), clips_before + 1, "{id}: one text layer");
            let animathio_core::ClipKind::Plugin { plugin_id, params } =
                &e.find_clip(added).expect("clip").kind
            else {
                panic!("{id}: plugin clip expected")
            };
            assert_eq!(plugin_id, "org.animathio.text");
            assert_eq!(params["font_weight"], serde_json::json!(weight), "{id}");
            assert_eq!(
                params["font"],
                serde_json::json!("inter"),
                "{id}: typography is Inter at every weight"
            );
            let size = params["font_size"].as_f64().unwrap();
            let (width, height) = project_size(&e.project);
            // Normalized against 360 px of the shorter canvas edge (16:9 here).
            assert!(width >= height);
            assert!((size - px / 360.0).abs() < 1e-9, "{id}: size {size}");
        });
        let applied = ui.read(|e| e.project.clone());
        ui.dispatch(Undo);
        assert_eq!(
            ui.read(|e| e.project.clone()),
            before,
            "{id}: one undo step"
        );
        ui.dispatch(Redo);
        assert_eq!(ui.read(|e| e.project.clone()), applied, "{id}: redo");
        ui.dispatch(Undo);
    }
}

/// Font weight is a schema parameter of the text plugin and edits go through commands.
#[gpui::test]
fn text_font_weight_edit_is_a_command_with_undo(cx: &mut TestAppContext) {
    if !require_plugins("text_font_weight_edit_is_a_command_with_undo") {
        return;
    }
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| e.add_title_preset("typography_body", Time(0), cx));
    let id = ui.read(|e| e.selected.expect("selected"));
    let weight = |ui: &Ui| ui.read(|e| e.plugin_number_value(id, "font_weight"));
    assert!((weight(&ui) - 400.0).abs() < 1e-9);
    assert!(
        ui.read(|e| e
            .plugin_model(id, "font_weight")
            .is_some_and(|model| matches!(
                model,
                crate::inspector::ParamModel::Number { min, max, .. } if (min - 100.0).abs() < 1e-9 && (max - 900.0).abs() < 1e-9
            ))),
        "the inspector builds a 100-900 Font weight field from the schema"
    );
    assert!(
        ui.read(|e| e.plugin_model(id, "font").is_some_and(|model| matches!(
            model,
            crate::inspector::ParamModel::Enum { options, default, .. }
                if options == ["dejavu", "inter"] && default == "dejavu"
        ))),
        "the inspector builds a Font dropdown (dejavu | inter) from the schema"
    );
    let before = ui.read(|e| e.project.clone());
    // The inspector's number field commits through this command; out-of-range input clamps.
    ui.update(|e, _, cx| {
        let command = e
            .plugin_number_command(id, "font_weight", 700.0)
            .expect("edit command");
        e.execute(&command, cx);
    });
    assert!((weight(&ui) - 700.0).abs() < 1e-9);
    let edited = ui.read(|e| e.project.clone());
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
    ui.dispatch(Redo);
    assert_eq!(ui.read(|e| e.project.clone()), edited);
    ui.update(|e, _, cx| {
        let command = e
            .plugin_number_command(id, "font_weight", 5000.0)
            .expect("edit command");
        e.execute(&command, cx);
    });
    assert!((weight(&ui) - 900.0).abs() < 1e-9);
}

/// A scripted AI assist client: records what it was asked and returns a canned reply.
#[derive(Debug)]
struct MockAssist {
    reply: std::sync::Mutex<Result<String, animathio_assist::AssistError>>,
    seen: std::sync::Mutex<Vec<(String, String)>>,
    /// Whether each request carried an API key.
    keys: std::sync::Mutex<Vec<bool>>,
    /// Block in `complete` until the request is cancelled (then record that it was).
    block: bool,
    cancelled: std::sync::atomic::AtomicBool,
}

impl MockAssist {
    fn new(reply: Result<String, animathio_assist::AssistError>) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            reply: std::sync::Mutex::new(reply),
            seen: std::sync::Mutex::default(),
            keys: std::sync::Mutex::default(),
            block: false,
            cancelled: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// A client that waits for its token to be cancelled, like a request stuck on the network.
    fn blocking() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            reply: std::sync::Mutex::new(Err(animathio_assist::AssistError::Cancelled)),
            seen: std::sync::Mutex::default(),
            keys: std::sync::Mutex::default(),
            block: true,
            cancelled: std::sync::atomic::AtomicBool::new(false),
        })
    }

    fn seen(&self) -> Vec<(String, String)> {
        self.seen.lock().unwrap().clone()
    }
}

impl animathio_assist::Converter for MockAssist {
    fn complete(
        &self,
        endpoint: &animathio_assist::Endpoint,
        prose: &str,
        cancel: &animathio_assist::CancelToken,
    ) -> Result<String, animathio_assist::AssistError> {
        self.seen
            .lock()
            .unwrap()
            .push((endpoint.host().to_owned(), prose.to_owned()));
        self.keys.lock().unwrap().push(endpoint.has_key());
        if self.block {
            let started = std::time::Instant::now();
            while !cancel.is_cancelled() && started.elapsed() < std::time::Duration::from_secs(20) {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            self.cancelled
                .store(cancel.is_cancelled(), std::sync::atomic::Ordering::SeqCst);
        }
        self.reply.lock().unwrap().clone()
    }
}

fn local_assist_settings() -> animathio_assist::AssistSettings {
    animathio_assist::AssistSettings {
        base_url: "http://localhost:11434/v1".into(),
        model: "llama3.2".into(),
    }
}

/// An editor whose AI assist client is `mock`, a Latex math panel on it, and its assist panel.
fn assist_ui(
    cx: &mut TestAppContext,
    mock: &std::sync::Arc<MockAssist>,
) -> (Ui, Entity<MathEditor>, Entity<AssistPanel>) {
    let (project, assets) = sample();
    let mut env = super::test_env();
    env.assist = mock.clone();
    let mut ui = Ui::open_with_env(cx, project, assets, env);
    let panel = ui.update(|e, window, cx| {
        let draft = MathObjectDraft::new(MathObjectKind::Latex);
        let range = math_object_range(e.position).expect("range");
        let editor = cx.entity().downgrade();
        let session = e.project_session;
        let size = project_size(&e.project);
        cx.new(|cx| MathEditor::new(draft, editor, session, range, None, false, size, window, cx))
    });
    let assist = ui
        .cx
        .update(|_, cx| panel.read(cx).assist_panel().expect("latex has assist"));
    ui.cx.update(|_, cx| {
        assist.update(cx, |assist, _| assist.set_settings(local_assist_settings()));
    });
    (ui, panel, assist)
}

/// The conversion runs on its own thread, so wait (bounded) for it to report back.
fn finish_convert(ui: &mut Ui, assist: &Entity<AssistPanel>) {
    let started = std::time::Instant::now();
    loop {
        ui.advance_clock(std::time::Duration::from_millis(30));
        ui.settle();
        if !ui.cx.update(|_, cx| assist.read(cx).is_converting()) {
            return;
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "the conversion never finished"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

fn type_prose(ui: &mut Ui, assist: &Entity<AssistPanel>, text: &str) {
    ui.cx.update(|window, cx| {
        assist.update(cx, |assist, cx| assist.set_prose(text, window, cx));
    });
    ui.settle();
}

#[gpui::test]
fn assist_convert_preview_insert_undo(cx: &mut TestAppContext) {
    if !require_plugins("assist_convert_preview_insert_undo") {
        return;
    }
    let mock = MockAssist::new(Ok("```latex\n\\frac{1}{2} + x^2\n```".into()));
    let (mut ui, panel, assist) = assist_ui(cx, &mock);
    let before = ui.read(|e| e.project.clone());
    let initial = ui.cx.update(|_, cx| panel.read(cx).draft_source());

    // Typing the description sends nothing.
    type_prose(&mut ui, &assist, "one half plus x squared");
    ui.advance_clock(std::time::Duration::from_millis(400));
    assert!(mock.seen().is_empty(), "nothing is sent before Convert");

    // Convert sends exactly the prose to the configured host and shows editable LaTeX.
    ui.cx
        .update(|_, cx| assist.update(cx, AssistPanel::convert));
    finish_convert(&mut ui, &assist);
    assert_eq!(
        mock.seen(),
        vec![(
            "localhost:11434".to_owned(),
            "one half plus x squared".to_owned()
        )]
    );
    assert_ne!(initial, "\\frac{1}{2} + x^2");
    assert_eq!(
        ui.cx.update(|_, cx| panel.read(cx).draft_source()),
        "\\frac{1}{2} + x^2"
    );
    assert!(!ui.cx.update(|_, cx| assist.read(cx).is_converting()));

    // The live preview validates it through the latex plugin; then Add inserts one object.
    ui.advance_clock(std::time::Duration::from_millis(400));
    ui.settle();
    ui.cx
        .update(|window, cx| panel.update(cx, |panel, cx| panel.apply(window, cx)));
    ui.settle();
    let added = ui.read(|e| e.selected.expect("inserted clip selected"));
    ui.read(|e| {
        let clip = e.find_clip(added).expect("clip");
        assert!(matches!(
            &clip.kind,
            animathio_core::ClipKind::Plugin { plugin_id, .. } if plugin_id == "org.animathio.latex"
        ));
        let draft = animathio_control::math_object_draft(clip, project_size(&e.project))
            .expect("math object draft");
        assert_eq!(draft.fields["source"], "\\frac{1}{2} + x^2");
    });
    // One undo removes it.
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

#[gpui::test]
fn assist_cancel_ignores_the_result(cx: &mut TestAppContext) {
    let mock = MockAssist::new(Ok("x^2".into()));
    let (mut ui, panel, assist) = assist_ui(cx, &mock);
    let initial = ui.cx.update(|_, cx| panel.read(cx).draft_source());
    type_prose(&mut ui, &assist, "x squared");
    ui.cx.update(|_, cx| {
        assist.update(cx, |assist, cx| {
            assist.convert(cx);
            assert!(assist.is_converting());
            assist.cancel(cx);
            assert!(!assist.is_converting());
        });
    });
    ui.settle();
    std::thread::sleep(std::time::Duration::from_millis(50));
    ui.advance_clock(std::time::Duration::from_millis(30));
    ui.settle();
    assert_eq!(ui.cx.update(|_, cx| panel.read(cx).draft_source()), initial);
    assert!(!ui.cx.update(|_, cx| assist.read(cx).is_converting()));
}

/// Cancelling after the keyring read keeps its answer, so the next Convert (or the setup form)
/// does not read the keyring a second time.
#[gpui::test]
fn assist_cancel_keeps_the_key_status_it_already_read(cx: &mut TestAppContext) {
    use crate::editor::assist::{KeyStatus, cached_key_status_for_tests};
    let mock = MockAssist::blocking();
    let (mut ui, _panel, assist) = assist_ui(cx, &mock);
    type_prose(&mut ui, &assist, "x squared");
    ui.cx.update(|_, cx| {
        assist.update(cx, |assist, _| {
            assist.set_settings(assist_settings("https://api.openai.com/v1"));
        });
    });
    let status = |ui: &mut Ui| {
        ui.cx
            .update(|_, cx| cached_key_status_for_tests(cx, "https://api.openai.com:443"))
    };
    assert_eq!(status(&mut ui), None);
    ui.cx.update(|_, cx| {
        assist.update(cx, |assist, cx| {
            assist.convert(cx);
            assist.cancel(cx);
        });
    });
    ui.settle();
    std::thread::sleep(std::time::Duration::from_millis(50));
    ui.advance_clock(std::time::Duration::from_millis(30));
    ui.settle();
    assert_eq!(status(&mut ui), Some(KeyStatus::Missing));
}

#[gpui::test]
fn assist_errors_are_shown_and_leave_the_formula_alone(cx: &mut TestAppContext) {
    use crate::editor::assist::Message;
    let mock = MockAssist::new(Err(animathio_assist::AssistError::Auth {
        status: 401,
        detail: "bad key".into(),
    }));
    let (mut ui, panel, assist) = assist_ui(cx, &mock);
    let initial = ui.cx.update(|_, cx| panel.read(cx).draft_source());

    // Empty description: no request at all.
    ui.cx
        .update(|_, cx| assist.update(cx, AssistPanel::convert));
    finish_convert(&mut ui, &assist);
    assert_eq!(mock.seen(), Vec::<(String, String)>::new());
    let message = ui.cx.update(|_, cx| assist.read(cx).message().cloned());
    assert!(matches!(message, Some(Message::Error(_))), "{message:?}");

    // An authentication failure is reported with its status.
    type_prose(&mut ui, &assist, "x squared");
    ui.cx
        .update(|_, cx| assist.update(cx, AssistPanel::convert));
    finish_convert(&mut ui, &assist);
    let Some(Message::Error(text)) = ui.cx.update(|_, cx| assist.read(cx).message().cloned())
    else {
        panic!("expected an error message");
    };
    assert!(text.contains("HTTP 401"), "{text}");
    assert_eq!(ui.cx.update(|_, cx| panel.read(cx).draft_source()), initial);

    // A reply that is not LaTeX is rejected before it reaches the formula field.
    *mock.reply.lock().unwrap() = Ok("Sorry, I cannot help with that request today.".into());
    ui.cx
        .update(|_, cx| assist.update(cx, AssistPanel::convert));
    finish_convert(&mut ui, &assist);
    let Some(Message::Error(text)) = ui.cx.update(|_, cx| assist.read(cx).message().cloned())
    else {
        panic!("expected an error message");
    };
    assert!(text.contains("did not return LaTeX"), "{text}");
    assert_eq!(ui.cx.update(|_, cx| panel.read(cx).draft_source()), initial);

    // Unconfigured: off, nothing is sent.
    let calls = mock.seen().len();
    ui.cx.update(|_, cx| {
        assist.update(cx, |assist, cx| {
            assist.set_settings(animathio_assist::AssistSettings::default());
            assist.convert(cx);
        });
    });
    finish_convert(&mut ui, &assist);
    assert_eq!(mock.seen().len(), calls);
}

#[gpui::test]
fn assist_setup_saves_settings_without_the_key(cx: &mut TestAppContext) {
    let mock = MockAssist::new(Ok("x".into()));
    let (mut ui, _panel, assist) = assist_ui(cx, &mock);
    ui.cx.update(|window, cx| {
        assist.update(cx, |assist, cx| {
            assist.set_form(
                (
                    "https://api.openai.com/v1",
                    "gpt-4.1-mini",
                    "sk-secret-value",
                ),
                window,
                cx,
            );
            assist.save_setup(window, cx);
        });
    });
    ui.settle();
    let path = crate::editor_env::config_file("assist.json").expect("config path");
    let text = std::fs::read_to_string(&path).expect("settings file");
    assert!(text.contains("api.openai.com"), "{text}");
    assert!(!text.contains("sk-secret-value"), "{text}");
    ui.cx.update(|_, cx| {
        let assist = assist.read(cx);
        assert_eq!(assist.settings().model, "gpt-4.1-mini");
        // The key left the text field as soon as it was saved.
        assert_eq!(assist.api_key_field(cx), "");
    });
    // An invalid base URL is refused and not saved.
    ui.cx.update(|window, cx| {
        assist.update(cx, |assist, cx| {
            assist.set_form(("http://api.openai.com/v1", "m", ""), window, cx);
            assist.save_setup(window, cx);
        });
    });
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("https://api.openai.com/v1"),
        "the invalid URL must not replace the saved one"
    );
    // Leave the shared test config directory as found.
    let _ = std::fs::remove_file(path);
}

fn assist_settings(url: &str) -> animathio_assist::AssistSettings {
    animathio_assist::AssistSettings {
        base_url: url.into(),
        model: "m".into(),
    }
}

#[gpui::test]
fn assist_panel_does_not_touch_the_keyring_until_asked(cx: &mut TestAppContext) {
    use crate::editor::assist::{KeyStatus, cached_key_status_for_tests};
    let mock = MockAssist::new(Ok("x".into()));
    let (mut ui, _panel, assist) = assist_ui(cx, &mock);
    ui.cx.update(|_, cx| {
        assist.update(cx, |assist, _| {
            assist.set_settings(assist_settings("https://api.openai.com/v1"));
        });
    });
    ui.settle();
    // Building (and rebuilding) the panel never reads the keyring: no status is known.
    let origin = "https://api.openai.com:443";
    ui.cx.update(|window, cx| {
        for _ in 0..3 {
            let panel = cx.new(|cx| AssistPanel::new(window, cx));
            assert_eq!(panel.read(cx).key_status(cx), KeyStatus::Unknown);
        }
        assert_eq!(cached_key_status_for_tests(cx, origin), None);
    });
    ui.settle();
    ui.cx
        .update(|_, cx| assert_eq!(cached_key_status_for_tests(cx, origin), None));
    // Opening the setup form is the user action that looks the key up (the test platform
    // keyring is empty).
    ui.cx.update(|_, cx| {
        assist.update(cx, |assist, cx| {
            if assist.is_setup_open() {
                assist.toggle_setup(cx);
            }
            assert!(!assist.is_setup_open());
            assist.toggle_setup(cx);
            assert!(assist.is_setup_open());
        });
    });
    ui.settle();
    ui.cx.update(|_, cx| {
        assert_eq!(assist.read(cx).key_status(cx), KeyStatus::Missing);
    });
}

#[gpui::test]
fn assist_convert_reads_the_keyring_only_when_a_key_can_matter(cx: &mut TestAppContext) {
    use crate::editor::assist::{KeyStatus, set_key_status};
    let mock = MockAssist::new(Ok("x".into()));
    let (mut ui, _panel, assist) = assist_ui(cx, &mock);
    let status = |ui: &mut Ui, origin: &str| {
        ui.cx
            .update(|_, cx| crate::editor::assist::cached_key_status_for_tests(cx, origin))
    };
    type_prose(&mut ui, &assist, "x");
    let run = |ui: &mut Ui, url: &str| {
        ui.cx.update(|_, cx| {
            assist.update(cx, |assist, _| assist.set_settings(assist_settings(url)));
        });
        ui.cx
            .update(|_, cx| assist.update(cx, AssistPanel::convert));
        finish_convert(ui, &assist);
    };
    // An online service: the key is looked up on Convert.
    run(&mut ui, "https://api.openai.com/v1");
    assert_eq!(
        status(&mut ui, "https://api.openai.com:443"),
        Some(KeyStatus::Missing)
    );
    // A server on this machine: not read, as nothing says a key is saved for it.
    run(&mut ui, "http://localhost:11434/v1");
    assert_eq!(status(&mut ui, "http://localhost:11434"), None);
    // ... unless the setup form already found one.
    ui.cx
        .update(|_, cx| set_key_status(cx, "http://localhost:11434", KeyStatus::Saved));
    run(&mut ui, "http://localhost:11434/v1");
    assert_eq!(
        status(&mut ui, "http://localhost:11434"),
        Some(KeyStatus::Missing)
    );
    // A network server over plain http: never read, never sent.
    run(&mut ui, "http://192.168.1.20:11434/v1");
    assert_eq!(status(&mut ui, "http://192.168.1.20:11434"), None);
    assert_eq!(mock.keys.lock().unwrap().len(), 4);
}

#[gpui::test]
fn assist_refuses_a_key_for_plain_http_on_the_network(cx: &mut TestAppContext) {
    use crate::editor::assist::Message;
    let mock = MockAssist::new(Ok("x".into()));
    let (mut ui, _panel, assist) = assist_ui(cx, &mock);
    ui.cx.update(|window, cx| {
        assist.update(cx, |assist, cx| {
            assist.set_form(
                ("http://192.168.1.20:11434/v1", "m", "sk-secret-value"),
                window,
                cx,
            );
            assist.save_setup(window, cx);
        });
    });
    ui.settle();
    ui.cx.update(|_, cx| {
        let assist = assist.read(cx);
        let Some(Message::Error(text)) = assist.message() else {
            panic!("expected a refusal, got {:?}", assist.message());
        };
        assert!(text.contains("plain http://"), "{text}");
        assert!(!text.contains("sk-secret-value"), "{text}");
        // The settings were not changed.
        assert_eq!(assist.settings().base_url, "http://localhost:11434/v1");
    });
}

#[gpui::test]
fn dropping_the_assist_panel_cancels_the_request(cx: &mut TestAppContext) {
    let mock = MockAssist::blocking();
    let (mut ui, panel, assist) = assist_ui(cx, &mock);
    type_prose(&mut ui, &assist, "x squared");
    ui.cx
        .update(|_, cx| assist.update(cx, AssistPanel::convert));
    ui.settle();
    // Wait until the worker has the request.
    let started = std::time::Instant::now();
    while mock.seen().is_empty() {
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    // Closing the dialog drops the panel; its request is cancelled.
    drop(assist);
    ui.cx.update(|_, _| drop(panel));
    ui.settle();
    let started = std::time::Instant::now();
    while !mock.cancelled.load(std::sync::atomic::Ordering::SeqCst) {
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "the request was not cancelled"
        );
        ui.advance_clock(std::time::Duration::from_millis(30));
        ui.settle();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
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
fn launcher_actions_and_canvas_cards_are_in_view_at_720p(cx: &mut TestAppContext) {
    let mut ui = Ui::open(cx, Project::default(), BTreeMap::new());
    assert!(ui.read(Editor::is_start_screen));
    ui.cx
        .simulate_resize(gpui::size(gpui::px(1280.), gpui::px(720.)));
    ui.settle();
    for selector in ["start-new", "start-open", "start-import", "canvas-preset-0"] {
        let bounds = ui
            .cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is rendered"));
        // Above the status bar and the window edge, without scrolling the launcher.
        assert!(bounds.bottom() <= gpui::px(680.), "{selector}: {bounds:?}");
        assert!(bounds.right() <= gpui::px(1280.), "{selector}: {bounds:?}");
        assert!(bounds.size.width > gpui::px(40.), "{selector}: {bounds:?}");
    }
    // The two imports live in the Import… menu.
    let import = ui.cx.debug_bounds("start-import").unwrap();
    ui.click(import.center(), gpui::Modifiers::default());
    ui.advance_clock(std::time::Duration::from_millis(400));
    for selector in ["start-v1", "start-manim"] {
        let bounds = ui
            .cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is in the menu"));
        assert!(bounds.bottom() <= gpui::px(720.), "{selector}: {bounds:?}");
        assert!(bounds.size.width > gpui::px(40.), "{selector}: {bounds:?}");
    }
}

#[gpui::test]
fn library_tabs_are_icon_only_at_the_default_dock_width_and_fit(cx: &mut TestAppContext) {
    use crate::editor::workspace::LIBRARY_TABS_LABEL_WIDTH;
    assert!(crate::layout::DockLayout::default().left_width < LIBRARY_TABS_LABEL_WIDTH);
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    // `Editor::new` loads the dock layout persisted in the shared test config dir, which other
    // tests rewrite (a closed Library panel) while this one runs in parallel; pin the default.
    ui.update(|e, _, cx| {
        e.dock = crate::layout::DockLayout::default();
        cx.notify();
    });
    ui.cx
        .simulate_resize(gpui::size(gpui::px(1280.), gpui::px(720.)));
    ui.settle();
    let names = ["media", "elements", "titles", "effects", "plugins"];
    let missing: Vec<&str> = names
        .into_iter()
        .filter(|name| bounds_of(&mut ui, format!("library-tab-{name}")).is_none())
        .collect();
    let (start, left_width, left_tab, dock) = ui.read(|e| {
        (
            e.is_start_screen(),
            e.dock.left_width,
            e.left_tab as usize,
            format!("{:?}", e.dock),
        )
    });
    assert!(
        missing.is_empty(),
        "library tabs not rendered: {missing:?} (start screen: {start}, left width: {left_width}, tab index: {left_tab}, dock: {dock})"
    );
    let mut previous_right = gpui::px(0.);
    for name in names {
        let bounds = bounds_of(&mut ui, format!("library-tab-{name}")).expect("tab rendered");
        assert!(bounds.size.width >= gpui::px(30.), "{name}: {bounds:?}");
        assert!(
            bounds.left() >= previous_right,
            "{name} overlaps its neighbour"
        );
        previous_right = bounds.right();
    }
    // The dock stays within its panel: the last tab ends left of the preview.
    assert!(previous_right < gpui::px(300.));
}

#[test]
fn element_cards_use_the_dock_width() {
    use crate::editor::math::element_card_columns;
    let rem = gpui::px(16.);
    assert_eq!(element_card_columns(rem, 200.), 1);
    assert_eq!(element_card_columns(rem, 260.), 2);
    assert_eq!(element_card_columns(rem, 400.), 3);
    assert_eq!(element_card_columns(rem, 640.), 3);
}

/// The Library grids never clip a card at the panel's right edge (see
/// `docs/design.md` "Library cards and lists"): at 1280x720 the second
/// element card ends inside the Library body.
#[gpui::test]
fn element_cards_stay_inside_the_library_panel_at_720p(cx: &mut TestAppContext) {
    if !require_plugins("element_cards_stay_inside_the_library_panel_at_720p") {
        return;
    }
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.cx
        .simulate_resize(gpui::size(gpui::px(1280.), gpui::px(720.)));
    ui.update(|e, _, cx| e.select_left_tab(LeftTab::Elements, cx));
    ui.settle();
    let card = ui
        .cx
        .debug_bounds("element-card-1")
        .expect("the second element card is rendered");
    let panel = ui
        .cx
        .debug_bounds("library-body")
        .expect("the Library body is rendered");
    assert!(
        card.right() <= panel.right() + gpui::px(1.),
        "element-card-1 ends at {}, past the Library panel edge at {}",
        f32::from(card.right()),
        f32::from(panel.right())
    );
    assert!(
        card.left() >= panel.left(),
        "element-card-1 starts at {}, left of the Library panel edge at {}",
        f32::from(card.left()),
        f32::from(panel.left())
    );
}

/// The Add math object dialog at `width` x `height`: every control is laid out inside the dialog
/// form's visible area (not below a scroll) or, for the palette, inside the window.
fn math_dialog_controls(
    cx: &mut TestAppContext,
    width: f32,
    height: f32,
) -> Vec<(&'static str, gpui::Bounds<gpui::Pixels>)> {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.cx
        .simulate_resize(gpui::size(gpui::px(width), gpui::px(height)));
    ui.settle();
    ui.update(|e, window, cx| e.open_math(MathObjectKind::Latex, None, window, cx));
    ui.settle();
    [
        "math-form",
        "math-colour-row",
        "math-write-on",
        "math-palette",
    ]
    .into_iter()
    .map(|selector| {
        (
            selector,
            ui.cx
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} is rendered at {width}x{height}")),
        )
    })
    .collect()
}

#[gpui::test]
fn math_dialog_shows_colour_write_on_and_templates_at_720p(cx: &mut TestAppContext) {
    let controls = math_dialog_controls(cx, 1280., 720.);
    let bounds = |name: &str| controls.iter().find(|(n, _)| *n == name).unwrap().1;
    let form = bounds("math-form");
    // Wide window: the palette sits beside the form, fully inside the window.
    let palette = bounds("math-palette");
    assert!(
        palette.left() >= form.right() - gpui::px(1.),
        "{palette:?} {form:?}"
    );
    assert!(palette.bottom() <= gpui::px(720.));
    assert!(palette.right() <= gpui::px(1280.));
    // Colour and Write on need no scrolling inside the form.
    for name in ["math-colour-row", "math-write-on"] {
        let control = bounds(name);
        assert!(
            control.top() >= form.top() && control.bottom() <= form.bottom() + gpui::px(1.),
            "{name}: {control:?} in {form:?}"
        );
        assert!(control.bottom() <= gpui::px(720.));
    }
}

#[gpui::test]
fn math_dialog_keeps_colour_and_write_on_above_the_palette_in_a_narrow_window(
    cx: &mut TestAppContext,
) {
    let controls = math_dialog_controls(cx, 900., 720.);
    let bounds = |name: &str| controls.iter().find(|(n, _)| *n == name).unwrap().1;
    let form = bounds("math-form");
    for name in ["math-colour-row", "math-write-on"] {
        let control = bounds(name);
        assert!(
            control.bottom() <= form.bottom() + gpui::px(1.),
            "{name}: {control:?} in {form:?}"
        );
        // Before the palette, so the palette is the part that scrolls.
        assert!(control.bottom() <= bounds("math-palette").top());
    }
}

// ---- first-time-user pass (#286) ----------------------------------------------------

fn clear_error(ui: &mut Ui) {
    ui.update(|editor, _, _| editor.last_error = None);
}

fn last_error(ui: &Ui) -> String {
    ui.read(|editor| editor.last_error.clone().unwrap_or_default())
}

#[test]
fn counts_are_pluralised_and_import_errors_summarised() {
    use crate::editor::support::{count_label, import_error_summary, unsupported_media_message};
    assert_eq!(count_label(1, "media file"), "1 media file");
    assert_eq!(count_label(3, "media file"), "3 media files");
    assert_eq!(import_error_summary(&[]), None);
    assert_eq!(import_error_summary(&["a".into()]).as_deref(), Some("a"));
    assert_eq!(
        import_error_summary(&["first".into(), "second".into(), "third".into()]).as_deref(),
        Some("3 files could not be imported. first")
    );
    let message = unsupported_media_message(std::path::Path::new("/some/dir/clip.gif"));
    assert!(message.starts_with("clip.gif is not a supported media file"));
    assert!(message.contains("MP4") && message.contains("WAV") && message.contains("PNG"));
    assert!(
        !message.contains("/some/dir"),
        "names the file, not the path"
    );
}

#[gpui::test]
fn import_names_every_refused_file_and_still_imports_the_rest(cx: &mut TestAppContext) {
    let mut ui = Ui::open(cx, Project::default(), BTreeMap::new());
    let dir = super::config_dir();
    let good = dir.join("usability-good.wav");
    let bad_one = dir.join("usability-notes.txt");
    let bad_two = dir.join("usability-clip.gif");
    std::fs::write(&good, wav(&[(0.25, 800)])).unwrap();
    std::fs::write(&bad_one, b"hello").unwrap();
    std::fs::write(&bad_two, b"GIF89a").unwrap();
    ui.update(|editor, _, cx| {
        editor.import_paths(&[good, bad_one.clone(), bad_two], None, cx);
    });
    let error = last_error(&ui);
    assert!(
        error.starts_with("2 files could not be imported."),
        "{error}"
    );
    assert!(error.contains("usability-notes.txt"), "{error}");
    assert_eq!(ui.read(|e| e.project.assets().len()), 1, "the wav imported");
    clear_error(&mut ui);
    ui.update(|editor, _, cx| editor.import_paths(&[bad_one], None, cx));
    let error = last_error(&ui);
    assert!(
        error.starts_with("usability-notes.txt is not a supported"),
        "{error}"
    );
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

#[gpui::test]
fn effects_tab_hints_at_selecting_a_clip_first(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.dispatch(ShowEffects);
    ui.update(|editor, _, cx| editor.clear_selection(cx));
    assert!(
        ui.cx.debug_bounds("effects-hint").is_some(),
        "hint while nothing is selected"
    );
    let id = ui.read(|e| e.project.tracks()[0].clips().values().next().unwrap().id);
    ui.update(|editor, _, cx| editor.select_only(id, cx));
    ui.update(|_, window, _| window.refresh());
    assert!(
        ui.cx.debug_bounds("effects-hint").is_none(),
        "hint disappears once a clip is selected"
    );
    // Clicking an effect with nothing selected explains what to do instead of doing nothing.
    ui.update(|editor, _, cx| {
        editor.clear_selection(cx);
        editor.add_effect_to_selected("org.animathio.effects-basic", "blur", cx);
    });
    assert!(last_error(&ui).starts_with("Select a clip first"));
}

#[test]
fn about_reports_the_running_version() {
    let text = crate::editor::actions::about_text();
    assert!(text.contains(env!("CARGO_PKG_VERSION")), "{text}");
}

#[gpui::test]
fn enter_and_escape_in_an_inspector_field_return_the_keyboard_to_the_timeline(
    cx: &mut TestAppContext,
) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    let id = ui.read(|e| e.project.tracks()[0].clips().values().next().unwrap().id);
    ui.update(|editor, _, cx| editor.select_only(id, cx));
    ui.update(|_, window, _| window.refresh());
    let state = ui
        .read(|editor| editor.fields.first_input())
        .expect("the inspector has an input for the selected clip");
    let focus_field = |ui: &mut Ui| {
        let state = state.clone();
        ui.update(move |_, window, cx| {
            let handle = state.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        });
    };
    let field_focused =
        |ui: &mut Ui| ui.update(|editor, window, cx| editor.fields.any_focused(window, cx));
    let timeline_focused =
        |ui: &mut Ui| ui.update(|editor, window, _| editor.panel_focus[0].is_focused(window));

    // Escape drops the typed-but-uncommitted text and leaves the field.
    focus_field(&mut ui);
    assert!(field_focused(&mut ui), "the field took focus");
    let original = state.read_with(ui.cx, |s, _| s.value().to_string());
    ui.update(|_, window, cx| {
        state.update(cx, |s, cx| s.set_value("999", window, cx));
    });
    ui.press("escape");
    assert!(!field_focused(&mut ui), "escape leaves the field");
    assert!(timeline_focused(&mut ui), "focus returns to the timeline");
    assert_eq!(
        state.read_with(ui.cx, |s, _| s.value().to_string()),
        original,
        "the unsaved edit was dropped"
    );

    // Enter commits and leaves the field too, so Space and Delete work again.
    focus_field(&mut ui);
    let before = ui.read(|e| e.project.clone());
    let typed = original.parse::<f64>().expect("a numeric field") + 7.0;
    ui.update(|_, window, cx| {
        state.update(cx, |s, cx| s.set_value(typed.to_string(), window, cx));
    });
    ui.press("enter");
    assert!(!field_focused(&mut ui), "enter leaves the field");
    assert!(timeline_focused(&mut ui));
    let after = ui.read(|e| e.project.clone());
    assert_ne!(
        after, before,
        "enter committed the typed value to the model"
    );
    ui.dispatch(Undo);
    assert_eq!(
        ui.read(|e| e.project.clone()),
        before,
        "one undo reverts it"
    );
}

#[gpui::test]
fn escape_in_a_multiline_inspector_field_returns_the_keyboard_to_the_timeline(
    cx: &mut TestAppContext,
) {
    if !require_plugins("escape_in_a_multiline_inspector_field") {
        return;
    }
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| e.add_plugin_clip_by_id("org.animathio.text", cx));
    let latex = ui.read(|e| {
        e.selected.or_else(|| {
            e.project
                .tracks()
                .iter()
                .flat_map(|t| t.clips().values())
                .find(|c| matches!(c.kind, animathio_core::ClipKind::Plugin { .. }))
                .map(|c| c.id)
        })
    });
    let latex = latex.expect("the text clip was added");
    ui.update(|editor, _, cx| editor.select_only(latex, cx));
    ui.update(|_, window, _| window.refresh());
    ui.settle();
    ui.update(|_, window, _| window.refresh());
    let state = ui
        .read(|editor| editor.fields.first_textarea())
        .expect("the text clip has a multi-line content field");
    {
        let state = state.clone();
        ui.update(move |_, window, cx| {
            let handle = state.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        });
    }
    assert!(ui.update(|editor, window, cx| editor.fields.any_focused(window, cx)));

    // Enter still inserts a newline in a multi-line field.
    let lines = |ui: &Ui| state.read_with(ui.cx, |s, _| s.value().matches('\n').count());
    let before = lines(&ui);
    ui.press("enter");
    assert_eq!(lines(&ui), before + 1, "enter inserts a newline");
    assert!(ui.update(|editor, window, cx| editor.fields.any_focused(window, cx)));

    // Escape hands the keyboard back to the timeline.
    ui.press("escape");
    assert!(!ui.update(|editor, window, cx| editor.fields.any_focused(window, cx)));
    assert!(ui.update(|editor, window, _| editor.panel_focus[0].is_focused(window)));
}

#[gpui::test]
fn clicking_media_over_an_occupied_playhead_lands_on_a_free_layer(cx: &mut TestAppContext) {
    // The sample has an image clip at 0..3 s; clicking its library card again must not fail
    // with a bare "overlapping clips".
    let (project, assets) = sample();
    let before = project.clone();
    let image = project
        .assets()
        .iter()
        .find(|(_, asset)| asset.media_type.starts_with("image/"))
        .map(|(id, _)| id.clone())
        .expect("the sample has an image");
    let mut ui = Ui::open(cx, project, assets);
    let clips = ui.read(clip_count);
    ui.update(|editor, _, cx| editor.add_asset_at_playhead(image.clone(), cx));
    assert_eq!(last_error(&ui), "", "no error");
    assert_eq!(ui.read(clip_count), clips + 1);
    ui.read(|editor| editor.project.validate().expect("the project stays valid"));
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before, "one undo step");

    // The same for audio: an occupied voice track gets a new audio track instead of an error.
    let (project, assets) = voice_and_music();
    let voice = project
        .assets()
        .keys()
        .next()
        .cloned()
        .expect("an audio asset");
    let mut ui = Ui::open(cx, project, assets);
    let tracks = ui.read(|e| e.project.tracks().len());
    ui.update(|editor, _, cx| editor.add_asset_at_playhead(voice, cx));
    assert_eq!(last_error(&ui), "", "no error");
    assert_eq!(ui.read(|e| e.project.tracks().len()), tracks + 1);
    ui.read(|editor| editor.project.validate().expect("the project stays valid"));
}

#[gpui::test]
fn repeated_audio_clicks_reuse_a_free_audio_track(cx: &mut TestAppContext) {
    use super::fixtures::{MUSIC_CLIP, MUSIC_TRACK};
    // Voice is busy at the playhead; the music track is free there (its clip starts at 10 s).
    let (mut project, assets) = voice_and_music();
    Command::MoveClip {
        id: MUSIC_CLIP,
        track: MUSIC_TRACK,
        start: Time(10_000 * 1_000_000),
    }
    .apply(&mut project)
    .unwrap();
    let voice = project.assets().keys().next().cloned().unwrap();
    let mut ui = Ui::open(cx, project, assets);
    let tracks = ui.read(|e| e.project.tracks().len());
    let clips = ui.read(clip_count);
    ui.update(|editor, _, cx| editor.add_asset_at_playhead(voice.clone(), cx));
    assert_eq!(last_error(&ui), "", "no error");
    assert_eq!(ui.read(clip_count), clips + 1);
    assert_eq!(
        ui.read(|e| e.project.tracks().len()),
        tracks,
        "no new track"
    );
    // Voice and the new clip occupy 0..3 s now; a second click must still not pile up tracks
    // beyond the one it needs, and a third click at a later free spot adds none.
    ui.update(|editor, _, cx| {
        editor.position = Time(5_000 * 1_000_000);
        editor.add_asset_at_playhead(voice.clone(), cx);
    });
    assert_eq!(ui.read(|e| e.project.tracks().len()), tracks);
    ui.read(|editor| editor.project.validate().expect("the project stays valid"));
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

#[test]
fn plugin_errors_lose_their_transport_prefixes() {
    use crate::editor::math::plain_plugin_error;
    assert_eq!(
        plain_plugin_error(
            "plugin produced an invalid scene: service-failed: invalid LaTeX: unbalanced opening brace"
        ),
        "Invalid LaTeX: unbalanced opening brace"
    );
    assert_eq!(plain_plugin_error("boom"), "Boom");
    assert_eq!(plain_plugin_error(""), "");
}

#[gpui::test]
fn inspector_leads_with_content_for_generated_clips_and_audio_for_audio_clips(
    cx: &mut TestAppContext,
) {
    if !require_plugins(
        "inspector_leads_with_content_for_generated_clips_and_audio_for_audio_clips",
    ) {
        return;
    }
    let top = |ui: &mut Ui, section: &str| {
        ui.cx
            .debug_bounds(Box::leak(format!("section-{section}").into_boxed_str()))
            .map(|bounds| f32::from(bounds.origin.y))
    };
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    let text = ui.read(|editor| {
        editor
            .project
            .tracks()
            .iter()
            .flat_map(|track| track.clips().values())
            .find(|clip| {
                matches!(&clip.kind, animathio_core::ClipKind::Plugin { plugin_id, .. }
                    if plugin_id == "org.animathio.text")
            })
            .map(|clip| clip.id)
            .expect("the sample has a text clip")
    });
    ui.update(|editor, _, cx| editor.select_only(text, cx));
    ui.update(|_, window, _| window.refresh());
    let (params, transform) = (top(&mut ui, "params"), top(&mut ui, "transform"));
    assert!(
        params.is_some() && transform.is_some(),
        "content and transform sections render"
    );
    assert!(
        params < transform,
        "a text clip leads with its text, not Speed"
    );
    if let Some(speed) = top(&mut ui, "speed") {
        assert!(
            speed > transform.unwrap(),
            "Speed follows for generated clips"
        );
    }

    let (project, assets) = voice_and_music();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|editor, _, cx| editor.select_only(VOICE_CLIP, cx));
    ui.update(|_, window, _| window.refresh());
    let speed = top(&mut ui, "speed").expect("an audio clip shows Speed");
    let audio = top(&mut ui, "audio").expect("and its Audio section");
    assert!(audio < speed, "audio clips lead with their Audio section");
}
