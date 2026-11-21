//! The text plugin's per-character slide (`char_slide*` parameters): the glyph units come from
//! the shaped text, so a letter keeps its dot or accent, lines stay separate, the stagger uses
//! normalized positions and any resolution animates the same way.
use animathio_core::{
    ClipId, ClipKind, CoordinateSpace, ParamValue, PathVerb, Scene, SceneClip, Time, TrackId,
    Transform, VectorElement,
};
use animathio_plugin_host::test_support;
use serde_json::json;
use std::collections::BTreeMap;

/// How the text is laid out: the wrapped default, a fitted title or the single-line pixel
/// layout. Each has its own glyph source.
#[derive(Clone, Copy, Debug)]
enum Layout {
    Wrapped,
    Fitted,
    Pixels,
}

fn scene(text: &str, layout: Layout, slide: &[(&str, f64)]) -> Scene {
    let mut params = json!({
        "text": text, "font_size": 0.12, "color": [200, 100, 50, 255], "align": "left",
        "units": "normalized",
    });
    match layout {
        Layout::Wrapped => {}
        Layout::Fitted => {
            params["fit_width"] = json!(0.9);
            params["fit_height"] = json!(0.5);
        }
        Layout::Pixels => {
            params["units"] = json!("pixels");
            params["font_size"] = json!(60);
        }
    }
    Scene {
        canvas: (0, 0),
        background: [0; 4],
        clips: vec![SceneClip {
            id: ClipId(1),
            track: TrackId(1),
            kind: ClipKind::Plugin {
                plugin_id: "org.animathio.text".into(),
                params,
            },
            source_time: Time(0),
            fit: animathio_core::Fit::Stretch,
            transform: Transform::default(),
            params: slide
                .iter()
                .map(|(name, value)| ((*name).to_owned(), ParamValue::Number(*value)))
                .collect::<BTreeMap<_, _>>(),
            effects: Vec::new(),
            transition: None,
        }],
    }
}

/// One glyph unit as drawn: contours, alpha and every point.
struct Unit {
    contours: usize,
    alpha: u8,
    points: Vec<(f64, f64)>,
}

fn elements(
    text: &str,
    layout: Layout,
    slide: &[(&str, f64)],
    size: (u32, u32),
) -> Vec<VectorElement> {
    let resolved = test_support::resolve(&scene(text, layout, slide), size);
    let ClipKind::Vector(vector) = &resolved.clips[0].kind else {
        panic!("component failed: {:?}", resolved.clips[0].kind)
    };
    assert!(matches!(
        vector.coordinate_space,
        CoordinateSpace::Normalized | CoordinateSpace::Canvas
    ));
    vector.elements.clone()
}

fn units(text: &str, layout: Layout, slide: &[(&str, f64)], size: (u32, u32)) -> Vec<Unit> {
    elements(text, layout, slide, size)
        .into_iter()
        .map(|element| {
            let VectorElement::FillPath { path, color } = element else {
                panic!("expected filled glyph paths while sliding")
            };
            let mut points = Vec::new();
            let mut contours = 0;
            for verb in &path {
                match verb {
                    PathVerb::MoveTo(p) => {
                        contours += 1;
                        points.push((p.x, p.y));
                    }
                    PathVerb::LineTo(p) | PathVerb::QuadTo(_, p) | PathVerb::CubicTo(_, _, p) => {
                        points.push((p.x, p.y));
                    }
                    PathVerb::Close => {}
                }
            }
            Unit {
                contours,
                alpha: color.a,
                points,
            }
        })
        .collect()
}

const HD: (u32, u32) = (1920, 1080);
/// Half way, no stagger, no fade, no mask: every unit is at the same state.
const SLIDING: [(&str, f64); 5] = [
    ("char_slide", 0.5),
    ("char_slide_direction", 1.0),
    ("char_slide_stagger", 0.0),
    ("char_slide_clip", 0.0),
    ("char_slide_fade", 0.0),
];

#[test]
fn at_rest_the_text_is_untouched_in_every_layout() {
    for layout in [Layout::Wrapped, Layout::Fitted, Layout::Pixels] {
        let rest = elements("Hello", layout, &[("char_slide", 1.0)], HD);
        let none = elements("Hello", layout, &[], HD);
        assert_eq!(rest, none, "{layout:?}");
        // The wrapped layout stays the renderer's own text run.
        if matches!(layout, Layout::Wrapped) {
            assert!(matches!(rest[0], VectorElement::TextRun(_)));
        }
    }
}

#[test]
fn a_dotted_letter_moves_with_its_dot_and_neighbours_separately() {
    for layout in [Layout::Wrapped, Layout::Fitted, Layout::Pixels] {
        let units = units("in", layout, &SLIDING, HD);
        assert_eq!(units.len(), 2, "{layout:?}: one unit per glyph");
        assert_eq!(
            units[0].contours, 2,
            "{layout:?}: the i is its stem and its dot"
        );
        assert_eq!(units[1].contours, 1, "{layout:?}: the n");
    }
}

#[test]
fn accents_and_combining_marks_stay_with_their_base_letter() {
    for layout in [Layout::Wrapped, Layout::Fitted, Layout::Pixels] {
        for text in ["é", "e\u{301}", "ae\u{301}"] {
            let expected = if text.starts_with('a') { 2 } else { 1 };
            let units = units(text, layout, &SLIDING, HD);
            assert_eq!(units.len(), expected, "{layout:?} {text:?}");
            // The last unit carries the accent as an extra contour on the letter.
            assert!(units.last().unwrap().contours >= 2, "{layout:?} {text:?}");
        }
    }
}

#[test]
fn each_cjk_character_is_its_own_unit() {
    for layout in [Layout::Wrapped, Layout::Fitted, Layout::Pixels] {
        let units = units("日本語", layout, &SLIDING, HD);
        assert_eq!(units.len(), 3, "{layout:?}");
        assert!(units.iter().all(|u| !u.points.is_empty()), "{layout:?}");
    }
}

#[test]
fn a_ligature_is_one_unit_when_the_layout_forms_it() {
    // The renderer's layout applies the font's `fi` ligature; the plugin sees it as one glyph.
    let wrapped = units("office", Layout::Wrapped, &SLIDING, HD);
    let letters = "office".chars().count();
    assert!(
        wrapped.len() < letters,
        "{} units for {letters} letters",
        wrapped.len()
    );
}

#[test]
fn lines_stay_separate_and_the_stagger_follows_reading_order() {
    // Far enough along that every glyph has started (a glyph that has not is not drawn).
    let slide = [
        ("char_slide", 0.7),
        ("char_slide_direction", 4.0),
        ("char_slide_stagger", 0.5),
        ("char_slide_clip", 0.0),
        ("char_slide_fade", 1.0),
    ];
    for layout in [Layout::Wrapped, Layout::Fitted] {
        let units = units("ab\ncd", layout, &slide, HD);
        assert_eq!(units.len(), 4, "{layout:?}");
        // Fade ramps with each glyph's own progress: earlier glyphs are further along.
        let alphas: Vec<u8> = units.iter().map(|u| u.alpha).collect();
        assert!(
            alphas.windows(2).all(|w| w[0] >= w[1]),
            "{layout:?}: reading order {alphas:?}"
        );
        assert!(alphas[0] > alphas[3], "{layout:?}: {alphas:?}");
        // The second line is below the first.
        let top = |u: &Unit| u.points.iter().map(|p| p.1).fold(f64::MAX, f64::min);
        assert!(top(&units[2]) > top(&units[1]), "{layout:?}");
    }
}

#[test]
fn a_negative_stagger_starts_with_the_last_glyph() {
    let slide = |stagger: f64| {
        [
            ("char_slide", 0.7),
            ("char_slide_direction", 1.0),
            ("char_slide_stagger", stagger),
            ("char_slide_clip", 0.0),
            ("char_slide_fade", 1.0),
        ]
    };
    let forward: Vec<u8> = units("abcd", Layout::Wrapped, &slide(0.5), HD)
        .iter()
        .map(|u| u.alpha)
        .collect();
    let reverse: Vec<u8> = units("abcd", Layout::Wrapped, &slide(-0.5), HD)
        .iter()
        .map(|u| u.alpha)
        .collect();
    assert!(forward[0] > forward[3], "{forward:?}");
    assert!(reverse[0] < reverse[3], "{reverse:?}");
    let mut mirrored = reverse.clone();
    mirrored.reverse();
    assert_eq!(forward, mirrored);
}

#[test]
fn timing_and_geometry_do_not_depend_on_the_resolution() {
    let slide = [
        ("char_slide", 0.55),
        ("char_slide_direction", 4.0),
        ("char_slide_stagger", 0.6),
        ("char_slide_clip", 1.0),
        ("char_slide_fade", 1.0),
    ];
    for layout in [Layout::Wrapped, Layout::Fitted] {
        let hd = units("Slide in\nline two", layout, &slide, HD);
        let uhd = units("Slide in\nline two", layout, &slide, (3840, 2160));
        assert_eq!(hd.len(), uhd.len(), "{layout:?}");
        assert_eq!(
            hd.iter().map(|u| u.alpha).collect::<Vec<_>>(),
            uhd.iter().map(|u| u.alpha).collect::<Vec<_>>(),
            "{layout:?}: the same glyph timing"
        );
        for (a, b) in hd.iter().zip(&uhd) {
            assert_eq!(a.contours, b.contours, "{layout:?}");
            // Normalized coordinates agree to a fraction of an HD pixel.
            for (p, q) in a.points.iter().zip(&b.points) {
                assert!(
                    (p.0 - q.0).abs() < 0.002 && (p.1 - q.1).abs() < 0.002,
                    "{layout:?}: {p:?} vs {q:?}"
                );
            }
        }
    }
}

#[test]
fn the_mask_cuts_each_glyph_at_its_own_line_band() {
    let slide = |clip: f64| {
        [
            ("char_slide", 0.4),
            ("char_slide_direction", 4.0),
            ("char_slide_stagger", 0.0),
            ("char_slide_clip", clip),
            ("char_slide_fade", 0.0),
        ]
    };
    let max_y = |units: &[Unit]| {
        units
            .iter()
            .flat_map(|u| u.points.iter().map(|p| p.1))
            .fold(f64::MIN, f64::max)
    };
    for layout in [Layout::Wrapped, Layout::Fitted, Layout::Pixels] {
        let free = units("Hxg", layout, &slide(0.0), HD);
        let masked = units("Hxg", layout, &slide(1.0), HD);
        // Rising from below: unmasked glyphs hang below the line; masked ones are cut off at
        // the bottom of the band, and fewer or equal pieces remain.
        assert!(
            max_y(&masked) < max_y(&free) - 0.01,
            "{layout:?}: {} vs {}",
            max_y(&masked),
            max_y(&free)
        );
        assert!(masked.len() <= free.len(), "{layout:?}");
        assert!(
            !masked.is_empty(),
            "{layout:?}: the upper part of the glyphs shows"
        );
    }
}

#[test]
fn hidden_and_resting_states_bound_the_animation() {
    for layout in [Layout::Wrapped, Layout::Fitted, Layout::Pixels] {
        let hidden = units(
            "Hi",
            layout,
            &[
                ("char_slide", 0.0),
                ("char_slide_fade", 1.0),
                ("char_slide_clip", 1.0),
            ],
            HD,
        );
        assert!(
            hidden.is_empty(),
            "{layout:?}: nothing shows before the first glyph starts"
        );
    }
}
