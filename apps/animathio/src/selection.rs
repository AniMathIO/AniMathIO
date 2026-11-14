//! Pure multi-selection logic: range and marquee selection and the grouped
//! move, nudge and delete edits. Everything here is window-free and testable;
//! the editor only feeds it pointer geometry and applies the returned commands.

use std::collections::BTreeSet;

use animathio_core::{ClipId, Command, Project, Time, Track, TrackId};

use crate::timeline::{self, TimelineLayout};

/// Wrap `commands` in one undoable edit, unwrapping a single command.
pub(crate) fn batch(mut commands: Vec<Command>) -> Option<Command> {
    match commands.len() {
        0 => None,
        1 => commands.pop(),
        _ => Some(Command::Batch { commands }),
    }
}

/// The ids that still exist in `project`.
pub(crate) fn existing(project: &Project, ids: &BTreeSet<ClipId>) -> BTreeSet<ClipId> {
    project
        .tracks()
        .iter()
        .flat_map(|track| track.clips().keys().copied())
        .filter(|id| ids.contains(id))
        .collect()
}

/// `ids` plus every linked companion of each of them.
pub(crate) fn with_companions(project: &Project, ids: &BTreeSet<ClipId>) -> BTreeSet<ClipId> {
    let groups: BTreeSet<ClipId> = project
        .tracks()
        .iter()
        .flat_map(|track| track.clips().values())
        .filter(|clip| ids.contains(&clip.id))
        .filter_map(|clip| clip.link_group)
        .collect();
    project
        .tracks()
        .iter()
        .flat_map(|track| track.clips().values())
        .filter(|clip| {
            ids.contains(&clip.id) || clip.link_group.is_some_and(|g| groups.contains(&g))
        })
        .map(|clip| clip.id)
        .collect()
}

/// Keep one clip per link group (the first in `ids` order); core propagates a
/// split/trim/move to the companions, so listing both members would apply it twice.
pub(crate) fn one_per_link_group(project: &Project, ids: &[ClipId]) -> Vec<ClipId> {
    let mut seen = BTreeSet::new();
    ids.iter()
        .copied()
        .filter(|id| {
            let group = track_of(project, *id)
                .and_then(|track| track.clips().get(id))
                .and_then(|clip| clip.link_group);
            group.is_none_or(|group| seen.insert(group))
        })
        .collect()
}

/// Every clip of the project.
pub(crate) fn all_clips(project: &Project) -> BTreeSet<ClipId> {
    project
        .tracks()
        .iter()
        .flat_map(|track| track.clips().keys().copied())
        .collect()
}

/// The track that holds `id`.
pub(crate) fn track_of(project: &Project, id: ClipId) -> Option<&Track> {
    project
        .tracks()
        .iter()
        .find(|track| track.clips().contains_key(&id))
}

/// Shift-click: add every clip of `anchor`'s track that lies between `anchor` and
/// `target` (inclusive) to `base`. Across tracks only `target` is added.
pub(crate) fn range_select(
    project: &Project,
    anchor: ClipId,
    target: ClipId,
    base: &BTreeSet<ClipId>,
) -> BTreeSet<ClipId> {
    let mut selection = base.clone();
    selection.insert(anchor);
    selection.insert(target);
    let (Some(a_track), Some(t_track)) = (track_of(project, anchor), track_of(project, target))
    else {
        return selection;
    };
    if a_track.id() != t_track.id() {
        return selection;
    }
    let (a, t) = (&a_track.clips()[&anchor], &a_track.clips()[&target]);
    let start = a.range.start.min(t.range.start);
    let end = a.range.end.max(t.range.end);
    selection.extend(
        a_track
            .clips()
            .values()
            .filter(|clip| clip.range.start >= start && clip.range.end <= end)
            .map(|clip| clip.id),
    );
    selection
}

/// Ctrl/Cmd-click: toggle `id` in `base`.
pub(crate) fn toggle(base: &BTreeSet<ClipId>, id: ClipId) -> BTreeSet<ClipId> {
    let mut selection = base.clone();
    if !selection.remove(&id) {
        selection.insert(id);
    }
    selection
}

/// Clips touched by a rubber band between two canvas-relative points (any corner
/// order). A clip is hit when its block overlaps the rectangle.
pub(crate) fn marquee(
    layout: &TimelineLayout,
    project: &Project,
    a: (f32, f32),
    b: (f32, f32),
) -> BTreeSet<ClipId> {
    let (left, right) = (a.0.min(b.0), a.0.max(b.0));
    let (top, bottom) = (a.1.min(b.1), a.1.max(b.1));
    let row_height = layout.row_height.max(1.0);
    let mut hits = BTreeSet::new();
    for (row, track) in project.tracks().iter().enumerate() {
        let row_top = layout.ruler_height + row as f32 * row_height;
        if row_top + row_height <= top || row_top >= bottom {
            continue;
        }
        for clip in track.clips().values() {
            let x0 = timeline::time_to_x(layout, clip.range.start);
            let x1 = timeline::time_to_x(layout, clip.range.end);
            if x1 >= left && x0 <= right {
                hits.insert(clip.id);
            }
        }
    }
    hits
}

/// Rows (track indices) spanned by `ids`: `(first, last)`.
pub(crate) fn row_span(project: &Project, ids: &BTreeSet<ClipId>) -> Option<(usize, usize)> {
    let mut rows = project
        .tracks()
        .iter()
        .enumerate()
        .filter(|(_, track)| track.clips().keys().any(|id| ids.contains(id)))
        .map(|(row, _)| row);
    let first = rows.clone().next()?;
    let last = rows.next_back()?;
    Some((first, last))
}

/// Earliest start among `ids`.
pub(crate) fn earliest_start(project: &Project, ids: &BTreeSet<ClipId>) -> Option<Time> {
    project
        .tracks()
        .iter()
        .flat_map(|track| track.clips().values())
        .filter(|clip| ids.contains(&clip.id))
        .map(|clip| clip.range.start)
        .min()
}

/// Move `ids` together by `delta` ticks and `row_delta` tracks as one batch. The
/// time delta is clamped so nothing starts before zero and the row delta so no
/// clip leaves the track stack.
pub(crate) fn move_command(
    project: &Project,
    ids: &BTreeSet<ClipId>,
    delta: i64,
    row_delta: isize,
) -> Option<Command> {
    let first = earliest_start(project, ids)?.0;
    let (first_row, last_row) = row_span(project, ids)?;
    let delta = delta.max(first.checked_neg()?);
    let row_delta = row_delta
        .max(-isize::try_from(first_row).ok()?)
        .min(isize::try_from(project.tracks().len() - 1 - last_row).ok()?);
    if delta == 0 && row_delta == 0 {
        return None;
    }
    let mut commands = Vec::new();
    for (row, track) in project.tracks().iter().enumerate() {
        let mut members = track
            .clips()
            .values()
            .filter(|clip| ids.contains(&clip.id))
            .peekable();
        if members.peek().is_none() {
            continue;
        }
        let destination = project.tracks()[row.checked_add_signed(row_delta)?].id();
        for clip in members {
            commands.push(Command::MoveClip {
                id: clip.id,
                track: destination,
                start: Time(clip.range.start.0.checked_add(delta)?),
            });
        }
    }
    batch(commands)
}

/// Remove `ids` as one batch.
pub(crate) fn delete_command(project: &Project, ids: &BTreeSet<ClipId>) -> Option<Command> {
    batch(
        existing(project, ids)
            .into_iter()
            .map(|id| Command::RemoveClip { id })
            .collect(),
    )
}

/// What the inspector shows for a multi-selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Summary {
    pub(crate) count: usize,
    pub(crate) tracks: usize,
    pub(crate) start: Time,
    pub(crate) end: Time,
}

/// Count, spanned tracks and overall time range of `ids`.
pub(crate) fn summarize(project: &Project, ids: &BTreeSet<ClipId>) -> Option<Summary> {
    let mut summary: Option<Summary> = None;
    for track in project.tracks() {
        let mut touched = false;
        for clip in track.clips().values().filter(|clip| ids.contains(&clip.id)) {
            touched = true;
            let entry = summary.get_or_insert(Summary {
                count: 0,
                tracks: 0,
                start: clip.range.start,
                end: clip.range.end,
            });
            entry.count += 1;
            entry.start = entry.start.min(clip.range.start);
            entry.end = entry.end.max(clip.range.end);
        }
        if touched && let Some(entry) = &mut summary {
            entry.tracks += 1;
        }
    }
    summary
}

/// How many rows a dragged group moves when the pointer is over `pointer_row`:
/// the pointer's row offset from where the drag began, clamped so the group's
/// first and last rows (`span`) stay inside the `rows`-track stack.
pub(crate) fn row_delta(
    origin_row: usize,
    pointer_row: usize,
    rows: usize,
    span: (usize, usize),
) -> isize {
    let signed = |value: usize| isize::try_from(value).unwrap_or(isize::MAX);
    let lowest = -signed(span.0);
    let highest = signed(rows.saturating_sub(1 + span.1)).max(lowest);
    (signed(pointer_row) - signed(origin_row)).clamp(lowest, highest)
}

/// The destination track of a clip moved by `row_delta`, for painting the drag.
pub(crate) fn shifted_row(row: usize, row_delta: isize, rows: usize) -> usize {
    row.saturating_add_signed(row_delta)
        .min(rows.saturating_sub(1))
}

/// The track a row index resolves to, if any.
pub(crate) fn track_at(project: &Project, row: usize) -> Option<TrackId> {
    project.tracks().get(row).map(Track::id)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use animathio_core::{Clip, ClipKind, FrameRate, History, TimeRange};

    /// A track id with `(clip id, start, end)` clips.
    pub(crate) type TrackSpec<'a> = (u64, &'a [(u64, i64, i64)]);

    pub(crate) fn project_with(layout: &[TrackSpec<'_>]) -> Project {
        let mut project = Project::new(FrameRate::new(30, 1).unwrap());
        for (index, (track, clips)) in layout.iter().enumerate() {
            Command::AddTrack {
                track: Track::new(TrackId(*track), format!("T{track}")),
                index,
            }
            .apply(&mut project)
            .unwrap();
            for (id, start, end) in *clips {
                Command::AddClip {
                    track: TrackId(*track),
                    clip: Clip::new(
                        ClipId(*id),
                        ClipKind::Plugin {
                            plugin_id: "test".into(),
                            params: serde_json::json!({}),
                        },
                        TimeRange::new(Time(*start), Time(*end)).unwrap(),
                    ),
                }
                .apply(&mut project)
                .unwrap();
            }
        }
        project
    }

    fn set(ids: &[u64]) -> BTreeSet<ClipId> {
        ids.iter().copied().map(ClipId).collect()
    }

    /// Link `ids` into one group named after the first.
    pub(crate) fn link(project: &mut Project, ids: &[u64]) {
        for id in ids {
            Command::SetClipLink {
                id: ClipId(*id),
                group: Some(ClipId(ids[0])),
            }
            .apply(project)
            .unwrap();
        }
    }

    #[test]
    fn splitting_both_members_of_a_linked_pair_succeeds_once_per_group() {
        let mut project = project_with(&[(1, &[(1, 0, 100)][..]), (2, &[(2, 0, 100)][..])]);
        link(&mut project, &[1, 2]);
        let targets = one_per_link_group(&project, &[ClipId(1), ClipId(2)]);
        assert_eq!(targets, [ClipId(1)]);
        let batch = batch(
            targets
                .into_iter()
                .zip(10u64..)
                .map(|(id, right)| Command::SplitClip {
                    id,
                    right_id: ClipId(right),
                    at: Time(50),
                })
                .collect(),
        )
        .unwrap();
        let mut history = History::default();
        history.execute(&mut project, &batch).unwrap();
        assert_eq!(project.tracks()[0].clips().len(), 2);
        assert_eq!(project.tracks()[1].clips().len(), 2);
        // Without the dedupe the same selection is rejected.
        let mut project = project_with(&[(1, &[(1, 0, 100)][..]), (2, &[(2, 0, 100)][..])]);
        link(&mut project, &[1, 2]);
        let both = Command::Batch {
            commands: vec![
                Command::SplitClip {
                    id: ClipId(1),
                    right_id: ClipId(10),
                    at: Time(50),
                },
                Command::SplitClip {
                    id: ClipId(2),
                    right_id: ClipId(11),
                    at: Time(50),
                },
            ],
        };
        assert!(both.apply(&mut project).is_err());
    }

    #[test]
    fn with_companions_expands_link_groups() {
        let mut project =
            project_with(&[(1, &[(1, 0, 10)][..]), (2, &[(2, 0, 10), (3, 10, 20)][..])]);
        link(&mut project, &[1, 2]);
        assert_eq!(with_companions(&project, &set(&[1])), set(&[1, 2]));
        assert_eq!(with_companions(&project, &set(&[3])), set(&[3]));
    }

    fn starts(project: &Project) -> Vec<(u64, u64, i64)> {
        let mut out: Vec<_> = project
            .tracks()
            .iter()
            .flat_map(|track| {
                track
                    .clips()
                    .values()
                    .map(|clip| (clip.id.0, track.id().0, clip.range.start.0))
            })
            .collect();
        out.sort_unstable();
        out
    }

    #[test]
    fn toggle_adds_and_removes() {
        let base = set(&[1, 2]);
        assert_eq!(toggle(&base, ClipId(3)), set(&[1, 2, 3]));
        assert_eq!(toggle(&base, ClipId(2)), set(&[1]));
    }

    #[test]
    fn shift_click_selects_the_clips_between_on_one_track() {
        let project = project_with(&[
            (1, &[(1, 0, 10), (2, 10, 20), (3, 20, 30), (4, 30, 40)][..]),
            (2, &[(5, 0, 40)][..]),
        ]);
        assert_eq!(
            range_select(&project, ClipId(1), ClipId(3), &BTreeSet::new()),
            set(&[1, 2, 3])
        );
        // Order of anchor and target does not matter and existing picks are kept.
        assert_eq!(
            range_select(&project, ClipId(4), ClipId(2), &set(&[5])),
            set(&[2, 3, 4, 5])
        );
        // A different track only adds the clicked clip.
        assert_eq!(
            range_select(&project, ClipId(1), ClipId(5), &set(&[1])),
            set(&[1, 5])
        );
    }

    #[test]
    fn marquee_hits_overlapping_clips_in_any_corner_order() {
        let project = project_with(&[
            (1, &[(1, 0, 100), (2, 200, 300)][..]),
            (2, &[(3, 50, 250)][..]),
            (3, &[(4, 0, 300)][..]),
        ]);
        // 1000 px for 300 ticks is irrelevant here: use a 1 px per tick layout.
        let layout = TimelineLayout::zoomed(300.0, 20.0, 40.0, 300, 1.0, 0.0);
        // Rows: 20..60, 60..100, 100..140.
        assert_eq!(
            marquee(&layout, &project, (150.0, 30.0), (210.0, 80.0)),
            set(&[2, 3])
        );
        assert_eq!(
            marquee(&layout, &project, (110.0, 110.0), (120.0, 130.0)),
            set(&[4])
        );
        assert!(marquee(&layout, &project, (110.0, 30.0), (190.0, 50.0)).is_empty());
        assert!(marquee(&layout, &project, (0.0, 0.0), (300.0, 15.0)).is_empty());
    }

    #[test]
    fn group_move_is_one_undo_step_and_keeps_relative_positions() {
        let mut project = project_with(&[
            (1, &[(1, 10, 20), (2, 30, 40)][..]),
            (2, &[(3, 12, 18)][..]),
        ]);
        let original = project.clone();
        let ids = set(&[1, 2, 3]);
        let command = move_command(&project, &ids, 5, 0).unwrap();
        let mut history = History::default();
        history.execute(&mut project, &command).unwrap();
        assert_eq!(starts(&project), [(1, 1, 15), (2, 1, 35), (3, 2, 17)]);
        history.undo(&mut project).unwrap();
        assert_eq!(project, original);
        // Never before zero: the whole group stops together.
        let command = move_command(&project, &ids, -100, 0).unwrap();
        history.execute(&mut project, &command).unwrap();
        assert_eq!(starts(&project), [(1, 1, 0), (2, 1, 20), (3, 2, 2)]);
        history.undo(&mut project).unwrap();
        // Moving down rows keeps the stack in bounds for the lowest clip.
        let mut roomy = project_with(&[
            (1, &[(1, 10, 20), (2, 30, 40)][..]),
            (2, &[(3, 50, 60)][..]),
        ]);
        let roomy_original = roomy.clone();
        let command = move_command(&roomy, &set(&[1, 2]), 0, 5).unwrap();
        history.execute(&mut roomy, &command).unwrap();
        assert_eq!(starts(&roomy), [(1, 2, 10), (2, 2, 30), (3, 2, 50)]);
        assert!(history.undo(&mut roomy).unwrap());
        assert_eq!(roomy, roomy_original);
        assert!(move_command(&project, &ids, 0, 0).is_none());
        assert!(move_command(&project, &BTreeSet::new(), 5, 0).is_none());
    }

    #[test]
    fn group_move_onto_an_occupied_slot_is_rejected_atomically() {
        let mut project = project_with(&[(1, &[(1, 0, 10), (2, 20, 30)][..])]);
        let original = project.clone();
        // Moving only clip 1 onto clip 2 overlaps.
        let command = move_command(&project, &set(&[1]), 15, 0).unwrap();
        assert!(History::default().execute(&mut project, &command).is_err());
        assert_eq!(project, original);
        // Moving both together is fine even though they swap places transiently.
        let command = move_command(&project, &set(&[1, 2]), 15, 0).unwrap();
        History::default().execute(&mut project, &command).unwrap();
        assert_eq!(starts(&project), [(1, 1, 15), (2, 1, 35)]);
    }

    #[test]
    fn delete_is_one_batch_and_ignores_missing_ids() {
        let mut project =
            project_with(&[(1, &[(1, 0, 10), (2, 10, 20)][..]), (2, &[(3, 0, 5)][..])]);
        let original = project.clone();
        let command = delete_command(&project, &set(&[1, 3, 99])).unwrap();
        let mut history = History::default();
        history.execute(&mut project, &command).unwrap();
        assert_eq!(all_clips(&project), set(&[2]));
        history.undo(&mut project).unwrap();
        assert_eq!(project, original);
        assert!(delete_command(&project, &set(&[99])).is_none());
        let single = delete_command(&project, &set(&[2])).unwrap();
        assert_eq!(single, Command::RemoveClip { id: ClipId(2) });
    }

    #[test]
    fn summary_counts_clips_tracks_and_span() {
        let project = project_with(&[
            (1, &[(1, 5, 10), (2, 10, 20)][..]),
            (2, &[(3, 0, 8)][..]),
            (3, &[][..]),
        ]);
        assert_eq!(
            summarize(&project, &set(&[1, 3, 99])),
            Some(Summary {
                count: 2,
                tracks: 2,
                start: Time(0),
                end: Time(10)
            })
        );
        assert_eq!(summarize(&project, &set(&[99])), None);
    }

    #[test]
    fn dragged_rows_stay_inside_the_stack() {
        // Group occupies rows 1..=2 of 5, grabbed on row 1.
        assert_eq!(row_delta(1, 1, 5, (1, 2)), 0);
        assert_eq!(row_delta(1, 4, 5, (1, 2)), 2);
        assert_eq!(row_delta(1, 0, 5, (1, 2)), -1);
        assert_eq!(row_delta(2, 0, 5, (1, 2)), -1);
        assert_eq!(row_delta(0, 3, 4, (0, 0)), 3);
    }

    #[test]
    fn row_helpers_clamp() {
        assert_eq!(shifted_row(2, -5, 4), 0);
        assert_eq!(shifted_row(2, 5, 4), 3);
        assert_eq!(shifted_row(1, 1, 4), 2);
    }
}
