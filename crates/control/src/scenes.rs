//! Snapshot-bound scene proposals shared by interactive and headless clients.
use crate::{
    ControlError,
    scene_detector::{Detector, thumbnail},
};
use animathio_core::{
    AssetId, ClipId, ClipKind, Command, Marker, Project, TICKS_PER_SECOND, Time, TrackId,
};
use animathio_media::{FilmCraftBackend, MediaBackend};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicBool, Ordering},
};

/// Scene analysis options. Exactly one clip or track must be selected.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SceneParams {
    /// Selected video clip.
    pub clip: Option<ClipId>,
    /// Selected video track, mutually exclusive with clip.
    pub track: Option<TrackId>,
    /// Larger values detect smaller changes; range 0..=1.
    pub sensitivity: f64,
    /// Minimum spacing between cuts in project frames.
    pub min_shot_frames: u32,
    /// Delete resulting shots shorter than this many frames; zero disables.
    pub delete_shorter_than_frames: u32,
}
impl Default for SceneParams {
    fn default() -> Self {
        Self {
            clip: None,
            track: None,
            sensitivity: 0.5,
            min_shot_frames: 6,
            delete_shorter_than_frames: 0,
        }
    }
}
/// One independently selectable cut, on the project frame grid.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SceneCut {
    /// Original video clip identity.
    pub clip: ClipId,
    /// Absolute timeline cut position.
    pub time: Time,
    /// Review toggle.
    pub enabled: bool,
    /// 96 by 54 grayscale review thumbnail.
    #[serde(default, with = "thumbnail_encoding")]
    pub thumbnail: Vec<u8>,
}
/// Proposals are valid only for the project snapshot that was analysed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SceneDraft {
    expected: Project,
    #[serde(default)]
    delete_shorter_than_frames: u32,
    /// Independently selectable proposed cuts.
    pub cuts: Vec<SceneCut>,
}
fn invalid(s: &str) -> ControlError {
    ControlError::InvalidParams(s.into())
}
/// Decode through the regular media backend at four samples per second.
#[allow(
    clippy::too_many_lines,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub fn detect_scenes(
    project: &Project,
    assets: &BTreeMap<AssetId, Vec<u8>>,
    params: &SceneParams,
    cancel: &AtomicBool,
    progress: &dyn Fn(u32),
) -> Result<SceneDraft, ControlError> {
    if params.clip.is_some() == params.track.is_some() {
        return Err(invalid("select one video clip or track"));
    }
    if !params.sensitivity.is_finite() || !(0.0..=1.0).contains(&params.sensitivity) {
        return Err(invalid("sensitivity must be 0..=1"));
    }
    let clips: Vec<_> = project
        .tracks()
        .iter()
        .flat_map(|t| t.clips().values().map(move |c| (t, c)))
        .filter(|(t, c)| {
            matches!(c.kind, ClipKind::Video(_))
                && params
                    .clip
                    .map_or_else(|| params.track == Some(t.id()), |id| c.id == id)
        })
        .collect();
    if clips.is_empty() {
        return Err(invalid("no video clips selected"));
    }
    let step = TICKS_PER_SECOND / 4;
    let total: i64 = clips
        .iter()
        .map(|(_, c)| (c.range.end.0 - c.range.start.0 - 1) / step + 1)
        .sum();
    if total > 20_000 {
        return Err(invalid("analysis exceeds 20000 samples"));
    }
    let minimum = project.frame_rate().frame_to_time(
        i64::from(params.min_shot_frames),
        animathio_core::Rounding::Ceil,
    )?;
    let mut cuts = Vec::new();
    let mut done = 0;
    progress(0);
    for (_, clip) in clips {
        let ClipKind::Video(asset) = &clip.kind else {
            unreachable!()
        };
        let bytes = assets
            .get(asset)
            .ok_or_else(|| invalid("missing video asset"))?;
        if AssetId::of(bytes) != *asset {
            return Err(invalid("video asset hash mismatch"));
        }
        let decoder = FilmCraftBackend::new()
            .open_video_bytes("media", bytes.clone().into())
            .map_err(|e| ControlError::Other(e.to_string()))?;
        let mut detector = Detector::default();
        for local in (0..clip.range.end.0 - clip.range.start.0)
            .step_by(usize::try_from(step).map_err(|_| invalid("unsupported sample step"))?)
        {
            if cancel.load(Ordering::Relaxed) {
                return Err(invalid("cancelled"));
            }
            let frame = decoder
                .frame_at(clip.source_time(Time(local)))
                .map_err(|e| ControlError::Other(e.to_string()))?;
            if detector.sample(
                &thumbnail(&frame),
                Time(local),
                0.36 * (1. - params.sensitivity) + 0.001,
                minimum,
            ) {
                let time = Time(clip.range.start.0 + local);
                let time = project.frame_rate().frame_to_time(
                    project
                        .frame_rate()
                        .time_to_frame(time, animathio_core::Rounding::Nearest)?,
                    animathio_core::Rounding::Nearest,
                )?;
                if time > clip.range.start && clip.range.end.0 - time.0 >= minimum.0 {
                    if cuts.len() >= 1000 {
                        return Err(invalid(
                            "more than 1000 scene changes; select a shorter clip",
                        ));
                    }
                    cuts.push(SceneCut {
                        clip: clip.id,
                        time,
                        enabled: true,
                        thumbnail: thumbnail(&frame)
                            .iter()
                            .map(|v| (v * 255.).round() as u8)
                            .collect(),
                    });
                }
            }
            done += 1;
            progress(u32::try_from(done * 100 / total).unwrap_or(100));
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(invalid("cancelled"));
    }
    progress(100);
    Ok(SceneDraft {
        expected: project.clone(),
        delete_shorter_than_frames: params.delete_shorter_than_frames,
        cuts,
    })
}
/// Build a single atomic undo step. Core splitting propagates to linked audio.
pub fn scene_command(
    project: &Project,
    draft: &SceneDraft,
    markers: bool,
) -> Result<Command, ControlError> {
    if draft.cuts.len() > 1000 {
        return Err(invalid("too many scene cuts"));
    }
    if project != &draft.expected {
        return Err(invalid("project changed; detect scenes again"));
    }
    let mut scratch = project.clone();
    let mut commands = Vec::new();
    let mut segments = std::collections::BTreeSet::new();
    let mut cuts: Vec<_> = draft.cuts.iter().filter(|c| c.enabled).collect();
    cuts.sort_by_key(|c| (c.clip, std::cmp::Reverse(c.time)));
    for cut in cuts {
        if !project
            .tracks()
            .iter()
            .find_map(|t| t.clips().get(&cut.clip))
            .is_some_and(|c| {
                matches!(c.kind, ClipKind::Video(_))
                    && c.range.start < cut.time
                    && cut.time < c.range.end
            })
        {
            return Err(invalid("scene cut must be inside a video clip"));
        }
        let command = if markers {
            let mut marker = Marker::new(scratch.next_marker_id(), cut.time);
            marker.name = "Scene change".into();
            Command::AddMarker { marker }
        } else {
            let next = scratch
                .tracks()
                .iter()
                .flat_map(|t| t.clips().keys())
                .map(|id| id.0)
                .max()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| invalid("clip ids exhausted"))?;
            segments.insert(cut.clip);
            segments.insert(ClipId(next));
            Command::SplitClip {
                id: cut.clip,
                right_id: ClipId(next),
                at: cut.time,
            }
        };
        command.apply(&mut scratch)?;
        commands.push(command);
    }
    if !markers && draft.delete_shorter_than_frames > 0 {
        let minimum = project.frame_rate().frame_to_time(
            i64::from(draft.delete_shorter_than_frames),
            animathio_core::Rounding::Ceil,
        )?;
        for id in segments {
            if scratch
                .tracks()
                .iter()
                .find_map(|t| t.clips().get(&id))
                .is_some_and(|c| c.range.end.0 - c.range.start.0 < minimum.0)
            {
                let command = Command::RemoveClip { id };
                command.apply(&mut scratch)?;
                commands.push(command);
            }
        }
    }
    while commands.len() > 512 {
        commands = commands
            .chunks(512)
            .map(|c| Command::Batch {
                commands: c.to_vec(),
            })
            .collect();
    }
    Ok(Command::Batch { commands })
}

// Compact wire representation keeps large reviewed proposals under transport limits.
mod thumbnail_encoding {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let encoded = String::deserialize(deserializer)?;
        if encoded.len() > 96 * 54 * 2 {
            return Err(serde::de::Error::custom("thumbnail too large"));
        }
        STANDARD.decode(encoded).map_err(serde::de::Error::custom)
    }
}
