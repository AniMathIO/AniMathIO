//! Typed interpolation, validation, and command history contracts.
use animathio_core::*;
use proptest::prelude::*;

fn pairs() -> impl Strategy<Value = (ParamValue, ParamValue)> {
    prop_oneof![
        (-1.0e12_f64..1.0e12, -1.0e12_f64..1.0e12).prop_map(|(a, b)| (a.into(), b.into())),
        (any::<[u8; 4]>(), any::<[u8; 4]>())
            .prop_map(|(a, b)| (ParamValue::from_srgba(a), ParamValue::from_srgba(b))),
        (any::<[i32; 2]>(), any::<[i32; 2]>()).prop_map(|(a, b)| (
            ParamValue::Point(a.map(f64::from)),
            ParamValue::Point(b.map(f64::from))
        )),
        (any::<bool>(), any::<bool>())
            .prop_map(|(a, b)| (ParamValue::Bool(a), ParamValue::Bool(b))),
        ("[a-z]{0,16}", "[a-z]{0,16}")
            .prop_map(|(a, b)| (ParamValue::Enum(a), ParamValue::Enum(b))),
        (".{0,16}", ".{0,16}").prop_map(|(a, b)| (ParamValue::String(a), ParamValue::String(b))),
        (any::<Option<u64>>(), any::<Option<u64>>()).prop_map(|(a, b)| (
            ParamValue::AssetRef(a.map(|v| AssetId::of(&v.to_le_bytes()))),
            ParamValue::AssetRef(b.map(|v| AssetId::of(&v.to_le_bytes())))
        )),
    ]
}
fn project(value: ParamValue) -> Project {
    let mut project = Project::default();
    Command::AddTrack {
        track: Track::new(TrackId(0), "typed"),
        index: 0,
    }
    .apply(&mut project)
    .unwrap();
    let mut clip = Clip::new(
        ClipId(0),
        ClipKind::Plugin {
            plugin_id: "test".into(),
            params: serde_json::json!({"original": {"preserved": true}}),
        },
        TimeRange::new(Time(0), Time(1000)).unwrap(),
    );
    clip.params.insert("p".into(), value);
    Command::AddClip {
        track: TrackId(0),
        clip,
    }
    .apply(&mut project)
    .unwrap();
    project
}
fn add(value: ParamValue, time: i64, interpolation: Interpolation) -> Command {
    Command::AddKeyframe {
        id: ClipId(0),
        target: Target::Param("p".into()),
        time: Time(time),
        keyframe: Keyframe {
            value,
            interpolation,
        },
    }
}
fn sample(project: &Project, time: i64) -> ParamValue {
    evaluate(project, Time(time)).clips[0].params["p"].clone()
}
proptest! {
    #[test]
    fn exact_keys_and_clamped_endpoints((left, right) in pairs(), mode in 0_u8..3) {
        let mode = [Interpolation::Linear, Interpolation::EaseInOut, Interpolation::Hold][usize::from(mode)];
        let mut p = project(left.clone());
        add(left.clone(), 10, mode).apply(&mut p).unwrap();
        add(right.clone(), 900, mode).apply(&mut p).unwrap();
        prop_assert_eq!(sample(&p, 0), left.clone());
        prop_assert_eq!(sample(&p, 10), left);
        prop_assert_eq!(sample(&p, 900), right.clone());
        prop_assert_eq!(sample(&p, 999), right);
    }
    #[test]
    fn explicit_hold_and_discrete_values_never_blend((left, right) in pairs(), t in 11_i64..900, mode in 0_u8..3) {
        let mode = if matches!(left, ParamValue::Number(_) | ParamValue::Color(_) | ParamValue::Point(_)) { Interpolation::Hold } else { [Interpolation::Linear, Interpolation::EaseInOut, Interpolation::Hold][usize::from(mode)] };
        let mut p = project(left.clone());
        add(left.clone(), 10, mode).apply(&mut p).unwrap();
        add(right.clone(), 900, mode).apply(&mut p).unwrap();
        prop_assert_eq!(sample(&p, t), left);
        prop_assert_eq!(sample(&p, 900), right);
    }
    #[test]
    fn typed_commands_undo_redo_and_serde_are_exact((left, right) in pairs()) {
        let mut p = project(left.clone());
        let commands = [
            Command::SetPluginParam { id: ClipId(0), name: "p".into(), value: Some(right.clone()) },
            add(left.clone(), 10, Interpolation::EaseInOut),
            add(right.clone(), 900, Interpolation::Linear),
            add(right, 10, Interpolation::Hold),
            Command::MoveKeyframe { id: ClipId(0), target: Target::Param("p".into()), from: Time(900), to: Time(800) },
            Command::RemoveKeyframe { id: ClipId(0), target: Target::Param("p".into()), time: Time(10) },
            Command::RemoveKeyframe { id: ClipId(0), target: Target::Param("p".into()), time: Time(800) },
            Command::SetPluginParam { id: ClipId(0), name: "p".into(), value: None },
        ];
        let original = p.clone();
        let mut history = History::default();
        for command in &commands {
            let encoded = serde_json::to_string(command).unwrap();
            prop_assert_eq!(&serde_json::from_str::<Command>(&encoded).unwrap(), command);
            history.execute(&mut p, command).unwrap();
        }
        let edited = p.clone();
        for _ in &commands { history.undo(&mut p).unwrap(); }
        prop_assert_eq!(&p, &original);
        for _ in &commands { history.redo(&mut p).unwrap(); }
        prop_assert_eq!(&p, &edited);
        prop_assert_eq!(serde_json::from_str::<Project>(&serde_json::to_string(&p).unwrap()).unwrap(), p);
    }
}

#[test]
fn mixed_types_and_non_numeric_transform_keys_are_rejected_transactionally() {
    let mut p = project(ParamValue::Bool(false));
    add(ParamValue::Bool(true), 10, Interpolation::Linear)
        .apply(&mut p)
        .unwrap();
    let before = p.clone();
    for command in [
        add(
            ParamValue::String("wrong".into()),
            20,
            Interpolation::Linear,
        ),
        Command::SetPluginParam {
            id: ClipId(0),
            name: "p".into(),
            value: Some(1.0.into()),
        },
        Command::AddKeyframe {
            id: ClipId(0),
            target: Target::Transform(TransformProperty::X),
            time: Time(0),
            keyframe: Keyframe {
                value: ParamValue::Bool(true),
                interpolation: Interpolation::Hold,
            },
        },
    ] {
        assert!(command.apply(&mut p).is_err());
        assert_eq!(p, before);
    }
    let mut json = serde_json::to_value(&p).unwrap();
    json["tracks"][0]["clips"]["0"]["animations"][0]["keys"]["20"] =
        serde_json::json!({"value":{"String":"wrong"},"interpolation":"Linear"});
    assert!(serde_json::from_value::<Project>(json).is_err());
}

#[test]
fn continuous_types_use_linear_light_and_easing() {
    for mode in [Interpolation::Linear, Interpolation::EaseInOut] {
        for (left, right, expected) in [
            (
                ParamValue::Number(0.0),
                ParamValue::Number(8.0),
                ParamValue::Number(if mode == Interpolation::Linear {
                    2.0
                } else {
                    1.25
                }),
            ),
            (
                ParamValue::Point([0.0, 8.0]),
                ParamValue::Point([8.0, 0.0]),
                ParamValue::Point(if mode == Interpolation::Linear {
                    [2.0, 6.0]
                } else {
                    [1.25, 6.75]
                }),
            ),
            (
                ParamValue::Color([0.0, 0.0, 0.0, 0.0]),
                ParamValue::Color([1.0, 1.0, 1.0, 1.0]),
                ParamValue::Color(
                    [if mode == Interpolation::Linear {
                        0.25
                    } else {
                        0.15625
                    }; 4],
                ),
            ),
        ] {
            let mut p = project(left.clone());
            add(left, 0, mode).apply(&mut p).unwrap();
            add(right, 100, mode).apply(&mut p).unwrap();
            assert_eq!(sample(&p, 25), expected);
        }
    }
    // Half linear white is ~188 sRGB, not byte-space 128.
    assert_eq!(
        ParamValue::Color([0.5, 0.5, 0.5, 1.0]).to_plugin_json(),
        serde_json::json!([188, 188, 188, 255])
    );
}
