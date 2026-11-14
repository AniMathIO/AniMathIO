use animathio_control::{
    MathObjectDraft, MathObjectKind, math_object_command, math_object_draft, math_object_range,
};
use animathio_core::{Command, History, Project, Time};

#[test]
fn every_object_maps_to_a_centered_clip_and_undoes_exactly() {
    for (width, height) in [(1920, 1080), (1080, 1920)] {
        for kind in MathObjectKind::ALL {
            let mut project = Project::default();
            Command::SetSettings {
                width,
                height,
                frame_rate: project.frame_rate(),
                background: project.background(),
                duration: Time(0),
            }
            .apply(&mut project)
            .unwrap();
            let before = project.clone();
            let draft = MathObjectDraft::new(kind);
            let range = math_object_range(Time(123)).unwrap();
            let (command, added) = math_object_command(&project, &draft, range, None).unwrap();
            assert!(matches!(command, Command::Batch { .. }));
            let mut history = History::default();
            history.execute(&mut project, &command).unwrap();
            let clip = project.tracks()[0].clips().get(&added.clip_id).unwrap();
            assert_eq!(clip.range, range);
            assert_eq!(clip.transform.x, 0.5);
            assert_eq!(clip.transform.y, 0.5);
            assert_eq!(math_object_draft(clip, (width, height)), Some(draft));
            history.undo(&mut project).unwrap();
            assert_eq!(project, before, "{kind:?}");
            history.redo(&mut project).unwrap();
            assert_eq!(project.tracks().len(), 1);
        }
    }
}

#[test]
fn adds_on_a_free_layer_and_edits_as_one_undo_step() {
    let mut project = Project::default();
    let mut history = History::default();
    let range = math_object_range(Time(0)).unwrap();
    let draft = MathObjectDraft::new(MathObjectKind::Circle);
    let (command, first) = math_object_command(&project, &draft, range, None).unwrap();
    history.execute(&mut project, &command).unwrap();
    let (command, second) = math_object_command(&project, &draft, range, None).unwrap();
    history.execute(&mut project, &command).unwrap();
    assert_ne!(first.track_id, second.track_id);
    assert_eq!(project.tracks().len(), 2);
    let before = project.clone();
    let mut changed = draft;
    changed.fields.insert("radius".into(), "3".into());
    changed.color = "#ff0000".into();
    let (command, added) =
        math_object_command(&project, &changed, range, Some(first.clip_id)).unwrap();
    assert_eq!(added, first);
    history.execute(&mut project, &command).unwrap();
    assert_eq!(
        math_object_draft(
            project.tracks()[0].clips().get(&first.clip_id).unwrap(),
            animathio_export::resolution(&project)
        ),
        Some(changed)
    );
    history.undo(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn rejects_invalid_fields_without_mutating_project() {
    let project = Project::default();
    let before = project.clone();
    let range = math_object_range(Time(0)).unwrap();
    for (kind, field, value) in [
        (MathObjectKind::Circle, "radius", "-1"),
        (MathObjectKind::Point, "x", "NaN"),
        (MathObjectKind::Ellipse, "ry", "0"),
        (MathObjectKind::Polygon, "vertices", "1,2; 2,3"),
        (MathObjectKind::Plot, "min", "10"),
        (MathObjectKind::Polar, "extent", "99"),
    ] {
        let mut draft = MathObjectDraft::new(kind);
        draft.fields.insert(field.into(), value.into());
        assert!(math_object_command(&project, &draft, range, None).is_err());
    }
    assert_eq!(project, before);
}

#[test]
fn geometry_preserves_math_units_in_landscape_and_portrait() {
    for size in [(1920, 1080), (1080, 1920)] {
        let draft = MathObjectDraft::new(MathObjectKind::Circle);
        let (plugin, params) = draft.parameters(size).unwrap();
        assert_eq!(plugin, "org.animathio.shapes");
        let w = params["width"].as_f64().unwrap() * f64::from(size.0);
        let h = params["height"].as_f64().unwrap() * f64::from(size.1);
        assert!((w - h).abs() < 1e-8);
        assert!((w / f64::from(size.0.min(size.1)) - 0.32).abs() < 1e-8);
    }
}

#[test]
fn formula_palette_typesets_and_write_preset_is_atomic() {
    use animathio_control::{
        AnimationApplyParams, LATEX_SNIPPETS, LATEX_TEMPLATES, animation_preset_command,
    };
    use animathio_core::TICKS_PER_SECOND;
    use std::collections::BTreeMap;
    let mut host = animathio_plugin_host::test_support::host();
    for &(label, source) in LATEX_SNIPPETS.iter().chain(LATEX_TEMPLATES) {
        let mut draft = MathObjectDraft::new(MathObjectKind::Latex);
        draft.fields.insert("source".into(), source.into());
        let (plugin, params) = draft.parameters((640, 360)).unwrap();
        host.render_clip(&plugin, &BTreeMap::new(), &params, 0, (640, 360))
            .unwrap_or_else(|e| panic!("{label}: {e}"));
    }
    let mut project = Project::default();
    let before = project.clone();
    let draft = MathObjectDraft::new(MathObjectKind::Latex);
    let (add, added) =
        math_object_command(&project, &draft, math_object_range(Time(0)).unwrap(), None).unwrap();
    let mut candidate = project.clone();
    add.apply(&mut candidate).unwrap();
    let (animation, _) = animation_preset_command(
        &candidate,
        Some(&host),
        &AnimationApplyParams {
            clip: added.clip_id,
            preset: "typewriter-in".into(),
            plugin: None,
            duration: Some(Time(TICKS_PER_SECOND)),
            options: BTreeMap::new(),
        },
    )
    .unwrap();
    let mut history = History::default();
    history
        .execute(
            &mut project,
            &Command::Batch {
                commands: vec![add, animation],
            },
        )
        .unwrap();
    let evaluated = animathio_core::evaluate(&project, Time(TICKS_PER_SECOND / 2));
    assert!((evaluated.clips[0].params["reveal"].as_number().unwrap() - 0.5).abs() < 1e-8);
    history.undo(&mut project).unwrap();
    assert_eq!(project, before);
}

#[test]
fn each_object_edits_without_losing_timing_or_transform_and_undoes_exactly() {
    use animathio_core::{
        Interpolation, Keyframe, ParamValue, TICKS_PER_SECOND, Target, Transform,
    };
    for kind in MathObjectKind::ALL {
        let mut project = Project::default();
        let range = math_object_range(Time(500)).unwrap();
        let draft = MathObjectDraft::new(kind);
        let (add, added) = math_object_command(&project, &draft, range, None).unwrap();
        add.apply(&mut project).unwrap();
        Command::SetTransform {
            id: added.clip_id,
            transform: Transform {
                x: 0.3,
                scale_x: 0.7,
                ..Transform::default()
            },
        }
        .apply(&mut project)
        .unwrap();
        Command::AddKeyframe {
            id: added.clip_id,
            target: Target::Param("reveal".into()),
            time: Time(TICKS_PER_SECOND),
            keyframe: Keyframe {
                value: ParamValue::Number(0.5),
                interpolation: Interpolation::Linear,
            },
        }
        .apply(&mut project)
        .unwrap();
        let before = project.clone();
        let mut changed = draft;
        changed.color = "#ff0055".into();
        let (edit, _) =
            math_object_command(&project, &changed, range, Some(added.clip_id)).unwrap();
        let undo = edit.apply(&mut project).unwrap();
        let clip = project.tracks()[0].clips().get(&added.clip_id).unwrap();
        assert_eq!(clip.range, range);
        assert!((clip.transform.x - 0.3).abs() < 1e-8);
        assert_eq!(
            clip.animations,
            before.tracks()[0].clips()[&added.clip_id].animations
        );
        undo.apply(&mut project).unwrap();
        assert_eq!(project, before, "{kind:?}");
    }
}
