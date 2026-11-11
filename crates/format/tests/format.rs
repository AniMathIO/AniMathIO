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
        calls += 1;
        match version {
            0 => {
                let fps = value.as_object_mut().unwrap().remove("fps").unwrap();
                value["frame_rate"] = json!({"numerator": fps, "denominator": 1});
                value["schemaVersion"] = json!(1);
            }
            1 => {
                let fps = value.as_object_mut().unwrap().remove("frame_rate").unwrap();
                value["settings"] = json!({
                    "width": 1920,
                    "height": 1080,
                    "frame_rate": fps,
                    "background": [0, 0, 0, 255],
                });
                value["schemaVersion"] = json!(2);
            }
            _ => panic!("unexpected schema version {version}"),
        }
        Ok(value)
    })
    .unwrap();
    assert_eq!(calls, 2);
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
fn v1_projects_migrate_to_v2_with_default_settings() {
    // Build a current project and rewrite its JSON into the v1 shape: a top-level
    // `frame_rate` and no `settings`. The built-in migration must move the frame
    // rate into settings and fill the new fields with the documented defaults.
    let mut project = Project::default();
    Command::SetProjectSettings {
        settings: ProjectSettings::new(320, 180, FrameRate::new(60, 1).unwrap(), [1, 2, 3, 255])
            .unwrap(),
    }
    .apply(&mut project)
    .unwrap();
    let mut value = serde_json::to_value(&project).unwrap();
    let settings = value.as_object_mut().unwrap().remove("settings").unwrap();
    value["format"] = json!("animathio");
    value["frame_rate"] = settings["frame_rate"].clone();
    value["schemaVersion"] = json!(1);
    let migrated = read_json(serde_json::to_vec(&value).unwrap().as_slice()).unwrap();
    assert_eq!(
        migrated.settings().frame_rate,
        FrameRate::new(60, 1).unwrap()
    );
    assert_eq!(migrated.settings().width, 1920);
    assert_eq!(migrated.settings().height, 1080);
    assert_eq!(migrated.settings().background, [0, 0, 0, 255]);
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
