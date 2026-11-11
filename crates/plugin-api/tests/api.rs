use animathio_plugin_api::*;

#[test]
fn manifest_toml_roundtrip_and_validation() {
    let toml_str = r#"
api_version = "0.1.0"
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
api_version = "0.1.0"
id = "org/animathio/bad"
name = "Bad"
version = "0.1.0"
kinds = ["clip-source"]
"#;
    assert!(PluginManifestToml::parse(bad_id).is_err());

    // Empty kinds
    let empty_kinds = r#"
api_version = "0.1.0"
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
        api_version: "0.1.0".into(),
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
