//! Standalone fit editing preserves the input and requires explicit overwrite.
use animathio_core::*;
use std::{collections::BTreeMap, process::Command as Process};
#[test]
fn edits_fit_aliases_and_refuses_unrequested_overwrites() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.amio");
    let output = dir.path().join("output.amio");
    let mut project = Project::default();
    let id = AssetId::of(b"encoded asset");
    Command::Batch {
        commands: vec![
            Command::AddAsset {
                id: id.clone(),
                asset: Asset {
                    name: "image.png".into(),
                    media_type: "image/png".into(),
                },
            },
            Command::AddTrack {
                track: Track::new(TrackId(1), "media"),
                index: 0,
            },
            Command::AddClip {
                track: TrackId(1),
                clip: Clip::new(
                    ClipId(1),
                    ClipKind::Image(id.clone()),
                    TimeRange::new(Time(0), Time(100)).unwrap(),
                ),
            },
        ],
    }
    .apply(&mut project)
    .unwrap();
    animathio_format::save_amio(
        &input,
        &project,
        &BTreeMap::from([(id, b"encoded asset".to_vec())]),
    )
    .unwrap();
    let original = std::fs::read(&input).unwrap();
    for (mode, fit) in [
        ("fit", Fit::Contain),
        ("fill", Fit::Cover),
        ("stretch", Fit::Stretch),
        ("none", Fit::Original),
    ] {
        let status = Process::new(env!("CARGO_BIN_EXE_animathio-cli"))
            .arg("media-fit")
            .arg(&input)
            .args(["--clip", "1", "--mode", mode, "--output"])
            .arg(&output)
            .arg("--overwrite")
            .status()
            .unwrap();
        assert!(status.success());
        let bundle = animathio_format::read_amio(
            std::fs::File::open(&output).unwrap(),
            animathio_format::Limits::default(),
        )
        .unwrap();
        assert_eq!(bundle.project.tracks()[0].clips()[&ClipId(1)].fit, fit);
        assert_eq!(std::fs::read(&input).unwrap(), original);
    }
    let before = std::fs::read(&output).unwrap();
    let result = Process::new(env!("CARGO_BIN_EXE_animathio-cli"))
        .arg("media-fit")
        .arg(&input)
        .args(["--clip", "1", "--mode", "fit", "--output"])
        .arg(&output)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(std::fs::read(&output).unwrap(), before);
}
