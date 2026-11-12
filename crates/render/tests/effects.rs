//! Golden PNGs for each builtin effect, applied to a committed fixture image.
use animathio_core::{
    ClipId, ClipKind, EffectId, EffectInstance, EffectKind, Scene, SceneClip, Time, TrackId,
    Transform,
};
use animathio_render::{RenderCtx, render};
use std::path::Path;

fn render_effect(kind: EffectKind, params: &[(&str, f64)], name: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let bytes = std::fs::read(root.join("fixtures").join("effects.png")).unwrap();

    let mut effect = EffectInstance::new(EffectId(1), kind);
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
        }],
    };

    let mut ctx = RenderCtx::default();
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
    render_effect(
        EffectKind::Grayscale,
        &[("amount", 1.0)],
        "effects-grayscale",
    );
}

#[test]
fn sepia_effect() {
    render_effect(EffectKind::Sepia, &[("amount", 1.0)], "effects-sepia");
}

#[test]
fn invert_effect() {
    render_effect(EffectKind::Invert, &[("amount", 1.0)], "effects-invert");
}

#[test]
fn saturate_effect() {
    render_effect(EffectKind::Saturate, &[("amount", 2.0)], "effects-saturate");
}

#[test]
fn brightness_effect() {
    render_effect(
        EffectKind::Brightness,
        &[("amount", 0.7)],
        "effects-brightness",
    );
}

#[test]
fn contrast_effect() {
    render_effect(EffectKind::Contrast, &[("amount", 1.5)], "effects-contrast");
}

#[test]
fn blur_effect() {
    render_effect(EffectKind::Blur, &[("radius", 0.05)], "effects-blur");
}

#[test]
fn opacity_effect() {
    render_effect(EffectKind::Opacity, &[("amount", 0.5)], "effects-opacity");
}
