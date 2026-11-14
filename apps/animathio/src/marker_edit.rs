//! Pure planning for marker selection edits: delete, nudge, move together and
//! copy/paste. Every plan is one `Command` (a `Batch` for several markers), so each is a
//! single undo step.

use std::collections::BTreeSet;

use animathio_core::{Command, Marker, MarkerId, Project, Time};

use crate::selection::batch;

/// Markers that still exist in `project`.
pub(crate) fn existing(project: &Project, ids: &BTreeSet<MarkerId>) -> BTreeSet<MarkerId> {
    project
        .markers()
        .iter()
        .map(|m| m.id)
        .filter(|id| ids.contains(id))
        .collect()
}

/// Remove every selected marker.
pub(crate) fn delete_command(project: &Project, ids: &BTreeSet<MarkerId>) -> Option<Command> {
    batch(
        existing(project, ids)
            .into_iter()
            .map(|id| Command::RemoveMarker { id })
            .collect(),
    )
}

/// Move every selected marker by `delta` ticks. The shift is limited so none goes before
/// time zero; the group keeps its spacing.
pub(crate) fn move_command(
    project: &Project,
    ids: &BTreeSet<MarkerId>,
    delta: i64,
) -> Option<Command> {
    let members: Vec<&Marker> = project
        .markers()
        .iter()
        .filter(|m| ids.contains(&m.id))
        .collect();
    let earliest = members.iter().map(|m| m.time.0).min()?;
    let delta = delta.max(-earliest);
    if delta == 0 {
        return None;
    }
    batch(
        members
            .iter()
            .map(|m| Command::MoveMarker {
                id: m.id,
                time: Time(m.time.0.saturating_add(delta)),
            })
            .collect(),
    )
}

/// Copied markers, with times relative to the earliest one.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MarkerBoard {
    markers: Vec<Marker>,
}

impl MarkerBoard {
    pub(crate) fn copy(project: &Project, ids: &BTreeSet<MarkerId>) -> Option<Self> {
        let picked: Vec<&Marker> = project
            .markers()
            .iter()
            .filter(|m| ids.contains(&m.id))
            .collect();
        let origin = picked.iter().map(|m| m.time.0).min()?;
        Some(Self {
            markers: picked
                .into_iter()
                .map(|m| Marker {
                    time: Time(m.time.0 - origin),
                    ..m.clone()
                })
                .collect(),
        })
    }

    /// Add the copies with the earliest at `at`. Positions that already hold a marker are
    /// skipped so pasting twice at the same spot never stacks markers. Returns the command and
    /// the new ids.
    pub(crate) fn paste(&self, project: &Project, at: Time) -> Option<(Command, Vec<MarkerId>)> {
        let taken: BTreeSet<Time> = project.markers().iter().map(|m| m.time).collect();
        let mut next = project.next_marker_id().0;
        let mut commands = Vec::new();
        let mut created = Vec::new();
        for source in &self.markers {
            let time = Time(at.0.checked_add(source.time.0)?);
            if taken.contains(&time) {
                continue;
            }
            let id = MarkerId(next);
            next += 1;
            created.push(id);
            commands.push(Command::AddMarker {
                marker: Marker {
                    id,
                    time,
                    ..source.clone()
                },
            });
        }
        Some((batch(commands)?, created))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use animathio_core::History;

    fn project(times: &[i64]) -> Project {
        let mut project = Project::default();
        for (i, t) in times.iter().enumerate() {
            let mut marker = Marker::new(MarkerId(i as u64 + 1), Time(*t));
            marker.name = format!("m{}", i + 1);
            Command::AddMarker { marker }.apply(&mut project).unwrap();
        }
        project
    }
    fn ids(list: &[u64]) -> BTreeSet<MarkerId> {
        list.iter().copied().map(MarkerId).collect()
    }
    fn times(project: &Project) -> Vec<i64> {
        project.markers().iter().map(|m| m.time.0).collect()
    }

    #[test]
    fn delete_removes_only_the_selection_in_one_undo_step() {
        let mut project = project(&[10, 20, 30]);
        let before = project.clone();
        let command = delete_command(&project, &ids(&[1, 3, 99])).unwrap();
        let mut history = History::default();
        history.execute(&mut project, &command).unwrap();
        assert_eq!(times(&project), [20]);
        assert!(history.undo(&mut project).unwrap());
        assert_eq!(project, before);
        assert!(delete_command(&project, &ids(&[99])).is_none());
    }

    #[test]
    fn nudge_moves_the_group_and_stops_at_zero() {
        let mut project = project(&[10, 25, 40]);
        let command = move_command(&project, &ids(&[1, 2]), 5).unwrap();
        command.apply(&mut project).unwrap();
        assert_eq!(times(&project), [15, 30, 40]);
        // Moving left cannot cross zero and keeps the spacing.
        let command = move_command(&project, &ids(&[1, 2]), -100).unwrap();
        command.apply(&mut project).unwrap();
        assert_eq!(times(&project), [0, 15, 40]);
        assert!(move_command(&project, &ids(&[1]), -3).is_none());
        assert!(move_command(&project, &ids(&[]), 3).is_none());
    }

    #[test]
    fn paste_lands_at_the_playhead_keeping_spacing_and_skips_occupied_times() {
        let mut project = project(&[100, 140, 500]);
        let board = MarkerBoard::copy(&project, &ids(&[1, 2])).unwrap();
        let before = project.clone();
        let (command, created) = board.paste(&project, Time(300)).unwrap();
        let mut history = History::default();
        history.execute(&mut project, &command).unwrap();
        assert_eq!(times(&project), [100, 140, 300, 340, 500]);
        assert_eq!(created.len(), 2);
        let copied = project.marker(created[0]).unwrap();
        assert_eq!(copied.name, "m1", "name, colour and note are kept");
        assert!(history.undo(&mut project).unwrap());
        assert_eq!(project, before);
        // Pasting onto markers that are already there adds nothing; a partial overlap skips
        // just the colliding copy.
        assert!(board.paste(&project, Time(100)).is_none());
        let (command, created) = board.paste(&project, Time(60)).unwrap();
        command.apply(&mut project).unwrap();
        assert_eq!(created.len(), 1, "the copy at 100 collides and is skipped");
        // A paste that adds nothing is no command at all.
        assert!(board.paste(&project, Time(60)).is_none());
        assert!(MarkerBoard::copy(&project, &ids(&[])).is_none());
    }
}
