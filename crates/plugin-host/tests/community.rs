//! Community plugins end to end on the host: the trust gate, hot load/unload and rendering.
//! Uses the frozen `hello` fixture (`com.example.hello`, a filled circle), so no plugin build
//! is needed.

use std::collections::BTreeMap;

use animathio_core::{ClipId, ClipKind, Scene, SceneClip, Time, TrackId};
use animathio_plugin_host::{CommunityStore, PluginDirectory, PluginHost};

const HELLO: &[u8] = include_bytes!("fixtures/hello-1.0.0.amplugin");
const ID: &str = "com.example.hello";

struct Fixture {
    _root: tempfile::TempDir,
    store: CommunityStore,
    host: PluginHost,
}

fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let store = CommunityStore::new(
        root.path().join("plugins"),
        root.path().join("config/community-plugins.json"),
    );
    let host = PluginHost::new(root.path().join("cache")).unwrap();
    Fixture {
        _root: root,
        store,
        host,
    }
}

fn hello_scene() -> Scene {
    Scene {
        canvas: (0, 0),
        background: [0; 4],
        clips: vec![SceneClip {
            track: TrackId(0),
            id: ClipId(1),
            kind: ClipKind::Plugin {
                plugin_id: ID.to_owned(),
                params: serde_json::Value::Null,
            },
            source_time: Time(0),
            fit: animathio_core::Fit::Stretch,
            transform: animathio_core::Transform::default(),
            params: BTreeMap::new(),
            effects: Vec::new(),
            transition: None,
        }],
    }
}

fn renders(host: &mut PluginHost) -> bool {
    matches!(
        host.resolve(&hello_scene(), (320, 180)).clips[0].kind,
        ClipKind::Vector(_)
    )
}

#[test]
fn installed_plugins_stay_unloaded_until_switch_and_flag_are_on() {
    let mut fx = fixture();
    fx.store.install_bytes(HELLO, None, false).unwrap();

    // Switch off, plugin not enabled.
    let report = fx.host.sync_community_plugins(&fx.store);
    assert!(!report.changed() && report.problems.is_empty());
    assert!(fx.host.loaded_plugin_ids().is_empty());
    assert!(!renders(&mut fx.host));

    // Plugin enabled but the global switch is still off: the folder is not loaded.
    fx.store.set_plugin_enabled(ID, true).unwrap();
    assert!(!fx.host.sync_community_plugins(&fx.store).changed());
    assert!(!renders(&mut fx.host));

    // Switch on: the enabled plugin loads, with no restart, and its clip renders.
    fx.store.set_enabled(true).unwrap();
    let report = fx.host.sync_community_plugins(&fx.store);
    assert_eq!(report.loaded, [ID]);
    assert!(fx.host.is_community_plugin(ID));
    assert!(renders(&mut fx.host));

    // Syncing again is a no-op.
    assert!(!fx.host.sync_community_plugins(&fx.store).changed());

    // Disable it: it unloads and the clip falls back to the missing-plugin error clip.
    fx.store.set_plugin_enabled(ID, false).unwrap();
    let report = fx.host.sync_community_plugins(&fx.store);
    assert_eq!(report.unloaded, [ID]);
    assert!(!fx.host.is_community_plugin(ID));
    assert!(!renders(&mut fx.host));

    // Re-enable then switch off globally: unloads everything again.
    fx.store.set_plugin_enabled(ID, true).unwrap();
    assert_eq!(fx.host.sync_community_plugins(&fx.store).loaded, [ID]);
    fx.store.set_enabled(false).unwrap();
    assert_eq!(fx.host.sync_community_plugins(&fx.store).unloaded, [ID]);
    assert!(fx.host.loaded_plugin_ids().is_empty());
}

#[test]
fn uninstalling_an_enabled_plugin_unloads_it_on_sync() {
    let mut fx = fixture();
    fx.store.install_bytes(HELLO, None, false).unwrap();
    fx.store.set_plugin_enabled(ID, true).unwrap();
    fx.store.set_enabled(true).unwrap();
    assert_eq!(fx.host.sync_community_plugins(&fx.store).loaded, [ID]);
    fx.store.uninstall(ID).unwrap();
    assert_eq!(fx.host.sync_community_plugins(&fx.store).unloaded, [ID]);
    assert!(!renders(&mut fx.host));
}

#[test]
fn the_community_folder_is_never_loaded_wholesale() {
    let mut fx = fixture();
    fx.store.install_bytes(HELLO, None, false).unwrap();
    let dir = PluginDirectory {
        path: fx.store.dir().to_owned(),
        allow_reserved_ids: false,
        community: true,
    };
    assert!(fx.host.load_plugin_directory(&dir).unwrap().is_empty());
    assert!(fx.host.loaded_plugin_ids().is_empty());
}

#[test]
fn unload_plugin_removes_a_loaded_plugin() {
    let mut fx = fixture();
    let path = fx.store.install_bytes(HELLO, None, false).unwrap().path;
    assert_eq!(fx.host.load_plugin_file(path).unwrap(), ID);
    assert!(renders(&mut fx.host));
    assert!(fx.host.unload_plugin(ID));
    assert!(!fx.host.unload_plugin(ID));
    assert!(!renders(&mut fx.host));
}

#[test]
fn a_broken_enabled_package_is_reported_not_loaded() {
    let mut fx = fixture();
    fx.store.install_bytes(HELLO, None, false).unwrap();
    fx.store.set_plugin_enabled(ID, true).unwrap();
    fx.store.set_enabled(true).unwrap();
    // Truncated after install: the folder entry is unreadable, nothing loads, nothing panics.
    std::fs::write(fx.store.dir().join(format!("{ID}.amplugin")), b"junk").unwrap();
    let report = fx.host.sync_community_plugins(&fx.store);
    assert!(report.loaded.is_empty());
    assert!(!report.problems.is_empty());
    assert!(fx.host.loaded_plugin_ids().is_empty());
}
