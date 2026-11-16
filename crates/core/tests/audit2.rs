//! Regression tests for the second audit (linked edits, large generated inverses).
use animathio_core::*;

fn range(a: i64, b: i64) -> TimeRange {
    TimeRange::new(Time(a), Time(b)).unwrap()
}

/// Two tracks, clip 1 (track 1) linked to clip 2 (track 2), both `a..b`.
fn pair(a: i64, b: i64) -> Project {
    let mut project = Project::default();
    for n in 1..=2u64 {
        Command::AddTrack {
            track: Track::new(TrackId(n), "track"),
            index: usize::try_from(n - 1).unwrap(),
        }
        .apply(&mut project)
        .unwrap();
        let mut clip = Clip::new(ClipId(n), ClipKind::Error("test".into()), range(a, b));
        clip.link_group = Some(ClipId(1));
        Command::AddClip {
            track: TrackId(n),
            clip,
        }
        .apply(&mut project)
        .unwrap();
    }
    project
}

fn clip(project: &Project, id: u64) -> Clip {
    project
        .tracks()
        .iter()
        .find_map(|t| t.clips().get(&ClipId(id)))
        .unwrap()
        .clone()
}

#[test]
fn undo_of_large_generated_edit_works() {
    let mut project = Project::default();
    Command::AddTrack {
        track: Track::new(TrackId(1), "t"),
        index: 0,
    }
    .apply(&mut project)
    .unwrap();
    for n in 0..1100u64 {
        let start = i64::try_from(n).unwrap() * 100;
        Command::AddClip {
            track: TrackId(1),
            clip: Clip::new(
                ClipId(n + 1),
                ClipKind::Error("x".into()),
                range(start, start + 100),
            ),
        }
        .apply(&mut project)
        .unwrap();
    }
    let before = project.clone();
    let mut history = History::default();
    // A small edit first: it must stay undoable after the large one fails or succeeds.
    history
        .execute(
            &mut project,
            &Command::RenameTrack {
                id: TrackId(1),
                name: "renamed".into(),
            },
        )
        .unwrap();
    let renamed = project.clone();
    let times: Vec<Time> = (0..1100i64).map(|n| Time(n * 100 + 50)).collect();
    history
        .execute(&mut project, &Command::SplitAt { times })
        .unwrap();
    assert_eq!(project.tracks()[0].clips().len(), 2200);
    let edited = project.clone();
    assert!(history.undo(&mut project).unwrap());
    assert_eq!(project, renamed);
    assert!(history.redo(&mut project).unwrap());
    assert_eq!(project, edited);
    assert!(history.undo(&mut project).unwrap());
    assert!(history.undo(&mut project).unwrap());
    assert_eq!(project, before);
}

#[test]
fn failed_undo_does_not_block_the_stack() {
    let mut project = pair(0, 100);
    let mut history = History::default();
    history
        .execute(
            &mut project,
            &Command::RenameTrack {
                id: TrackId(1),
                name: "kept".into(),
            },
        )
        .unwrap();
    history
        .execute(&mut project, &Command::RemoveTrack { id: TrackId(2) })
        .unwrap();
    // Corrupt the world so the newest inverse cannot apply (a clip id now collides).
    Command::AddTrack {
        track: Track::new(TrackId(2), "other"),
        index: 1,
    }
    .apply(&mut project)
    .unwrap();
    assert!(history.undo(&mut project).is_err());
    // The failed entry is gone and the earlier one still undoes.
    assert!(history.undo(&mut project).unwrap());
    assert!(!history.undo(&mut project).unwrap());
}

#[test]
fn linked_move_with_extreme_delta_errors_instead_of_panicking() {
    let mut project = pair(10, 20);
    let error = Command::MoveClip {
        id: ClipId(1),
        track: TrackId(1),
        start: Time(i64::MIN),
    }
    .apply(&mut project)
    .unwrap_err();
    assert!(error.0.contains("overflow"), "{error:?}");
}

#[test]
fn trim_applies_deltas_to_misaligned_companions() {
    let mut project = pair(0, 100);
    // Misalign the companion by hand: 5 ticks later, same length.
    for command in [
        Command::SetClipLink {
            id: ClipId(2),
            group: None,
        },
        Command::MoveClip {
            id: ClipId(2),
            track: TrackId(2),
            start: Time(5),
        },
        Command::SetClipLink {
            id: ClipId(2),
            group: Some(ClipId(1)),
        },
    ] {
        command.apply(&mut project).unwrap();
    }
    let a = clip(&project, 1);
    assert_eq!(clip(&project, 2).range, range(5, 105));
    Command::TrimClip {
        id: ClipId(1),
        range: range(10, 90),
        source_offset: Time(10),
    }
    .apply(&mut project)
    .unwrap();
    assert_eq!(clip(&project, 1).range, range(10, 90));
    // Deltas: start +10, end -10 → companion 15..95, not the anchor's 10..90.
    assert_eq!(clip(&project, 2).range, range(15, 95));
    assert_eq!(clip(&project, 2).source_offset, Time(10));
    assert_eq!(a.range, range(0, 100));
}

#[test]
fn merge_of_linked_split_merges_the_companion_too() {
    let mut project = pair(0, 100);
    let before = project.clone();
    Command::SplitClip {
        id: ClipId(1),
        right_id: ClipId(3),
        at: Time(50),
    }
    .apply(&mut project)
    .unwrap();
    let split = project.clone();
    let inverse = Command::MergeClip {
        left: ClipId(1),
        right: ClipId(3),
    }
    .apply(&mut project)
    .unwrap();
    for id in 1..=2 {
        assert_eq!(clip(&project, id).range, range(0, 100));
        assert_eq!(clip(&project, id).link_group, Some(ClipId(1)));
    }
    assert_eq!(project.tracks()[0].clips().len(), 1);
    assert_eq!(project.tracks()[1].clips().len(), 1);
    assert_eq!(project, before);
    inverse.apply(&mut project).unwrap();
    assert_eq!(project, split);
}

#[test]
fn merge_of_linked_with_unlinked_is_refused() {
    let mut project = pair(0, 100);
    Command::SplitClip {
        id: ClipId(1),
        right_id: ClipId(3),
        at: Time(50),
    }
    .apply(&mut project)
    .unwrap();
    Command::SetClipLink {
        id: ClipId(3),
        group: None,
    }
    .apply(&mut project)
    .unwrap();
    let before = project.clone();
    let error = Command::MergeClip {
        left: ClipId(1),
        right: ClipId(3),
    }
    .apply(&mut project)
    .unwrap_err();
    assert!(error.0.contains("linked"), "{error:?}");
    assert_eq!(project, before);
}
