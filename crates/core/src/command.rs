use crate::builtin::Color;
use crate::{
    Animation, Asset, AssetId, Clip, ClipId, ClipKind, EffectId, EffectInstance, Error, FrameRate,
    Keyframe, Keyframes, Project, Target, Time, TimeRange, Track, TrackId, Transform, Transition,
    TransitionId, invalid,
};
use serde::{Deserialize, Serialize};

/// Exhaustive serializable edits. Applying one returns the exact inverse.
/// A failed edit leaves the project unchanged. IDs are supplied by callers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Command {
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
        background: Color,
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
        let mut candidate = project.clone();
        let inverse = self.apply_inner(&mut candidate)?;
        candidate.validate()?;
        *project = candidate;
        Ok(inverse)
    }

    // Keep the exhaustive command/inverse pairs together for review.
    #[allow(clippy::too_many_lines)]
    fn apply_inner(&self, project: &mut Project) -> Result<Self, Error> {
        Ok(match self {
            Self::Batch { commands } => apply_all(project, commands)?,
            Self::SplitAt { times } => {
                let commands = split_at_commands(project, times)?;
                apply_all(project, &commands)?
            }
            Self::RemoveRange { range, ripple } => {
                let commands = remove_range_commands(project, *range, *ripple)?;
                apply_all(project, &commands)?
            }
            Self::RippleDelete { clip } => {
                let commands = ripple_delete_commands(project, *clip)?;
                apply_all(project, &commands)?
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
                project.tracks[index]
                    .clips
                    .get_mut(id)
                    .expect("clip checked above")
                    .range = TimeRange::new(left.range.start, *at)?;
                project.tracks[index].clips.insert(*right_id, right);
                Self::MergeClip {
                    left: *id,
                    right: *right_id,
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
                let at = left_clip.range.end;
                let original_source = right_clip.source_offset.0 - (at.0 - left_clip.range.start.0);
                if original_source < 0 {
                    return Err(invalid("merge would produce a negative source offset"));
                }
                let restored = project.tracks[left_index]
                    .clips
                    .get_mut(left)
                    .ok_or_else(|| invalid("clip missing"))?;
                restored.range = TimeRange::new(left_clip.range.start, right_clip.range.end)?;
                restored.source_offset = Time(original_source);
                project.tracks[right_index].clips.remove(right);
                Self::SplitClip {
                    id: *left,
                    right_id: *right,
                    at,
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
/// Apply `commands` in order without intermediate validation, returning the combined
/// inverse. On failure the caller's candidate project is discarded by `Command::apply`.
fn apply_all(project: &mut Project, commands: &[Command]) -> Result<Command, Error> {
    let mut inverses = Vec::with_capacity(commands.len());
    for command in commands {
        inverses.push(command.apply_inner(project)?);
    }
    inverses.reverse();
    Ok(Command::Batch { commands: inverses })
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
) -> Result<Vec<Command>, Error> {
    range.validate()?;
    let (cut_start, cut_end) = (range.start.0, range.end.0);
    let length = cut_end - cut_start;
    let shift = |t: i64| if ripple { t - length } else { t };
    let mut next = next_clip_id(project)?;
    let mut commands = Vec::new();
    for track in &project.tracks {
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
    if ripple && project.duration.0 > cut_start {
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
    /// Undo one edit, returning false if the stack is empty.
    pub fn undo(&mut self, project: &mut Project) -> Result<bool, Error> {
        let Some(command) = self.undo.last() else {
            return Ok(false);
        };
        let inverse = command.apply(project)?;
        self.undo.pop();
        self.redo.push(inverse);
        Ok(true)
    }
    /// Redo one edit, returning false if the stack is empty.
    pub fn redo(&mut self, project: &mut Project) -> Result<bool, Error> {
        let Some(command) = self.redo.last() else {
            return Ok(false);
        };
        let inverse = command.apply(project)?;
        self.redo.pop();
        self.undo.push(inverse);
        Ok(true)
    }
}
