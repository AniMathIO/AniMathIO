//! High-level timeline edits: batches, multi-split, range removal and ripple delete.
use animathio_core::*;
use serde_json::json;

fn range(start: i64, end: i64) -> TimeRange {
    TimeRange::new(Time(start), Time(end)).unwrap()
}
fn plugin(id: u64, start: i64, end: i64, offset: i64) -> Clip {
    let mut clip = Clip::new(
        ClipId(id),
        ClipKind::Plugin {
            plugin_id: "org.animathio.test@1".into(),
            params: json!({}),
        },
        range(start, end),
    );
    clip.source_offset = Time(offset);
    clip
}
/// Track 0: [0,100) and [150,200). Track 1: [50,250).
fn project() -> Project {
    let mut project = Project::default();
    let commands = vec![
        Command::AddTrack {
            track: Track::new(TrackId(0), "video"),
            index: 0,
        },
        Command::AddTrack {
            track: Track::new(TrackId(1), "audio"),
            index: 1,
        },
        Command::AddClip {
            track: TrackId(0),
            clip: plugin(1, 0, 100, 0),
        },
        Command::AddClip {
            track: TrackId(0),
            clip: plugin(2, 150, 200, 0),
        },
        Command::AddClip {
            track: TrackId(1),
            clip: plugin(3, 50, 250, 10),
        },
    ];
    Command::Batch { commands }.apply(&mut project).unwrap();
    project
}
fn spans(project: &Project, track: usize) -> Vec<(u64, i64, i64, i64)> {
    let mut spans: Vec<_> = project.tracks()[track]
        .clips()
        .values()
        .map(|c| (c.id.0, c.range.start.0, c.range.end.0, c.source_offset.0))
        .collect();
    spans.sort_by_key(|s| s.1);
    spans
}
/// Apply, then check undo restores the original and redo restores the edit.
fn round_trip(project: &mut Project, command: &Command) {
    let before = project.clone();
    let mut history = History::default();
    history.execute(project, command).unwrap();
    let after = project.clone();
    assert!(history.undo(project).unwrap());
    assert_eq!(*project, before);
    assert!(history.redo(project).unwrap());
    assert_eq!(*project, after);
}

#[test]
fn batch_is_one_undo_step_and_atomic() {
    let mut project = Project::default();
    let mut history = History::default();
    let batch = Command::Batch {
        commands: vec![
            Command::AddTrack {
                track: Track::new(TrackId(0), "a"),
                index: 0,
            },
            Command::AddClip {
                track: TrackId(0),
                clip: plugin(1, 0, 10, 0),
            },
        ],
    };
    history.execute(&mut project, &batch).unwrap();
    assert_eq!(project.tracks()[0].clips().len(), 1);
    assert!(history.undo(&mut project).unwrap());
    assert_eq!(project, Project::default());
    assert!(!history.undo(&mut project).unwrap());

    // A failing member rejects the whole batch.
    let failing = Command::Batch {
        commands: vec![
            Command::AddTrack {
                track: Track::new(TrackId(0), "a"),
                index: 0,
            },
            Command::RemoveClip { id: ClipId(99) },
        ],
    };
    assert!(failing.apply(&mut project).is_err());
    assert_eq!(project, Project::default());

    // Overlap is validated once, at the end.
    let overlapping = Command::Batch {
        commands: vec![
            Command::AddTrack {
                track: Track::new(TrackId(0), "a"),
                index: 0,
            },
            Command::AddClip {
                track: TrackId(0),
                clip: plugin(1, 0, 10, 0),
            },
            Command::AddClip {
                track: TrackId(0),
                clip: plugin(2, 5, 15, 0),
            },
        ],
    };
    assert!(overlapping.apply(&mut project).is_err());
    assert_eq!(project, Project::default());
}

#[test]
fn split_at_splits_every_crossing_clip() {
    let mut project = project();
    let command = Command::SplitAt {
        times: vec![Time(175), Time(60), Time(60), Time(0), Time(100)],
    };
    round_trip(&mut project, &command);
    assert_eq!(
        spans(&project, 0),
        vec![
            (1, 0, 60, 0),
            (4, 60, 100, 60),
            (2, 150, 175, 0),
            (5, 175, 200, 25)
        ]
    );
    assert_eq!(
        spans(&project, 1),
        vec![
            (3, 50, 60, 10),
            (8, 60, 100, 20),
            (7, 100, 175, 60),
            (6, 175, 250, 135)
        ]
    );
    let json = serde_json::to_string(&command).unwrap();
    assert_eq!(serde_json::from_str::<Command>(&json).unwrap(), command);
}

#[test]
fn remove_range_without_ripple_leaves_a_gap() {
    let mut project = project();
    let command = Command::RemoveRange {
        range: range(80, 160),
        ripple: false,
        markers: animathio_core::MarkerRipple::default(),
    };
    round_trip(&mut project, &command);
    assert_eq!(spans(&project, 0), vec![(1, 0, 80, 0), (2, 160, 200, 10)]);
    assert_eq!(
        spans(&project, 1),
        vec![(3, 50, 80, 10), (4, 160, 250, 120)]
    );
}

#[test]
fn remove_range_with_ripple_closes_the_gap_everywhere() {
    let mut project = project();
    let command = Command::RemoveRange {
        range: range(80, 160),
        ripple: true,
        markers: animathio_core::MarkerRipple::default(),
    };
    round_trip(&mut project, &command);
    assert_eq!(spans(&project, 0), vec![(1, 0, 80, 0), (2, 80, 120, 10)]);
    assert_eq!(spans(&project, 1), vec![(3, 50, 80, 10), (4, 80, 170, 120)]);

    // Fully covered clips disappear; later clips shift.
    let mut project = self::project();
    Command::RemoveRange {
        range: range(0, 100),
        ripple: true,
        markers: animathio_core::MarkerRipple::default(),
    }
    .apply(&mut project)
    .unwrap();
    assert_eq!(spans(&project, 0), vec![(2, 50, 100, 0)]);
    assert_eq!(spans(&project, 1), vec![(3, 0, 150, 60)]);
}

#[test]
fn remove_range_with_ripple_shrinks_explicit_duration() {
    let mut project = project();
    Command::SetSettings {
        width: 0,
        height: 0,
        frame_rate: project.frame_rate(),
        background: project.background(),
        duration: Time(300),
    }
    .apply(&mut project)
    .unwrap();
    round_trip(
        &mut project,
        &Command::RemoveRange {
            range: range(80, 160),
            ripple: true,
            markers: animathio_core::MarkerRipple::default(),
        },
    );
    assert_eq!(project.duration(), Time(220));
}

#[test]
fn ripple_delete_closes_the_gap_on_its_track_only() {
    let mut project = project();
    round_trip(
        &mut project,
        &Command::RippleDelete {
            clip: ClipId(1),
            markers: animathio_core::MarkerRipple::default(),
        },
    );
    assert_eq!(spans(&project, 0), vec![(2, 50, 100, 0)]);
    assert_eq!(spans(&project, 1), vec![(3, 50, 250, 10)]);
    assert!(
        Command::RippleDelete {
            clip: ClipId(42),
            markers: animathio_core::MarkerRipple::default()
        }
        .apply(&mut project)
        .is_err()
    );
}

/// Opacity of clip `id` at timeline time `at`, as evaluated.
fn opacity_at(project: &Project, id: u64, at: i64) -> f64 {
    evaluate(project, Time(at))
        .clips
        .into_iter()
        .find(|clip| clip.id == ClipId(id))
        .map(|clip| clip.transform.opacity)
        .unwrap()
}

#[test]
fn remove_range_keeps_keyframes_of_the_kept_tail_in_place() {
    // Track 0 clip 1 [0,100) fades opacity from 1 at local 50 to 0 at local 90.
    let mut project = project();
    for (time, value) in [(50, 1.0), (90, 0.0)] {
        Command::AddKeyframe {
            id: ClipId(1),
            target: Target::Transform(TransformProperty::Opacity),
            time: Time(time),
            keyframe: Keyframe {
                value: ParamValue::Number(value),
                interpolation: Interpolation::Linear,
            },
        }
        .apply(&mut project)
        .unwrap();
    }
    let before = project.clone();
    let reference = [70, 80, 95].map(|t| opacity_at(&before, 1, t));

    // Removing [20,40) without ripple splits the clip; the tail looks the same.
    let gap = Command::RemoveRange {
        range: range(20, 40),
        ripple: false,
        markers: animathio_core::MarkerRipple::default(),
    };
    round_trip(&mut project, &gap);
    let tail = project.tracks()[0]
        .clips()
        .values()
        .find(|clip| clip.range.start == Time(40))
        .unwrap()
        .id
        .0;
    assert_eq!(
        [70, 80, 95].map(|t| opacity_at(&project, tail, t)),
        reference
    );

    // With ripple the tail moves left by 20 but keeps its animation.
    let mut project = before.clone();
    let ripple = Command::RemoveRange {
        range: range(20, 40),
        ripple: true,
        markers: animathio_core::MarkerRipple::default(),
    };
    round_trip(&mut project, &ripple);
    let tail = project.tracks()[0]
        .clips()
        .values()
        .find(|clip| clip.range.start == Time(20))
        .unwrap()
        .id
        .0;
    assert_eq!(
        [50, 60, 75].map(|t| opacity_at(&project, tail, t)),
        reference
    );

    // Cutting the clip's head keeps the id and re-bases its keys too.
    let mut project = before.clone();
    let head = Command::RemoveRange {
        range: range(0, 30),
        ripple: false,
        markers: animathio_core::MarkerRipple::default(),
    };
    round_trip(&mut project, &head);
    assert_eq!(spans(&project, 0)[0], (1, 30, 100, 30));
    assert_eq!([70, 80, 95].map(|t| opacity_at(&project, 1, t)), reference);
}

#[test]
fn remove_range_keeps_split_tails_linked_to_each_other() {
    // Clips 1 (track 0, [0,100)) and 3 (track 1, [50,250)) linked as one group.
    let mut project = project();
    for id in [1, 3] {
        Command::SetClipLink {
            id: ClipId(id),
            group: Some(ClipId(1)),
        }
        .apply(&mut project)
        .unwrap();
    }
    round_trip(
        &mut project,
        &Command::RemoveRange {
            range: range(60, 80),
            ripple: false,
            markers: animathio_core::MarkerRipple::default(),
        },
    );
    let group_of = |track: usize, start: i64| {
        project.tracks()[track]
            .clips()
            .values()
            .find(|clip| clip.range.start == Time(start))
            .unwrap()
            .link_group
    };
    // Heads stay in the original group; both tails share a new one.
    assert_eq!(group_of(0, 0), Some(ClipId(1)));
    assert_eq!(group_of(1, 50), Some(ClipId(1)));
    assert!(group_of(0, 80).is_some());
    assert_eq!(group_of(0, 80), group_of(1, 80));
    assert_ne!(group_of(0, 80), Some(ClipId(1)));
}
