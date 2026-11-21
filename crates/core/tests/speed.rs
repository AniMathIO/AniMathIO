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
fn units(percent: u32) -> ClipSpeed {
    ClipSpeed::new(percent, 100).unwrap()
}

#[test]
fn speed_at_curve_interpolates_exactly_in_millionths() {
    let map = ClipTimeMap {
        ramps: vec![
            SpeedPoint {
                time: Time(100),
                speed: ClipSpeed::from_millionths(500_000).unwrap(),
            },
            SpeedPoint {
                time: Time(400),
                speed: ClipSpeed::from_millionths(2_000_000).unwrap(),
            },
        ],
        ..Default::default()
    };
    assert_eq!(
        map.speed_at_curve(Time(0)),
        units(50),
        "held before the keys"
    );
    assert_eq!(map.speed_at_curve(Time(100)), units(50));
    // 0.5 + 1.5 * (t - 100) / 300, to the nearest millionth.
    assert_eq!(map.speed_at_curve(Time(200)).millionths(), 1_000_000);
    assert_eq!(map.speed_at_curve(Time(250)).millionths(), 1_250_000);
    assert_eq!(map.speed_at_curve(Time(101)).millionths(), 505_000);
    assert_eq!(map.speed_at_curve(Time(400)), units(200));
    assert_eq!(
        map.speed_at_curve(Time(900)),
        units(200),
        "held after the keys"
    );
    // Odd thirds round to the nearest unit rather than to a whole percent.
    let third = ClipTimeMap {
        ramps: vec![
            SpeedPoint {
                time: Time(0),
                speed: units(100),
            },
            SpeedPoint {
                time: Time(3),
                speed: ClipSpeed::from_millionths(1_000_001).unwrap(),
            },
        ],
        ..Default::default()
    };
    assert_eq!(third.speed_at_curve(Time(2)).millionths(), 1_000_001);
    // Without keys the constant speed holds.
    assert_eq!(ClipTimeMap::default().speed_at_curve(Time(5)), units(100));
}

#[test]
fn local_ramps_rebase_a_phased_curve_onto_the_clip() {
    let key = |time: i64, percent: u32| SpeedPoint {
        time: Time(time),
        speed: units(percent),
    };
    let map = ClipTimeMap {
        ramps: vec![key(0, 100), key(400, 300), key(800, 100)],
        phase: Time(100),
        ..Default::default()
    };
    // A 500-tick clip sees curve ticks 100..600: the key at 0 is cut away (the curve
    // enters at the interpolated 150%), the key at 400 becomes local 300, and the curve
    // leaves at the interpolated 200% where the key at 800 is cut away.
    let local = map.local_ramps(500);
    assert_eq!(
        local,
        [key(0, 150), key(300, 300), key(500, 200)],
        "{local:?}"
    );
    for t in 0..=500 {
        let rebased = ClipTimeMap {
            ramps: local.clone(),
            ..Default::default()
        };
        let (a, b) = (
            map.speed_at_curve(Time(t + 100)).millionths(),
            rebased.speed_at_curve(Time(t)).millionths(),
        );
        assert!(a.abs_diff(b) <= 1, "tick {t}: {a} vs {b}");
    }
    // Keys that are all inside need no extra vertices; an empty map stays empty.
    let inside = ClipTimeMap {
        ramps: vec![key(120, 100), key(500, 200)],
        phase: Time(100),
        ..Default::default()
    };
    assert_eq!(inside.local_ramps(500), [key(20, 100), key(400, 200)]);
    assert_eq!(ClipTimeMap::default().local_ramps(500), []);
    // A key exactly on the cut is kept as is.
    let cut = ClipTimeMap {
        ramps: vec![key(100, 150), key(700, 50)],
        phase: Time(100),
        ..Default::default()
    };
    assert_eq!(cut.local_ramps(300), [key(0, 150), key(300, 100)]);
}

#[test]
fn rebased_ramps_reproduce_a_head_trimmed_clip_through_set_clip_speed() {
    let mut p = project(false, false);
    let key = |time: i64, percent: u32| SpeedPoint {
        time: Time(time),
        speed: units(percent),
    };
    Command::SetClipSpeed {
        id: ClipId(1),
        speed: ClipSpeed::default(),
        reverse: false,
        ramps: vec![key(0, 100), key(500, 300), key(900, 100)],
        ripple: false,
    }
    .apply(&mut p)
    .unwrap();
    let full = clip(&p, 1).clone();
    // Head-trim, then re-apply the clip's own curve as the editor does after a handle
    // gesture: nothing may change, though the clock's phase is rebuilt from zero.
    Command::TrimClip {
        id: ClipId(1),
        range: range(250, full.range.end.0),
        source_offset: full.source_time(Time(250)),
    }
    .apply(&mut p)
    .unwrap();
    let trimmed = clip(&p, 1).clone();
    assert_eq!(trimmed.time_map.phase, Time(250));
    let length = trimmed.range.end.0 - trimmed.range.start.0;
    Command::SetClipSpeed {
        id: ClipId(1),
        speed: trimmed.time_map.speed,
        reverse: false,
        ramps: trimmed.time_map.local_ramps(length),
        ripple: false,
    }
    .apply(&mut p)
    .unwrap();
    let again = clip(&p, 1);
    assert_eq!(again.time_map.phase, Time(0));
    assert!(
        (again.range.end.0 - trimmed.range.end.0).abs() <= 1,
        "duration {} vs {}",
        again.range.end.0,
        trimmed.range.end.0
    );
    for t in (0..length).step_by(7) {
        let (a, b) = (trimmed.source_time(Time(t)).0, again.source_time(Time(t)).0);
        assert!((a - b).abs() <= 1, "tick {t}: {a} vs {b}");
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
fn frame_hold_at_the_tick_limit_reports_a_clean_overflow() {
    // A clip that runs to the very end of the tick domain validates, but no hold can fit
    // one frame before its end: the batch must report the overflow instead of panicking
    // on, or wrapping around, `at.0 + duration`.
    let mut p = Project::default();
    Command::AddTrack {
        track: Track::new(TrackId(1), "layer"),
        index: 0,
    }
    .apply(&mut p)
    .unwrap();
    let end = i64::MAX;
    Command::AddClip {
        track: TrackId(1),
        clip: Clip::new(ClipId(1), ClipKind::Error("fixture".into()), range(0, end)),
    }
    .apply(&mut p)
    .unwrap();
    let error = freeze_frame_command(&p, ClipId(1), Time(end - 1), 1).unwrap_err();
    assert!(error.0.contains("hold overflow"), "{error:?}");
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

#[test]
fn maintain_pitch_is_serializable_linked_invertible_and_lock_safe() {
    let mut p = project(true, false);
    assert!(clip(&p, 1).maintain_pitch);
    let command = Command::SetClipMaintainPitch {
        id: ClipId(1),
        maintain_pitch: false,
    };
    let command: Command = serde_json::from_slice(&serde_json::to_vec(&command).unwrap()).unwrap();
    exact(&mut p, &command);
    assert!(!clip(&p, 1).maintain_pitch);
    assert!(!clip(&p, 2).maintain_pitch);
    speed(2, 1, false, false).apply(&mut p).unwrap();
    assert!(!clip(&p, 1).maintain_pitch);
    Command::SetTrackLocked {
        id: TrackId(2),
        locked: true,
    }
    .apply(&mut p)
    .unwrap();
    let before = p.clone();
    assert!(
        Command::SetClipMaintainPitch {
            id: ClipId(1),
            maintain_pitch: true
        }
        .apply(&mut p)
        .is_err()
    );
    assert_eq!(p, before);
}

fn video_project(n: u32, d: u32, reverse: bool) -> Project {
    // A 30 fps project (the default) with one video second.
    let mut p = Project::default();
    for (name, media_type) in [(&b"video"[..], "video/mp4"), (&b"still"[..], "image/png")] {
        Command::AddAsset {
            id: AssetId::of(name),
            asset: Asset {
                name: "media".into(),
                media_type: media_type.into(),
            },
        }
        .apply(&mut p)
        .unwrap();
    }
    Command::AddTrack {
        track: Track::new(TrackId(1), "video"),
        index: 0,
    }
    .apply(&mut p)
    .unwrap();
    Command::AddClip {
        track: TrackId(1),
        clip: Clip::new(
            ClipId(1),
            ClipKind::Video(AssetId::of(b"video")),
            range(0, TICKS_PER_SECOND),
        ),
    }
    .apply(&mut p)
    .unwrap();
    speed(n, d, reverse, false).apply(&mut p).unwrap();
    p
}

#[test]
fn fast_video_frames_carry_box_filter_taps_that_cover_the_source_span() {
    let frame = TICKS_PER_SECOND / 30;
    // 3x forward: three taps centred on the output frame's own source time, one per source
    // frame of the span (the average sits where the single point sample lies).
    let p = video_project(3, 1, false);
    let at = |frames: i64| evaluate(&p, Time(frames * frame)).clips[0].clone();
    let clip = at(2);
    assert_eq!(clip.source_time, Time(6 * frame));
    assert_eq!(clip.source_taps, [5, 6, 7].map(|n| Time(n * frame)));
    // Reversed taps walk backwards through the same span.
    let p = video_project(2, 1, true);
    let clip = evaluate(&p, Time(frame)).clips[0].clone();
    assert_eq!(clip.source_taps.len(), 2);
    assert!(clip.source_taps[0] > clip.source_taps[1]);
    // The 16x limit averages at most 16 frames.
    let p = video_project(16, 1, false);
    assert_eq!(evaluate(&p, Time(0)).clips[0].source_taps.len(), 16);
    // Every tap is a valid source time, even in the clip's last output frame.
    let last = Time(p.tracks()[0].clips()[&ClipId(1)].range.end.0 - 1);
    assert!(
        evaluate(&p, last).clips[0]
            .source_taps
            .iter()
            .all(|t| t.0 >= 0 && t.0 < TICKS_PER_SECOND)
    );
}

#[test]
fn point_samples_remain_for_normal_slow_still_and_held_video() {
    let frame = TICKS_PER_SECOND / 30;
    for (n, d) in [(1, 1), (1, 2), (5, 4)] {
        let p = video_project(n, d, false);
        let scene = evaluate(&p, Time(3 * frame));
        assert!(scene.clips[0].source_taps.is_empty(), "{n}/{d}");
    }
    // An image or audio clip never filters, even when retimed.
    let mut p = video_project(4, 1, false);
    Command::AddClip {
        track: TrackId(1),
        clip: Clip::new(
            ClipId(2),
            ClipKind::Image(AssetId::of(b"still")),
            range(TICKS_PER_SECOND, 2 * TICKS_PER_SECOND),
        ),
    }
    .apply(&mut p)
    .unwrap();
    Command::SetClipSpeed {
        id: ClipId(2),
        speed: ClipSpeed::new(4, 1).unwrap(),
        reverse: false,
        ramps: vec![],
        ripple: false,
    }
    .apply(&mut p)
    .unwrap();
    assert_eq!(
        evaluate(&p, Time(TICKS_PER_SECOND + frame / 2)).clips[0]
            .source_taps
            .len(),
        0
    );
    // A frame hold shows one frame; the segments around it still filter.
    let mut p = video_project(2, 1, false);
    Command::InsertFrameHold {
        id: ClipId(1),
        at: Time(3 * frame),
        frames: 4,
    }
    .apply(&mut p)
    .unwrap();
    assert_eq!(evaluate(&p, Time(frame)).clips[0].source_taps.len(), 2);
    assert_eq!(evaluate(&p, Time(5 * frame)).clips[0].source_taps.len(), 0);
    assert_eq!(evaluate(&p, Time(9 * frame)).clips[0].source_taps.len(), 2);
}

#[test]
fn ramps_filter_only_where_the_speed_exceeds_the_frame_rate() {
    let frame = TICKS_PER_SECOND / 30;
    let mut p = video_project(1, 1, false);
    Command::SetClipSpeed {
        id: ClipId(1),
        speed: ClipSpeed::default(),
        reverse: false,
        ramps: vec![
            SpeedPoint {
                time: Time(0),
                speed: ClipSpeed::new(1, 1).unwrap(),
            },
            SpeedPoint {
                time: Time(TICKS_PER_SECOND / 2),
                speed: ClipSpeed::new(4, 1).unwrap(),
            },
        ],
        ripple: false,
    }
    .apply(&mut p)
    .unwrap();
    let end = p.tracks()[0].clips()[&ClipId(1)].range.end.0;
    assert!(
        evaluate(&p, Time(0)).clips[0].source_taps.is_empty(),
        "starts at 1x"
    );
    let late = evaluate(&p, Time(end - 2 * frame));
    assert!(late.clips[0].source_taps.len() >= 3);
}
