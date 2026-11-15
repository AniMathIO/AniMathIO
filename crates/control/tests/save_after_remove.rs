//! Regression tests for the headless save path after removing a clip and its
//! asset (issue found in #244/#250): orphan cached bytes must not fail the
//! save, and undoing `RemoveAsset` must still restore working asset bytes.

use animathio_control::{ControlHandler, HeadlessHandler};
use animathio_core::{
    Asset, AssetId, Clip, ClipId, ClipKind, Command, Project, TICKS_PER_SECOND, Time, TimeRange,
    Track, TrackId,
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

    // Undo the asset removal, save again: the cached bytes must come back.
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
