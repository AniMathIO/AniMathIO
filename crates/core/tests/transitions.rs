//! Edit-point transitions: commands, validation, serde defaults and evaluation.
#![allow(clippy::float_cmp)]
use animathio_core::*;
use serde_json::json;

fn clip(id: u64, start: i64, end: i64) -> Clip {
    Clip::new(
        ClipId(id),
        ClipKind::Plugin {
            plugin_id: "org.animathio.test@1".into(),
            params: json!({}),
        },
        TimeRange::new(Time(start), Time(end)).unwrap(),
    )
}
/// Track 0 holds clips 1 `[0,100)`, 2 `[100,200)` and 3 `[250,300)`.
fn project() -> Project {
    let mut project = Project::default();
    Command::AddTrack {
        track: Track::new(TrackId(0), "v"),
        index: 0,
    }
    .apply(&mut project)
    .unwrap();
    for c in [clip(1, 0, 100), clip(2, 100, 200), clip(3, 250, 300)] {
        Command::AddClip {
            track: TrackId(0),
            clip: c,
        }
        .apply(&mut project)
        .unwrap();
    }
    project
}
fn dissolve(id: u64, from: u64, to: u64, duration: i64) -> Transition {
    Transition {
        id: TransitionId(id),
        kind: TransitionKind::CrossDissolve,
        duration: Time(duration),
        from: ClipId(from),
        to: ClipId(to),
    }
}
fn add(project: &mut Project, transition: Transition) -> Result<Command, Error> {
    Command::AddTransition {
        track: TrackId(0),
        transition,
    }
    .apply(project)
}

#[test]
fn add_remove_and_resize_are_exactly_invertible() {
    let original = project();
    let mut project = original.clone();
    let undo_add = add(&mut project, dissolve(7, 1, 2, 40)).unwrap();
    assert_eq!(project.tracks()[0].transitions().len(), 1);
    let with_transition = project.clone();

    let undo_resize = Command::SetTransitionDuration {
        id: TransitionId(7),
        duration: Time(60),
    }
    .apply(&mut project)
    .unwrap();
    assert_eq!(project.tracks()[0].transitions()[0].duration, Time(60));
    undo_resize.apply(&mut project).unwrap();
    assert_eq!(project, with_transition);

    let undo_remove = Command::RemoveTransition {
        id: TransitionId(7),
    }
    .apply(&mut project)
    .unwrap();
    assert_eq!(project, original);
    undo_remove.apply(&mut project).unwrap();
    assert_eq!(project, with_transition);
    undo_add.apply(&mut project).unwrap();
    assert_eq!(project, original);
}

#[test]
fn validation_rejects_bad_transitions_without_changing_the_project() {
    let original = project();
    let mut project = original.clone();
    // Not adjacent.
    assert!(add(&mut project, dissolve(1, 2, 3, 20)).is_err());
    // Reversed order.
    assert!(add(&mut project, dissolve(1, 2, 1, 20)).is_err());
    // Too long for the clips.
    assert!(add(&mut project, dissolve(1, 1, 2, 202)).is_err());
    // Nonpositive duration.
    assert!(add(&mut project, dissolve(1, 1, 2, 0)).is_err());
    // Wrong track.
    assert!(
        Command::AddTransition {
            track: TrackId(9),
            transition: dissolve(1, 1, 2, 20),
        }
        .apply(&mut project)
        .is_err()
    );
    assert_eq!(project, original);

    add(&mut project, dissolve(1, 1, 2, 20)).unwrap();
    // Duplicate id and duplicate edit point.
    assert!(add(&mut project, dissolve(1, 1, 2, 10)).is_err());
    assert!(add(&mut project, dissolve(2, 1, 2, 10)).is_err());
    // Resize past the clip length.
    assert!(
        Command::SetTransitionDuration {
            id: TransitionId(1),
            duration: Time(500),
        }
        .apply(&mut project)
        .is_err()
    );
    // Edits that break adjacency fail until the transition is removed.
    assert!(
        Command::MoveClip {
            id: ClipId(2),
            track: TrackId(0),
            start: Time(120),
        }
        .apply(&mut project)
        .is_err()
    );
    assert!(
        Command::RemoveClip { id: ClipId(1) }
            .apply(&mut project)
            .is_err()
    );
}

#[test]
fn clip_must_cover_transitions_on_both_edges() {
    let mut project = Project::default();
    Command::AddTrack {
        track: Track::new(TrackId(0), "v"),
        index: 0,
    }
    .apply(&mut project)
    .unwrap();
    for c in [clip(1, 0, 100), clip(2, 100, 140), clip(3, 140, 300)] {
        Command::AddClip {
            track: TrackId(0),
            clip: c,
        }
        .apply(&mut project)
        .unwrap();
    }
    add(&mut project, dissolve(1, 1, 2, 40)).unwrap(); // uses 20 of clip 2
    assert!(add(&mut project, dissolve(2, 2, 3, 42)).is_err()); // would need 21 more
    add(&mut project, dissolve(2, 2, 3, 40)).unwrap();
}

#[test]
fn old_projects_load_without_transitions() {
    let project = project();
    let mut value = serde_json::to_value(&project).unwrap();
    assert!(value["tracks"][0].get("transitions").is_none());
    value["tracks"][0]
        .as_object_mut()
        .unwrap()
        .remove("transitions");
    let loaded: Project = serde_json::from_value(value).unwrap();
    assert_eq!(loaded, project);

    let mut with = project.clone();
    add(&mut with, dissolve(1, 1, 2, 20)).unwrap();
    let round: Project = serde_json::from_str(&serde_json::to_string(&with).unwrap()).unwrap();
    assert_eq!(round, with);
}

#[test]
fn evaluate_shows_both_clips_with_matching_progress() {
    let mut project = project();
    add(&mut project, dissolve(1, 1, 2, 40)).unwrap();
    // Window is [80, 120).
    let before = evaluate(&project, Time(79));
    assert_eq!(before.clips.len(), 1);
    assert!(before.clips[0].transition.is_none());

    let scene = evaluate(&project, Time(90));
    assert_eq!(scene.clips.len(), 2);
    let out = scene.clips.iter().find(|c| c.id == ClipId(1)).unwrap();
    let inc = scene.clips.iter().find(|c| c.id == ClipId(2)).unwrap();
    let (o, i) = (out.transition.unwrap(), inc.transition.unwrap());
    assert_eq!(o.role, TransitionRole::Outgoing);
    assert_eq!(i.role, TransitionRole::Incoming);
    assert_eq!(o.progress, 0.25);
    assert_eq!(i.progress, 0.25);
    assert_eq!(
        inc.source_time,
        Time(0),
        "early incoming frames hold source 0"
    );

    let after_cut = evaluate(&project, Time(110));
    let out = after_cut.clips.iter().find(|c| c.id == ClipId(1)).unwrap();
    assert_eq!(
        out.source_time,
        Time(110),
        "outgoing clip plays past its end"
    );
    assert_eq!(out.transition.unwrap().progress, 0.75);

    let end = evaluate(&project, Time(120));
    assert_eq!(end.clips.len(), 1);
    assert_eq!(end.clips[0].id, ClipId(2));
    assert!(end.clips[0].transition.is_none());
    // Pure: repeated evaluation is identical.
    assert_eq!(evaluate(&project, Time(90)), evaluate(&project, Time(90)));
}

#[test]
fn equal_power_curves_keep_constant_power() {
    for step in 0..=10 {
        let (a, b) = equal_power(f64::from(step) / 10.0);
        assert!((a * a + b * b - 1.0).abs() < 1e-12);
    }
    assert_eq!(equal_power(0.0), (1.0, 0.0));
}

#[test]
fn batch_edit_commands_keep_transitions_valid_and_atomic() {
    let mut project = project();
    add(&mut project, dissolve(7, 1, 2, 40)).unwrap();
    let before = project.clone();

    // Removing the whole outgoing clip would orphan the transition: the batch is
    // rejected as a whole and the project is untouched.
    let remove = Command::RemoveRange {
        range: TimeRange::new(Time(0), Time(100)).unwrap(),
        ripple: true,
    };
    assert!(remove.apply(&mut project).is_err());
    assert_eq!(project, before);

    // A batch mixing a transition edit with a ripple delete after it is one undo step.
    let batch = Command::Batch {
        commands: vec![
            Command::SetTransitionDuration {
                id: TransitionId(7),
                duration: Time(20),
            },
            Command::RippleDelete { clip: ClipId(3) },
        ],
    };
    let undo = batch.apply(&mut project).unwrap();
    assert_eq!(project.tracks()[0].transitions()[0].duration, Time(20));
    assert!(!project.tracks()[0].clips().contains_key(&ClipId(3)));
    undo.apply(&mut project).unwrap();
    assert_eq!(project, before);
}
