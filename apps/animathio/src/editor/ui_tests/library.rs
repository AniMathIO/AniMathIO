//! Library tabs, asset import and adding media and elements.

use super::fixtures::{sample, voice_and_music, wav};
use super::helpers::{bounds_of, clear_error, clip_count, last_error, search_library};
use super::{
    BTreeMap, Command, LeftTab, Project, TestAppContext, Time, Ui, actions, require_plugins,
};
use crate::editor::actions::{Redo, ShowEffects, Undo};
use crate::editor::support::project_size;

#[gpui::test]
fn library_import_at_an_occupied_playhead_leaves_the_timeline_alone(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let before = project.clone();
    let mut ui = Ui::open(cx, project, assets);
    let bytes = wav(&[(0.25, 1200)]);
    let path = super::config_dir().join("library-import-occupied.wav");
    std::fs::write(&path, &bytes).unwrap();
    // The Import media picker uses this library-only path, even at time zero.
    ui.update(|editor, _, cx| editor.import_paths(&[path], None, cx));
    ui.read(|editor| {
        assert_eq!(editor.project.tracks(), before.tracks());
        assert!(
            editor
                .project
                .assets()
                .contains_key(&animathio_core::AssetId::of(&bytes))
        );
    });
    let imported = ui.read(|editor| editor.project.clone());
    ui.dispatch(Undo);
    assert_eq!(ui.read(|editor| editor.project.clone()), before);
    ui.dispatch(Redo);
    assert_eq!(ui.read(|editor| editor.project.clone()), imported);
}

#[gpui::test]
fn launcher_library_import_creates_a_drop_target_in_one_undo_step(cx: &mut TestAppContext) {
    let mut ui = Ui::open(cx, Project::default(), BTreeMap::new());
    let before = ui.read(|editor| editor.project.clone());
    let path = super::config_dir().join("library-import-launcher.wav");
    std::fs::write(&path, wav(&[(0.25, 1000)])).unwrap();
    ui.update(|editor, _, cx| editor.import_paths(&[path], None, cx));
    ui.read(|editor| {
        assert!(!editor.is_start_screen());
        assert_eq!(editor.project.assets().len(), 1);
        assert_eq!(editor.project.tracks().len(), 1);
        assert_eq!(clip_count(editor), 0);
    });
    ui.dispatch(Undo);
    assert_eq!(ui.read(|editor| editor.project.clone()), before);
}

/// Exercise the real rendered Library and its scoped input subscriptions.
#[gpui::test]
fn library_tabs_have_distinct_contents_and_scoped_search(cx: &mut TestAppContext) {
    if !require_plugins("library_tabs_have_distinct_contents_and_scoped_search") {
        return;
    }
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.dispatch(actions::FocusLibrary);
    for (tab, selector) in [
        (LeftTab::Media, "media-kind-filters"),
        (LeftTab::Elements, "element-card-0"),
        (LeftTab::Titles, "title-centered_title"),
        (LeftTab::Effects, "library-write-on"),
        (LeftTab::Plugins, "plugin-inventory"),
    ] {
        ui.update(|e, _, cx| e.select_left_tab(tab, cx));
        assert!(
            ui.cx.debug_bounds(selector).is_some(),
            "missing content for {}",
            tab.label()
        );
        assert_eq!(
            ui.cx.debug_bounds("media-kind-filters").is_some(),
            tab == LeftTab::Media
        );
        assert_eq!(
            ui.cx.debug_bounds("element-card-0").is_some(),
            tab == LeftTab::Elements
        );
        assert_eq!(
            ui.cx.debug_bounds("title-centered_title").is_some(),
            tab == LeftTab::Titles
        );
        assert_eq!(
            ui.cx.debug_bounds("plugin-inventory").is_some(),
            tab == LeftTab::Plugins
        );
        assert_eq!(
            ui.cx.debug_bounds("library-write-on").is_some(),
            tab == LeftTab::Effects
        );
    }
    search_library(&mut ui, LeftTab::Elements, "latex");
    assert!(ui.cx.debug_bounds("element-card-0").is_some());
    assert!(ui.cx.debug_bounds("element-card-1").is_none());
    assert_eq!(ui.read(|e| e.element_kinds().len()), 1);
    search_library(&mut ui, LeftTab::Titles, "caption");
    assert!(ui.cx.debug_bounds("title-caption").is_some());
    assert!(ui.cx.debug_bounds("title-centered_title").is_none());
    // Transitions live in Effects and are filtered by its search like everything else.
    ui.update(|e, _, cx| e.select_left_tab(LeftTab::Effects, cx));
    for key in ["video", "audio", "both"] {
        assert!(bounds_of(&mut ui, format!("library-transition-{key}")).is_some());
    }
    search_library(&mut ui, LeftTab::Effects, "invert");
    assert!(ui.cx.debug_bounds("library-transition-video").is_none());
    assert!(
        ui.cx
            .debug_bounds("effect-item-org.animathio.effects-basic-invert")
            .is_some()
    );
    assert!(ui.cx.debug_bounds("library-write-on").is_none());
    assert!(ui.cx.debug_bounds("effects-empty").is_none());
    // A search that hides everything says so instead of claiming no plugins are loaded.
    search_library(&mut ui, LeftTab::Effects, "no such effect");
    assert!(ui.cx.debug_bounds("effects-empty").is_some());
    assert!(ui.cx.debug_bounds("library-transition-video").is_none());
    search_library(&mut ui, LeftTab::Effects, "write on");
    assert!(ui.cx.debug_bounds("library-write-on").is_some());
    assert!(ui.cx.debug_bounds("effects-empty").is_none());
    search_library(&mut ui, LeftTab::Effects, "invert");
    search_library(&mut ui, LeftTab::Plugins, "import-manim");
    assert!(
        ui.cx
            .debug_bounds("installed-org.animathio.import-manim")
            .is_some()
    );
    assert!(ui.cx.debug_bounds("installed-org.animathio.text").is_none());
    search_library(&mut ui, LeftTab::Media, "independent media query");
    ui.update(|e, _, cx| e.select_left_tab(LeftTab::Elements, cx));
    assert_eq!(
        ui.read(|e| e.media_view.queries[LeftTab::Elements as usize].clone()),
        "latex"
    );
    assert!(ui.cx.debug_bounds("element-card-0").is_some());
    assert!(ui.cx.debug_bounds("element-card-1").is_none());
    assert!(ui.cx.debug_bounds("media-kind-filters").is_none());
}

#[gpui::test]
fn library_plugins_lists_all_eight_kinds_without_duplicate_element_sources(
    cx: &mut TestAppContext,
) {
    if !require_plugins("library_plugins_lists_all_eight_kinds_without_duplicate_element_sources") {
        return;
    }
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.dispatch(actions::FocusLibrary);
    ui.dispatch(actions::ShowPlugins);
    let ids = ui.read(|e| e.plugin_host.as_ref().unwrap().loaded_plugin_ids());
    let expected: Vec<_> = [
        "animations",
        "audio-basic",
        "effects-basic",
        "graph",
        "import-manim",
        "latex",
        "shapes",
        "text",
    ]
    .into_iter()
    .map(|id| format!("org.animathio.{id}"))
    .collect();
    assert_eq!(ids, expected);
    for id in ids {
        search_library(&mut ui, LeftTab::Plugins, &id);
        assert!(
            bounds_of(&mut ui, format!("installed-{id}")).is_some(),
            "missing {id}"
        );
        assert!(ui.read(|e| {
            e.plugin_host
                .as_ref()
                .unwrap()
                .get_plugin(&id)
                .unwrap()
                .source_path
                .is_some()
        }));
    }
    search_library(&mut ui, LeftTab::Elements, "");
    ui.read(|e| {
        assert_eq!(e.element_kinds().len(), 11);
        let sources: Vec<_> = e
            .element_plugin_sources()
            .iter()
            .map(|p| p.id.as_str())
            .collect();
        assert_eq!(sources, ["org.animathio.shapes"]);
    });
    for id in ["text", "latex", "graph"] {
        assert!(bounds_of(&mut ui, format!("element-plugin-org.animathio.{id}")).is_none());
    }
    // A third-party clip source belongs to Elements, not to Titles or Effects.
    ui.update(|e, _, cx| {
        let mut descriptor = e
            .plugins
            .iter()
            .find(|p| p.id == "org.animathio.text")
            .unwrap()
            .clone();
        descriptor.id = "example.generated".into();
        descriptor.name = "Example generator".into();
        e.plugins.push(descriptor);
        cx.notify();
    });
    search_library(&mut ui, LeftTab::Elements, "example");
    assert!(
        ui.cx
            .debug_bounds("element-plugin-example.generated")
            .is_some()
    );
    assert_eq!(ui.read(|e| e.element_plugin_sources().len()), 1);
}

#[gpui::test]
fn library_tab_arrows_move_selection_and_focus(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.dispatch(actions::FocusLibrary);
    ui.update(|e, window, cx| {
        e.select_left_tab(LeftTab::Media, cx);
        window.focus(&e.media_view.tab_focus[0], cx);
    });
    ui.press("right");
    assert!(ui.read(|e| e.left_tab == LeftTab::Elements));
    ui.press("right");
    assert!(ui.read(|e| e.left_tab == LeftTab::Titles));
    ui.press("end");
    assert!(ui.read(|e| e.left_tab == LeftTab::Plugins));
    ui.press("right");
    assert!(ui.read(|e| e.left_tab == LeftTab::Media));
    ui.press("left");
    assert!(ui.read(|e| e.left_tab == LeftTab::Plugins));
    ui.press("home");
    assert!(ui.read(|e| e.left_tab == LeftTab::Media));
    // Tab can focus a tab header without selecting it; arrows start from the focused one.
    ui.update(|e, window, cx| window.focus(&e.media_view.tab_focus[LeftTab::Effects as usize], cx));
    ui.press("right");
    assert!(ui.read(|e| e.left_tab == LeftTab::Plugins));
    ui.update(|e, window, cx| window.focus(&e.media_view.tab_focus[LeftTab::Titles as usize], cx));
    ui.press("left");
    assert!(ui.read(|e| e.left_tab == LeftTab::Elements));
}

/// The ten 1.x text styles: listed under Typography, one undo step each, exact size/weight.
#[gpui::test]
fn typography_presets_add_with_their_weight_in_one_undo_step(cx: &mut TestAppContext) {
    if !require_plugins("typography_presets_add_with_their_weight_in_one_undo_step") {
        return;
    }
    // 1.x sizes in px against its 360-high canvas, with the legacy weights.
    let expected = [
        ("typography_title", 28.0, 600.0),
        ("typography_subtitle", 16.0, 600.0),
        ("typography_body", 14.0, 400.0),
        ("typography_caption", 12.0, 400.0),
        ("typography_heading_1", 24.0, 800.0),
        ("typography_heading_2", 20.0, 800.0),
        ("typography_heading_3", 18.0, 800.0),
        ("typography_heading_4", 16.0, 800.0),
        ("typography_heading_5", 14.0, 800.0),
        ("typography_heading_6", 12.0, 800.0),
    ];
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.dispatch(actions::FocusLibrary);
    ui.update(|e, _, cx| e.select_left_tab(LeftTab::Titles, cx));
    for (id, _, _) in expected {
        assert!(
            bounds_of(&mut ui, format!("title-{id}")).is_some(),
            "{id} card is listed"
        );
    }
    for (id, px, weight) in expected {
        let before = ui.read(|e| e.project.clone());
        let clips_before = ui.read(clip_count);
        ui.update(|e, _, cx| e.add_title_preset(id, Time(0), cx));
        let added = ui.read(|e| e.selected.expect("the added text clip is selected"));
        ui.read(|e| {
            assert_eq!(clip_count(e), clips_before + 1, "{id}: one text layer");
            let animathio_core::ClipKind::Plugin { plugin_id, params } =
                &e.find_clip(added).expect("clip").kind
            else {
                panic!("{id}: plugin clip expected")
            };
            assert_eq!(plugin_id, "org.animathio.text");
            assert_eq!(params["font_weight"], serde_json::json!(weight), "{id}");
            assert_eq!(
                params["font"],
                serde_json::json!("inter"),
                "{id}: typography is Inter at every weight"
            );
            let size = params["font_size"].as_f64().unwrap();
            let (width, height) = project_size(&e.project);
            // Normalized against 360 px of the shorter canvas edge (16:9 here).
            assert!(width >= height);
            assert!((size - px / 360.0).abs() < 1e-9, "{id}: size {size}");
        });
        let applied = ui.read(|e| e.project.clone());
        ui.dispatch(Undo);
        assert_eq!(
            ui.read(|e| e.project.clone()),
            before,
            "{id}: one undo step"
        );
        ui.dispatch(Redo);
        assert_eq!(ui.read(|e| e.project.clone()), applied, "{id}: redo");
        ui.dispatch(Undo);
    }
}

#[gpui::test]
fn library_tabs_are_icon_only_at_the_default_dock_width_and_fit(cx: &mut TestAppContext) {
    use crate::editor::workspace::LIBRARY_TABS_LABEL_WIDTH;
    assert!(crate::layout::DockLayout::default().left_width < LIBRARY_TABS_LABEL_WIDTH);
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    // `Editor::new` loads the dock layout persisted in the shared test config dir, which other
    // tests rewrite (a closed Library panel) while this one runs in parallel; pin the default.
    ui.update(|e, _, cx| {
        e.dock = crate::layout::DockLayout::default();
        cx.notify();
    });
    ui.cx
        .simulate_resize(gpui::size(gpui::px(1280.), gpui::px(720.)));
    ui.settle();
    let names = ["media", "elements", "titles", "effects", "plugins"];
    let missing: Vec<&str> = names
        .into_iter()
        .filter(|name| bounds_of(&mut ui, format!("library-tab-{name}")).is_none())
        .collect();
    let (start, left_width, left_tab, dock) = ui.read(|e| {
        (
            e.is_start_screen(),
            e.dock.left_width,
            e.left_tab as usize,
            format!("{:?}", e.dock),
        )
    });
    assert!(
        missing.is_empty(),
        "library tabs not rendered: {missing:?} (start screen: {start}, left width: {left_width}, tab index: {left_tab}, dock: {dock})"
    );
    let mut previous_right = gpui::px(0.);
    for name in names {
        let bounds = bounds_of(&mut ui, format!("library-tab-{name}")).expect("tab rendered");
        assert!(bounds.size.width >= gpui::px(30.), "{name}: {bounds:?}");
        assert!(
            bounds.left() >= previous_right,
            "{name} overlaps its neighbour"
        );
        previous_right = bounds.right();
    }
    // The dock stays within its panel: the last tab ends left of the preview.
    assert!(previous_right < gpui::px(300.));
}

#[test]
fn element_cards_use_the_dock_width() {
    use crate::editor::math::element_card_columns;
    let rem = gpui::px(16.);
    assert_eq!(element_card_columns(rem, 200.), 1);
    assert_eq!(element_card_columns(rem, 260.), 2);
    assert_eq!(element_card_columns(rem, 400.), 3);
    assert_eq!(element_card_columns(rem, 640.), 3);
}

/// The Library grids never clip a card at the panel's right edge (see
/// `docs/design.md` "Library cards and lists"): at 1280x720 the second
/// element card ends inside the Library body.
#[gpui::test]
fn element_cards_stay_inside_the_library_panel_at_720p(cx: &mut TestAppContext) {
    if !require_plugins("element_cards_stay_inside_the_library_panel_at_720p") {
        return;
    }
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.cx
        .simulate_resize(gpui::size(gpui::px(1280.), gpui::px(720.)));
    ui.update(|e, _, cx| e.select_left_tab(LeftTab::Elements, cx));
    ui.settle();
    let card = ui
        .cx
        .debug_bounds("element-card-1")
        .expect("the second element card is rendered");
    let panel = ui
        .cx
        .debug_bounds("library-body")
        .expect("the Library body is rendered");
    assert!(
        card.right() <= panel.right() + gpui::px(1.),
        "element-card-1 ends at {}, past the Library panel edge at {}",
        f32::from(card.right()),
        f32::from(panel.right())
    );
    assert!(
        card.left() >= panel.left(),
        "element-card-1 starts at {}, left of the Library panel edge at {}",
        f32::from(card.left()),
        f32::from(panel.left())
    );
}

#[test]
fn counts_are_pluralised_and_import_errors_summarised() {
    use crate::editor::support::{count_label, import_error_summary, unsupported_media_message};
    assert_eq!(count_label(1, "media file"), "1 media file");
    assert_eq!(count_label(3, "media file"), "3 media files");
    assert_eq!(import_error_summary(&[]), None);
    assert_eq!(import_error_summary(&["a".into()]).as_deref(), Some("a"));
    assert_eq!(
        import_error_summary(&["first".into(), "second".into(), "third".into()]).as_deref(),
        Some("3 files could not be imported. first")
    );
    let message = unsupported_media_message(std::path::Path::new("/some/dir/clip.gif"));
    assert!(message.starts_with("clip.gif is not a supported media file"));
    assert!(message.contains("MP4") && message.contains("WAV") && message.contains("PNG"));
    assert!(
        !message.contains("/some/dir"),
        "names the file, not the path"
    );
}

#[gpui::test]
fn import_names_every_refused_file_and_still_imports_the_rest(cx: &mut TestAppContext) {
    let mut ui = Ui::open(cx, Project::default(), BTreeMap::new());
    let dir = super::config_dir();
    let good = dir.join("usability-good.wav");
    let bad_one = dir.join("usability-notes.txt");
    let bad_two = dir.join("usability-clip.gif");
    std::fs::write(&good, wav(&[(0.25, 800)])).unwrap();
    std::fs::write(&bad_one, b"hello").unwrap();
    std::fs::write(&bad_two, b"GIF89a").unwrap();
    ui.update(|editor, _, cx| {
        editor.import_paths(&[good, bad_one.clone(), bad_two], None, cx);
    });
    let error = last_error(&ui);
    assert!(
        error.starts_with("2 files could not be imported."),
        "{error}"
    );
    assert!(error.contains("usability-notes.txt"), "{error}");
    assert_eq!(ui.read(|e| e.project.assets().len()), 1, "the wav imported");
    clear_error(&mut ui);
    ui.update(|editor, _, cx| editor.import_paths(&[bad_one], None, cx));
    let error = last_error(&ui);
    assert!(
        error.starts_with("usability-notes.txt is not a supported"),
        "{error}"
    );
}

#[gpui::test]
fn effects_tab_hints_at_selecting_a_clip_first(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.dispatch(ShowEffects);
    ui.update(|editor, _, cx| editor.clear_selection(cx));
    assert!(
        ui.cx.debug_bounds("effects-hint").is_some(),
        "hint while nothing is selected"
    );
    let id = ui.read(|e| e.project.tracks()[0].clips().values().next().unwrap().id);
    ui.update(|editor, _, cx| editor.select_only(id, cx));
    ui.update(|_, window, _| window.refresh());
    assert!(
        ui.cx.debug_bounds("effects-hint").is_none(),
        "hint disappears once a clip is selected"
    );
    // Clicking an effect with nothing selected explains what to do instead of doing nothing.
    ui.update(|editor, _, cx| {
        editor.clear_selection(cx);
        editor.add_effect_to_selected("org.animathio.effects-basic", "blur", cx);
    });
    assert!(last_error(&ui).starts_with("Select a clip first"));
}

#[gpui::test]
fn clicking_media_over_an_occupied_playhead_lands_on_a_free_layer(cx: &mut TestAppContext) {
    // The sample has an image clip at 0..3 s; clicking its library card again must not fail
    // with a bare "overlapping clips".
    let (project, assets) = sample();
    let before = project.clone();
    let image = project
        .assets()
        .iter()
        .find(|(_, asset)| asset.media_type.starts_with("image/"))
        .map(|(id, _)| id.clone())
        .expect("the sample has an image");
    let mut ui = Ui::open(cx, project, assets);
    let clips = ui.read(clip_count);
    ui.update(|editor, _, cx| editor.add_asset_at_playhead(image.clone(), cx));
    assert_eq!(last_error(&ui), "", "no error");
    assert_eq!(ui.read(clip_count), clips + 1);
    ui.read(|editor| editor.project.validate().expect("the project stays valid"));
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before, "one undo step");

    // The same for audio: an occupied voice track gets a new audio track instead of an error.
    let (project, assets) = voice_and_music();
    let voice = project
        .assets()
        .keys()
        .next()
        .cloned()
        .expect("an audio asset");
    let mut ui = Ui::open(cx, project, assets);
    let tracks = ui.read(|e| e.project.tracks().len());
    ui.update(|editor, _, cx| editor.add_asset_at_playhead(voice, cx));
    assert_eq!(last_error(&ui), "", "no error");
    assert_eq!(ui.read(|e| e.project.tracks().len()), tracks + 1);
    ui.read(|editor| editor.project.validate().expect("the project stays valid"));
}

#[gpui::test]
fn repeated_audio_clicks_reuse_a_free_audio_track(cx: &mut TestAppContext) {
    use super::fixtures::{MUSIC_CLIP, MUSIC_TRACK};
    // Voice is busy at the playhead; the music track is free there (its clip starts at 10 s).
    let (mut project, assets) = voice_and_music();
    Command::MoveClip {
        id: MUSIC_CLIP,
        track: MUSIC_TRACK,
        start: Time(10_000 * 1_000_000),
    }
    .apply(&mut project)
    .unwrap();
    let voice = project.assets().keys().next().cloned().unwrap();
    let mut ui = Ui::open(cx, project, assets);
    let tracks = ui.read(|e| e.project.tracks().len());
    let clips = ui.read(clip_count);
    ui.update(|editor, _, cx| editor.add_asset_at_playhead(voice.clone(), cx));
    assert_eq!(last_error(&ui), "", "no error");
    assert_eq!(ui.read(clip_count), clips + 1);
    assert_eq!(
        ui.read(|e| e.project.tracks().len()),
        tracks,
        "no new track"
    );
    // Voice and the new clip occupy 0..3 s now; a second click must still not pile up tracks
    // beyond the one it needs, and a third click at a later free spot adds none.
    ui.update(|editor, _, cx| {
        editor.position = Time(5_000 * 1_000_000);
        editor.add_asset_at_playhead(voice.clone(), cx);
    });
    assert_eq!(ui.read(|e| e.project.tracks().len()), tracks);
    ui.read(|editor| editor.project.validate().expect("the project stays valid"));
}
