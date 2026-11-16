//! Track selection for newly added visual elements (math, graphs, text, shapes).
//!
//! Elements are overlays: adding one where media already plays must never fail or force the
//! user to move the playhead. The element goes to the first free track layered above
//! everything it overlaps, and a new track is created when none is free. Placement is
//! returned as commands so callers apply it as one undoable `Command::Batch`.

use animathio_core::{Clip, ClipKind, Command, Project, TimeRange, Track, TrackId};

/// The track a new visual clip over `range` lands on, and whether it must be created.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// An existing track that is free over the range.
    Existing(TrackId),
    /// A new track appended above every existing layer.
    New(TrackId),
}

impl Slot {
    /// The track id the clip is added to.
    #[must_use]
    pub fn track(self) -> TrackId {
        match self {
            Self::Existing(id) | Self::New(id) => id,
        }
    }
}

fn free_over(track: &Track, range: TimeRange) -> bool {
    !track
        .clips()
        .values()
        .any(|clip| clip.range.start < range.end && range.start < clip.range.end)
}

fn takes_visuals(track: &Track) -> bool {
    !track.is_locked()
        && !track
            .clips()
            .values()
            .any(|clip| matches!(clip.kind, ClipKind::Audio(_)))
}

fn next_track_id(project: &Project) -> TrackId {
    TrackId(
        project
            .tracks()
            .iter()
            .map(|track| track.id().0)
            .max()
            .map_or(1, |max| max.saturating_add(1)),
    )
}

/// Choose where a new visual clip over `range` goes.
///
/// `preferred` (the row a clip was dropped on) wins when it is unlocked and free over the
/// range. Otherwise the clip goes to the first free, unlocked, audio-free track above every
/// track that has a clip overlapping the range (so it is never hidden behind the media it
/// overlays), or onto a new top track.
#[must_use]
pub fn visual_slot(project: &Project, range: TimeRange, preferred: Option<TrackId>) -> Slot {
    let tracks = project.tracks();
    if let Some(track) = preferred.and_then(|id| tracks.iter().find(|t| t.id() == id))
        && !track.is_locked()
        && free_over(track, range)
    {
        return Slot::Existing(track.id());
    }
    let floor = tracks.iter().rposition(|track| !free_over(track, range));
    let start = floor.map_or(0, |index| index + 1);
    tracks
        .iter()
        .skip(start)
        .find(|track| takes_visuals(track) && free_over(track, range))
        .map_or_else(
            || Slot::New(next_track_id(project)),
            |track| Slot::Existing(track.id()),
        )
}

/// Commands that add `clip` on the slot chosen by [`visual_slot`]: the clip alone on a free
/// track, or a new track named `label` plus the clip. The clip's `range` is the placement
/// range. Wrap them in one `Command::Batch` together with any follow-up edits so undo is a
/// single step. Returns the chosen track.
#[must_use]
pub fn place_visual_clip(
    project: &Project,
    clip: Clip,
    label: &str,
    preferred: Option<TrackId>,
) -> (Vec<Command>, TrackId) {
    let slot = visual_slot(project, clip.range, preferred);
    let mut commands = Vec::new();
    if let Slot::New(id) = slot {
        commands.push(Command::AddTrack {
            track: Track::new(id, label),
            index: project.tracks().len(),
        });
    }
    let track = slot.track();
    commands.push(Command::AddClip { track, clip });
    (commands, track)
}

#[cfg(test)]
mod tests {
    use super::*;
    use animathio_core::{ClipId, FrameRate, Time};

    fn range(start: i64, end: i64) -> TimeRange {
        TimeRange::new(Time(start), Time(end)).unwrap()
    }

    fn project_with(tracks: &[&[(u64, i64, i64)]]) -> Project {
        let mut project = Project::new(FrameRate::new(30, 1).unwrap());
        for (index, clips) in tracks.iter().enumerate() {
            let id = TrackId(index as u64 + 1);
            Command::AddTrack {
                track: Track::new(id, "t"),
                index,
            }
            .apply(&mut project)
            .unwrap();
            for (clip, start, end) in *clips {
                Command::AddClip {
                    track: id,
                    clip: Clip::new(
                        ClipId(*clip),
                        ClipKind::Plugin {
                            plugin_id: "p".into(),
                            params: serde_json::Value::Null,
                        },
                        range(*start, *end),
                    ),
                }
                .apply(&mut project)
                .unwrap();
            }
        }
        project
    }

    #[test]
    fn empty_project_creates_a_track() {
        let project = Project::new(FrameRate::new(30, 1).unwrap());
        assert_eq!(
            visual_slot(&project, range(0, 10), None),
            Slot::New(TrackId(1))
        );
    }

    #[test]
    fn free_first_track_is_reused() {
        let project = project_with(&[&[(10, 0, 5)]]);
        assert_eq!(
            visual_slot(&project, range(5, 9), None),
            Slot::Existing(TrackId(1))
        );
    }

    #[test]
    fn occupied_range_goes_above_or_to_a_new_track() {
        let project = project_with(&[&[(10, 0, 50)]]);
        assert_eq!(
            visual_slot(&project, range(10, 20), None),
            Slot::New(TrackId(2))
        );
        let project = project_with(&[&[(10, 0, 50)], &[]]);
        assert_eq!(
            visual_slot(&project, range(10, 20), None),
            Slot::Existing(TrackId(2))
        );
    }

    #[test]
    fn never_lands_behind_an_overlapped_track() {
        // Track 1 is empty here but track 2 is occupied: track 1 is beneath it.
        let project = project_with(&[&[], &[(10, 0, 50)]]);
        assert_eq!(
            visual_slot(&project, range(10, 20), None),
            Slot::New(TrackId(3))
        );
    }

    #[test]
    fn preferred_track_is_honoured_only_when_free() {
        let project = project_with(&[&[(10, 0, 50)], &[(11, 60, 70)], &[]]);
        assert_eq!(
            visual_slot(&project, range(10, 20), Some(TrackId(2))),
            Slot::Existing(TrackId(2))
        );
        assert_eq!(
            visual_slot(&project, range(10, 20), Some(TrackId(1))),
            Slot::Existing(TrackId(2))
        );
    }

    #[test]
    fn placement_batch_applies_and_undoes_as_one_step() {
        let mut project = project_with(&[&[(10, 0, 50)]]);
        let mut history = animathio_core::History::default();
        let clip = Clip::new(
            ClipId(11),
            ClipKind::Plugin {
                plugin_id: "p".into(),
                params: serde_json::Value::Null,
            },
            range(10, 20),
        );
        let (commands, track) = place_visual_clip(&project, clip, "Math", None);
        assert_eq!(track, TrackId(2));
        history
            .execute(&mut project, &Command::Batch { commands })
            .unwrap();
        assert_eq!(project.tracks().len(), 2);
        history.undo(&mut project).unwrap();
        assert_eq!(project.tracks().len(), 1);
    }
}
