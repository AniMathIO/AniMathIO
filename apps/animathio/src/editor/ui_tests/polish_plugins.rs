//! UI polish for community plugins, Preferences, Export and the launcher.

use std::collections::BTreeMap;
use std::sync::Arc;

use animathio_audio::Mixer;
use animathio_core::{AudioChain, ClipKind, Command, EffectId, EffectInstance, Time, TimeRange};
use animathio_plugin_api::PluginPackage;
use animathio_plugin_host::CommunityStore;
use gpui::TestAppContext;
use gpui_component::WindowExt;

use super::fixtures::*;
use super::{Ui, config_dir, require_plugins};
use crate::editor::actions::NewProject;
use crate::editor::dialogs::{export_summary, middle_truncate};
use crate::editor::{Editor, LeftTab, Project};
use crate::playback::Clock;

const HELLO: &[u8] =
    include_bytes!("../../../../../crates/plugin-host/tests/fixtures/hello-1.0.0.amplugin");
const ID: &str = "com.example.hello";

fn community_ui(cx: &mut TestAppContext) -> (Ui, tempfile::TempDir, CommunityStore) {
    let root = tempfile::tempdir().unwrap();
    let store = CommunityStore::new(
        root.path().join("plugins"),
        root.path().join("settings.json"),
    );
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| {
        e.community.store = Some(store.clone());
        e.community.fetch = std::sync::Arc::new(|| {
            crate::editor::community_plugins::registry::parse(include_bytes!(
                "../community_plugins/fixtures/index.json"
            ))
        });
        e.community.download = std::sync::Arc::new(|_, _| Ok(HELLO.to_vec()));
        e.select_left_tab(LeftTab::Plugins, cx);
        e.refresh_community_inventory(cx);
    });
    (ui, root, store)
}

fn visible(ui: &mut Ui, selector: &'static str) -> bool {
    ui.cx.debug_bounds(selector).is_some()
}

#[gpui::test]
fn trust_caption_is_permanent_and_tabs_follow_the_switch(cx: &mut TestAppContext) {
    let (mut ui, _root, _store) = community_ui(cx);
    // Off: the caption is there, the lone "Installed" pill and the trust card are not.
    assert!(visible(&mut ui, "community-trust-caption"));
    assert!(!visible(&mut ui, "community-installed"));
    assert!(!visible(&mut ui, "community-browse"));
    assert!(!visible(&mut ui, "community-explanation"));
    ui.update(|e, _, cx| e.set_community_plugins_enabled(true, cx).unwrap());
    assert!(visible(&mut ui, "community-trust-caption"));
    assert!(visible(&mut ui, "community-explanation"));
    assert!(visible(&mut ui, "community-installed"));
    ui.update(|e, _, cx| e.browse_community(cx));
    assert!(
        !visible(&mut ui, "community-explanation"),
        "Browse leaves room for the cards; each card carries its own trust line"
    );
    ui.update(|e, _, cx| {
        e.community.browsing = false;
        cx.notify();
    });
    assert!(visible(&mut ui, "community-browse"));
    assert!(
        visible(&mut ui, "community-empty"),
        "empty Installed list says what to do"
    );
    // The card has no "Got it": it never goes away while the switch is on.
    ui.update(|_, _, _| {});
    assert!(visible(&mut ui, "community-explanation"));
    ui.update(|e, _, cx| e.set_community_plugins_enabled(false, cx).unwrap());
    assert!(visible(&mut ui, "community-trust-caption"));
    assert!(!visible(&mut ui, "community-explanation"));
}

#[gpui::test]
fn plus_button_installs_a_plugin_file_on_the_plugins_tab(cx: &mut TestAppContext) {
    let (mut ui, _root, _store) = community_ui(cx);
    assert!(visible(&mut ui, "library-install-plugin"));
    assert!(!visible(&mut ui, "library-import"));
    ui.update(|e, _, cx| e.select_left_tab(LeftTab::Media, cx));
    assert!(visible(&mut ui, "library-import"));
    assert!(!visible(&mut ui, "library-install-plugin"));
}

#[gpui::test]
fn install_shows_sha256_verified_and_remove_asks_first(cx: &mut TestAppContext) {
    let (mut ui, _root, store) = community_ui(cx);
    ui.update(|e, _, cx| {
        e.set_community_plugins_enabled(true, cx).unwrap();
        e.browse_community(cx);
    });
    ui.settle();
    assert!(!visible(&mut ui, "catalog-verified-com.example.hello"));
    let entry = ui.read(|e| e.community.index.as_ref().unwrap().plugins[0].clone());
    ui.update(|e, _, cx| e.install_community_entry(entry, cx));
    ui.settle();
    assert!(visible(&mut ui, "catalog-verified-com.example.hello"));
    ui.update(|e, _, cx| {
        e.community.browsing = false;
        cx.notify();
    });
    assert!(visible(&mut ui, "verified-com.example.hello"));
    assert!(ui.read(|e| e.community.verified.contains(ID)));

    // Remove only asks; the plugin stays installed until the confirmation.
    let remove = ui.cx.debug_bounds("installed-com.example.hello");
    assert!(remove.is_some());
    ui.update(|e, _, cx| {
        e.community.confirm_remove = Some(ID.into());
        cx.notify();
    });
    assert!(visible(&mut ui, "confirm-remove-com.example.hello"));
    assert_eq!(store.list().0.len(), 1);
    ui.update(|e, _, cx| e.remove_community_plugin(ID.into(), cx));
    ui.settle();
    assert_eq!(store.list().0, []);
    assert!(!visible(&mut ui, "confirm-remove-com.example.hello"));
    assert!(!ui.read(|e| e.community.verified.contains(ID)));
}

#[gpui::test]
fn preferences_links_to_the_library_instead_of_a_second_switch(cx: &mut TestAppContext) {
    let (mut ui, _root, store) = community_ui(cx);
    ui.update(|e, _, cx| e.set_community_plugins_enabled(true, cx).unwrap());
    ui.update(|e, window, cx| {
        e.select_left_tab(LeftTab::Media, cx);
        e.open_preferences(window, cx);
    });
    ui.advance_clock(std::time::Duration::from_millis(400));
    assert!(visible(&mut ui, "preferences-community-plugins"));
    assert!(visible(&mut ui, "preferences-export-folder"));
    assert!(visible(&mut ui, "preferences-export-folder-browse"));
    let open = ui.cx.debug_bounds("preferences-open-plugins");
    assert!(open.is_some());
    ui.click(open.unwrap().center(), gpui::Modifiers::default());
    ui.advance_clock(std::time::Duration::from_millis(400));
    assert!(ui.read(|e| e.left_tab == LeftTab::Plugins && e.dock.left_open));
    assert!(!ui.cx.update(WindowExt::has_active_dialog));
    assert!(
        store.settings().enabled,
        "the store stays the only source of truth"
    );
}

#[gpui::test]
fn export_dialog_shows_custom_times_only_for_custom_and_summarises(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(Editor::open_export_dialog);
    ui.advance_clock(std::time::Duration::from_millis(400));
    assert!(visible(&mut ui, "export-output"));
    assert!(!visible(&mut ui, "export-custom-start"));
    assert!(!visible(&mut ui, "export-custom-end"));
    assert!(visible(&mut ui, "export-summary"));
    assert!(
        !visible(&mut ui, "export-output-caption"),
        "the path is shown once, in the File field"
    );
}

#[gpui::test]
fn export_format_selector_picks_webm_and_rewrites_the_extension(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    // Keep the default output folder inside a temp dir, never the real export folder.
    let dir = tempfile::tempdir().unwrap();
    let project_file = dir.path().join("talk.amio");
    ui.update(|editor, _window, _cx| editor.path = Some(project_file.clone()));
    ui.update(Editor::open_export_dialog);
    ui.advance_clock(std::time::Duration::from_millis(400));
    assert!(visible(&mut ui, "export-format-select"));
    assert!(visible(&mut ui, "export-format-note"));

    // The default is MP4; choose WebM in the segmented control.
    let webm = ui.cx.debug_bounds("export-format-webm").unwrap();
    ui.click(webm.center(), gpui::Modifiers::default());
    ui.advance_clock(std::time::Duration::from_millis(400));
    assert!(
        visible(&mut ui, "export-format-select"),
        "choosing a format keeps the dialog open"
    );

    // Export: the job writes `talk.webm`, the extension followed the selector.
    let export_now = ui.cx.debug_bounds("export-now").expect("Export button");
    ui.click(export_now.center(), gpui::Modifiers::default());
    ui.settle();
    let (output, id) = ui.read(|editor| {
        let job = editor.export_queue.jobs().first().expect("a queued export");
        (job.output.clone(), job.id)
    });
    assert_eq!(output, dir.path().join("talk.webm"));
    ui.update(|editor, _window, _cx| editor.export_queue.cancel(id));
}

#[gpui::test]
fn export_dialog_refuses_a_file_name_that_contradicts_the_format(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    let dir = tempfile::tempdir().unwrap();
    for (format, name, needle) in [
        (animathio_export::Format::Mp4, "clip.webm", "ends in .webm"),
        (animathio_export::Format::Webm, "clip.mp4", "ends in .mp4"),
    ] {
        let outcome = ui.update(|editor, _window, cx| {
            editor.submit_export(
                None,
                format,
                "Whole project",
                None,
                None,
                dir.path().join(name),
                false,
                cx,
            )
        });
        assert!(matches!(
            outcome,
            crate::editor::dialogs::Submitted::Invalid
        ));
        ui.read(|editor| {
            assert!(
                editor
                    .last_error
                    .as_deref()
                    .is_some_and(|e| e.contains(needle)),
                "{:?}",
                editor.last_error
            );
        });
        assert!(ui.read(|editor| editor.export_queue.is_empty()));
    }
    // A matching WebM job queues, and is then cancelled before it writes anything.
    let outcome = ui.update(|editor, _window, cx| {
        editor.submit_export(
            None,
            animathio_export::Format::Webm,
            "Whole project",
            None,
            None,
            dir.path().join("clip.webm"),
            false,
            cx,
        )
    });
    assert!(matches!(outcome, crate::editor::dialogs::Submitted::Queued));
    let id = ui.read(|editor| editor.export_queue.jobs()[0].id);
    ui.update(|editor, _window, _cx| editor.export_queue.cancel(id));
}

#[test]
fn export_summary_and_path_truncation() {
    assert_eq!(
        export_summary((640, 360), "30 fps", 3, animathio_export::Format::Webm),
        "640 × 360, 30 fps, no chapters"
    );
    assert_eq!(
        export_summary((640, 360), "30 fps", 3, animathio_export::Format::Mp4),
        "640 × 360, 30 fps, chapters on"
    );
    assert!(
        export_summary((640, 360), "30 fps", 0, animathio_export::Format::Mp4)
            .contains("no chapters")
    );
    assert_eq!(middle_truncate("/a/b.mp4", 40), "/a/b.mp4");
    let long = "/home/someone/Videos/a/very/deep/folder/structure/that/keeps/going/final-cut.mp4";
    let short = middle_truncate(long, 40);
    assert_eq!(short.chars().count(), 40);
    assert!(short.starts_with("/home/"), "{short}");
    assert!(short.ends_with("final-cut.mp4"), "{short}");
    assert!(short.contains('…'));
}

#[gpui::test]
fn launcher_hides_project_only_chrome_and_shows_an_empty_recent_state(cx: &mut TestAppContext) {
    let mut ui = Ui::open(cx, Project::default(), BTreeMap::new());
    ui.update(|e, _, cx| {
        e.recent.paths.clear();
        cx.notify();
    });
    assert!(ui.read(Editor::is_start_screen));
    assert!(
        !visible(&mut ui, "title-export"),
        "no Export button without a project"
    );
    assert!(
        !visible(&mut ui, "status-export-queue"),
        "no export queue on the launcher"
    );
    assert!(visible(&mut ui, "status-renderer"));
    assert!(visible(&mut ui, "start-recent-empty"));
    let names = ui.cx.update(|_, cx| {
        gpui_component::global_state::GlobalState::global(cx)
            .app_menus()
            .iter()
            .map(|m| m.name.to_string())
            .collect::<Vec<_>>()
    });
    for hidden in ["Playback", "Tools", "Clip"] {
        assert!(!names.iter().any(|n| n == hidden), "{hidden} in {names:?}");
    }
    assert!(names.iter().any(|n| n == "File"));
    // Starting a project brings the chrome back.
    ui.update(|e, _, cx| e.new_project(cx));
    assert!(!ui.read(Editor::is_start_screen));
    assert!(visible(&mut ui, "title-export"));
    assert!(visible(&mut ui, "status-export-queue"));
    let names = ui.cx.update(|_, cx| {
        gpui_component::global_state::GlobalState::global(cx)
            .app_menus()
            .iter()
            .map(|m| m.name.to_string())
            .collect::<Vec<_>>()
    });
    for shown in ["Playback", "Tools", "Clip"] {
        assert!(names.iter().any(|n| n == shown), "{shown} in {names:?}");
    }
}

#[gpui::test]
fn window_title_names_the_project_and_marks_unsaved_changes(cx: &mut TestAppContext) {
    let ui = Ui::open(cx, Project::default(), BTreeMap::new());
    assert_eq!(ui.read(Editor::window_title), "AniMathIO");
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    assert_eq!(
        ui.read(Editor::window_title),
        "Untitled project - AniMathIO"
    );
    ui.update(|e, _, cx| {
        e.dirty = true;
        cx.notify();
    });
    assert_eq!(
        ui.read(Editor::window_title),
        "• Untitled project - AniMathIO"
    );
    let path = config_dir().join("polish-title.amio");
    crate::editor::support::write_project(&path, &Project::default(), &BTreeMap::new()).unwrap();
    ui.update(|e, _, cx| {
        assert!(e.try_open_path(&path, cx));
        assert!(!e.dirty);
    });
    assert_eq!(ui.read(Editor::window_title), "polish-title - AniMathIO");
    std::fs::remove_file(path).ok();
}

#[gpui::test]
fn status_bar_names_the_renderer_actually_used(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| {
        e.preview.gpu = false;
        cx.notify();
    });
    assert_eq!(ui.read(Editor::renderer_label), "CPU preview");
    ui.update(|e, _, cx| {
        e.preview.gpu = true;
        cx.notify();
    });
    let label = ui.read(Editor::renderer_label);
    assert!(
        matches!(label, "GPU preview" | "CPU preview (fallback)"),
        "{label}"
    );
}

/// A Browse card keeps the plugin id, licence and ABI behind its Details link, which toggles.
#[gpui::test]
fn browse_card_details_disclose_and_hide_the_plugin_facts(cx: &mut TestAppContext) {
    let (mut ui, _root, _store) = community_ui(cx);
    ui.update(|e, _, cx| {
        e.set_community_plugins_enabled(true, cx).unwrap();
        e.browse_community(cx);
    });
    ui.settle();
    let facts = "plugin-id-com.example.hello";
    assert!(!visible(&mut ui, facts), "closed by default");
    let link = ui
        .cx
        .debug_bounds("details-catalog:com.example.hello")
        .expect("the Details link");
    ui.click(link.center(), gpui::Modifiers::default());
    assert!(visible(&mut ui, facts), "Details opens the plugin facts");
    assert!(visible(&mut ui, "plugin-license-com.example.hello"));
    assert!(visible(&mut ui, "plugin-abi-com.example.hello"));
    // The open card grows below the Library body's fold: scroll it into view, as a user would.
    let body = ui
        .cx
        .debug_bounds("library-body")
        .expect("the Library body");
    ui.cx.simulate_event(gpui::ScrollEvent {
        position: body.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-400.))),
        ..Default::default()
    });
    ui.settle();
    let link = ui
        .cx
        .debug_bounds("details-catalog:com.example.hello")
        .expect("the link stays");
    ui.click(link.center(), gpui::Modifiers::default());
    assert!(!visible(&mut ui, facts), "Hide details closes them again");
}

/// The launcher shows no canvas yet, so the status bar hides the placeholder
/// resolution/duration chips until a project exists.
#[gpui::test]
fn launcher_status_bar_hides_placeholder_resolution_and_duration(cx: &mut TestAppContext) {
    let mut ui = Ui::open(cx, Project::default(), BTreeMap::new());
    assert!(ui.read(Editor::is_start_screen));
    ui.read(|editor| assert!(!editor.status_summary_visible()));
    ui.dispatch(NewProject);
    ui.press("enter");
    ui.read(|editor| {
        assert!(!editor.is_start_screen());
        assert!(editor.status_summary_visible());
        assert_eq!(editor.project.tracks().len(), 1);
    });
}

/// Community plugins start off. Enabling one exposes its clip source and lets the preview
/// render it. Turning the switch off replaces the rendered clip with a placeholder.
#[gpui::test]
fn community_plugins_load_and_unload_without_restart(cx: &mut TestAppContext) {
    const HELLO: &[u8] =
        include_bytes!("../../../../../crates/plugin-host/tests/fixtures/hello-1.0.0.amplugin");
    const ID: &str = "com.example.hello";
    if !require_plugins("community_plugins_load_and_unload_without_restart") {
        return;
    }
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.settle();
    // A private store of this test: the shared one in the test config directory also holds the
    // `community-plugins.json` that other scenarios (Preferences) write concurrently.
    let root = tempfile::tempdir().unwrap();
    let store = animathio_plugin_host::CommunityStore::new(
        root.path().join("plugins"),
        root.path().join("community-plugins.json"),
    );
    ui.update(|editor, _, _| editor.community.store = Some(store.clone()));
    let has_hello = |ui: &Ui| ui.read(|editor| editor.plugins.iter().any(|plugin| plugin.id == ID));
    assert!(!store.settings().enabled, "community plugins start off");
    assert!(!has_hello(&ui));

    // Installed plugins start disabled, so even a switched-on editor does not load them.
    store.install_bytes(HELLO, None, false).unwrap();
    ui.update(|editor, _, cx| editor.set_community_plugins_enabled(true, cx).unwrap());
    assert!(!has_hello(&ui), "installed but not enabled");

    ui.update(|editor, _, cx| editor.toggle_community_plugin(ID.into(), true, cx));
    assert!(has_hello(&ui), "enabled plugin is loaded without a restart");
    ui.update(|editor, _, cx| {
        editor.add_plugin_clip_by_id(ID, cx);
        assert!(editor.error.is_none(), "{:?}", editor.error);
        assert!(!editor.missing_plugin_ids().contains(ID));
        assert_community_preview(editor, true);
    });

    // Switching the global switch off unloads it, and the project is unaffected.
    let before = ui.read(|editor| editor.project.clone());
    ui.update(|editor, _, cx| {
        let generation = editor.request_generation;
        editor.set_community_plugins_enabled(false, cx).unwrap();
        assert_ne!(
            editor.request_generation, generation,
            "unload requests a frame"
        );
        assert!(editor.missing_plugin_ids().contains(ID));
        assert_community_preview(editor, false);
    });
    assert!(!has_hello(&ui), "switch off unloads community plugins");
    assert_eq!(ui.read(|editor| editor.project.clone()), before);
    ui.read(|editor| {
        let host = editor.plugin_host.as_ref().expect("plugin host");
        assert!(host.get_plugin(ID).is_none());
    });

    store.uninstall(ID).unwrap();
}

fn assert_community_preview(editor: &Editor, loaded: bool) {
    let preview = editor.worker.wait_for_result(editor.request_generation);
    assert!(preview.effect_error.is_none(), "{:?}", preview.effect_error);
    assert!(preview.image.is_ok(), "{:?}", preview.image.err());
    let kind = &preview
        .scene
        .clips
        .iter()
        .find(|clip| Some(clip.id) == editor.selected)
        .expect("added clip is active at the playhead")
        .kind;
    if loaded {
        assert!(matches!(kind, ClipKind::Vector(_)), "{kind:?}");
    } else {
        assert!(
            matches!(kind, ClipKind::Error(error) if error.contains("plugin not found")),
            "{kind:?}"
        );
    }
}

/// Exercise mixer replacement through the real editor without opening an output device.
#[gpui::test]
fn community_audio_effects_refresh_the_playback_mixer(cx: &mut TestAppContext) {
    const AUDIO_ID: &str = "com.example.audio-basic";
    if !require_plugins("community_audio_effects_refresh_the_playback_mixer") {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let store = CommunityStore::new(
        root.path().join("plugins"),
        root.path().join("settings.json"),
    );
    let mut package = PluginPackage::from_bytes(include_bytes!(
        "../../../../../crates/plugin-host/tests/fixtures/audio-basic-0.2.2.amplugin"
    ))
    .unwrap();
    // Re-use the frozen gain component under a community id, avoiding the built-in package.
    package.manifest.id = AUDIO_ID.into();
    store
        .install_bytes(&package.create_bytes().unwrap(), None, false)
        .unwrap();
    let (mut project, assets) = voice_pair();
    Command::AddAudioEffect {
        chain: AudioChain::Clip(PAIR_FIRST),
        index: 0,
        effect: EffectInstance::new(
            EffectId(1),
            AUDIO_ID,
            "gain",
            BTreeMap::from([("gain_db".into(), -6.0)]),
        ),
    }
    .apply(&mut project)
    .unwrap();
    let mut ui = Ui::open(cx, project, assets);
    let slot = std::rc::Rc::new(std::cell::RefCell::new(ui.read(|editor| {
        Arc::new(
            editor
                .playback_effects
                .prepare(&editor.pool, 48_000, 1)
                .unwrap(),
        )
    })));
    ui.update(|editor, _, cx| {
        editor.clock = Box::new(TestMixerClock(slot.clone()));
        editor.community.store = Some(store.clone());
        editor.set_community_plugins_enabled(true, cx).unwrap();
    });
    let range = TimeRange::new(Time(0), Time(50 * MS)).unwrap();
    let mix = |ui: &Ui| ui.read(|editor| slot.borrow().mix(&editor.project, range).unwrap());
    let dry = mix(&ui);
    assert!(dry.iter().any(|sample| sample.abs() > 0.1));
    ui.update(|editor, _, cx| editor.toggle_community_plugin(AUDIO_ID.into(), true, cx));
    ui.read(|editor| assert!(editor.error.is_none(), "{:?}", editor.error));
    let wet = mix(&ui);
    let gain = 10.0_f32.powf(-6.0 / 20.0);
    assert_eq!(wet.len(), dry.len());
    assert!(
        wet.iter()
            .zip(&dry)
            .all(|(wet, dry)| (wet - dry * gain).abs() < 1e-6)
    );

    // The same range has now been cached. Disabling must discard its processed blocks.
    ui.update(|editor, _, cx| editor.toggle_community_plugin(AUDIO_ID.into(), false, cx));
    assert_eq!(mix(&ui), dry);
}

struct TestMixerClock(std::rc::Rc<std::cell::RefCell<Arc<Mixer>>>);

impl Clock for TestMixerClock {
    fn now(&self) -> Time {
        Time(0)
    }
    fn play(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn pause(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn seek(&mut self, _: Time) -> Result<(), String> {
        Ok(())
    }
    fn mixer_format(&self) -> Option<(u32, u16)> {
        Some((48_000, 1))
    }
    fn set_mixer(&mut self, mixer: Arc<Mixer>) {
        *self.0.borrow_mut() = mixer;
    }
}

#[gpui::test]
fn launcher_actions_and_canvas_cards_are_in_view_at_720p(cx: &mut TestAppContext) {
    let mut ui = Ui::open(cx, Project::default(), BTreeMap::new());
    assert!(ui.read(Editor::is_start_screen));
    ui.cx
        .simulate_resize(gpui::size(gpui::px(1280.), gpui::px(720.)));
    ui.settle();
    for selector in ["start-new", "start-open", "start-import", "canvas-preset-0"] {
        let bounds = ui
            .cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is rendered"));
        // Above the status bar and the window edge, without scrolling the launcher.
        assert!(bounds.bottom() <= gpui::px(680.), "{selector}: {bounds:?}");
        assert!(bounds.right() <= gpui::px(1280.), "{selector}: {bounds:?}");
        assert!(bounds.size.width > gpui::px(40.), "{selector}: {bounds:?}");
    }
    // The two imports live in the Import… menu.
    let import = ui.cx.debug_bounds("start-import").unwrap();
    ui.click(import.center(), gpui::Modifiers::default());
    ui.advance_clock(std::time::Duration::from_millis(400));
    for selector in ["start-v1", "start-manim"] {
        let bounds = ui
            .cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is in the menu"));
        assert!(bounds.bottom() <= gpui::px(720.), "{selector}: {bounds:?}");
        assert!(bounds.size.width > gpui::px(40.), "{selector}: {bounds:?}");
    }
}
