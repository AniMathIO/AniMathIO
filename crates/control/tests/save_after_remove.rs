//! Regression tests for the headless save path after removing a clip and its
//! asset (issue found in #244/#250): orphan cached bytes must not fail the
//! save, and undoing `RemoveAsset` must still restore working asset bytes.

use animathio_control::{ControlHandler, HeadlessHandler};
use animathio_core::{
    Asset, AssetId, Clip, ClipId, ClipKind, Command, Interpolation, Keyframe, ParamValue, Project,
    TICKS_PER_SECOND, Target, Time, TimeRange, Track, TrackId,
};
use animathio_format::{Bundle, Limits, read_amio};
use std::collections::BTreeMap;

fn handler_with_media() -> HeadlessHandler {
    let mut project = Project::default();
    let id = AssetId::of(b"video bytes");
    Command::AddAsset {
        id: id.clone(),
        asset: Asset {
            name: "input.mp4".into(),
            media_type: "video/mp4".into(),
        },
    }
    .apply(&mut project)
    .unwrap();
    Command::AddTrack {
        track: Track::new(TrackId(1), "video"),
        index: 0,
    }
    .apply(&mut project)
    .unwrap();
    Command::AddClip {
        track: TrackId(1),
        clip: Clip::new(
            ClipId(1),
            ClipKind::Video(id.clone()),
            TimeRange::new(Time(0), Time(TICKS_PER_SECOND)).unwrap(),
        ),
    }
    .apply(&mut project)
    .unwrap();
    HeadlessHandler::from_bundle(
        Bundle {
            project,
            assets: BTreeMap::from([(id, b"video bytes".to_vec())]),
        },
        None,
    )
}

fn load(path: &std::path::Path) -> Bundle {
    read_amio(std::fs::File::open(path).unwrap(), Limits::default()).unwrap()
}

#[test]
fn remove_clip_and_asset_then_save_reopens_and_undo_restores_bytes() {
    let mut handler = handler_with_media();
    let dir = tempfile::tempdir().unwrap();
    let saved = dir.path().join("after-remove.amio");
    let id = AssetId::of(b"video bytes");

    // One batch: remove the clip, then its now-unused asset.
    handler
        .command_apply(Command::Batch {
            commands: vec![
                Command::RemoveClip { id: ClipId(1) },
                Command::RemoveAsset { id: id.clone() },
            ],
        })
        .unwrap();
    handler.project_save(Some(saved.clone())).unwrap();
    let bundle = load(&saved);
    assert!(
        bundle.project.assets().is_empty(),
        "the removed asset must not survive the save"
    );
    assert!(bundle.assets.is_empty());

    // Undo the batch, save again: the cached bytes and clip must come back.
    assert!(handler.history_undo().unwrap());
    let restored = dir.path().join("after-undo.amio");
    handler.project_save(Some(restored.clone())).unwrap();
    let bundle = load(&restored);
    assert_eq!(
        bundle.assets.get(&id).map(Vec::as_slice),
        Some(b"video bytes".as_slice())
    );
    assert_eq!(
        bundle.project.assets().get(&id).map(|a| a.name.as_str()),
        Some("input.mp4")
    );
}

#[test]
fn unused_but_listed_asset_is_kept_on_headless_save() {
    let mut handler = handler_with_media();
    let id = AssetId::of(b"video bytes");
    // Remove only the clip: the asset stays in the manifest, unused.
    handler
        .command_apply(Command::RemoveClip { id: ClipId(1) })
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let saved = dir.path().join("unused-asset.amio");
    handler.project_save(Some(saved.clone())).unwrap();
    let bundle = load(&saved);
    assert!(bundle.project.assets().contains_key(&id));
    assert_eq!(
        bundle.assets.get(&id).map(Vec::as_slice),
        Some(b"video bytes".as_slice())
    );
}

fn handler_with_plugin_asset(keyframed: bool) -> HeadlessHandler {
    let mut handler = handler_with_media();
    handler
        .command_apply(Command::RemoveClip { id: ClipId(1) })
        .unwrap();
    let mut clip = Clip::new(
        ClipId(1),
        ClipKind::Plugin {
            plugin_id: "org.example.texture".into(),
            params: serde_json::json!({}),
        },
        TimeRange::new(Time(0), Time(TICKS_PER_SECOND)).unwrap(),
    );
    let id = AssetId::of(b"video bytes");
    clip.params.insert(
        "texture".into(),
        ParamValue::AssetRef(if keyframed { None } else { Some(id.clone()) }),
    );
    handler
        .command_apply(Command::AddClip {
            track: TrackId(1),
            clip,
        })
        .unwrap();
    if keyframed {
        handler
            .command_apply(Command::AddKeyframe {
                id: ClipId(1),
                target: Target::Param("texture".into()),
                time: Time(0),
                keyframe: Keyframe {
                    value: ParamValue::AssetRef(Some(id)),
                    interpolation: Interpolation::Hold,
                },
            })
            .unwrap();
    }
    handler
}

#[test]
fn plugin_asset_references_survive_archive_and_json_save_remove_undo_redo() {
    for extension in ["amio", "json"] {
        for keyframed in [false, true] {
            let mut handler = handler_with_plugin_asset(keyframed);
            let before = handler.project().clone();
            let id = AssetId::of(b"video bytes");
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(format!("project.{extension}"));
            assert!(
                handler
                    .command_apply(Command::RemoveAsset { id: id.clone() })
                    .is_err()
            );
            assert_eq!(handler.project(), &before);
            handler.project_save(Some(path.clone())).unwrap();
            assert_eq!(HeadlessHandler::load(&path).unwrap().project(), &before);

            // Use a fresh JSON directory so an old sidecar cannot hide dropped bytes.
            let removed_dir = dir.path().join("removed");
            std::fs::create_dir(&removed_dir).unwrap();
            let removed_path = removed_dir.join(format!("project.{extension}"));
            handler
                .command_apply(Command::Batch {
                    commands: vec![
                        Command::RemoveClip { id: ClipId(1) },
                        Command::RemoveAsset { id: id.clone() },
                    ],
                })
                .unwrap();
            handler.project_save(Some(removed_path.clone())).unwrap();
            let reopened = HeadlessHandler::load(&removed_path).unwrap();
            assert!(reopened.project().assets().is_empty());
            if extension == "json" {
                assert!(!removed_dir.join("assets").join(id.as_str()).exists());
            } else {
                assert!(load(&removed_path).assets.is_empty());
            }

            assert!(handler.history_undo().unwrap());
            assert_eq!(handler.project(), &before);
            handler.project_save(Some(removed_path.clone())).unwrap();
            assert_eq!(
                HeadlessHandler::load(&removed_path).unwrap().project(),
                &before
            );
            let bytes = if extension == "json" {
                std::fs::read(removed_dir.join("assets").join(id.as_str())).unwrap()
            } else {
                load(&removed_path).assets[&id].clone()
            };
            assert_eq!(bytes, b"video bytes");
            assert!(handler.history_redo().unwrap());
            handler.project_save(Some(removed_path.clone())).unwrap();
            assert!(
                HeadlessHandler::load(&removed_path)
                    .unwrap()
                    .project()
                    .assets()
                    .is_empty()
            );
        }
    }
}
