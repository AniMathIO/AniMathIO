//! Tests for `VectorScene` and vector graphics primitives in core.
use animathio_core::*;

#[test]
fn vector_scene_roundtrip_and_validation() {
    let mut scene = VectorScene::new();
    scene.elements.push(VectorElement::FillPath {
        path: vec![
            PathVerb::MoveTo(Point::new(10.0, 20.0)),
            PathVerb::LineTo(Point::new(30.0, 40.0)),
            PathVerb::QuadTo(Point::new(50.0, 60.0), Point::new(70.0, 80.0)),
            PathVerb::CubicTo(
                Point::new(90.0, 100.0),
                Point::new(110.0, 120.0),
                Point::new(130.0, 140.0),
            ),
            PathVerb::Close,
        ],
        color: Color::rgba(255, 128, 64, 200),
    });
    scene.elements.push(VectorElement::StrokePath {
        path: vec![
            PathVerb::MoveTo(Point::new(0.0, 0.0)),
            PathVerb::LineTo(Point::new(100.0, 100.0)),
        ],
        style: StrokeStyle {
            width: 3.5,
            color: Color::rgb(0, 255, 0),
            cap: StrokeCap::Round,
            join: StrokeJoin::Round,
        },
    });
    scene.elements.push(VectorElement::TextRun(TextElement {
        align: TextAlignment::Left,
        text: "AniMathIO".into(),
        font_size: 24.0,
        x: 50.0,
        y: 100.0,
        color: Color::rgb(255, 255, 255),
    }));

    assert!(scene.validate().is_ok());

    // JSON round-trip
    let json = serde_json::to_string(&scene).unwrap();
    let deserialized: VectorScene = serde_json::from_str(&json).unwrap();
    assert_eq!(scene, deserialized);
}

#[test]
fn vector_scene_rejects_nonfinite_and_invalid() {
    let bad_path = VectorScene {
        coordinate_space: animathio_core::CoordinateSpace::Canvas,
        elements: vec![VectorElement::FillPath {
            path: vec![PathVerb::MoveTo(Point::new(f64::NAN, 0.0))],
            color: Color::rgb(0, 0, 0),
        }],
    };
    assert!(bad_path.validate().is_err());

    let bad_stroke = VectorScene {
        coordinate_space: animathio_core::CoordinateSpace::Canvas,
        elements: vec![VectorElement::StrokePath {
            path: vec![PathVerb::MoveTo(Point::new(0.0, 0.0))],
            style: StrokeStyle {
                width: -2.0,
                color: Color::rgb(0, 0, 0),
                cap: StrokeCap::Butt,
                join: StrokeJoin::Miter,
            },
        }],
    };
    assert!(bad_stroke.validate().is_err());

    let bad_text = VectorScene {
        coordinate_space: animathio_core::CoordinateSpace::Canvas,
        elements: vec![VectorElement::TextRun(TextElement {
            align: TextAlignment::Left,
            text: "test".into(),
            font_size: 0.0,
            x: 0.0,
            y: 0.0,
            color: Color::rgb(0, 0, 0),
        })],
    };
    assert!(bad_text.validate().is_err());
}

#[test]
fn project_with_vector_and_error_clips() {
    let mut project = Project::default();
    Command::AddTrack {
        track: Track::new(TrackId(1), "Vector Layer"),
        index: 0,
    }
    .apply(&mut project)
    .unwrap();

    let mut vector_scene = VectorScene::new();
    vector_scene.elements.push(VectorElement::FillPath {
        path: vec![PathVerb::MoveTo(Point::new(0.0, 0.0)), PathVerb::Close],
        color: Color::rgb(255, 0, 0),
    });

    let clip1 = Clip::new(
        ClipId(1),
        ClipKind::Vector(vector_scene),
        TimeRange::new(Time(0), Time(600)).unwrap(),
    );
    let clip2 = Clip::new(
        ClipId(2),
        ClipKind::Error("Plugin trapped".into()),
        TimeRange::new(Time(600), Time(1200)).unwrap(),
    );

    Command::AddClip {
        track: TrackId(1),
        clip: clip1,
    }
    .apply(&mut project)
    .unwrap();

    Command::AddClip {
        track: TrackId(1),
        clip: clip2,
    }
    .apply(&mut project)
    .unwrap();

    assert!(project.validate().is_ok());

    let scene_at_10 = evaluate(&project, Time(10));
    assert_eq!(scene_at_10.clips.len(), 1);
    assert!(matches!(scene_at_10.clips[0].kind, ClipKind::Vector(_)));

    let scene_at_700 = evaluate(&project, Time(700));
    assert_eq!(scene_at_700.clips.len(), 1);
    assert_eq!(
        scene_at_700.clips[0].kind,
        ClipKind::Error("Plugin trapped".into())
    );
}
