use crate::{
    Animation, Asset, AssetId, AudioChain, Clip, ClipId, ClipKind, EffectId, EffectInstance, Error,
    FrameRate, Keyframe, Keyframes, Project, Target, Time, TimeRange, Track, TrackId, Transform,
    Transition, TransitionId, invalid,
};
use serde::{Deserialize, Serialize};

/// Largest number of edits in one [`Command::Batch`].
const MAX_BATCH: usize = 1024;

/// Exhaustive serializable edits. Applying one returns the exact inverse.
/// A failed edit leaves the project unchanged. IDs are supplied by callers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Command {
    /// Set or clear a clip's companion group (unlink each member in a batch).
    SetClipLink {
        /// Clip to link or unlink.
        id: ClipId,
        /// Companion group, or none to unlink.
        group: Option<ClipId>,
    },
    /// Apply an exact edit without companion propagation (used by undo).
    Exact {
        /// Exact command to apply.
        command: Box<Command>,
    },
    /// Insert a layer at a back-to-front compositing index.
    AddTrack {
        /// Layer data (also restores removed clips).
        track: Track,
        /// Position in the layer stack.
        index: usize,
    },
    /// Change the linear audio gain of a layer.
    SetTrackGain {
        /// Layer identity.
        id: TrackId,
        /// Finite, nonnegative gain.
        gain: f64,
    },
    /// Mute or unmute a layer (silent in the mix, hidden from rendering).
    SetTrackMuted {
        /// Layer identity.
        id: TrackId,
        /// New mute state.
        muted: bool,
    },
    /// Solo or unsolo a layer. While any layer is soloed, only soloed layers
    /// are audible and visible.
    SetTrackSolo {
        /// Layer identity.
        id: TrackId,
        /// New solo state.
        solo: bool,
    },
    /// Lock or unlock a layer against clip edits.
    SetTrackLocked {
        /// Layer identity.
        id: TrackId,
        /// New lock state.
        locked: bool,
    },
    /// Rename a layer.
    RenameTrack {
        /// Layer identity.
        id: TrackId,
        /// New display name.
        name: String,
    },
    /// Move a layer to `index` in the stack: the layer's position after the move,
    /// so it must be smaller than the layer count.
    MoveTrack {
        /// Layer identity.
        id: TrackId,
        /// Destination position in the layer stack.
        index: usize,
    },
    /// Remove a layer with all its clips.
    RemoveTrack {
        /// Layer identity.
        id: TrackId,
    },
    /// Register content-addressed metadata.
    AddAsset {
        /// Hash of the external bytes.
        id: AssetId,
        /// Resource-free metadata.
        asset: Asset,
    },
    /// Remove an unreferenced asset.
    RemoveAsset {
        /// Asset identity.
        id: AssetId,
    },
    /// Set project-level output settings (resolution, frame rate, background, duration).
    SetSettings {
        /// Output width in pixels, or `0` for no explicit resolution.
        width: u32,
        /// Output height in pixels, or `0` for no explicit resolution.
        height: u32,
        /// Output frame rate.
        frame_rate: FrameRate,
        /// Straight-alpha sRGB canvas background.
        background: [u8; 4],
        /// Explicit project duration in ticks; `0` means derive from clips.
        duration: Time,
    },
    /// Add a clip to a layer.
    AddClip {
        /// Destination layer.
        track: TrackId,
        /// Draft clip.
        clip: Clip,
    },
    /// Remove a clip.
    RemoveClip {
        /// Clip identity.
        id: ClipId,
    },
    /// Move a clip without changing duration or source offset.
    MoveClip {
        /// Clip identity.
        id: ClipId,
        /// Destination layer.
        track: TrackId,
        /// New inclusive start.
        start: Time,
    },
    /// Change placement and source offset together; automation stays clip-local.
    TrimClip {
        /// Clip identity.
        id: ClipId,
        /// New timeline range.
        range: TimeRange,
        /// New source offset.
        source_offset: Time,
    },
    /// Split one clip into two adjacent clips at a point strictly inside its range.
    SplitClip {
        /// Existing clip identity; it becomes the left half.
        id: ClipId,
        /// Identity for the new right half.
        right_id: ClipId,
        /// Split point in timeline ticks.
        at: Time,
    },
    /// Restore exact clip data after a split or merge.
    RestoreClips {
        /// Track containing the clips.
        track: TrackId,
        /// Identities to replace.
        ids: Vec<ClipId>,
        /// Original clip snapshots.
        clips: Vec<Clip>,
    },
    /// Merge two adjacent clips back into one (the inverse of `SplitClip`).
    MergeClip {
        /// Left clip identity.
        left: ClipId,
        /// Right clip identity.
        right: ClipId,
    },
    /// Replace the base normalized transform.
    SetTransform {
        /// Clip identity.
        id: ClipId,
        /// New base transform.
        transform: Transform,
    },
    /// Set or remove a base numeric parameter.
    SetParam {
        /// Clip identity.
        id: ClipId,
        /// Parameter name.
        name: String,
        /// New value; `None` removes it (only if not animated).
        value: Option<f64>,
    },
    /// Insert an effect into a clip's ordered effect chain.
    AddEffect {
        /// Clip identity.
        id: ClipId,
        /// Position in the effect chain.
        index: usize,
        /// Effect instance (with a unique id within the clip).
        effect: EffectInstance,
    },
    /// Remove an effect by id; fails if any animation targets it.
    RemoveEffect {
        /// Clip identity.
        id: ClipId,
        /// Effect identity.
        effect: EffectId,
    },
    /// Set or remove an effect parameter.
    SetEffectParam {
        /// Clip identity.
        id: ClipId,
        /// Effect identity.
        effect: EffectId,
        /// Parameter name.
        name: String,
        /// New value; `None` removes it (only if not animated).
        value: Option<f64>,
    },
    /// Insert an audio effect into a clip's or track's ordered audio-effect chain.
    AddAudioEffect {
        /// Chain to edit.
        chain: AudioChain,
        /// Position in the chain.
        index: usize,
        /// Effect instance (with an id unique within the clip, or within the track).
        effect: EffectInstance,
    },
    /// Remove an audio effect by id; fails if any animation targets it.
    RemoveAudioEffect {
        /// Chain to edit.
        chain: AudioChain,
        /// Effect identity.
        effect: EffectId,
    },
    /// Move an audio effect to another position in its chain.
    MoveAudioEffect {
        /// Chain to edit.
        chain: AudioChain,
        /// Effect identity.
        effect: EffectId,
        /// New index in the chain after the move.
        index: usize,
    },
    /// Set or remove a base parameter of an audio effect.
    SetAudioEffectParam {
        /// Chain to edit.
        chain: AudioChain,
        /// Effect identity.
        effect: EffectId,
        /// Parameter name.
        name: String,
        /// New value; `None` removes it (only if not animated).
        value: Option<f64>,
    },
    /// Bypass or re-enable an audio effect without removing it.
    SetAudioEffectBypass {
        /// Chain to edit.
        chain: AudioChain,
        /// Effect identity.
        effect: EffectId,
        /// New bypass state.
        bypass: bool,
    },
    /// Set or remove a typed plugin parameter override. Opaque plugin data is preserved.
    SetPluginParam {
        /// Clip identity.
        id: ClipId,
        /// Parameter name.
        name: String,
        /// New typed value; `None` removes it.
        value: Option<crate::ParamValue>,
    },
    /// Insert or replace a typed keyframe.
    AddKeyframe {
        /// Clip identity.
        id: ClipId,
        /// Property being animated.
        target: Target,
        /// Clip-local ticks.
        time: Time,
        /// Value and outgoing interpolation.
        keyframe: Keyframe<crate::ParamValue>,
    },
    /// Remove a typed keyframe; removes an empty animation too.
    RemoveKeyframe {
        /// Clip identity.
        id: ClipId,
        /// Property being animated.
        target: Target,
        /// Clip-local ticks.
        time: Time,
    },
    /// Move a typed keyframe to a new clip-local time.
    MoveKeyframe {
        /// Clip identity.
        id: ClipId,
        /// Property being animated.
        target: Target,
        /// Current clip-local ticks.
        from: Time,
        /// Destination clip-local ticks; must be free.
        to: Time,
    },
    /// Apply several commands as one transaction and one undo step. The inverse is a
    /// `Batch` of the inverses in reverse order. The project is validated once, after
    /// the last command, so intermediate states may be temporarily inconsistent.
    Batch {
        /// Commands applied in order.
        commands: Vec<Command>,
    },
    /// Split every clip on every track that strictly contains one of `times`.
    /// Right halves get fresh ids, allocated deterministically from the largest
    /// existing clip id. The inverse is a `Batch` of merges.
    SplitAt {
        /// Timeline split points, in any order; duplicates are ignored.
        times: Vec<Time>,
    },
    /// Remove all content inside `range` on every track: clips fully inside are removed,
    /// clips crossing an edge are trimmed and clips spanning the whole range are split.
    /// With `ripple`, content after the range moves left by the range length and an
    /// explicit project duration shrinks accordingly.
    RemoveRange {
        /// Half-open timeline range to remove.
        range: TimeRange,
        /// Close the gap by shifting later content left on every track.
        ripple: bool,
    },
    /// Ripple-remove a range on selected tracks. The selection must contain every
    /// linked companion's track; locked tracks are rejected atomically.
    RemoveRangeOnTracks {
        /// Half-open timeline range to remove.
        range: TimeRange,
        /// Tracks to cut and ripple. Other tracks keep their positions.
        tracks: Vec<TrackId>,
    },
    /// Remove a clip and close the gap it leaves on its own track by shifting later
    /// clips on that track left by the clip's duration.
    RippleDelete {
        /// Clip identity.
        clip: ClipId,
    },
    /// Add a transition on the edit point between two adjacent clips of a track.
    AddTransition {
        /// Layer owning both clips.
        track: TrackId,
        /// Transition (with a project-unique id).
        transition: Transition,
    },
    /// Remove a transition.
    RemoveTransition {
        /// Transition identity.
        id: TransitionId,
    },
    /// Change a transition's total (cut-centered) duration.
    SetTransitionDuration {
        /// Transition identity.
        id: TransitionId,
        /// New positive duration in ticks.
        duration: Time,
    },
}

impl Command {
    /// Apply transactionally, validating the resulting project and returning its inverse.
    /// Cloning for rollback is deliberately simple for the initial prototype.
    pub fn apply(&self, project: &mut Project) -> Result<Self, Error> {
        if let Some(track) = self.locked_track(project) {
            return Err(Error(format!("track {} is locked", track.0)));
        }
        let mut candidate = project.clone();
        let inverse = self.apply_inner(&mut candidate)?;
        candidate.validate()?;
        *project = candidate;
        Ok(inverse)
    }

    /// The locked layer this command would edit, if any. Layer-level commands
    /// (gain, mute, solo, lock, add/remove layer) and project-level commands are
    /// never blocked, so a locked layer can always be unlocked.
    #[must_use]
    pub fn locked_track(&self, project: &Project) -> Option<TrackId> {
        let of_clip = |id: ClipId| {
            project
                .tracks
                .iter()
                .find(|t| t.locked && t.clips.contains_key(&id))
                .map(|t| t.id)
        };
        match self {
            Self::AddClip { track, .. } => project
                .tracks
                .iter()
                .find(|t| t.id == *track && t.locked)
                .map(|t| t.id),
            Self::Exact { command } => command.locked_track(project),
            Self::SetClipLink { id, .. }
            | Self::RemoveClip { id }
            | Self::MoveClip { id, .. }
            | Self::TrimClip { id, .. }
            | Self::SplitClip { id, .. }
            | Self::SetTransform { id, .. }
            | Self::SetParam { id, .. }
            | Self::AddEffect { id, .. }
            | Self::RemoveEffect { id, .. }
            | Self::SetEffectParam { id, .. }
            | Self::SetPluginParam { id, .. }
            | Self::AddKeyframe { id, .. }
            | Self::RemoveKeyframe { id, .. }
            | Self::MoveKeyframe { id, .. } => of_clip(*id),
            Self::MergeClip { left, right } => of_clip(*left).or_else(|| of_clip(*right)),
            Self::AddAudioEffect { chain, .. }
            | Self::RemoveAudioEffect { chain, .. }
            | Self::MoveAudioEffect { chain, .. }
            | Self::SetAudioEffectParam { chain, .. }
            | Self::SetAudioEffectBypass { chain, .. } => match chain {
                AudioChain::Clip(id) => of_clip(*id),
                // Like gain and mute, track-level audio effects stay editable on a locked layer.
                AudioChain::Track(_) => None,
            },
            Self::Batch { commands } => commands
                .iter()
                .find_map(|command| command.locked_track(project)),
            _ => None,
        }
    }

    fn apply_inner(&self, project: &mut Project) -> Result<Self, Error> {
        if let Self::Exact { command } = self {
            return command.apply_raw(project).map(|inverse| Self::Exact {
                command: Box::new(inverse),
            });
        }
        if let Self::MergeClip { left, right } = self {
            let commands = merge_commands(project, *left, *right)?;
            if commands.len() > 1 {
                return apply_generated(project, &commands);
            }
        }
        let id = match self {
            Self::MoveClip { id, .. }
            | Self::TrimClip { id, .. }
            | Self::RemoveClip { id }
            | Self::SplitClip { id, .. } => Some(*id),
            _ => None,
        };
        if let Some(id) = id {
            let original = project.tracks[clip_track_index(project, id)?].clips[&id].clone();
            if let Some(group) = original.link_group {
                let members: Vec<_> = project
                    .tracks
                    .iter()
                    .flat_map(|t| t.clips.values().map(move |c| (t.id, c.clone())))
                    .filter(|(_, c)| c.id != id && c.link_group == Some(group))
                    .collect();
                let mut commands = vec![self.clone()];
                let mut next = next_clip_id(project)?;
                if let Self::SplitClip { right_id, .. } = self {
                    next = next.max(
                        right_id
                            .0
                            .checked_add(1)
                            .ok_or_else(|| invalid("clip id overflow"))?,
                    );
                }
                for (track, clip) in members {
                    commands.push(match self {
                        Self::RemoveClip { .. } => Self::RemoveClip { id: clip.id },
                        Self::MoveClip { start, .. } => Self::MoveClip {
                            id: clip.id,
                            track,
                            start: shifted(clip.range.start, original.range.start, *start)
                                .ok_or_else(|| invalid("move overflow"))?,
                        },
                        // Apply the edit's deltas, not the absolute range, so a pair
                        // that was ever misaligned keeps its relative offset.
                        Self::TrimClip {
                            range,
                            source_offset,
                            ..
                        } => Self::TrimClip {
                            id: clip.id,
                            range: TimeRange::new(
                                shifted(clip.range.start, original.range.start, range.start)
                                    .ok_or_else(|| invalid("trim overflow"))?,
                                shifted(clip.range.end, original.range.end, range.end)
                                    .ok_or_else(|| invalid("trim overflow"))?,
                            )?,
                            source_offset: shifted(
                                clip.source_offset,
                                original.source_offset,
                                *source_offset,
                            )
                            .ok_or_else(|| invalid("trim overflow"))?,
                        },
                        Self::SplitClip { at, .. } => {
                            let right_id = ClipId(next);
                            next = next
                                .checked_add(1)
                                .ok_or_else(|| invalid("clip id overflow"))?;
                            Self::SplitClip {
                                id: clip.id,
                                right_id,
                                at: *at,
                            }
                        }
                        _ => unreachable!(),
                    });
                }
                return apply_generated(project, &commands);
            }
        }
        self.apply_raw(project).map(|inverse| {
            if matches!(self, Self::AddClip { clip, .. } if clip.link_group.is_some()) {
                Self::Exact {
                    command: Box::new(inverse),
                }
            } else {
                inverse
            }
        })
    }

    // Keep the exhaustive command/inverse pairs together for review.
    #[allow(clippy::too_many_lines)]
    fn apply_raw(&self, project: &mut Project) -> Result<Self, Error> {
        Ok(match self {
            Self::Exact { command } => command.apply_raw(project)?,
            Self::SetClipLink { id, group } => {
                let clip = clip_mut(project, *id)?;
                let old = clip.link_group;
                clip.link_group = *group;
                Self::SetClipLink {
                    id: *id,
                    group: old,
                }
            }
            Self::Batch { commands } => {
                if commands.len() > MAX_BATCH {
                    return Err(invalid("batch too large"));
                }
                apply_all(project, commands)?
            }
            Self::SplitAt { times } => {
                let commands = split_at_commands(project, times)?;
                apply_generated(project, &commands)?
            }
            Self::RemoveRange { range, ripple } => {
                let commands = remove_range_commands(project, *range, *ripple, None)?;
                apply_generated(project, &commands)?
            }
            Self::RemoveRangeOnTracks { range, tracks } => {
                if tracks.is_empty() {
                    return Err(invalid("range selection has no tracks"));
                }
                for id in tracks {
                    if project.tracks.iter().any(|t| t.id == *id && t.locked) {
                        return Err(invalid("range selection includes a locked track"));
                    }
                    if !project.tracks.iter().any(|t| t.id == *id) {
                        return Err(invalid("range selection includes a missing track"));
                    }
                }
                let groups: std::collections::BTreeSet<_> = project
                    .tracks
                    .iter()
                    .filter(|t| tracks.contains(&t.id))
                    .flat_map(|t| t.clips.values().filter_map(|c| c.link_group))
                    .collect();
                if project.tracks.iter().any(|t| {
                    !tracks.contains(&t.id)
                        && t.clips
                            .values()
                            .any(|c| c.link_group.is_some_and(|g| groups.contains(&g)))
                }) {
                    return Err(invalid("range selection omits a linked companion track"));
                }
                let commands = remove_range_commands(project, *range, true, Some(tracks))?;
                apply_generated(project, &commands)?
            }
            Self::RippleDelete { clip } => {
                let original = &project.tracks[clip_track_index(project, *clip)?].clips[clip];
                let mut ids = vec![*clip];
                if let Some(group) = original.link_group {
                    ids.extend(
                        project
                            .tracks
                            .iter()
                            .flat_map(|t| t.clips.values())
                            .filter(|c| c.id != *clip && c.link_group == Some(group))
                            .map(|c| c.id),
                    );
                }
                let mut commands = Vec::new();
                for id in ids {
                    commands.extend(ripple_delete_commands(project, id)?);
                }
                let mut moved: std::collections::BTreeSet<_> = commands
                    .iter()
                    .filter_map(|c| {
                        if let Self::MoveClip { id, .. } = c {
                            Some(*id)
                        } else {
                            None
                        }
                    })
                    .collect();
                for command in commands.clone() {
                    if let Self::MoveClip { id, start, .. } = command {
                        let original = &project.tracks[clip_track_index(project, id)?].clips[&id];
                        if let Some(group) = original.link_group {
                            for track in &project.tracks {
                                for companion in
                                    track.clips.values().filter(|c| c.link_group == Some(group))
                                {
                                    if moved.insert(companion.id) {
                                        commands.push(Self::MoveClip {
                                            id: companion.id,
                                            track: track.id,
                                            start: shifted(
                                                companion.range.start,
                                                original.range.start,
                                                start,
                                            )
                                            .ok_or_else(|| invalid("ripple move overflow"))?,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
                apply_generated(project, &commands)?
            }
            Self::AddTrack { track, index } => {
                if *index > project.tracks.len() {
                    return Err(invalid("track index out of bounds"));
                }
                project.tracks.insert(*index, track.clone());
                Self::RemoveTrack { id: track.id }
            }
            Self::SetTrackGain { id, gain } => {
                let index = track_index(project, *id)?;
                let inverse = Self::SetTrackGain {
                    id: *id,
                    gain: project.tracks[index].gain,
                };
                project.tracks[index].gain = *gain;
                inverse
            }
            Self::SetTrackMuted { id, muted } => {
                let index = track_index(project, *id)?;
                let inverse = Self::SetTrackMuted {
                    id: *id,
                    muted: project.tracks[index].muted,
                };
                project.tracks[index].muted = *muted;
                inverse
            }
            Self::SetTrackSolo { id, solo } => {
                let index = track_index(project, *id)?;
                let inverse = Self::SetTrackSolo {
                    id: *id,
                    solo: project.tracks[index].solo,
                };
                project.tracks[index].solo = *solo;
                inverse
            }
            Self::SetTrackLocked { id, locked } => {
                let index = track_index(project, *id)?;
                let inverse = Self::SetTrackLocked {
                    id: *id,
                    locked: project.tracks[index].locked,
                };
                project.tracks[index].locked = *locked;
                inverse
            }
            Self::RenameTrack { id, name } => {
                let index = track_index(project, *id)?;
                Self::RenameTrack {
                    id: *id,
                    name: std::mem::replace(&mut project.tracks[index].name, name.clone()),
                }
            }
            Self::MoveTrack { id, index } => {
                let from = track_index(project, *id)?;
                if *index >= project.tracks.len() {
                    return Err(invalid("track index out of bounds"));
                }
                let track = project.tracks.remove(from);
                project.tracks.insert(*index, track);
                Self::MoveTrack {
                    id: *id,
                    index: from,
                }
            }
            Self::RemoveTrack { id } => {
                let index = track_index(project, *id)?;
                let track = project.tracks.remove(index);
                Self::AddTrack { track, index }
            }
            Self::AddAsset { id, asset } => {
                if project.assets.contains_key(id) {
                    return Err(invalid("asset already exists"));
                }
                project.assets.insert(id.clone(), asset.clone());
                Self::RemoveAsset { id: id.clone() }
            }
            Self::RemoveAsset { id } => {
                let asset = project
                    .assets
                    .remove(id)
                    .ok_or_else(|| invalid("asset missing"))?;
                Self::AddAsset {
                    id: id.clone(),
                    asset,
                }
            }
            Self::SetSettings {
                width,
                height,
                frame_rate,
                background,
                duration,
            } => {
                let inverse = Self::SetSettings {
                    width: project.width,
                    height: project.height,
                    frame_rate: project.frame_rate,
                    background: project.background,
                    duration: project.duration,
                };
                project.width = *width;
                project.height = *height;
                project.frame_rate = *frame_rate;
                project.background = *background;
                project.duration = *duration;
                inverse
            }
            Self::AddClip { track, clip } => {
                let index = track_index(project, *track)?;
                if project
                    .tracks
                    .iter()
                    .any(|t| t.clips.contains_key(&clip.id))
                {
                    return Err(invalid("clip already exists"));
                }
                project.tracks[index].clips.insert(clip.id, clip.clone());
                Self::RemoveClip { id: clip.id }
            }
            Self::RemoveClip { id } => {
                let index = clip_track_index(project, *id)?;
                let track = &mut project.tracks[index];
                let clip = track
                    .clips
                    .remove(id)
                    .ok_or_else(|| invalid("clip missing"))?;
                Self::AddClip {
                    track: track.id,
                    clip,
                }
            }
            Self::MoveClip { id, track, start } => {
                let source_index = clip_track_index(project, *id)?;
                let destination_index = track_index(project, *track)?;
                let source = &mut project.tracks[source_index];
                let mut clip = source
                    .clips
                    .remove(id)
                    .ok_or_else(|| invalid("clip missing"))?;
                let inverse = Self::MoveClip {
                    id: *id,
                    track: source.id,
                    start: clip.range.start,
                };
                let duration = clip.range.end.0 - clip.range.start.0;
                clip.range = TimeRange::new(
                    *start,
                    Time(
                        start
                            .0
                            .checked_add(duration)
                            .ok_or_else(|| invalid("move overflow"))?,
                    ),
                )?;
                project.tracks[destination_index].clips.insert(*id, clip);
                inverse
            }
            Self::TrimClip {
                id,
                range,
                source_offset,
            } => {
                let clip = clip_mut(project, *id)?;
                let inverse = Self::TrimClip {
                    id: *id,
                    range: clip.range,
                    source_offset: clip.source_offset,
                };
                clip.range = *range;
                clip.source_offset = *source_offset;
                inverse
            }
            Self::RestoreClips { track, ids, clips } => {
                let unique: std::collections::BTreeSet<_> = ids.iter().collect();
                let clip_ids: std::collections::BTreeSet<_> =
                    clips.iter().map(|clip| &clip.id).collect();
                if unique.len() != ids.len()
                    || clip_ids.len() != clips.len()
                    || !clip_ids.is_subset(&unique)
                {
                    return Err(invalid(
                        "restored clips must have unique replacement identities",
                    ));
                }
                let index = track_index(project, *track)?;
                let previous = ids
                    .iter()
                    .filter_map(|id| project.tracks[index].clips.remove(id))
                    .collect();
                for clip in clips {
                    project.tracks[index].clips.insert(clip.id, clip.clone());
                }
                Self::RestoreClips {
                    track: *track,
                    ids: ids.clone(),
                    clips: previous,
                }
            }
            Self::SplitClip { id, right_id, at } => {
                if id == right_id {
                    return Err(invalid("split clip ids must differ"));
                }
                let index = clip_track_index(project, *id)?;
                if project
                    .tracks
                    .iter()
                    .any(|t| t.clips.contains_key(right_id))
                {
                    return Err(invalid("right clip already exists"));
                }
                let left = project.tracks[index]
                    .clips
                    .get(id)
                    .ok_or_else(|| invalid("clip missing"))?
                    .clone();
                if *at <= left.range.start || *at >= left.range.end {
                    return Err(invalid("split point outside clip"));
                }
                let duration = at.0 - left.range.start.0;
                let mut right = left.clone();
                right.id = *right_id;
                right.range = TimeRange::new(*at, left.range.end)?;
                right.source_offset = Time(left.source_offset.0 + duration);
                for animation in &mut right.animations {
                    let boundary = animation.keys.sample(Time(duration));
                    let interpolation = animation
                        .keys
                        .0
                        .range(..=Time(duration))
                        .next_back()
                        .or_else(|| animation.keys.0.first_key_value())
                        .map_or_default(|(_, key)| key.interpolation);
                    animation.keys.0 = animation
                        .keys
                        .0
                        .iter()
                        .filter(|(time, _)| time.0 >= duration)
                        .map(|(time, key)| (Time(time.0 - duration), key.clone()))
                        .collect();
                    if let Some(value) = boundary {
                        animation.keys.0.entry(Time(0)).or_insert(Keyframe {
                            value,
                            interpolation,
                        });
                    }
                }

                project.tracks[index]
                    .clips
                    .get_mut(id)
                    .expect("clip checked above")
                    .range = TimeRange::new(left.range.start, *at)?;
                project.tracks[index].clips.insert(*right_id, right);
                Self::RestoreClips {
                    track: project.tracks[index].id,
                    ids: vec![*id, *right_id],
                    clips: vec![left],
                }
            }
            Self::MergeClip { left, right } => {
                let left_index = clip_track_index(project, *left)?;
                let right_index = clip_track_index(project, *right)?;
                if left_index != right_index {
                    return Err(invalid("merge clips must share a track"));
                }
                let left_clip = project.tracks[left_index]
                    .clips
                    .get(left)
                    .ok_or_else(|| invalid("clip missing"))?
                    .clone();
                let right_clip = project.tracks[right_index]
                    .clips
                    .get(right)
                    .ok_or_else(|| invalid("clip missing"))?
                    .clone();
                if left_clip.range.end != right_clip.range.start {
                    return Err(invalid("merge clips must be adjacent"));
                }
                if left_clip.kind != right_clip.kind {
                    return Err(invalid("merge clips must share a kind"));
                }
                let restored = project.tracks[left_index]
                    .clips
                    .get_mut(left)
                    .ok_or_else(|| invalid("clip missing"))?;
                restored.range = TimeRange::new(left_clip.range.start, right_clip.range.end)?;
                restored.source_offset = left_clip.source_offset;
                project.tracks[right_index].clips.remove(right);
                Self::RestoreClips {
                    track: project.tracks[left_index].id,
                    ids: vec![*left, *right],
                    clips: vec![left_clip, right_clip],
                }
            }
            Self::SetTransform { id, transform } => {
                let clip = clip_mut(project, *id)?;
                let inverse = Self::SetTransform {
                    id: *id,
                    transform: clip.transform,
                };
                clip.transform = *transform;
                inverse
            }
            Self::SetParam { id, name, value } => {
                let clip = clip_mut(project, *id)?;
                let previous = if let Some(value) = value {
                    clip.params
                        .insert(name.clone(), crate::ParamValue::Number(*value))
                } else {
                    clip.params.remove(name)
                };
                Self::SetParam {
                    id: *id,
                    name: name.clone(),
                    value: previous
                        .map(|v| {
                            v.as_number()
                                .ok_or_else(|| invalid("non-numeric parameter"))
                        })
                        .transpose()?,
                }
            }
            Self::AddEffect { id, index, effect } => {
                let clip = clip_mut(project, *id)?;
                if *index > clip.effects.len() {
                    return Err(invalid("effect index out of bounds"));
                }
                if clip.effects.iter().any(|existing| existing.id == effect.id) {
                    return Err(invalid("effect already exists"));
                }
                clip.effects.insert(*index, effect.clone());
                Self::RemoveEffect {
                    id: *id,
                    effect: effect.id,
                }
            }
            Self::RemoveEffect { id, effect } => {
                let clip = clip_mut(project, *id)?;
                let index = clip
                    .effects
                    .iter()
                    .position(|instance| instance.id == *effect)
                    .ok_or_else(|| invalid("effect missing"))?;
                if clip.animations.iter().any(|animation| {
                    matches!(&animation.target, Target::Effect { effect: target, .. } if target == effect)
                }) {
                    return Err(invalid("effect is animated"));
                }
                let removed = clip.effects.remove(index);
                Self::AddEffect {
                    id: *id,
                    index,
                    effect: removed,
                }
            }
            Self::SetEffectParam {
                id,
                effect,
                name,
                value,
            } => {
                let clip = clip_mut(project, *id)?;
                let instance = clip
                    .effects
                    .iter_mut()
                    .find(|instance| instance.id == *effect)
                    .ok_or_else(|| invalid("effect missing"))?;
                let previous = if let Some(value) = value {
                    instance.params.insert(name.clone(), *value)
                } else {
                    instance.params.remove(name)
                };
                Self::SetEffectParam {
                    id: *id,
                    effect: *effect,
                    name: name.clone(),
                    value: previous,
                }
            }
            Self::AddAudioEffect {
                chain,
                index,
                effect,
            } => {
                let (list, animations) = audio_chain_mut(project, *chain)?;
                if *index > list.len() {
                    return Err(invalid("audio effect index out of bounds"));
                }
                if list.iter().any(|existing| existing.id == effect.id) {
                    return Err(invalid("audio effect already exists"));
                }
                let _ = animations;
                list.insert(*index, effect.clone());
                Self::RemoveAudioEffect {
                    chain: *chain,
                    effect: effect.id,
                }
            }
            Self::RemoveAudioEffect { chain, effect } => {
                let (list, animations) = audio_chain_mut(project, *chain)?;
                let index = list
                    .iter()
                    .position(|instance| instance.id == *effect)
                    .ok_or_else(|| invalid("audio effect missing"))?;
                if animations.iter().any(|animation| {
                    matches!(&animation.target, Target::Effect { effect: target, .. } if target == effect)
                }) {
                    return Err(invalid("audio effect is animated"));
                }
                let removed = list.remove(index);
                Self::AddAudioEffect {
                    chain: *chain,
                    index,
                    effect: removed,
                }
            }
            Self::MoveAudioEffect {
                chain,
                effect,
                index,
            } => {
                let (list, _) = audio_chain_mut(project, *chain)?;
                let from = list
                    .iter()
                    .position(|instance| instance.id == *effect)
                    .ok_or_else(|| invalid("audio effect missing"))?;
                if *index >= list.len() {
                    return Err(invalid("audio effect index out of bounds"));
                }
                let moved = list.remove(from);
                list.insert(*index, moved);
                Self::MoveAudioEffect {
                    chain: *chain,
                    effect: *effect,
                    index: from,
                }
            }
            Self::SetAudioEffectParam {
                chain,
                effect,
                name,
                value,
            } => {
                let (list, animations) = audio_chain_mut(project, *chain)?;
                let instance = list
                    .iter_mut()
                    .find(|instance| instance.id == *effect)
                    .ok_or_else(|| invalid("audio effect missing"))?;
                if value.is_none()
                    && animations.iter().any(|animation| {
                        matches!(&animation.target, Target::Effect { effect: target, param } if target == effect && param == name)
                    })
                {
                    return Err(invalid("audio effect parameter is animated"));
                }
                let previous = if let Some(value) = value {
                    instance.params.insert(name.clone(), *value)
                } else {
                    instance.params.remove(name)
                };
                Self::SetAudioEffectParam {
                    chain: *chain,
                    effect: *effect,
                    name: name.clone(),
                    value: previous,
                }
            }
            Self::SetAudioEffectBypass {
                chain,
                effect,
                bypass,
            } => {
                let (list, _) = audio_chain_mut(project, *chain)?;
                let instance = list
                    .iter_mut()
                    .find(|instance| instance.id == *effect)
                    .ok_or_else(|| invalid("audio effect missing"))?;
                let previous = std::mem::replace(&mut instance.bypass, *bypass);
                Self::SetAudioEffectBypass {
                    chain: *chain,
                    effect: *effect,
                    bypass: previous,
                }
            }
            Self::SetPluginParam { id, name, value } => {
                let clip = clip_mut(project, *id)?;
                if !matches!(clip.kind, ClipKind::Plugin { .. }) {
                    return Err(invalid("plugin parameter on a non-plugin clip"));
                }
                let previous = if let Some(value) = value {
                    clip.params.insert(name.clone(), value.clone())
                } else {
                    clip.params.remove(name)
                };
                Self::SetPluginParam {
                    id: *id,
                    name: name.clone(),
                    value: previous,
                }
            }
            Self::AddKeyframe {
                id,
                target,
                time,
                keyframe,
            } => {
                let clip = clip_mut(project, *id)?;
                let index = match clip.animations.binary_search_by(|a| a.target.cmp(target)) {
                    Ok(index) => index,
                    Err(index) => {
                        clip.animations.insert(
                            index,
                            Animation {
                                target: target.clone(),
                                keys: Keyframes::default(),
                            },
                        );
                        index
                    }
                };
                if let Some(previous) = clip.animations[index]
                    .keys
                    .0
                    .insert(*time, keyframe.clone())
                {
                    Self::AddKeyframe {
                        id: *id,
                        target: target.clone(),
                        time: *time,
                        keyframe: previous,
                    }
                } else {
                    Self::RemoveKeyframe {
                        id: *id,
                        target: target.clone(),
                        time: *time,
                    }
                }
            }
            Self::RemoveKeyframe { id, target, time } => {
                let clip = clip_mut(project, *id)?;
                let index = clip
                    .animations
                    .binary_search_by(|a| a.target.cmp(target))
                    .map_err(|_| invalid("animation missing"))?;
                let keyframe = clip.animations[index]
                    .keys
                    .0
                    .remove(time)
                    .ok_or_else(|| invalid("keyframe missing"))?;
                if clip.animations[index].keys.0.is_empty() {
                    clip.animations.remove(index);
                }
                Self::AddKeyframe {
                    id: *id,
                    target: target.clone(),
                    time: *time,
                    keyframe,
                }
            }
            Self::MoveKeyframe {
                id,
                target,
                from,
                to,
            } => {
                if from == to {
                    return Err(invalid("move keyframe to its current time"));
                }
                if to.0 < 0 {
                    return Err(invalid("keyframe time must be nonnegative"));
                }
                let clip = clip_mut(project, *id)?;
                let index = clip
                    .animations
                    .binary_search_by(|a| a.target.cmp(target))
                    .map_err(|_| invalid("animation missing"))?;
                let keys = &mut clip.animations[index].keys.0;
                if keys.contains_key(to) {
                    return Err(invalid("keyframe destination occupied"));
                }
                let keyframe = keys
                    .remove(from)
                    .ok_or_else(|| invalid("keyframe missing"))?;
                keys.insert(*to, keyframe);
                Self::MoveKeyframe {
                    id: *id,
                    target: target.clone(),
                    from: *to,
                    to: *from,
                }
            }
            Self::AddTransition { track, transition } => {
                if project
                    .tracks
                    .iter()
                    .any(|t| t.transitions.iter().any(|x| x.id == transition.id))
                {
                    return Err(invalid("transition already exists"));
                }
                let index = track_index(project, *track)?;
                let transitions = &mut project.tracks[index].transitions;
                let position = transitions.partition_point(|x| x.id < transition.id);
                transitions.insert(position, transition.clone());
                Self::RemoveTransition { id: transition.id }
            }
            Self::RemoveTransition { id } => {
                let (track, position) = transition_index(project, *id)?;
                let transition = project.tracks[track].transitions.remove(position);
                Self::AddTransition {
                    track: project.tracks[track].id,
                    transition,
                }
            }
            Self::SetTransitionDuration { id, duration } => {
                let (track, position) = transition_index(project, *id)?;
                let transition = &mut project.tracks[track].transitions[position];
                let previous = transition.duration;
                transition.duration = *duration;
                Self::SetTransitionDuration {
                    id: *id,
                    duration: previous,
                }
            }
        })
    }
}
/// `value + (to - from)` in checked arithmetic.
fn shifted(value: Time, from: Time, to: Time) -> Option<Time> {
    value.0.checked_add(to.0.checked_sub(from.0)?).map(Time)
}

/// Apply `commands` in order without intermediate validation, returning the combined
/// inverse. On failure the caller's candidate project is discarded by `Command::apply`.
fn apply_all(project: &mut Project, commands: &[Command]) -> Result<Command, Error> {
    let mut inverses = Vec::with_capacity(commands.len());
    let mut removed_companions = std::collections::BTreeSet::new();
    for command in commands {
        if let Command::RemoveClip { id } = command {
            if removed_companions.contains(id) && clip_track_index(project, *id).is_err() {
                continue;
            }
            let clip = &project.tracks[clip_track_index(project, *id)?].clips[id];
            if let Some(group) = clip.link_group {
                removed_companions.extend(
                    project
                        .tracks
                        .iter()
                        .flat_map(|t| t.clips.values())
                        .filter(|c| c.link_group == Some(group))
                        .map(|c| c.id),
                );
            }
        }
        inverses.push(command.apply_inner(project)?);
    }
    inverses.reverse();
    Ok(inverse_batch(inverses))
}

/// Combine generated inverses into one command. The user-facing [`MAX_BATCH`] cap
/// applies when a batch is applied, so a long inverse list is nested into chunks of at
/// most `MAX_BATCH` instead of exceeding it (undo of a large generated edit must work).
fn inverse_batch(mut commands: Vec<Command>) -> Command {
    while commands.len() > MAX_BATCH {
        commands = commands
            .chunks(MAX_BATCH)
            .map(|chunk| Command::Batch {
                commands: chunk.to_vec(),
            })
            .collect();
    }
    Command::Batch { commands }
}

/// Apply commands generated by a compound edit, rejecting any that touch a locked track.
fn apply_generated(project: &mut Project, commands: &[Command]) -> Result<Command, Error> {
    if let Some(track) = commands.iter().find_map(|c| c.locked_track(project)) {
        return Err(Error(format!("track {} is locked", track.0)));
    }
    let mut inverses = Vec::new();
    let mut split_groups = std::collections::BTreeMap::new();
    for command in commands {
        if let Command::SplitClip { id, right_id, at } = command {
            let clip = &project.tracks[clip_track_index(project, *id)?].clips[id];
            if let Some(group) = clip.link_group {
                let new_group = *split_groups.entry((group, *at)).or_insert(*right_id);
                inverses.push(Command::Exact {
                    command: Box::new(command.apply_raw(project)?),
                });
                inverses.push(Command::Exact {
                    command: Box::new(
                        Command::SetClipLink {
                            id: *right_id,
                            group: Some(new_group),
                        }
                        .apply_raw(project)?,
                    ),
                });
                continue;
            }
        }
        inverses.push(Command::Exact {
            command: Box::new(command.apply_raw(project)?),
        });
    }
    inverses.reverse();
    Ok(inverse_batch(inverses))
}

/// The merges needed to join `left` and `right`, plus their linked companions.
///
/// Two clips of one group (or two unlinked clips) merge alone. Clips of two different
/// groups merge together with every companion pair, which must exist on one track each
/// and be adjacent; a mix of linked and unlinked clips is refused because it would
/// leave the companion desynchronized.
fn merge_commands(project: &Project, left: ClipId, right: ClipId) -> Result<Vec<Command>, Error> {
    let find = |id: ClipId| -> Result<Clip, Error> {
        Ok(project.tracks[clip_track_index(project, id)?].clips[&id].clone())
    };
    let (l, r) = (find(left)?, find(right)?);
    let mut commands = vec![Command::MergeClip { left, right }];
    match (l.link_group, r.link_group) {
        (None, None) => {}
        (Some(a), Some(b)) if a == b => {}
        (Some(a), Some(b)) => {
            let members = |group: ClipId, skip: ClipId| -> Vec<(TrackId, Clip)> {
                project
                    .tracks
                    .iter()
                    .flat_map(|t| t.clips.values().map(move |c| (t.id, c.clone())))
                    .filter(|(_, c)| c.id != skip && c.link_group == Some(group))
                    .collect()
            };
            let (left_side, right_side) = (members(a, left), members(b, right));
            if left_side.len() != right_side.len() {
                return Err(invalid(
                    "cannot merge linked clips: companion clips do not match",
                ));
            }
            for (track, companion) in left_side {
                let partner = right_side
                    .iter()
                    .find(|(t, c)| *t == track && c.range.start == companion.range.end)
                    .ok_or_else(|| {
                        invalid("cannot merge linked clips: companion clips do not match")
                    })?;
                commands.push(Command::MergeClip {
                    left: companion.id,
                    right: partner.1.id,
                });
            }
        }
        _ => {
            return Err(invalid(
                "cannot merge a linked clip with an unlinked clip; unlink first",
            ));
        }
    }
    Ok(commands)
}

/// First clip id strictly greater than every existing clip id.
fn next_clip_id(project: &Project) -> Result<u64, Error> {
    project
        .tracks
        .iter()
        .flat_map(|t| t.clips.keys())
        .map(|id| id.0)
        .max()
        .map_or(Some(1), |max| max.checked_add(1))
        .ok_or_else(|| invalid("clip id overflow"))
}

fn split_at_commands(project: &Project, times: &[Time]) -> Result<Vec<Command>, Error> {
    let mut times = times.to_vec();
    times.sort_unstable();
    times.dedup();
    let mut next = next_clip_id(project)?;
    let mut commands = Vec::new();
    for track in &project.tracks {
        for clip in track.clips.values() {
            // Split from the right so the original id keeps shrinking leftwards and each
            // later split point is still inside the clip being split.
            for at in times
                .iter()
                .rev()
                .filter(|at| clip.range.start < **at && **at < clip.range.end)
            {
                commands.push(Command::SplitClip {
                    id: clip.id,
                    right_id: ClipId(next),
                    at: *at,
                });
                next = next
                    .checked_add(1)
                    .ok_or_else(|| invalid("clip id overflow"))?;
            }
        }
    }
    Ok(commands)
}

fn remove_range_commands(
    project: &Project,
    range: TimeRange,
    ripple: bool,
    tracks: Option<&[TrackId]>,
) -> Result<Vec<Command>, Error> {
    range.validate()?;
    let (cut_start, cut_end) = (range.start.0, range.end.0);
    let length = cut_end - cut_start;
    let shift = |t: i64| if ripple { t - length } else { t };
    let mut next = next_clip_id(project)?;
    let mut commands = Vec::new();
    for track in &project.tracks {
        if tracks.is_some_and(|ids| !ids.contains(&track.id)) {
            continue;
        }
        for clip in track.clips.values() {
            let (start, end) = (clip.range.start.0, clip.range.end.0);
            let offset = clip.source_offset.0;
            if end <= cut_start {
                continue;
            }
            if start >= cut_end {
                if ripple {
                    commands.push(Command::MoveClip {
                        id: clip.id,
                        track: track.id,
                        start: Time(shift(start)),
                    });
                }
                continue;
            }
            match (start < cut_start, end > cut_end) {
                // Fully inside the removed range.
                (false, false) => commands.push(Command::RemoveClip { id: clip.id }),
                // Crosses the range start: keep the head.
                (true, false) => commands.push(Command::TrimClip {
                    id: clip.id,
                    range: TimeRange::new(clip.range.start, range.start)?,
                    source_offset: clip.source_offset,
                }),
                // Crosses the range end: keep the tail.
                (false, true) => commands.push(Command::TrimClip {
                    id: clip.id,
                    range: TimeRange::new(Time(shift(cut_end)), Time(shift(end)))?,
                    source_offset: Time(offset + (cut_end - start)),
                }),
                // Spans the whole range: split, then trim the right half.
                (true, true) => {
                    let right_id = ClipId(next);
                    next = next
                        .checked_add(1)
                        .ok_or_else(|| invalid("clip id overflow"))?;
                    commands.push(Command::SplitClip {
                        id: clip.id,
                        right_id,
                        at: range.start,
                    });
                    commands.push(Command::TrimClip {
                        id: clip.id,
                        range: TimeRange::new(clip.range.start, range.start)?,
                        source_offset: clip.source_offset,
                    });
                    commands.push(Command::TrimClip {
                        id: right_id,
                        range: TimeRange::new(Time(shift(cut_end)), Time(shift(end)))?,
                        source_offset: Time(offset + (cut_end - start)),
                    });
                }
            }
        }
    }
    if ripple
        && tracks.is_none_or(|ids| project.tracks.iter().all(|t| ids.contains(&t.id)))
        && project.duration.0 > cut_start
    {
        let removed = project.duration.0.min(cut_end) - cut_start;
        commands.push(Command::SetSettings {
            width: project.width,
            height: project.height,
            frame_rate: project.frame_rate,
            background: project.background,
            duration: Time(project.duration.0 - removed),
        });
    }
    Ok(commands)
}

fn ripple_delete_commands(project: &Project, id: ClipId) -> Result<Vec<Command>, Error> {
    let track = &project.tracks[clip_track_index(project, id)?];
    let clip = &track.clips[&id];
    let length = clip.range.end.0 - clip.range.start.0;
    let mut commands = vec![Command::RemoveClip { id }];
    for later in track.clips.values() {
        if later.range.start >= clip.range.end {
            commands.push(Command::MoveClip {
                id: later.id,
                track: track.id,
                start: Time(later.range.start.0 - length),
            });
        }
    }
    Ok(commands)
}

fn transition_index(project: &Project, id: TransitionId) -> Result<(usize, usize), Error> {
    project
        .tracks
        .iter()
        .enumerate()
        .find_map(|(track, t)| {
            t.transitions
                .iter()
                .position(|x| x.id == id)
                .map(|position| (track, position))
        })
        .ok_or_else(|| invalid("transition missing"))
}
fn track_index(project: &Project, id: TrackId) -> Result<usize, Error> {
    project
        .tracks
        .iter()
        .position(|t| t.id == id)
        .ok_or_else(|| invalid("track missing"))
}
fn clip_track_index(project: &Project, id: ClipId) -> Result<usize, Error> {
    project
        .tracks
        .iter()
        .position(|t| t.clips.contains_key(&id))
        .ok_or_else(|| invalid("clip missing"))
}
/// Mutable access to an audio-effect chain plus the animations that may target it
/// (track chains have none).
fn audio_chain_mut(
    project: &mut Project,
    chain: AudioChain,
) -> Result<(&mut Vec<EffectInstance>, &[Animation]), Error> {
    match chain {
        AudioChain::Clip(id) => {
            let clip = clip_mut(project, id)?;
            Ok((&mut clip.audio_effects, &clip.animations))
        }
        AudioChain::Track(id) => {
            let index = track_index(project, id)?;
            Ok((&mut project.tracks[index].audio_effects, &[]))
        }
    }
}
fn clip_mut(project: &mut Project, id: ClipId) -> Result<&mut Clip, Error> {
    let index = clip_track_index(project, id)?;
    project.tracks[index]
        .clips
        .get_mut(&id)
        .ok_or_else(|| invalid("clip missing"))
}

/// Session-only undo/redo stacks. Project files contain data, not editing history.
#[derive(Clone, Debug, Default)]
pub struct History {
    undo: Vec<Command>,
    redo: Vec<Command>,
}
impl History {
    /// Apply an edit, saving its inverse and clearing redo on success only.
    pub fn execute(&mut self, project: &mut Project, command: &Command) -> Result<(), Error> {
        let inverse = command.apply(project)?;
        self.undo.push(inverse);
        self.redo.clear();
        Ok(())
    }
    /// Undo one edit, returning false if the stack is empty. An entry that fails to
    /// apply is dropped (and the error returned) so it never blocks earlier undos.
    pub fn undo(&mut self, project: &mut Project) -> Result<bool, Error> {
        let Some(command) = self.undo.last() else {
            return Ok(false);
        };
        match command.apply(project) {
            Ok(inverse) => {
                self.undo.pop();
                self.redo.push(inverse);
                Ok(true)
            }
            Err(error) => {
                // An entry that cannot be applied must not block every earlier undo.
                self.undo.pop();
                Err(error)
            }
        }
    }
    /// Redo one edit, returning false if the stack is empty. A failing entry is dropped.
    pub fn redo(&mut self, project: &mut Project) -> Result<bool, Error> {
        let Some(command) = self.redo.last() else {
            return Ok(false);
        };
        match command.apply(project) {
            Ok(inverse) => {
                self.redo.pop();
                self.undo.push(inverse);
                Ok(true)
            }
            Err(error) => {
                self.redo.pop();
                Err(error)
            }
        }
    }
}
