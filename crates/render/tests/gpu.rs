//! GPU preview backend vs the CPU reference on the golden fixtures.
//! Skipped (with a message) when no GPU adapter is available, as on CI.
#![cfg(feature = "gpu")]
use animathio_core::{
    AssetId, ClipId, ClipKind, EffectId, EffectInstance, EffectKind, Rounding, Scene, SceneClip,
    Time, TrackId, Transform, evaluate,
};
use animathio_format::{Bundle, Limits, read_amio, read_json};
use animathio_render::{CpuBackend, Frame, GpuBackend, PlaceholderFrameSource, RenderCtx, render};
use std::{collections::BTreeMap, fs::File, path::Path};

/// Antialiasing differs slightly between the CPU and GPU rasterizers, so compare with
/// a mean-error bound plus a cap on the share of clearly different channels.
const MAX_MEAN_ERROR: f64 = 1.5;
const LARGE_ERROR: u8 = 24;
const MAX_LARGE_SHARE: f64 = 0.01;

fn gpu() -> Option<GpuBackend> {
    match GpuBackend::new() {
        Ok(gpu) => Some(gpu),
        Err(error) => {
            eprintln!("skipping GPU comparison: {error}");
            None
        }
    }
}

fn load(project: &str) -> Bundle {
    let input = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(project);
    if project.ends_with("amio") {
        return read_amio(File::open(input).unwrap(), Limits::default()).unwrap();
    }
    let project = read_json(File::open(&input).unwrap()).unwrap();
    let assets = project
        .assets()
        .keys()
        .map(|id| {
            let path = input.parent().unwrap().join("assets").join(id.as_str());
            (id.clone(), std::fs::read(path).unwrap())
        })
        .collect();
    Bundle { project, assets }
}

fn ctx(assets: &BTreeMap<AssetId, Vec<u8>>, gpu: Option<GpuBackend>) -> RenderCtx {
    let mut ctx = match gpu {
        Some(gpu) => RenderCtx::new(gpu, PlaceholderFrameSource::default()),
        None => RenderCtx::new(CpuBackend::default(), PlaceholderFrameSource::default()),
    };
    for (id, bytes) in assets {
        ctx.resources.insert(id.clone(), bytes.clone()).unwrap();
    }
    ctx
}

fn compare(name: &str, cpu: &Frame, gpu: &Frame) {
    assert_eq!(cpu.size, gpu.size, "{name}: size");
    let mut total = 0u64;
    let mut large = 0usize;
    for (a, b) in cpu.pixels.iter().zip(&gpu.pixels) {
        let diff = a.abs_diff(*b);
        total += u64::from(diff);
        if diff > LARGE_ERROR {
            large += 1;
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let (mean, share) = (
        total as f64 / cpu.pixels.len() as f64,
        large as f64 / cpu.pixels.len() as f64,
    );
    eprintln!("{name}: mean error {mean:.3}, large-error share {share:.5}");
    if mean > MAX_MEAN_ERROR || share > MAX_LARGE_SHARE {
        let dir = std::env::temp_dir();
        cpu.save_png(dir.join(format!("animathio-gpu-{name}-cpu.png")))
            .unwrap();
        gpu.save_png(dir.join(format!("animathio-gpu-{name}-gpu.png")))
            .unwrap();
        panic!("{name}: GPU frame differs from CPU (mean {mean:.3}, large share {share:.5})");
    }
}

#[test]
fn gpu_matches_cpu_on_golden_fixtures() {
    let Some(gpu) = gpu() else { return };
    let mut gpu_ctx = None;
    let mut gpu = Some(gpu);
    for (project, frame, name) in [
        ("sample.amio", 0, "sample-start"),
        ("sample.amio", 30, "sample-midpoint"),
        ("sample.amio", 60, "sample-last-keyframe"),
        ("sample.amio", 90, "sample-end"),
        ("media.json", 15, "media-placeholders"),
    ] {
        let bundle = load(project);
        let time = bundle
            .project
            .frame_rate()
            .frame_to_time(frame, Rounding::Nearest)
            .unwrap();
        let scene = evaluate(&bundle.project, time);
        // Reuse one GPU backend (device, renderer, target) across all fixtures.
        let gpu_ctx = gpu_ctx.get_or_insert_with(|| ctx(&bundle.assets, gpu.take()));
        for (id, bytes) in &bundle.assets {
            gpu_ctx.resources.insert(id.clone(), bytes.clone()).unwrap();
        }
        let mut cpu_ctx = ctx(&bundle.assets, None);
        for size in [(320, 180), (333, 187)] {
            let expected = render(&scene, &mut cpu_ctx, size).unwrap();
            let actual = render(&scene, gpu_ctx, size).unwrap();
            compare(&format!("{name}-{}x{}", size.0, size.1), &expected, &actual);
        }
    }
}

#[test]
fn gpu_composites_cpu_effect_layers() {
    let Some(gpu) = gpu() else { return };
    let bytes =
        std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/effects.png"))
            .unwrap();
    let id = AssetId::of(&bytes);
    let mut effect = EffectInstance::new(EffectId(1), EffectKind::Blur);
    effect.params.insert("radius".to_owned(), 3.0);
    let scene = Scene {
        clips: vec![SceneClip {
            track: TrackId(0),
            id: ClipId(0),
            kind: ClipKind::Image(id.clone()),
            source_time: Time(0),
            transform: Transform {
                opacity: 0.75,
                ..Transform::default()
            },
            params: BTreeMap::new(),
            effects: vec![effect],
        }],
    };
    let assets = BTreeMap::from([(id, bytes)]);
    let expected = render(&scene, &mut ctx(&assets, None), (64, 64)).unwrap();
    let actual = render(&scene, &mut ctx(&assets, Some(gpu)), (64, 64)).unwrap();
    compare("effect-blur", &expected, &actual);
}
