//! CPU goldens use a deterministic numbered/color video source, not decoder placeholders.
use animathio_core::*;
use animathio_render::{CpuBackend, Frame, FrameSource, RenderCtx};
use std::path::Path;
struct Video;
impl FrameSource for Video {
    fn frame_at(&mut self, _: &AssetId, time: Time) -> Option<Frame> {
        let frame = time.0 / (TICKS_PER_SECOND / 30);
        assert!(
            (0..30).contains(&frame),
            "source lookup escaped media: {time:?}"
        );
        let mut pixels = vec![];
        for y in 0..90 {
            for x in 0..160 {
                let band = (i64::from(x) / 16 + frame) % 3;
                let color = if y < 12 || i64::from(x) < frame * 5 {
                    [240, 240, 240, 255]
                } else {
                    match band {
                        0 => [220, 50, 70, 255],
                        1 => [40, 180, 120, 255],
                        _ => [40, 90, 220, 255],
                    }
                };
                pixels.extend_from_slice(&color);
            }
        }
        Some(Frame {
            size: (160, 90),
            pixels,
        })
    }
}
fn project() -> Project {
    let mut p = Project::default();
    let asset = AssetId::of(b"deterministic video");
    Command::AddAsset {
        id: asset.clone(),
        asset: Asset {
            name: "30 test frames".into(),
            media_type: "video/mp4".into(),
        },
    }
    .apply(&mut p)
    .unwrap();
    Command::AddTrack {
        track: Track::new(TrackId(1), "video"),
        index: 0,
    }
    .apply(&mut p)
    .unwrap();
    Command::AddClip {
        track: TrackId(1),
        clip: Clip::new(
            ClipId(1),
            ClipKind::Video(asset),
            TimeRange::new(Time(0), Time(TICKS_PER_SECOND)).unwrap(),
        ),
    }
    .apply(&mut p)
    .unwrap();
    p
}
fn golden(reverse: bool, name: &str) {
    let mut p = project();
    Command::SetClipSpeed {
        id: ClipId(1),
        speed: ClipSpeed::new(2, 1).unwrap(),
        reverse,
        ramps: vec![],
        ripple: false,
    }
    .apply(&mut p)
    .unwrap();
    let time = Time(5 * TICKS_PER_SECOND / 30); // 2x frame 10; reversed 2x frame 19.
    let scene = evaluate(&p, time);
    assert_eq!(
        scene.clips[0].source_time.0 / (TICKS_PER_SECOND / 30),
        if reverse { 19 } else { 10 }
    );
    let mut ctx = RenderCtx::new(CpuBackend::default(), Video);
    let actual = animathio_render::render(&scene, &mut ctx, (160, 90)).unwrap();
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.png"));
    if std::env::var("UPDATE_GOLDEN").as_deref() == Ok("1") {
        actual.save_png(&path).unwrap();
    }
    let expected = image::open(path).unwrap().to_rgba8();
    assert_eq!(expected.dimensions(), actual.size);
    assert!(
        expected
            .as_raw()
            .iter()
            .zip(&actual.pixels)
            .all(|(a, b)| a.abs_diff(*b) <= 2)
    );
    // Half-open boundary and the last visible sample must not request a frame beyond the source.
    assert_eq!(evaluate(&p, Time(TICKS_PER_SECOND / 2)).clips, []);
    animathio_render::render(
        &evaluate(&p, Time(TICKS_PER_SECOND / 2 - 1)),
        &mut ctx,
        (160, 90),
    )
    .unwrap();
}
#[test]
fn double_speed_video() {
    golden(false, "video-2x");
}
#[test]
fn reverse_video() {
    golden(true, "video-reverse");
}

/// Frame `n` of the source is a solid gray of `10 * n`, so averages are easy to predict.
struct Gray;
impl FrameSource for Gray {
    fn frame_at(&mut self, _: &AssetId, time: Time) -> Option<Frame> {
        let frame = time.0 / (TICKS_PER_SECOND / 30);
        assert!((0..30).contains(&frame), "source lookup escaped media");
        let value = u8::try_from(10 * frame).unwrap();
        Some(Frame {
            size: (4, 4),
            pixels: [value, value, value, 255].repeat(16),
        })
    }
}
fn gray_at(speed: (u32, u32), reverse: bool, output_frame: i64) -> u8 {
    let mut p = project();
    Command::SetClipSpeed {
        id: ClipId(1),
        speed: ClipSpeed::new(speed.0, speed.1).unwrap(),
        reverse,
        ramps: vec![],
        ripple: false,
    }
    .apply(&mut p)
    .unwrap();
    let scene = evaluate(&p, Time(output_frame * TICKS_PER_SECOND / 30));
    let mut ctx = RenderCtx::new(CpuBackend::default(), Gray);
    let frame = animathio_render::render(&scene, &mut ctx, (4, 4)).unwrap();
    assert_eq!(frame.pixels[3 * 4 + 3], 255);
    frame.pixels[(2 * 4 + 2) * 4]
}
#[test]
fn fast_video_averages_the_source_frames_one_output_frame_spans() {
    let close = |a: u8, b: u8| assert!(a.abs_diff(b) <= 2, "{a} vs {b}");
    // 1x and slow motion stay point samples: exactly source frame n.
    close(gray_at((1, 1), false, 7), 70);
    close(gray_at((1, 2), false, 8), 40);
    // 2x output frame 5 spans source frames 10 and 11; frame 2 at 3x spans 6, 7 and 8.
    close(gray_at((2, 1), false, 5), 105);
    close(gray_at((3, 1), false, 2), 70);
    // Reversed 2x: output frame 5 reads source frames 19 and 18 (the span 19.0 down to 17.0).
    close(gray_at((2, 1), true, 5), 185);
    // 16x averages sixteen frames: output frame 1 spans source frames 16..32 of a 30-frame
    // clip... the clip is only 30 source frames long, so use output frame 0 (frames 0..16).
    close(gray_at((16, 1), false, 0), 75);
}
