//! Transition compositing goldens at 25/50/75% progress (maximum channel error: 2).
mod common;
use animathio_core::{
    Clip, ClipId, Command, Project, TICKS_PER_SECOND, Time, TimeRange, Track, TrackId, Transition,
    TransitionId, TransitionKind, WipeDirection, evaluate,
};
use animathio_plugin_api::sources::Shape;
use animathio_render::{Frame, RenderCtx};
use common::render;
use std::path::Path;

const SIZE: (u32, u32) = (320, 180);

fn shape(id: u64, shape: Shape, color: [u8; 4], start: i64) -> Clip {
    Clip::new(
        ClipId(id),
        animathio_plugin_api::sources::shape(shape, Some(color), None),
        TimeRange::new(Time(start), Time(start + TICKS_PER_SECOND)).unwrap(),
    )
}

/// Red rectangle `[0s,1s)` cut to a blue ellipse `[1s,2s)`, with a 1 s transition.
fn project(kind: TransitionKind) -> Project {
    let mut project = Project::default();
    let commands = [
        Command::AddTrack {
            track: Track::new(TrackId(0), "v"),
            index: 0,
        },
        Command::AddClip {
            track: TrackId(0),
            clip: shape(1, Shape::Rect, [220, 40, 40, 255], 0),
        },
        Command::AddClip {
            track: TrackId(0),
            clip: shape(2, Shape::Ellipse, [40, 80, 220, 255], TICKS_PER_SECOND),
        },
        Command::AddTransition {
            track: TrackId(0),
            transition: Transition {
                id: TransitionId(1),
                kind,
                duration: Time(TICKS_PER_SECOND),
                from: ClipId(1),
                to: ClipId(2),
            },
        },
    ];
    for command in commands {
        command.apply(&mut project).unwrap();
    }
    project
}

fn frame(kind: TransitionKind, progress_percent: i64) -> Frame {
    let project = project(kind);
    // The window is [0.5 s, 1.5 s).
    let time = Time(TICKS_PER_SECOND / 2 + TICKS_PER_SECOND * progress_percent / 100);
    render(&evaluate(&project, time), &mut RenderCtx::default(), SIZE).unwrap()
}

fn golden(kind: TransitionKind, name: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    for percent in [25, 50, 75] {
        let actual = frame(kind, percent);
        let path = root.join(format!("transition-{name}-{percent}.png"));
        if std::env::var("UPDATE_GOLDEN").as_deref() == Ok("1") {
            actual.save_png(&path).unwrap();
        }
        let expected = image::open(&path)
            .unwrap_or_else(|e| panic!("{}: {e}; regenerate with UPDATE_GOLDEN=1", path.display()))
            .to_rgba8();
        let differences = expected
            .as_raw()
            .iter()
            .zip(&actual.pixels)
            .filter(|(a, b)| a.abs_diff(**b) > 2)
            .count();
        assert_eq!(differences, 0, "{} differs", path.display());
    }
}

fn pixel(frame: &Frame, x: usize, y: usize) -> [u8; 4] {
    let i = (y * frame.size.0 as usize + x) * 4;
    frame.pixels[i..i + 4].try_into().unwrap()
}

#[test]
fn cross_dissolve() {
    golden(TransitionKind::CrossDissolve, "dissolve");
    // Center: both opaque, so a straight 50/50 blend.
    assert_eq!(
        pixel(&frame(TransitionKind::CrossDissolve, 50), 160, 90),
        [130, 60, 130, 255]
    );
    // Corner: the ellipse is transparent, so the rectangle fades out.
    assert_eq!(
        pixel(&frame(TransitionKind::CrossDissolve, 50), 1, 1)[3],
        128
    );
}
#[test]
fn dip_to_black() {
    golden(TransitionKind::DipToBlack, "dip-black");
    assert_eq!(
        pixel(&frame(TransitionKind::DipToBlack, 50), 1, 1),
        [0, 0, 0, 255]
    );
}
#[test]
fn dip_to_white() {
    golden(TransitionKind::DipToWhite, "dip-white");
    assert_eq!(
        pixel(&frame(TransitionKind::DipToWhite, 50), 160, 90),
        [255, 255, 255, 255]
    );
}
#[test]
fn wipe_left_to_right() {
    let kind = TransitionKind::Wipe {
        direction: WipeDirection::LeftToRight,
    };
    golden(kind, "wipe");
    let half = frame(kind, 50);
    assert_eq!(pixel(&half, 150, 90), [40, 80, 220, 255]); // revealed ellipse
    assert_eq!(pixel(&half, 170, 90), [220, 40, 40, 255]); // remaining rectangle
}
