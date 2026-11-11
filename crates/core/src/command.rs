use crate::{
    Animation, Asset, AssetId, Clip, ClipId, Error, Keyframe, Keyframes, Project, Target, Time,
    TimeRange, Track, TrackId, Transform, invalid,
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
    /// Insert or replace a numeric keyframe.
    AddKeyframe {
        /// Clip identity.
        id: ClipId,
        /// Property being animated.
        target: Target,
        /// Clip-local ticks.
        time: Time,
        /// Value and outgoing interpolation.
        keyframe: Keyframe<f64>,
    },
    /// Remove a numeric keyframe; removes an empty animation too.
    RemoveKeyframe {
        /// Clip identity.
        id: ClipId,
        /// Property being animated.
        target: Target,
        /// Clip-local ticks.
        time: Time,
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
                    clip.params.insert(name.clone(), *value)
                } else {
                    clip.params.remove(name)
                };
                Self::SetParam {
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
