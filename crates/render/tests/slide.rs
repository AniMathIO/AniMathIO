//! The clipped slide effect and the text plugin's per-character slide through the real
//! renderer: any source slides out of its own bounds, per-character slides keep the layout the
//! text plugin produced, and both are pure functions of the scene.
mod common;
use animathio_core::{
    AssetId, ClipId, ClipKind, EffectId, EffectInstance, Fit, ParamValue, Scene, SceneClip, Time,
    TrackId, Transform,
};
use animathio_render::{Frame, RenderCtx};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;

const SIZE: (u32, u32) = (480, 270);

fn slide(direction: f64, progress: f64) -> EffectInstance {
    EffectInstance::new(
        EffectId(1),
        "org.animathio.effects-basic",
        "slide",
        BTreeMap::from([
            ("progress".to_owned(), progress),
            ("direction".to_owned(), direction),
            ("clip".to_owned(), 1.0),
        ]),
    )
}

fn clip(kind: ClipKind, fit: Fit, effects: Vec<EffectInstance>) -> Scene {
    Scene {
        canvas: (0, 0),
        background: [0; 4],
        clips: vec![SceneClip {
            track: TrackId(0),
            id: ClipId(0),
            kind,
            source_time: Time(0),
            fit,
            transform: Transform::default(),
            params: BTreeMap::new(),
            effects,
            transition: None,
        }],
    }
}

/// Default text clips use the wrapped, left-aligned layout; the long string wraps onto two
/// lines at this canvas size.
fn text(effects: Vec<EffectInstance>) -> Scene {
    clip(
        ClipKind::Plugin {
            plugin_id: "org.animathio.text".into(),
            params: json!({
                "text": "Math slides in letter by letter across lines",
                "font_size": 0.16, "color": [20, 30, 90, 255],
                "units": "normalized", "align": "left",
            }),
        },
        Fit::Stretch,
        effects,
    )
}

/// The text with the per-character slide parameters (the way the presets animate them).
fn char_text(progress: f64, direction: f64, clip: bool) -> Scene {
    let mut scene = text(Vec::new());
    scene.clips[0].params = BTreeMap::from([
        ("char_slide".to_owned(), ParamValue::Number(progress)),
        (
            "char_slide_direction".to_owned(),
            ParamValue::Number(direction),
        ),
        ("char_slide_stagger".to_owned(), ParamValue::Number(0.5)),
        (
            "char_slide_clip".to_owned(),
            ParamValue::Number(f64::from(u8::from(clip))),
        ),
        ("char_slide_fade".to_owned(), ParamValue::Number(1.0)),
    ]);
    scene
}

/// The test image drawn at 60 % size in the middle, so its own edge is well inside the
/// canvas and the cut at that edge is visible.
fn image(effects: Vec<EffectInstance>) -> (Scene, Vec<u8>) {
    let bytes =
        std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/effects.png"))
            .unwrap();
    let mut scene = clip(ClipKind::Image(AssetId::of(&bytes)), Fit::Stretch, effects);
    scene.clips[0].transform = Transform {
        scale_x: 0.6,
        scale_y: 0.6,
        ..Transform::default()
    };
    (scene, bytes)
}

fn render(scene: &Scene) -> Frame {
    let mut ctx = RenderCtx::default();
    common::render(scene, &mut ctx, SIZE).unwrap()
}

fn golden(actual: &Frame, name: &str) {
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
            "{differences} channels exceeded tolerance 2; actual: {}",
            output.display()
        );
    }
}

fn ink(frame: &Frame) -> usize {
    frame
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[3] > 0)
        .count()
}

#[test]
fn a_resting_slide_is_free_and_leaves_the_frame_bit_identical() {
    // The slide declares itself an identity at progress 1, so the clip never takes the
    // rasterize-and-filter layer path: the frame is exactly the plain one.
    let plain = render(&text(Vec::new()));
    assert!(ink(&plain) > 0);
    for direction in [1.0, 2.0, 3.0, 4.0] {
        let rest = render(&text(vec![slide(direction, 1.0)]));
        assert_eq!(rest.pixels, plain.pixels, "direction {direction}");
    }
}

#[test]
fn slides_start_hidden_and_grow_toward_the_resting_frame() {
    let plain = ink(&render(&text(Vec::new())));
    assert_eq!(ink(&render(&text(vec![slide(1.0, 0.0)]))), 0);
    let some = ink(&render(&text(vec![slide(1.0, 0.5)])));
    assert!(some > 0 && some < plain, "{some} of {plain}");
    // The text plugin's per-character slide: hidden at 0, part way at 0.5, the plain
    // text at rest (up to anti-aliasing of the same outlines).
    assert_eq!(ink(&render(&char_text(0.0, 1.0, true))), 0);
    let part = ink(&render(&char_text(0.5, 1.0, true)));
    assert!(part > 0 && part < plain, "{part} of {plain}");
    assert_eq!(
        render(&char_text(1.0, 1.0, true)).pixels,
        render(&text(Vec::new())).pixels
    );
}

#[test]
fn glyphs_in_place_match_the_plain_text_layout() {
    // Just short of rest every glyph is within a hair of its resting position, so the
    // per-glyph outlines must reproduce the renderer's own text layout.
    let plain = render(&text(Vec::new()));
    let near = render(&char_text(0.999_999, 1.0, true));
    // Compare premultiplied values: the colour of fully transparent pixels is meaningless.
    let premult = |p: &[u8; 4], c: usize| u16::from(p[c]) * u16::from(p[3]) / 255;
    let (mut worst, mut total) = (0_u16, 0_u64);
    for (a, b) in near
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(plain.pixels.as_chunks::<4>().0)
    {
        let d = (0..3)
            .map(|c| premult(a, c).abs_diff(premult(b, c)))
            .chain([u16::from(a[3].abs_diff(b[3]))])
            .max()
            .unwrap();
        worst = worst.max(d);
        total += u64::from(d);
    }
    // Same outlines, rasterized as paths instead of glyph runs: edge pixels may differ by
    // a fraction of coverage, and the mean difference stays tiny.
    assert!(
        worst <= 128,
        "per-glyph outlines drift from the layout by {worst}"
    );
    assert!(total < 40 * ink(&plain) as u64, "mean drift {total}");
    assert!(
        ink(&near).abs_diff(ink(&plain)) * 100 < ink(&plain),
        "{} vs {}",
        ink(&near),
        ink(&plain)
    );
}

#[test]
fn character_slide_rises_glyph_by_glyph_where_the_clipped_slide_moves_the_block() {
    let block = render(&text(vec![slide(1.0, 0.5)]));
    let letters = render(&char_text(0.55, 4.0, true));
    let sideways = render(&char_text(0.55, 1.0, true));
    assert_ne!(block.pixels, letters.pixels);
    assert_ne!(letters.pixels, sideways.pixels);
    golden(&letters, "slide-text-characters-rise");
    golden(&sideways, "slide-text-characters-left");
    golden(&block, "slide-text-block-left-half");
}

#[test]
fn clipped_slide_works_on_media_and_is_cut_at_the_bounds() {
    let (rest, bytes) = image(Vec::new());
    let mut ctx = RenderCtx::default();
    ctx.resources
        .insert(AssetId::of(&bytes), bytes.clone())
        .unwrap();
    let plain = common::render(&rest, &mut ctx, SIZE).unwrap();
    let (half, _) = image(vec![slide(3.0, 0.5)]);
    let mut ctx = RenderCtx::default();
    ctx.resources.insert(AssetId::of(&bytes), bytes).unwrap();
    let frame = common::render(&half, &mut ctx, SIZE).unwrap();
    // Sliding down from the top edge: only the upper half of the bounds shows content, and
    // nothing leaks outside the image's own rectangle.
    let rows: Vec<usize> = (0..SIZE.1 as usize)
        .filter(|&y| {
            (0..SIZE.0 as usize).any(|x| frame.pixels[(y * SIZE.0 as usize + x) * 4 + 3] > 0)
        })
        .collect();
    let plain_rows: Vec<usize> = (0..SIZE.1 as usize)
        .filter(|&y| {
            (0..SIZE.0 as usize).any(|x| plain.pixels[(y * SIZE.0 as usize + x) * 4 + 3] > 0)
        })
        .collect();
    assert_eq!(rows.first(), plain_rows.first());
    assert!(rows.last().unwrap() < plain_rows.last().unwrap());
    golden(&frame, "slide-image-clipped-top-half");
}

/// Cost of a clip that carries a resting effect, at 4K. Run with
/// `cargo test -p animathio-render --release --test slide -- --ignored --nocapture`.
/// "slide at rest" is declared an identity (direct path); "opacity 1" is an effect the plugin
/// does not declare an identity, so it pays the layer round trip the resting slide used to.
#[test]
#[ignore = "timing measurement"]
fn resting_effect_cost_at_4k() {
    const UHD: (u32, u32) = (3840, 2160);
    let opacity_one = EffectInstance::new(
        EffectId(2),
        "org.animathio.effects-basic",
        "opacity",
        BTreeMap::from([("amount".to_owned(), 1.0)]),
    );
    let time = |label: &str, scene: Scene| {
        let mut ctx = RenderCtx::default();
        common::render(&scene, &mut ctx, UHD).unwrap();
        let mut best = f64::MAX;
        for _ in 0..5 {
            let start = std::time::Instant::now();
            common::render(&scene, &mut ctx, UHD).unwrap();
            best = best.min(start.elapsed().as_secs_f64() * 1000.0);
        }
        println!("{label:<28} {best:8.1} ms");
    };
    time("no effect", text(Vec::new()));
    time("slide at rest (identity)", text(vec![slide(1.0, 1.0)]));
    time("opacity 1 (layer + wasm)", text(vec![opacity_one]));
}
