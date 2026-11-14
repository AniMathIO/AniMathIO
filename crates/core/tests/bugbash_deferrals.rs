//! Media fit and lossless merge contracts.
#![allow(clippy::float_cmp)]
use animathio_core::*;
use serde_json::json;

fn range(start: i64, end: i64) -> TimeRange {
    TimeRange::new(Time(start), Time(end)).unwrap()
}
fn project() -> Project {
    let mut project = Project::default();
    let asset = AssetId::of(b"video");
    Command::Batch {
        commands: vec![
            Command::AddAsset {
                id: asset.clone(),
                asset: Asset {
                    name: "video.mp4".into(),
                    media_type: "video/mp4".into(),
                },
            },
            Command::AddTrack {
                track: Track::new(TrackId(1), "Video"),
                index: 0,
            },
            Command::AddClip {
                track: TrackId(1),
                clip: Clip::new(ClipId(1), ClipKind::Video(asset), range(0, 100)),
            },
        ],
    }
    .apply(&mut project)
    .unwrap();
    project
}
fn split() -> Project {
    let mut project = project();
    Command::SplitClip {
        id: ClipId(1),
        right_id: ClipId(2),
        at: Time(50),
    }
    .apply(&mut project)
    .unwrap();
    project
}
fn merge() -> Command {
    Command::MergeClip {
        left: ClipId(1),
        right: ClipId(2),
    }
}

#[test]
fn new_media_fits_and_the_fit_edit_is_serializable_locked_and_undoable() {
    let mut project = project();
    assert_eq!(project.tracks()[0].clips()[&ClipId(1)].fit, Fit::Contain);
    let mut history = History::default();
    for fit in Fit::ALL {
        let before = project.clone();
        let command = Command::SetClipFit { id: ClipId(1), fit };
        let back: Command =
            serde_json::from_value(serde_json::to_value(&command).unwrap()).unwrap();
        history.execute(&mut project, &back).unwrap();
        assert_eq!(evaluate(&project, Time(0)).clips[0].fit, fit);
        assert!(history.undo(&mut project).unwrap());
        assert_eq!(project, before);
    }
    Command::SetTrackLocked {
        id: TrackId(1),
        locked: true,
    }
    .apply(&mut project)
    .unwrap();
    assert!(
        Command::SetClipFit {
            id: ClipId(1),
            fit: Fit::Cover
        }
        .apply(&mut project)
        .is_err()
    );
}

#[test]
fn fit_geometry_respects_portrait_canvas_templates_mirrors_and_project_pixels() {
    assert_eq!(
        Fit::Contain.unit_rect((160, 90), (90, 160)),
        [0., 0.341_796_875, 1., 0.316_406_25]
    );
    assert_eq!(
        Fit::Cover.unit_rect((200, 100), (100, 100)),
        [-0.5, 0., 2., 1.]
    );
    let t = Transform {
        scale_x: -0.5,
        scale_y: 0.25,
        ..Transform::default()
    };
    assert_eq!(
        Fit::Contain.rect((200, 100), (100, 100), t),
        [0., 0., 1., 1.]
    );
    assert_eq!(
        Fit::Original.unit_rect((20, 40), (100, 100)),
        [0.4, 0.3, 0.2, 0.4]
    );
    assert_eq!(
        Fit::Cover.visible_rect((200, 100), (100, 100), t),
        [0., 0., 1., 1.]
    );
}

#[test]
fn merging_refuses_discontinuous_sources_gaps_and_different_settings_atomically() {
    for change in [
        Command::TrimClip {
            id: ClipId(2),
            range: range(50, 100),
            source_offset: Time(51),
        },
        Command::MoveClip {
            id: ClipId(2),
            track: TrackId(1),
            start: Time(51),
        },
        Command::SetClipFit {
            id: ClipId(2),
            fit: Fit::Cover,
        },
        Command::SetTransform {
            id: ClipId(2),
            transform: Transform {
                opacity: 0.5,
                ..Transform::default()
            },
        },
        Command::SetParam {
            id: ClipId(2),
            name: "gain".into(),
            value: Some(0.5),
        },
        Command::AddEffect {
            id: ClipId(2),
            effect: EffectInstance::basic(EffectId(1), "blur"),
            index: 0,
        },
        Command::AddKeyframe {
            id: ClipId(2),
            target: Target::Transform(TransformProperty::X),
            time: Time(0),
            keyframe: Keyframe {
                value: 0.7.into(),
                interpolation: Interpolation::Linear,
            },
        },
    ] {
        let mut project = split();
        change.apply(&mut project).unwrap();
        let before = project.clone();
        assert!(merge().apply(&mut project).is_err(), "accepted {change:?}");
        assert_eq!(project, before);
    }
}

#[test]
fn merge_of_a_split_animated_clip_and_its_undo_are_exact() {
    let mut project = project();
    for (time, value) in [(0, 0.1), (75, 0.9)] {
        Command::AddKeyframe {
            id: ClipId(1),
            target: Target::Transform(TransformProperty::X),
            time: Time(time),
            keyframe: Keyframe {
                value: value.into(),
                interpolation: Interpolation::Linear,
            },
        }
        .apply(&mut project)
        .unwrap();
    }
    let original = project.clone();
    Command::SplitClip {
        id: ClipId(1),
        right_id: ClipId(2),
        at: Time(50),
    }
    .apply(&mut project)
    .unwrap();
    let split = project.clone();
    let mut history = History::default();
    history.execute(&mut project, &merge()).unwrap();
    assert_eq!(project, original);
    assert!(history.undo(&mut project).unwrap());
    assert_eq!(project, split);
    assert!(history.redo(&mut project).unwrap());
    assert_eq!(project, original);
}

#[test]
fn internal_commands_are_detected_at_any_batch_depth() {
    for internal in [
        Command::Exact {
            command: Box::new(Command::RemoveClip { id: ClipId(1) }),
        },
        Command::RestoreClips {
            track: TrackId(1),
            ids: vec![],
            clips: vec![],
        },
    ] {
        assert!(internal.is_internal());
        let nested = Command::Batch {
            commands: vec![Command::Batch {
                commands: vec![internal],
            }],
        };
        assert!(nested.is_internal());
        let back: Command = serde_json::from_value(serde_json::to_value(nested).unwrap()).unwrap();
        assert!(back.is_internal());
    }
    assert!(
        !Command::RenameTrack {
            id: TrackId(1),
            name: "safe".into()
        }
        .is_internal()
    );
    // The fit aliases are also accepted in control command JSON.
    let command: Command =
        serde_json::from_value(json!({"SetClipFit":{"id":1,"fit":"fill"}})).unwrap();
    assert_eq!(
        command,
        Command::SetClipFit {
            id: ClipId(1),
            fit: Fit::Cover
        }
    );
}
