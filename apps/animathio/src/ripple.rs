//! Ripple planning. A ripple edit closes (or opens) a gap by sliding later clips,
//! either on the edited track only or linked across every unlocked track. Plans are
//! plain `Command` batches, so a ripple is always a single undo step.

use std::collections::BTreeSet;

use animathio_core::{Clip, ClipId, Command, Project, Time, TimeRange, Track};

use crate::editing;
use crate::selection::{batch, existing};
use crate::timeline::TrimEdge;

/// Which tracks a ripple edit slides.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum RippleScope {
    /// Only the track being edited.
    #[default]
    ThisTrack,
    /// Every unlocked track, so cross-track sync is preserved.
    AllTracks,
}

impl RippleScope {
    pub(crate) fn toggled(self) -> Self {
        match self {
            Self::ThisTrack => Self::AllTracks,
            Self::AllTracks => Self::ThisTrack,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::ThisTrack => "Ripple: this track only",
            Self::AllTracks => "Ripple: all tracks",
        }
    }
}

/// Merge half-open `(start, end)` intervals that overlap or touch.
fn merge(mut gaps: Vec<(i64, i64)>) -> Vec<(i64, i64)> {
    gaps.sort_unstable();
    let mut merged: Vec<(i64, i64)> = Vec::new();
    for gap in gaps {
        match merged.last_mut() {
            Some(last) if gap.0 <= last.1 => last.1 = last.1.max(gap.1),
            _ => merged.push(gap),
        }
    }
    merged
}

/// Slide the clips of `track` by `offset(start)` (negative = earlier) without ever
/// overlapping the clip before them: a clip that would collide is held back.
/// `skip` clips are neither moved nor tracked (they are being removed);
/// `fixed` is a clip whose post-edit range is already decided and must still
/// bound its neighbours.
fn slide_track(
    track: &Track,
    skip: &BTreeSet<ClipId>,
    fixed: Option<(ClipId, TimeRange)>,
    offset: &dyn Fn(i64) -> i64,
) -> Option<Vec<Command>> {
    let mut clips: Vec<&Clip> = track
        .clips()
        .values()
        .filter(|clip| !skip.contains(&clip.id))
        .collect();
    clips.sort_by_key(|clip| clip.range.start);
    let mut commands = Vec::new();
    let mut previous_end = i64::MIN;
    for clip in clips {
        if let Some((id, range)) = fixed
            && id == clip.id
        {
            previous_end = range.end.0;
            continue;
        }
        let start = clip.range.start.0;
        let moved = start.checked_add(offset(start))?.max(previous_end).max(0);
        let length = clip.range.end.0 - start;
        if moved != start {
            commands.push(Command::MoveClip {
                id: clip.id,
                track: track.id(),
                start: Time(moved),
            });
        }
        previous_end = moved.checked_add(length)?;
    }
    Some(commands)
}

/// Delete `ids` and close the gaps they leave. With [`RippleScope::ThisTrack`] each
/// deleted clip only pulls later clips of its own track; with
/// [`RippleScope::AllTracks`] every deleted span pulls every unlocked track
/// (locked tracks neither move nor may hold a deleted clip).
pub(crate) fn delete_command(
    project: &Project,
    ids: &BTreeSet<ClipId>,
    scope: RippleScope,
) -> Option<Command> {
    let removed = existing(project, ids);
    if removed.is_empty() {
        return None;
    }
    let spans = |track: &Track| -> Vec<(i64, i64)> {
        track
            .clips()
            .values()
            .filter(|clip| removed.contains(&clip.id))
            .map(|clip| (clip.range.start.0, clip.range.end.0))
            .collect()
    };
    let all_spans = merge(project.tracks().iter().flat_map(spans).collect());
    let mut commands: Vec<Command> = removed
        .iter()
        .map(|&id| Command::RemoveClip { id })
        .collect();
    for track in project.tracks() {
        let gaps = match scope {
            RippleScope::ThisTrack => merge(spans(track)),
            RippleScope::AllTracks if track.is_locked() => continue,
            RippleScope::AllTracks => all_spans.clone(),
        };
        if gaps.is_empty() {
            continue;
        }
        let pulled = |start: i64| {
            -gaps
                .iter()
                .filter(|gap| gap.1 <= start)
                .map(|gap| gap.1 - gap.0)
                .sum::<i64>()
        };
        commands.extend(slide_track(track, &removed, None, &pulled)?);
    }
    batch(commands)
}

/// Trim one edge of a clip to `at`, rippling later clips by the length change. With
/// [`RippleScope::AllTracks`] every unlocked track slides, not only the clip's own.
pub(crate) fn trim_command(
    project: &Project,
    id: ClipId,
    edge: TrimEdge,
    at: Time,
    scope: RippleScope,
) -> Option<Command> {
    if scope == RippleScope::ThisTrack {
        return editing::trim_command(project, id, edge, at, true);
    }
    let own = crate::selection::track_of(project, id)?;
    let clip = own.clips().get(&id)?;
    let plain = editing::trim_command(project, id, edge, at, false)?;
    let Command::TrimClip { range, .. } = &plain else {
        return None;
    };
    // After a left trim the clip is moved back to where it started so the cut
    // collapses instead of leaving a gap at its head.
    let settled = match edge {
        TrimEdge::Left => TimeRange::new(
            clip.range.start,
            Time(clip.range.start.0 + (range.end.0 - range.start.0)),
        )
        .ok()?,
        TrimEdge::Right => *range,
    };
    let delta = (range.end.0 - range.start.0) - (clip.range.end.0 - clip.range.start.0);
    let from = clip.range.end.0;
    let later = |start: i64| if start >= from { delta } else { 0 };
    let mut commands = vec![plain];
    if edge == TrimEdge::Left {
        commands.push(Command::MoveClip {
            id,
            track: own.id(),
            start: clip.range.start,
        });
    }
    let skip = BTreeSet::new();
    for track in project.tracks() {
        if track.is_locked() && track.id() != own.id() {
            continue;
        }
        let fixed = (track.id() == own.id()).then_some((id, settled));
        commands.extend(slide_track(track, &skip, fixed, &later)?);
    }
    batch(commands)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::tests::project_with;
    use animathio_core::{History, TrackId};

    fn set(ids: &[u64]) -> BTreeSet<ClipId> {
        ids.iter().copied().map(ClipId).collect()
    }

    fn starts(project: &Project) -> Vec<(u64, i64, i64)> {
        let mut out: Vec<_> = project
            .tracks()
            .iter()
            .flat_map(|track| {
                track
                    .clips()
                    .values()
                    .map(|clip| (clip.id.0, clip.range.start.0, clip.range.end.0))
            })
            .collect();
        out.sort_unstable();
        out
    }

    fn lock(project: &mut Project, id: u64) {
        Command::SetTrackLocked {
            id: TrackId(id),
            locked: true,
        }
        .apply(project)
        .unwrap();
    }

    fn run(project: &mut Project, command: &Command) -> Project {
        let before = project.clone();
        let mut history = History::default();
        history.execute(project, command).unwrap();
        let after = project.clone();
        history.undo(project).unwrap();
        assert_eq!(*project, before, "one undo step restores the project");
        *project = after.clone();
        after
    }

    #[test]
    fn scope_toggles_and_defaults_to_this_track() {
        assert_eq!(RippleScope::default(), RippleScope::ThisTrack);
        assert_eq!(RippleScope::ThisTrack.toggled(), RippleScope::AllTracks);
        assert_eq!(RippleScope::AllTracks.toggled(), RippleScope::ThisTrack);
        assert_ne!(
            RippleScope::ThisTrack.label(),
            RippleScope::AllTracks.label()
        );
    }

    #[test]
    fn this_track_delete_only_pulls_the_clips_own_track() {
        let mut project = project_with(&[
            (1, &[(1, 0, 10), (2, 10, 20), (3, 20, 30)][..]),
            (2, &[(4, 10, 20), (5, 25, 35)][..]),
        ]);
        let command = delete_command(&project, &set(&[2]), RippleScope::ThisTrack).unwrap();
        let after = run(&mut project, &command);
        assert_eq!(
            starts(&after),
            [(1, 0, 10), (3, 10, 20), (4, 10, 20), (5, 25, 35)]
        );
    }

    #[test]
    fn linked_delete_pulls_every_unlocked_track_and_respects_locks() {
        let mut project = project_with(&[
            (1, &[(1, 0, 10), (2, 10, 20), (3, 20, 30)][..]),
            (2, &[(4, 25, 35)][..]),
            (3, &[(5, 30, 40)][..]),
        ]);
        lock(&mut project, 3);
        let command = delete_command(&project, &set(&[2]), RippleScope::AllTracks).unwrap();
        let after = run(&mut project, &command);
        assert_eq!(
            starts(&after),
            [(1, 0, 10), (3, 10, 20), (4, 15, 25), (5, 30, 40)],
            "track 3 is locked and stays put"
        );
    }

    #[test]
    fn linked_delete_holds_back_clips_that_would_collide_with_a_straddler() {
        // Track 2's clip straddles the deleted span, so its later neighbour can only
        // close up to the straddler instead of overlapping it.
        let mut project =
            project_with(&[(1, &[(1, 10, 20)][..]), (2, &[(2, 5, 25), (3, 30, 40)][..])]);
        let command = delete_command(&project, &set(&[1]), RippleScope::AllTracks).unwrap();
        let after = run(&mut project, &command);
        assert_eq!(starts(&after), [(2, 5, 25), (3, 25, 35)]);
    }

    #[test]
    fn multi_delete_merges_gaps_and_pulls_by_the_total() {
        let mut project = project_with(&[
            (1, &[(1, 0, 10), (2, 10, 20), (3, 20, 30), (4, 30, 40)][..]),
            (2, &[(5, 40, 50)][..]),
        ]);
        let command = delete_command(&project, &set(&[2, 3]), RippleScope::AllTracks).unwrap();
        let after = run(&mut project, &command);
        assert_eq!(starts(&after), [(1, 0, 10), (4, 10, 20), (5, 20, 30)]);
        // This-track mode with deletes on two tracks pulls each track independently.
        let command = delete_command(&project, &set(&[1, 5]), RippleScope::ThisTrack).unwrap();
        let after = run(&mut project, &command);
        assert_eq!(starts(&after), [(4, 0, 10)]);
    }

    #[test]
    fn deleting_on_a_locked_track_is_refused() {
        let mut project = project_with(&[(1, &[(1, 0, 10), (2, 10, 20)][..])]);
        lock(&mut project, 1);
        let command = delete_command(&project, &set(&[1]), RippleScope::ThisTrack).unwrap();
        assert!(History::default().execute(&mut project, &command).is_err());
        assert!(delete_command(&project, &set(&[77]), RippleScope::ThisTrack).is_none());
    }

    #[test]
    fn linked_trim_shifts_other_tracks_for_both_edges() {
        let base = project_with(&[
            (1, &[(1, 0, 10), (2, 10, 20)][..]),
            (2, &[(3, 10, 20), (4, 20, 30)][..]),
            (3, &[(5, 12, 18)][..]),
        ]);
        // Shorten clip 1 from the right: everything starting at or after 10 slides.
        let mut project = base.clone();
        let command = trim_command(
            &project,
            ClipId(1),
            TrimEdge::Right,
            Time(6),
            RippleScope::AllTracks,
        )
        .unwrap();
        let after = run(&mut project, &command);
        assert_eq!(
            starts(&after),
            [(1, 0, 6), (2, 6, 16), (3, 6, 16), (4, 16, 26), (5, 8, 14)]
        );
        // Lengthen it instead.
        let mut project = base.clone();
        let command = trim_command(
            &project,
            ClipId(1),
            TrimEdge::Right,
            Time(14),
            RippleScope::AllTracks,
        )
        .unwrap();
        let after = run(&mut project, &command);
        assert_eq!(after.tracks()[1].clips()[&ClipId(3)].range.start, Time(14));
        // Left trim collapses the head and pulls the rest.
        let mut project = base.clone();
        let command = trim_command(
            &project,
            ClipId(1),
            TrimEdge::Left,
            Time(4),
            RippleScope::AllTracks,
        )
        .unwrap();
        let after = run(&mut project, &command);
        assert_eq!(
            starts(&after),
            [(1, 0, 6), (2, 6, 16), (3, 6, 16), (4, 16, 26), (5, 8, 14)]
        );
        // The single-track scope is the existing behaviour.
        let command = trim_command(
            &base,
            ClipId(1),
            TrimEdge::Right,
            Time(6),
            RippleScope::ThisTrack,
        )
        .unwrap();
        assert_eq!(
            Some(command),
            editing::trim_command(&base, ClipId(1), TrimEdge::Right, Time(6), true)
        );
    }

    #[test]
    fn linked_trim_leaves_locked_tracks_alone() {
        let mut project = project_with(&[(1, &[(1, 0, 10)][..]), (2, &[(2, 10, 20)][..])]);
        lock(&mut project, 2);
        let command = trim_command(
            &project,
            ClipId(1),
            TrimEdge::Right,
            Time(5),
            RippleScope::AllTracks,
        )
        .unwrap();
        let after = run(&mut project, &command);
        assert_eq!(starts(&after), [(1, 0, 5), (2, 10, 20)]);
    }
}
