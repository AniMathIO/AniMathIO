//! Markers: commands, exact inversion, batches, ordering and validation.
use animathio_core::*;

fn marker(id: u64, time: i64) -> Marker {
    let mut m = Marker::new(MarkerId(id), Time(time));
    m.name = format!("m{id}");
    m
}
fn ids(project: &Project) -> Vec<u64> {
    project.markers().iter().map(|m| m.id.0).collect()
}

#[test]
fn add_remove_move_update_are_exactly_invertible() {
    let mut project = Project::default();
    let original = project.clone();
    let mut undo = Vec::new();
    for (id, t) in [(1, 300), (2, 100), (3, 200)] {
        undo.push(
            Command::AddMarker {
                marker: marker(id, t),
            }
            .apply(&mut project)
            .unwrap(),
        );
    }
    assert_eq!(ids(&project), [2, 3, 1], "sorted by time");
    let added = project.clone();

    let undo_move = Command::MoveMarker {
        id: MarkerId(2),
        time: Time(250),
    }
    .apply(&mut project)
    .unwrap();
    assert_eq!(ids(&project), [3, 2, 1]);
    undo_move.apply(&mut project).unwrap();
    assert_eq!(project, added);

    let mut edited = marker(3, 200);
    edited.name = "Drop".into();
    edited.color = MarkerColor::Red;
    edited.note = "chorus starts".into();
    edited.duration = Some(Time(50));
    let undo_update = Command::UpdateMarker {
        id: MarkerId(3),
        marker: edited.clone(),
    }
    .apply(&mut project)
    .unwrap();
    assert_eq!(project.marker(MarkerId(3)), Some(&edited));
    undo_update.apply(&mut project).unwrap();
    assert_eq!(project, added);

    let undo_remove = Command::RemoveMarker { id: MarkerId(3) }
        .apply(&mut project)
        .unwrap();
    assert_eq!(ids(&project), [2, 1]);
    undo_remove.apply(&mut project).unwrap();
    assert_eq!(project, added);

    for inverse in undo.into_iter().rev() {
        inverse.apply(&mut project).unwrap();
    }
    assert_eq!(project, original);
}

#[test]
fn rejects_duplicates_missing_and_invalid_markers_atomically() {
    let mut project = Project::default();
    Command::AddMarker {
        marker: marker(1, 10),
    }
    .apply(&mut project)
    .unwrap();
    let before = project.clone();
    for command in [
        Command::AddMarker {
            marker: marker(1, 20),
        },
        Command::AddMarker {
            marker: marker(2, -1),
        },
        Command::RemoveMarker { id: MarkerId(9) },
        Command::MoveMarker {
            id: MarkerId(1),
            time: Time(-5),
        },
        Command::MoveMarker {
            id: MarkerId(9),
            time: Time(5),
        },
    ] {
        assert!(command.apply(&mut project).is_err(), "{command:?}");
        assert_eq!(project, before);
    }
    let mut bad = marker(2, 5);
    bad.duration = Some(Time(0));
    assert!(
        Command::AddMarker { marker: bad }
            .apply(&mut project)
            .is_err()
    );
    let mut long = marker(2, 5);
    long.name = "x".repeat(MAX_MARKER_NAME + 1);
    assert!(
        Command::AddMarker { marker: long }
            .apply(&mut project)
            .is_err()
    );
    assert_eq!(project, before);
}

#[test]
fn batch_is_one_undo_step() {
    let mut project = Project::default();
    let mut history = History::default();
    history
        .execute(
            &mut project,
            &Command::Batch {
                commands: (1..=5)
                    .map(|i| Command::AddMarker {
                        marker: marker(i, i64::try_from(i).unwrap() * 100),
                    })
                    .collect(),
            },
        )
        .unwrap();
    assert_eq!(project.markers().len(), 5);
    history.undo(&mut project).unwrap();
    assert_eq!(project.markers(), []);
    history.redo(&mut project).unwrap();
    assert_eq!(project.markers().len(), 5);
}

#[test]
fn serde_round_trip_sorts_and_omits_empty_fields() {
    let mut project = Project::default();
    for (id, t) in [(2, 50), (1, 50), (3, 10)] {
        Command::AddMarker {
            marker: marker(id, t),
        }
        .apply(&mut project)
        .unwrap();
    }
    assert_eq!(ids(&project), [3, 1, 2], "ties break by id");
    let value = serde_json::to_value(&project).unwrap();
    assert!(value["markers"][0].get("duration").is_none());
    assert!(value["markers"][0].get("note").is_none());
    let back: Project = serde_json::from_value(value).unwrap();
    assert_eq!(back, project);

    // An unsorted document is normalised on load; a project without markers has no key.
    let json = serde_json::json!({"frame_rate": {"numerator": 30, "denominator": 1},
        "tracks": [], "assets": {},
        "markers": [{"id": 2, "time": 9}, {"id": 1, "time": 3}]});
    let loaded: Project = serde_json::from_value(json).unwrap();
    assert_eq!(ids(&loaded), [1, 2]);
    assert!(
        serde_json::to_value(Project::default())
            .unwrap()
            .get("markers")
            .is_none()
    );
}

#[test]
fn marker_ids_are_allocated_past_the_largest() {
    let mut project = Project::default();
    assert_eq!(project.next_marker_id(), MarkerId(1));
    Command::AddMarker {
        marker: marker(7, 0),
    }
    .apply(&mut project)
    .unwrap();
    assert_eq!(project.next_marker_id(), MarkerId(8));
}

#[test]
fn palette_names_round_trip() {
    for color in MarkerColor::ALL {
        assert_eq!(MarkerColor::parse(color.name()), Some(color));
    }
    assert_eq!(MarkerColor::parse("magenta"), None);
}
