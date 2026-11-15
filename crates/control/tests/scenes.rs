//! Real media detection, review, linked editing and transactional undo.
use animathio_control::{SceneParams, detect_scenes, scene_command};
use animathio_core::{
    Asset, AssetId, Clip, ClipId, ClipKind, Command, History, Project, TICKS_PER_SECOND, Time,
    TimeRange, Track, TrackId,
};
use std::{collections::BTreeMap, sync::atomic::AtomicBool};
#[test]
#[allow(clippy::too_many_lines)]
fn hard_cut_fade_linked_audio_and_exact_undo() {
    let bytes = include_bytes!("scenes.mp4").to_vec();
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
    for (i, kind) in [
        (1, ClipKind::Video(asset.clone())),
        (2, ClipKind::Audio(asset.clone())),
    ] {
        Command::AddTrack {
            track: Track::new(TrackId(i), "Media"),
            index: usize::try_from(i - 1).unwrap(),
        }
        .apply(&mut project)
        .unwrap();
        let mut clip = Clip::new(
            ClipId(i),
            kind,
            TimeRange::new(Time(0), Time(4 * TICKS_PER_SECOND)).unwrap(),
        );
        clip.link_group = Some(ClipId(1));
        Command::AddClip {
            track: TrackId(i),
            clip,
        }
        .apply(&mut project)
        .unwrap();
    }
    let original = project.clone();
    let draft = detect_scenes(
        &project,
        &BTreeMap::from([(asset.clone(), bytes.clone())]),
        &SceneParams {
            clip: Some(ClipId(1)),
            ..SceneParams::default()
        },
        &AtomicBool::new(false),
        &|_| {},
    )
    .unwrap();
    assert_eq!(
        serde_json::from_value::<animathio_control::SceneDraft>(
            serde_json::to_value(&draft).unwrap()
        )
        .unwrap(),
        draft
    );
    assert_eq!(draft.cuts.len(), 2);
    assert_eq!(draft.cuts[1].time, Time(3 * TICKS_PER_SECOND));
    assert_eq!(draft.cuts[0].time, Time(TICKS_PER_SECOND));
    let mut locked = original.clone();
    Command::SetTrackLocked {
        id: TrackId(2),
        locked: true,
    }
    .apply(&mut locked)
    .unwrap();
    let locked_draft = detect_scenes(
        &locked,
        &BTreeMap::from([(asset.clone(), bytes.clone())]),
        &SceneParams {
            clip: Some(ClipId(1)),
            ..SceneParams::default()
        },
        &AtomicBool::new(false),
        &|_| {},
    )
    .unwrap();
    assert!(scene_command(&locked, &locked_draft, false).is_err());
    let command = scene_command(&project, &draft, false).unwrap();
    let mut history = History::default();
    history.execute(&mut project, &command).unwrap();
    assert!(project.tracks().iter().all(|t| t.clips().len() == 3));
    assert!(scene_command(&project, &draft, false).is_err());
    history.undo(&mut project).unwrap();
    assert_eq!(project, original);
    let delete_draft = detect_scenes(
        &project,
        &BTreeMap::from([(asset, bytes)]),
        &SceneParams {
            clip: Some(ClipId(1)),
            delete_shorter_than_frames: 31,
            ..SceneParams::default()
        },
        &AtomicBool::new(false),
        &|_| {},
    )
    .unwrap();
    history
        .execute(
            &mut project,
            &scene_command(&original, &delete_draft, false).unwrap(),
        )
        .unwrap();
    assert!(project.tracks().iter().all(|t| t.clips().len() == 1
        && t.clips().values().next().unwrap().range.start == Time(TICKS_PER_SECOND)));
    history.undo(&mut project).unwrap();
    assert_eq!(project, original);
    let mut disabled = draft.clone();
    for cut in &mut disabled.cuts {
        cut.enabled = false;
    }
    scene_command(&project, &disabled, false)
        .unwrap()
        .apply(&mut project)
        .unwrap();
    assert_eq!(project, original);
    history
        .execute(
            &mut project,
            &scene_command(&original, &draft, true).unwrap(),
        )
        .unwrap();
    assert_eq!(project.markers().len(), 2);
    history.undo(&mut project).unwrap();
    assert_eq!(project, original);
}
