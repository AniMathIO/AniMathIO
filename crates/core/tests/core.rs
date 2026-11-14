//! Core domain, command and evaluation contracts.
#![allow(clippy::float_cmp)]
use animathio_core::*;
use proptest::prelude::*;
use serde_json::json;

fn range(start: i64, end: i64) -> TimeRange {
    TimeRange::new(Time(start), Time(end)).unwrap()
}
fn plugin(id: u64, start: i64, end: i64) -> Clip {
    Clip::new(
        ClipId(id),
        ClipKind::Plugin {
            plugin_id: "org.animathio.test@1".into(),
            params: json!({"unknown": {"preserve": true}}),
        },
        range(start, end),
    )
}
fn base() -> Project {
    let mut project = Project::default();
    for id in 0..2 {
        Command::AddTrack {
            track: Track::new(TrackId(id), "layer"),
            index: usize::try_from(id).unwrap(),
        }
        .apply(&mut project)
        .unwrap();
    }
    Command::AddClip {
        track: TrackId(0),
        clip: plugin(0, 0, 100),
    }
    .apply(&mut project)
    .unwrap();
    project
}
fn key(value: f64, interpolation: Interpolation) -> Keyframe<ParamValue> {
    Keyframe {
        value: value.into(),
        interpolation,
    }
}
fn animate(
    project: &mut Project,
    target: &Target,
    time: i64,
    value: f64,
    interpolation: Interpolation,
) {
    Command::AddKeyframe {
        id: ClipId(0),
        target: target.clone(),
        time: Time(time),
        keyframe: key(value, interpolation),
    }
    .apply(project)
    .unwrap();
}
fn animated() -> Project {
    let mut project = base();
    Command::SetParam {
        id: ClipId(0),
        name: "gain".into(),
        value: Some(0.5),
    }
    .apply(&mut project)
    .unwrap();
    for target in [
        Target::Transform(TransformProperty::X),
        Target::Param("gain".into()),
    ] {
        animate(&mut project, &target, 0, 0.0, Interpolation::EaseInOut);
        animate(&mut project, &target, 100, 1.0, Interpolation::Linear);
    }
    project
}

#[test]
fn exact_common_frame_and_sample_boundaries() {
    for rate in [24, 25, 30, 50, 60, 120, 240] {
        let fps = FrameRate::new(rate, 1).unwrap();
        let frame = fps.frame_to_time(1, Rounding::Nearest).unwrap();
        assert_eq!(frame.0 * i64::from(rate), TICKS_PER_SECOND);
    }
    for numerator in [24_000, 30_000, 60_000, 120_000] {
        let fps = FrameRate::new(numerator, 1001).unwrap();
        for index in [-100_000, -1, 0, 1, 100_000] {
            let time = fps.frame_to_time(index, Rounding::Nearest).unwrap();
            assert_eq!(fps.time_to_frame(time, Rounding::Nearest).unwrap(), index);
        }
    }
    for sample_rate in [44_100, 48_000, 88_200, 96_000, 192_000] {
        assert_eq!(TICKS_PER_SECOND % sample_rate, 0);
    }
}

#[test]
fn explicit_signed_rounding_and_overflow() {
    let fps = FrameRate::new(2, 1).unwrap();
    let half_frame = Time(TICKS_PER_SECOND / 4);
    assert_eq!(fps.time_to_frame(half_frame, Rounding::Floor).unwrap(), 0);
    assert_eq!(fps.time_to_frame(half_frame, Rounding::Ceil).unwrap(), 1);
    assert_eq!(fps.time_to_frame(half_frame, Rounding::Nearest).unwrap(), 1);
    assert_eq!(
        fps.time_to_frame(Time(-half_frame.0), Rounding::Floor)
            .unwrap(),
        -1
    );
    assert_eq!(
        fps.time_to_frame(Time(-half_frame.0), Rounding::Ceil)
            .unwrap(),
        0
    );
    assert_eq!(
        fps.time_to_frame(Time(-half_frame.0), Rounding::Nearest)
            .unwrap(),
        -1
    );
    assert!(fps.frame_to_time(i64::MAX, Rounding::Nearest).is_err());
    assert!(FrameRate::new(0, 1).is_err());
    assert!(FrameRate::new(1, 0).is_err());
    assert_eq!(FrameRate::new(60, 2).unwrap(), FrameRate::default());
    assert!(serde_json::from_str::<FrameRate>(r#"{"numerator":1,"denominator":0}"#).is_err());
}

#[test]
fn fractional_tick_frames_round_explicitly() {
    let fps = FrameRate::new(11, 1).unwrap();
    let floor = fps.frame_to_time(1, Rounding::Floor).unwrap();
    let ceil = fps.frame_to_time(1, Rounding::Ceil).unwrap();
    assert_eq!(ceil.0, floor.0 + 1);
    assert_eq!(fps.frame_to_time(-1, Rounding::Floor).unwrap().0, -ceil.0);
}

#[test]
fn hashes_are_canonical_content_addresses() {
    let id = AssetId::of(b"media");
    assert_eq!(id.as_str().len(), 64);
    assert_eq!(AssetId::try_from(id.as_str().to_owned()).unwrap(), id);
    assert!(AssetId::try_from("../media".to_owned()).is_err());
    assert!(AssetId::try_from("A".repeat(64)).is_err());
    assert_ne!(id, AssetId::of(b"other media"));
}

#[test]
fn linear_hold_easing_and_endpoint_clamping() {
    for (mode, expected) in [
        (Interpolation::Linear, 0.25),
        (Interpolation::Hold, 0.0),
        (Interpolation::EaseInOut, 0.15625),
    ] {
        let mut project = base();
        let target = Target::Transform(TransformProperty::X);
        animate(&mut project, &target, 10, 0.0, mode);
        animate(&mut project, &target, 90, 1.0, Interpolation::Linear);
        assert_eq!(evaluate(&project, Time(30)).clips[0].transform.x, expected);
        assert_eq!(evaluate(&project, Time(0)).clips[0].transform.x, 0.0);
        assert_eq!(evaluate(&project, Time(90)).clips[0].transform.x, 1.0);
        assert_eq!(evaluate(&project, Time(99)).clips[0].transform.x, 1.0);
    }
    assert_eq!(Keyframes::<f64>::default().sample(Time(0)), None);
}

#[test]
fn evaluation_uses_half_open_ranges_local_ticks_and_layer_order() {
    let mut project = animated();
    Command::TrimClip {
        id: ClipId(0),
        range: range(20, 120),
        source_offset: Time(500),
    }
    .apply(&mut project)
    .unwrap();
    Command::AddClip {
        track: TrackId(1),
        clip: plugin(1, 20, 120),
    }
    .apply(&mut project)
    .unwrap();
    assert_eq!(evaluate(&project, Time(19)).clips, Vec::<SceneClip>::new());
    let scene = evaluate(&project, Time(70));
    assert_eq!(
        scene.clips.iter().map(|c| c.track).collect::<Vec<_>>(),
        vec![TrackId(0), TrackId(1)]
    );
    assert_eq!(scene.clips[0].source_time, Time(550));
    assert_eq!(scene.clips[0].transform.x, 0.5);
    assert_eq!(
        scene.clips[0].params["gain"],
        animathio_core::ParamValue::Number(0.5)
    );
    assert_eq!(evaluate(&project, Time(120)).clips, Vec::<SceneClip>::new());
    assert_eq!(
        evaluate(&project, Time(i64::MIN)).clips,
        Vec::<SceneClip>::new()
    );
}

#[test]
#[allow(clippy::too_many_lines)] // Explicit coverage of every command/inverse pair.
fn all_command_variants_restore_exact_data() {
    let mut project = Project::default();
    let asset_id = AssetId::of(b"media");
    let mut commands = vec![
        Command::AddTrack {
            track: Track::new(TrackId(0), "one"),
            index: 0,
        },
        Command::AddTrack {
            track: Track::new(TrackId(1), "two"),
            index: 1,
        },
        Command::AddAsset {
            id: asset_id.clone(),
            asset: Asset {
                name: "test".into(),
                media_type: "video/mp4".into(),
            },
        },
    ];
    for (i, kind) in [
        ClipKind::Video(asset_id.clone()),
        ClipKind::Image(asset_id.clone()),
        ClipKind::Audio(asset_id.clone()),
    ]
    .into_iter()
    .enumerate()
    {
        let i = i64::try_from(i).unwrap();
        commands.push(Command::AddClip {
            track: TrackId(0),
            clip: Clip::new(
                ClipId(u64::try_from(i).unwrap()),
                kind,
                range(i * 100, (i + 1) * 100),
            ),
        });
    }
    commands.extend([
        Command::MoveClip {
            id: ClipId(0),
            track: TrackId(1),
            start: Time(5),
        },
        Command::TrimClip {
            id: ClipId(0),
            range: range(10, 90),
            source_offset: Time(5),
        },
        Command::SetTransform {
            id: ClipId(0),
            transform: Transform {
                x: -0.2,
                scale_x: -1.0,
                ..Transform::default()
            },
        },
        Command::SetParam {
            id: ClipId(0),
            name: "gain".into(),
            value: Some(0.75),
        },
        Command::AddKeyframe {
            id: ClipId(0),
            target: Target::Param("gain".into()),
            time: Time(0),
            keyframe: key(0.1, Interpolation::Hold),
        },
        Command::AddKeyframe {
            id: ClipId(0),
            target: Target::Transform(TransformProperty::X),
            time: Time(0),
            keyframe: key(0.3, Interpolation::Linear),
        },
        Command::AddKeyframe {
            id: ClipId(0),
            target: Target::Param("gain".into()),
            time: Time(0),
            keyframe: key(0.9, Interpolation::EaseInOut),
        },
        Command::RemoveKeyframe {
            id: ClipId(0),
            target: Target::Transform(TransformProperty::X),
            time: Time(0),
        },
        Command::RemoveKeyframe {
            id: ClipId(0),
            target: Target::Param("gain".into()),
            time: Time(0),
        },
        Command::SetParam {
            id: ClipId(0),
            name: "gain".into(),
            value: None,
        },
        Command::RemoveClip { id: ClipId(0) },
        Command::RemoveTrack { id: TrackId(0) },
        Command::RemoveAsset { id: asset_id },
    ]);
    for command in commands {
        let before = project.clone();
        let encoded = serde_json::to_string(&command).unwrap();
        assert_eq!(serde_json::from_str::<Command>(&encoded).unwrap(), command);
        let inverse = command.apply(&mut project).unwrap();
        let after = project.clone();
        inverse.apply(&mut project).unwrap();
        assert_eq!(project, before, "inverse of {command:?}");
        command.apply(&mut project).unwrap();
        assert_eq!(project, after);
    }
}

#[test]
fn invalid_edits_are_atomic_and_adjacent_ranges_are_valid() {
    let mut project = base();
    Command::AddClip {
        track: TrackId(0),
        clip: plugin(1, 100, 200),
    }
    .apply(&mut project)
    .unwrap();
    let before = project.clone();
    let invalid = [
        Command::AddClip {
            track: TrackId(0),
            clip: plugin(2, 99, 120),
        },
        Command::AddClip {
            track: TrackId(1),
            clip: plugin(0, 200, 300),
        },
        Command::AddTrack {
            track: Track::new(TrackId(0), "duplicate"),
            index: 0,
        },
        Command::MoveClip {
            id: ClipId(0),
            track: TrackId(0),
            start: Time(1),
        },
        Command::MoveClip {
            id: ClipId(0),
            track: TrackId(1),
            start: Time(i64::MAX),
        },
        Command::TrimClip {
            id: ClipId(0),
            range: TimeRange {
                start: Time(-1),
                end: Time(0),
            },
            source_offset: Time(0),
        },
        Command::TrimClip {
            id: ClipId(0),
            range: range(0, 10),
            source_offset: Time(i64::MAX),
        },
        Command::SetParam {
            id: ClipId(0),
            name: "bad".into(),
            value: Some(f64::NAN),
        },
        Command::SetTransform {
            id: ClipId(0),
            transform: Transform {
                opacity: 2.0,
                ..Transform::default()
            },
        },
        Command::AddKeyframe {
            id: ClipId(0),
            target: Target::Effect {
                effect: EffectId(999),
                param: "absent".into(),
            },
            time: Time(0),
            keyframe: key(1.0, Interpolation::Linear),
        },
        Command::AddKeyframe {
            id: ClipId(0),
            target: Target::Transform(TransformProperty::Opacity),
            time: Time(0),
            keyframe: key(-1.0, Interpolation::Linear),
        },
    ];
    for command in invalid {
        assert!(command.apply(&mut project).is_err(), "{command:?}");
        assert_eq!(project, before);
    }
    project.validate().unwrap();
}

#[test]
fn assets_cannot_be_removed_while_referenced() {
    let mut project = base();
    let id = AssetId::of(b"image");
    Command::AddAsset {
        id: id.clone(),
        asset: Asset {
            name: "image".into(),
            media_type: "image/png".into(),
        },
    }
    .apply(&mut project)
    .unwrap();
    Command::AddClip {
        track: TrackId(1),
        clip: Clip::new(ClipId(1), ClipKind::Image(id.clone()), range(0, 10)),
    }
    .apply(&mut project)
    .unwrap();
    let before = project.clone();
    assert!(Command::RemoveAsset { id }.apply(&mut project).is_err());
    assert_eq!(project, before);
}

#[test]
fn deserialization_rejects_invalid_projects() {
    let project = base();
    let mut value = serde_json::to_value(&project).unwrap();
    value["tracks"][0]["clips"]["0"]["range"]["end"] = json!(0);
    assert!(serde_json::from_value::<Project>(value).is_err());
    let mut value = serde_json::to_value(&project).unwrap();
    value["tracks"][0]["clips"]["0"]["id"] = json!(1);
    assert!(serde_json::from_value::<Project>(value).is_err());
}

#[test]
fn failed_history_edit_keeps_redo_and_new_edit_clears_it() {
    let mut project = base();
    let mut history = History::default();
    assert!(!history.undo(&mut project).unwrap());
    assert!(!history.redo(&mut project).unwrap());
    let command = Command::MoveClip {
        id: ClipId(0),
        track: TrackId(1),
        start: Time(10),
    };
    history.execute(&mut project, &command).unwrap();
    history.undo(&mut project).unwrap();
    assert!(
        history
            .execute(&mut project, &Command::RemoveClip { id: ClipId(999) })
            .is_err()
    );
    assert!(history.redo(&mut project).unwrap());
    history.undo(&mut project).unwrap();
    history
        .execute(
            &mut project,
            &Command::SetParam {
                id: ClipId(0),
                name: "a".into(),
                value: Some(1.0),
            },
        )
        .unwrap();
    assert!(!history.redo(&mut project).unwrap());
}

fn generated_command(project: &Project, action: u8, id: u8, value: i16) -> Command {
    let clip_id = ClipId(u64::from(id % 12));
    let track_id = TrackId(u64::from(id % 4));
    let start = Time(i64::from(value.unsigned_abs() % 1000));
    match action % 14 {
        0 => Command::AddTrack {
            track: Track::new(track_id, "generated"),
            index: project.tracks().len(),
        },
        1 => Command::RemoveTrack { id: track_id },
        2 => Command::AddClip {
            track: track_id,
            clip: plugin(clip_id.0, start.0, start.0 + 20),
        },
        3 => Command::RemoveClip { id: clip_id },
        4 => Command::MoveClip {
            id: clip_id,
            track: track_id,
            start,
        },
        5 => Command::TrimClip {
            id: clip_id,
            range: range(start.0, start.0 + 10),
            source_offset: Time(7),
        },
        6 => Command::SetParam {
            id: clip_id,
            name: "gain".into(),
            value: Some(f64::from(value) / 100.0),
        },
        7 => Command::SetParam {
            id: clip_id,
            name: "gain".into(),
            value: None,
        },
        8 => Command::AddKeyframe {
            id: clip_id,
            target: Target::Param("gain".into()),
            time: Time(i64::from(id % 4)),
            keyframe: key(f64::from(value), Interpolation::Linear),
        },
        9 => Command::RemoveKeyframe {
            id: clip_id,
            target: Target::Param("gain".into()),
            time: Time(i64::from(id % 4)),
        },
        10 => Command::SetTransform {
            id: clip_id,
            transform: Transform {
                x: f64::from(value) / 100.0,
                ..Transform::default()
            },
        },
        11 => Command::AddAsset {
            id: AssetId::of(&[id]),
            asset: Asset {
                name: id.to_string(),
                media_type: "test".into(),
            },
        },
        12 => Command::RemoveAsset {
            id: AssetId::of(&[id]),
        },
        _ => Command::AddKeyframe {
            id: clip_id,
            target: Target::Transform(TransformProperty::X),
            time: start,
            keyframe: key(f64::from(value), Interpolation::EaseInOut),
        },
    }
}

proptest! {
    #[test]
    fn command_sequences_preserve_validation_and_inverse_identity(actions in prop::collection::vec((any::<u8>(), any::<u8>(), any::<i16>()), 1..120)) {
        let mut project = base();
        let initial = project.clone();
        let mut history = History::default();
        let mut accepted = 0;
        for (action, id, value) in actions {
            let command = generated_command(&project, action, id, value);
            let before = project.clone();
            let encoded = serde_json::to_vec(&command).unwrap();
            prop_assert_eq!(serde_json::from_slice::<Command>(&encoded).unwrap(), command.clone());
            if let Ok(inverse) = command.apply(&mut project) {
                    let after = project.clone();
                    let inverse = serde_json::from_slice::<Command>(&serde_json::to_vec(&inverse).unwrap()).unwrap();
                    inverse.apply(&mut project).unwrap();
                    prop_assert_eq!(&project, &before);
                    history.execute(&mut project, &command).unwrap();
                    prop_assert_eq!(&project, &after);
                    accepted += 1;
            } else {
                prop_assert_eq!(&project, &before);
            }
            prop_assert!(project.validate().is_ok());
            prop_assert_eq!(serde_json::from_slice::<Project>(&serde_json::to_vec(&project).unwrap()).unwrap(), project.clone());
        }
        let edited = project.clone();
        for _ in 0..accepted { prop_assert!(history.undo(&mut project).unwrap()); }
        prop_assert_eq!(&project, &initial);
        for _ in 0..accepted { prop_assert!(history.redo(&mut project).unwrap()); }
        prop_assert_eq!(&project, &edited);
        // undo(redo(x)) == x for the complete generated history as well.
        for _ in 0..accepted { history.undo(&mut project).unwrap(); }
        prop_assert_eq!(project, initial);
    }

    #[test]
    fn evaluation_is_independent_of_seek_order(times in prop::collection::vec(-100_i64..200, 0..100), target in -100_i64..200) {
        let project = animated();
        let expected = evaluate(&project, Time(target));
        let scenes: Vec<_> = times.iter().map(|t| evaluate(&project, Time(*t))).collect();
        for (time, scene) in times.iter().zip(&scenes).rev() {
            prop_assert_eq!(evaluate(&project, Time(*time)), scene.clone());
        }
        prop_assert_eq!(evaluate(&project, Time(target)), expected);
    }

    #[test]
    fn frame_conversion_roundtrips(frame in -1_000_000_i64..1_000_000, numerator in prop_oneof![Just(24_000_u32), Just(30_000), Just(60_000)], denominator in prop_oneof![Just(1_u32), Just(1001)]) {
        let fps = FrameRate::new(numerator, denominator).unwrap();
        let time = fps.frame_to_time(frame, Rounding::Nearest).unwrap();
        prop_assert_eq!(fps.time_to_frame(time, Rounding::Nearest).unwrap(), frame);
    }
}

#[test]
fn project_settings_default_for_old_json_without_the_new_fields() {
    // Older v1 project.json files predate the width/height/background/duration
    // fields; serde defaults keep them loadable with an unset resolution.
    let project = base();
    let mut value = serde_json::to_value(&project).unwrap();
    let object = value.as_object_mut().unwrap();
    for key in ["width", "height", "background", "duration"] {
        object.remove(key);
    }
    let decoded: Project = serde_json::from_value(value).unwrap();
    assert_eq!(decoded.width(), 0);
    assert_eq!(decoded.height(), 0);
    assert_eq!(decoded.background(), [0, 0, 0, 0]);
    assert_eq!(decoded.duration(), Time(0));
}

#[test]
fn split_and_merge_are_single_step_inverses() {
    let mut project = base();
    let original = project.clone();
    let split = Command::SplitClip {
        id: ClipId(0),
        right_id: ClipId(7),
        at: Time(40),
    };
    let inverse = split.apply(&mut project).unwrap();
    assert_eq!(project.tracks()[0].clips()[&ClipId(0)].range, range(0, 40));
    assert_eq!(
        project.tracks()[0].clips()[&ClipId(7)].range,
        range(40, 100)
    );
    assert_eq!(
        project.tracks()[0].clips()[&ClipId(7)].source_offset,
        Time(40)
    );
    inverse.apply(&mut project).unwrap();
    assert_eq!(project, original);
    let inverse = split.apply(&mut project).unwrap();
    assert!(matches!(inverse, Command::RestoreClips { .. }));
    inverse.apply(&mut project).unwrap();
    assert_eq!(project, original);
}

#[test]
fn split_rejects_collisions_and_out_of_range_points() {
    let mut project = base();
    Command::AddClip {
        track: TrackId(1),
        clip: plugin(1, 0, 100),
    }
    .apply(&mut project)
    .unwrap();
    let before = project.clone();
    for invalid in [
        Command::SplitClip {
            id: ClipId(0),
            right_id: ClipId(0),
            at: Time(50),
        },
        Command::SplitClip {
            id: ClipId(0),
            right_id: ClipId(1),
            at: Time(50),
        },
        Command::SplitClip {
            id: ClipId(0),
            right_id: ClipId(9),
            at: Time(0),
        },
        Command::SplitClip {
            id: ClipId(0),
            right_id: ClipId(9),
            at: Time(100),
        },
    ] {
        assert!(invalid.apply(&mut project).is_err(), "{invalid:?}");
        assert_eq!(project, before);
    }
}

#[test]
fn track_gain_is_validated_reversible_and_defaults_for_old_projects() {
    let mut project = base();
    assert_eq!(project.tracks()[0].gain(), 1.0);
    let original = project.clone();
    let inverse = Command::SetTrackGain {
        id: TrackId(0),
        gain: 0.25,
    }
    .apply(&mut project)
    .unwrap();
    assert_eq!(project.tracks()[0].gain(), 0.25);
    let decoded: Project = serde_json::from_str(&serde_json::to_string(&project).unwrap()).unwrap();
    assert_eq!(decoded, project);
    inverse.apply(&mut project).unwrap();
    assert_eq!(project, original);
    for gain in [-1.0, f64::NAN, f64::INFINITY] {
        assert!(
            Command::SetTrackGain {
                id: TrackId(0),
                gain
            }
            .apply(&mut project)
            .is_err()
        );
        assert_eq!(project, original);
    }
    let mut json = serde_json::to_value(&project).unwrap();
    for track in json["tracks"].as_array_mut().unwrap() {
        track.as_object_mut().unwrap().remove("gain");
    }
    let decoded: Project = serde_json::from_value(json).unwrap();
    assert_eq!(decoded, original);
}

#[test]
fn track_mute_solo_and_lock_are_reversible_and_default_for_old_projects() {
    let mut project = base();
    assert!(!project.tracks()[0].is_muted());
    assert!(!project.tracks()[0].is_solo());
    assert!(!project.tracks()[0].is_locked());
    let original = project.clone();
    for command in [
        Command::SetTrackMuted {
            id: TrackId(0),
            muted: true,
        },
        Command::SetTrackSolo {
            id: TrackId(1),
            solo: true,
        },
        Command::SetTrackLocked {
            id: TrackId(0),
            locked: true,
        },
    ] {
        let inverse = command.apply(&mut project).unwrap();
        assert_ne!(project, original, "{command:?} must change the project");
        let decoded: Project =
            serde_json::from_str(&serde_json::to_string(&project).unwrap()).unwrap();
        assert_eq!(decoded, project);
        inverse.apply(&mut project).unwrap();
        assert_eq!(project, original);
    }
    // Missing fields from pre-flags projects decode to the unlocked defaults.
    let mut json = serde_json::to_value(&project).unwrap();
    for track in json["tracks"].as_array_mut().unwrap() {
        let object = track.as_object_mut().unwrap();
        object.remove("muted");
        object.remove("solo");
        object.remove("locked");
    }
    let decoded: Project = serde_json::from_value(json).unwrap();
    assert_eq!(decoded, original);
}

#[test]
fn batch_is_atomic_one_undo_step_and_allows_nesting() {
    let mut project = base();
    Command::AddClip {
        track: TrackId(0),
        clip: plugin(1, 200, 300),
    }
    .apply(&mut project)
    .unwrap();
    let original = project.clone();
    let move_clip = |start| Command::MoveClip {
        id: ClipId(1),
        track: TrackId(0),
        start: Time(start),
    };
    let mut history = History::default();
    history
        .execute(
            &mut project,
            &Command::Batch {
                commands: vec![move_clip(210), move_clip(220)],
            },
        )
        .unwrap();
    assert_ne!(project, original);
    assert!(history.undo(&mut project).unwrap());
    assert_eq!(project, original);
    assert!(history.redo(&mut project).unwrap());
    assert_eq!(
        project.tracks()[0].clips()[&ClipId(1)].range.start,
        Time(220)
    );

    // A failing member rolls the whole batch back.
    let before = project.clone();
    let failing = Command::Batch {
        commands: vec![move_clip(400), Command::RemoveClip { id: ClipId(99) }],
    };
    assert!(failing.apply(&mut project).is_err());
    assert_eq!(project, before);
    // Nesting is allowed: inverses of compound edits are themselves batches.
    let nested = Command::Batch {
        commands: vec![Command::Batch { commands: vec![] }],
    };
    assert!(nested.apply(&mut project).is_ok());
    // A batch touching a locked layer is rejected.
    Command::SetTrackLocked {
        id: TrackId(0),
        locked: true,
    }
    .apply(&mut project)
    .unwrap();
    let locked = Command::Batch {
        commands: vec![move_clip(7)],
    };
    assert!(locked.apply(&mut project).unwrap_err().0.contains("locked"));
}

#[test]
fn locked_tracks_reject_clip_edits_but_can_be_unlocked() {
    let mut project = base();
    Command::AddClip {
        track: TrackId(0),
        clip: plugin(1, 200, 300),
    }
    .apply(&mut project)
    .unwrap();
    Command::SetTrackLocked {
        id: TrackId(0),
        locked: true,
    }
    .apply(&mut project)
    .unwrap();
    let before = project.clone();
    for command in [
        Command::RemoveClip { id: ClipId(1) },
        Command::MoveClip {
            id: ClipId(1),
            track: TrackId(0),
            start: Time(10),
        },
        Command::AddClip {
            track: TrackId(0),
            clip: plugin(2, 400, 500),
        },
    ] {
        let error = command.apply(&mut project).unwrap_err();
        assert!(error.0.contains("locked"), "{error:?}");
        assert_eq!(project, before);
    }
    // Another layer stays editable and the lock itself can always be undone.
    Command::AddClip {
        track: TrackId(1),
        clip: plugin(3, 0, 50),
    }
    .apply(&mut project)
    .unwrap();
    Command::SetTrackLocked {
        id: TrackId(0),
        locked: false,
    }
    .apply(&mut project)
    .unwrap();
    Command::RemoveClip { id: ClipId(1) }
        .apply(&mut project)
        .unwrap();
}

#[test]
fn moving_onto_a_locked_track_is_rejected() {
    let mut project = base();
    Command::AddClip {
        track: TrackId(0),
        clip: plugin(1, 200, 300),
    }
    .apply(&mut project)
    .unwrap();
    Command::SetTrackLocked {
        id: TrackId(1),
        locked: true,
    }
    .apply(&mut project)
    .unwrap();
    let before = project.clone();
    let onto_locked = Command::MoveClip {
        id: ClipId(1),
        track: TrackId(1),
        start: Time(0),
    };
    assert!(
        onto_locked
            .apply(&mut project)
            .unwrap_err()
            .0
            .contains("locked")
    );
    assert_eq!(project, before);
}

#[test]
fn batch_locks_are_checked_against_the_state_each_member_sees() {
    let mut project = base();
    Command::AddClip {
        track: TrackId(0),
        clip: plugin(1, 200, 300),
    }
    .apply(&mut project)
    .unwrap();
    let lock = |locked| Command::SetTrackLocked {
        id: TrackId(0),
        locked,
    };
    let nudge = Command::MoveClip {
        id: ClipId(1),
        track: TrackId(0),
        start: Time(210),
    };
    // Locking first and then editing the layer is refused, and rolled back.
    let before = project.clone();
    let lock_then_edit = Command::Batch {
        commands: vec![lock(true), nudge.clone()],
    };
    assert!(
        lock_then_edit
            .apply(&mut project)
            .unwrap_err()
            .0
            .contains("locked")
    );
    assert_eq!(project, before);
    // Unlocking first makes the edit legal.
    lock(true).apply(&mut project).unwrap();
    let unlock_then_edit = Command::Batch {
        commands: vec![lock(false), nudge, lock(true)],
    };
    let mut history = History::default();
    history.execute(&mut project, &unlock_then_edit).unwrap();
    assert_eq!(
        project.tracks()[0].clips()[&ClipId(1)].range.start,
        Time(210)
    );
    // Undo and redo replay history even though the layer is locked again.
    let after = project.clone();
    let locked_before = {
        let mut p = before.clone();
        lock(true).apply(&mut p).unwrap();
        p
    };
    assert!(history.undo(&mut project).unwrap());
    assert_eq!(project, locked_before);
    assert!(history.redo(&mut project).unwrap());
    assert_eq!(project, after);
}

#[test]
fn undo_of_an_add_that_locked_its_track_restores_the_project() {
    let mut project = base();
    let before = project.clone();
    let add_and_lock = Command::Batch {
        commands: vec![
            Command::AddClip {
                track: TrackId(0),
                clip: plugin(1, 200, 300),
            },
            Command::SetTrackLocked {
                id: TrackId(0),
                locked: true,
            },
        ],
    };
    let mut history = History::default();
    history.execute(&mut project, &add_and_lock).unwrap();
    assert!(history.undo(&mut project).unwrap());
    assert_eq!(project, before);
}

#[test]
fn evaluate_honours_track_mute_and_solo() {
    let mut project = base();
    Command::AddClip {
        track: TrackId(1),
        clip: plugin(1, 0, 100),
    }
    .apply(&mut project)
    .unwrap();
    let both = evaluate(&project, Time(50));
    assert_eq!(both.clips.len(), 2);

    // Muting a track hides its clips from the resolved scene.
    Command::SetTrackMuted {
        id: TrackId(0),
        muted: true,
    }
    .apply(&mut project)
    .unwrap();
    assert_eq!(
        evaluate(&project, Time(50))
            .clips
            .iter()
            .map(|clip| clip.track)
            .collect::<Vec<_>>(),
        vec![TrackId(1)]
    );

    // Solo beats mute: only soloed tracks resolve while any track is soloed.
    Command::SetTrackMuted {
        id: TrackId(0),
        muted: false,
    }
    .apply(&mut project)
    .unwrap();
    Command::SetTrackSolo {
        id: TrackId(0),
        solo: true,
    }
    .apply(&mut project)
    .unwrap();
    assert_eq!(
        evaluate(&project, Time(50))
            .clips
            .iter()
            .map(|clip| clip.track)
            .collect::<Vec<_>>(),
        vec![TrackId(0)]
    );
}

#[test]
fn move_keyframe_is_reversible_and_rejects_occupied_destinations() {
    let mut project = animated();
    let original = project.clone();
    let target = Target::Param("gain".into());

    let inverse = Command::MoveKeyframe {
        id: ClipId(0),
        target: target.clone(),
        from: Time(0),
        to: Time(50),
    }
    .apply(&mut project)
    .unwrap();
    // The keyframe moved and the old slot is free.
    let clip = project.tracks()[0].clips()[&ClipId(0)].clone();
    let animation = clip.animations.iter().find(|a| a.target == target).unwrap();
    assert!(!animation.keys.keys().contains_key(&Time(0)));
    assert!(animation.keys.keys().contains_key(&Time(50)));

    inverse.apply(&mut project).unwrap();
    assert_eq!(project, original);

    // Destination collision and same-time moves are rejected atomically.
    for command in [
        Command::MoveKeyframe {
            id: ClipId(0),
            target: target.clone(),
            from: Time(0),
            to: Time(100),
        },
        Command::MoveKeyframe {
            id: ClipId(0),
            target: target.clone(),
            from: Time(0),
            to: Time(0),
        },
        Command::MoveKeyframe {
            id: ClipId(0),
            target: target.clone(),
            from: Time(0),
            to: Time(-1),
        },
    ] {
        let before = project.clone();
        assert!(command.apply(&mut project).is_err(), "{command:?}");
        assert_eq!(project, before);
    }
}

#[test]
fn set_plugin_param_preserves_opaque_json_and_is_reversible() {
    let mut project = base();
    let original = project.clone();
    let value = ParamValue::String("hello".into());

    let inverse = Command::SetPluginParam {
        id: ClipId(0),
        name: "opaque".into(),
        value: Some(value.clone()),
    }
    .apply(&mut project)
    .unwrap();
    let clip = project.tracks()[0].clips()[&ClipId(0)].clone();
    match &clip.kind {
        ClipKind::Plugin { params, .. } => {
            assert!(params.get("opaque").is_none());
            assert_eq!(clip.params["opaque"], value);
        }
        other => panic!("expected plugin clip, got {other:?}"),
    }
    inverse.apply(&mut project).unwrap();
    assert_eq!(project, original);

    // Removing an absent key is a reversible no-op, matching `SetParam`.
    let inverse = Command::SetPluginParam {
        id: ClipId(0),
        name: "missing".into(),
        value: None,
    }
    .apply(&mut project)
    .unwrap();
    assert_eq!(project, original);
    inverse.apply(&mut project).unwrap();
    assert_eq!(project, original);
}

#[test]
fn animated_split_preserves_boundary_and_exact_undo_redo() {
    let mut project = base();
    for (time, value) in [(0, 0.0), (100, 1.0)] {
        Command::AddKeyframe {
            id: ClipId(0),
            target: Target::Transform(TransformProperty::Opacity),
            time: Time(time),
            keyframe: Keyframe {
                value: ParamValue::Number(value),
                interpolation: Interpolation::Linear,
            },
        }
        .apply(&mut project)
        .unwrap();
    }
    let original = project.clone();
    let undo = Command::SplitClip {
        id: ClipId(0),
        right_id: ClipId(9),
        at: Time(50),
    }
    .apply(&mut project)
    .unwrap();
    let split = project.clone();
    assert_eq!(
        project.tracks()[0].clips()[&ClipId(9)].animations[0]
            .keys
            .sample(Time(0)),
        Some(ParamValue::Number(0.5))
    );
    let redo = undo.apply(&mut project).unwrap();
    assert_eq!(project, original);
    redo.apply(&mut project).unwrap();
    assert_eq!(project, split);
}

#[test]
fn merge_undo_restores_independent_clip_data() {
    let mut project = base();
    let mut right = plugin(9, 100, 200);
    right.source_offset = Time(500);
    right.transform.opacity = 0.25;
    Command::AddClip {
        track: TrackId(0),
        clip: right,
    }
    .apply(&mut project)
    .unwrap();
    let original = project.clone();
    let undo = Command::MergeClip {
        left: ClipId(0),
        right: ClipId(9),
    }
    .apply(&mut project)
    .unwrap();
    let merged = project.clone();
    let redo = undo.apply(&mut project).unwrap();
    assert_eq!(project, original);
    redo.apply(&mut project).unwrap();
    assert_eq!(project, merged);
}

#[test]
fn track_rename_and_move_are_reversible_and_validate_their_targets() {
    let mut project = base();
    Command::AddTrack {
        track: Track::new(TrackId(2), "top"),
        index: 2,
    }
    .apply(&mut project)
    .unwrap();
    let original = project.clone();
    let order = |project: &Project| project.tracks().iter().map(Track::id).collect::<Vec<_>>();

    let rename = Command::RenameTrack {
        id: TrackId(1),
        name: "Titles".into(),
    };
    let inverse = rename.apply(&mut project).unwrap();
    assert_eq!(project.tracks()[1].name(), "Titles");
    inverse.apply(&mut project).unwrap();
    assert_eq!(project, original);

    let to_front = Command::MoveTrack {
        id: TrackId(2),
        index: 0,
    };
    let inverse = to_front.apply(&mut project).unwrap();
    assert_eq!(order(&project), [TrackId(2), TrackId(0), TrackId(1)]);
    // Clips travel with their layer.
    assert_eq!(project.tracks()[1].clips().len(), 1);
    let json = serde_json::to_string(&to_front).unwrap();
    assert_eq!(serde_json::from_str::<Command>(&json).unwrap(), to_front);
    inverse.apply(&mut project).unwrap();
    assert_eq!(project, original);

    for bad in [
        Command::MoveTrack {
            id: TrackId(0),
            index: 3,
        },
        Command::MoveTrack {
            id: TrackId(9),
            index: 0,
        },
        Command::RenameTrack {
            id: TrackId(9),
            name: "x".into(),
        },
    ] {
        assert!(bad.apply(&mut project).is_err(), "{bad:?}");
        assert_eq!(project, original, "failed commands leave no trace");
    }

    // Layer-level edits stay possible on a locked layer.
    Command::SetTrackLocked {
        id: TrackId(0),
        locked: true,
    }
    .apply(&mut project)
    .unwrap();
    Command::RenameTrack {
        id: TrackId(0),
        name: "Locked".into(),
    }
    .apply(&mut project)
    .unwrap();
    Command::MoveTrack {
        id: TrackId(0),
        index: 2,
    }
    .apply(&mut project)
    .unwrap();
    assert_eq!(project.tracks()[2].name(), "Locked");
}
