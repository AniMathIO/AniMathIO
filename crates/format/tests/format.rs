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
        zip.file_names()
            .map(|name| name.unwrap().into_owned())
            .collect::<Vec<_>>(),
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

#[test]
fn schema_four_projects_load_with_no_markers_and_markers_round_trip() {
    let mut project = Project::default();
    let mut old: serde_json::Value = serde_json::from_slice(&json_bytes(&project)).unwrap();
    assert!(old.get("markers").is_none(), "no markers, no key");
    old["schemaVersion"] = json!(4);
    let loaded = read_json(serde_json::to_vec(&old).unwrap().as_slice()).unwrap();
    assert_eq!(loaded.markers(), []);
    assert_eq!(loaded, project);

    let mut marker = Marker::new(MarkerId(3), Time(705_600_000));
    marker.name = "Drop".into();
    marker.color = MarkerColor::Red;
    marker.note = "chorus".into();
    marker.duration = Some(Time(1_000));
    Command::AddMarker {
        marker: marker.clone(),
    }
    .apply(&mut project)
    .unwrap();
    Command::AddMarker {
        marker: Marker::new(MarkerId(1), Time(5)),
    }
    .apply(&mut project)
    .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&json_bytes(&project)).unwrap();
    assert_eq!(value["schemaVersion"], SCHEMA_VERSION);
    assert_eq!(value["markers"].as_array().unwrap().len(), 2);
    assert_eq!(read_json(json_bytes(&project).as_slice()).unwrap(), project);
    let mut cursor = write_amio(Cursor::new(Vec::new()), &project, &BTreeMap::new()).unwrap();
    cursor.set_position(0);
    let back = read_amio(cursor, Limits::default()).unwrap().project;
    assert_eq!(back.marker(MarkerId(3)), Some(&marker));
}

#[test]
fn schema_five_media_clips_keep_stretching_after_the_fit_migration() {
    let (mut project, assets) = fixture(b"video bytes");
    // Pre-schema-6 files have no `fit` key; they must keep their stretched look.
    let mut old: serde_json::Value = serde_json::from_slice(&json_bytes(&project)).unwrap();
    old["schemaVersion"] = json!(5);
    for track in old["tracks"].as_array_mut().unwrap() {
        for clip in track["clips"].as_object_mut().unwrap().values_mut() {
            clip.as_object_mut().unwrap().remove("fit");
        }
    }
    let loaded = read_json(serde_json::to_vec(&old).unwrap().as_slice()).unwrap();
    let clip = |p: &Project, id| p.tracks()[0].clips()[&ClipId(id)].clone();
    assert_eq!(clip(&loaded, 1).fit, Fit::Stretch);

    // New media clips default to contain, which is written out and read back.
    assert_eq!(clip(&project, 1).fit, Fit::Contain);
    let value: serde_json::Value = serde_json::from_slice(&json_bytes(&project)).unwrap();
    assert_eq!(value["schemaVersion"], SCHEMA_VERSION);
    assert_eq!(value["tracks"][0]["clips"]["1"]["fit"], "contain");
    // Explicit stretch survives the next schema-6 save/load, whose new default is contain.
    assert_eq!(value["tracks"][1]["clips"]["2"]["fit"], "stretch");
    assert_eq!(read_json(json_bytes(&loaded).as_slice()).unwrap(), loaded);
    Command::SetClipFit {
        id: ClipId(1),
        fit: Fit::Cover,
    }
    .apply(&mut project)
    .unwrap();
    let mut cursor = write_amio(Cursor::new(Vec::new()), &project, &assets).unwrap();
    cursor.set_position(0);
    let back = read_amio(cursor, Limits::default()).unwrap().project;
    assert_eq!(clip(&back, 1).fit, Fit::Cover);
    assert_eq!(back, project);
}

#[test]
fn fit_accepts_its_aliases() {
    for (name, fit) in [
        ("fit", Fit::Contain),
        ("fill", Fit::Cover),
        ("none", Fit::Original),
        ("stretch", Fit::Stretch),
    ] {
        assert_eq!(serde_json::from_value::<Fit>(json!(name)).unwrap(), fit);
        assert_eq!(Fit::parse(name), Some(fit));
        assert_eq!(Fit::parse(fit.name()), Some(fit));
    }
}

#[test]
fn schema_five_template_fit_is_promoted_without_changing_placement() {
    for mode in ["contain", "cover"] {
        let (project, _) = fixture(b"video bytes");
        let mut value: serde_json::Value = serde_json::from_slice(&json_bytes(&project)).unwrap();
        value["schemaVersion"] = json!(5);
        let clip = &mut value["tracks"][0]["clips"]["1"];
        clip.as_object_mut().unwrap().remove("fit");
        clip["params"]["media_fit"] = json!({"String":mode});
        let loaded = read_json(serde_json::to_vec(&value).unwrap().as_slice()).unwrap();
        let clip = &loaded.tracks()[0].clips()[&ClipId(1)];
        assert_eq!(clip.fit, Fit::parse(mode).unwrap());
        assert!(!clip.params.contains_key("media_fit"));
        assert_eq!(
            clip.transform,
            project.tracks()[0].clips()[&ClipId(1)].transform
        );
        let saved = json_bytes(&loaded);
        assert!(!String::from_utf8_lossy(&saved).contains("media_fit"));
        assert_eq!(read_json(saved.as_slice()).unwrap(), loaded);
    }
}

#[test]
fn schema_six_migrates_to_identity_speed_and_current_schema_roundtrips() {
    use animathio_core::{ClipSpeed, ClipTimeMap, SpeedPoint};
    let mut project = Project::default();
    Command::AddTrack {
        track: Track::new(TrackId(1), "test"),
        index: 0,
    }
    .apply(&mut project)
    .unwrap();
    Command::AddClip {
        track: TrackId(1),
        clip: Clip::new(
            ClipId(1),
            ClipKind::Error("source".into()),
            TimeRange::new(Time(0), Time(1000)).unwrap(),
        ),
    }
    .apply(&mut project)
    .unwrap();
    let mut bytes = Vec::new();
    write_json(&mut bytes, &project).unwrap();
    let mut json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    json["schemaVersion"] = 6.into();
    json["tracks"][0]["clips"]["1"]
        .as_object_mut()
        .unwrap()
        .remove("time_map");
    // A schema-6 file predates pitch preservation too: it must load with it off.
    json["tracks"][0]["clips"]["1"]
        .as_object_mut()
        .unwrap()
        .remove("maintain_pitch");
    let migrated = read_json(serde_json::to_vec(&json).unwrap().as_slice()).unwrap();
    assert!(!migrated.tracks()[0].clips()[&ClipId(1)].maintain_pitch);
    let mut legacy = project.clone();
    Command::SetClipMaintainPitch {
        id: ClipId(1),
        maintain_pitch: false,
    }
    .apply(&mut legacy)
    .unwrap();
    assert_eq!(migrated, legacy);
    assert_eq!(
        migrated.tracks()[0].clips()[&ClipId(1)].time_map,
        ClipTimeMap::default()
    );
    Command::SetClipSpeed {
        id: ClipId(1),
        speed: ClipSpeed::new(2, 1).unwrap(),
        reverse: true,
        ramps: vec![
            SpeedPoint {
                time: Time(0),
                speed: ClipSpeed::new(1, 2).unwrap(),
            },
            SpeedPoint {
                time: Time(1000),
                speed: ClipSpeed::new(2, 1).unwrap(),
            },
        ],
        ripple: false,
    }
    .apply(&mut project)
    .unwrap();
    let mut bytes = Vec::new();
    write_json(&mut bytes, &project).unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["schemaVersion"],
        SCHEMA_VERSION
    );
    assert_eq!(read_json(bytes.as_slice()).unwrap(), project);
}

/// Minimized cargo-fuzz reproducers: malformed legacy projects are errors, never panics.
#[test]
fn fuzz_regressions_reject_malformed_legacy_json() {
    for bytes in [
        &include_bytes!(
            "../../../fuzz/regressions/project_json/minimized-from-04541f12aac167628b9b67d9ccd1e49acfc0992a"
        )[..],
        &include_bytes!(
            "../../../fuzz/regressions/project_json/minimized-from-497d5c65db5b2e4d1f081411b442b18ff5b62a1f"
        )[..],
        br#"{"schemaVersion":5,"tracks":[1,{"clips":{"1":7}}]}"#,
        br#"{"schemaVersion":1,"tracks":[{"clips":{"1":7}}]}"#,
    ] {
        let result = std::panic::catch_unwind(|| {
            animathio_format::read_json_with_migration(bytes, |_, v| Ok(v))
        });
        assert!(result.expect("must not panic").is_err());
    }
}

#[test]
fn save_amio_drops_orphan_bytes_but_keeps_manifest_assets() {
    let (project, assets) = fixture(b"referenced");
    let mut cache = assets.clone();
    // Bytes of a removed asset: the manifest no longer lists it, so the save
    // must not write them, but the cache keeps them for undo.
    cache.insert(AssetId::of(b"removed"), b"removed".to_vec());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.amio");
    save_amio(&path, &project, &cache).unwrap();
    let bundle = read_amio(std::fs::File::open(&path).unwrap(), Limits::default()).unwrap();
    assert_eq!(bundle.project, project);
    assert_eq!(bundle.assets, assets);
    // An unused but manifest-listed asset is still written.
    let mut unused = project.clone();
    let unused_id = AssetId::of(b"unused bytes");
    Command::AddAsset {
        id: unused_id.clone(),
        asset: Asset {
            name: "unused.mp4".into(),
            media_type: "video/mp4".into(),
        },
    }
    .apply(&mut unused)
    .unwrap();
    let unused_bytes = b"unused bytes".to_vec();
    let mut cache = cache.clone();
    cache.insert(unused_id, unused_bytes.clone());
    let path = dir.path().join("unused.amio");
    save_amio(&path, &unused, &cache).unwrap();
    let bundle = read_amio(std::fs::File::open(&path).unwrap(), Limits::default()).unwrap();
    assert_eq!(
        bundle.assets.get(&AssetId::of(b"unused bytes")),
        Some(&unused_bytes)
    );
    // Missing bytes for a manifest asset still fail the save.
    let path = dir.path().join("missing.amio");
    let mut no_bytes = unused.clone();
    Command::RemoveAsset {
        id: AssetId::of(b"unused bytes"),
    }
    .apply(&mut no_bytes)
    .unwrap();
    Command::AddAsset {
        id: AssetId::of(b"unused bytes"),
        asset: Asset {
            name: "unused.mp4".into(),
            media_type: "video/mp4".into(),
        },
    }
    .apply(&mut no_bytes)
    .unwrap();
    assert!(save_amio(&path, &no_bytes, &assets).is_err());
    assert!(!path.exists());
}

#[test]
fn referenced_assets_matches_manifest_filter() {
    let (project, assets) = fixture(b"kept");
    let mut cache = assets.clone();
    cache.insert(AssetId::of(b"dropped"), b"dropped".to_vec());
    let referenced = referenced_assets(&project, &cache);
    assert_eq!(referenced, assets);
}

#[test]
fn plugin_asset_references_roundtrip_and_dangling_documents_are_rejected() {
    for keyframed in [false, true] {
        let (mut project, assets) = fixture(b"plugin texture");
        // Leave only a plugin reference, so a native source cannot mask the bug.
        Command::RemoveClip { id: ClipId(1) }
            .apply(&mut project)
            .unwrap();
        let id = AssetId::of(b"plugin texture");
        let command = if keyframed {
            Command::AddKeyframe {
                id: ClipId(2),
                target: Target::Param("texture".into()),
                time: Time(200),
                keyframe: Keyframe {
                    value: ParamValue::AssetRef(Some(id)),
                    interpolation: Interpolation::Hold,
                },
            }
        } else {
            Command::SetPluginParam {
                id: ClipId(2),
                name: "texture".into(),
                value: Some(ParamValue::AssetRef(Some(id))),
            }
        };
        command.apply(&mut project).unwrap();
        assert_eq!(read_json(json_bytes(&project).as_slice()).unwrap(), project);
        let mut bytes = write_amio(Cursor::new(Vec::new()), &project, &assets).unwrap();
        bytes.set_position(0);
        assert_eq!(read_amio(bytes, Limits::default()).unwrap().assets, assets);

        // Simulate an older writer's corrupted document; reopening must fail.
        let mut value = serde_json::to_value(&project).unwrap();
        value["format"] = json!("animathio");
        value["schemaVersion"] = json!(SCHEMA_VERSION);
        value["assets"] = json!({});
        let json = serde_json::to_vec(&value).unwrap();
        let error = read_json(json.as_slice()).unwrap_err().to_string();
        assert!(error.contains("missing asset"), "{error}");
        assert!(read_amio(archive(&[("project.json", &json)]), Limits::default()).is_err());
    }
}

#[test]
fn effect_parameters_cannot_contain_typed_asset_references() {
    let (project, _) = fixture(b"texture");
    let effect = json!({
        "id": 1,
        "plugin": "org.example.effect",
        "effect": "texture",
        "params": {"texture": {"AssetRef": AssetId::of(b"texture")}}
    });
    for chain in ["effects", "audio_effects"] {
        let mut value = serde_json::to_value(&project).unwrap();
        value["tracks"][1]["clips"]["2"][chain] = json!([effect.clone()]);
        assert!(serde_json::from_value::<Project>(value).is_err());
    }
    let mut value = serde_json::to_value(&project).unwrap();
    value["tracks"][1]["audio_effects"] = json!([effect]);
    assert!(serde_json::from_value::<Project>(value).is_err());
}

#[test]
fn schema_seven_keeps_legacy_pitch_and_schema_eight_roundtrips_the_option() {
    let (project, _) = fixture(b"pitch migration");
    assert!(project.tracks()[0].clips()[&ClipId(1)].maintain_pitch);
    let mut value: serde_json::Value = serde_json::from_slice(&json_bytes(&project)).unwrap();
    value["schemaVersion"] = json!(7);
    for track in value["tracks"].as_array_mut().unwrap() {
        for clip in track["clips"].as_object_mut().unwrap().values_mut() {
            clip.as_object_mut().unwrap().remove("maintain_pitch");
        }
    }
    let mut migrated = read_json(serde_json::to_vec(&value).unwrap().as_slice()).unwrap();
    assert!(!migrated.tracks()[0].clips()[&ClipId(1)].maintain_pitch);
    Command::SetClipMaintainPitch {
        id: ClipId(1),
        maintain_pitch: true,
    }
    .apply(&mut migrated)
    .unwrap();
    assert_eq!(
        read_json(json_bytes(&migrated).as_slice()).unwrap(),
        migrated
    );
}

#[test]
fn checked_in_schema_seven_project_loads_with_pitch_preservation_off() {
    // The fixture is a real schema-7 file: no `maintain_pitch` key anywhere.
    let text = include_str!("fixtures/v7-retimed-audio.json");
    assert!(!text.contains("maintain_pitch"));
    let project = read_json(text.as_bytes()).unwrap();
    let clip = &project.tracks()[0].clips()[&ClipId(1)];
    assert!(
        !clip.maintain_pitch,
        "old projects keep their varispeed sound"
    );
    assert_eq!(
        clip.time_map.speed,
        animathio_core::ClipSpeed::new(3, 2).unwrap()
    );
    // Saving writes the current schema with the explicit flag, and it round-trips.
    let written: serde_json::Value = serde_json::from_slice(&json_bytes(&project)).unwrap();
    assert_eq!(written["schemaVersion"], SCHEMA_VERSION);
    assert_eq!(written["tracks"][0]["clips"]["1"]["maintain_pitch"], false);
    assert_eq!(read_json(json_bytes(&project).as_slice()).unwrap(), project);
}
