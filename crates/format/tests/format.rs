//! Persistence contracts and malformed-input regression tests.
use animathio_core::*;
use animathio_format::*;
use proptest::prelude::*;
use serde_json::json;
use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

fn fixture(bytes: &[u8]) -> (Project, BTreeMap<AssetId, Vec<u8>>) {
    let mut project = Project::default();
    let id = AssetId::of(bytes);
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
            TimeRange::new(Time(0), Time(100)).unwrap(),
        ),
    }
    .apply(&mut project)
    .unwrap();
    Command::AddTrack {
        track: Track::new(TrackId(2), "unavailable plugin"),
        index: 1,
    }
    .apply(&mut project)
    .unwrap();
    Command::AddClip {
        track: TrackId(2),
        clip: Clip::new(
            ClipId(2),
            ClipKind::Plugin {
                plugin_id: "org.example.missing@9".into(),
                params: json!({"schema": 9, "latex": "x^2", "opaque": [false, {"future": 42}]}),
            },
            TimeRange::new(Time(0), Time(100)).unwrap(),
        ),
    }
    .apply(&mut project)
    .unwrap();
    (project, BTreeMap::from([(id, bytes.to_vec())]))
}
fn json_bytes(project: &Project) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_json(&mut bytes, project).unwrap();
    bytes
}
fn archive(entries: &[(&str, &[u8])]) -> Cursor<Vec<u8>> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        writer
            .start_file(*name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    let mut output = writer.finish().unwrap();
    output.set_position(0);
    output
}

#[test]
fn json_has_schema_and_preserves_missing_plugin_data() {
    let (project, _) = fixture(b"video bytes");
    let bytes = json_bytes(&project);
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["schemaVersion"], SCHEMA_VERSION);
    assert_eq!(value["format"], "animathio");
    assert_eq!(read_json(bytes.as_slice()).unwrap(), project);
}

#[test]
fn archive_has_exact_content_addressed_entries_and_roundtrips() {
    let (project, assets) = fixture(b"video bytes");
    let mut cursor = write_amio(Cursor::new(Vec::new()), &project, &assets).unwrap();
    cursor.set_position(0);
    let zip = ZipArchive::new(cursor.clone()).unwrap();
    assert_eq!(zip.len(), 2);
    assert_eq!(
        zip.file_names().collect::<Vec<_>>(),
        vec![
            "project.json",
            &format!("assets/{}", AssetId::of(b"video bytes").as_str())
        ]
    );
    let bundle = read_amio(cursor, Limits::default()).unwrap();
    assert_eq!(bundle.project, project);
    assert_eq!(bundle.assets, assets);
}

#[test]
fn empty_project_roundtrips_both_formats() {
    let project = Project::default();
    assert_eq!(read_json(json_bytes(&project).as_slice()).unwrap(), project);
    let mut cursor = write_amio(Cursor::new(Vec::new()), &project, &BTreeMap::new()).unwrap();
    cursor.set_position(0);
    assert_eq!(
        read_amio(cursor, Limits::default()).unwrap().project,
        project
    );
}

#[test]
fn rejects_missing_invalid_future_schema_and_wrong_format() {
    for value in [
        json!({}),
        json!({"schemaVersion": "1"}),
        json!({"schemaVersion": -1}),
        json!({"schemaVersion": 2}),
        json!({"schemaVersion": 4_294_967_296_u64}),
    ] {
        assert!(read_json(serde_json::to_vec(&value).unwrap().as_slice()).is_err());
    }
    let mut value: serde_json::Value =
        serde_json::from_slice(&json_bytes(&Project::default())).unwrap();
    value["format"] = json!("other");
    assert!(read_json(serde_json::to_vec(&value).unwrap().as_slice()).is_err());
    assert!(read_json(b"not json".as_slice()).is_err());
}

#[test]
fn synthetic_legacy_fixture_exercises_forward_migration_hook() {
    // v0 is synthetic test data, not a claim of compatibility with AniMathIO v1.
    let legacy = include_bytes!("fixtures/v0.json");
    assert!(read_json(legacy.as_slice()).is_err());
    let mut calls = 0;
    let project = read_json_with_migration(legacy.as_slice(), |version, mut value| {
        assert_eq!(version, 0);
        calls += 1;
        let fps = value.as_object_mut().unwrap().remove("fps").unwrap();
        value["frame_rate"] = json!({"numerator": fps, "denominator": 1});
        value["schemaVersion"] = json!(1);
        Ok(value)
    })
    .unwrap();
    assert_eq!(calls, 1);
    assert_eq!(project, Project::default());
    assert!(read_json_with_migration(legacy.as_slice(), |_, value| Ok(value)).is_err());
    assert!(
        read_json_with_migration(legacy.as_slice(), |_, mut value| {
            value["schemaVersion"] = json!(2);
            Ok(value)
        })
        .is_err()
    );
}

#[test]
fn rejects_missing_extra_and_corrupt_asset_bytes_on_write() {
    let (project, assets) = fixture(b"original");
    assert!(write_amio(Cursor::new(Vec::new()), &project, &BTreeMap::new()).is_err());
    let mut corrupt = assets.clone();
    *corrupt.values_mut().next().unwrap() = b"corrupt".to_vec();
    assert!(write_amio(Cursor::new(Vec::new()), &project, &corrupt).is_err());
    let mut extra = assets;
    extra.insert(AssetId::of(b"extra"), b"extra".to_vec());
    assert!(write_amio(Cursor::new(Vec::new()), &project, &extra).is_err());
}

#[test]
fn rejects_malformed_archives_without_extracting_paths() {
    let (project, _) = fixture(b"original");
    let json = json_bytes(&project);
    let path = format!("assets/{}", AssetId::of(b"original").as_str());
    for entries in [
        vec![("unknown", b"x".as_slice())],
        vec![("../project.json", json.as_slice())],
        vec![("project.json", json.as_slice())],
        vec![
            ("project.json", json.as_slice()),
            (path.as_str(), b"corrupt".as_slice()),
        ],
        vec![
            ("project.json", json.as_slice()),
            ("assets/../file", b"x".as_slice()),
        ],
        vec![(path.as_str(), b"original".as_slice())],
    ] {
        assert!(read_amio(archive(&entries), Limits::default()).is_err());
    }
    assert!(read_amio(Cursor::new(b"not zip"), Limits::default()).is_err());
}

#[test]
fn duplicate_archive_names_use_the_zip_library_canonical_entry() {
    let json = json_bytes(&Project::default());
    let original = archive(&[("project.json", &json), ("project.pson", &json)]).into_inner();
    let mut bytes = original;
    // Forge duplicate entry names in both headers; entry contents and CRC are unchanged.
    for start in 0..bytes.len() - 12 {
        if &bytes[start..start + 12] == b"project.pson" {
            bytes[start..start + 12].copy_from_slice(b"project.json");
        }
    }
    assert_eq!(
        read_amio(Cursor::new(bytes), Limits::default())
            .unwrap()
            .project,
        Project::default()
    );
}

#[test]
fn configurable_limits_reject_oversized_archives() {
    let (project, assets) = fixture(b"original");
    let cursor = write_amio(Cursor::new(Vec::new()), &project, &assets).unwrap();
    let bytes = cursor.into_inner();
    for limits in [
        Limits {
            project_bytes: 1,
            ..Limits::default()
        },
        Limits {
            asset_bytes: 1,
            ..Limits::default()
        },
        Limits {
            total_bytes: 1,
            ..Limits::default()
        },
        Limits {
            entries: 1,
            ..Limits::default()
        },
    ] {
        assert!(read_amio(Cursor::new(&bytes), limits).is_err());
    }
}

proptest! {
    #[test]
    fn json_and_container_roundtrip_with_arbitrary_content(bytes in prop::collection::vec(any::<u8>(), 0..2048), gain in -1e12_f64..1e12_f64) {
        let (mut project, assets) = fixture(&bytes);
        Command::SetParam { id: ClipId(1), name: "gain".into(), value: Some(gain) }.apply(&mut project).unwrap();
        prop_assert_eq!(read_json(json_bytes(&project).as_slice()).unwrap(), project.clone());
        let mut cursor = write_amio(Cursor::new(Vec::new()), &project, &assets).unwrap();
        cursor.set_position(0);
        let bundle = read_amio(cursor, Limits::default()).unwrap();
        prop_assert_eq!(bundle.project, project);
        prop_assert_eq!(bundle.assets, assets);
    }
}

#[test]
fn legacy_numeric_parameters_and_keyframes_load_as_typed_numbers() {
    let project =
        read_json(include_bytes!("fixtures/legacy-numeric-keyframes.json").as_slice()).unwrap();
    let clip = &project.tracks()[0].clips()[&ClipId(1)];
    assert_eq!(clip.params["gain"], ParamValue::Number(0.25));
    let scene = evaluate(&project, Time(40));
    assert_eq!(scene.clips[0].params["gain"], ParamValue::Number(0.5));
    assert!((scene.clips[0].transform.x - 0.5).abs() < f64::EPSILON);
    assert_eq!(read_json(json_bytes(&project).as_slice()).unwrap(), project);
    let ClipKind::Plugin { params, .. } = &clip.kind else {
        panic!("plugin preserved")
    };
    assert_eq!(params["unknown"], json!(["preserve", true]));
}

#[test]
fn schema_one_builtins_migrate_through_json_and_amio() {
    let fixture = include_bytes!("fixtures/v1-builtins.json");
    let project = read_json(fixture.as_slice()).unwrap();
    let archived = read_amio(archive(&[("project.json", fixture)]), Limits::default()).unwrap();
    assert_eq!(archived.project, project);
    let original: serde_json::Value = serde_json::from_slice(fixture).unwrap();
    let mut current = Vec::new();
    write_json(&mut current, &project).unwrap();
    let current: serde_json::Value = serde_json::from_slice(&current).unwrap();
    assert_eq!(current["schemaVersion"], SCHEMA_VERSION);
    for (old, new) in original["tracks"]
        .as_array()
        .unwrap()
        .iter()
        .zip(current["tracks"].as_array().unwrap())
    {
        for (id, old) in old["clips"].as_object().unwrap() {
            let new = &new["clips"][id];
            for key in ["id", "range", "transform", "source_offset"] {
                assert_eq!(old[key], new[key], "{key}");
            }
            // Schema 1 permits bare numbers; current serialization tags ParamValue.
            assert_eq!(
                serde_json::from_value::<Vec<Animation>>(old["animations"].clone()).unwrap(),
                serde_json::from_value::<Vec<Animation>>(new["animations"].clone()).unwrap()
            );
            assert_eq!(
                serde_json::from_value::<BTreeMap<String, ParamValue>>(old["params"].clone())
                    .unwrap(),
                serde_json::from_value::<BTreeMap<String, ParamValue>>(new["params"].clone())
                    .unwrap()
            );
            let params = &new["kind"]["Plugin"]["params"];
            assert_eq!(params["units"], "normalized");
            if let Some(text) = old["kind"]["Builtin"].get("Text") {
                assert_eq!(new["kind"]["Plugin"]["plugin_id"], "org.animathio.text");
                assert_eq!(params["text"], text["text"]);
                assert_eq!(params["font_size"], text["font_size"]);
                assert_eq!(params["color"], text["color"]);
            } else {
                assert_eq!(new["kind"]["Plugin"]["plugin_id"], "org.animathio.shapes");
            }
        }
    }
    assert!(!serde_json::to_string(&current).unwrap().contains("Builtin"));
}

#[test]
fn schema_one_migration_rejects_invalid_legacy_sources() {
    let fixture: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/v1-builtins.json")).unwrap();
    for source in [
        json!({"Text": {"text": "bad", "font_size": 0, "color": [255,255,255,255]}}),
        json!({"Text": {"text": "bad", "font_size": 17, "color": [255,255,255,255]}}),
        json!({"Shape": {"shape": {"Path": [{"LineTo": [0,0]}]}, "fill": null, "stroke": null}}),
        json!({"Shape": {"shape": "Rect", "fill": null, "stroke": {"color": [255,255,255,255], "width": -1}}}),
    ] {
        let mut bad = fixture.clone();
        let clip = bad["tracks"][0]["clips"]
            .as_object_mut()
            .unwrap()
            .values_mut()
            .next()
            .unwrap();
        clip["kind"] = json!({"Builtin": source});
        assert!(read_json(serde_json::to_vec(&bad).unwrap().as_slice()).is_err());
    }
}

#[test]
fn schema_two_builtin_effects_migrate_to_the_effects_plugin() {
    let fixture = include_bytes!("fixtures/v2-effects.json");
    let project = read_json(fixture.as_slice()).unwrap();
    let archived = read_amio(archive(&[("project.json", fixture)]), Limits::default()).unwrap();
    assert_eq!(archived.project, project);

    let clip = &project.tracks()[0].clips()[&ClipId(1)];
    let migrated: Vec<_> = clip
        .effects
        .iter()
        .map(|effect| (effect.plugin.as_str(), effect.effect.as_str()))
        .collect();
    assert_eq!(
        migrated,
        [
            "grayscale",
            "sepia",
            "invert",
            "saturate",
            "brightness",
            "contrast",
            "blur",
            "opacity"
        ]
        .map(|id| ("org.animathio.effects-basic", id))
    );
    // Parameters and ids carry over unchanged, including the keyframed target.
    assert_eq!(clip.effects[3].params["amount"], 2.0);
    assert_eq!(clip.effects[6].params["radius"], 0.02);
    assert_eq!(clip.animations.len(), 1);
    assert_eq!(
        clip.animations[0].target,
        Target::Effect {
            effect: EffectId(1),
            param: "amount".into()
        }
    );

    // Rewriting produces the current schema, which reads back identically.
    let mut current = Vec::new();
    write_json(&mut current, &project).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&current).unwrap();
    assert_eq!(value["schemaVersion"], SCHEMA_VERSION);
    assert!(
        !String::from_utf8(current.clone())
            .unwrap()
            .contains("\"kind\": \"Grayscale\"")
    );
    assert_eq!(read_json(current.as_slice()).unwrap(), project);
}

#[test]
fn schema_two_migration_rejects_unknown_builtin_effects() {
    let mut value: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/v2-effects.json")).unwrap();
    value["tracks"][0]["clips"]["1"]["effects"][0]["kind"] = json!("Vignette");
    assert!(read_json(serde_json::to_vec(&value).unwrap().as_slice()).is_err());
}
