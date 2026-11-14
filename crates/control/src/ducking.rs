//! Auto-ducking: lower music under speech by writing clip `gain` keyframes.
//!
//! Analysis never mutates the project. Voice activity is found by running the autocut
//! silence detector (same dry 16 kHz mono mix, same hysteresis) over the chosen voice
//! clips and tracks and taking its complement. Active regions closer than the minimum gap
//! (or than `attack + release`, where the music could not recover anyway) are merged.
//! Each region becomes a gain dip that completes its attack ramp when the voice starts
//! and starts its release ramp when the voice ends. The edit is one undoable
//! `Command::Batch` of `AddKeyframe` (plus a base `gain` of 1 where the clip has none and, with
//! `replace_existing`, `RemoveKeyframe`).

use crate::autocut::{SAMPLE_TICKS, revision, selection};
use crate::{AutocutParams, ControlError};
use animathio_core::AssetId;
use animathio_core::{
    ClipId, ClipKind, Command, History, Interpolation, Keyframe, ParamValue, Project,
    TICKS_PER_SECOND, Target, Time, TimeRange, TrackId,
};
use animathio_transcribe::prepare::{Selection, mix_selection};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Most voice regions one proposal may contain.
const MAX_REGIONS: usize = 2000;
/// Largest nested batch chunk (the core caps one batch at 1024 edits).
const CHUNK: usize = 1000;
const MS: i64 = TICKS_PER_SECOND / 1000;

/// Ducking options. All times are timeline ticks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DuckParams {
    /// Voice clips (a video clip resolves its linked audio companion).
    pub voice_clips: Vec<ClipId>,
    /// Voice tracks (their mixed audio).
    pub voice_tracks: Vec<TrackId>,
    /// Music tracks: every audio clip on them receives gain keyframes.
    pub music_tracks: Vec<TrackId>,
    /// How far the music drops under speech, in dB (0..=60; 12 means -12 dB).
    pub depth_db: f64,
    /// Ramp down, finishing when the voice starts.
    pub attack: Time,
    /// Ramp up, starting when the voice ends.
    pub release: Time,
    /// Voice regions separated by less than this are bridged into one dip.
    pub min_gap: Time,
    /// Voice is absent below this dBFS level (10 ms RMS windows).
    pub threshold_db: f64,
    /// Voice returns this many dB above the threshold.
    pub hysteresis_db: f64,
    /// Restrict voice analysis to this timeline range.
    pub range: Option<TimeRange>,
    /// Remove existing `gain` keyframes on a music clip before writing the duck.
    pub replace_existing: bool,
}

impl Default for DuckParams {
    fn default() -> Self {
        Self {
            voice_clips: Vec::new(),
            voice_tracks: Vec::new(),
            music_tracks: Vec::new(),
            depth_db: 12.0,
            attack: Time(300 * MS),
            release: Time(500 * MS),
            min_gap: Time(400 * MS),
            threshold_db: -40.0,
            hysteresis_db: 3.0,
            range: None,
            replace_existing: false,
        }
    }
}

/// One gain keyframe: clip-local time and the absolute linear clip gain.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DuckKey {
    /// Clip-local ticks.
    pub time: Time,
    /// Linear gain (the clip's own base gain times the ducking envelope).
    pub gain: f64,
}

/// The keyframes proposed for one music clip.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClipDuck {
    /// Music clip.
    pub clip: ClipId,
    /// Track holding it.
    pub track: TrackId,
    /// Ascending keyframes.
    pub keys: Vec<DuckKey>,
}

/// Snapshot-bound review draft.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DuckProposal {
    /// Content hash of the analysed project.
    pub revision: String,
    /// Voice activity after bridging short gaps.
    pub voice: Vec<TimeRange>,
    /// Timeline spans where the music is below full level (ramps included), clamped to
    /// the timeline start.
    pub ducked: Vec<TimeRange>,
    /// Per-clip keyframes; clips the envelope never touches are omitted.
    pub clips: Vec<ClipDuck>,
    /// Remove existing gain keyframes of the listed clips first.
    pub replace_existing: bool,
}

/// What an applied duck changed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuckResult {
    /// Music clips that received keyframes.
    pub clips: usize,
    /// Keyframes written.
    pub keyframes: usize,
    /// Timeline spans where the music is ducked.
    pub ducked: Vec<TimeRange>,
}

fn invalid(message: &str) -> ControlError {
    ControlError::InvalidParams(message.into())
}

fn validate(params: &DuckParams) -> Result<(), ControlError> {
    if !params.depth_db.is_finite() || !(0.0..=60.0).contains(&params.depth_db) {
        return Err(invalid("duck depth must be within 0..=60 dB"));
    }
    if !params.threshold_db.is_finite()
        || !(-120.0..=0.0).contains(&params.threshold_db)
        || !params.hysteresis_db.is_finite()
        || !(0.0..=24.0).contains(&params.hysteresis_db)
    {
        return Err(invalid("invalid voice threshold or hysteresis"));
    }
    let ramp = MS..=TICKS_PER_SECOND * 30;
    if !ramp.contains(&params.attack.0) || !ramp.contains(&params.release.0) {
        return Err(invalid("attack and release must be within 1 ms..=30 s"));
    }
    if params.min_gap.0 < 0 || params.min_gap.0 > TICKS_PER_SECOND * 60 {
        return Err(invalid("minimum gap must be within 0..=60 s"));
    }
    if let Some(range) = params.range {
        TimeRange::new(range.start, range.end)?;
    }
    if params.voice_clips.is_empty() && params.voice_tracks.is_empty() {
        return Err(invalid("choose a voice clip or track"));
    }
    if params.music_tracks.is_empty() {
        return Err(invalid("choose at least one music track"));
    }
    Ok(())
}

/// Voice-activity ranges of 16 kHz mono samples, found with the autocut silence detector
/// (so threshold and hysteresis behave identically) and inverted over the sample span.
pub fn detect_voice_samples(
    samples: &[f32],
    origin: Time,
    params: &DuckParams,
) -> Result<Vec<TimeRange>, ControlError> {
    let silence = AutocutParams {
        silence_db: params.threshold_db,
        hysteresis_db: params.hysteresis_db,
        min_silence: Time(1),
        keep_short_pauses: false,
        padding_before: Time(0),
        padding_after: Time(0),
        ..AutocutParams::default()
    };
    let silent = crate::detect_silent_samples(samples, origin, &silence)?;
    let span = i64::try_from(samples.len())
        .ok()
        .and_then(|n| n.checked_mul(SAMPLE_TICKS))
        .ok_or_else(|| invalid("audio too long"))?;
    let end = origin.0 + span;
    let mut voice = Vec::new();
    let mut cursor = origin.0;
    for range in silent {
        if range.start.0 > cursor {
            voice.push(TimeRange {
                start: Time(cursor),
                end: range.start,
            });
        }
        cursor = cursor.max(range.end.0);
    }
    if end > cursor {
        voice.push(TimeRange {
            start: Time(cursor),
            end: Time(end),
        });
    }
    Ok(voice)
}

/// Union overlapping voice ranges, then bridge gaps shorter than the larger of `min_gap`
/// and `attack + release` (the music has no time to recover in between).
#[must_use]
pub fn merge_voice(mut voice: Vec<TimeRange>, params: &DuckParams) -> Vec<TimeRange> {
    voice.retain(|r| r.end > r.start);
    voice.sort_by_key(|r| (r.start, r.end));
    let bridge = params
        .min_gap
        .0
        .max(params.attack.0.saturating_add(params.release.0));
    let mut merged: Vec<TimeRange> = Vec::new();
    for range in voice {
        match merged.last_mut() {
            Some(last) if range.start.0 - last.end.0 < bridge => {
                last.end = last.end.max(range.end);
            }
            _ => merged.push(range),
        }
    }
    merged
}

/// The ducking envelope as breakpoints `(timeline tick, linear gain)` for merged voice
/// regions; the gain is 1 before the first and after the last point.
#[must_use]
pub fn duck_envelope(voice: &[TimeRange], params: &DuckParams) -> Vec<(i64, f64)> {
    let floor = 10_f64.powf(-params.depth_db / 20.0);
    let mut points: Vec<(i64, f64)> = Vec::new();
    for range in voice {
        for point in [
            (range.start.0 - params.attack.0, 1.0),
            (range.start.0, floor),
            (range.end.0, floor),
            (range.end.0 + params.release.0, 1.0),
        ] {
            match points.last_mut() {
                // Adjoining ramps meet at the same tick: keep the deeper value.
                Some(last) if last.0 == point.0 => last.1 = last.1.min(point.1),
                _ => points.push(point),
            }
        }
    }
    points
}

fn envelope_at(points: &[(i64, f64)], tick: i64) -> f64 {
    let Some(first) = points.first() else {
        return 1.0;
    };
    if tick <= first.0 {
        return first.1;
    }
    let index = points.partition_point(|p| p.0 <= tick);
    if index == points.len() {
        return points[index - 1].1;
    }
    let (t0, g0) = points[index - 1];
    let (t1, g1) = points[index];
    #[allow(clippy::cast_precision_loss)] // Relative ticks become a blend fraction only.
    let fraction = (tick - t0) as f64 / (t1 - t0) as f64;
    g0 + (g1 - g0) * fraction
}

fn base_gain(clip: &animathio_core::Clip) -> f64 {
    clip.params
        .get("gain")
        .and_then(ParamValue::as_number)
        .unwrap_or(1.0)
}

fn has_gain_animation(clip: &animathio_core::Clip) -> bool {
    clip.animations
        .iter()
        .any(|a| matches!(&a.target, Target::Param(n) if n == "gain"))
}

fn clip_keys(clip: &animathio_core::Clip, points: &[(i64, f64)]) -> Vec<DuckKey> {
    let (start, end) = (clip.range.start.0, clip.range.end.0);
    // Dips whose ramps lie entirely outside the clip leave it untouched.
    let touched = points
        .windows(2)
        .any(|w| w[0].0 < end && w[1].0 > start && (w[0].1 < 1.0 || w[1].1 < 1.0));
    if !touched {
        return Vec::new();
    }
    let base = base_gain(clip);
    let mut ticks: Vec<i64> = vec![start, end];
    ticks.extend(
        points
            .iter()
            .map(|p| p.0)
            .filter(|t| *t > start && *t < end),
    );
    ticks.sort_unstable();
    ticks.dedup();
    ticks
        .into_iter()
        .map(|tick| DuckKey {
            time: Time(tick - start),
            gain: base * envelope_at(points, tick),
        })
        .collect()
}

fn audio_end(project: &Project, selection: Selection) -> Time {
    project
        .tracks()
        .iter()
        .filter(|t| selection.track.is_none_or(|id| id == t.id()))
        .flat_map(|t| t.clips().values())
        .filter(|c| {
            matches!(c.kind, ClipKind::Audio(_)) && selection.clip.is_none_or(|id| id == c.id)
        })
        .map(|c| c.range.end)
        .max()
        .unwrap_or_default()
}

/// Merged voice regions of every voice clip and track.
fn voice_regions(
    project: &Project,
    assets: &BTreeMap<AssetId, Vec<u8>>,
    params: &DuckParams,
) -> Result<Vec<TimeRange>, ControlError> {
    let mut sources = Vec::new();
    for id in &params.voice_clips {
        let base = AutocutParams {
            clip: Some(*id),
            ..AutocutParams::default()
        };
        let resolved = selection(project, &base)?;
        let owner = project
            .tracks()
            .iter()
            .find(|t| resolved.clip.is_some_and(|c| t.clips().contains_key(&c)))
            .map(animathio_core::Track::id);
        if owner.is_some_and(|t| params.music_tracks.contains(&t)) {
            return Err(invalid("a voice clip is on a music track"));
        }
        sources.push(Selection {
            range: params.range,
            ..resolved
        });
    }
    for id in &params.voice_tracks {
        if !project.tracks().iter().any(|t| t.id() == *id) {
            return Err(invalid("voice track missing"));
        }
        sources.push(Selection {
            clip: None,
            track: Some(*id),
            range: params.range,
        });
    }
    let mut voice = Vec::new();
    for source in sources {
        let (samples, origin) = mix_selection(project, assets, source)?;
        let end = audio_end(project, source);
        let end = source.range.map_or(end, |r| end.min(r.end));
        for mut range in detect_voice_samples(&samples, origin, params)? {
            range.end = range.end.min(end);
            if range.end > range.start {
                voice.push(range);
            }
        }
    }
    Ok(merge_voice(voice, params))
}

/// Analyse voice and music and return a review draft. No mutation; run it on a worker
/// in a GUI.
pub fn propose_duck(
    project: &Project,
    assets: &BTreeMap<AssetId, Vec<u8>>,
    params: &DuckParams,
) -> Result<DuckProposal, ControlError> {
    validate(params)?;
    for id in &params.music_tracks {
        let track = project
            .tracks()
            .iter()
            .find(|t| t.id() == *id)
            .ok_or_else(|| invalid("music track missing"))?;
        if track.is_locked() {
            return Err(invalid("music track is locked"));
        }
        if params.voice_tracks.contains(id) {
            return Err(invalid("a track cannot be both voice and music"));
        }
    }
    let voice = voice_regions(project, assets, params)?;
    if voice.len() > MAX_REGIONS {
        return Err(invalid("too many voice regions; analyse a shorter range"));
    }
    let points = duck_envelope(&voice, params);
    let mut ducked: Vec<TimeRange> = voice
        .iter()
        .map(|r| TimeRange {
            start: Time((r.start.0 - params.attack.0).max(0)),
            end: Time(r.end.0 + params.release.0),
        })
        .collect();
    ducked.sort_by_key(|r| r.start);
    let mut clips = Vec::new();
    let mut existing = Vec::new();
    for track in project
        .tracks()
        .iter()
        .filter(|t| params.music_tracks.contains(&t.id()))
    {
        for clip in track
            .clips()
            .values()
            .filter(|c| matches!(c.kind, ClipKind::Audio(_)))
        {
            let keys = clip_keys(clip, &points);
            if keys.is_empty() {
                continue;
            }
            if has_gain_animation(clip) && !params.replace_existing {
                existing.push(clip.id.0.to_string());
            }
            clips.push(ClipDuck {
                clip: clip.id,
                track: track.id(),
                keys,
            });
        }
    }
    if !existing.is_empty() {
        return Err(invalid(&format!(
            "music clip(s) {} already have gain keyframes; enable replace_existing to overwrite them",
            existing.join(", ")
        )));
    }
    Ok(DuckProposal {
        revision: revision(project)?,
        voice,
        ducked,
        clips,
        replace_existing: params.replace_existing,
    })
}

/// Build the single undoable batch for a proposal. Stale drafts and keyframes that do
/// not fit their clip are rejected before any mutation.
pub fn duck_command(project: &Project, proposal: &DuckProposal) -> Result<Command, ControlError> {
    if proposal.revision != revision(project)? {
        return Err(invalid("project changed; analyse again before applying"));
    }
    let gain = Target::Param("gain".into());
    let mut commands = Vec::new();
    for duck in &proposal.clips {
        let clip = project
            .tracks()
            .iter()
            .filter(|t| t.id() == duck.track)
            .find_map(|t| t.clips().get(&duck.clip))
            .ok_or_else(|| invalid("music clip missing"))?;
        if !matches!(clip.kind, ClipKind::Audio(_)) {
            return Err(invalid("ducking applies to audio clips only"));
        }
        if duck.keys.is_empty()
            || duck.keys.windows(2).any(|w| w[0].time >= w[1].time)
            || duck
                .keys
                .iter()
                .any(|k| !k.gain.is_finite() || k.gain < 0.0 || k.time.0 < 0)
        {
            return Err(invalid("invalid duck keyframes"));
        }
        if let Some(animation) = clip.animations.iter().find(|a| a.target == gain) {
            if !proposal.replace_existing {
                return Err(invalid("music clip already has gain keyframes"));
            }
            commands.extend(
                animation
                    .keys
                    .keys()
                    .keys()
                    .map(|time| Command::RemoveKeyframe {
                        id: clip.id,
                        target: gain.clone(),
                        time: *time,
                    }),
            );
        }
        // A parameter can only be animated when the clip holds a base value for it.
        if !clip.params.contains_key("gain") {
            commands.push(Command::SetParam {
                id: clip.id,
                name: "gain".into(),
                value: Some(1.0),
            });
        }
        commands.extend(duck.keys.iter().map(|key| Command::AddKeyframe {
            id: clip.id,
            target: gain.clone(),
            time: key.time,
            keyframe: Keyframe {
                value: ParamValue::Number(key.gain),
                interpolation: Interpolation::Linear,
            },
        }));
    }
    while commands.len() > CHUNK {
        commands = commands
            .chunks(CHUNK)
            .map(|chunk| Command::Batch {
                commands: chunk.to_vec(),
            })
            .collect();
    }
    Ok(Command::Batch { commands })
}

/// Apply a reviewed proposal as one history entry.
pub fn apply_duck(
    project: &mut Project,
    history: &mut History,
    proposal: &DuckProposal,
) -> Result<DuckResult, ControlError> {
    let command = duck_command(project, proposal)?;
    let keyframes = proposal.clips.iter().map(|c| c.keys.len()).sum();
    if !proposal.clips.is_empty() {
        history.execute(project, &command)?;
    }
    Ok(DuckResult {
        clips: proposal.clips.len(),
        keyframes,
        ducked: proposal.ducked.clone(),
    })
}
