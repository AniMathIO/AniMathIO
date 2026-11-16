//! Timeline clipboard: serialized clips with their source tracks, pasted at the
//! playhead with fresh ids. The same JSON travels through the system clipboard so
//! a copy can also be pasted into another project window.

use std::collections::{BTreeMap, BTreeSet};

use animathio_core::{Clip, ClipId, Command, Project, Time, TimeRange, Track, TrackId};
use serde::{Deserialize, Serialize};

use crate::selection::{batch, existing, with_companions};

/// Format tag so unrelated clipboard text is never mistaken for clips.
const FORMAT: &str = "animathio.clips/1";

/// One copied clip and the track it came from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct CopiedClip {
    pub(crate) track: TrackId,
    pub(crate) track_name: String,
    pub(crate) clip: Clip,
}

/// A set of copied clips, in track then start order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Clipboard {
    format: String,
    pub(crate) clips: Vec<CopiedClip>,
}

impl Clipboard {
    /// Copy `ids`; `None` when none of them exist.
    pub(crate) fn copy(project: &Project, ids: &BTreeSet<ClipId>) -> Option<Self> {
        let ids = existing(project, ids);
        let mut clips = Vec::new();
        for track in project.tracks() {
            let mut in_track: Vec<_> = track
                .clips()
                .values()
                .filter(|clip| ids.contains(&clip.id))
                .collect();
            in_track.sort_by_key(|clip| clip.range.start);
            clips.extend(in_track.into_iter().map(|clip| CopiedClip {
                track: track.id(),
                track_name: track.name().to_owned(),
                clip: clip.clone(),
            }));
        }
        (!clips.is_empty()).then(|| Self {
            format: FORMAT.to_owned(),
            clips,
        })
    }

    /// Copy `ids` together with their linked companions, as the timeline edits
    /// them (moving or deleting a linked clip takes its companions along), so
    /// copying a video with linked audio pastes both, still linked.
    pub(crate) fn copy_linked(project: &Project, ids: &BTreeSet<ClipId>) -> Option<Self> {
        Self::copy(project, &with_companions(project, &existing(project, ids)))
    }

    /// Start of the earliest copied clip.
    pub(crate) fn start(&self) -> Time {
        self.clips
            .iter()
            .map(|copied| copied.clip.range.start)
            .min()
            .unwrap_or(Time(0))
    }

    /// End of the latest copied clip.
    pub(crate) fn end(&self) -> Time {
        self.clips
            .iter()
            .map(|copied| copied.clip.range.end)
            .max()
            .unwrap_or(Time(0))
    }

    pub(crate) fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Parse clipboard text, rejecting anything that is not our format or that
    /// holds no clips.
    pub(crate) fn from_json(text: &str) -> Option<Self> {
        let parsed: Self = serde_json::from_str(text).ok()?;
        (parsed.format == FORMAT && !parsed.clips.is_empty()).then_some(parsed)
    }

    /// Plan pasting with the earliest clip landing on `at`. Each clip returns to its
    /// original track; a missing, locked or occupied track is replaced by a new one
    /// appended to the stack, so a paste never overwrites anything. Fresh ids come
    /// from `new_id`. Returns the command and the ids of the pasted clips.
    pub(crate) fn paste(
        &self,
        project: &Project,
        at: Time,
        mut new_id: impl FnMut() -> u64,
    ) -> Option<(Command, BTreeSet<ClipId>)> {
        let offset = at.0.checked_sub(self.start().0)?;
        let mut by_track: BTreeMap<TrackId, Vec<&CopiedClip>> = BTreeMap::new();
        for copied in &self.clips {
            by_track.entry(copied.track).or_default().push(copied);
        }
        // Pasted clips must never join the originals' link groups. Copied clips
        // that shared a group get a fresh group of their own; a lone member is
        // unlinked.
        let mut group_sizes: BTreeMap<ClipId, usize> = BTreeMap::new();
        for copied in &self.clips {
            if let Some(group) = copied.clip.link_group {
                *group_sizes.entry(group).or_default() += 1;
            }
        }
        let mut fresh_groups: BTreeMap<ClipId, ClipId> = BTreeMap::new();
        let mut commands = Vec::new();
        let mut pasted = BTreeSet::new();
        let mut appended = project.tracks().len();
        for (source, group) in by_track {
            let moved: Vec<(&CopiedClip, TimeRange)> = group
                .iter()
                .map(|copied| {
                    let start = copied.clip.range.start.0.checked_add(offset)?;
                    let end = copied.clip.range.end.0.checked_add(offset)?;
                    Some((*copied, TimeRange::new(Time(start), Time(end)).ok()?))
                })
                .collect::<Option<_>>()?;
            let target = project
                .tracks()
                .iter()
                .find(|track| track.id() == source)
                .filter(|track| !track.is_locked() && fits(track, &moved));
            let target_id = if let Some(track) = target {
                track.id()
            } else {
                let id = TrackId(new_id());
                let name = &group[0].track_name;
                commands.push(Command::AddTrack {
                    track: Track::new(id, name.clone()),
                    index: appended,
                });
                appended += 1;
                id
            };
            for (copied, range) in moved {
                let mut clip = copied.clip.clone();
                clip.id = ClipId(new_id());
                clip.range = range;
                clip.link_group = clip
                    .link_group
                    .filter(|group| group_sizes.get(group).copied().unwrap_or(0) > 1)
                    .map(|group| *fresh_groups.entry(group).or_insert(clip.id));
                pasted.insert(clip.id);
                commands.push(Command::AddClip {
                    track: target_id,
                    clip,
                });
            }
        }
        Some((batch(commands)?, pasted))
    }

    /// Plan duplicating the copied clips right after the whole group, on their own
    /// tracks (see [`Clipboard::paste`] for the placement rules).
    pub(crate) fn duplicate(
        &self,
        project: &Project,
        new_id: impl FnMut() -> u64,
    ) -> Option<(Command, BTreeSet<ClipId>)> {
        self.paste(project, self.end(), new_id)
    }
}

/// Whether every range fits `track` without overlapping an existing clip.
fn fits(track: &Track, ranges: &[(&CopiedClip, TimeRange)]) -> bool {
    ranges.iter().all(|(_, range)| {
        track
            .clips()
            .values()
            .all(|clip| clip.range.end <= range.start || clip.range.start >= range.end)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::tests::project_with;
    use animathio_core::History;

    fn set(ids: &[u64]) -> BTreeSet<ClipId> {
        ids.iter().copied().map(ClipId).collect()
    }

    fn counter(project: &Project) -> impl FnMut() -> u64 {
        let mut next = project
            .tracks()
            .iter()
            .flat_map(|track| track.clips().keys().map(|id| id.0).chain([track.id().0]))
            .max()
            .unwrap_or(0)
            + 1;
        move || {
            next += 1;
            next - 1
        }
    }

    fn layout(project: &Project) -> Vec<(String, Vec<(i64, i64)>)> {
        project
            .tracks()
            .iter()
            .map(|track| {
                let mut ranges: Vec<_> = track
                    .clips()
                    .values()
                    .map(|clip| (clip.range.start.0, clip.range.end.0))
                    .collect();
                ranges.sort_unstable();
                (track.name().to_owned(), ranges)
            })
            .collect()
    }

    #[test]
    fn copy_round_trips_through_json_and_rejects_foreign_text() {
        let project = project_with(&[(1, &[(1, 0, 10), (2, 10, 20)][..]), (2, &[(3, 5, 8)][..])]);
        let board = Clipboard::copy(&project, &set(&[2, 3, 99])).unwrap();
        assert_eq!(board.clips.len(), 2);
        assert_eq!((board.start(), board.end()), (Time(5), Time(20)));
        let text = board.to_json();
        assert_eq!(Clipboard::from_json(&text), Some(board));
        assert!(Clipboard::from_json("not json").is_none());
        assert!(Clipboard::from_json("{\"format\":\"other\",\"clips\":[]}").is_none());
        assert!(Clipboard::copy(&project, &set(&[99])).is_none());
    }

    #[test]
    fn pasted_clips_never_join_the_originals_link_groups() {
        let mut project =
            project_with(&[(1, &[(1, 0, 10)][..]), (2, &[(2, 0, 10), (3, 20, 30)][..])]);
        let mut history = History::default();
        for id in [1, 2] {
            let link = Command::SetClipLink {
                id: ClipId(id),
                group: Some(ClipId(1)),
            };
            history.execute(&mut project, &link).unwrap();
        }
        let group_of = |project: &Project, id: ClipId| {
            project
                .tracks()
                .iter()
                .find_map(|track| track.clips().get(&id))
                .and_then(|clip| clip.link_group)
        };

        // Both linked clips copied: the copies form their own fresh group.
        let board = Clipboard::copy(&project, &set(&[1, 2])).unwrap();
        let (command, pasted) = board.paste(&project, Time(100), counter(&project)).unwrap();
        history.execute(&mut project, &command).unwrap();
        let groups: BTreeSet<_> = pasted.iter().map(|id| group_of(&project, *id)).collect();
        assert_eq!(groups.len(), 1);
        let fresh = groups
            .into_iter()
            .next()
            .flatten()
            .expect("copies stay linked");
        assert_ne!(fresh, ClipId(1));
        assert!(pasted.contains(&fresh));

        // Only one member copied: the copy is unlinked.
        let board = Clipboard::copy(&project, &set(&[1])).unwrap();
        let (command, pasted) = board.paste(&project, Time(200), counter(&project)).unwrap();
        history.execute(&mut project, &command).unwrap();
        assert!(pasted.iter().all(|id| group_of(&project, *id).is_none()));
        assert_eq!(group_of(&project, ClipId(1)), Some(ClipId(1)));
        assert_eq!(group_of(&project, ClipId(2)), Some(ClipId(1)));
    }

    #[test]
    fn copying_a_linked_clip_brings_its_companion_along() {
        let mut project = project_with(&[(1, &[(1, 0, 10)][..]), (2, &[(2, 0, 10)][..])]);
        for id in [1, 2] {
            Command::SetClipLink {
                id: ClipId(id),
                group: Some(ClipId(1)),
            }
            .apply(&mut project)
            .unwrap();
        }
        let board = Clipboard::copy_linked(&project, &set(&[1])).unwrap();
        assert_eq!(board.clips.len(), 2);
        let (command, pasted) = board.paste(&project, Time(50), counter(&project)).unwrap();
        command.apply(&mut project).unwrap();
        let groups: BTreeSet<_> = project
            .tracks()
            .iter()
            .flat_map(|track| track.clips().values())
            .filter(|clip| pasted.contains(&clip.id))
            .map(|clip| clip.link_group)
            .collect();
        assert_eq!(groups.len(), 1);
        assert!(groups.into_iter().next().flatten().is_some());
    }

    #[test]
    fn paste_lands_at_the_playhead_on_original_tracks_with_new_ids() {
        let mut project =
            project_with(&[(1, &[(1, 0, 10), (2, 10, 20)][..]), (2, &[(3, 5, 8)][..])]);
        let board = Clipboard::copy(&project, &set(&[1, 2, 3])).unwrap();
        let (command, pasted) = board.paste(&project, Time(100), counter(&project)).unwrap();
        assert_eq!(pasted.len(), 3);
        assert!(pasted.iter().all(|id| id.0 > 3));
        let mut history = History::default();
        let before = project.clone();
        history.execute(&mut project, &command).unwrap();
        // Earliest copied clip starts at the playhead; relative offsets are kept.
        assert_eq!(
            layout(&project),
            [
                ("T1".into(), vec![(0, 10), (10, 20), (100, 110), (110, 120)]),
                ("T2".into(), vec![(5, 8), (105, 108)]),
            ]
        );
        history.undo(&mut project).unwrap();
        assert_eq!(project, before, "paste is a single undo step");
    }

    #[test]
    fn paste_creates_missing_tracks_and_avoids_occupied_or_locked_ones() {
        let mut project = project_with(&[(1, &[(1, 0, 10)][..]), (2, &[(2, 0, 10)][..])]);
        let board = Clipboard::copy(&project, &set(&[1, 2])).unwrap();
        // Occupied: pasting on top of the originals goes to new tracks.
        let (command, _) = board.paste(&project, Time(0), counter(&project)).unwrap();
        let mut history = History::default();
        history.execute(&mut project, &command).unwrap();
        assert_eq!(project.tracks().len(), 4);
        assert_eq!(project.tracks()[2].name(), "T1");
        assert_eq!(project.tracks()[3].name(), "T2");
        history.undo(&mut project).unwrap();
        // Missing: the source tracks were removed before pasting elsewhere.
        let mut other = project_with(&[(9, &[(50, 0, 5)][..])]);
        let (command, _) = board.paste(&other, Time(30), counter(&other)).unwrap();
        history.execute(&mut other, &command).unwrap();
        assert_eq!(
            layout(&other),
            [
                ("T9".into(), vec![(0, 5)]),
                ("T1".into(), vec![(30, 40)]),
                ("T2".into(), vec![(30, 40)]),
            ]
        );
        // Locked: never touch a locked track.
        Command::SetTrackLocked {
            id: TrackId(1),
            locked: true,
        }
        .apply(&mut project)
        .unwrap();
        let (command, _) = board.paste(&project, Time(50), counter(&project)).unwrap();
        history.execute(&mut project, &command).unwrap();
        assert_eq!(project.tracks()[0].clips().len(), 1);
        assert_eq!(project.tracks().len(), 3);
    }

    #[test]
    fn duplicate_places_the_group_right_after_itself() {
        let mut project =
            project_with(&[(1, &[(1, 0, 10), (2, 10, 20)][..]), (2, &[(3, 0, 5)][..])]);
        let board = Clipboard::copy(&project, &set(&[1, 3])).unwrap();
        assert_eq!(board.end(), Time(10));
        let (command, pasted) = board.duplicate(&project, counter(&project)).unwrap();
        // Track 1 has a clip at 10..20, so its copy is pushed to a new track; track 2 is free.
        History::default().execute(&mut project, &command).unwrap();
        assert_eq!(pasted.len(), 2);
        assert_eq!(
            layout(&project),
            [
                ("T1".into(), vec![(0, 10), (10, 20)]),
                ("T2".into(), vec![(0, 5), (10, 15)]),
                ("T1".into(), vec![(10, 20)]),
            ]
        );
    }
}
