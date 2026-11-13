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
        Command::RippleDelete { clip: ClipId(1) },
        Command::SplitAt {
            times: vec![Time(50)],
        },
        Command::RemoveRange {
            range: TimeRange::new(Time(30), Time(60)).unwrap(),
            ripple: true,
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
