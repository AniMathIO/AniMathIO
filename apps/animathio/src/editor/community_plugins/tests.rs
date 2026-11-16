//! Offline scenarios through the real editor and the same handlers used by its controls.
use super::*;
use crate::editor::ui_tests::Ui;
use gpui::TestAppContext;
use std::sync::atomic::{AtomicUsize, Ordering};
const HELLO: &[u8] =
    include_bytes!("../../../../../crates/plugin-host/tests/fixtures/hello-1.0.0.amplugin");
const ID: &str = "com.example.hello";
fn index() -> Index {
    registry::parse(include_bytes!("fixtures/index.json")).unwrap()
}
fn setup(cx: &mut TestAppContext) -> (Ui, tempfile::TempDir, CommunityStore, Arc<AtomicUsize>) {
    let root = tempfile::tempdir().unwrap();
    let store = CommunityStore::new(
        root.path().join("plugins"),
        root.path().join("settings.json"),
    );
    let sample = animathio_render::sample::build().unwrap();
    let mut ui = Ui::open(cx, sample.project, sample.assets);
    let calls = Arc::new(AtomicUsize::new(0));
    ui.update(|e, _, cx| {
        e.community.store = Some(store.clone());
        let calls = calls.clone();
        e.community.fetch = Arc::new(move || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(index())
        });
        e.community.download = Arc::new(|_, _| Ok(HELLO.to_vec()));
        e.select_left_tab(LeftTab::Plugins, cx);
        e.refresh_community_inventory(cx);
    });
    (ui, root, store, calls)
}
#[gpui::test]
fn switch_off_hides_browse_and_never_fetches(cx: &mut TestAppContext) {
    let (mut ui, _root, _store, calls) = setup(cx);
    assert!(ui.cx.debug_bounds("community-browse").is_none());
    ui.update(|e, _, cx| e.browse_community(cx));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(!ui.read(|e| e.community.browsing));
    ui.update(|e, _, cx| e.set_community_plugins_enabled(true, cx).unwrap());
    assert!(ui.cx.debug_bounds("community-browse").is_some());
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "enabling never fetches automatically"
    );
    ui.update(|e, _, cx| e.browse_community(cx));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    ui.update(|e, _, cx| e.set_community_plugins_enabled(false, cx).unwrap());
    assert!(ui.cx.debug_bounds("community-catalog").is_none());
}
#[gpui::test]
fn install_verify_enable_render_and_remove_live(cx: &mut TestAppContext) {
    let (mut ui, _root, store, _) = setup(cx);
    ui.update(|e, _, cx| {
        e.set_community_plugins_enabled(true, cx).unwrap();
        e.browse_community(cx);
    });
    let entry = ui.read(|e| e.community.index.as_ref().unwrap().plugins[0].clone());
    // A wrong checksum must leave no installed copy or enabled flag.
    let mut corrupt = entry.clone();
    corrupt.sha256 = "0".repeat(64);
    ui.update(|e, _, cx| e.install_community_entry(corrupt, cx));
    assert!(ui.read(|e| {
        e.community
            .error
            .as_ref()
            .unwrap()
            .contains("sha-256 mismatch")
    }));
    assert!(store.list().0.is_empty());
    ui.update(|e, _, cx| {
        e.refresh_community_inventory(cx);
        e.install_community_entry(entry, cx);
        assert!(e.community.busy);
        assert!(
            e.set_community_plugins_enabled(false, cx).is_err(),
            "serialize settings writes"
        );
    });
    assert!(ui.read(|e| e.community.error.is_none()));
    assert_eq!(
        ui.read(|e| e.community.installed.len()),
        1,
        "stale refresh cannot overwrite install result"
    );
    assert!(store.settings().enabled);
    assert_eq!(store.list().0.len(), 1);
    assert!(
        !store.list().0[0].enabled,
        "new installs need an explicit Enable"
    );
    assert!(ui.read(|e| e.plugin_host.as_ref().unwrap().get_plugin(ID).is_none()));
    ui.update(|e, _, cx| e.toggle_community_plugin(ID.into(), true, cx));
    assert!(ui.read(|e| e.plugins.iter().any(|p| p.id == ID)));
    ui.update(|e, _, cx| {
        e.add_plugin_clip_by_id(ID, cx);
        let scene = evaluate(&e.project, Time(0));
        let resolved = e.plugin_host.as_mut().unwrap().resolve(&scene, (320, 180));
        assert!(
            resolved
                .clips
                .iter()
                .any(|c| matches!(c.kind, ClipKind::Vector(_))),
            "enabled plugin renders its clip"
        );
    });
    ui.update(|e, _, cx| e.remove_community_plugin(ID.into(), cx));
    assert!(store.list().0.is_empty());
    assert!(ui.read(|e| e.plugin_host.as_ref().unwrap().get_plugin(ID).is_none()));
    assert!(
        ui.read(|e| e.missing_plugin_ids().contains(ID)),
        "project retains its missing plugin reference"
    );
}
#[gpui::test]
fn incompatible_abi_is_visible_but_cannot_install(cx: &mut TestAppContext) {
    let (mut ui, _root, store, _) = setup(cx);
    let downloads = Arc::new(AtomicUsize::new(0));
    let mut incompatible = index();
    incompatible.plugins[0].min_abi = "9.0.0".into();
    ui.update(|e, _, cx| {
        e.community.fetch = Arc::new(move || Ok(incompatible.clone()));
        let downloads = downloads.clone();
        e.community.download = Arc::new(move |_, _| {
            downloads.fetch_add(1, Ordering::SeqCst);
            Ok(HELLO.to_vec())
        });
        e.set_community_plugins_enabled(true, cx).unwrap();
        e.browse_community(cx);
    });
    assert!(ui.cx.debug_bounds("install-com.example.hello").is_some());
    let entry = ui.read(|e| e.community.index.as_ref().unwrap().plugins[0].clone());
    assert!(entry.compatible().is_err());
    ui.update(|e, _, cx| e.install_community_entry(entry, cx));
    assert_eq!(downloads.load(Ordering::SeqCst), 0);
    assert!(store.list().0.is_empty());
}
#[gpui::test]
fn network_error_has_retry_and_keeps_installed_plugins(cx: &mut TestAppContext) {
    let (mut ui, _root, store, _) = setup(cx);
    store.install_bytes(HELLO, None, false).unwrap();
    ui.update(|e, _, cx| {
        e.community.fetch = Arc::new(|| Err("offline".into()));
        e.set_community_plugins_enabled(true, cx).unwrap();
        e.browse_community(cx);
    });
    assert!(
        ui.read(
            |e| !e.community.fetching && e.community.error.as_ref().unwrap().contains("offline")
        )
    );
    assert!(ui.cx.debug_bounds("community-error").is_some());
    assert_eq!(store.list().0.len(), 1);
    ui.update(|e, _, cx| {
        e.community.fetch = Arc::new(|| Ok(index()));
        e.browse_community(cx);
    });
    assert!(ui.read(|e| e.community.error.is_none() && e.community.index.is_some()));
}
#[test]
fn index_contract_checks_schema_size_metadata_and_numeric_versions() {
    assert!(registry::parse(br#"{"schema_version":2,"plugins":[],"delisted":[]}"#).is_err());
    let mut entry = index().plugins.remove(0);
    entry.version = "1.10.0".into();
    assert!(entry.newer_than("1.9.0"));
    entry.version = "1.0.0".into();
    let root = tempfile::tempdir().unwrap();
    let store = CommunityStore::new(root.path().join("plugins"), root.path().join("settings"));
    let download: Downloader = Arc::new(|_, _| Ok(HELLO.to_vec()));
    entry.size -= 1;
    assert!(
        entry
            .install(&store, &download, false)
            .unwrap_err()
            .contains("size mismatch")
    );
    entry.size += 1;
    entry.id = "com.example.other".into();
    assert!(
        entry
            .install(&store, &download, false)
            .unwrap_err()
            .contains("metadata mismatch")
    );
    assert!(store.list().0.is_empty());
}
