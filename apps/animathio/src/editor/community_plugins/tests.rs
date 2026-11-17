//! Offline scenarios through the real editor and the same handlers used by its controls.
use super::{Downloader, Index, registry};
use crate::editor::ui_tests::Ui;
use crate::editor::{ClipKind, LeftTab, Time, evaluate};
use animathio_plugin_host::CommunityStore;
use gpui::TestAppContext;
use std::sync::Arc;
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
    assert_eq!(store.list().0, []);
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
        ui.cx
            .debug_bounds("catalog-enable-com.example.hello")
            .is_some(),
        "Browse offers Enable after installation"
    );
    assert!(
        !store.list().0[0].enabled,
        "new installs need an explicit Enable"
    );
    assert!(ui.read(|e| e.plugin_host.as_ref().unwrap().get_plugin(ID).is_none()));
    ui.update(|e, _, cx| e.toggle_community_plugin(ID.into(), true, cx));
    assert!(ui.read(|e| e.plugins.iter().any(|p| p.id == ID)));
    ui.update(|e, _, cx| {
        e.add_plugin_clip_by_id(ID, cx);
        let hello_clip = e.selected.expect("new hello clip selected");
        let scene = evaluate(&e.project, Time(0));
        let resolved = e.plugin_host.as_mut().unwrap().resolve(&scene, (320, 180));
        assert!(
            resolved
                .clips
                .iter()
                .any(|c| c.id == hello_clip && matches!(c.kind, ClipKind::Vector(_))),
            "enabled plugin renders its clip"
        );
    });
    ui.update(|e, _, cx| e.remove_community_plugin(ID.into(), cx));
    assert_eq!(store.list().0, []);
    assert!(ui.read(|e| e.plugin_host.as_ref().unwrap().get_plugin(ID).is_none()));
    ui.update(|e, _, _| {
        let hello_clip = e.selected.expect("hello clip retained");
        let scene = evaluate(&e.project, Time(0));
        let resolved = e.plugin_host.as_mut().unwrap().resolve(&scene, (320, 180));
        let clip = resolved
            .clips
            .iter()
            .find(|c| c.id == hello_clip)
            .expect("missing clip retained");
        assert!(
            !matches!(clip.kind, ClipKind::Vector(_)),
            "unloading invalidates rendered plugin output"
        );
    });
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
    assert_eq!(store.list().0, []);
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
    assert_eq!(store.list().0, []);
}
#[test]
fn malformed_index_json_is_an_error_not_a_panic() {
    for bytes in [
        &b""[..],
        b"{",
        b"null",
        b"[]",
        br#"{"schema_version":"1"}"#,
        br#"{"schema_version":1}"#,
        br#"{"schema_version":1,"plugins":[{"id":1}],"delisted":[]}"#,
        br#"{"schema_version":1,"plugins":[],"delisted":[{"id":"x"}]}"#,
    ] {
        assert!(registry::parse(bytes).is_err(), "{bytes:?}");
    }
    // Unknown extra fields are ignored (registry contract, schema 1).
    let index = registry::parse(
        br#"{"schema_version":1,"plugins":[],"delisted":[],"$schema":"x","generated":"y"}"#,
    )
    .unwrap();
    assert!(index.plugins.is_empty());
}
#[test]
fn a_download_with_the_wrong_hash_is_rejected_before_it_is_parsed() {
    let entry = index().plugins.remove(0);
    let root = tempfile::tempdir().unwrap();
    let store = CommunityStore::new(root.path().join("plugins"), root.path().join("settings"));
    // Right size, wrong bytes: not even a zip, so a parse-first install would report an
    // archive error instead of the checksum.
    let size = usize::try_from(entry.size).unwrap();
    let download: Downloader = Arc::new(move |_, _| Ok(vec![0xAB; size]));
    let error = entry.install(&store, &download, false).unwrap_err();
    assert!(error.contains("sha-256 mismatch"), "{error}");
    assert_eq!(store.list().0, []);
}
/// Live check against the real registry: Browse, install with SHA-256 verification, Enable and
/// render a frame of the plugin's clip. Needs network access, so it only runs on request:
/// `cargo test -p animathio --locked live_registry -- --ignored`
/// (`ANIMATHIO_LIVE_FRAME=<path.png>` also saves the frame).
#[gpui::test]
#[ignore = "network: fetches the live community index and downloads a plugin"]
fn live_registry_install_enable_and_render(cx: &mut TestAppContext) {
    const LIVE: &str = "io.github.animathio.code-snippets";
    let (mut ui, _root, store, _) = setup(cx);
    ui.update(|e, _, cx| {
        e.community.fetch = registry::fetcher();
        e.community.download = registry::downloader();
        e.set_community_plugins_enabled(true, cx).unwrap();
        e.browse_community(cx);
    });
    ui.settle();
    let entry = ui.read(|e| {
        assert_eq!(e.community.error, None);
        e.community
            .index
            .as_ref()
            .expect("live index")
            .plugins
            .iter()
            .find(|p| p.id == LIVE)
            .cloned()
            .expect("code-snippets listed")
    });
    assert!(entry.compatible().is_ok(), "{:?}", entry.compatible());
    ui.update(|e, _, cx| e.install_community_entry(entry.clone(), cx));
    ui.settle();
    assert_eq!(ui.read(|e| e.community.error.clone()), None);
    let installed = store.list().0;
    assert_eq!(installed.len(), 1);
    assert_eq!(installed[0].version, entry.version);
    ui.update(|e, _, cx| e.toggle_community_plugin(LIVE.into(), true, cx));
    ui.settle();
    assert!(ui.read(|e| e.plugins.iter().any(|p| p.id == LIVE)));
    ui.update(|e, _, cx| {
        e.add_plugin_clip_by_id(LIVE, cx);
        let clip = e.selected.expect("new clip selected");
        let time = e.current_render_time();
        let size = (640, 360);
        let mut scene = e
            .plugin_host
            .as_mut()
            .unwrap()
            .resolve(&evaluate(&e.project, time), size);
        scene.clips.retain(|c| c.id == clip);
        assert_eq!(scene.clips.len(), 1);
        assert!(
            !matches!(scene.clips[0].kind, ClipKind::Plugin { .. }),
            "enabled community plugin resolves its clip: {:?}",
            scene.clips[0].kind
        );
        scene.background = [0, 0, 0, 0];
        let mut ctx =
            animathio_render::RenderCtx::from_assets(std::collections::BTreeMap::default())
                .unwrap();
        let frame = animathio_render::render(&scene, &mut ctx, size).unwrap();
        let drawn = frame.pixels.chunks(4).filter(|p| p[3] != 0).count();
        assert!(drawn > 100, "plugin clip drew {drawn} pixels");
        if let Some(path) = std::env::var_os("ANIMATHIO_LIVE_FRAME") {
            frame.save_png(path).unwrap();
        }
    });
}
