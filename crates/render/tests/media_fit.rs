//! Actual image/video placement and shared CPU/GPU media fitting.
use animathio_core::*;
use animathio_render::{CpuBackend, Frame, FrameSource, RenderCtx};
use std::collections::BTreeMap;

fn source() -> Frame {
    let mut pixels = Vec::new();
    for _ in 0..90 {
        for x in 0..160 {
            pixels.extend_from_slice(if x < 40 {
                &[255, 0, 0, 255]
            } else if x < 120 {
                &[0, 255, 0, 255]
            } else {
                &[0, 0, 255, 255]
            });
        }
    }
    Frame {
        size: (160, 90),
        pixels,
    }
}
fn source_bytes() -> Vec<u8> {
    let frame = source();
    let image = image::RgbaImage::from_raw(frame.size.0, frame.size.1, frame.pixels).unwrap();
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}
fn asset() -> AssetId {
    AssetId::of(&source_bytes())
}
struct Video;
impl FrameSource for Video {
    fn frame_at(&mut self, _: &AssetId, _: Time) -> Option<Frame> {
        Some(source())
    }
}
fn scene(fit: Fit, video: bool) -> Scene {
    let asset = asset();
    Scene {
        canvas: (90, 160),
        clips: vec![SceneClip {
            id: ClipId(1),
            track: TrackId(1),
            kind: if video {
                ClipKind::Video(asset)
            } else {
                ClipKind::Image(asset)
            },
            fit,
            source_time: Time(0),
            transform: Transform::default(),
            params: BTreeMap::new(),
            effects: vec![],
            transition: None,
        }],
    }
}
fn cpu() -> RenderCtx {
    let mut ctx = RenderCtx::new(CpuBackend::default(), Video);
    ctx.resources.insert(asset(), source_bytes()).unwrap();
    ctx
}
fn pixel(frame: &Frame, x: usize, y: usize) -> &[u8] {
    let i = (y * frame.size.0 as usize + x) * 4;
    &frame.pixels[i..i + 4]
}
#[test]
fn image_and_video_fit_fill_stretch_and_native_project_pixels() {
    for video in [false, true] {
        let mut ctx = cpu();
        for size in [(90, 160), (180, 320)] {
            let k = size.0 as usize / 90;
            for (fit, top_alpha, left_color) in [
                (Fit::Contain, 0, [255, 0, 0, 255]),
                (Fit::Cover, 255, [0, 255, 0, 255]),
                (Fit::Stretch, 255, [255, 0, 0, 255]),
                (Fit::Original, 0, [0, 255, 0, 255]),
            ] {
                let frame = animathio_render::render(&scene(fit, video), &mut ctx, size).unwrap();
                assert_eq!(pixel(&frame, 45 * k, 5 * k)[3], top_alpha, "{fit:?}");
                assert_eq!(pixel(&frame, 8 * k, 80 * k), left_color, "{fit:?}");
                assert_eq!(pixel(&frame, 45 * k, 80 * k), [0, 255, 0, 255]);
            }
        }
    }
}

#[test]
fn fitting_preserves_aspect_at_rounded_or_overridden_output_sizes() {
    let frame =
        animathio_render::render(&scene(Fit::Contain, false), &mut cpu(), (100, 100)).unwrap();
    assert_eq!(pixel(&frame, 50, 10)[3], 0);
    assert_eq!(pixel(&frame, 50, 25), [0, 255, 0, 255]);
    assert_eq!(pixel(&frame, 50, 75), [0, 255, 0, 255]);
    assert_eq!(pixel(&frame, 50, 90)[3], 0);
}

#[cfg(feature = "gpu")]
#[test]
fn gpu_and_cpu_fit_the_same_pixels_including_crops_and_transforms() {
    let gpu = match animathio_render::GpuBackend::new() {
        Ok(gpu) => gpu,
        Err(error) => {
            eprintln!("skipping GPU fit comparison: {error}");
            return;
        }
    };
    let mut gpu = RenderCtx::new(gpu, Video);
    gpu.resources.insert(asset(), source_bytes()).unwrap();
    let mut cpu = cpu();
    for fit in Fit::ALL {
        for video in [false, true] {
            for t in [
                Transform::default(),
                Transform {
                    scale_x: -0.6,
                    scale_y: 0.4,
                    rotation: 0.3,
                    ..Transform::default()
                },
            ] {
                let mut scene = scene(fit, video);
                scene.clips[0].transform = t;
                let expected = animathio_render::render(&scene, &mut cpu, (90, 160)).unwrap();
                let actual = animathio_render::render(&scene, &mut gpu, (90, 160)).unwrap();
                let total: u64 = expected
                    .pixels
                    .iter()
                    .zip(&actual.pixels)
                    .map(|(a, b)| u64::from(a.abs_diff(*b)))
                    .sum();
                assert!(
                    total < 90 * 160 * 4 * 2,
                    "{fit:?}: total channel error {total}"
                );
            }
        }
    }
}
