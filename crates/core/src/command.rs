use crate::builtin::Color;
use crate::{
    Animation, Asset, AssetId, Clip, ClipId, ClipKind, EffectId, EffectInstance, Error, FrameRate,
    Keyframe, Keyframes, Project, Target, Time, TimeRange, Track, TrackId, Transform, invalid,
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
        })
    }
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
