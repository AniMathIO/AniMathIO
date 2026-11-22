//! Audio effect cards, Auto duck and denoise workflows.

use animathio_core::{EffectId, EffectInstance};

use super::fixtures::{
    MS, MUSIC_CLIP, MUSIC_TRACK, VOICE_CLIP, VOICE_TRACK, sample, voice_and_music,
};
use super::helpers::dialog_ready;
use super::{
    AudioChain, BTreeMap, BTreeSet, ClipId, Command, Editor, MarkerId, Project, TestAppContext,
    Time, Ui, actions, require_plugins,
};
use crate::editor::actions::Undo;

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
