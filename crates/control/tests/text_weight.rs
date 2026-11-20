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

// ---------------------------------------------------------------------------
// Typeface (`font`), glyph fallback, limits and layout parity.
// ---------------------------------------------------------------------------

type Bbox = (u32, u32, u32, u32);

/// A one-clip scene with arbitrary text plugin parameters on top of the normalized defaults.
fn scene_with(text: &str, params: &[(&str, serde_json::Value)]) -> Scene {
    let mut scene = scene(text, None, false);
    if let ClipKind::Plugin { params: map, .. } = &mut scene.clips[0].kind {
        map["align"] = "center".into();
        for (name, value) in params {
            map[*name] = value.clone();
        }
    }
    scene
}

/// Resolve and render, returning the pixels or the plugin error message.
fn render_scene(scene: &Scene) -> Result<Vec<u8>, String> {
    let mut host = test_support::host();
    let resolved = host.resolve(scene, SIZE);
    if let ClipKind::Error(message) = &resolved.clips[0].kind {
        return Err(message.clone());
    }
    let mut ctx = animathio_render::RenderCtx::default();
    Ok(animathio_render::render(&resolved, &mut ctx, SIZE)
        .unwrap()
        .pixels)
}

/// Bounding box of lit pixels within the pixel rows `rows`.
fn bbox_in_rows(pixels: &[u8], rows: std::ops::Range<u32>) -> Option<Bbox> {
    let mut bbox: Option<Bbox> = None;
    for (index, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
        let (x, y) = (
            u32::try_from(index).unwrap() % SIZE.0,
            u32::try_from(index).unwrap() / SIZE.0,
        );
        if rows.contains(&y) && pixel[0] > 127 {
            let b = bbox.get_or_insert((x, y, x, y));
            *b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
        }
    }
    bbox
}

fn lit(pixels: &[u8]) -> u64 {
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| u64::from(pixel[0]))
        .sum()
}

fn ink_of(text: &str, params: &[(&str, serde_json::Value)]) -> u64 {
    lit(&render_scene(&scene_with(text, params)).unwrap())
}

#[test]
fn characters_missing_from_inter_render_from_the_fallback_face_not_notdef() {
    // U+10FFFE is in neither face: it draws DejaVu's .notdef box, the reference "missing glyph".
    let notdef = render_scene(&scene_with("\u{10FFFE}", &[("font_weight", 700.0.into())])).unwrap();
    for font in ["dejavu", "inter"] {
        let params = [("font_weight", 700.0.into()), ("font", font.into())];
        for ch in "∀∃∈∉⊂⊆∪∩∇∮ℝℕℤℚא".chars() {
            let pixels = render_scene(&scene_with(&ch.to_string(), &params)).unwrap();
            assert!(lit(&pixels) > 0, "{ch} draws ink ({font})");
            assert_ne!(pixels, notdef, "{ch} must not be the .notdef box ({font})");
        }
        // The whole expression has ink and no box-shaped run: it is wider than "x" alone.
        let expr = ink_of("∀x ∈ ℝ", &params);
        assert!(expr > ink_of("x", &params), "{font}");
    }
}

#[test]
fn font_decides_the_typeface_and_weight_does_not() {
    // `font = inter` outlines regular too: it is not the host's DejaVu text run.
    let mut host = test_support::host();
    let inter = host.resolve(
        &scene_with("Body", &[("font", "inter".into())]),
        SIZE,
    );
    let ClipKind::Vector(vector) = &inter.clips[0].kind else {
        panic!("inter text failed")
    };
    assert!(matches!(vector.elements[0], VectorElement::FillPath { .. }));
    // An absent `font` is dejavu: regular stays the host text run, so old projects are unchanged.
    let dejavu = host.resolve(&scene_with("Body", &[]), SIZE);
    let ClipKind::Vector(vector) = &dejavu.clips[0].kind else {
        panic!("dejavu text failed")
    };
    assert!(matches!(vector.elements[0], VectorElement::TextRun(_)));
    assert_eq!(
        render_scene(&scene_with("Body", &[("font", "dejavu".into())])).unwrap(),
        render_scene(&scene_with("Body", &[])).unwrap()
    );
    assert_ne!(
        render_scene(&scene_with("Body", &[("font", "inter".into())])).unwrap(),
        render_scene(&scene_with("Body", &[])).unwrap()
    );
}

#[test]
fn keyframed_weight_on_an_inter_clip_never_switches_typeface() {
    use animathio_control::{TitleAddParams, title_preset_command};
    use animathio_core::{
        Command, History, Interpolation, Keyframe, Project, TICKS_PER_SECOND, Target, TimeRange,
        evaluate,
    };
    let mut project = Project::default();
    let params = TitleAddParams {
        preset: "typography_body".into(),
        fields: BTreeMap::new(),
        range: TimeRange::new(Time(0), Time(5 * TICKS_PER_SECOND)).unwrap(),
    };
    let (command, added) = title_preset_command(&project, &params).unwrap();
    let mut history = History::default();
    history.execute(&mut project, &command).unwrap();
    for (time, weight) in [(0, 300.0), (TICKS_PER_SECOND, 500.0)] {
        history
            .execute(
                &mut project,
                &Command::AddKeyframe {
                    id: added.clip_id,
                    target: Target::Param("font_weight".into()),
                    time: Time(time),
                    keyframe: Keyframe {
                        value: ParamValue::Number(weight),
                        interpolation: Interpolation::Linear,
                    },
                },
            )
            .unwrap();
    }
    let mut host = test_support::host();
    let mut inks = Vec::new();
    // 300 -> 500 across 0..1 s in tenths, crossing 400 where the dejavu typeface would jump.
    for step in 0..=10 {
        let time = Time(TICKS_PER_SECOND * step / 10);
        let scene = host.resolve(&evaluate(&project, time), SIZE);
        let ClipKind::Vector(vector) = &scene.clips[0].kind else {
            panic!("title failed: {:?}", scene.clips[0].kind)
        };
        assert!(
            vector
                .elements
                .iter()
                .all(|element| matches!(element, VectorElement::FillPath { .. })),
            "step {step}: every keyframed weight stays outlined Inter paths"
        );
        let mut ctx = animathio_render::RenderCtx::default();
        let pixels = animathio_render::render(&scene, &mut ctx, SIZE)
            .unwrap()
            .pixels;
        inks.push(lit(&pixels) as f64);
    }
    for pair in inks.windows(2) {
        let ratio = pair[1] / pair[0];
        assert!(
            (1.0..1.10).contains(&ratio),
            "ink grows smoothly with the weight, no typeface jump: {inks:?}"
        );
    }
}

#[test]
fn weighted_captions_use_the_weight() {
    let words = "0:300 300:600 600:900 900:1200";
    for style in ["word-highlight", "karaoke", "pop", "one-word", "bold-outline"] {
        let render = |weight: f64, font: &str| {
            let scene = scene_with(
                "bold caption words",
                &[
                    ("fit_width", 0.9.into()),
                    ("fit_height", 0.4.into()),
                    ("caption_style", style.into()),
                    ("words", words.into()),
                    ("font_weight", weight.into()),
                    ("font", font.into()),
                ],
            );
            lit(&render_scene(&scene).unwrap())
        };
        let regular = render(400.0, "inter");
        let bold = render(800.0, "inter");
        assert!(regular > 0, "{style}: regular inter caption is visible");
        assert!(bold > regular, "{style}: {regular} < {bold}");
        // Dejavu keeps the previous behaviour: weight 400 is the host face, 800 is Inter.
        assert!(render(400.0, "dejavu") > 0, "{style}");
    }
}

#[test]
fn pixel_unit_text_honours_weight_and_alignment() {
    let pixel = |weight: f64, align: &str| {
        scene_with(
            "Pixel text",
            &[
                ("units", "pixels".into()),
                ("font_size", 48.0.into()),
                ("font_weight", weight.into()),
                ("align", align.into()),
            ],
        )
    };
    let regular = lit(&render_scene(&pixel(400.0, "center")).unwrap());
    let bold = lit(&render_scene(&pixel(800.0, "center")).unwrap());
    assert!(regular > 0 && bold > regular, "{regular} < {bold}");
    let rows = 0..SIZE.1;
    let side = |align: &str| {
        bbox_in_rows(&render_scene(&pixel(800.0, align)).unwrap(), rows.clone()).unwrap()
    };
    let (left, centre, right) = (side("left"), side("center"), side("right"));
    assert!(left.0 <= 1, "left aligned starts at the edge: {left:?}");
    assert!(right.2 >= SIZE.0 - 2, "right aligned ends at the edge: {right:?}");
    let middle = f64::from(centre.0 + centre.2) / 2.0;
    assert!((middle - f64::from(SIZE.0) / 2.0).abs() < 3.0, "{centre:?}");
}

#[test]
fn weighted_lines_align_left_centre_and_right_inside_the_block() {
    let text = "Short\nA considerably longer second line";
    let lines = |align: &str| {
        let pixels = render_scene(&scene_with(
            text,
            &[("font_weight", 700.0.into()), ("align", align.into())],
        ))
        .unwrap();
        let all = bbox_in_rows(&pixels, 0..SIZE.1).unwrap();
        let middle = (all.1 + all.3) / 2;
        (
            bbox_in_rows(&pixels, 0..middle).unwrap(),
            bbox_in_rows(&pixels, middle..SIZE.1).unwrap(),
        )
    };
    let (first, second) = lines("left");
    assert!(first.0.abs_diff(second.0) <= 6, "left: {first:?} {second:?}");
    assert!(first.2 < second.2);
    let (first, second) = lines("right");
    assert!(first.2.abs_diff(second.2) <= 6, "right: {first:?} {second:?}");
    assert!(first.0 > second.0);
    let (first, second) = lines("center");
    let (a, b) = (first.0 + first.2, second.0 + second.2);
    assert!(a.abs_diff(b) <= 12, "centre: {first:?} {second:?}");
}

#[test]
fn weighted_layout_matches_the_host_for_blanks_and_trailing_newlines() {
    // The host keeps repeated blanks and a trailing newline; weighted text must too, so
    // switching the weight changes the typeface but not the layout.
    let wide = |text: &str, weight: f64| {
        let pixels = render_scene(&scene_with(text, &[("font_weight", weight.into())])).unwrap();
        let b = bbox_in_rows(&pixels, 0..SIZE.1).unwrap();
        (b.2 - b.0, b.3 - b.1, (b.1 + b.3) / 2)
    };
    for weight in [400.0, 700.0] {
        let single = wide("a b", weight);
        let spaced = wide("a    b", weight);
        assert!(spaced.0 > single.0 + 10, "w{weight}: repeated blanks keep width");
        let tabbed = wide("a\tb", weight);
        assert!(tabbed.0 > single.0, "w{weight}: a tab is not dropped");
        // A trailing newline adds an empty line: the block (and so the ink) moves up.
        let plain = wide("Hi", weight);
        let trailing = wide("Hi\n", weight);
        assert!(trailing.2 < plain.2 - 5, "w{weight}: {plain:?} {trailing:?}");
        // Trailing blanks on the last line still count towards centring (they only hang at
        // a soft wrap), so the ink moves left.
        let left_edge = |text: &str| {
            let pixels =
                render_scene(&scene_with(text, &[("font_weight", weight.into())])).unwrap();
            bbox_in_rows(&pixels, 0..SIZE.1).unwrap().0
        };
        assert!(left_edge("Hi    ") + 10 < left_edge("Hi"), "w{weight}");
        // A leading blank pushes the ink to the right under left alignment.
        let lead = |text: &str| {
            let pixels = render_scene(&scene_with(
                text,
                &[("font_weight", weight.into()), ("align", "left".into())],
            ))
            .unwrap();
            bbox_in_rows(&pixels, 0..SIZE.1).unwrap().0
        };
        assert!(lead("    Hi") > lead("Hi") + 10, "w{weight}");
    }
}

#[test]
fn weighted_text_rejects_invalid_sizes_and_over_long_text_like_the_host() {
    for size in [0.0, -1.0] {
        let result = render_scene(&scene_with(
            "x",
            &[("font_weight", 700.0.into()), ("font_size", size.into())],
        ));
        assert!(result.is_err(), "font size {size} is rejected");
    }
    let result = render_scene(&scene_with(
        &".".repeat(10_001),
        &[("font_weight", 700.0.into()), ("font_size", 0.01.into())],
    ));
    assert!(result.is_err(), "more than 10,000 characters is rejected");
    let result = render_scene(&scene_with(
        &".".repeat(10_001),
        &[
            ("font_weight", 700.0.into()),
            ("units", "pixels".into()),
            ("font_size", 8.0.into()),
        ],
    ));
    assert!(result.is_err(), "pixel text has the same cap");
}

#[test]
fn a_two_thousand_character_paragraph_at_weight_700_fits_the_sandbox_budgets() {
    // 50M fuel per call and 100,000 verbs per path: the plugin caches glyph outlines and
    // splits the fill into several paths. A failure would show up as an Error clip.
    let text = "The quick brown fox jumps over the lazy dog, then ∀x ∈ ℝ does it again. ".repeat(28);
    assert!(text.chars().count() > 2000);
    let normalized = scene_with(
        &text,
        &[("font_weight", 700.0.into()), ("font_size", 0.03.into())],
    );
    let mut host = test_support::host();
    let resolved = host.resolve(&normalized, SIZE);
    let ClipKind::Vector(vector) = &resolved.clips[0].kind else {
        panic!("paragraph failed: {:?}", resolved.clips[0].kind)
    };
    assert!(vector.elements.len() > 1, "split into several paths");
    assert!(vector.elements.len() <= animathio_core::MAX_VECTOR_ELEMENTS);
    for element in &vector.elements {
        let VectorElement::FillPath { path, .. } = element else {
            panic!("paths expected")
        };
        assert!(path.len() <= animathio_core::MAX_PATH_VERBS);
    }
    vector.validate().unwrap();
    assert!(lit(&render_scene(&normalized).unwrap()) > 0);
    // The same text on one pixel-unit line (no wrapping) also stays inside the budgets.
    let pixels = scene_with(
        &text,
        &[
            ("font_weight", 700.0.into()),
            ("units", "pixels".into()),
            ("font_size", 10.0.into()),
        ],
    );
    let resolved = host.resolve(&pixels, SIZE);
    let ClipKind::Vector(vector) = &resolved.clips[0].kind else {
        panic!("pixel paragraph failed: {:?}", resolved.clips[0].kind)
    };
    vector.validate().unwrap();
}
