//! Inspector consistency, the content-hugging selection box, timeline header controls and the
//! short-window density: scenarios for the UI polish pass.

use animathio_control::MathObjectKind;
use animathio_core::ClipKind;
use gpui::{px, size};

use super::fixtures::*;
use super::{ClipId, Command, LeftTab, Time, Ui, actions, require_plugins};
use crate::editor::support::project_size;
use crate::editor::timeline_view::track_has_audio;

/// An environment with its own config directory. (The first environment of a process decides the
/// config directory for helpers without an editor handle, so scenarios here never save the dock
/// layout: that would leak a track height into every other scenario.)
fn private_env(name: &str) -> super::EditorEnv {
    let dir = super::config_dir().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("config dir");
    super::EditorEnv::headless(
        crate::editor_env::built_plugins_dir().map_or(
            crate::editor_env::PluginSource::None,
            crate::editor_env::PluginSource::Dir,
        ),
        dir,
    )
}

/// The top y of an inspector section, by its debug selector (`inspector-section-<name>`).
fn section_top(ui: &mut Ui, selector: &'static str) -> Option<f32> {
    ui.cx
        .debug_bounds(selector)
        .map(|bounds| f32::from(bounds.origin.y))
}

fn select_clip(ui: &mut Ui, id: ClipId) {
    ui.update(|e, _, cx| e.select_only(id, cx));
}

/// Text, formulas and shapes select with a box around what they draw, not around the canvas.
#[gpui::test]
fn generated_clips_select_with_a_box_that_hugs_their_ink(cx: &mut gpui::TestAppContext) {
    if !require_plugins("generated_clips_select_with_a_box_that_hugs_their_ink") {
        return;
    }
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    for kind in [MathObjectKind::Latex, MathObjectKind::Circle] {
        ui.update(|e, _, cx| e.add_math_default(kind, Time(0), cx));
        let (id, raw, fitted) = ui
            .update(|e, _, _| e.selected_visual())
            .expect("a visual clip is selected");
        assert!(
            fitted.scale_x < raw.scale_x * 0.6 && fitted.scale_y < raw.scale_y * 0.6,
            "{kind:?} {id:?}: box {:?} should be well inside the canvas-sized {:?}",
            (fitted.scale_x, fitted.scale_y),
            (raw.scale_x, raw.scale_y)
        );
        assert!(fitted.scale_x > 0. && fitted.scale_y > 0.);
        // The ink of a default object sits near the middle of the canvas.
        assert!((fitted.x - raw.x).abs() < 0.2 && (fitted.y - raw.y).abs() < 0.2);
    }
}

/// The bounds come from rendering the clip, so the same clip always gives the same box.
#[gpui::test]
fn content_bounds_are_deterministic(cx: &mut gpui::TestAppContext) {
    if !require_plugins("content_bounds_are_deterministic") {
        return;
    }
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| e.add_math_default(MathObjectKind::Latex, Time(0), cx));
    let first = ui
        .update(|e, _, _| e.selected_visual())
        .expect("selected")
        .2;
    // Forget the cache so the second box is computed from scratch.
    ui.update(|e, _, _| e.drop_content_cache());
    let second = ui
        .update(|e, _, _| e.selected_visual())
        .expect("selected")
        .2;
    assert_eq!(first, second);
}

/// A text clip shows its font size as a percentage of the short canvas edge, and a typed
/// percentage is stored in the clip's own unit.
#[gpui::test]
fn text_font_size_is_shown_as_percent_of_the_short_edge(cx: &mut gpui::TestAppContext) {
    if !require_plugins("text_font_size_is_shown_as_percent_of_the_short_edge") {
        return;
    }
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| e.add_title_preset("typography_body", Time(0), cx));
    let id = ui.read(|e| e.selected.expect("selected"));
    let (stored, shown, units) = ui.read(|e| {
        (
            e.plugin_number_value(id, "font_size"),
            e.plugin_number_display_value(id, "font_size"),
            e.plugin_text_value(id, "units", "pixels"),
        )
    });
    let (width, height) = ui.read(|e| project_size(&e.project));
    let short = f64::from(width.min(height));
    let expected = match units.as_str() {
        "normalized" => stored * 100.0 * f64::from(height) / short,
        "percent" => stored,
        _ => stored / short * 100.0,
    };
    assert!(
        (shown - expected).abs() < 1e-9,
        "{units}: stored {stored} shown {shown}"
    );
    // Typing the displayed value back changes nothing.
    let back = ui.read(|e| e.plugin_number_from_display(id, "font_size", shown));
    assert!((back - stored).abs() < 1e-9, "{back} vs {stored}");
    // Other number parameters are not converted.
    let weight = ui.read(|e| e.plugin_number_display_value(id, "font_weight"));
    assert!((weight - 400.0).abs() < 1e-9);
}

/// LaTeX and text clips use one unit: the inspector row for LaTeX is a percent, as stored.
#[gpui::test]
fn latex_font_size_shows_as_stored_percent(cx: &mut gpui::TestAppContext) {
    if !require_plugins("latex_font_size_shows_as_stored_percent") {
        return;
    }
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| e.add_math_default(MathObjectKind::Latex, Time(0), cx));
    let id = ui.read(|e| e.selected.expect("selected"));
    let (units, stored, shown) = ui.read(|e| {
        (
            e.plugin_text_value(id, "units", "pixels"),
            e.plugin_number_value(id, "size"),
            e.plugin_number_display_value(id, "size"),
        )
    });
    assert_eq!(units, "percent");
    assert!((stored - shown).abs() < 1e-9);
}

/// Text, choices and switches get no keyframe controls; numbers, colors and points do.
#[gpui::test]
fn only_interpolating_parameters_are_animatable(cx: &mut gpui::TestAppContext) {
    if !require_plugins("only_interpolating_parameters_are_animatable") {
        return;
    }
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| e.add_title_preset("typography_body", Time(0), cx));
    let id = ui.read(|e| e.selected.expect("selected"));
    ui.read(|e| {
        let animatable = |name: &str| {
            crate::editor::inspector_units::is_animatable(
                &e.plugin_model(id, name)
                    .unwrap_or_else(|| panic!("text parameter {name}")),
            )
        };
        for name in ["font_size", "font_weight", "color", "fit_width"] {
            assert!(animatable(name), "{name} keyframes");
        }
        for name in ["text", "units", "font", "caption_style"] {
            assert!(!animatable(name), "{name} does not keyframe");
        }
    });
}

/// Audio clips lead with the Audio section; video and generated clips keep their order.
#[gpui::test]
fn audio_inspector_leads_with_audio_before_speed(cx: &mut gpui::TestAppContext) {
    let (project, assets) = voice_and_music();
    let mut ui = Ui::open(cx, project, assets);
    select_clip(&mut ui, VOICE_CLIP);
    ui.update(|_, window, _| window.refresh());
    let audio = section_top(&mut ui, "section-audio").expect("Audio section");
    let speed = section_top(&mut ui, "section-speed").expect("Speed section");
    assert!(audio < speed, "Audio ({audio}) above Speed ({speed})");
}

/// A text clip's Speed section stays below its content.
#[gpui::test]
fn text_inspector_keeps_content_before_speed(cx: &mut gpui::TestAppContext) {
    if !require_plugins("text_inspector_keeps_content_before_speed") {
        return;
    }
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    let text = ui.read(|e| {
        e.project
            .tracks()
            .iter()
            .flat_map(|t| t.clips().values())
            .find(|c| {
                matches!(&c.kind, ClipKind::Plugin { plugin_id, .. } if plugin_id == "org.animathio.text")
            })
            .map(|c| c.id)
            .expect("the sample has a text clip")
    });
    select_clip(&mut ui, text);
    ui.update(|_, window, _| window.refresh());
    let params = section_top(&mut ui, "section-params").expect("content section");
    let speed = section_top(&mut ui, "section-speed").expect("speed section");
    assert!(params < speed);
}

/// Picture-only tracks lose the gain slider and solo; tracks with sound keep them.
#[gpui::test]
fn only_tracks_with_sound_have_audio_header_controls(cx: &mut gpui::TestAppContext) {
    let (project, assets) = voice_and_music();
    let ui = Ui::open(cx, project, assets);
    ui.read(|e| {
        let track = |id| {
            e.project
                .tracks()
                .iter()
                .find(|t| t.id() == id)
                .expect("track")
        };
        assert!(track_has_audio(track(VOICE_TRACK)));
        assert!(track_has_audio(track(MUSIC_TRACK)));
    });
    let (project, assets) = sample();
    let ui = Ui::open(cx, project, assets);
    ui.read(|e| {
        for track in e.project.tracks() {
            let sound = track
                .clips()
                .values()
                .any(|c| matches!(c.kind, ClipKind::Audio(_) | ClipKind::Video(_)));
            assert_eq!(
                track_has_audio(track),
                sound || track.clips().is_empty(),
                "track {}",
                track.name()
            );
        }
    });
}

/// Hiding a picture-only track is the existing mute command, which the renderer already honors.
#[gpui::test]
fn the_eye_toggle_hides_a_picture_track_through_the_mute_command(cx: &mut gpui::TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    let track = ui.read(|e| {
        e.project
            .tracks()
            .iter()
            .find(|t| !track_has_audio(t))
            .map(animathio_core::Track::id)
    });
    let Some(track) = track else {
        eprintln!("sample has no picture-only track");
        return;
    };
    ui.update(|e, _, cx| {
        e.execute(
            &Command::SetTrackMuted {
                id: track,
                muted: true,
            },
            cx,
        );
    });
    assert!(ui.read(|e| {
        e.project
            .tracks()
            .iter()
            .any(|t| t.id() == track && t.is_muted())
    }));
    ui.dispatch(actions::Undo);
    assert!(ui.read(|e| {
        e.project
            .tracks()
            .iter()
            .all(|t| t.id() != track || !t.is_muted())
    }));
}

/// Below 800 px the timeline drops its overview strip and the editor reports a compact density.
#[gpui::test]
fn short_windows_hide_the_overview_and_compact_the_transport(cx: &mut gpui::TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.cx.simulate_resize(size(px(1280.), px(900.)));
    ui.settle();
    assert!(!ui.read(|e| e.compact_height));
    assert!(ui.cx.debug_bounds("timeline-overview").is_some());
    ui.cx.simulate_resize(size(px(1280.), px(720.)));
    ui.settle();
    assert!(ui.read(|e| e.compact_height));
    assert!(ui.cx.debug_bounds("timeline-overview").is_none());
    ui.cx.simulate_resize(size(px(1920.), px(1080.)));
    ui.settle();
    assert!(ui.cx.debug_bounds("timeline-overview").is_some());
}

/// An icon-only Library names its active tab in the panel header.
#[gpui::test]
fn narrow_library_names_the_active_tab(cx: &mut gpui::TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| e.select_left_tab(LeftTab::Elements, cx));
    assert_eq!(LeftTab::Elements.header(), "Library \u{b7} Elements");
    assert!(ui.read(|e| e.dock.left_width < crate::editor::workspace::LIBRARY_TABS_LABEL_WIDTH));
}

/// Every Library effect has its own icon, so the list reads at a glance.
#[gpui::test]
fn library_effects_have_distinct_icons(cx: &mut gpui::TestAppContext) {
    let _ = cx;
    let video = [
        "grayscale",
        "sepia",
        "invert",
        "saturate",
        "brightness",
        "contrast",
        "blur",
        "opacity",
        "slide",
    ];
    let icons: Vec<_> = video
        .iter()
        .map(|e| format!("{:?}", crate::editor::media_pool::effect_icon(false, e)))
        .collect();
    let unique: std::collections::BTreeSet<_> = icons.iter().collect();
    assert_eq!(unique.len(), video.len(), "{icons:?}");
    let audio = [
        "gain",
        "eq3",
        "compressor",
        "gate",
        "highpass",
        "voice_cleanup",
        "denoise",
    ];
    let icons: Vec<_> = audio
        .iter()
        .map(|e| format!("{:?}", crate::editor::media_pool::effect_icon(true, e)))
        .collect();
    let unique: std::collections::BTreeSet<_> = icons.iter().collect();
    assert_eq!(unique.len(), audio.len(), "{icons:?}");
}

/// A first run (no saved layout) in a short window starts with small track rows.
#[gpui::test]
fn first_run_in_a_short_window_starts_with_small_tracks(cx: &mut gpui::TestAppContext) {
    let (project, assets) = sample();
    let mut env = private_env("first-run-density");
    env.adaptive_density = true;
    let mut ui = Ui::open_with_env(cx, project, assets, env);
    ui.cx.simulate_resize(size(px(1280.), px(720.)));
    ui.settle();
    ui.update(|_, window, _| window.refresh());
    assert!(ui.read(|e| e.compact_height));
    // The decision is made once, on the first editor frame, at whatever size the window opened.
    assert!(ui.read(|e| e.density_decided));
}
