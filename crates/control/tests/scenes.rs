use animathio_control::{SceneParams, detect_scenes, scene_command};
use animathio_core::{
    Asset, AssetId, Clip, ClipId, ClipKind, Command, History, Project, TICKS_PER_SECOND, Time,
    TimeRange, Track, TrackId,
};
use std::{collections::BTreeMap, sync::atomic::AtomicBool};
#[test]
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
            index: (i - 1) as usize,
        }
        .apply(&mut project)
        .unwrap();
        let mut clip = Clip::new(
            ClipId(i),
            kind,
            TimeRange::new(Time(0), Time(3 * TICKS_PER_SECOND)).unwrap(),
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
        &BTreeMap::from([(asset, bytes)]),
        &SceneParams {
            clip: Some(ClipId(1)),
            ..SceneParams::default()
        },
        &AtomicBool::new(false),
        &|_| {},
    )
    .unwrap();
    assert_eq!(draft.cuts.len(), 1);
    assert_eq!(draft.cuts[0].time, Time(TICKS_PER_SECOND));
    let command = scene_command(&project, &draft, false).unwrap();
    let mut history = History::default();
    history.execute(&mut project, &command).unwrap();
    assert!(project.tracks().iter().all(|t| t.clips().len() == 2));
    assert!(scene_command(&project, &draft, false).is_err());
    history.undo(&mut project).unwrap();
    assert_eq!(project, original);
    let mut disabled = draft.clone();
    disabled.cuts[0].enabled = false;
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
    assert_eq!(project.markers().len(), 1);
    history.undo(&mut project).unwrap();
    assert_eq!(project, original);
}
