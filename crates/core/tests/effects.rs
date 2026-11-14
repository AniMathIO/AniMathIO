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

fn audio_effect(id: u64, effect: &str, value: f64) -> EffectInstance {
    EffectInstance::new(
        EffectId(id),
        BASIC_AUDIO_EFFECTS_PLUGIN,
        effect,
        std::collections::BTreeMap::from([("gain_db".to_owned(), value)]),
    )
}

#[test]
fn audio_effect_chain_commands_are_reversible_on_clips_and_tracks() {
    for chain in [AudioChain::Clip(ClipId(0)), AudioChain::Track(TrackId(0))] {
        let mut project = project_with_clip();
        let original = project.clone();
        let mut history = History::default();
        let chain_of = |project: &Project| -> Vec<EffectInstance> {
            match chain {
                AudioChain::Clip(_) => clip(project).audio_effects.clone(),
                AudioChain::Track(_) => project.tracks()[0].audio_effects().to_vec(),
            }
        };
        let gain = audio_effect(1, "gain", 3.0);
        let eq = audio_effect(2, "eq3", 0.0);
        for (index, effect) in [(0, gain.clone()), (1, eq.clone())] {
            history
                .execute(
                    &mut project,
                    &Command::AddAudioEffect {
                        chain,
                        index,
                        effect,
                    },
                )
                .unwrap();
        }
        assert_eq!(chain_of(&project), vec![gain.clone(), eq.clone()]);

        history
            .execute(
                &mut project,
                &Command::MoveAudioEffect {
                    chain,
                    effect: EffectId(2),
                    index: 0,
                },
            )
            .unwrap();
        assert_eq!(chain_of(&project), vec![eq.clone(), gain.clone()]);

        history
            .execute(
                &mut project,
                &Command::SetAudioEffectParam {
                    chain,
                    effect: EffectId(1),
                    name: "gain_db".into(),
                    value: Some(-6.0),
                },
            )
            .unwrap();
        assert_eq!(chain_of(&project)[1].params["gain_db"], -6.0);

        history
            .execute(
                &mut project,
                &Command::SetAudioEffectBypass {
                    chain,
                    effect: EffectId(1),
                    bypass: true,
                },
            )
            .unwrap();
        assert!(chain_of(&project)[1].bypass);

        history
            .execute(
                &mut project,
                &Command::RemoveAudioEffect {
                    chain,
                    effect: EffectId(2),
                },
            )
            .unwrap();
        assert_eq!(chain_of(&project).len(), 1);

        // Undo walks back through every step, redo replays them.
        for _ in 0..6 {
            history.undo(&mut project).unwrap();
        }
        assert_eq!(project, original);
        for _ in 0..6 {
            history.redo(&mut project).unwrap();
        }
        assert_eq!(chain_of(&project).len(), 1);
        assert!(chain_of(&project)[0].bypass);
    }
}

#[test]
fn audio_effect_commands_reject_bad_edits_and_leave_the_project_unchanged() {
    let mut project = project_with_clip();
    let chain = AudioChain::Clip(ClipId(0));
    Command::AddAudioEffect {
        chain,
        index: 0,
        effect: audio_effect(1, "gain", 0.0),
    }
    .apply(&mut project)
    .unwrap();
    // The same id twice, an out-of-range index, an unknown effect and a missing target.
    for bad in [
        Command::AddAudioEffect {
            chain,
            index: 1,
            effect: audio_effect(1, "gain", 0.0),
        },
        Command::AddAudioEffect {
            chain,
            index: 5,
            effect: audio_effect(2, "gain", 0.0),
        },
        Command::MoveAudioEffect {
            chain,
            effect: EffectId(1),
            index: 1,
        },
        Command::RemoveAudioEffect {
            chain,
            effect: EffectId(9),
        },
        Command::SetAudioEffectBypass {
            chain: AudioChain::Track(TrackId(9)),
            effect: EffectId(1),
            bypass: true,
        },
        Command::AddAudioEffect {
            chain: AudioChain::Clip(ClipId(9)),
            index: 0,
            effect: audio_effect(2, "gain", 0.0),
        },
    ] {
        let before = project.clone();
        assert!(bad.apply(&mut project).is_err(), "{bad:?}");
        assert_eq!(project, before);
    }
    // Ids are unique across a clip's video and audio chains.
    let error = Command::AddEffect {
        id: ClipId(0),
        index: 0,
        effect: EffectInstance::basic(EffectId(1), "grayscale"),
    }
    .apply(&mut project)
    .unwrap_err();
    assert!(error.0.contains("duplicate"), "{error}");
    // Non-finite parameters are rejected by validation.
    let mut nan = audio_effect(3, "gain", 0.0);
    nan.params.insert("gain_db".into(), f64::NAN);
    assert!(
        Command::AddAudioEffect {
            chain,
            index: 0,
            effect: nan
        }
        .apply(&mut project)
        .is_err()
    );
}

#[test]
fn animated_audio_effects_resolve_and_cannot_be_removed() {
    let mut project = project_with_clip();
    let chain = AudioChain::Clip(ClipId(0));
    Command::AddAudioEffect {
        chain,
        index: 0,
        effect: audio_effect(1, "gain", 0.0),
    }
    .apply(&mut project)
    .unwrap();
    let target = Target::Effect {
        effect: EffectId(1),
        param: "gain_db".into(),
    };
    for (time, value) in [(0, 0.0), (100, 10.0)] {
        Command::AddKeyframe {
            id: ClipId(0),
            target: target.clone(),
            time: Time(time),
            keyframe: Keyframe {
                value: value.into(),
                interpolation: Interpolation::Linear,
            },
        }
        .apply(&mut project)
        .unwrap();
    }
    let resolved = clip(&project).audio_effects_at(Time(50));
    assert_eq!(resolved[0].params["gain_db"], 5.0);
    // Base parameters stay as stored, and the video evaluation is unaffected.
    assert_eq!(clip(&project).audio_effects[0].params["gain_db"], 0.0);
    assert_eq!(evaluate(&project, Time(50)).clips[0].effects.len(), 0);
    assert!(
        Command::RemoveAudioEffect {
            chain,
            effect: EffectId(1)
        }
        .apply(&mut project)
        .is_err()
    );
    assert!(
        Command::SetAudioEffectParam {
            chain,
            effect: EffectId(1),
            name: "gain_db".into(),
            value: None
        }
        .apply(&mut project)
        .is_err()
    );
    // Bypass hides the effect from the resolved chain without losing it.
    Command::SetAudioEffectBypass {
        chain,
        effect: EffectId(1),
        bypass: true,
    }
    .apply(&mut project)
    .unwrap();
    assert_eq!(clip(&project).audio_effects_at(Time(50)).len(), 0);
}

#[test]
fn locked_tracks_reject_clip_audio_effects_but_not_track_chains() {
    let mut project = project_with_clip();
    Command::SetTrackLocked {
        id: TrackId(0),
        locked: true,
    }
    .apply(&mut project)
    .unwrap();
    let clip_edit = Command::AddAudioEffect {
        chain: AudioChain::Clip(ClipId(0)),
        index: 0,
        effect: audio_effect(1, "gain", 0.0),
    };
    assert_eq!(clip_edit.locked_track(&project), Some(TrackId(0)));
    assert!(clip_edit.apply(&mut project).is_err());
    Command::AddAudioEffect {
        chain: AudioChain::Track(TrackId(0)),
        index: 0,
        effect: audio_effect(1, "gain", 0.0),
    }
    .apply(&mut project)
    .unwrap();
}

#[test]
fn audio_effects_round_trip_through_json_and_old_files_still_load() {
    let mut project = project_with_clip();
    Command::AddAudioEffect {
        chain: AudioChain::Clip(ClipId(0)),
        index: 0,
        effect: audio_effect(1, "gain", 2.0),
    }
    .apply(&mut project)
    .unwrap();
    Command::AddAudioEffect {
        chain: AudioChain::Track(TrackId(0)),
        index: 0,
        effect: audio_effect(1, "eq3", 0.0),
    }
    .apply(&mut project)
    .unwrap();
    Command::SetAudioEffectBypass {
        chain: AudioChain::Track(TrackId(0)),
        effect: EffectId(1),
        bypass: true,
    }
    .apply(&mut project)
    .unwrap();
    let json = serde_json::to_string(&project).unwrap();
    assert_eq!(serde_json::from_str::<Project>(&json).unwrap(), project);
    // Absent chains and bypass flags serialize to nothing, so v3 files are unchanged.
    let plain = serde_json::to_string(&project_with_clip()).unwrap();
    assert!(!plain.contains("audio_effects") && !plain.contains("bypass"));
    // Commands themselves are serializable.
    let command = Command::MoveAudioEffect {
        chain: AudioChain::Track(TrackId(0)),
        effect: EffectId(1),
        index: 0,
    };
    let text = serde_json::to_string(&command).unwrap();
    assert_eq!(serde_json::from_str::<Command>(&text).unwrap(), command);
}

#[test]
fn video_effect_bypass_is_reversible_and_dropped_from_the_evaluated_scene() {
    let mut project = project_with_clip();
    let mut history = History::default();
    history
        .execute(
            &mut project,
            &Command::AddEffect {
                id: ClipId(0),
                index: 0,
                effect: EffectInstance::basic(EffectId(1), "grayscale"),
            },
        )
        .unwrap();
    let evaluated = |project: &Project| evaluate(project, Time(10)).clips[0].effects.len();
    assert_eq!(evaluated(&project), 1);

    let bypass = |bypass| Command::SetEffectBypass {
        id: ClipId(0),
        effect: EffectId(1),
        bypass,
    };
    history.execute(&mut project, &bypass(true)).unwrap();
    assert!(clip(&project).effects[0].bypass);
    // The effect stays in the chain with its parameters, but is not evaluated.
    assert_eq!(clip(&project).effects[0].params["amount"], 1.0);
    assert_eq!(evaluated(&project), 0);

    history.undo(&mut project).unwrap();
    assert!(!clip(&project).effects[0].bypass);
    assert_eq!(evaluated(&project), 1);
    history.redo(&mut project).unwrap();
    assert!(clip(&project).effects[0].bypass);

    // Serializable, and a missing effect is rejected without changing anything.
    let json = serde_json::to_string(&bypass(true)).unwrap();
    assert_eq!(
        serde_json::from_str::<Command>(&json).unwrap(),
        bypass(true)
    );
    let before = project.clone();
    let missing = Command::SetEffectBypass {
        id: ClipId(0),
        effect: EffectId(9),
        bypass: true,
    };
    assert!(history.execute(&mut project, &missing).is_err());
    assert_eq!(project, before);
}
