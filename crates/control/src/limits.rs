//! Source-media limits for edits. The core model is resource free and does not know how
//! long a media asset is, so the layers that own the asset bytes (this crate's handler and
//! the editor) check edits against probed durations here.
//!
//! The rule: an edit must not make a video or audio clip read further into its source than
//! it did before, past the end of the media. Stills, generated
//! clips (plugins, vectors) and clips whose duration is unknown are unbounded. Shortening
//! or moving a clip, and splitting, never extend the source range, so they always pass.

use animathio_core::{AssetId, ClipId, ClipKind, Command, Project, Time};
use std::collections::{BTreeMap, BTreeSet};

/// Probed media durations per asset.
#[derive(Clone, Debug, Default)]
pub struct SourceLimits {
    durations: BTreeMap<AssetId, Time>,
    probed: BTreeSet<AssetId>,
}

/// Exclusive mapped source end of clips before an edit.
#[derive(Clone, Debug, Default)]
pub struct SourceSnapshot {
    ends: BTreeMap<ClipId, i64>,
    existing: BTreeSet<ClipId>,
    asset_ends: BTreeMap<AssetId, i64>,
}

impl SourceLimits {
    /// No known durations: every clip is unbounded.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record the duration of `asset` (from a media pool or a test).
    pub fn set(&mut self, asset: AssetId, duration: Time) {
        self.probed.insert(asset.clone());
        self.durations.insert(asset, duration);
    }

    /// Probe every video/audio asset used by a clip that has not been probed yet. Assets that
    /// cannot be probed stay unbounded (and are not probed again).
    pub fn probe_missing(&mut self, project: &Project, assets: &BTreeMap<AssetId, Vec<u8>>) {
        use animathio_media::MediaBackend;
        for track in project.tracks() {
            for clip in track.clips().values() {
                let (ClipKind::Video(id) | ClipKind::Audio(id)) = &clip.kind else {
                    continue;
                };
                let Some(bytes) = assets.get(id) else {
                    continue;
                };
                if !self.probed.insert(id.clone()) {
                    continue;
                }
                if let Ok(info) = animathio_media::FilmCraftBackend::new()
                    .probe_bytes("media", bytes.clone().into())
                {
                    self.durations.insert(id.clone(), info.duration);
                }
            }
        }
    }

    /// Known media duration of a clip's source; `None` when unbounded.
    #[must_use]
    pub fn duration_of(&self, kind: &ClipKind) -> Option<Time> {
        match kind {
            ClipKind::Video(id) | ClipKind::Audio(id) => self.durations.get(id).copied(),
            _ => None,
        }
    }

    /// Latest timeline end a clip may have for its current source offset, or `None` when the
    /// clip is unbounded. Frame-aligned cuts must stay within the media end.
    #[must_use]
    pub fn max_end(&self, _project: &Project, clip: &animathio_core::Clip) -> Option<Time> {
        let limit = self.duration_of(&clip.kind)?;
        if clip.time_map.hold {
            return None;
        }
        if clip.time_map.reverse {
            // Extending a reverse tail reads toward the source start. An unused
            // prefix of a split clock may lie beyond the asset, so only the retained
            // clock's lower boundary limits this trim, not limit - source_offset.
            let length = clip.time_map.source_length?;
            let duration = clip.time_map.duration_for(length).ok()?.0 - clip.time_map.phase.0;
            return Some(Time(clip.range.start.0.saturating_add(duration.max(0))));
        }
        let room = limit.0 - clip.source_offset.0;
        let duration = if clip.time_map == animathio_core::ClipTimeMap::default() {
            room.max(0)
        } else {
            let room = clip
                .time_map
                .source_length
                .map_or(room, |length| room.min(length.0));
            if room <= 0 {
                return Some(clip.range.start);
            }
            clip.time_map.duration_for(Time(room)).ok()?.0 - clip.time_map.phase.0
        };
        Some(Time(clip.range.start.0.saturating_add(duration.max(0))))
    }

    /// Record the source ends of `ids` and of every clip linked to them.
    #[must_use]
    pub fn snapshot(project: &Project, ids: &[ClipId]) -> SourceSnapshot {
        let mut groups = BTreeSet::new();
        for track in project.tracks() {
            for clip in track.clips().values() {
                if ids.contains(&clip.id)
                    && let Some(group) = clip.link_group
                {
                    groups.insert(group);
                }
            }
        }
        let mut ends = BTreeMap::new();
        let mut existing = BTreeSet::new();
        let mut asset_ends = BTreeMap::<AssetId, i64>::new();
        for track in project.tracks() {
            for clip in track.clips().values() {
                existing.insert(clip.id);
                if ids.contains(&clip.id) || clip.link_group.is_some_and(|g| groups.contains(&g)) {
                    let end = source_end(clip);
                    ends.insert(clip.id, end);
                    if let ClipKind::Video(asset) | ClipKind::Audio(asset) = &clip.kind {
                        asset_ends
                            .entry(asset.clone())
                            .and_modify(|old| *old = (*old).max(end))
                            .or_insert(end);
                    }
                }
            }
        }
        SourceSnapshot {
            ends,
            existing,
            asset_ends,
        }
    }

    /// Fail when an edit pushed a snapshotted clip's source end further out than before and past
    /// the end of its media.
    pub fn verify(&self, project: &Project, before: &SourceSnapshot) -> Result<(), String> {
        for track in project.tracks() {
            for clip in track.clips().values() {
                let old_end = before.ends.get(&clip.id).or_else(|| {
                    // A split/hold can replace the original id inside the same batch.
                    // Compare newly generated ids against their asset's previous bound;
                    // an edit cannot hide an overrun by splitting or inserting a hold.
                    if before.existing.contains(&clip.id) {
                        return None;
                    }
                    match &clip.kind {
                        ClipKind::Video(asset) | ClipKind::Audio(asset) => {
                            before.asset_ends.get(asset)
                        }
                        _ => None,
                    }
                });
                let Some(old_end) = old_end else {
                    continue;
                };
                let Some(limit) = self.duration_of(&clip.kind) else {
                    continue;
                };
                let new_end = source_end(clip);
                if new_end > *old_end && new_end > limit.0 {
                    return Err(format!(
                        "clip {} would run past the end of its source media ({} ticks long)",
                        clip.id.0, limit.0
                    ));
                }
            }
        }
        Ok(())
    }

    /// Check `command` against the limits without changing `project`. Only commands that can
    /// change a clip's source range are examined; a command that fails to apply is left for the
    /// real execution to report.
    pub fn guard(
        &mut self,
        project: &Project,
        assets: &BTreeMap<AssetId, Vec<u8>>,
        command: &Command,
    ) -> Result<(), String> {
        let mut ids = Vec::new();
        trimmed_ids(command, &mut ids);
        if ids.is_empty() {
            return Ok(());
        }
        self.probe_missing(project, assets);
        if self.durations.is_empty() {
            return Ok(());
        }
        let snapshot = Self::snapshot(project, &ids);
        let mut scratch = project.clone();
        if command.apply(&mut scratch).is_err() {
            return Ok(());
        }
        self.verify(&scratch, &snapshot)
    }
}

fn source_end(clip: &animathio_core::Clip) -> i64 {
    clip.source_bounds().1.0
}

fn trimmed_ids(command: &Command, out: &mut Vec<ClipId>) {
    match command {
        Command::TrimClip { id, .. }
        | Command::SetClipSpeed { id, .. }
        | Command::InsertFrameHold { id, .. } => out.push(*id),
        Command::Batch { commands } => commands.iter().for_each(|c| trimmed_ids(c, out)),
        _ => {}
    }
}
