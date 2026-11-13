//! First-party sources use the sandbox, with typed edits and canvas-independent geometry.
use animathio_core::{
    ClipId, ClipKind, CoordinateSpace, ParamValue, Scene, SceneClip, Time, TrackId, Transform,
    VectorElement,
};
use animathio_plugin_api::sources::{self, PathElement, Shape, Stroke};
use animathio_plugin_host::test_support;
use std::collections::BTreeMap;

fn clip(kind: ClipKind) -> Scene {
    Scene {
        clips: vec![SceneClip {
            id: ClipId(1),
            track: TrackId(1),
            kind,
            source_time: Time(0),
            transform: Transform::default(),
            params: BTreeMap::new(),
            effects: Vec::new(),
            transition: None,
        }],
    }
}

#[test]
fn normalized_text_retains_content_size_color_and_typed_alignment() {
    let mut scene = clip(sources::text(
        "αβ مرحبا\nwrapped words".into(),
        0.1,
        [12, 34, 56, 200],
    ));
    scene.clips[0]
        .params
        .insert("align".into(), ParamValue::Enum("right".into()));
    scene.clips[0]
        .params
        .insert("font_size".into(), ParamValue::Number(0.2));
    let resolved = test_support::resolve(&scene, (640, 360));
    let ClipKind::Vector(vector) = &resolved.clips[0].kind else {
        panic!("component failed: {:?}", resolved.clips[0].kind)
    };
    assert_eq!(vector.coordinate_space, CoordinateSpace::Normalized);
    let VectorElement::TextRun(text) = &vector.elements[0] else {
        panic!("expected a generic text run")
    };
    assert_eq!(text.text, "αβ مرحبا\nwrapped words");
    assert!((text.font_size - 0.2).abs() < 1e-7);
    assert_eq!(text.align, animathio_core::TextAlignment::Right);
    assert_eq!(text.color, animathio_core::Color::rgba(12, 34, 56, 200));
}

#[test]
fn normalized_paths_preserve_all_verbs_optional_paint_and_local_strokes() {
    let shape = Shape::Path(vec![
        PathElement::MoveTo([0.0, 0.0]),
        PathElement::LineTo([1.0, 0.0]),
        PathElement::QuadTo([1.0, 0.5], [0.5, 1.0]),
        PathElement::CurveTo([0.3, 1.0], [0.0, 0.7], [0.0, 0.0]),
        PathElement::Close,
    ]);
    let scene = clip(sources::shape(
        shape,
        None,
        Some(Stroke {
            color: [255, 0, 0, 180],
            width: 0.025,
        }),
    ));
    for size in [(640, 360), (90, 160)] {
        let resolved = test_support::resolve(&scene, size);
        let ClipKind::Vector(vector) = &resolved.clips[0].kind else {
            panic!("component failed: {:?}", resolved.clips[0].kind)
        };
        assert_eq!(vector.coordinate_space, CoordinateSpace::Normalized);
        assert_eq!(
            vector.elements.len(),
            1,
            "no fill for transparent interiors"
        );
        let VectorElement::StrokePath { path, style } = &vector.elements[0] else {
            panic!("expected outline")
        };
        assert_eq!(path.len(), 5);
        assert!(matches!(path[2], animathio_core::PathVerb::QuadTo(..)));
        assert!(matches!(path[3], animathio_core::PathVerb::CubicTo(..)));
        assert!((style.width - 0.025).abs() < 1e-7);
        assert_eq!(style.cap, animathio_core::StrokeCap::Round);
        assert_eq!(style.join, animathio_core::StrokeJoin::Round);
    }
    let resolved = test_support::resolve(&clip(sources::shape(Shape::Rect, None, None)), (90, 160));
    assert!(matches!(&resolved.clips[0].kind, ClipKind::Vector(v) if v.elements.is_empty()));
}
