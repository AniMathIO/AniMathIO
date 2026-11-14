//! Real decoding, transform geometry, transactional undo and stale/cancel guards.
use animathio_control::{ReframeParams, analyse_reframe, reframe_command};
use animathio_core::{
    Asset, AssetId, Clip, ClipId, ClipKind, Command, History, Interpolation, Keyframe, ParamValue,
    Project, TICKS_PER_SECOND, Target, Time, TimeRange, Track, TrackId, TransformProperty,
};
use std::{collections::BTreeMap, sync::atomic::AtomicBool};
fn fixture() -> (Project, BTreeMap<AssetId, Vec<u8>>) {
    let bytes = include_bytes!("../../media/tests/fixtures/sample.mp4").to_vec();
    let id = AssetId::of(&bytes);
    let mut project = Project::default();
    let mut video = Clip::new(
        ClipId(2),
        ClipKind::Video(id.clone()),
        TimeRange::new(Time(TICKS_PER_SECOND), Time(2 * TICKS_PER_SECOND)).unwrap(),
    );
    video.source_offset = Time(TICKS_PER_SECOND / 2);
    video.transform.opacity = 0.6;
    Command::Batch {
        commands: vec![
            Command::AddAsset {
                id: id.clone(),
                asset: Asset {
                    name: "sample.mp4".into(),
                    media_type: "video/mp4".into(),
                },
            },
            Command::AddTrack {
                track: Track::new(TrackId(1), "Video"),
                index: 0,
            },
            Command::AddClip {
                track: TrackId(1),
                clip: video,
            },
            Command::AddKeyframe {
                id: ClipId(2),
                target: Target::Transform(TransformProperty::X),
                time: Time(42),
                keyframe: Keyframe {
                    value: ParamValue::Number(0.2),
                    interpolation: Interpolation::EaseInOut,
                },
            },
        ],
    }
    .apply(&mut project)
    .unwrap();
    (project, BTreeMap::from([(id, bytes)]))
}
#[test]
fn decode_fill_all_aspects_and_exact_single_step_undo_redo() {
    for (aspect, width, height) in [
        ("9:16", 1080, 1920),
        ("1:1", 1080, 1080),
        ("4:5", 1080, 1350),
    ] {
        let (mut project, assets) = fixture();
        let original = project.clone();
        let params = ReframeParams {
            aspect: aspect.into(),
            clips: vec![ClipId(2)],
            ..ReframeParams::default()
        };
        let progress = std::cell::Cell::new(0);
        let draft = analyse_reframe(&project, &assets, &params, &AtomicBool::new(false), &|n| {
            progress.set(n);
        })
        .unwrap();
        assert_eq!(progress.get(), 100);
        assert_eq!(draft.clips, 1);
        assert!(draft.points <= 256);
        let command = reframe_command(&project, &draft).unwrap();
        assert!(matches!(command, Command::Batch { .. }));
        let mut history = History::default();
        history.execute(&mut project, &command).unwrap();
        assert_eq!((project.width(), project.height()), (width, height));
        let clip = &project.tracks()[0].clips()[&ClipId(2)];
        assert_eq!(clip.fit, animathio_core::Fit::Stretch);
        assert!(clip.transform.scale_x >= 1. && clip.transform.scale_y >= 1.);
        assert!((clip.transform.opacity - 0.6).abs() < 1e-12);
        // Source is 320x180: a uniform physical scale, with no stretch.
        let ratio = clip.transform.scale_x * f64::from(width)
            / (clip.transform.scale_y * f64::from(height));
        assert!((ratio - 320. / 180.).abs() < 1e-12);
        for animation in &clip.animations {
            assert!(animation.keys.keys().keys().all(|t| t.0 < TICKS_PER_SECOND));
        }
        let reframed = project.clone();
        assert!(history.undo(&mut project).unwrap());
        assert_eq!(project, original);
        assert!(!history.undo(&mut project).unwrap());
        assert!(history.redo(&mut project).unwrap());
        assert_eq!(project, reframed);
        assert!(reframe_command(&project, &draft).is_err());
    }
}
#[test]
fn cancellation_missing_selection_and_invalid_settings_do_not_mutate() {
    let (project, assets) = fixture();
    assert!(
        analyse_reframe(
            &project,
            &assets,
            &ReframeParams::default(),
            &AtomicBool::new(true),
            &|_| {}
        )
        .is_err()
    );
    for params in [
        ReframeParams {
            aspect: "16:9".into(),
            ..ReframeParams::default()
        },
        ReframeParams {
            smoothness: f64::NAN,
            ..ReframeParams::default()
        },
        ReframeParams {
            clips: vec![ClipId(99)],
            ..ReframeParams::default()
        },
    ] {
        assert!(
            analyse_reframe(&project, &assets, &params, &AtomicBool::new(false), &|_| {}).is_err()
        );
    }
}
#[test]
fn control_call_serializes_and_headless_handler_uses_shared_history() {
    use animathio_control::{Call, ControlHandler, HeadlessHandler};
    let (project, assets) = fixture();
    let original = project.clone();
    let mut handler =
        HeadlessHandler::from_bundle(animathio_format::Bundle { project, assets }, None);
    let call: Call = serde_json::from_value(serde_json::json!({
        "method": "video.reframe",
        "params": {"params": {"aspect": "4:5", "smoothness": 0.8}}
    }))
    .unwrap();
    let result = handler.handle(call).unwrap();
    assert_eq!(result, 1);
    assert_eq!(
        (handler.project().width(), handler.project().height()),
        (1080, 1350)
    );
    assert!(handler.history_undo().unwrap());
    assert_eq!(handler.project(), &original);
}
#[test]
fn locked_rotated_and_changed_projects_are_rejected() {
    let (mut project, assets) = fixture();
    let params = ReframeParams::default();
    let draft =
        analyse_reframe(&project, &assets, &params, &AtomicBool::new(false), &|_| {}).unwrap();
    Command::SetTrackLocked {
        id: TrackId(1),
        locked: true,
    }
    .apply(&mut project)
    .unwrap();
    assert!(reframe_command(&project, &draft).is_err());
    assert!(analyse_reframe(&project, &assets, &params, &AtomicBool::new(false), &|_| {}).is_err());
    Command::SetTrackLocked {
        id: TrackId(1),
        locked: false,
    }
    .apply(&mut project)
    .unwrap();
    let mut transform = project.tracks()[0].clips()[&ClipId(2)].transform;
    transform.rotation = 0.2;
    Command::SetTransform {
        id: ClipId(2),
        transform,
    }
    .apply(&mut project)
    .unwrap();
    assert!(analyse_reframe(&project, &assets, &params, &AtomicBool::new(false), &|_| {}).is_err());
}
#[test]
fn selection_reframes_only_requested_video() {
    let (mut project, assets) = fixture();
    let mut other = project.tracks()[0].clips()[&ClipId(2)].clone();
    other.id = ClipId(3);
    other.range = TimeRange::new(Time(2 * TICKS_PER_SECOND), Time(3 * TICKS_PER_SECOND)).unwrap();
    Command::AddClip {
        track: TrackId(1),
        clip: other.clone(),
    }
    .apply(&mut project)
    .unwrap();
    let params = ReframeParams {
        clips: vec![ClipId(2)],
        ..ReframeParams::default()
    };
    let draft =
        analyse_reframe(&project, &assets, &params, &AtomicBool::new(false), &|_| {}).unwrap();
    assert_eq!(draft.clips, 1);
    reframe_command(&project, &draft)
        .unwrap()
        .apply(&mut project)
        .unwrap();
    assert_eq!(project.tracks()[0].clips()[&ClipId(3)], other);
}
#[test]
fn dense_existing_animation_is_replaced_and_undone_in_one_step() {
    let (mut project, assets) = fixture();
    let additions: Vec<_> = (0..1100)
        .map(|i| Command::AddKeyframe {
            id: ClipId(2),
            target: Target::Transform(TransformProperty::X),
            time: Time(i),
            keyframe: Keyframe {
                value: ParamValue::Number(0.2),
                interpolation: Interpolation::Linear,
            },
        })
        .collect();
    Command::Batch {
        commands: additions
            .chunks(512)
            .map(|commands| Command::Batch {
                commands: commands.to_vec(),
            })
            .collect(),
    }
    .apply(&mut project)
    .unwrap();
    let original = project.clone();
    let draft = analyse_reframe(
        &project,
        &assets,
        &ReframeParams::default(),
        &AtomicBool::new(false),
        &|_| {},
    )
    .unwrap();
    let command = reframe_command(&project, &draft).unwrap();
    let mut history = History::default();
    history.execute(&mut project, &command).unwrap();
    assert!(history.undo(&mut project).unwrap());
    assert_eq!(project, original);
    assert!(!history.undo(&mut project).unwrap());
}
