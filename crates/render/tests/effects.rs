//! Golden PNGs for each builtin effect, applied to a committed fixture image.
use animathio_core::{
    ClipId, ClipKind, EffectId, EffectInstance, Scene, SceneClip, Time, TrackId, Transform,
};
use animathio_render::{RenderCtx, render};
use std::path::Path;

fn render_effect(kind: &str, params: &[(&str, f64)], name: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let bytes = std::fs::read(root.join("fixtures").join("effects.png")).unwrap();

    let mut effect = EffectInstance::basic(EffectId(1), kind);
    for (param, value) in params {
        effect.params.insert((*param).to_owned(), *value);
    }
    let scene = Scene {
        clips: vec![SceneClip {
            track: TrackId(0),
            id: ClipId(0),
            kind: ClipKind::Image(animathio_core::AssetId::of(&bytes)),
            source_time: Time(0),
            transform: Transform::default(),
            params: std::collections::BTreeMap::new(),
            effects: vec![effect],
            transition: None,
        }],
    };

    let mut ctx = RenderCtx::default();
    ctx.set_effect_processor(animathio_plugin_host::test_support::effect_processor());
    ctx.resources
        .insert(animathio_core::AssetId::of(&bytes), bytes)
        .unwrap();
    let actual = render(&scene, &mut ctx, (64, 64)).unwrap();

    let path = root.join("golden").join(format!("{name}.png"));
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
        .zip(actual.pixels.iter())
        .filter(|(a, b)| a.abs_diff(**b) > 2)
        .count();
    if differences != 0 {
        let output = std::env::temp_dir().join(format!("animathio-effects-{name}-actual.png"));
        actual.save_png(&output).unwrap();
        panic!(
            "{differences} channels exceeded tolerance 2; actual frame: {}",
            output.display()
        );
    }
}

#[test]
fn grayscale_effect() {
    render_effect("grayscale", &[("amount", 1.0)], "effects-grayscale");
}

#[test]
fn sepia_effect() {
    render_effect("sepia", &[("amount", 1.0)], "effects-sepia");
}

#[test]
fn invert_effect() {
    render_effect("invert", &[("amount", 1.0)], "effects-invert");
}

#[test]
fn saturate_effect() {
    render_effect("saturate", &[("amount", 2.0)], "effects-saturate");
}

#[test]
fn brightness_effect() {
    render_effect("brightness", &[("amount", 0.7)], "effects-brightness");
}

#[test]
fn contrast_effect() {
    render_effect("contrast", &[("amount", 1.5)], "effects-contrast");
}

#[test]
fn blur_effect() {
    render_effect("blur", &[("radius", 0.05)], "effects-blur");
}

#[test]
fn opacity_effect() {
    render_effect("opacity", &[("amount", 0.5)], "effects-opacity");
}

/// Test double: `broken` fails like a missing plugin, `invert` flips RGB.
struct Stub;

impl animathio_core::EffectProcessor for Stub {
    fn apply(
        &mut self,
        effect: &EffectInstance,
        _time: Time,
        _size: (u32, u32),
        pixels: &mut Vec<u8>,
    ) -> Result<(), String> {
        if effect.effect == "broken" {
            return Err(format!("plugin {} is not loaded", effect.plugin));
        }
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel[0] = 255 - pixel[0];
            pixel[1] = 255 - pixel[1];
            pixel[2] = 255 - pixel[2];
        }
        Ok(())
    }
}

fn effect_scene(bytes: &[u8], effects: Vec<EffectInstance>) -> Scene {
    Scene {
        clips: vec![SceneClip {
            track: TrackId(0),
            id: ClipId(7),
            kind: ClipKind::Image(animathio_core::AssetId::of(bytes)),
            source_time: Time(0),
            transform: Transform::default(),
            params: std::collections::BTreeMap::new(),
            effects,
            transition: None,
        }],
    }
}

#[test]
fn preview_passes_failing_effects_through_while_export_render_fails() {
    let bytes = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("effects.png"),
    )
    .unwrap();
    let mut ctx = RenderCtx::default();
    ctx.set_effect_processor(Stub);
    ctx.resources
        .insert(animathio_core::AssetId::of(&bytes), bytes.clone())
        .unwrap();
    let broken = EffectInstance::new(
        EffectId(1),
        "org.example.missing",
        "broken",
        std::collections::BTreeMap::default(),
    );
    let invert = EffectInstance::new(
        EffectId(2),
        "org.example.ok",
        "invert",
        std::collections::BTreeMap::default(),
    );
    let size = (64, 64);

    let failing = effect_scene(&bytes, vec![broken.clone()]);
    assert!(matches!(
        render(&failing, &mut ctx, size),
        Err(animathio_render::Error::Effect(_))
    ));
    let preview = animathio_render::render_preview(&failing, &mut ctx, size).unwrap();
    let plain = render(&effect_scene(&bytes, Vec::new()), &mut ctx, size).unwrap();
    assert_eq!(preview.frame, plain);
    assert_eq!(preview.effect_failures.len(), 1);
    assert_eq!(preview.effect_failures[0].0, ClipId(7));
    assert!(preview.effect_failures[0].1.contains("org.example.missing"));

    // Working effects in the same chain still apply.
    let mixed = effect_scene(&bytes, vec![broken, invert.clone()]);
    let preview = animathio_render::render_preview(&mixed, &mut ctx, size).unwrap();
    let inverted = render(&effect_scene(&bytes, vec![invert]), &mut ctx, size).unwrap();
    assert_eq!(preview.frame, inverted);
    assert_eq!(preview.effect_failures.len(), 1);

    // A clean scene reports nothing.
    let clean = animathio_render::render_preview(&effect_scene(&bytes, Vec::new()), &mut ctx, size)
        .unwrap();
    assert_eq!(clean.effect_failures.len(), 0);

    // Without any processor the preview also degrades instead of going black.
    let mut bare = RenderCtx::default();
    bare.resources
        .insert(animathio_core::AssetId::of(&bytes), bytes)
        .unwrap();
    let preview = animathio_render::render_preview(&failing, &mut bare, size).unwrap();
    assert_eq!(preview.frame, plain);
}
