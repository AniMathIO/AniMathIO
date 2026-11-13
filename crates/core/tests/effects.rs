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

    let grayscale = EffectInstance::basic(EffectId(1), "grayscale");
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
fn effect_identity_and_finiteness_are_validated_transactionally() {
    let mut project = project_with_clip();
    let before = project.clone();

    // Plugin and effect ids must be plain identifiers; parameters must be finite.
    for effect in [
        EffectInstance::new(
            EffectId(1),
            "",
            "grayscale",
            std::collections::BTreeMap::new(),
        ),
        EffectInstance::new(
            EffectId(1),
            "org.animathio.effects-basic",
            "bad id",
            std::collections::BTreeMap::new(),
        ),
        EffectInstance::new(
            EffectId(1),
            "org.animathio.effects-basic",
            "grayscale",
            [("amount".to_owned(), f64::NAN)].into(),
        ),
    ] {
        assert!(
            Command::AddEffect {
                id: ClipId(0),
                index: 0,
                effect,
            }
            .apply(&mut project)
            .is_err()
        );
    }
    assert_eq!(project, before);

    // Parameter ranges belong to the plugin, so the core accepts any finite value.
    Command::AddEffect {
        id: ClipId(0),
        index: 0,
        effect: EffectInstance::new(
            EffectId(1),
            "com.example.fx",
            "swirl",
            [("turns".to_owned(), -3.5)].into(),
        ),
    }
    .apply(&mut project)
    .unwrap();

    // Duplicate ids within one clip are rejected.
    let with_effect = project.clone();
    assert!(
        Command::AddEffect {
            id: ClipId(0),
            index: 1,
            effect: EffectInstance::basic(EffectId(1), "sepia"),
        }
        .apply(&mut project)
        .is_err()
    );
    assert_eq!(project, with_effect);
}

#[test]
fn animated_effect_parameters_cannot_be_removed() {
    let mut project = project_with_clip();
    Command::AddEffect {
        id: ClipId(0),
        index: 0,
        effect: EffectInstance::basic(EffectId(1), "invert"),
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
        Command::SetEffectParam {
            id: ClipId(0),
            effect: EffectId(1),
            name: "amount".into(),
            value: None,
        }
        .apply(&mut project)
        .is_err()
    );
    assert_eq!(project, before);
}

#[test]
fn effect_parameters_are_keyframeable_in_the_evaluated_scene() {
    let mut project = project_with_clip();
    Command::AddEffect {
        id: ClipId(0),
        index: 0,
        effect: EffectInstance::basic(EffectId(1), "grayscale"),
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
        effect: EffectInstance::basic(EffectId(1), "invert"),
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
