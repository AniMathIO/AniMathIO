//! External clients cannot send undo primitives, including in nested batches.
use animathio_control::{Call, ControlError, ControlHandler, HeadlessHandler};
use animathio_core::{Clip, ClipId, ClipKind, Command, Project, Time, TimeRange, Track, TrackId};

#[test]
fn external_internal_commands_are_rejected_without_an_edit_or_history_entry() {
    for internal in [
        Command::Exact {
            command: Box::new(Command::RemoveClip { id: ClipId(1) }),
        },
        Command::RestoreClips {
            track: TrackId(1),
            ids: vec![ClipId(1)],
            clips: vec![],
        },
    ] {
        for nested in [false, true] {
            let mut handler = HeadlessHandler::new();
            let initial = Command::Batch {
                commands: vec![
                    Command::AddTrack {
                        track: Track::new(TrackId(1), "track"),
                        index: 0,
                    },
                    Command::AddClip {
                        track: TrackId(1),
                        clip: Clip::new(
                            ClipId(1),
                            ClipKind::Plugin {
                                plugin_id: "org.animathio.test".into(),
                                params: serde_json::json!({}),
                            },
                            TimeRange::new(Time(0), Time(100)).unwrap(),
                        ),
                    },
                ],
            };
            handler.command_apply(initial).unwrap();
            let before = handler.project().clone();
            let command = if nested {
                Command::Batch {
                    commands: vec![
                        Command::RenameTrack {
                            id: TrackId(1),
                            name: "must not land".into(),
                        },
                        Command::Batch {
                            commands: vec![internal.clone()],
                        },
                    ],
                }
            } else {
                internal.clone()
            };
            // Exercise the same deserialization path as JSON-RPC, not just a helper.
            let call: Call = serde_json::from_value(
                serde_json::json!({"method":"command.apply", "params":{"command":command}}),
            )
            .unwrap();
            assert!(matches!(
                handler.handle(call),
                Err(ControlError::InvalidParams(_))
            ));
            assert_eq!(handler.project(), &before);
            assert!(handler.history_undo().unwrap());
            assert_eq!(handler.project(), &Project::default());
            assert!(!handler.history_undo().unwrap());
        }
    }
}
