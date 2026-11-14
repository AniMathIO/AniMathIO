//! Companion edit transactions.
use animathio_core::*;

#[test]
fn companion_edit_and_undo() {
    let mut project = Project::default();
    for n in 1..=2 {
        Command::AddTrack {
            track: Track::new(TrackId(n), "track"),
            index: usize::try_from(n - 1).unwrap(),
        }
        .apply(&mut project)
        .unwrap();
        let mut clip = Clip::new(
            ClipId(n),
            ClipKind::Error("test".into()),
            TimeRange::new(Time(0), Time(100)).unwrap(),
        );
        clip.link_group = Some(ClipId(1));
        Command::AddClip {
            track: TrackId(n),
            clip,
        }
        .apply(&mut project)
        .unwrap();
    }
    for command in [
        Command::MoveClip {
            id: ClipId(1),
            track: TrackId(1),
            start: Time(10),
        },
        Command::TrimClip {
            id: ClipId(1),
            range: TimeRange::new(Time(10), Time(90)).unwrap(),
            source_offset: Time(10),
        },
        Command::SplitClip {
            id: ClipId(1),
            right_id: ClipId(3),
            at: Time(50),
        },
        Command::RemoveClip { id: ClipId(1) },
        Command::Batch {
            commands: vec![
                Command::RemoveClip { id: ClipId(1) },
                Command::RemoveClip { id: ClipId(2) },
            ],
        },
        Command::RippleDelete {
            clip: ClipId(1),
            markers: animathio_core::MarkerRipple::default(),
        },
        Command::SplitAt {
            times: vec![Time(50)],
        },
        Command::RemoveRange {
            range: TimeRange::new(Time(30), Time(60)).unwrap(),
            ripple: true,
            markers: animathio_core::MarkerRipple::default(),
        },
    ] {
        let before = project.clone();
        let inverse = command.apply(&mut project).unwrap();
        assert_eq!(
            project.tracks()[0].clips().len(),
            project.tracks()[1].clips().len()
        );
        let edited = project.clone();
        let redo = inverse.apply(&mut project).unwrap();
        assert_eq!(project, before);
        let undo = redo.apply(&mut project).unwrap();
        assert_eq!(project, edited);
        undo.apply(&mut project).unwrap();
        assert_eq!(project, before);
    }
}

#[test]
fn split_halves_link_independently_and_locked_companions_block_edits() {
    let mut project = Project::default();
    for n in 1..=2 {
        Command::AddTrack {
            track: Track::new(TrackId(n), "track"),
            index: usize::try_from(n - 1).unwrap(),
        }
        .apply(&mut project)
        .unwrap();
        let mut clip = Clip::new(
            ClipId(n),
            ClipKind::Error("test".into()),
            TimeRange::new(Time(0), Time(100)).unwrap(),
        );
        clip.link_group = Some(ClipId(1));
        Command::AddClip {
            track: TrackId(n),
            clip,
        }
        .apply(&mut project)
        .unwrap();
    }
    Command::SetTrackLocked {
        id: TrackId(2),
        locked: true,
    }
    .apply(&mut project)
    .unwrap();
    let before = project.clone();
    assert!(
        Command::RemoveClip { id: ClipId(1) }
            .apply(&mut project)
            .is_err()
    );
    assert_eq!(before, project);
    Command::SetTrackLocked {
        id: TrackId(2),
        locked: false,
    }
    .apply(&mut project)
    .unwrap();
    Command::SplitClip {
        id: ClipId(1),
        right_id: ClipId(3),
        at: Time(50),
    }
    .apply(&mut project)
    .unwrap();
    assert_eq!(
        project.tracks()[0].clips()[&ClipId(3)].link_group,
        project.tracks()[1].clips()[&ClipId(4)].link_group
    );
    Command::RemoveClip { id: ClipId(3) }
        .apply(&mut project)
        .unwrap();
    assert_eq!(project.tracks()[0].clips().len(), 1);
    assert_eq!(project.tracks()[1].clips().len(), 1);
    Command::SetClipLink {
        id: ClipId(1),
        group: None,
    }
    .apply(&mut project)
    .unwrap();
    Command::RemoveClip { id: ClipId(1) }
        .apply(&mut project)
        .unwrap();
    assert_eq!(project.tracks()[1].clips().len(), 1);
}

#[test]
fn split_skips_a_linked_companion_that_does_not_span_the_split_point() {
    let mut project = Project::default();
    // Track 1: clip 1 [0,100). Track 2: clip 2 [0,40) ends before the cut and
    // clip 4 [60,100) starts after it; all three are linked.
    for (n, track, start, end) in [(1u64, 1u64, 0, 100), (2, 2, 0, 40), (4, 2, 60, 100)] {
        if n != 4 {
            Command::AddTrack {
                track: Track::new(TrackId(track), "track"),
                index: usize::try_from(track - 1).unwrap(),
            }
            .apply(&mut project)
            .unwrap();
        }
        let mut clip = Clip::new(
            ClipId(n),
            ClipKind::Error("test".into()),
            TimeRange::new(Time(start), Time(end)).unwrap(),
        );
        clip.link_group = Some(ClipId(1));
        Command::AddClip {
            track: TrackId(track),
            clip,
        }
        .apply(&mut project)
        .unwrap();
    }
    let before = project.clone();
    let mut history = History::default();
    history
        .execute(
            &mut project,
            &Command::SplitClip {
                id: ClipId(1),
                right_id: ClipId(3),
                at: Time(50),
            },
        )
        .unwrap();
    assert_eq!(project.tracks()[0].clips().len(), 2);
    assert_eq!(project.tracks()[1].clips().len(), 2);
    let group = |track: usize, id: u64| project.tracks()[track].clips()[&ClipId(id)].link_group;
    // The left half keeps the old group with the companion before the cut; the
    // right half shares a new group with the companion after it.
    assert_eq!(group(0, 1), group(1, 2));
    assert_eq!(group(0, 3), group(1, 4));
    assert_ne!(group(0, 1), group(0, 3));
    assert!(history.undo(&mut project).unwrap());
    assert_eq!(project, before);
}
