//! The Classic caption style end to end: generated cues become a text clip over a translucent
//! bar, and the rendered frame keeps every glyph (counters, diacritics, symbols), both lines
//! of a multi-line cue and the bar behind them. Frames are compared with committed goldens
//! (`UPDATE_GOLDEN=1` regenerates them).
use animathio_control::{CaptionStyleParams, apply_cues};
use animathio_core::{History, Project, TICKS_PER_SECOND, Time};
use animathio_transcribe::Cue;
use std::path::Path;

const MS: i64 = TICKS_PER_SECOND / 1000;
const SIZE: (u32, u32) = (640, 360);

fn frame(text: &str) -> animathio_render::Frame {
    let mut project = Project::default();
    let mut history = History::default();
    let cues = [Cue::new(Time(1000 * MS), Time(4000 * MS), text)];
    apply_cues(
        &mut project,
        &mut history,
        "en".into(),
        &cues,
        &CaptionStyleParams::default(),
    )
    .unwrap();
    let scene = animathio_core::evaluate(&project, Time(2500 * MS));
    let scene = animathio_plugin_host::test_support::resolve(&scene, SIZE);
    let mut ctx = animathio_render::RenderCtx::default();
    ctx.set_effect_processor(animathio_plugin_host::test_support::effect_processor());
    animathio_render::render(&scene, &mut ctx, SIZE).unwrap()
}

fn pixel(frame: &animathio_render::Frame, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * frame.size.0 + x) * 4) as usize;
    [
        frame.pixels[i],
        frame.pixels[i + 1],
        frame.pixels[i + 2],
        frame.pixels[i + 3],
    ]
}

/// Bright (text) pixels: `(x, y)` where the red channel is clearly above the bar colour.
fn glyph_pixels(frame: &animathio_render::Frame) -> Vec<(u32, u32)> {
    (0..frame.size.1)
        .flat_map(|y| (0..frame.size.0).map(move |x| (x, y)))
        .filter(|&(x, y)| pixel(frame, x, y)[0] > 120)
        .collect()
}

fn golden(frame: &animathio_render::Frame, name: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.png"));
    if std::env::var("UPDATE_GOLDEN").as_deref() == Ok("1") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        frame.save_png(&path).unwrap();
    }
    let expected = image::open(&path)
        .unwrap_or_else(|e| panic!("{}: {e}; regenerate with UPDATE_GOLDEN=1", path.display()))
        .to_rgba8();
    assert_eq!(expected.dimensions(), frame.size);
    assert!(
        expected
            .as_raw()
            .iter()
            .zip(&frame.pixels)
            .all(|(a, b)| a.abs_diff(*b) <= 2),
        "{name}: frame changed; actual differs from the golden"
    );
}

/// Contiguous vertical bands of rows holding glyph pixels.
fn bands(glyphs: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let mut rows: Vec<u32> = glyphs.iter().map(|g| g.1).collect();
    rows.sort_unstable();
    rows.dedup();
    let mut bands: Vec<(u32, u32)> = Vec::new();
    for row in rows {
        match bands.last_mut() {
            Some(last) if row <= last.1 + 2 => last.1 = row,
            _ => bands.push((row, row)),
        }
    }
    bands
}

#[test]
fn classic_caption_draws_a_bar_and_a_single_line_of_text() {
    let frame = frame("Hello, brave world 8 @ %");
    // The translucent bar (rgba 22, 28, 48, 180) blends over the transparent black canvas.
    let bar = pixel(&frame, 70, 318);
    assert!(bar[3] > 150 && bar[2] > bar[0], "bar missing: {bar:?}");
    let glyphs = glyph_pixels(&frame);
    assert!(glyphs.len() > 400, "text missing: {} pixels", glyphs.len());
    assert_eq!(bands(&glyphs).len(), 1);
    // The text stays inside the bar (84 % of the width) and is centred on it.
    let (min_x, max_x) = glyphs
        .iter()
        .fold((u32::MAX, 0), |(l, r), g| (l.min(g.0), r.max(g.0)));
    assert!(min_x > SIZE.0 * 8 / 100 && max_x < SIZE.0 * 92 / 100);
    assert!((i64::from(min_x + max_x) - i64::from(SIZE.0)).abs() < 12);
    golden(&frame, "caption-classic-single");
}

#[test]
fn classic_caption_renders_multiline_and_non_ascii_glyphs() {
    let text = "Élève naïve — cañón façade\nΣπ ≈ 3,14 · Привет ∞ “ok”";
    let frame = frame(text);
    let glyphs = glyph_pixels(&frame);
    // Two lines come out as two separate bands of rows.
    let lines = bands(&glyphs);
    assert_eq!(lines.len(), 2, "bands: {lines:?}");
    assert!(lines[0].1 < lines[1].0);
    // Nothing leaks out of the bar.
    let (min_x, max_x) = glyphs
        .iter()
        .fold((u32::MAX, 0), |(l, r), g| (l.min(g.0), r.max(g.0)));
    assert!(min_x > SIZE.0 * 8 / 100 && max_x < SIZE.0 * 92 / 100);
    // Accents and non-Latin letters are real outlines, not blanks or identical boxes: the
    // frame differs from the same line with plain ASCII stand-ins.
    let plain = self::frame("Eleve naive - canon facade\nSp = 3,14 . Privet inf \"ok\"");
    assert_ne!(frame.pixels, plain.pixels);
    golden(&frame, "caption-classic-multiline-unicode");
}

#[test]
fn classic_caption_counters_stay_open() {
    // A large caption of counter letters: the hole of `O`, `8`, `B` and `@` is still the
    // bar colour inside the glyph, not filled white (winding preserved).
    let frame = frame("O 8 B @");
    let glyphs = glyph_pixels(&frame);
    let (min_x, max_x) = glyphs
        .iter()
        .fold((u32::MAX, 0), |(l, r), g| (l.min(g.0), r.max(g.0)));
    let (min_y, max_y) = glyphs
        .iter()
        .fold((u32::MAX, 0), |(l, r), g| (l.min(g.1), r.max(g.1)));
    // The first glyph `O`: its centre pixel is not part of the glyph.
    let letter_width = (max_x - min_x) / 7;
    let (cx, cy) = (min_x + letter_width / 2, (min_y + max_y) / 2);
    assert!(
        pixel(&frame, cx, cy)[0] < 100,
        "counter of O is filled: {:?}",
        pixel(&frame, cx, cy)
    );
    golden(&frame, "caption-classic-counters");
}

#[test]
fn classic_caption_survives_scripts_the_embedded_font_lacks() {
    // DejaVu Sans has no CJK: those characters must not break the caption or hide the rest.
    let with_cjk = frame("日本語 ok");
    let without = frame("ok");
    let (a, b) = (glyph_pixels(&with_cjk), glyph_pixels(&without));
    assert!(
        !b.is_empty() && a.len() >= b.len() / 2,
        "{} vs {}",
        a.len(),
        b.len()
    );
}
