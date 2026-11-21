//! The clipped / per-character slide effect through the real renderer: any source slides
//! out of its own bounds, per-character slides keep the layout the text plugin produced,
//! and both are pure functions of the scene.
mod common;
use animathio_core::{
    AssetId, ClipId, ClipKind, EffectId, EffectInstance, Fit, Scene, SceneClip, Time, TrackId,
    Transform,
};
use animathio_render::{Frame, RenderCtx};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;

const SIZE: (u32, u32) = (480, 270);

fn slide(direction: f64, characters: bool, progress: f64) -> EffectInstance {
    EffectInstance::new(
        EffectId(1),
        "org.animathio.effects-basic",
        "slide",
        BTreeMap::from([
            ("progress".to_owned(), progress),
            ("direction".to_owned(), direction),
            ("characters".to_owned(), f64::from(u8::from(characters))),
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

fn image(effects: Vec<EffectInstance>) -> (Scene, Vec<u8>) {
    let bytes =
        std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/effects.png"))
            .unwrap();
    (
        clip(ClipKind::Image(AssetId::of(&bytes)), Fit::Stretch, effects),
        bytes,
    )
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
fn character_slide_at_rest_keeps_the_exact_wrapped_layout() {
    let plain = render(&text(Vec::new()));
    assert!(ink(&plain) > 0);
    for direction in [1.0, 2.0, 3.0, 4.0] {
        let rest = render(&text(vec![slide(direction, true, 1.0)]));
        // The effect path rasterizes into a layer first, so compare premultiplied values.
        let premult = |p: &[u8; 4], c: usize| u16::from(p[c]) * u16::from(p[3]) / 255;
        let worst = rest
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .zip(plain.pixels.as_chunks::<4>().0)
            .map(|(a, b)| {
                (0..3)
                    .map(|c| premult(a, c).abs_diff(premult(b, c)))
                    .chain([u16::from(a[3].abs_diff(b[3]))])
                    .max()
                    .unwrap()
            })
            .max()
            .unwrap();
        assert!(worst <= 2, "direction {direction}: off by {worst}");
    }
}

#[test]
fn slides_start_hidden_and_grow_toward_the_resting_frame() {
    let plain = ink(&render(&text(Vec::new())));
    for characters in [false, true] {
        assert_eq!(ink(&render(&text(vec![slide(1.0, characters, 0.0)]))), 0);
        let some = ink(&render(&text(vec![slide(1.0, characters, 0.5)])));
        assert!(
            some > 0 && some < plain,
            "characters {characters}: {some} of {plain}"
        );
    }
}

#[test]
fn character_slide_goes_glyph_by_glyph_where_a_plain_slide_moves_the_block() {
    let block = render(&text(vec![slide(1.0, false, 0.5)]));
    let letters = render(&text(vec![slide(1.0, true, 0.5)]));
    assert_ne!(block.pixels, letters.pixels);
    golden(&letters, "slide-text-characters-left-half");
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
    let (half, _) = image(vec![slide(3.0, false, 0.5)]);
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
