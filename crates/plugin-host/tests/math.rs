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
use animathio_plugin_host::{PluginDirectory, PluginHost};
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
        .env("CARGO_INCREMENTAL", "0")
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

#[test]
fn discovery_rejects_user_packages_with_reserved_ids_but_loads_trusted_bundles() {
    let built = &built_plugins()
        .iter()
        .find(|(name, _)| name == "latex")
        .unwrap()
        .1;
    let user = tempfile::tempdir().unwrap();
    let bundled = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let package = PluginPackage::new(built.manifest.clone(), built.wasm.clone()).unwrap();
    let bytes = package.create_bytes().unwrap();
    std::fs::write(user.path().join("latex.amplugin"), &bytes).unwrap();
    std::fs::write(bundled.path().join("latex.amplugin"), &bytes).unwrap();
    let mut host = PluginHost::new(cache.path()).unwrap();
    assert!(
        host.load_plugin_directory(&PluginDirectory {
            path: user.path().to_owned(),
            allow_reserved_ids: false
        })
        .unwrap()
        .is_empty()
    );
    assert!(host.loaded_plugin_ids().is_empty());
    let compiled = cache.path().join(format!(
        "{}.bin",
        animathio_plugin_host::ComponentDiskCache::cache_key(&built.wasm)
    ));
    assert!(
        !compiled.exists(),
        "rejected first-party ids must not reach component compilation"
    );
    assert_eq!(
        host.load_plugin_directory(&PluginDirectory {
            path: bundled.path().to_owned(),
            allow_reserved_ids: true
        })
        .unwrap(),
        vec!["org.animathio.latex"]
    );
    let mut manifest = built.manifest.clone();
    manifest.id = "com.example.math".to_owned();
    let package = PluginPackage::new(manifest, built.wasm.clone()).unwrap();
    std::fs::write(
        user.path().join("custom.amplugin"),
        package.create_bytes().unwrap(),
    )
    .unwrap();
    assert_eq!(
        host.load_plugin_directory(&PluginDirectory {
            path: user.path().to_owned(),
            allow_reserved_ids: false
        })
        .unwrap(),
        vec!["com.example.math"]
    );
}

fn vector_clip(scene: VectorScene) -> SceneClip {
    SceneClip {
        track: TrackId(0),
        id: ClipId(1),
        kind: ClipKind::Vector(scene),
        source_time: Time(0),
        transform: Transform::default(),
        params: BTreeMap::new(),
        effects: Vec::new(),
        transition: None,
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
fn invalid_latex_draws_its_message_without_failing_the_frame() {
    let tmp = tempfile::tempdir().unwrap();
    let mut host = PluginHost::new(tmp.path()).unwrap();
    let latex = load_plugin(&mut host, "latex");

    // A formula that does not typeset (mid-edit in the inspector) draws a red message in place
    // of the formula instead of failing the whole frame.
    let scene = host
        .render_clip(
            &latex,
            &BTreeMap::new(),
            &json!({ "source": r"\frac{a}{b", "mode": "latex", "size": 48.0 }),
            0,
            (640, 360),
        )
        .unwrap();
    assert!(!scene.elements.is_empty());

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

#[test]
fn graph_styles_survive_the_component_boundary() {
    use animathio_core::{Color, VectorElement};

    let tmp = tempfile::tempdir().unwrap();
    let mut host = PluginHost::new(tmp.path()).unwrap();
    let graph = load_plugin(&mut host, "graph");
    let scene = host
        .render_clip(
            &graph,
            &BTreeMap::new(),
            &json!({
                "items": "plot y=sin(x)\nvector x1=0 y1=0 x2=1 y2=1",
                "show_labels": false,
                "color": [12, 34, 56, 255],
                "stroke_width": 6.0,
                "axis_color": [210, 100, 20, 255],
                "grid_color": [10, 30, 50, 80],
            }),
            0,
            (640, 360),
        )
        .unwrap();
    let styles: Vec<_> = scene
        .elements
        .iter()
        .filter_map(|element| match element {
            VectorElement::StrokePath { style, .. } => Some(style),
            _ => None,
        })
        .collect();
    assert!(
        styles
            .iter()
            .any(|style| style.color == Color::rgba(10, 30, 50, 80))
    );
    assert_eq!(
        styles
            .iter()
            .filter(|style| style.color == Color::rgb(210, 100, 20))
            .count(),
        2,
        "both axes must use the requested color"
    );
    let plots: Vec<_> = styles
        .iter()
        .filter(|style| style.color == Color::rgb(12, 34, 56))
        .collect();
    assert_eq!(plots.len(), 2, "plot and vector must both survive");
    assert!(
        plots
            .iter()
            .all(|style| (style.width - 6.0).abs() < f64::EPSILON)
    );
}

#[test]
fn graph_parametric_progress_and_axis_scaling() {
    use animathio_core::{PathVerb, VectorElement};

    let tmp = tempfile::tempdir().unwrap();
    let mut host = PluginHost::new(tmp.path()).unwrap();
    let graph = load_plugin(&mut host, "graph");
    for (width, height) in [(640, 360), (360, 640)] {
        for (mode, half_x, half_y) in [
            ("uniform", 180.0, 180.0),
            ("stretch", f64::from(width) / 2.0, f64::from(height) / 2.0),
        ] {
            for draw in [0.0, 0.25, 0.5, 1.0] {
                let mut params = json!({
                    "items": "parametric x=cos(t) y=sin(t)",
                    "x_min": -1, "x_max": 1, "y_min": -1, "y_max": 1,
                    "show_grid": false, "show_axes": false, "show_labels": false,
                    "draw": draw,
                });
                // Omission must retain the uniform default introduced in #234.
                if mode == "stretch" {
                    params["scale_mode"] = json!(mode);
                }
                let scene = host
                    .render_clip(&graph, &BTreeMap::new(), &params, 0, (width, height))
                    .unwrap();
                if draw < f64::EPSILON {
                    assert!(scene.elements.is_empty());
                    continue;
                }
                let VectorElement::StrokePath { path, .. } = &scene.elements[0] else {
                    panic!("expected parametric curve");
                };
                let Some(PathVerb::LineTo(end)) = path.last() else {
                    panic!("expected sampled curve endpoint");
                };
                let angle = std::f64::consts::TAU * draw;
                assert!((end.x - (f64::from(width) / 2.0 + half_x * angle.cos())).abs() < 0.001);
                assert!((end.y - (f64::from(height) / 2.0 - half_y * angle.sin())).abs() < 0.001);
            }
        }
    }
}

#[test]
fn graph_grid_stays_inside_a_transformed_viewport() {
    use animathio_core::{PathVerb, VectorElement};

    let tmp = tempfile::tempdir().unwrap();
    let mut host = PluginHost::new(tmp.path()).unwrap();
    let graph = load_plugin(&mut host, "graph");
    let scene = host
        .render_clip(
            &graph,
            &BTreeMap::new(),
            &json!({
                "items": "", "scale_mode": "stretch",
                "x_min": 0.2, "x_max": 6.1, "y_min": -1.5, "y_max": 1.5,
                "show_axes": false, "show_labels": false,
            }),
            0,
            (640, 360),
        )
        .unwrap();
    assert!(!scene.elements.is_empty());
    for element in scene.elements {
        let VectorElement::StrokePath { path, .. } = element else {
            panic!("expected grid line");
        };
        for verb in path {
            let (PathVerb::MoveTo(point) | PathVerb::LineTo(point)) = verb else {
                panic!("expected straight grid line");
            };
            assert!((0.0..=640.0).contains(&point.x), "{point:?}");
            assert!((0.0..=360.0).contains(&point.y), "{point:?}");
        }
    }
}

#[test]
fn graph_parametric_rejects_invalid_domains_and_breaks_at_nonfinite_values() {
    use animathio_core::{PathVerb, VectorElement};

    let tmp = tempfile::tempdir().unwrap();
    let mut host = PluginHost::new(tmp.path()).unwrap();
    let graph = load_plugin(&mut host, "graph");
    for (items, expected_moves) in [
        ("parametric x=t y=1/t from=-1 to=1", 2),
        ("parametric x=unknown(t) y=t", 0),
        ("parametric x=t y=t from=1 to=-1", 0),
        ("parametric x=t y=t from=NaN to=1", 0),
    ] {
        let scene = host
            .render_clip(
                &graph,
                &BTreeMap::new(),
                &json!({
                    "items": items,
                    "show_grid": false, "show_axes": false, "show_labels": false,
                }),
                0,
                (640, 360),
            )
            .unwrap();
        let moves = scene
            .elements
            .iter()
            .map(|element| match element {
                VectorElement::StrokePath { path, .. } => path
                    .iter()
                    .filter(|verb| matches!(verb, PathVerb::MoveTo(_)))
                    .count(),
                _ => 0,
            })
            .sum::<usize>();
        assert_eq!(moves, expected_moves, "{items}");
    }
}
