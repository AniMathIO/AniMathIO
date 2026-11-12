//! Cache lifetime, random access and compositing contracts beyond pixel goldens.
use animathio_core::{
    builtin::{Builtin, Shape},
    *,
};
use animathio_render::{Frame, FrameSource, RenderCtx, ResourcePool, render, sample};
use proptest::prelude::*;
use std::{cell::RefCell, rc::Rc, sync::Arc};

fn layer(id: u64, kind: ClipKind, transform: Transform) -> SceneClip {
    SceneClip {
        id: ClipId(id),
        track: TrackId(id),
        kind,
        source_time: Time(0),
        transform,
        params: std::collections::BTreeMap::new(),
        effects: Vec::new(),
    }
}
fn rect(color: [u8; 4], transform: Transform) -> SceneClip {
    layer(
        0,
        ClipKind::Builtin(Builtin::Shape {
            shape: Shape::Rect,
            fill: Some(color),
            stroke: None,
        }),
        transform,
    )
}
fn pixel(frame: &Frame, x: usize, y: usize) -> &[u8] {
    &frame.pixels[(y * frame.size.0 as usize + x) * 4..(y * frame.size.0 as usize + x + 1) * 4]
}
#[test]
fn normalized_transform_rotation_mirroring_opacity_and_z_order() {
    let background = rect([0, 0, 255, 255], Transform::default());
    let foreground = rect(
        [255, 0, 0, 255],
        Transform {
            x: 0.25,
            y: 0.5,
            scale_x: -0.5,
            scale_y: 0.5,
            opacity: 0.5,
            ..Transform::default()
        },
    );
    let scene = Scene {
        clips: vec![background.clone(), foreground.clone()],
    };
    let frame = render(&scene, &mut RenderCtx::default(), (100, 80)).unwrap();
    assert_eq!(pixel(&frame, 75, 40), &[0, 0, 255, 255]);
    let p = pixel(&frame, 25, 40);
    assert!((i32::from(p[0]) - 128).abs() <= 1 && (i32::from(p[2]) - 127).abs() <= 1);
    let reverse = render(
        &Scene {
            clips: vec![foreground, background],
        },
        &mut RenderCtx::default(),
        (100, 80),
    )
    .unwrap();
    assert_eq!(pixel(&reverse, 25, 40), &[0, 0, 255, 255]);
    let rotated = rect(
        [255; 4],
        Transform {
            scale_x: 0.6,
            scale_y: 0.2,
            rotation: std::f64::consts::FRAC_PI_2,
            ..Transform::default()
        },
    );
    let frame = render(
        &Scene {
            clips: vec![rotated],
        },
        &mut RenderCtx::default(),
        (100, 100),
    )
    .unwrap();
    assert_eq!(pixel(&frame, 50, 25), &[255; 4]);
    assert_eq!(pixel(&frame, 25, 50), &[0; 4]);
}
#[test]
fn images_decode_once_and_gc_releases_every_asset_after_clips_are_deleted() {
    let sample = sample::build().unwrap();
    let mut ctx = RenderCtx::default();
    for (id, bytes) in sample.assets {
        ctx.resources.insert(id, bytes).unwrap();
    }
    let mut project = sample.project;
    let id = project.assets().keys().next().unwrap().clone();
    let a = ctx.resources.image(&id).unwrap();
    let b = ctx.resources.image(&id).unwrap();
    assert!(Arc::ptr_eq(&a, &b));
    for t in [0, 100, TICKS_PER_SECOND] {
        render(&evaluate(&project, Time(t)), &mut ctx, (64, 36)).unwrap();
    }
    assert_eq!(ctx.resources.decoded_images(), 1);
    let clips: Vec<_> = project
        .tracks()
        .iter()
        .flat_map(|t| t.clips().keys().copied())
        .collect();
    for id in clips {
        Command::RemoveClip { id }.apply(&mut project).unwrap();
    }
    ctx.resources.gc(&project);
    assert!(ctx.resources.is_empty());
}
#[test]
fn transparent_images_preserve_straight_alpha_and_mirrored_orientation() {
    let image = Frame {
        size: (2, 1),
        pixels: vec![255, 0, 0, 128, 0, 255, 0, 255],
    };
    let mut bytes = Vec::new();
    image.write_png(&mut bytes).unwrap();
    let mut ctx = RenderCtx::default();
    let id = ctx.resources.add(bytes);
    let scene = Scene {
        clips: vec![layer(
            0,
            ClipKind::Image(id),
            Transform {
                scale_x: -1.0,
                ..Transform::default()
            },
        )],
    };
    let frame = render(&scene, &mut ctx, (2, 1)).unwrap();
    assert_eq!(pixel(&frame, 0, 0), &[0, 255, 0, 255]);
    assert_eq!(pixel(&frame, 1, 0), &[255, 0, 0, 128]);
}
#[test]
fn missing_corrupt_and_mismatched_assets_and_invalid_sizes_report_errors() {
    let mut pool = ResourcePool::default();
    let id = AssetId::of(b"missing");
    assert!(pool.image(&id).is_err());
    assert!(pool.insert(id, b"other".to_vec()).is_err());
    let id = pool.add(b"not an image".to_vec());
    assert!(pool.image(&id).is_err());
    for size in [(0, 1), (1, 0), (u32::MAX, 1), (8192, 8192)] {
        assert!(render(&Scene::default(), &mut RenderCtx::default(), size).is_err());
    }
}
#[test]
fn video_adapter_receives_exact_evaluated_media_time_and_audio_is_invisible() {
    type Calls = Rc<RefCell<Vec<(AssetId, Time)>>>;
    struct Source(Calls);
    impl FrameSource for Source {
        fn frame_at(&mut self, asset: &AssetId, time: Time) -> Option<Frame> {
            self.0.borrow_mut().push((asset.clone(), time));
            Some(Frame {
                size: (1, 1),
                pixels: vec![12, 34, 56, 255],
            })
        }
    }
    let mut project = Project::default();
    let id = AssetId::of(b"video");
    Command::AddAsset {
        id: id.clone(),
        asset: Asset {
            name: "movie".into(),
            media_type: "video/test".into(),
        },
    }
    .apply(&mut project)
    .unwrap();
    Command::AddTrack {
        track: Track::new(TrackId(0), "video"),
        index: 0,
    }
    .apply(&mut project)
    .unwrap();
    let mut clip = Clip::new(
        ClipId(0),
        ClipKind::Video(id.clone()),
        TimeRange::new(Time(100), Time(300)).unwrap(),
    );
    clip.source_offset = Time(500);
    Command::AddClip {
        track: TrackId(0),
        clip,
    }
    .apply(&mut project)
    .unwrap();
    let calls = Calls::default();
    let mut ctx = RenderCtx::new(
        animathio_render::CpuBackend::default(),
        Source(calls.clone()),
    );
    let mut scene = evaluate(&project, Time(150));
    scene
        .clips
        .push(layer(1, ClipKind::Audio(id.clone()), Transform::default()));
    let frame = render(&scene, &mut ctx, (4, 4)).unwrap();
    assert_eq!(pixel(&frame, 2, 2), &[12, 34, 56, 255]);
    assert_eq!(*calls.borrow(), vec![(id, Time(550))]);
}
#[test]
fn malformed_decoder_frame_is_an_error() {
    struct BadSource;
    impl FrameSource for BadSource {
        fn frame_at(&mut self, _: &AssetId, _: Time) -> Option<Frame> {
            Some(Frame {
                size: (2, 2),
                pixels: vec![0],
            })
        }
    }
    let mut ctx = RenderCtx::new(animathio_render::CpuBackend::default(), BadSource);
    let scene = Scene {
        clips: vec![layer(
            0,
            ClipKind::Video(AssetId::of(b"video")),
            Transform::default(),
        )],
    };
    assert!(render(&scene, &mut ctx, (10, 10)).is_err());
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    #[test]
    fn rendering_is_independent_of_prior_times_and_sizes(target in -1_i64..100, prior in prop::collection::vec(-1_i64..100, 0..10)) {
        let sample=sample::build().unwrap();
        let mut fresh=RenderCtx::default();
        let mut reused=RenderCtx::default();
        for (id,bytes) in sample.assets {
            fresh.resources.insert(id.clone(),bytes.clone()).unwrap();
            reused.resources.insert(id,bytes).unwrap();
        }
        let rate=sample.project.frame_rate();
        let scene=|frame| evaluate(&sample.project,rate.frame_to_time(frame,Rounding::Nearest).unwrap());
        let expected=render(&scene(target),&mut fresh,(64,36)).unwrap();
        for (index,time) in prior.into_iter().enumerate() {
            let size=if index%2==0 { (48,48) } else { (64,36) };
            render(&scene(time),&mut reused,size).unwrap();
        }
        prop_assert_eq!(render(&scene(target),&mut reused,(64,36)).unwrap(),expected);
    }
}
#[test]
fn vector_scenes_render_pixel_space_paths() {
    let scene = Scene {
        clips: vec![layer(
            0,
            ClipKind::Vector(VectorScene {
                elements: vec![VectorElement::FillPath {
                    path: vec![
                        PathVerb::MoveTo(Point::new(10.0, 10.0)),
                        PathVerb::LineTo(Point::new(30.0, 10.0)),
                        PathVerb::LineTo(Point::new(30.0, 30.0)),
                        PathVerb::LineTo(Point::new(10.0, 30.0)),
                        PathVerb::Close,
                    ],
                    color: Color::rgb(255, 0, 0),
                }],
            }),
            Transform::default(),
        )],
    };
    let frame = render(&scene, &mut RenderCtx::default(), (64, 36)).unwrap();
    // The default clip transform is the identity for pixel-space vector scenes.
    assert_eq!(pixel(&frame, 20, 20), &[255, 0, 0, 255]);
    assert_eq!(pixel(&frame, 0, 0), &[0, 0, 0, 0]);
}
