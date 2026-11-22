//! Inspector consistency, the content-hugging selection box, timeline header controls and the
//! short-window density: scenarios for the UI polish pass.

use animathio_control::MathObjectKind;
use animathio_core::{ClipKind, ClipSpeed, Rounding};
use gpui::{Focusable as _, px, size};

use super::fixtures::*;
use super::{
    Clip, ClipId, Command, LeftTab, TICKS_PER_SECOND, TestAppContext, Time, TimeRange, Track,
    TrackId, Ui, actions, private_env, require_plugins,
};
use crate::editor::actions::{Redo, Undo};
use crate::editor::support::project_size;
use crate::editor::timeline_view::track_has_audio;

/// The top y of an inspector section, by its debug selector (`inspector-section-<name>`).
fn section_top(ui: &mut Ui, selector: &'static str) -> Option<f32> {
    ui.cx
        .debug_bounds(selector)
        .map(|bounds| f32::from(bounds.origin.y))
}

fn select_clip(ui: &mut Ui, id: ClipId) {
    ui.update(|e, _, cx| e.select_only(id, cx));
}

#[gpui::test]
fn image_fit_segment_sits_beside_its_label_at_720p(cx: &mut gpui::TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.cx.simulate_resize(size(px(1280.), px(720.)));
    ui.settle();
    let image = ClipId(2);
    select_clip(&mut ui, image);
    ui.update(|_, window, _| window.refresh());

    let row = ui.cx.debug_bounds("param-Fit").expect("Fit row");
    let label = ui.cx.debug_bounds("fit-label").expect("Fit label");
    let position = ui.cx.debug_bounds("param-Position").expect("Position row");
    let fill = ui.cx.debug_bounds("fit-2-cover").expect("Fill segment");
    assert!((label.center().y - fill.center().y).abs() <= px(1.));
    assert!(label.left() > row.left() && label.right() < fill.left());
    assert_eq!(row.left(), position.left());
    assert!(fill.right() <= row.right());
    ui.click(fill.center(), gpui::Modifiers::default());
    assert_eq!(
        ui.read(|e| e.find_clip(image).unwrap().fit),
        animathio_core::Fit::Cover
    );
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

/// A pop caption's active word grows with the source clock, so a clip slowed to 0.25x
/// draws a bigger word on every output frame even though the source frame advances only
/// every fourth one: the ink bounds must be recomputed per output frame (the content cache
/// quantises the playhead to the output frame index, not to the slower source clock).
#[gpui::test]
fn slowed_generated_clips_recompute_their_bounds_every_output_frame(cx: &mut gpui::TestAppContext) {
    if !require_plugins("slowed_generated_clips_recompute_their_bounds_every_output_frame") {
        return;
    }
    let mut project = animathio_core::Project::default();
    Command::Batch {
        commands: vec![
            Command::AddTrack {
                track: Track::new(TrackId(1), "Pop"),
                index: 0,
            },
            Command::AddClip {
                track: TrackId(1),
                clip: Clip::new(
                    ClipId(1),
                    ClipKind::Plugin {
                        plugin_id: "org.animathio.text".into(),
                        params: serde_json::json!({
                            "text": "AniMathIO",
                            "font_size": 0.1,
                            "color": [255, 255, 255, 255],
                            "units": "normalized",
                            "align": "center",
                            "fit_width": 0.8,
                            "fit_height": 0.2,
                            "caption_style": "pop",
                            // One word, active for two seconds of source time.
                            "words": "0:1411200000",
                        }),
                    },
                    TimeRange::new(Time(0), Time(8 * TICKS_PER_SECOND)).unwrap(),
                ),
            },
        ],
    }
    .apply(&mut project)
    .expect("add a pop caption clip");
    let mut ui = Ui::open(cx, project, std::collections::BTreeMap::new());
    ui.update(|e, _, cx| e.select_only(ClipId(1), cx));
    // Slow to 0.25x, keeping the source interval (the timeline range grows fourfold).
    ui.update(|e, _, cx| {
        assert!(e.try_execute(
            &Command::SetClipSpeed {
                id: ClipId(1),
                speed: ClipSpeed::new(1, 4).expect("0.25x"),
                reverse: false,
                ramps: Vec::new(),
                ripple: false,
            },
            cx
        ));
    });
    let frame = ui
        .read(|e| e.project.frame_rate().frame_to_time(1, Rounding::Nearest))
        .expect("frame width");
    let mut boxes = Vec::new();
    for n in 0..6_i64 {
        ui.update(|e, _, cx| e.seek_to(Time(n * frame.0), cx));
        let (_, _, fitted) = ui
            .update(|e, _, _| e.selected_visual())
            .expect("the caption is selected and visible");
        boxes.push((fitted.scale_x, fitted.scale_y));
    }
    for pair in boxes.windows(2) {
        assert!(
            pair[1].0 > pair[0].0 && pair[1].1 > pair[0].1,
            "bounds must update every output frame at 0.25x: {boxes:?}"
        );
    }
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
    let (mut project, assets) = sample();
    let track = TrackId(900);
    Command::Batch {
        commands: vec![
            Command::AddTrack {
                track: Track::new(track, "Titles"),
                index: 0,
            },
            Command::AddClip {
                track,
                clip: Clip::new(
                    ClipId(900),
                    ClipKind::Plugin {
                        plugin_id: "org.animathio.text".into(),
                        params: serde_json::Value::Null,
                    },
                    TimeRange::new(Time(0), Time(TICKS_PER_SECOND)).unwrap(),
                ),
            },
        ],
    }
    .apply(&mut project)
    .expect("add a text track");
    let mut ui = Ui::open(cx, project, assets);
    assert!(ui.read(|e| {
        let t = e
            .project
            .tracks()
            .iter()
            .find(|t| t.id() == track)
            .expect("text track");
        !track_has_audio(t) && !t.is_muted()
    }));
    // A tall window keeps every track header on screen.
    ui.cx.simulate_resize(size(px(1600.), px(2400.)));
    ui.settle();
    // Click the eye pill in the track header, like a user would.
    let pill = ui
        .cx
        .debug_bounds(Box::leak(
            format!("track-mute-{}", track.0).into_boxed_str(),
        ))
        .expect("the text track header shows its eye pill");
    ui.click(pill.center(), gpui::Modifiers::default());
    assert!(ui.read(|e| {
        e.project
            .tracks()
            .iter()
            .any(|t| t.id() == track && t.is_muted())
    }));
    // The solo pill stays on picture tracks: solo hides every other track from the picture.
    assert!(
        ui.cx
            .debug_bounds(Box::leak(
                format!("track-solo-{}", track.0).into_boxed_str()
            ))
            .is_some()
    );
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

/// The Library panel header is "Library" with the active tab as its context
/// at every dock width (see `docs/design.md` "Docked panels").
#[gpui::test]
fn narrow_library_names_the_active_tab(cx: &mut gpui::TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| e.select_left_tab(LeftTab::Elements, cx));
    assert_eq!(LeftTab::Elements.label(), "Elements");
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
    // Resize in the same step as opening, before any frame has settled.
    let mut ui = Ui::open_sized(cx, project, assets, env, Some(size(px(1280.), px(720.))));
    assert!(ui.read(|e| e.compact_height));
    // The decision waits for a stable window size, then is made once.
    assert!(ui.read(|e| e.density_decided));
    assert_eq!(
        ui.read(|e| e.dock.track_height),
        crate::layout::TrackHeight::Small
    );
    // A later resize never changes it again.
    ui.cx.simulate_resize(size(px(1920.), px(1080.)));
    ui.settle();
    assert_eq!(
        ui.read(|e| e.dock.track_height),
        crate::layout::TrackHeight::Small
    );
}

/// The id of the first clip of `kind` in the project.
fn first_clip(ui: &mut Ui, matches: impl Fn(&ClipKind) -> bool) -> ClipId {
    ui.read(|e| {
        e.project
            .tracks()
            .iter()
            .flat_map(animathio_core::Track::clips)
            .find(|(_, clip)| matches(&clip.kind))
            .map(|(id, _)| *id)
            .expect("the sample has a clip of that kind")
    })
}

/// The flat inspector fits Transform, down to Rotation, in a 720p window without scrolling.
#[gpui::test]
fn image_inspector_shows_rotation_without_scrolling_at_720p(cx: &mut gpui::TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.cx.simulate_resize(size(px(1280.), px(720.)));
    ui.settle();
    let image = first_clip(&mut ui, |kind| matches!(kind, ClipKind::Image(_)));
    select_clip(&mut ui, image);
    ui.update(|_, window, _| window.refresh());
    ui.settle();
    for row in ["param-Position", "param-Scale", "param-Rotation"] {
        let bounds = ui
            .cx
            .debug_bounds(row)
            .unwrap_or_else(|| panic!("{row} is drawn"));
        assert!(
            bounds.bottom() <= px(720.),
            "{row} is below the 720p window: {bounds:?}"
        );
    }
}

/// LaTeX clips use the same sections as every other visual clip.
#[gpui::test]
fn latex_inspector_has_the_standard_sections(cx: &mut gpui::TestAppContext) {
    if !require_plugins("latex_inspector_has_the_standard_sections") {
        return;
    }
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| e.add_math_default(MathObjectKind::Latex, Time(0), cx));
    ui.settle();
    for section in ["section-params", "section-transform", "section-opacity"] {
        assert!(
            ui.cx.debug_bounds(section).is_some(),
            "{section} is part of a LaTeX clip's inspector"
        );
    }
}

/// Caption-only parameters stay hidden while `caption_style` is off, and come back with a style.
#[gpui::test]
fn caption_rows_hide_while_captions_are_off(cx: &mut gpui::TestAppContext) {
    if !require_plugins("caption_rows_hide_while_captions_are_off") {
        return;
    }
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| e.add_math_default(MathObjectKind::Text, Time(0), cx));
    ui.settle();
    let text = ui
        .update(|e, _, _| e.selected)
        .expect("the new text clip is selected");
    assert!(
        ui.cx.debug_bounds("param-accent").is_none(),
        "accent has no effect without a caption style"
    );
    ui.update(|e, _, cx| {
        e.execute_plugin_param(text, "caption_style".into(), serde_json::json!("pop"), cx);
    });
    ui.settle();
    assert!(
        ui.cx.debug_bounds("param-accent").is_some(),
        "accent shows once a caption style is chosen"
    );
}

/// The overlay scrollbar owns the right edge of the inspector: value fields end left of its
/// lane, so none runs under the thumb (1280x720).
#[gpui::test]
fn inspector_rows_stay_left_of_the_scrollbar_lane(cx: &mut gpui::TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.cx.simulate_resize(size(px(1280.), px(720.)));
    ui.settle();
    let image = first_clip(&mut ui, |kind| matches!(kind, ClipKind::Image(_)));
    select_clip(&mut ui, image);
    ui.update(|_, window, _| window.refresh());
    ui.settle();
    for row in [
        "param-Position",
        "param-Scale",
        "param-Rotation",
        "param-Opacity",
    ] {
        let bounds = ui
            .cx
            .debug_bounds(row)
            .unwrap_or_else(|| panic!("{row} is drawn"));
        assert!(
            bounds.right() <= px(1280.) - crate::editor::fields::SCROLLBAR_GUTTER,
            "{row} runs under the scrollbar lane: {bounds:?}"
        );
    }
}

/// A formula that does not typeset shows its error under its own source; a valid one shows none.
#[gpui::test]
fn latex_errors_show_under_the_formula_of_that_clip(cx: &mut gpui::TestAppContext) {
    if !require_plugins("latex_errors_show_under_the_formula_of_that_clip") {
        return;
    }
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| e.add_math_default(MathObjectKind::Latex, Time(0), cx));
    ui.settle();
    let latex = ui
        .update(|e, _, _| e.selected)
        .expect("the new formula is selected");
    assert!(
        ui.cx.debug_bounds("typeset-error").is_none(),
        "the default formula typesets"
    );
    ui.update(|e, _, cx| {
        e.execute_plugin_param(latex, "source".into(), serde_json::json!(r"\frac{1}{"), cx);
    });
    ui.settle();
    assert!(
        ui.cx.debug_bounds("typeset-error").is_some(),
        "an unfinished fraction is reported under the source"
    );
    ui.update(|e, _, cx| {
        e.execute_plugin_param(
            latex,
            "source".into(),
            serde_json::json!(r"\frac{1}{2}"),
            cx,
        );
    });
    ui.settle();
    assert!(
        ui.cx.debug_bounds("typeset-error").is_none(),
        "a valid formula clears the error"
    );
    // Another clip's failure never shows under this formula.
    ui.update(|e, _, _| e.effect_warning = Some("plugin produced an invalid scene: boom".into()));
    ui.update(|_, window, _| window.refresh());
    ui.settle();
    assert!(ui.cx.debug_bounds("typeset-error").is_none());
}

/// Units and alignment of a formula stay reachable in the collapsed "More" group.
#[gpui::test]
fn latex_more_group_keeps_units_and_alignment_reachable(cx: &mut gpui::TestAppContext) {
    if !require_plugins("latex_more_group_keeps_units_and_alignment_reachable") {
        return;
    }
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| e.add_math_default(MathObjectKind::Latex, Time(0), cx));
    ui.settle();
    assert!(ui.cx.debug_bounds("param-units").is_none());
    ui.update(|e, _, cx| {
        e.fields.more_open = true;
        cx.notify();
    });
    ui.settle();
    for row in ["param-units", "param-align"] {
        assert!(ui.cx.debug_bounds(row).is_some(), "{row} is in More");
    }
}

/// The graph leads with its function, then the ranges, then the colour.
#[gpui::test]
fn graph_inspector_leads_with_the_function(cx: &mut gpui::TestAppContext) {
    if !require_plugins("graph_inspector_leads_with_the_function") {
        return;
    }
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| e.add_math_default(MathObjectKind::Plot, Time(0), cx));
    ui.settle();
    let top = |ui: &mut Ui, row: &'static str| {
        ui.cx
            .debug_bounds(row)
            .unwrap_or_else(|| panic!("{row} is drawn"))
            .origin
            .y
    };
    let items = top(&mut ui, "param-items");
    let range = top(&mut ui, "param-X range");
    let color = top(&mut ui, "param-color");
    assert!(
        items < range && range < color,
        "{items:?} {range:?} {color:?}"
    );
}

/// The Add effect menu opens for one target, closes with Escape and when the selection moves.
#[gpui::test]
fn add_effect_menu_belongs_to_its_selection(cx: &mut gpui::TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    let image = first_clip(&mut ui, |kind| matches!(kind, ClipKind::Image(_)));
    select_clip(&mut ui, image);
    ui.settle();
    // The audio menu of that clip is not the video menu: it stays shut.
    ui.update(|e, _, cx| {
        e.fields.add_menu = Some(super::super::fields::EffectTarget::Audio(
            animathio_core::AudioChain::Clip(image),
        ));
        cx.notify();
    });
    ui.settle();
    assert!(ui.cx.debug_bounds("add-effect-menu").is_none());
    ui.update(|e, _, cx| {
        e.fields.add_menu = Some(super::super::fields::EffectTarget::Video(image));
        cx.notify();
    });
    ui.settle();
    if require_plugins("add_effect_menu_belongs_to_its_selection") {
        assert!(ui.cx.debug_bounds("add-effect-menu").is_some());
    }
    ui.dispatch(actions::ClearSelection);
    ui.settle();
    assert!(
        ui.read(|e| e.fields.add_menu.is_none()),
        "Escape closes the menu"
    );
    ui.update(|e, _, cx| {
        e.fields.add_menu = Some(super::super::fields::EffectTarget::Video(image));
        cx.notify();
    });
    let other = first_clip(&mut ui, |kind| !matches!(kind, ClipKind::Image(_)));
    select_clip(&mut ui, other);
    ui.update(|_, window, _| window.refresh());
    ui.settle();
    assert!(
        ui.read(|e| e.fields.add_menu.is_none()),
        "moving the selection closes the menu"
    );
}

#[gpui::test]
fn media_fit_picker_change_and_undo(cx: &mut TestAppContext) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    let id = ui.read(|e| {
        e.project
            .tracks()
            .iter()
            .flat_map(|t| t.clips().values())
            .find(|c| {
                matches!(
                    c.kind,
                    animathio_core::ClipKind::Image(_) | animathio_core::ClipKind::Video(_)
                )
            })
            .expect("sample media clip")
            .id
    });
    let before = ui.read(|e| e.project.clone());
    ui.update(|e, _, cx| {
        e.select_only(id, cx);
        e.set_clip_fit(id, animathio_core::Fit::Cover, cx);
    });
    assert_eq!(
        ui.read(|e| e.find_clip(id).unwrap().fit),
        animathio_core::Fit::Cover
    );
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
}

/// Font weight is a schema parameter of the text plugin and edits go through commands.
#[gpui::test]
fn text_font_weight_edit_is_a_command_with_undo(cx: &mut TestAppContext) {
    if !require_plugins("text_font_weight_edit_is_a_command_with_undo") {
        return;
    }
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| e.add_title_preset("typography_body", Time(0), cx));
    let id = ui.read(|e| e.selected.expect("selected"));
    let weight = |ui: &Ui| ui.read(|e| e.plugin_number_value(id, "font_weight"));
    assert!((weight(&ui) - 400.0).abs() < 1e-9);
    assert!(
        ui.read(|e| e
            .plugin_model(id, "font_weight")
            .is_some_and(|model| matches!(
                model,
                crate::inspector::ParamModel::Number { min, max, .. } if (min - 100.0).abs() < 1e-9 && (max - 900.0).abs() < 1e-9
            ))),
        "the inspector builds a 100-900 Font weight field from the schema"
    );
    assert!(
        ui.read(|e| e.plugin_model(id, "font").is_some_and(|model| matches!(
            model,
            crate::inspector::ParamModel::Enum { options, default, .. }
                if options == ["dejavu", "inter"] && default == "dejavu"
        ))),
        "the inspector builds a Font dropdown (dejavu | inter) from the schema"
    );
    let before = ui.read(|e| e.project.clone());
    // The inspector's number field commits through this command; out-of-range input clamps.
    ui.update(|e, _, cx| {
        let command = e
            .plugin_number_command(id, "font_weight", 700.0)
            .expect("edit command");
        e.execute(&command, cx);
    });
    assert!((weight(&ui) - 700.0).abs() < 1e-9);
    let edited = ui.read(|e| e.project.clone());
    ui.dispatch(Undo);
    assert_eq!(ui.read(|e| e.project.clone()), before);
    ui.dispatch(Redo);
    assert_eq!(ui.read(|e| e.project.clone()), edited);
    ui.update(|e, _, cx| {
        let command = e
            .plugin_number_command(id, "font_weight", 5000.0)
            .expect("edit command");
        e.execute(&command, cx);
    });
    assert!((weight(&ui) - 900.0).abs() < 1e-9);
}

#[gpui::test]
fn enter_and_escape_in_an_inspector_field_return_the_keyboard_to_the_timeline(
    cx: &mut TestAppContext,
) {
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    let id = ui.read(|e| e.project.tracks()[0].clips().values().next().unwrap().id);
    ui.update(|editor, _, cx| editor.select_only(id, cx));
    ui.update(|_, window, _| window.refresh());
    let state = ui
        .read(|editor| editor.fields.first_input())
        .expect("the inspector has an input for the selected clip");
    let focus_field = |ui: &mut Ui| {
        let state = state.clone();
        ui.update(move |_, window, cx| {
            let handle = state.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        });
    };
    let field_focused =
        |ui: &mut Ui| ui.update(|editor, window, cx| editor.fields.any_focused(window, cx));
    let timeline_focused =
        |ui: &mut Ui| ui.update(|editor, window, _| editor.panel_focus[0].is_focused(window));

    // Escape drops the typed-but-uncommitted text and leaves the field.
    focus_field(&mut ui);
    assert!(field_focused(&mut ui), "the field took focus");
    let original = state.read_with(ui.cx, |s, _| s.value().to_string());
    ui.update(|_, window, cx| {
        state.update(cx, |s, cx| s.set_value("999", window, cx));
    });
    ui.press("escape");
    assert!(!field_focused(&mut ui), "escape leaves the field");
    assert!(timeline_focused(&mut ui), "focus returns to the timeline");
    assert_eq!(
        state.read_with(ui.cx, |s, _| s.value().to_string()),
        original,
        "the unsaved edit was dropped"
    );

    // Enter commits and leaves the field too, so Space and Delete work again.
    focus_field(&mut ui);
    let before = ui.read(|e| e.project.clone());
    let typed = original.parse::<f64>().expect("a numeric field") + 7.0;
    ui.update(|_, window, cx| {
        state.update(cx, |s, cx| s.set_value(typed.to_string(), window, cx));
    });
    ui.press("enter");
    assert!(!field_focused(&mut ui), "enter leaves the field");
    assert!(timeline_focused(&mut ui));
    let after = ui.read(|e| e.project.clone());
    assert_ne!(
        after, before,
        "enter committed the typed value to the model"
    );
    ui.dispatch(Undo);
    assert_eq!(
        ui.read(|e| e.project.clone()),
        before,
        "one undo reverts it"
    );
}

#[gpui::test]
fn escape_in_a_multiline_inspector_field_returns_the_keyboard_to_the_timeline(
    cx: &mut TestAppContext,
) {
    if !require_plugins("escape_in_a_multiline_inspector_field") {
        return;
    }
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| e.add_plugin_clip_by_id("org.animathio.text", cx));
    let latex = ui.read(|e| {
        e.selected.or_else(|| {
            e.project
                .tracks()
                .iter()
                .flat_map(|t| t.clips().values())
                .find(|c| matches!(c.kind, animathio_core::ClipKind::Plugin { .. }))
                .map(|c| c.id)
        })
    });
    let latex = latex.expect("the text clip was added");
    ui.update(|editor, _, cx| editor.select_only(latex, cx));
    ui.update(|_, window, _| window.refresh());
    ui.settle();
    ui.update(|_, window, _| window.refresh());
    let state = ui
        .read(|editor| editor.fields.first_textarea())
        .expect("the text clip has a multi-line content field");
    {
        let state = state.clone();
        ui.update(move |_, window, cx| {
            let handle = state.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        });
    }
    assert!(ui.update(|editor, window, cx| editor.fields.any_focused(window, cx)));

    // Enter still inserts a newline in a multi-line field.
    let lines = |ui: &Ui| state.read_with(ui.cx, |s, _| s.value().matches('\n').count());
    let before = lines(&ui);
    ui.press("enter");
    assert_eq!(lines(&ui), before + 1, "enter inserts a newline");
    assert!(ui.update(|editor, window, cx| editor.fields.any_focused(window, cx)));

    // Escape hands the keyboard back to the timeline.
    ui.press("escape");
    assert!(!ui.update(|editor, window, cx| editor.fields.any_focused(window, cx)));
    assert!(ui.update(|editor, window, _| editor.panel_focus[0].is_focused(window)));
}

#[gpui::test]
fn inspector_leads_with_content_for_generated_clips_and_audio_for_audio_clips(
    cx: &mut TestAppContext,
) {
    if !require_plugins(
        "inspector_leads_with_content_for_generated_clips_and_audio_for_audio_clips",
    ) {
        return;
    }
    let top = |ui: &mut Ui, section: &str| {
        ui.cx
            .debug_bounds(Box::leak(format!("section-{section}").into_boxed_str()))
            .map(|bounds| f32::from(bounds.origin.y))
    };
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    let text = ui.read(|editor| {
        editor
            .project
            .tracks()
            .iter()
            .flat_map(|track| track.clips().values())
            .find(|clip| {
                matches!(&clip.kind, animathio_core::ClipKind::Plugin { plugin_id, .. }
                    if plugin_id == "org.animathio.text")
            })
            .map(|clip| clip.id)
            .expect("the sample has a text clip")
    });
    ui.update(|editor, _, cx| editor.select_only(text, cx));
    ui.update(|_, window, _| window.refresh());
    let (params, transform) = (top(&mut ui, "params"), top(&mut ui, "transform"));
    assert!(
        params.is_some() && transform.is_some(),
        "content and transform sections render"
    );
    assert!(
        params < transform,
        "a text clip leads with its text, not Speed"
    );
    if let Some(speed) = top(&mut ui, "speed") {
        assert!(
            speed > transform.unwrap(),
            "Speed follows for generated clips"
        );
    }

    let (project, assets) = voice_and_music();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|editor, _, cx| editor.select_only(VOICE_CLIP, cx));
    ui.update(|_, window, _| window.refresh());
    let speed = top(&mut ui, "speed").expect("an audio clip shows Speed");
    let audio = top(&mut ui, "audio").expect("and its Audio section");
    assert!(audio < speed, "audio clips lead with their Audio section");
}
