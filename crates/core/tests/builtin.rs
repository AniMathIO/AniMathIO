//! Temporary builtin data must obey the same validation and command contracts.
use animathio_core::{
    builtin::{Builtin, PathElement, Shape, Stroke},
    *,
};

fn insert(project: &mut Project, source: Builtin) -> Result<Command, Error> {
    Command::AddClip {
        track: TrackId(1),
        clip: Clip::new(
            ClipId(1),
            ClipKind::Builtin(source),
            TimeRange::new(Time(0), Time(10)).unwrap(),
        ),
    }
    .apply(project)
}
fn project() -> Project {
    let mut project = Project::default();
    Command::AddTrack {
        track: Track::new(TrackId(1), "builtin"),
        index: 0,
    }
    .apply(&mut project)
    .unwrap();
    project
}
#[test]
fn builtin_roundtrip_evaluation_and_inverse() {
    for source in [
        Builtin::Text {
            text: "αβ مرحبا".into(),
            font_size: 0.1,
            color: [255; 4],
        },
        Builtin::Shape {
            shape: Shape::Path(vec![
                PathElement::MoveTo([0.0, 0.0]),
                PathElement::QuadTo([0.5, 1.0], [1.0, 0.0]),
                PathElement::Close,
            ]),
            fill: Some([100; 4]),
            stroke: None,
        },
    ] {
        let mut project = project();
        let before = project.clone();
        let inverse = insert(&mut project, source.clone()).unwrap();
        assert_eq!(
            evaluate(&project, Time(5)).clips[0].kind,
            ClipKind::Builtin(source)
        );
        assert_eq!(
            serde_json::from_slice::<Project>(&serde_json::to_vec(&project).unwrap()).unwrap(),
            project
        );
        inverse.apply(&mut project).unwrap();
        assert_eq!(project, before);
    }
}
#[test]
fn invalid_builtin_data_is_transactionally_rejected() {
    let mut project = project();
    let before = project.clone();
    for font_size in [0.0, -1.0, f64::NAN, f64::INFINITY, 17.0] {
        assert!(
            insert(
                &mut project,
                Builtin::Text {
                    text: String::new(),
                    font_size,
                    color: [0; 4]
                }
            )
            .is_err()
        );
        assert_eq!(project, before);
    }
    for shape in [
        Shape::Path(vec![PathElement::LineTo([0.0, 0.0])]),
        Shape::Path(vec![PathElement::MoveTo([f64::NAN, 0.0])]),
        Shape::Path(vec![
            PathElement::MoveTo([0.0, 0.0]),
            PathElement::CurveTo([0.0, 0.0], [f64::INFINITY, 0.0], [1.0, 1.0]),
        ]),
    ] {
        assert!(
            insert(
                &mut project,
                Builtin::Shape {
                    shape,
                    fill: None,
                    stroke: None
                }
            )
            .is_err()
        );
        assert_eq!(project, before);
    }
    assert!(
        insert(
            &mut project,
            Builtin::Shape {
                shape: Shape::Rect,
                fill: None,
                stroke: Some(Stroke {
                    color: [255; 4],
                    width: -1.0
                })
            }
        )
        .is_err()
    );
    assert_eq!(project, before);
}
