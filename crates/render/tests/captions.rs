//! Golden frames for the animated caption styles of the text plugin at fixed times. The
//! frames are a pure function of the clip-local time, so preview and export agree.
mod common;
use animathio_core::{ClipId, ClipKind, Scene, SceneClip, Time, TrackId, Transform};
use animathio_render::RenderCtx;
use serde_json::json;
use std::path::Path;

const MS: i64 = 705_600;
const SIZE: (u32, u32) = (480, 270);
const WORDS: &str = "Math is beautiful today";

/// Four words, 0.5 s each, as the transcriber stores them (`start:end` in ticks).
fn timings() -> String {
    (0..4)
        .map(|i| format!("{}:{}", i * 500 * MS, (i + 1) * 500 * MS))
        .collect::<Vec<_>>()
        .join(" ")
}

fn scene(style: &str, extra: &serde_json::Value, time_ms: i64) -> Scene {
    let mut params = json!({
        "text": WORDS, "font_size": 0.09, "color": [255, 255, 255, 255],
        "units": "normalized", "align": "center", "fit_width": 0.9, "fit_height": 0.4,
        "caption_style": style, "words": timings(),
    });
    for (key, value) in extra.as_object().unwrap() {
        params[key] = value.clone();
    }
    Scene {
        canvas: (0, 0),
        clips: vec![SceneClip {
            track: TrackId(0),
            id: ClipId(0),
            kind: ClipKind::Plugin {
                plugin_id: "org.animathio.text".into(),
                params,
            },
            source_time: Time(time_ms * MS),
            fit: animathio_core::Fit::Stretch,
            transform: Transform {
                y: 0.6,
                scale_x: 0.9,
                scale_y: 0.4,
                ..Transform::default()
            },
            params: std::collections::BTreeMap::new(),
            effects: Vec::new(),
            transition: None,
        }],
    }
}

fn render_scene(scene: &Scene) -> animathio_render::Frame {
    let mut ctx = RenderCtx::default();
    common::render(scene, &mut ctx, SIZE).unwrap()
}

fn golden(style: &str, extra: &serde_json::Value, time_ms: i64, name: &str) {
    let actual = render_scene(&scene(style, extra, time_ms));
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.png"));
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
        let output = std::env::temp_dir().join(format!("animathio-{name}-actual.png"));
        actual.save_png(&output).unwrap();
        panic!(
            "{differences} channels exceeded tolerance 2; actual frame: {}",
            output.display()
        );
    }
}

#[test]
fn word_highlight_second_word() {
    golden(
        "word-highlight",
        &json!({}),
        700,
        "caption-word-highlight-700",
    );
}

#[test]
fn karaoke_half_filled_word() {
    golden("karaoke", &json!({}), 1250, "caption-karaoke-1250");
}

#[test]
fn bold_outline_third_word() {
    golden(
        "bold-outline",
        &json!({}),
        1200,
        "caption-bold-outline-1200",
    );
}

#[test]
fn pop_and_one_word_render() {
    golden("pop", &json!({}), 560, "caption-pop-560");
    golden("one-word", &json!({}), 1100, "caption-one-word-1100");
}

#[test]
fn frames_are_pure_functions_of_time() {
    for style in [
        "word-highlight",
        "karaoke",
        "pop",
        "one-word",
        "bold-outline",
    ] {
        let first = render_scene(&scene(style, &json!({}), 900));
        let again = render_scene(&scene(style, &json!({}), 900));
        assert_eq!(first.pixels, again.pixels, "{style} is not deterministic");
        let other = render_scene(&scene(style, &json!({}), 1600));
        assert_ne!(first.pixels, other.pixels, "{style} ignores time");
    }
}

#[test]
fn classic_and_off_match_the_plain_fitted_text() {
    let off = render_scene(&scene("off", &json!({}), 100));
    let classic = render_scene(&scene("classic", &json!({}), 1700));
    assert_eq!(off.pixels, classic.pixels);
}

#[test]
fn stale_or_missing_timings_still_render() {
    // Edited text no longer matches the stored word count: timings are re-spread.
    let respread = render_scene(&scene(
        "word-highlight",
        &json!({ "text": "Maths is so very beautiful today" }),
        1000,
    ));
    assert!(respread.pixels.iter().any(|b| *b != 0));
    // No timings at all: nothing is active but the words are still drawn.
    let untimed = render_scene(&scene("pop", &json!({ "words": "" }), 1000));
    assert!(untimed.pixels.iter().any(|b| *b != 0));
}

#[test]
fn words_per_line_pages_follow_the_active_word() {
    let early = render_scene(&scene("pop", &json!({ "max_words": 1 }), 100));
    let late = render_scene(&scene("pop", &json!({ "max_words": 1 }), 1900));
    assert_ne!(early.pixels, late.pixels);
}
