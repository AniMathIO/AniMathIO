//! Exact source clocks and transactional retiming edits.
use animathio_core::*;

fn range(a: i64, b: i64) -> TimeRange {
    TimeRange::new(Time(a), Time(b)).unwrap()
}
fn project(linked: bool, trailing: bool) -> Project {
    let mut p = Project::default();
    for n in 1..=2 {
        Command::AddTrack {
            track: Track::new(TrackId(n), "layer"),
            index: usize::try_from(n - 1).unwrap(),
        }
        .apply(&mut p)
        .unwrap();
        let mut c = Clip::new(ClipId(n), ClipKind::Error("fixture".into()), range(0, 1000));
        c.source_offset = Time(100);
        c.link_group = linked.then_some(ClipId(1));
        Command::AddClip {
            track: TrackId(n),
            clip: c,
        }
        .apply(&mut p)
        .unwrap();
    }
    if trailing {
        Command::AddClip {
            track: TrackId(1),
            clip: Clip::new(ClipId(3), ClipKind::Error("tail".into()), range(1000, 1200)),
        }
        .apply(&mut p)
        .unwrap();
    }
    p
}
fn clip(p: &Project, id: u64) -> &Clip {
    p.tracks()
        .iter()
        .find_map(|t| t.clips().get(&ClipId(id)))
        .unwrap()
}
fn speed(n: u32, d: u32, reverse: bool, ripple: bool) -> Command {
    Command::SetClipSpeed {
        id: ClipId(1),
        speed: ClipSpeed::new(n, d).unwrap(),
        reverse,
        ramps: vec![],
        ripple,
    }
}
fn exact(p: &mut Project, command: &Command) {
    let before = serde_json::to_vec(p).unwrap();
    let inverse = command.apply(p).unwrap();
    let after = p.clone();
    let redo = inverse.apply(p).unwrap();
    assert_eq!(serde_json::to_vec(p).unwrap(), before);
    redo.apply(p).unwrap();
    assert_eq!(*p, after);
}
#[test]
fn rational_validation_and_linear_integral() {
    assert_eq!(ClipSpeed::new(2, 4).unwrap(), ClipSpeed::new(1, 2).unwrap());
    for (n, d) in [(0, 1), (17, 1), (1, 0), (1, 17)] {
        assert!(ClipSpeed::new(n, d).is_err());
    }
    let m = ClipTimeMap {
        ramps: vec![
            SpeedPoint {
                time: Time(0),
                speed: ClipSpeed::new(1, 1).unwrap(),
            },
            SpeedPoint {
                time: Time(1000),
                speed: ClipSpeed::new(3, 1).unwrap(),
            },
        ],
        ..Default::default()
    };
    // Integral of 1 + t/500 is t + t*t/1000.
    for t in 0..=1500 {
        let expected = if t <= 1000 {
            t + t * t / 1000
        } else {
            2000 + (t - 1000) * 3
        };
        assert_eq!(m.integrated(Time(t)), Time(expected));
    }
    assert_eq!(m.duration_for(Time(2000)).unwrap(), Time(1000));
    let mut invalid = m.clone();
    invalid.ramps.reverse();
    assert!(invalid.validate(100).is_err());
    invalid.ramps[0].time = Time(i64::MAX);
    assert!(invalid.validate(100).is_err());
}
#[test]
fn source_span_reverse_boundaries_and_exact_undo() {
    let mut p = project(true, false);
    for reverse in [false, true] {
        exact(&mut p, &speed(2, 1, reverse, false));
        for id in [1, 2] {
            let c = clip(&p, id);
            assert_eq!(c.range, range(0, 500));
            assert_eq!(c.source_bounds(), (Time(100), Time(1100)));
            assert_eq!(
                c.source_time(Time(0)),
                Time(if reverse { 1099 } else { 100 })
            );
            assert_eq!(
                c.source_time(Time(499)),
                Time(if reverse { 101 } else { 1098 })
            );
        }
    }
    exact(&mut p, &speed(1, 2, false, false));
    assert_eq!(clip(&p, 1).range.end, Time(2000));
}
#[test]
fn ripple_companions_overlap_and_locked_tracks() {
    let mut p = project(true, true);
    let before = p.clone();
    assert!(speed(1, 2, false, false).apply(&mut p).is_err());
    assert_eq!(p, before);
    exact(&mut p, &speed(1, 2, false, true));
    assert_eq!(clip(&p, 3).range, range(2000, 2200));
    Command::SetTrackLocked {
        id: TrackId(2),
        locked: true,
    }
    .apply(&mut p)
    .unwrap();
    let before = p.clone();
    assert!(speed(2, 1, false, true).apply(&mut p).is_err());
    assert_eq!(p, before);
}
#[test]
fn split_trim_merge_keep_ramp_phase_and_source_samples() {
    let mut p = project(false, false);
    let ramps = vec![
        SpeedPoint {
            time: Time(0),
            speed: ClipSpeed::new(1, 1).unwrap(),
        },
        SpeedPoint {
            time: Time(500),
            speed: ClipSpeed::new(3, 1).unwrap(),
        },
    ];
    Command::SetClipSpeed {
        id: ClipId(1),
        speed: ClipSpeed::default(),
        reverse: true,
        ramps,
        ripple: false,
    }
    .apply(&mut p)
    .unwrap();
    let original = clip(&p, 1).clone();
    exact(
        &mut p,
        &Command::SplitClip {
            id: ClipId(1),
            right_id: ClipId(4),
            at: Time(200),
        },
    );
    for t in 200..500 {
        assert_eq!(
            clip(&p, 4).source_time(Time(t - 200)),
            original.source_time(Time(t))
        );
    }
    exact(
        &mut p,
        &Command::MergeClip {
            left: ClipId(1),
            right: ClipId(4),
        },
    );
    assert_eq!(clip(&p, 1), &original);
    exact(
        &mut p,
        &Command::TrimClip {
            id: ClipId(1),
            range: range(200, 500),
            source_offset: Time(300),
        },
    );
    for t in 0..300 {
        assert_eq!(
            clip(&p, 1).source_time(Time(t)),
            original.source_time(Time(t + 200))
        );
    }
}
#[test]
fn frame_hold_is_a_single_batch_with_exact_undo_and_continuous_tail() {
    let mut p = project(true, true);
    speed(2, 1, true, true).apply(&mut p).unwrap();
    let original = clip(&p, 1).clone();
    let command = freeze_frame_command(&p, ClipId(1), Time(200), 1).unwrap();
    assert!(matches!(command, Command::Batch { .. }));
    exact(&mut p, &command);
    let length = p
        .frame_rate()
        .frame_to_time(1, Rounding::Nearest)
        .unwrap()
        .0;
    let pieces = p.tracks()[0].clips();
    let hold = pieces.values().find(|c| c.time_map.hold).unwrap();
    assert_eq!(hold.range, range(200, 200 + length));
    assert_eq!(
        hold.source_time(Time(length - 1)),
        original.source_time(Time(200))
    );
    let tail = pieces
        .values()
        .find(|c| c.range.start == Time(200 + length))
        .unwrap();
    assert_eq!(tail.source_time(Time(30)), original.source_time(Time(230)));
    assert_eq!(clip(&p, 3).range.start, Time(500 + length));
}

#[test]
fn extreme_valid_curves_and_invalid_serialized_clocks_are_bounded() {
    let map = ClipTimeMap {
        ramps: vec![
            SpeedPoint {
                time: Time(0),
                speed: ClipSpeed::new(16, 1).unwrap(),
            },
            SpeedPoint {
                time: Time(MAX_RETIME_TICKS),
                speed: ClipSpeed::new(1, 16).unwrap(),
            },
        ],
        ..ClipTimeMap::default()
    };
    map.validate(MAX_RETIME_TICKS).unwrap();
    assert!(map.integrated(Time(MAX_RETIME_TICKS / 2)).0 > 0);
    assert!(map.integrated(Time(MAX_RETIME_TICKS)).0 < 16 * MAX_RETIME_TICKS);
    let p = project(false, false);
    let mut value = serde_json::to_value(&p).unwrap();
    let clock = &mut value["tracks"][0]["clips"]["1"]["time_map"];
    clock["reverse"] = true.into();
    assert!(serde_json::from_value::<Project>(value.clone()).is_err());
    value["tracks"][0]["clips"]["1"]["time_map"]["source_length"] = 1000.into();
    value["tracks"][0]["clips"]["1"]["time_map"]["phase"] = i64::MAX.into();
    assert!(serde_json::from_value::<Project>(value).is_err());
}

#[test]
fn hold_at_start_and_retime_rollback_leave_no_orphaned_links() {
    let mut p = project(true, true);
    exact(
        &mut p,
        &Command::InsertFrameHold {
            id: ClipId(1),
            at: Time(0),
            frames: 1,
        },
    );
    let hold = p.tracks()[0]
        .clips()
        .values()
        .find(|c| c.time_map.hold)
        .unwrap();
    let other = p.tracks()[1]
        .clips()
        .values()
        .find(|c| c.time_map.hold)
        .unwrap();
    assert_eq!(hold.link_group, other.link_group);
    let before = p.clone();
    let command = Command::SetClipSpeed {
        id: hold.id,
        speed: ClipSpeed::new(2, 1).unwrap(),
        reverse: false,
        ramps: vec![],
        ripple: false,
    };
    assert!(command.apply(&mut p).is_err());
    assert_eq!(p, before);
}

#[test]
fn fractional_decelerating_ramps_round_only_after_integrating() {
    let clock = ClipTimeMap {
        ramps: vec![
            SpeedPoint {
                time: Time(5),
                speed: ClipSpeed::new(1, 2).unwrap(),
            },
            SpeedPoint {
                time: Time(12),
                speed: ClipSpeed::new(3, 2).unwrap(),
            },
            SpeedPoint {
                time: Time(23),
                speed: ClipSpeed::new(1, 4).unwrap(),
            },
        ],
        ..Default::default()
    };
    clock.validate(100).unwrap();
    for t in 0..100 {
        // Independent analytic integrals, retaining fractional area across joins.
        let expected = if t < 5 {
            t / 2
        } else if t < 12 {
            let u = t - 5;
            (35 + 7 * u + u * u) / 14
        } else if t < 23 {
            let u = t - 12;
            (836 + 132 * u - 5 * u * u) / 88
        } else {
            (153 + 2 * (t - 23)) / 8
        };
        assert_eq!(clock.integrated(Time(t)), Time(expected), "tick {t}");
    }
    for source in 1..40 {
        let duration = clock.duration_for(Time(source)).unwrap();
        assert!(clock.integrated(duration) >= Time(source));
        assert!(clock.integrated(Time(duration.0 - 1)) < Time(source));
    }
}

#[test]
fn retiming_keeps_automation_at_its_original_timeline_ticks() {
    let mut p = project(false, false);
    Command::AddKeyframe {
        id: ClipId(1),
        target: Target::Transform(TransformProperty::Opacity),
        time: Time(800),
        keyframe: Keyframe {
            value: 0.5.into(),
            interpolation: Interpolation::Linear,
        },
    }
    .apply(&mut p)
    .unwrap();
    let original = clip(&p, 1).animations.clone();
    exact(&mut p, &speed(2, 1, true, false));
    assert_eq!(clip(&p, 1).animations, original);
    assert!(clip(&p, 1).range.end < Time(800));
}
