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
    };
    round_trip(&mut project, &command);
    assert_eq!(spans(&project, 0), vec![(1, 0, 80, 0), (2, 80, 120, 10)]);
    assert_eq!(spans(&project, 1), vec![(3, 50, 80, 10), (4, 80, 170, 120)]);

    // Fully covered clips disappear; later clips shift.
    let mut project = self::project();
    Command::RemoveRange {
        range: range(0, 100),
        ripple: true,
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
        },
    );
    assert_eq!(project.duration(), Time(220));
}

#[test]
fn ripple_delete_closes_the_gap_on_its_track_only() {
    let mut project = project();
    round_trip(&mut project, &Command::RippleDelete { clip: ClipId(1) });
    assert_eq!(spans(&project, 0), vec![(2, 50, 100, 0)]);
    assert_eq!(spans(&project, 1), vec![(3, 50, 250, 10)]);
    assert!(
        Command::RippleDelete { clip: ClipId(42) }
            .apply(&mut project)
            .is_err()
    );
}
