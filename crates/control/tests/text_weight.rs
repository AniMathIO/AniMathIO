//! Text plugin 1.3.0 `font_weight`: schema, rendering and backward compatibility.
use animathio_core::{
    ClipId, ClipKind, CoordinateSpace, Fit, ParamValue, Scene, SceneClip, Time, TrackId, Transform,
    VectorElement,
};
use animathio_plugin_api::sources;
use animathio_plugin_host::test_support;
use std::collections::BTreeMap;

const SIZE: (u32, u32) = (640, 360);

fn scene(text: &str, weight: Option<f64>, fit: bool) -> Scene {
    let mut kind = sources::text(text.into(), 0.12, [255; 4]);
    if let ClipKind::Plugin { params, .. } = &mut kind {
        params["align"] = "center".into();
        if let Some(weight) = weight {
            params["font_weight"] = weight.into();
        }
        if fit {
            params["fit_width"] = 0.8.into();
            params["fit_height"] = 0.3.into();
        }
    }
    Scene {
        canvas: SIZE,
        background: [0, 0, 0, 255],
        clips: vec![SceneClip {
            id: ClipId(1),
            track: TrackId(1),
            kind,
            source_time: Time(0),
            fit: Fit::Stretch,
            transform: Transform::default(),
            params: BTreeMap::new(),
            effects: Vec::new(),
            transition: None,
        }],
    }
}

/// Lit pixels and their bounding box `(min_x, min_y, max_x, max_y)` of one rendered frame.
fn ink(text: &str, weight: Option<f64>, fit: bool) -> (u64, (u32, u32, u32, u32)) {
    let mut host = test_support::host();
    let resolved = host.resolve(&scene(text, weight, fit), SIZE);
    assert!(
        !matches!(resolved.clips[0].kind, ClipKind::Error(_)),
        "{:?}",
        resolved.clips[0].kind
    );
    let mut ctx = animathio_render::RenderCtx::default();
    let frame = animathio_render::render(&resolved, &mut ctx, SIZE).unwrap();
    let (mut total, mut bbox) = (0_u64, (u32::MAX, u32::MAX, 0, 0));
    for (index, pixel) in frame.pixels.as_chunks::<4>().0.iter().enumerate() {
        let x = u32::try_from(index).unwrap() % SIZE.0;
        let y = u32::try_from(index).unwrap() / SIZE.0;
        total += u64::from(pixel[0]);
        if pixel[0] > 127 {
            bbox = (bbox.0.min(x), bbox.1.min(y), bbox.2.max(x), bbox.3.max(y));
        }
    }
    (total, bbox)
}

#[test]
fn heavier_weights_render_more_ink_than_regular() {
    for fit in [false, true] {
        let regular = ink("Heading text", Some(400.0), fit).0;
        let semibold = ink("Heading text", Some(600.0), fit).0;
        let bold = ink("Heading text", Some(700.0), fit).0;
        let heavy = ink("Heading text", Some(800.0), fit).0;
        assert!(regular > 0, "regular text is visible (fit {fit})");
        assert!(
            semibold > regular && bold > semibold && heavy > bold,
            "fit {fit}: {regular} < {semibold} < {bold} < {heavy}"
        );
    }
}

#[test]
fn a_missing_weight_equals_regular() {
    // Projects saved before text 1.3.0 have no `font_weight`: same frame as an explicit 400.
    for fit in [false, true] {
        assert_eq!(
            ink("Regular", None, fit),
            ink("Regular", Some(400.0), fit),
            "fit {fit}"
        );
    }
}

#[test]
fn regular_keeps_the_host_text_run_and_weighted_text_is_paths() {
    let mut host = test_support::host();
    let regular = host.resolve(&scene("Body", Some(400.0), false), SIZE);
    let ClipKind::Vector(vector) = &regular.clips[0].kind else {
        panic!("regular text failed")
    };
    assert!(matches!(vector.elements[0], VectorElement::TextRun(_)));
    let bold = host.resolve(&scene("Body", Some(700.0), false), SIZE);
    let ClipKind::Vector(vector) = &bold.clips[0].kind else {
        panic!("bold text failed")
    };
    assert_eq!(vector.coordinate_space, CoordinateSpace::Normalized);
    assert!(matches!(vector.elements[0], VectorElement::FillPath { .. }));
}

#[test]
fn weighted_text_is_centred_like_regular_text() {
    let centre = |bbox: (u32, u32, u32, u32)| {
        (
            f64::from(bbox.0 + bbox.2) / 2.0 / f64::from(SIZE.0),
            f64::from(bbox.1 + bbox.3) / 2.0 / f64::from(SIZE.1),
        )
    };
    for weight in [400.0, 600.0, 800.0] {
        let (x, y) = centre(ink("Centred title", Some(weight), false).1);
        assert!((x - 0.5).abs() < 0.02, "weight {weight}: x {x}");
        assert!((y - 0.5).abs() < 0.05, "weight {weight}: y {y}");
    }
}

#[test]
fn weighted_text_wraps_inside_the_canvas_and_honours_newlines() {
    let long = "A fairly long sentence that has to wrap onto several lines to fit inside the frame";
    let (_, wrapped) = ink(long, Some(800.0), false);
    assert!(wrapped.0 > 0 && wrapped.2 < SIZE.0 - 1, "{wrapped:?}");
    let one = ink("Two", Some(700.0), false).1;
    let two = ink("Two\nlines", Some(700.0), false).1;
    assert!(two.3 - two.1 > one.3 - one.1, "{one:?} {two:?}");
}

#[test]
fn weight_is_clamped_and_editable_as_a_plain_parameter() {
    let (extreme, _) = ink("Clamp", Some(5000.0), false);
    assert_eq!(extreme, ink("Clamp", Some(900.0), false).0);
    let (low, _) = ink("Clamp", Some(1.0), false);
    assert_eq!(low, ink("Clamp", Some(100.0), false).0);
    // The inspector and Command::SetPluginParam address it by name with a number.
    let mut scene = scene("Param", None, false);
    scene.clips[0]
        .params
        .insert("font_weight".into(), ParamValue::Number(700.0));
    let resolved = test_support::resolve(&scene, SIZE);
    let ClipKind::Vector(vector) = &resolved.clips[0].kind else {
        panic!("weighted text failed")
    };
    assert!(matches!(vector.elements[0], VectorElement::FillPath { .. }));
}

#[test]
fn set_param_edits_the_weight_of_a_title_clip_through_commands() {
    use animathio_control::{TitleAddParams, title_preset_command};
    use animathio_core::{Command, History, Project, TICKS_PER_SECOND, TimeRange, evaluate};
    let mut project = Project::default();
    let params = TitleAddParams {
        preset: "typography_body".into(),
        fields: BTreeMap::new(),
        range: TimeRange::new(Time(0), Time(5 * TICKS_PER_SECOND)).unwrap(),
    };
    let (command, added) = title_preset_command(&project, &params).unwrap();
    let mut history = History::default();
    history.execute(&mut project, &command).unwrap();
    let mut host = test_support::host();
    let frame = |project: &Project, host: &mut animathio_plugin_host::PluginHost| {
        let scene = host.resolve(&evaluate(project, Time(TICKS_PER_SECOND)), SIZE);
        let mut ctx = animathio_render::RenderCtx::default();
        animathio_render::render(&scene, &mut ctx, SIZE)
            .unwrap()
            .pixels
    };
    let regular = frame(&project, &mut host);
    let edit = Command::SetParam {
        id: added.clip_id,
        name: "font_weight".into(),
        value: Some(800.0),
    };
    history.execute(&mut project, &edit).unwrap();
    let heavy = frame(&project, &mut host);
    assert_ne!(regular, heavy, "the edit changes the rendered frame");
    history.undo(&mut project).unwrap();
    assert_eq!(
        frame(&project, &mut host),
        regular,
        "undo restores the frame"
    );
}
