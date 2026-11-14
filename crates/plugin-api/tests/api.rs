use animathio_plugin_api::*;
use std::io::Write;

#[test]
fn manifest_toml_roundtrip_and_validation() {
    let toml_str = r#"
api_version = "0.2.0"
id = "org.animathio.shapes"
name = "Shapes"
version = "0.1.0"
description = "Basic shapes plugin"
kinds = ["clip-source"]
capabilities = []
"#;

    let manifest = PluginManifestToml::parse(toml_str).unwrap();
    assert_eq!(manifest.id, "org.animathio.shapes");
    assert_eq!(manifest.kinds, vec![PluginKind::ClipSource]);

    let serialized = manifest.to_toml_string().unwrap();
    let re_parsed = PluginManifestToml::parse(&serialized).unwrap();
    assert_eq!(manifest, re_parsed);
}

#[test]
fn manifest_validation_rejects_invalid() {
    // Missing api_version
    assert!(PluginManifestToml::parse(r#"id = "foo"\nname = "bar"\nversion = "1""#).is_err());

    // Bad id character
    let bad_id = r#"
api_version = "0.2.0"
id = "org/animathio/bad"
name = "Bad"
version = "0.1.0"
kinds = ["clip-source"]
"#;
    assert!(PluginManifestToml::parse(bad_id).is_err());

    // Empty kinds
    let empty_kinds = r#"
api_version = "0.2.0"
id = "org.animathio.bad"
name = "Bad"
version = "0.1.0"
kinds = []
"#;
    assert!(PluginManifestToml::parse(empty_kinds).is_err());
}

#[test]
fn package_roundtrip_zip_archive() {
    let manifest = PluginManifestToml {
        api_version: "0.2.0".into(),
        id: "org.animathio.test".into(),
        name: "Test Plugin".into(),
        version: "0.1.0".into(),
        description: "Test".into(),
        kinds: vec![PluginKind::ClipSource, PluginKind::Animation],
        capabilities: vec![],
    };

    let fake_wasm = b"\x00asm\x01\x00\x00\x00fake component bytes";
    let package = PluginPackage::new(manifest.clone(), fake_wasm.to_vec()).unwrap();

    let bytes = package.create_bytes().unwrap();
    let loaded = PluginPackage::from_bytes(&bytes).unwrap();

    assert_eq!(loaded.manifest, manifest);
    assert_eq!(loaded.wasm_bytes, fake_wasm);
}

#[test]
fn inspector_schema_roundtrip() {
    let schema = InspectorSchema::new()
        .with_number("radius", "Radius", 10.0, 200.0, 50.0, 1.0)
        .with_color(
            "color",
            "Fill Color",
            ColorValue {
                r: 255,
                g: 0,
                b: 0,
                a: 255,
            },
        )
        .with_string("label", "Text", "Default")
        .with_enum("mode", "Mode", vec!["A".into(), "B".into()], "A")
        .with_bool("visible", "Visible", true)
        .with_point("center", "Center", PointValue { x: 0.5, y: 0.5 })
        .with_asset_ref("bg", "Background", "image/*", None);

    assert_eq!(schema.params.len(), 7);
    let json = serde_json::to_string(&schema).unwrap();
    let deserialized: InspectorSchema = serde_json::from_str(&json).unwrap();
    assert_eq!(schema, deserialized);
}

#[test]
fn guest_shape_helpers() {
    use animathio_plugin_api::guest::{PathVerb, Shapes};

    let rect = Shapes::rect(10.0, 10.0, 100.0, 50.0);
    assert_eq!(rect.len(), 5);
    assert!(matches!(rect[4], PathVerb::Close));

    let circle = Shapes::circle(50.0, 50.0, 25.0);
    assert_eq!(circle.len(), 6);

    let star = Shapes::star(50.0, 50.0, 30.0, 15.0, 5, 0.0);
    assert_eq!(star.len(), 11);

    let arrow = Shapes::arrow(0.0, 0.0, 100.0, 0.0, 10.0, 8.0);
    assert_eq!(arrow.len(), 5);
}

fn zip_bytes(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut cursor);
        let options = zip::write::SimpleFileOptions::default();
        for (name, data) in entries {
            zip.start_file(name.as_str(), options).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }
    cursor.into_inner()
}

const MINIMAL_MANIFEST: &str = r#"
api_version = "0.2.0"
id = "org.animathio.ziptest"
name = "Zip Test"
version = "0.1.0"
kinds = ["clip-source"]
"#;

#[test]
fn oversized_manifest_entry_is_rejected() {
    let mut manifest = MINIMAL_MANIFEST.to_string();
    manifest.push_str("# ");
    manifest.push_str(&"x".repeat(MAX_MANIFEST_BYTES as usize));
    let bytes = zip_bytes(&[
        ("plugin.toml".into(), manifest.into_bytes()),
        ("plugin.wasm".into(), b"\x00asm\x01\x00\x00\x00".to_vec()),
    ]);
    assert!(matches!(
        PluginPackage::from_bytes(&bytes),
        Err(PackageError::EntryTooLarge(MANIFEST_FILENAME))
    ));
}

#[test]
fn oversized_wasm_entry_is_rejected_without_decompressing() {
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut cursor);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file(MANIFEST_FILENAME, options).unwrap();
        zip.write_all(MINIMAL_MANIFEST.as_bytes()).unwrap();
        zip.start_file(WASM_FILENAME, options).unwrap();
        let chunk = [0u8; 65_536];
        let mut remaining = MAX_WASM_BYTES as usize + 1;
        while remaining > 0 {
            let n = remaining.min(chunk.len());
            zip.write_all(&chunk[..n]).unwrap();
            remaining -= n;
        }
        zip.finish().unwrap();
    }
    let bytes = cursor.into_inner();
    assert!(matches!(
        PluginPackage::from_bytes(&bytes),
        Err(PackageError::EntryTooLarge(WASM_FILENAME))
    ));
}

#[test]
fn path_traversal_entry_is_rejected() {
    let bytes = zip_bytes(&[
        ("plugin.toml".into(), MINIMAL_MANIFEST.into()),
        ("plugin.wasm".into(), b"\x00asm\x01\x00\x00\x00".to_vec()),
        ("../evil.toml".into(), b"x".to_vec()),
    ]);
    assert!(matches!(
        PluginPackage::from_bytes(&bytes),
        Err(PackageError::UnsafeEntry(_))
    ));
}

#[test]
fn too_many_entries_is_rejected() {
    let mut entries = vec![
        (
            "plugin.toml".to_string(),
            MINIMAL_MANIFEST.as_bytes().to_vec(),
        ),
        (
            "plugin.wasm".to_string(),
            b"\x00asm\x01\x00\x00\x00".to_vec(),
        ),
    ];
    for index in 0..(MAX_ENTRIES + 1) {
        entries.push((format!("extra-{index}.bin"), b"x".to_vec()));
    }
    let bytes = zip_bytes(&entries);
    assert!(matches!(
        PluginPackage::from_bytes(&bytes),
        Err(PackageError::TooManyEntries)
    ));
}

#[test]
fn rejects_the_incompatible_previous_component_abi() {
    let manifest = PluginManifestToml {
        api_version: "0.1.0".into(),
        id: "org.example.old".into(),
        name: "Old".into(),
        version: "1.0.0".into(),
        description: String::new(),
        kinds: vec![PluginKind::ClipSource],
        capabilities: vec![],
    };
    assert!(manifest.validate().is_err());
}

#[test]
fn abi_negotiation_is_strict_and_rejects_future_requirements() {
    let mut manifest = PluginManifestToml::parse(MINIMAL_MANIFEST).unwrap();
    for api in ["0.2.0", "0.2.1", "0.2.2", "1.0.0", "1.1.0"] {
        manifest.api_version = api.into();
        assert!(manifest.validate().is_ok(), "{api}");
    }
    for api in [
        "0.20.0",
        "0.2",
        "0.2.garbage",
        "0.2.3",
        "0.3.0",
        "1",
        "1.0.1",
        "1.1.1",
        "1.2.0",
        "2.0.0",
        "1.0.0-rc.1",
        "1.0.0+local",
    ] {
        manifest.api_version = api.into();
        assert!(manifest.validate().is_err(), "{api}");
    }
    manifest.api_version = "1.0.0".into();
    for kind in [
        PluginKind::ClipSource,
        PluginKind::Effect,
        PluginKind::AudioEffect,
    ] {
        manifest.kinds = vec![kind];
        assert!(manifest.validate().is_ok());
    }
    // The 1.1 worlds are rejected for a 1.0 requirement and must stand alone.
    for kind in [PluginKind::AnimationPreset, PluginKind::Importer] {
        manifest.api_version = "1.0.0".into();
        manifest.kinds = vec![kind];
        assert!(manifest.validate().is_err(), "{kind:?} needs 1.1.0");
        manifest.api_version = "1.1.0".into();
        assert!(manifest.validate().is_ok());
        manifest.kinds = vec![kind, PluginKind::ClipSource];
        assert!(manifest.validate().is_err());
    }
    manifest.kinds = vec![PluginKind::Animation];
    assert!(manifest.validate().is_err());
    manifest.kinds = vec![PluginKind::ClipSource, PluginKind::ClipSource];
    assert!(manifest.validate().is_err());
    manifest.kinds = vec![PluginKind::ClipSource];
    manifest.version = "garbage".into();
    assert!(manifest.validate().is_err());
}

#[test]
fn documented_example_is_verbatim_and_targets_the_frozen_abi() {
    // Windows checkouts may convert line endings; compare on LF only.
    let docs = include_str!("../../../docs/plugins.md").replace("\r\n", "\n");
    for (path, lang, contents) in [
        (
            "plugins/examples/hello/Cargo.toml",
            "toml",
            include_str!("../../../plugins/examples/hello/Cargo.toml"),
        ),
        (
            "plugins/examples/hello/plugin.toml",
            "toml",
            include_str!("../../../plugins/examples/hello/plugin.toml"),
        ),
        (
            "plugins/examples/hello/src/lib.rs",
            "rust",
            include_str!("../../../plugins/examples/hello/src/lib.rs"),
        ),
    ] {
        let contents = contents.replace("\r\n", "\n");
        let marker = format!("**`{path}`**\n\n```{lang}\n{}\n```", contents.trim_end());
        assert!(docs.contains(&marker), "{path} must be copied verbatim");
    }
    assert!(
        include_str!("../wit/plugin.wit")
            .starts_with(&format!("package animathio:plugin@{API_VERSION};"))
    );
}
