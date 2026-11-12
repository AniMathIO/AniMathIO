//! Integration tests for the plugin host: loading, sandboxing, caching and schema conversion.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use animathio_core::{ClipId, ClipKind, Scene as CoreScene, SceneClip, Time, TrackId};
use animathio_plugin_api::{InspectorSchema, PluginManifestToml, PluginPackage};
use animathio_plugin_host::{HostError, PluginHost};
use serde_json::json;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("plugin-host should live in crates/plugin-host")
        .to_path_buf()
}

#[derive(Clone)]
struct BuiltPlugin {
    manifest: PluginManifestToml,
    wasm: Vec<u8>,
}

/// Build a plugin crate for `wasm32-wasip2` and return its manifest + component bytes.
fn build_plugin(name: &str) -> BuiltPlugin {
    let root = workspace_root();
    let manifest_path = root.join("plugins").join(name).join("plugin.toml");
    let crate_path = root.join("plugins").join(name).join("Cargo.toml");
    let target_dir = root.join("target").join("plugin-tests");

    let mut command = if cfg!(target_os = "linux") {
        let mut command = Command::new("nice");
        command.args(["-n", "19", "ionice", "-c3", "cargo"]);
        command
    } else {
        Command::new("cargo")
    };
    let status = command
        .args([
            "build",
            "--manifest-path",
            crate_path.to_str().expect("valid plugin path"),
            "--target",
            "wasm32-wasip2",
            "--release",
            "--locked",
        ])
        .env("CARGO_TARGET_DIR", &target_dir)
        .env("CARGO_BUILD_JOBS", "3")
        .env("CARGO_PROFILE_DEV_DEBUG", "line-tables-only")
        .status()
        .expect("failed to spawn cargo build for plugin");
    assert!(status.success(), "failed to build plugin `{name}`");

    let wasm = std::fs::read(
        target_dir
            .join("wasm32-wasip2")
            .join("release")
            .join(format!("{name}.wasm")),
    )
    .unwrap_or_else(|e| panic!("plugin `{name}` produced no wasm binary: {e}"));

    let manifest_str = std::fs::read_to_string(&manifest_path).expect("plugin.toml missing");
    let manifest = PluginManifestToml::parse(&manifest_str).expect("invalid plugin.toml");

    BuiltPlugin { manifest, wasm }
}

fn built_plugins() -> &'static Vec<(String, BuiltPlugin)> {
    static PLUGINS: OnceLock<Vec<(String, BuiltPlugin)>> = OnceLock::new();
    PLUGINS.get_or_init(|| {
        ["shapes", "text", "trap"]
            .into_iter()
            .map(|name| (name.to_string(), build_plugin(name)))
            .collect()
    })
}

/// `.amplugin` bytes for a plugin: the prebuilt artifact if present, else build + package.
fn amplugin_bytes(name: &str) -> Vec<u8> {
    let prebuilt = workspace_root()
        .join("target")
        .join("plugins")
        .join(format!("{name}.amplugin"));
    if prebuilt.exists() {
        return std::fs::read(&prebuilt).unwrap();
    }

    let built = built_plugins()
        .iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("plugin `{name}` not built"))
        .1
        .clone();
    let package = PluginPackage::new(built.manifest.clone(), built.wasm.clone()).unwrap();
    package.create_bytes().unwrap()
}

/// Load a plugin into a fresh host, building it first if the prebuilt artifact is absent.
fn load_plugin(host: &mut PluginHost, name: &str) -> String {
    let bytes = amplugin_bytes(name);
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(format!("{name}.amplugin"));
    std::fs::write(&path, bytes).unwrap();
    host.load_plugin_file(&path).unwrap()
}

fn plugin_clip(plugin_id: impl Into<String>, params: serde_json::Value) -> SceneClip {
    SceneClip {
        track: TrackId(0),
        id: ClipId(1),
        kind: ClipKind::Plugin {
            plugin_id: plugin_id.into(),
            params,
        },
        source_time: Time(0),
        transform: animathio_core::Transform::default(),
        params: BTreeMap::new(),
        effects: Vec::new(),
    }
}

#[test]
fn loads_both_plugins_and_resolves_scene() {
    let tmp = tempfile::tempdir().unwrap();
    let mut host = PluginHost::new(tmp.path()).unwrap();

    // Exercise the directory registry: write both `.amplugin` files and load them.
    for name in ["shapes", "text"] {
        let bytes = amplugin_bytes(name);
        std::fs::write(tmp.path().join(format!("{name}.amplugin")), bytes).unwrap();
    }

    let ids = host.load_plugins_dir(tmp.path()).unwrap();
    assert_eq!(ids.len(), 2);
    assert!(ids.iter().any(|id| id == "org.animathio.shapes"));
    assert!(ids.iter().any(|id| id == "org.animathio.text"));

    let scene = CoreScene {
        clips: vec![
            plugin_clip(
                "org.animathio.shapes",
                json!({ "shape": "star", "radius": 80.0, "points": 5.0 }),
            ),
            plugin_clip(
                "org.animathio.text",
                json!({ "text": "AniMathIO", "font_size": 40.0 }),
            ),
        ],
    };

    let resolved = host.resolve(&scene, (1280, 720));
    assert_eq!(resolved.clips.len(), 2);
    for clip in &resolved.clips {
        match &clip.kind {
            ClipKind::Vector(vector) => assert!(!vector.elements.is_empty()),
            other => panic!("expected vector clip, got {other:?}"),
        }
    }
}

#[test]
fn renders_plugins_at_several_times() {
    let tmp = tempfile::tempdir().unwrap();
    let mut host = PluginHost::new(tmp.path()).unwrap();
    let shapes = load_plugin(&mut host, "shapes");
    let text = load_plugin(&mut host, "text");

    let times = [0, 7_056_000, 70_560_000, 705_600_000];
    for t in times {
        let shapes_scene = host
            .render_clip(
                &shapes,
                &BTreeMap::from([("radius".to_string(), 100.0.into())]),
                &json!({ "shape": "polygon", "points": 6.0 }),
                t,
                (1280, 720),
            )
            .unwrap();
        assert!(!shapes_scene.elements.is_empty(), "shapes empty at t={t}");

        let text_scene = host
            .render_clip(
                &text,
                &BTreeMap::new(),
                &json!({ "text": "Hello", "font_size": 32.0, "align": "left" }),
                t,
                (1280, 720),
            )
            .unwrap();
        assert!(!text_scene.elements.is_empty(), "text empty at t={t}");
    }
}

#[test]
fn trap_fixture_is_contained() {
    let tmp = tempfile::tempdir().unwrap();
    let mut host = PluginHost::new(tmp.path()).unwrap();
    let trap = load_plugin(&mut host, "trap");

    // The infinite-loop fixture must trap instead of crashing the host.
    let result = host.render_clip(&trap, &BTreeMap::new(), &json!({}), 0, (640, 360));
    assert!(result.is_err(), "trap fixture should fail to render");

    let scene = CoreScene {
        clips: vec![plugin_clip(&trap, json!({}))],
    };
    let resolved = host.resolve(&scene, (640, 360));
    assert!(matches!(resolved.clips[0].kind, ClipKind::Error(_)));

    // The host stays usable: a healthy plugin still renders afterwards.
    let shapes = load_plugin(&mut host, "shapes");
    let ok = host
        .render_clip(
            &shapes,
            &BTreeMap::new(),
            &json!({ "shape": "rect" }),
            0,
            (640, 360),
        )
        .unwrap();
    assert!(!ok.elements.is_empty());
}

#[test]
fn output_cache_hits_on_identical_params() {
    let tmp = tempfile::tempdir().unwrap();
    let mut host = PluginHost::new(tmp.path()).unwrap();
    let shapes = load_plugin(&mut host, "shapes");

    let params = BTreeMap::from([
        ("x".to_string(), 100.0.into()),
        ("y".to_string(), 120.0.into()),
    ]);
    let opaque = json!({ "shape": "rect", "width": 50.0, "height": 40.0 });

    let first = host
        .render_clip(&shapes, &params, &opaque, 0, (200, 200))
        .unwrap();
    let second = host
        .render_clip(&shapes, &params, &opaque, 0, (200, 200))
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(host.cache_misses(), 1);
    assert_eq!(host.cache_hits(), 1);

    // A different time is a different cache key (a miss), and an animated shape
    // produces different geometry one second later.
    let poly = json!({ "shape": "polygon", "points": 5.0 });
    let at_zero = host
        .render_clip(&shapes, &BTreeMap::new(), &poly, 0, (200, 200))
        .unwrap();
    let at_one_second = host
        .render_clip(&shapes, &BTreeMap::new(), &poly, 705_600_000, (200, 200))
        .unwrap();
    assert_ne!(at_zero, at_one_second);
    assert_eq!(host.cache_misses(), 3);
    assert_eq!(host.cache_hits(), 1);
}

#[test]
fn oversized_shape_text_returns_empty_scene() {
    let tmp = tempfile::tempdir().unwrap();
    let mut host = PluginHost::new(tmp.path()).unwrap();
    let text = load_plugin(&mut host, "text");

    let huge = "x".repeat(20_000);
    let scene = host
        .render_clip(
            &text,
            &BTreeMap::new(),
            &json!({ "text": huge, "font_size": 48.0 }),
            0,
            (1280, 720),
        )
        .unwrap();
    assert!(
        scene.elements.is_empty(),
        "oversized text must be rejected by the host shaper"
    );
}

#[test]
fn duplicate_plugin_id_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let mut host = PluginHost::new(tmp.path()).unwrap();
    let package = PluginPackage::from_bytes(&amplugin_bytes("shapes")).unwrap();

    host.load_plugin_package(package.clone()).unwrap();
    let err = host.load_plugin_package(package).unwrap_err();
    assert!(matches!(err, HostError::DuplicateId(_)));
}

#[test]
fn inspector_schema_roundtrips() {
    let tmp = tempfile::tempdir().unwrap();
    let mut host = PluginHost::new(tmp.path()).unwrap();
    let shapes = load_plugin(&mut host, "shapes");

    let schema = host.get_inspector_schema(&shapes).unwrap();
    assert!(!schema.params.is_empty());
    assert!(schema.params.iter().any(|p| p.id == "shape"));
    assert!(schema.params.iter().any(|p| p.id == "radius"));

    let json = serde_json::to_string(schema).unwrap();
    let roundtripped: InspectorSchema = serde_json::from_str(&json).unwrap();
    assert_eq!(schema, &roundtripped);
}

#[test]
fn evaluated_colour_keyframes_reach_plugin_output_and_cache() {
    use animathio_core::{
        Clip, Command as Edit, Interpolation, Keyframe, ParamValue, Project, Target, TimeRange,
        Track, evaluate,
    };
    let tmp = tempfile::tempdir().unwrap();
    let mut host = PluginHost::new(tmp.path()).unwrap();
    let shapes = load_plugin(&mut host, "shapes");
    let mut project = Project::default();
    Edit::AddTrack {
        track: Track::new(TrackId(0), "colour"),
        index: 0,
    }
    .apply(&mut project)
    .unwrap();
    let mut clip = Clip::new(
        ClipId(0),
        ClipKind::Plugin {
            plugin_id: shapes,
            params: json!({"shape":"circle","fill":[255,0,0,255]}),
        },
        TimeRange::new(Time(0), Time(101)).unwrap(),
    );
    clip.params
        .insert("fill".into(), ParamValue::Color([0.0, 0.0, 0.0, 1.0]));
    Edit::AddClip {
        track: TrackId(0),
        clip,
    }
    .apply(&mut project)
    .unwrap();
    for (time, color) in [(0, [0.0, 0.0, 0.0, 1.0]), (100, [1.0, 1.0, 1.0, 1.0])] {
        Edit::AddKeyframe {
            id: ClipId(0),
            target: Target::Param("fill".into()),
            time: Time(time),
            keyframe: Keyframe {
                value: ParamValue::Color(color),
                interpolation: Interpolation::Linear,
            },
        }
        .apply(&mut project)
        .unwrap();
    }
    for (time, expected) in [(0, 0), (50, 188), (100, 255)] {
        let scene = evaluate(&project, Time(time));
        let resolved = host.resolve(&scene, (200, 200));
        let ClipKind::Vector(vector) = &resolved.clips[0].kind else {
            panic!("plugin must render")
        };
        assert!(vector.elements.iter().any(|element| matches!(element, animathio_core::VectorElement::FillPath { color, .. } if *color == animathio_core::Color::rgba(expected,expected,expected,255))));
    }
    // Same time and plugin, different typed colour: cached output must change.
    let mut scene = evaluate(&project, Time(50));
    scene.clips[0]
        .params
        .insert("fill".into(), ParamValue::Color([1.0, 0.0, 0.0, 1.0]));
    let resolved = host.resolve(&scene, (200, 200));
    let ClipKind::Vector(vector) = &resolved.clips[0].kind else {
        panic!("plugin must render")
    };
    assert!(vector.elements.iter().any(|element| matches!(element, animathio_core::VectorElement::FillPath { color, .. } if *color == animathio_core::Color::rgba(255,0,0,255))));
}
