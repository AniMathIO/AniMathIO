//! Effect chain commands, validation, keyframing and old-file compatibility.
use animathio_core::*;

fn project_with_clip() -> Project {
    let mut project = Project::default();
    Command::AddTrack {
        track: Track::new(TrackId(0), "layer"),
        index: 0,
    }
    .apply(&mut project)
    .unwrap();
    Command::AddClip {
        track: TrackId(0),
        clip: Clip::new(
            ClipId(0),
            ClipKind::Error("fixture".into()),
            TimeRange::new(Time(0), Time(100)).unwrap(),
        ),
    }
    .apply(&mut project)
    .unwrap();
    project
}

fn clip(project: &Project) -> &Clip {
    &project.tracks()[0].clips()[&ClipId(0)]
}

#[test]
fn add_remove_and_set_effect_param_are_reversible() {
    let mut project = project_with_clip();
    let original = project.clone();
    let mut history = History::default();

    let grayscale = EffectInstance::new(EffectId(1), EffectKind::Grayscale);
    history
        .execute(
            &mut project,
            &Command::AddEffect {
                id: ClipId(0),
                index: 0,
                effect: grayscale.clone(),
            },
        )
        .unwrap();
    assert_eq!(clip(&project).effects, vec![grayscale.clone()]);

    history
        .execute(
            &mut project,
            &Command::SetEffectParam {
                id: ClipId(0),
                effect: EffectId(1),
                name: "amount".into(),
                value: Some(0.25),
            },
        )
        .unwrap();
    assert_eq!(clip(&project).effects[0].params["amount"], 0.25);

    history.undo(&mut project).unwrap();
    assert_eq!(clip(&project).effects[0].params["amount"], 1.0);
    history.undo(&mut project).unwrap();
    assert_eq!(clip(&project).effects, Vec::<EffectInstance>::new());
    assert_eq!(project, original);

    history.redo(&mut project).unwrap();
    assert_eq!(clip(&project).effects, vec![grayscale.clone()]);
    history.redo(&mut project).unwrap();
    assert_eq!(clip(&project).effects[0].params["amount"], 0.25);
}

#[test]
fn effect_parameters_are_validated_transactionally() {
    let mut project = project_with_clip();
    let before = project.clone();

    let mut bad = EffectInstance::new(EffectId(1), EffectKind::Grayscale);
    bad.params.insert("amount".into(), 2.0);
    assert!(
        Command::AddEffect {
            id: ClipId(0),
            index: 0,
            effect: bad,
        }
        .apply(&mut project)
        .is_err()
    );

    let mut missing = EffectInstance::new(EffectId(1), EffectKind::Blur);
    missing.params.clear();
    assert!(
        Command::AddEffect {
            id: ClipId(0),
            index: 0,
            effect: missing,
        }
        .apply(&mut project)
        .is_err()
    );
    assert_eq!(project, before);

    // Duplicate ids within one clip are rejected.
    Command::AddEffect {
        id: ClipId(0),
        index: 0,
        effect: EffectInstance::new(EffectId(1), EffectKind::Invert),
    }
    .apply(&mut project)
    .unwrap();
    let with_effect = project.clone();
    assert!(
        Command::AddEffect {
            id: ClipId(0),
            index: 1,
            effect: EffectInstance::new(EffectId(1), EffectKind::Sepia),
        }
        .apply(&mut project)
        .is_err()
    );

    // Removing the only required parameter is rejected by validation.
    assert!(
        Command::SetEffectParam {
            id: ClipId(0),
            effect: EffectId(1),
            name: "amount".into(),
            value: None,
        }
        .apply(&mut project)
        .is_err()
    );

    assert_eq!(project, with_effect);
}

#[test]
fn effect_parameters_are_keyframeable_in_the_evaluated_scene() {
    let mut project = project_with_clip();
    Command::AddEffect {
        id: ClipId(0),
        index: 0,
        effect: EffectInstance::new(EffectId(1), EffectKind::Grayscale),
    }
    .apply(&mut project)
    .unwrap();
    for (time, value) in [(Time(0), 0.0), (Time(100), 1.0)] {
        Command::AddKeyframe {
            id: ClipId(0),
            target: Target::Effect {
                effect: EffectId(1),
                param: "amount".into(),
            },
            time,
            keyframe: Keyframe {
                value: value.into(),
                interpolation: Interpolation::Linear,
            },
        }
        .apply(&mut project)
        .unwrap();
    }

    let scene = evaluate(&project, Time(50));
    assert_eq!(scene.clips[0].effects[0].params["amount"], 0.5);
    assert_eq!(
        evaluate(&project, Time(0)).clips[0].effects[0].params["amount"],
        0.0
    );
}

#[test]
fn removing_an_animated_effect_is_rejected() {
    let mut project = project_with_clip();
    Command::AddEffect {
        id: ClipId(0),
        index: 0,
        effect: EffectInstance::new(EffectId(1), EffectKind::Invert),
    }
    .apply(&mut project)
    .unwrap();
    Command::AddKeyframe {
        id: ClipId(0),
        target: Target::Effect {
            effect: EffectId(1),
            param: "amount".into(),
        },
        time: Time(0),
        keyframe: Keyframe {
            value: (0.5).into(),
            interpolation: Interpolation::Linear,
        },
    }
    .apply(&mut project)
    .unwrap();
    let before = project.clone();
    assert!(
        Command::RemoveEffect {
            id: ClipId(0),
            effect: EffectId(1),
        }
        .apply(&mut project)
        .is_err()
    );
    assert_eq!(project, before);
}

#[test]
fn clips_deserialize_without_effects() {
    let project = project_with_clip();
    let mut value = serde_json::to_value(&project).unwrap();
    value["tracks"][0]["clips"]["0"]
        .as_object_mut()
        .unwrap()
        .remove("effects");
    let decoded: Project = serde_json::from_value(value).unwrap();
    assert_eq!(clip(&decoded).effects, Vec::<EffectInstance>::new());
}
