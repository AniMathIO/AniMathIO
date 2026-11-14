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

// --- ripple edits move markers --------------------------------------------------------

fn two_track_project() -> Project {
    let mut p = Project::default();
    for (t, name) in [(1, "V"), (2, "A")] {
        Command::AddTrack {
            track: Track::new(TrackId(t), name),
            index: (t - 1) as usize,
        }
        .apply(&mut p)
        .unwrap();
        Command::AddClip {
            track: TrackId(t),
            clip: Clip::new(
                ClipId(t),
                ClipKind::Plugin {
                    plugin_id: "x".into(),
                    params: serde_json::json!({}),
                },
                TimeRange::new(Time(0), Time(1000)).unwrap(),
            ),
        }
        .apply(&mut p)
        .unwrap();
    }
    for m in [
        marker(1, 100),
        marker(2, 300),
        marker(3, 450),
        marker(4, 700),
    ] {
        Command::AddMarker { marker: m }.apply(&mut p).unwrap();
    }
    let mut range_marker = marker(5, 250);
    range_marker.duration = Some(Time(300)); // [250, 550) straddles the cut [300, 500)
    Command::AddMarker {
        marker: range_marker,
    }
    .apply(&mut p)
    .unwrap();
    p
}
fn cut(a: i64, b: i64) -> TimeRange {
    TimeRange::new(Time(a), Time(b)).unwrap()
}
fn times(p: &Project) -> Vec<(u64, i64, Option<i64>)> {
    let mut v: Vec<_> = p
        .markers()
        .iter()
        .map(|m| (m.id.0, m.time.0, m.duration.map(|d| d.0)))
        .collect();
    v.sort_unstable();
    v
}
fn exact_undo(project: &mut Project, command: &Command) -> Project {
    let before = project.clone();
    let mut history = History::default();
    history.execute(project, command).unwrap();
    let after = project.clone();
    assert!(history.undo(project).unwrap());
    assert_eq!(*project, before, "undo restores clips and markers exactly");
    assert!(history.redo(project).unwrap());
    assert_eq!(*project, after);
    after
}

#[test]
fn remove_range_ripple_moves_markers_and_undo_is_exact() {
    let mut p = two_track_project();
    let after = exact_undo(
        &mut p,
        &Command::RemoveRange {
            range: cut(300, 500),
            ripple: true,
            markers: MarkerRipple::Auto,
        },
    );
    assert_eq!(
        times(&after),
        [
            (1, 100, None),      // before the cut: untouched
            (2, 300, None),      // at the cut start: stays
            (3, 300, None),      // inside the cut: collapsed onto it
            (4, 500, None),      // after the cut: shifted by 200
            (5, 250, Some(100)), // spanning range marker shrinks by 200 minus the clipped tail
        ]
    );
}

#[test]
fn range_markers_are_clipped_and_collapse_to_points() {
    let mut p = two_track_project();
    let mut inside = marker(6, 320);
    inside.duration = Some(Time(100)); // fully inside the cut
    Command::AddMarker { marker: inside }.apply(&mut p).unwrap();
    let mut crossing = marker(7, 400);
    crossing.duration = Some(Time(300)); // [400, 700): starts inside, ends after
    Command::AddMarker { marker: crossing }
        .apply(&mut p)
        .unwrap();
    let after = exact_undo(
        &mut p,
        &Command::RemoveRange {
            range: cut(300, 500),
            ripple: true,
            markers: MarkerRipple::Ride,
        },
    );
    let by_id: std::collections::BTreeMap<_, _> = times(&after)
        .into_iter()
        .map(|(i, t, d)| (i, (t, d)))
        .collect();
    assert_eq!(by_id[&6], (300, None), "fully inside: a point at the cut");
    assert_eq!(by_id[&7], (300, Some(200)), "start clamped, tail kept");
}

#[test]
fn markers_stay_without_ripple_and_with_stay_and_when_other_tracks_have_content() {
    let mut p = two_track_project();
    let before = times(&p);
    // No ripple: markers never move.
    Command::RemoveRange {
        range: cut(300, 500),
        ripple: false,
        markers: MarkerRipple::Ride,
    }
    .apply(&mut p)
    .unwrap();
    assert_eq!(times(&p), before);
    // Explicit Stay.
    let mut p = two_track_project();
    Command::RemoveRange {
        range: cut(300, 500),
        ripple: true,
        markers: MarkerRipple::Stay,
    }
    .apply(&mut p)
    .unwrap();
    assert_eq!(times(&p), before);
    // Auto on one track while the other still has content after the cut: markers stay.
    let mut p = two_track_project();
    Command::RemoveRangeOnTracks {
        range: cut(300, 500),
        tracks: vec![TrackId(1)],
        markers: MarkerRipple::Auto,
    }
    .apply(&mut p)
    .unwrap();
    assert_eq!(times(&p), before);
    // Ride overrides that.
    let mut p = two_track_project();
    Command::RemoveRangeOnTracks {
        range: cut(300, 500),
        tracks: vec![TrackId(1)],
        markers: MarkerRipple::Ride,
    }
    .apply(&mut p)
    .unwrap();
    assert_eq!(times(&p)[3], (4, 500, None));
}

#[test]
fn ripple_delete_and_on_tracks_ride_when_all_tracks_are_affected() {
    // Both tracks selected: Auto rides.
    let mut p = two_track_project();
    let after = exact_undo(
        &mut p,
        &Command::RemoveRangeOnTracks {
            range: cut(300, 500),
            tracks: vec![TrackId(1), TrackId(2)],
            markers: MarkerRipple::Auto,
        },
    );
    assert_eq!(times(&after)[3], (4, 500, None));

    // RippleDelete of the only clip on a single-track project rides; with another track
    // holding content after the clip it stays.
    let mut single = Project::default();
    Command::AddTrack {
        track: Track::new(TrackId(1), "V"),
        index: 0,
    }
    .apply(&mut single)
    .unwrap();
    for (id, a, b) in [(1, 0, 200), (2, 200, 600)] {
        Command::AddClip {
            track: TrackId(1),
            clip: Clip::new(
                ClipId(id),
                ClipKind::Plugin {
                    plugin_id: "x".into(),
                    params: serde_json::json!({}),
                },
                cut(a, b),
            ),
        }
        .apply(&mut single)
        .unwrap();
    }
    Command::AddMarker {
        marker: marker(1, 400),
    }
    .apply(&mut single)
    .unwrap();
    let after = exact_undo(
        &mut single,
        &Command::RippleDelete {
            clip: ClipId(1),
            markers: MarkerRipple::Auto,
        },
    );
    assert_eq!(times(&after), [(1, 200, None)]);

    let mut multi = two_track_project();
    let before = times(&multi);
    Command::RippleDelete {
        clip: ClipId(1),
        markers: MarkerRipple::Auto,
    }
    .apply(&mut multi)
    .unwrap();
    assert_eq!(
        times(&multi),
        before,
        "track 2 still has content after the clip"
    );
}

#[test]
fn marker_ripple_serializes_with_a_default_for_old_commands() {
    let old: Command =
        serde_json::from_str(r#"{"RemoveRange":{"range":{"start":0,"end":10},"ripple":true}}"#)
            .unwrap();
    assert!(matches!(
        old,
        Command::RemoveRange {
            markers: MarkerRipple::Auto,
            ..
        }
    ));
}
