//! Composition layout, command history, control dispatch and CPU golden frames.
use animathio_control::{
    Call, ControlHandler, ExportPreset, HeadlessHandler, TitleAddParams, title_preset_command,
    title_presets,
};
use animathio_core::{
    ClipKind, Command, History, Project, TICKS_PER_SECOND, Time, TimeRange, evaluate,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

fn params(preset: &str) -> TitleAddParams {
    TitleAddParams {
        preset: preset.into(),
        fields: BTreeMap::new(),
        range: TimeRange::new(Time(5 * TICKS_PER_SECOND), Time(10 * TICKS_PER_SECOND)).unwrap(),
    }
}

#[test]
fn all_aspects_apply_as_one_linked_undo_step() {
    assert_eq!(title_presets().len(), 8);
    let mut ids = BTreeSet::new();
    for preset in title_presets() {
        assert!(ids.insert(&preset.id));
    }
    for format in ExportPreset::ALL {
        for preset in title_presets() {
            let mut project = Project::default();
            format
                .command(&project)
                .unwrap()
                .apply(&mut project)
                .unwrap();
            let before = project.clone();
            let (command, added) = title_preset_command(&project, &params(&preset.id)).unwrap();
            let serialized = serde_json::to_string(&command).unwrap();
            let command: Command = serde_json::from_str(&serialized).unwrap();
            let mut history = History::default();
            history.execute(&mut project, &command).unwrap();
            let clips: Vec<_> = project
                .tracks()
                .iter()
                .flat_map(|t| t.clips().values())
                .collect();
            assert_eq!(clips.len(), preset.layers.len());
            assert!(clips.iter().all(|c| c.link_group == clips[0].link_group));
            assert!(clips.iter().any(|c| c.id == added.clip_id));
            for (clip, layer) in clips.iter().zip(&preset.layers) {
                assert!(matches!(clip.kind, ClipKind::Plugin { .. }));
                assert_eq!(clip.range, params(&preset.id).range);
                assert_eq!(clip.animations.len(), preset.curves.len());
                if layer.kind == "text" {
                    // Text's source starts at the top of its scaled unit square.
                    let top = clip.transform.y - clip.transform.scale_y / 2.;
                    assert!((0.08..=0.92).contains(&top));
                    let left = clip.transform.x - clip.transform.scale_x / 2.;
                    assert!(left >= 0.079);
                    assert!(clip.transform.x + clip.transform.scale_x / 2. <= 0.921);
                }
            }
            history.undo(&mut project).unwrap();
            assert_eq!(project, before);
            history.redo(&mut project).unwrap();
            assert_eq!(project.tracks().len(), preset.layers.len());
        }
    }
}

#[test]
fn controls_validate_fields_and_keep_fresh_link_groups() {
    let mut handler = HeadlessHandler::new();
    let list = handler.handle(Call::TitlesList).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 8);
    let before = handler.project_get().unwrap().project;
    let mut p = params("lower_third");
    p.fields.insert("name".into(), "Jane Doe".into());
    p.fields.insert("role".into(), "Researcher".into());
    handler
        .handle(Call::TitlesAdd { params: p.clone() })
        .unwrap();
    let first = handler.project_get().unwrap().project;
    assert_eq!(first.tracks().len(), 3);
    handler.history_undo().unwrap();
    assert_eq!(handler.project_get().unwrap().project, before);
    handler.history_redo().unwrap();
    handler
        .handle(Call::TitlesAdd { params: p.clone() })
        .unwrap();
    let second = handler.project_get().unwrap().project;
    let groups: BTreeSet<_> = second
        .tracks()
        .iter()
        .flat_map(|t| t.clips().values().map(|c| c.link_group))
        .collect();
    assert_eq!(groups.len(), 2);
    p.fields.insert("unknown".into(), "bad".into());
    assert!(handler.handle(Call::TitlesAdd { params: p }).is_err());
    assert_eq!(handler.project_get().unwrap().project, second);
}

#[test]
fn linked_title_moves_together_and_rejects_invalid_requests() {
    let mut project = Project::default();
    for p in [
        params("missing"),
        TitleAddParams {
            fields: BTreeMap::from([("title".into(), " ".into())]),
            ..params("centered_title")
        },
    ] {
        assert!(title_preset_command(&project, &p).is_err());
    }
    let (command, added) = title_preset_command(&project, &params("lower_third")).unwrap();
    command.apply(&mut project).unwrap();
    let before = project.clone();
    let inverse = Command::MoveClip {
        id: added.clip_id,
        track: added.track_id,
        start: Time(12 * TICKS_PER_SECOND),
    }
    .apply(&mut project)
    .unwrap();
    assert!(
        project
            .tracks()
            .iter()
            .flat_map(|t| t.clips().values())
            .all(|c| c.range.start == Time(12 * TICKS_PER_SECOND))
    );
    inverse.apply(&mut project).unwrap();
    assert_eq!(project, before);
    // Imported/custom link groups need not refer to a live clip identity.
    for id in project
        .tracks()
        .iter()
        .flat_map(|t| t.clips().keys())
        .copied()
        .collect::<Vec<_>>()
    {
        Command::SetClipLink {
            id,
            group: Some(animathio_core::ClipId(1000)),
        }
        .apply(&mut project)
        .unwrap();
    }
    let (command, _) = title_preset_command(&project, &params("lower_third")).unwrap();
    command.apply(&mut project).unwrap();
    let groups: BTreeSet<_> = project
        .tracks()
        .iter()
        .flat_map(|t| t.clips().values().map(|c| c.link_group))
        .collect();
    assert_eq!(groups.len(), 2);
}

#[test]
fn title_cpu_goldens_and_safe_areas() {
    let mut host = animathio_plugin_host::test_support::host();
    let mut ctx = animathio_render::RenderCtx::default();
    let mut failures = Vec::new();
    for format in ExportPreset::ALL {
        let (width, height) = format.resolution();
        let size = (width / 6, height / 6);
        for preset in title_presets() {
            let id = preset.id.as_str();
            let mut project = Project::default();
            format
                .command(&project)
                .unwrap()
                .apply(&mut project)
                .unwrap();
            let (command, _) = title_preset_command(&project, &params(id)).unwrap();
            command.apply(&mut project).unwrap();
            let scene = host.resolve(&evaluate(&project, Time(6 * TICKS_PER_SECOND)), size);
            assert!(
                scene
                    .clips
                    .iter()
                    .all(|c| !matches!(c.kind, ClipKind::Error(_)))
            );
            let frame = animathio_render::render(&scene, &mut ctx, size).unwrap();
            let bg = project.background();
            let mut visible = 0;
            for (index, pixel) in frame.pixels.as_chunks::<4>().0.iter().enumerate() {
                if pixel[..3] != bg[..3] && id != "chapter_card" {
                    let x = u32::try_from(index).unwrap() % size.0;
                    let y = u32::try_from(index).unwrap() / size.0;
                    assert!(
                        f64::from(x) >= f64::from(size.0) * 0.07
                            && f64::from(x) <= f64::from(size.0) * 0.93,
                        "{id} {size:?} x={x}"
                    );
                    assert!(
                        f64::from(y) >= f64::from(size.1) * 0.07
                            && f64::from(y) <= f64::from(size.1) * 0.93,
                        "{id} {size:?} y={y}"
                    );
                    visible += 1;
                }
            }
            assert!(visible > 20 || id == "chapter_card", "empty preset {id}");
            if format == ExportPreset::Youtube1080p
                && ["centered_title", "lower_third", "caption"].contains(&id)
            {
                let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
                let path = root.join("golden").join(format!("title-{id}.png"));
                if std::env::var("UPDATE_GOLDEN").as_deref() == Ok("1") {
                    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                    frame.save_png(&path).unwrap();
                    let fixture = root.join("fixtures").join(format!("title-{id}.json"));
                    std::fs::create_dir_all(fixture.parent().unwrap()).unwrap();
                    animathio_format::write_json(std::fs::File::create(fixture).unwrap(), &project)
                        .unwrap();
                }
                let expected = image_pixels(&path);
                assert_eq!(frame.pixels.len(), expected.len());
                let (mut count, mut max, mut x0, mut y0, mut x1, mut y1) =
                    (0_u32, 0_u8, u32::MAX, u32::MAX, 0_u32, 0_u32);
                for (index, (a, b)) in frame
                    .pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(expected.as_chunks::<4>().0.iter())
                    .enumerate()
                {
                    let diff = (0..4).map(|c| a[c].abs_diff(b[c])).max().unwrap();
                    if diff > 2 {
                        let x = u32::try_from(index).unwrap() % size.0;
                        let y = u32::try_from(index).unwrap() / size.0;
                        (count, max) = (count + 1, max.max(diff));
                        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
                    }
                }
                if count > 0 {
                    let actual = std::env::temp_dir().join(format!("title-{id}-actual.png"));
                    frame.save_png(&actual).unwrap();
                    failures.push(format!(
                        "golden changed: {} ({count} px over tolerance, max diff {max}, bbox \
                         {x0},{y0}..{x1},{y1}, actual saved to {})",
                        path.display(),
                        actual.display()
                    ));
                    use base64::Engine as _;
                    failures.push(format!(
                        "actual {id} png base64: {}",
                        base64::engine::general_purpose::STANDARD
                            .encode(std::fs::read(&actual).unwrap())
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn image_pixels(path: &Path) -> Vec<u8> {
    image::open(path).unwrap().to_rgba8().into_raw()
}

#[test]
fn edited_multiline_text_stays_in_its_safe_box() {
    let mut host = animathio_plugin_host::test_support::host();
    let mut ctx = animathio_render::RenderCtx::default();
    for format in [
        ExportPreset::Youtube1080p,
        ExportPreset::ShortsVertical,
        ExportPreset::Square,
    ] {
        let mut project = Project::default();
        format
            .command(&project)
            .unwrap()
            .apply(&mut project)
            .unwrap();
        let (command, _) = title_preset_command(&project, &params("lower_third")).unwrap();
        command.apply(&mut project).unwrap();
        let id = project.tracks().iter().flat_map(|t| t.clips().values()).find(|c| matches!(&c.kind, ClipKind::Plugin { params, .. } if params["_title_field"] == "name")).unwrap().id;
        Command::SetPluginParam { id, name: "text".into(), value: Some(animathio_core::ParamValue::String("A very long speaker name that must fit safely\nA second line that should not overlap the role".into())) }.apply(&mut project).unwrap();
        let (w, h) = format.resolution();
        let size = (w / 6, h / 6);
        let mut scene = evaluate(&project, Time(6 * TICKS_PER_SECOND));
        scene.clips.retain(|clip| clip.id == id);
        let scene = host.resolve(&scene, size);
        let frame = animathio_render::render(&scene, &mut ctx, size).unwrap();
        let bg = project.background();
        let pixels: Vec<_> = frame
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .filter(|(_, pixel)| pixel[..3] != bg[..3])
            .map(|(i, _)| {
                (
                    u32::try_from(i).unwrap() % size.0,
                    u32::try_from(i).unwrap() / size.0,
                )
            })
            .collect();
        assert_ne!(pixels, []);
        assert!(
            pixels
                .iter()
                .all(|(x, y)| f64::from(*x) >= f64::from(size.0) * 0.07
                    && f64::from(*x) <= f64::from(size.0) * 0.93
                    && f64::from(*y) >= f64::from(size.1) * 0.735
                    && f64::from(*y) <= f64::from(size.1) * 0.815)
        );
    }
}

/// Plain `add_title` text (no preset) is centred on its clip position: the ink
/// sits around the transform position instead of hanging from the canvas origin.
#[test]
fn default_add_title_is_centred_with_goldens() {
    let mut host = animathio_plugin_host::test_support::host();
    let mut ctx = animathio_render::RenderCtx::default();
    for (format, name) in [
        (ExportPreset::Youtube1080p, "16x9"),
        (ExportPreset::ShortsVertical, "9x16"),
    ] {
        let mut project = Project::default();
        format
            .command(&project)
            .unwrap()
            .apply(&mut project)
            .unwrap();
        let range =
            TimeRange::new(Time(5 * TICKS_PER_SECOND), Time(10 * TICKS_PER_SECOND)).unwrap();
        let (command, _, _) = animathio_control::title_command(
            &project,
            "AniMathIO Title",
            range,
            animathio_control::TitleStyle::Title,
        )
        .unwrap();
        command.apply(&mut project).unwrap();
        let (w, h) = format.resolution();
        let size = (w / 6, h / 6);
        let scene = host.resolve(&evaluate(&project, Time(6 * TICKS_PER_SECOND)), size);
        let frame = animathio_render::render(&scene, &mut ctx, size).unwrap();
        let bg = project.background();
        let (mut min_x, mut max_x, mut min_y, mut max_y) = (u32::MAX, 0, u32::MAX, 0);
        for (index, pixel) in frame.pixels.as_chunks::<4>().0.iter().enumerate() {
            if pixel[..3] != bg[..3] {
                let x = u32::try_from(index).unwrap() % size.0;
                let y = u32::try_from(index).unwrap() / size.0;
                (min_x, max_x) = (min_x.min(x), max_x.max(x));
                (min_y, max_y) = (min_y.min(y), max_y.max(y));
            }
        }
        assert!(max_x > min_x, "{name}: no ink");
        // Fully on canvas and centred (title preset centre is 0.5, 0.5) within a few percent.
        assert!(
            min_x > 0 && max_x + 1 < size.0 && min_y > 0 && max_y + 1 < size.1,
            "{name}"
        );
        let cx = f64::from(min_x + max_x) / 2.0 / f64::from(size.0);
        let cy = f64::from(min_y + max_y) / 2.0 / f64::from(size.1);
        assert!((cx - 0.5).abs() < 0.03, "{name}: ink centre x {cx}");
        assert!((cy - 0.5).abs() < 0.04, "{name}: ink centre y {cy}");
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
        let path = root
            .join("golden")
            .join(format!("title-default-{name}.png"));
        if std::env::var("UPDATE_GOLDEN").as_deref() == Ok("1") {
            frame.save_png(&path).unwrap();
        }
        let expected = image_pixels(&path);
        assert_eq!(frame.pixels.len(), expected.len());
        assert!(
            frame
                .pixels
                .iter()
                .zip(expected)
                .all(|(a, b)| a.abs_diff(b) <= 2),
            "golden changed: {}",
            path.display()
        );
    }
}
