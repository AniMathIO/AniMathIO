//! Integration tests for the `latex` and `graph` math plugins, including
//! golden PNGs rendered through `crates/render`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use animathio_core::{
    ClipId, ClipKind, Scene as CoreScene, SceneClip, Time, TrackId, Transform, VectorScene,
};
use animathio_plugin_api::{PluginManifestToml, PluginPackage};
use animathio_plugin_host::PluginHost;
use animathio_render::{RenderCtx, render};
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

fn build_plugin(name: &str) -> BuiltPlugin {
    let root = workspace_root();
    let manifest_path = root.join("plugins").join(name).join("plugin.toml");
    let crate_path = root.join("plugins").join(name).join("Cargo.toml");
    let target_dir = root.join("target").join("plugin-tests-math");

    let status = Command::new("cargo")
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
        ["latex", "graph"]
            .into_iter()
            .map(|name| (name.to_string(), build_plugin(name)))
            .collect()
    })
}

fn load_plugin(host: &mut PluginHost, name: &str) -> String {
    let built = built_plugins()
        .iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("plugin `{name}` not built"))
        .1
        .clone();
    let package = PluginPackage::new(built.manifest.clone(), built.wasm.clone()).unwrap();
    host.load_plugin_package(package).unwrap()
}

fn vector_clip(scene: VectorScene) -> SceneClip {
    SceneClip {
        track: TrackId(0),
        id: ClipId(1),
        kind: ClipKind::Vector(scene),
        source_time: Time(0),
        transform: Transform::default(),
        params: BTreeMap::new(),
    }
}

fn golden_vector(scene: VectorScene, name: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden");
    std::fs::create_dir_all(&root).unwrap();
    let core_scene = CoreScene {
        clips: vec![vector_clip(scene)],
    };
    let mut ctx = RenderCtx::default();
    let actual = render(&core_scene, &mut ctx, (640, 360)).unwrap();
    let path = root.join(format!("{name}.png"));
    if std::env::var("UPDATE_GOLDEN").as_deref() == Ok("1") {
        actual.save_png(&path).unwrap();
    }
    let expected = image::open(&path)
        .unwrap_or_else(|e| panic!("{}: {e}; regenerate with UPDATE_GOLDEN=1", path.display()))
        .to_rgba8();
    assert_eq!(expected.dimensions(), actual.size);
    let differences = expected
        .as_raw()
        .iter()
        .zip(&actual.pixels)
        .filter(|(a, b)| a.abs_diff(**b) > 2)
        .count();
    if differences != 0 {
        let output = std::env::temp_dir().join(format!("animathio-math-{name}-actual.png"));
        actual.save_png(&output).unwrap();
        panic!(
            "{differences} channels exceeded tolerance 2; actual frame: {}",
            output.display()
        );
    }
}

#[test]
fn latex_typesets_required_expressions() {
    let tmp = tempfile::tempdir().unwrap();
    let mut host = PluginHost::new(tmp.path()).unwrap();
    let latex = load_plugin(&mut host, "latex");

    for source in [r"\frac{a}{b}", r"\int_0^1 x^2\,dx", r"e^{i\pi}+1=0"] {
        let scene = host
            .render_clip(
                &latex,
                &BTreeMap::new(),
                &json!({ "source": source, "mode": "latex", "size": 48.0 }),
                0,
                (640, 360),
            )
            .unwrap();
        assert!(
            !scene.elements.is_empty(),
            "latex `{source}` produced an empty scene"
        );
    }
}

#[test]
fn invalid_latex_returns_empty_scene_without_panicking() {
    let tmp = tempfile::tempdir().unwrap();
    let mut host = PluginHost::new(tmp.path()).unwrap();
    let latex = load_plugin(&mut host, "latex");

    let scene = host
        .render_clip(
            &latex,
            &BTreeMap::new(),
            &json!({ "source": r"\frac{a}{b", "mode": "latex", "size": 48.0 }),
            0,
            (640, 360),
        )
        .unwrap();
    assert!(
        scene.elements.is_empty(),
        "invalid LaTeX must not produce paths"
    );

    // The host stays usable after the failed typesetting call.
    let ok = host
        .render_clip(
            &latex,
            &BTreeMap::new(),
            &json!({ "source": r"\frac{a}{b}", "mode": "latex", "size": 48.0 }),
            0,
            (640, 360),
        )
        .unwrap();
    assert!(!ok.elements.is_empty());
}

#[test]
fn graph_renders_parabola_and_unit_circle() {
    let tmp = tempfile::tempdir().unwrap();
    let mut host = PluginHost::new(tmp.path()).unwrap();
    let graph = load_plugin(&mut host, "graph");

    let scene = host
        .render_clip(
            &graph,
            &BTreeMap::new(),
            &json!({
                "items": "plot y=x^2\ncircle cx=0 cy=0 r=1",
                "show_labels": false,
            }),
            0,
            (640, 360),
        )
        .unwrap();
    assert!(!scene.elements.is_empty(), "graph produced an empty scene");
    assert!(
        scene
            .elements
            .iter()
            .any(|e| matches!(e, animathio_core::VectorElement::StrokePath { .. })),
        "graph should contain stroked plot and circle paths"
    );
}

#[test]
fn latex_golden_frame() {
    let tmp = tempfile::tempdir().unwrap();
    let mut host = PluginHost::new(tmp.path()).unwrap();
    let latex = load_plugin(&mut host, "latex");

    let scene = host
        .render_clip(
            &latex,
            &BTreeMap::new(),
            &json!({ "source": r"e^{i\pi}+1=0", "mode": "latex", "size": 64.0 }),
            0,
            (640, 360),
        )
        .unwrap();
    golden_vector(scene, "latex-identity");
}

#[test]
fn graph_golden_frame() {
    let tmp = tempfile::tempdir().unwrap();
    let mut host = PluginHost::new(tmp.path()).unwrap();
    let graph = load_plugin(&mut host, "graph");

    let scene = host
        .render_clip(
            &graph,
            &BTreeMap::new(),
            &json!({
                "items": "plot y=x^2\ncircle cx=0 cy=0 r=1",
                "show_labels": false,
            }),
            0,
            (640, 360),
        )
        .unwrap();
    golden_vector(scene, "graph-parabola-circle");
}
