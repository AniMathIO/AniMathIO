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
        canvas: (0, 0),
        clips: vec![SceneClip {
            track: TrackId(0),
            id: ClipId(0),
            kind: ClipKind::Image(animathio_core::AssetId::of(&bytes)),
            source_time: Time(0),
            fit: animathio_core::Fit::Stretch,
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

/// Counts effect executions of [`Counting`].
struct Counting(std::sync::Arc<std::sync::atomic::AtomicUsize>);

impl animathio_core::EffectProcessor for Counting {
    fn apply(
        &mut self,
        effect: &EffectInstance,
        _time: Time,
        _size: (u32, u32),
        _pixels: &mut Vec<u8>,
    ) -> Result<(), String> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(format!("{} unavailable", effect.plugin))
    }
}

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
        canvas: (0, 0),
        clips: vec![SceneClip {
            track: TrackId(0),
            id: ClipId(7),
            kind: ClipKind::Image(animathio_core::AssetId::of(bytes)),
            source_time: Time(0),
            fit: animathio_core::Fit::Stretch,
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

#[test]
fn preview_work_is_bounded_when_many_clips_fail() {
    let bytes = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("effects.png"),
    )
    .unwrap();
    let counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut ctx = RenderCtx::default();
    ctx.set_effect_processor(Counting(counter.clone()));
    ctx.resources
        .insert(animathio_core::AssetId::of(&bytes), bytes.clone())
        .unwrap();
    let broken = EffectInstance::new(
        EffectId(1),
        "org.example.slow",
        "broken",
        std::collections::BTreeMap::new(),
    );
    let mut scene = effect_scene(&bytes, vec![broken.clone(), broken.clone(), broken]);
    let template = scene.clips[0].clone();
    for id in 8..60 {
        let mut clip = template.clone();
        clip.id = ClipId(id);
        scene.clips.push(clip);
    }
    let preview = animathio_render::render_preview(&scene, &mut ctx, (64, 64)).unwrap();
    assert_eq!(preview.effect_failures.len(), scene.clips.len());
    // One failing full-frame render plus at most a fixed number of probes, never one
    // run per clip or effect.
    assert!(counter.load(std::sync::atomic::Ordering::SeqCst) <= 16);
}

#[test]
fn bypassed_effects_are_skipped_without_needing_a_processor() {
    let bytes = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("effects.png"),
    )
    .unwrap();
    let size = (64, 64);
    let mut invert = EffectInstance::new(
        EffectId(2),
        "org.example.ok",
        "invert",
        std::collections::BTreeMap::default(),
    );
    let mut ctx = RenderCtx::default();
    ctx.set_effect_processor(Stub);
    ctx.resources
        .insert(animathio_core::AssetId::of(&bytes), bytes.clone())
        .unwrap();
    let plain = render(&effect_scene(&bytes, Vec::new()), &mut ctx, size).unwrap();
    let active = render(&effect_scene(&bytes, vec![invert.clone()]), &mut ctx, size).unwrap();
    assert_ne!(active, plain);

    // A bypassed effect left in the scene is still ignored by the renderer itself.
    invert.bypass = true;
    let bypassed = render(&effect_scene(&bytes, vec![invert.clone()]), &mut ctx, size).unwrap();
    assert_eq!(bypassed, plain);

    // It never reaches the processor, so a missing processor or plugin is not an error.
    let mut bare = RenderCtx::default();
    bare.resources
        .insert(animathio_core::AssetId::of(&bytes), bytes.clone())
        .unwrap();
    assert_eq!(
        render(&effect_scene(&bytes, vec![invert]), &mut bare, size).unwrap(),
        plain
    );
}

/// Test double whose output depends on the effect's `amount` parameter: it blends the
/// frame towards white by that amount, so different sampled values give different pixels.
struct Whiten;

impl animathio_core::EffectProcessor for Whiten {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn apply(
        &mut self,
        effect: &EffectInstance,
        _time: Time,
        _size: (u32, u32),
        pixels: &mut Vec<u8>,
    ) -> Result<(), String> {
        let amount = effect.params.get("amount").copied().unwrap_or(0.0);
        for pixel in pixels.as_chunks_mut::<4>().0 {
            for channel in &mut pixel[..3] {
                let value = f64::from(*channel);
                *channel = (value + (255.0 - value) * amount).clamp(0.0, 255.0).round() as u8;
            }
        }
        Ok(())
    }
}

#[test]
fn keyframed_effect_parameters_render_differently_over_time() {
    use animathio_core::{
        Asset, AssetId, Clip, Command, Interpolation, Keyframe, Project, Target, TimeRange, Track,
        evaluate,
    };
    let bytes = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("effects.png"),
    )
    .unwrap();
    let asset = AssetId::of(&bytes);
    let mut project = Project::default();
    for command in [
        Command::AddAsset {
            id: asset.clone(),
            asset: Asset {
                name: "effects.png".into(),
                media_type: "image/png".into(),
            },
        },
        Command::AddTrack {
            track: Track::new(TrackId(0), "video"),
            index: 0,
        },
        Command::AddClip {
            track: TrackId(0),
            clip: Clip::new(
                ClipId(7),
                ClipKind::Image(asset.clone()),
                TimeRange::new(Time(0), Time(1000)).unwrap(),
            ),
        },
        Command::AddEffect {
            id: ClipId(7),
            index: 0,
            effect: EffectInstance::basic(EffectId(1), "grayscale"),
        },
    ] {
        command.apply(&mut project).unwrap();
    }
    // Keyframe the effect's `amount` from 0 (at the clip start) to 1 (at its end).
    for (time, value) in [(Time(0), 0.0), (Time(1000), 1.0)] {
        Command::AddKeyframe {
            id: ClipId(7),
            target: Target::Effect {
                effect: EffectId(1),
                param: "amount".into(),
            },
            time,
            keyframe: Keyframe {
                value: value.into(),
                interpolation: Interpolation::Linear,
            },
        }
        .apply(&mut project)
        .unwrap();
    }
    let mut ctx = RenderCtx::default();
    ctx.set_effect_processor(Whiten);
    ctx.resources.insert(asset, bytes.clone()).unwrap();
    let size = (64, 64);
    let at =
        |ctx: &mut RenderCtx, time| render(&evaluate(&project, Time(time)), ctx, size).unwrap();
    let start = at(&mut ctx, 0);
    let middle = at(&mut ctx, 500);
    let end = at(&mut ctx, 1000 - 1);

    // The sampled amount drives the output.
    assert_ne!(start, middle);
    assert_ne!(middle, end);

    // The animated frame equals the same scene with a static effect at the sampled value.
    let mut half_scene = evaluate(&project, Time(0));
    half_scene.clips[0].effects[0]
        .params
        .insert("amount".into(), 0.5);
    assert_eq!(middle, render(&half_scene, &mut ctx, size).unwrap());
}
