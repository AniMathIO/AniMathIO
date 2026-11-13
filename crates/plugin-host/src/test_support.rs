//! Shared integration-test setup. Builds real WASI components on Linux and Windows.
use crate::{AudioEffectHandle, EffectHandle, PluginHost};
use animathio_core::Scene;
use animathio_plugin_api::{PluginManifestToml, PluginPackage};
use std::{
    path::Path,
    process::Command,
    sync::{Mutex, OnceLock},
};

fn packages() -> &'static (tempfile::TempDir, Vec<PluginPackage>) {
    static PACKAGES: OnceLock<(tempfile::TempDir, Vec<PluginPackage>)> = OnceLock::new();
    PACKAGES.get_or_init(|| {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let target = root.join("target/plugin-tests");
        let mut packages = Vec::new();
        for name in [
            "text",
            "shapes",
            "latex",
            "effects-basic",
            "audio-basic",
        ] {
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
                    "--target",
                    "wasm32-wasip2",
                    "--release",
                    "--locked",
                    "--manifest-path",
                ])
                .arg(root.join("plugins").join(name).join("Cargo.toml"))
                .env("CARGO_TARGET_DIR", &target)
                .env("CARGO_BUILD_JOBS", "3")
                .env("CARGO_PROFILE_DEV_DEBUG", "line-tables-only")
                .status()
                .expect("build first-party test components");
            assert!(
                status.success(),
                "building {name}; install the wasm32-wasip2 target"
            );
            let manifest = PluginManifestToml::parse(
                &std::fs::read_to_string(root.join("plugins").join(name).join("plugin.toml"))
                    .unwrap(),
            )
            .unwrap();
            let wasm = std::fs::read(
                target
                    .join("wasm32-wasip2/release")
                    .join(format!("{}.wasm", name.replace('-', "_"))),
            )
            .unwrap();
            packages.push(PluginPackage::new(manifest, wasm).unwrap());
        }
        (tempfile::tempdir().unwrap(), packages)
    })
}

/// The built first-party packages, for tests that need custom host limits.
pub fn packages_for_tests() -> &'static [PluginPackage] {
    &packages().1
}

/// A host with the first-party text, shapes, LaTeX and effects plugins, built once per test process.
pub fn host() -> PluginHost {
    let (cache, packages) = packages();
    let mut host = PluginHost::new(cache.path()).unwrap();
    for package in packages {
        host.load_plugin_package(package.clone()).unwrap();
    }
    host
}

/// A renderer handle onto the first-party effect plugins, shared by the test process.
pub fn effect_processor() -> EffectHandle {
    static HANDLE: OnceLock<EffectHandle> = OnceLock::new();
    HANDLE.get_or_init(|| host().effect_processor()).clone()
}

/// A mixer handle onto the first-party audio-effect plugins, shared by the test process.
pub fn audio_effect_processor() -> AudioEffectHandle {
    static HANDLE: OnceLock<AudioEffectHandle> = OnceLock::new();
    HANDLE
        .get_or_init(|| host().audio_effect_processor())
        .clone()
}

/// Resolve through the actual sandbox, reusing a host between test frames.
pub fn resolve(scene: &Scene, size: (u32, u32)) -> Scene {
    static HOST: OnceLock<Mutex<PluginHost>> = OnceLock::new();
    HOST.get_or_init(|| Mutex::new(host()))
        .lock()
        .unwrap()
        .resolve(scene, size)
}

/// Explicit bundle directory for subprocess integration tests.
pub fn plugin_dir() -> &'static Path {
    static DIR: OnceLock<tempfile::TempDir> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        for package in &packages().1 {
            let file =
                std::fs::File::create(dir.path().join(format!("{}.amplugin", package.manifest.id)))
                    .unwrap();
            package.write(file).unwrap();
        }
        dir
    })
    .path()
}
