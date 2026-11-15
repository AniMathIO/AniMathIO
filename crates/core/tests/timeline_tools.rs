//! Command contracts used by Premiere timeline drag planners.
use animathio_core::*;

fn fixture() -> Project {
    let mut project = Project::default();
    let asset = AssetId::of(b"timeline tool media");
    Command::AddAsset {
        id: asset.clone(),
        asset: Asset {
            name: "media.mp4".into(),
            media_type: "video/mp4".into(),
        },
    }
    .apply(&mut project)
    .unwrap();
    for row in 0..2 {
        let track = TrackId(row + 1);
        Command::AddTrack {
            track: Track::new(track, "Linked"),
            index: usize::try_from(row).unwrap(),
        }
        .apply(&mut project)
        .unwrap();
        for i in 0..3 {
            let mut clip = Clip::new(
                ClipId(row * 3 + i + 1),
                if row == 0 {
                    ClipKind::Video(asset.clone())
                } else {
                    ClipKind::Audio(asset.clone())
                },
                TimeRange::new(
                    Time(i64::try_from(i).unwrap() * 1000),
                    Time(i64::try_from(i + 1).unwrap() * 1000),
                )
                .unwrap(),
            );
            clip.source_offset = Time(1000);
            clip.link_group = Some(ClipId(i + 1));
            Command::AddClip { track, clip }
                .apply(&mut project)
                .unwrap();
        }
    }
    project
}
fn trim(id: u64, start: i64, end: i64, offset: i64) -> Command {
    Command::TrimClip {
        id: ClipId(id),
        range: TimeRange::new(Time(start), Time(end)).unwrap(),
        source_offset: Time(offset),
    }
}
#[test]
fn linked_audio_tool_batches_are_exactly_invertible() {
    let commands = [
        vec![Command::SetClipSpeed {
            id: ClipId(2),
            speed: ClipSpeed::new(2, 1).unwrap(),
            reverse: false,
            ramps: vec![],
            ripple: false,
        }],
        vec![trim(2, 1000, 2000, 1100)],
        vec![
            trim(1, 0, 1100, 1000),
            trim(3, 2100, 3000, 1100),
            Command::MoveClip {
                id: ClipId(2),
                track: TrackId(1),
                start: Time(1100),
            },
        ],
        vec![trim(3, 2100, 3000, 1100), trim(2, 1000, 2100, 1000)],
    ];
    for commands in commands {
        let before = fixture();
        let mut project = before.clone();
        let command = Command::Batch { commands };
        let inverse = command.apply(&mut project).unwrap();
        let a = &project.tracks()[0].clips()[&ClipId(2)];
        let b = &project.tracks()[1].clips()[&ClipId(5)];
        assert_eq!(a.range, b.range);
        assert_eq!(a.source_bounds(), b.source_bounds());
        let after = project.clone();
        let redo = inverse.apply(&mut project).unwrap();
        assert_eq!(project, before);
        redo.apply(&mut project).unwrap();
        assert_eq!(project, after);
    }
}
