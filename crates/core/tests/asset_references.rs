//! Asset references must survive edits, including references only used by plugins.
use animathio_core::*;
use serde_json::json;

fn fixture(keyframed: bool) -> (Project, AssetId) {
    let mut project = Project::default();
    let id = AssetId::of(b"plugin asset");
    Command::AddAsset {
        id: id.clone(),
        asset: Asset {
            name: "texture.png".into(),
            media_type: "image/png".into(),
        },
    }
    .apply(&mut project)
    .unwrap();
    Command::AddTrack {
        track: Track::new(TrackId(1), "plugin"),
        index: 0,
    }
    .apply(&mut project)
    .unwrap();
    let mut clip = Clip::new(
        ClipId(1),
        ClipKind::Plugin {
            plugin_id: "org.example.texture".into(),
            params: json!({}),
        },
        TimeRange::new(Time(0), Time(100)).unwrap(),
    );
    clip.params.insert(
        "texture".into(),
        ParamValue::AssetRef(if keyframed { None } else { Some(id.clone()) }),
    );
    Command::AddClip {
        track: TrackId(1),
        clip,
    }
    .apply(&mut project)
    .unwrap();
    if keyframed {
        // A reference outside the visible range must still protect its bytes.
        Command::AddKeyframe {
            id: ClipId(1),
            target: Target::Param("texture".into()),
            time: Time(200),
            keyframe: Keyframe {
                value: ParamValue::AssetRef(Some(id.clone())),
                interpolation: Interpolation::Hold,
            },
        }
        .apply(&mut project)
        .unwrap();
    }
    (project, id)
}

#[test]
fn plugin_parameters_and_keyframes_block_asset_removal_transactionally() {
    for keyframed in [false, true] {
        let (mut project, id) = fixture(keyframed);
        let before = project.clone();
        for command in [
            Command::RemoveAsset { id: id.clone() },
            Command::Batch {
                commands: vec![
                    Command::SetTrackGain {
                        id: TrackId(1),
                        gain: 0.5,
                    },
                    Command::RemoveAsset { id: id.clone() },
                ],
            },
        ] {
            let error = command.apply(&mut project).unwrap_err().to_string();
            assert!(error.contains("missing asset"), "{error}");
            assert!(error.contains(id.as_str()), "{error}");
            assert_eq!(project, before);
        }
    }
}

#[test]
fn batches_validate_final_references_and_history_restores_exactly() {
    for keyframed in [false, true] {
        let (mut project, id) = fixture(keyframed);
        let before = project.clone();
        let mut history = History::default();
        history
            .execute(
                &mut project,
                &Command::Batch {
                    // Intermediate dangling references are allowed inside a batch.
                    commands: vec![
                        Command::RemoveAsset { id },
                        Command::RemoveClip { id: ClipId(1) },
                    ],
                },
            )
            .unwrap();
        let removed = project.clone();
        assert!(project.assets().is_empty());
        history.undo(&mut project).unwrap();
        assert_eq!(project, before);
        history.redo(&mut project).unwrap();
        assert_eq!(project, removed);
    }
}

#[test]
fn commands_cannot_introduce_missing_parameter_or_keyframe_references() {
    let (mut project, _) = fixture(false);
    let before = project.clone();
    let missing = ParamValue::AssetRef(Some(AssetId::of(b"missing")));
    for command in [
        Command::SetPluginParam {
            id: ClipId(1),
            name: "texture".into(),
            value: Some(missing.clone()),
        },
        Command::AddKeyframe {
            id: ClipId(1),
            target: Target::Param("texture".into()),
            time: Time(0),
            keyframe: Keyframe {
                value: missing,
                interpolation: Interpolation::Hold,
            },
        },
    ] {
        assert!(command.apply(&mut project).is_err());
        assert_eq!(project, before);
    }
}
