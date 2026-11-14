//! Beat analysis and beat/marker sync. Analysis never mutates the project; every action
//! builds one `Command::Batch` (a single undo step) from the analysed beat times.

use crate::{ControlError, SourceLimits, autocut::resolve_selection};
use animathio_audio::{BeatParams, detect_beats, downbeat_indices};
use animathio_core::{
    AssetId, Clip, ClipId, Command, FrameRate, Marker, MarkerColor, Project, Rounding,
    TICKS_PER_SECOND, Time, TimeRange, TrackId,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Largest number of commands in one batch layer; longer lists nest in chunks.
const CHUNK: usize = 512;
/// Most clips one sync action may touch.
const MAX_CLIPS: usize = 10_000;

fn invalid(message: impl Into<String>) -> ControlError {
    ControlError::InvalidParams(message.into())
}

/// Which audio to analyse and how. With no selection the whole project mix is analysed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BeatDetectParams {
    /// Analyse one clip (a video clip resolves to its linked audio companion).
    pub clip: Option<ClipId>,
    /// Analyse one track's mixed audio.
    pub track: Option<TrackId>,
    /// Restrict analysis to this timeline range.
    pub range: Option<TimeRange>,
    /// Slowest tempo considered.
    pub min_bpm: f64,
    /// Fastest tempo considered.
    pub max_bpm: f64,
    /// Force this tempo instead of estimating it.
    pub bpm: Option<f64>,
    /// Beats per bar used to pick downbeats (default 4).
    pub beats_per_bar: u32,
}
impl Default for BeatDetectParams {
    fn default() -> Self {
        let d = BeatParams::default();
        Self {
            clip: None,
            track: None,
            range: None,
            min_bpm: d.min_bpm,
            max_bpm: d.max_bpm,
            bpm: None,
            beats_per_bar: 4,
        }
    }
}

/// One detected beat on the timeline.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BeatInfo {
    /// Timeline position in ticks.
    pub time: Time,
    /// Onset strength at the beat (standard deviations of the onset envelope).
    pub strength: f32,
}

/// Result of beat analysis, in timeline coordinates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BeatDetection {
    /// Tempo in beats per minute.
    pub bpm: f64,
    /// Strength of the tempo periodicity, `0..=1`.
    pub confidence: f64,
    /// Beats in ascending time order.
    pub beats: Vec<BeatInfo>,
    /// Indices into `beats` that are downbeats for the requested beats per bar.
    pub downbeats: Vec<usize>,
}

/// Analyse the selected (or whole-project) audio for tempo and beats. Heavy: run it on a
/// worker thread in a GUI. Deterministic: the same project and assets give the same beats.
pub fn analyze_beats(
    project: &Project,
    assets: &BTreeMap<AssetId, Vec<u8>>,
    params: &BeatDetectParams,
) -> Result<BeatDetection, ControlError> {
    if params.beats_per_bar == 0 || params.beats_per_bar > 32 {
        return Err(invalid("beats_per_bar must be 1..=32"));
    }
    let selection = resolve_selection(project, params.clip, params.track, params.range)?;
    let (samples, origin) = animathio_transcribe::mix_selection(project, assets, selection)
        .map_err(|e| invalid(e.to_string().replace("to transcribe", "to analyse")))?;
    let analysis = detect_beats(
        &samples,
        animathio_transcribe::prepare::SAMPLE_RATE,
        &BeatParams {
            min_bpm: params.min_bpm,
            max_bpm: params.max_bpm,
            bpm: params.bpm,
            ..BeatParams::default()
        },
    )
    .map_err(|e| invalid(e.to_string()))?;
    let beats: Vec<BeatInfo> = analysis
        .beats
        .iter()
        .map(|b| BeatInfo {
            time: Time(origin.0.saturating_add(b.time.0)),
            strength: b.strength,
        })
        .collect();
    let downbeats = downbeat_indices(&analysis.beats, params.beats_per_bar as usize);
    Ok(BeatDetection {
        bpm: analysis.bpm,
        confidence: analysis.confidence,
        beats,
        downbeats,
    })
}

/// Which beats to use: every Nth, or downbeats only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BeatSelection {
    /// Use every Nth beat, starting with the first (default 1 = every beat).
    pub every: u32,
    /// Use only downbeats (ignores `every`).
    pub downbeats_only: bool,
}
impl Default for BeatSelection {
    fn default() -> Self {
        Self {
            every: 1,
            downbeats_only: false,
        }
    }
}

/// Beats picked by `selection`, with their index in `detection.beats`.
pub fn select_beats(
    detection: &BeatDetection,
    selection: BeatSelection,
) -> Result<Vec<(usize, Time)>, ControlError> {
    if selection.every == 0 {
        return Err(invalid("every must be at least 1"));
    }
    Ok(if selection.downbeats_only {
        detection
            .downbeats
            .iter()
            .map(|i| (*i, detection.beats[*i].time))
            .collect()
    } else {
        detection
            .beats
            .iter()
            .enumerate()
            .step_by(selection.every as usize)
            .map(|(i, b)| (i, b.time))
            .collect()
    })
}

/// Chunk commands so every batch stays well under the core batch limit.
fn batch(mut commands: Vec<Command>) -> Command {
    while commands.len() > CHUNK {
        commands = commands
            .chunks(CHUNK)
            .map(|chunk| Command::Batch {
                commands: chunk.to_vec(),
            })
            .collect();
    }
    Command::Batch { commands }
}

/// How "add markers at beats" names and colours its markers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BeatMarkerOptions {
    /// Beat subset.
    #[serde(flatten)]
    pub selection: BeatSelection,
    /// Marker colour.
    pub color: MarkerColor,
    /// Name prefix; markers are named `<prefix> <n>` (n counts the beats, or bars).
    pub name_prefix: Option<String>,
}
impl Default for BeatMarkerOptions {
    fn default() -> Self {
        Self {
            selection: BeatSelection::default(),
            color: MarkerColor::Cyan,
            name_prefix: None,
        }
    }
}

/// One undoable batch adding a marker at each selected beat. Beats that already carry a
/// marker at exactly the same time are skipped, so repeating the action is idempotent.
/// Returns the command and the number of markers it adds.
pub fn markers_at_beats_command(
    project: &Project,
    detection: &BeatDetection,
    options: &BeatMarkerOptions,
) -> Result<(Command, usize), ControlError> {
    let picked = select_beats(detection, options.selection)?;
    let existing: BTreeSet<Time> = project.markers().iter().map(|m| m.time).collect();
    let prefix = options.name_prefix.clone().unwrap_or_else(|| {
        if options.selection.downbeats_only {
            "Bar".into()
        } else {
            "Beat".into()
        }
    });
    let mut next = project.next_marker_id().0;
    let mut commands = Vec::new();
    for (n, (_, time)) in picked.iter().enumerate() {
        if existing.contains(time) || time.0 < 0 {
            continue;
        }
        let mut marker = Marker::new(animathio_core::MarkerId(next), *time);
        next += 1;
        marker.color = options.color;
        marker.name = format!("{prefix} {}", n + 1);
        commands.push(Command::AddMarker { marker });
    }
    if commands.is_empty() {
        return Err(invalid("no new markers to add"));
    }
    let count = commands.len();
    if count + project.markers().len() > animathio_core::MAX_MARKERS {
        return Err(invalid("too many markers"));
    }
    Ok((batch(commands), count))
}

/// What to snap cut points to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncSource {
    /// Detected beats.
    #[default]
    Beats,
    /// Project markers (the start and, for ranges, the end of each).
    Markers,
    /// Beats and markers together.
    Both,
}
impl SyncSource {
    /// Whether audio analysis is needed.
    #[must_use]
    pub const fn needs_beats(self) -> bool {
        !matches!(self, Self::Markers)
    }
}

/// What to do with the selected clips.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncMode {
    /// Move existing cut points (clip edges) to the nearest target, as roll edits.
    #[default]
    Snap,
    /// Split long clips into segments at the targets inside them.
    Split,
}

/// Arguments for beat/marker cut sync.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SyncCutsParams {
    /// Clips whose cut points (or span, for `split`) are edited. Linked companions follow.
    pub clips: Vec<ClipId>,
    /// Targets to sync to.
    pub source: SyncSource,
    /// Snap cut points or split clips.
    pub mode: SyncMode,
    /// Largest distance a cut point may move, in ticks (default 250 ms).
    pub max_shift: Time,
    /// Smallest segment `split` may create, in ticks (default 100 ms).
    pub min_segment: Time,
    /// Round every edit to the project's frame grid (default true).
    pub frame_align: bool,
    /// Beat subset (every Nth beat or downbeats only).
    #[serde(flatten)]
    pub selection: BeatSelection,
    /// Audio analysis settings, used when `source` includes beats.
    pub detect: BeatDetectParams,
}
impl Default for SyncCutsParams {
    fn default() -> Self {
        Self {
            clips: Vec::new(),
            source: SyncSource::Beats,
            mode: SyncMode::Snap,
            max_shift: Time(TICKS_PER_SECOND / 4),
            min_segment: Time(TICKS_PER_SECOND / 10),
            frame_align: true,
            selection: BeatSelection::default(),
            detect: BeatDetectParams::default(),
        }
    }
}

/// One cut point that moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CutShift {
    /// Track holding the cut.
    pub track: TrackId,
    /// Clip ending at the cut, if any.
    pub left: Option<ClipId>,
    /// Clip starting at the cut, if any.
    pub right: Option<ClipId>,
    /// Original position.
    pub from: Time,
    /// New position.
    pub to: Time,
}

/// What a sync action did.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SyncCutsResult {
    /// Cut points that moved (`snap`).
    pub shifted: Vec<CutShift>,
    /// Split points added (`split`).
    pub splits: Vec<Time>,
    /// Cut points left alone because no target was in range.
    pub unchanged: usize,
    /// Cut points skipped because the edit was invalid (overlap, locked track, ...).
    pub skipped: Vec<String>,
    /// Detected tempo when beats were analysed.
    pub bpm: Option<f64>,
}

fn marker_times(project: &Project) -> Vec<Time> {
    project
        .markers()
        .iter()
        .flat_map(|m| {
            let end = m.duration.map(|_| m.end());
            std::iter::once(m.time).chain(end)
        })
        .collect()
}

fn snap_to_frame(rate: FrameRate, time: Time) -> Time {
    rate.time_to_frame(time, Rounding::Nearest)
        .and_then(|f| rate.frame_to_time(f, Rounding::Nearest))
        .unwrap_or(time)
}

/// Nearest target to `time` within `max_shift`, earlier on ties.
fn nearest(targets: &[Time], time: Time, max_shift: Time) -> Option<Time> {
    let at = targets.partition_point(|t| *t < time);
    [at.checked_sub(1), Some(at)]
        .into_iter()
        .flatten()
        .filter_map(|i| targets.get(i).copied())
        .map(|t| (t.0.abs_diff(time.0), t))
        .filter(|(d, _)| *d <= max_shift.0.unsigned_abs())
        .min()
        .map(|(_, t)| t)
}

fn find_clip(project: &Project, id: ClipId) -> Option<(TrackId, &Clip)> {
    project
        .tracks()
        .iter()
        .find_map(|t| t.clips().get(&id).map(|c| (t.id(), c)))
}

/// Selected clips deduplicated to one per link group (companions follow their primary).
fn representatives(project: &Project, clips: &[ClipId]) -> Result<Vec<ClipId>, ControlError> {
    if clips.is_empty() {
        return Err(invalid("select at least one clip"));
    }
    if clips.len() > MAX_CLIPS {
        return Err(invalid("too many clips"));
    }
    let mut groups = BTreeSet::new();
    let mut out = Vec::new();
    for id in clips {
        let (_, clip) =
            find_clip(project, *id).ok_or_else(|| invalid(format!("clip {} missing", id.0)))?;
        if clip.link_group.is_none_or(|g| groups.insert(g)) && !out.contains(id) {
            out.push(*id);
        }
    }
    Ok(out)
}

/// Build one undoable batch syncing the selected clips to `beats` and/or markers.
/// `beats` are the already-selected beat times (see [`select_beats`]); pass none for the
/// `markers` source. Edits that would be invalid (overlaps, locked tracks, collapsed
/// clips) are skipped and reported rather than failing the whole action.
pub fn sync_cuts_command(
    project: &Project,
    params: &SyncCutsParams,
    beats: &[Time],
    bpm: Option<f64>,
    limits: &SourceLimits,
) -> Result<(Command, SyncCutsResult), ControlError> {
    if params.max_shift.0 < 0 || params.min_segment.0 < 0 {
        return Err(invalid("max_shift and min_segment must be nonnegative"));
    }
    let mut targets: Vec<Time> = Vec::new();
    if params.source.needs_beats() {
        targets.extend_from_slice(beats);
    }
    if !matches!(params.source, SyncSource::Beats) {
        targets.extend(marker_times(project));
    }
    targets.retain(|t| t.0 >= 0);
    if params.frame_align {
        let rate = project.frame_rate();
        for t in &mut targets {
            *t = snap_to_frame(rate, *t);
        }
    }
    targets.sort_unstable();
    targets.dedup();
    if targets.is_empty() {
        return Err(invalid("no beats or markers to sync to"));
    }
    let reps = representatives(project, &params.clips)?;
    let mut scratch = project.clone();
    let mut commands = Vec::new();
    let mut result = SyncCutsResult {
        bpm,
        ..SyncCutsResult::default()
    };
    match params.mode {
        SyncMode::Snap => snap_cuts(
            &reps,
            &targets,
            params.max_shift,
            limits,
            &mut scratch,
            &mut commands,
            &mut result,
        ),
        SyncMode::Split => split_at_targets(
            &reps,
            &targets,
            params.min_segment,
            &mut scratch,
            &mut commands,
            &mut result,
        ),
    }
    if commands.is_empty() {
        let why = if result.skipped.is_empty() {
            "nothing to change: no cut point is within range of a beat or marker".to_owned()
        } else {
            format!("no valid edits: {}", result.skipped.join("; "))
        };
        return Err(invalid(why));
    }
    Ok((batch(commands), result))
}

fn snap_cuts(
    reps: &[ClipId],
    targets: &[Time],
    max_shift: Time,
    limits: &SourceLimits,
    scratch: &mut Project,
    commands: &mut Vec<Command>,
    result: &mut SyncCutsResult,
) {
    // Cut points keyed by (track, time), in the original coordinates; adjacent clips share one.
    let mut points: BTreeMap<(TrackId, Time), (Option<ClipId>, Option<ClipId>)> = BTreeMap::new();
    for id in reps {
        let Some((track, clip)) = find_clip(scratch, *id) else {
            continue;
        };
        let neighbours: Vec<&Clip> = scratch
            .tracks()
            .iter()
            .find(|t| t.id() == track)
            .map_or_default(|t| t.clips().values().collect());
        let before = neighbours
            .iter()
            .find(|c| c.id != clip.id && c.range.end == clip.range.start)
            .map(|c| c.id);
        let after = neighbours
            .iter()
            .find(|c| c.id != clip.id && c.range.start == clip.range.end)
            .map(|c| c.id);
        points.entry((track, clip.range.start)).or_default().0 = before;
        points.entry((track, clip.range.start)).or_default().1 = Some(clip.id);
        points.entry((track, clip.range.end)).or_default().0 = Some(clip.id);
        points.entry((track, clip.range.end)).or_default().1 = after;
    }
    for ((track, from), (left, right)) in points {
        let Some(to) = nearest(targets, from, max_shift) else {
            result.unchanged += 1;
            continue;
        };
        if to == from {
            result.unchanged += 1;
            continue;
        }
        let mut edit = Vec::new();
        let mut failure = None;
        if let Some(id) = left
            && let Some((_, c)) = find_clip(scratch, id)
        {
            match TimeRange::new(c.range.start, to) {
                Ok(range) => edit.push(Command::TrimClip {
                    id,
                    range,
                    source_offset: c.source_offset,
                }),
                Err(e) => failure = Some(e.to_string()),
            }
        }
        if let Some(id) = right
            && let Some((_, c)) = find_clip(scratch, id)
        {
            match TimeRange::new(to, c.range.end) {
                Ok(range) => edit.push(Command::TrimClip {
                    id,
                    range,
                    source_offset: Time(c.source_offset.0 + (to.0 - from.0)),
                }),
                Err(e) => failure = Some(e.to_string()),
            }
        }
        let attempt = Command::Batch { commands: edit };
        let touched: Vec<ClipId> = left.into_iter().chain(right).collect();
        let snapshot = SourceLimits::snapshot(scratch, &touched);
        let outcome = match failure {
            Some(message) => Err(message),
            // An edit that would read past the end of the source media cannot fit: undo it
            // on the scratch project and report it instead of emitting it.
            None => attempt.apply(scratch).map_err(|e| e.0).and_then(|inverse| {
                limits.verify(scratch, &snapshot).inspect_err(|_| {
                    let _ = inverse.apply(scratch);
                })
            }),
        };
        match outcome {
            Ok(()) => {
                result.shifted.push(CutShift {
                    track,
                    left,
                    right,
                    from,
                    to,
                });
                if let Command::Batch { commands: edit } = attempt {
                    commands.extend(edit);
                }
            }
            Err(message) => result.skipped.push(format!(
                "cut at {} ticks on track {}: {message}",
                from.0, track.0
            )),
        }
    }
}

fn split_at_targets(
    reps: &[ClipId],
    targets: &[Time],
    min_segment: Time,
    scratch: &mut Project,
    commands: &mut Vec<Command>,
    result: &mut SyncCutsResult,
) {
    for id in reps {
        let Some((_, clip)) = find_clip(scratch, *id) else {
            continue;
        };
        let (start, end) = (clip.range.start, clip.range.end);
        // Keep only targets leaving a long enough segment on both sides of each split.
        let mut picked: Vec<Time> = Vec::new();
        for t in targets {
            if t.0 - start.0 < min_segment.0.max(1) || end.0 - t.0 < min_segment.0.max(1) {
                continue;
            }
            if picked
                .last()
                .is_some_and(|last| t.0 - last.0 < min_segment.0)
            {
                continue;
            }
            picked.push(*t);
        }
        // Latest first: the original id always stays the left part.
        for at in picked.into_iter().rev() {
            let right_id = ClipId(
                scratch
                    .tracks()
                    .iter()
                    .flat_map(|t| t.clips().keys())
                    .map(|c| c.0)
                    .max()
                    .map_or(1, |m| m + 1),
            );
            let command = Command::SplitClip {
                id: *id,
                right_id,
                at,
            };
            match command.apply(scratch) {
                Ok(_) => {
                    commands.push(command);
                    result.splits.push(at);
                }
                Err(e) => result
                    .skipped
                    .push(format!("split of clip {} at {} ticks: {}", id.0, at.0, e.0)),
            }
        }
    }
    result.splits.sort_unstable();
}

/// Arguments for `markers.add`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MarkerAddParams {
    /// Position in ticks.
    pub time: Time,
    /// Length of a range marker; none for a point marker.
    pub duration: Option<Time>,
    /// Display name.
    pub name: String,
    /// Palette colour.
    pub color: MarkerColor,
    /// Free-form note.
    pub note: String,
}
impl Default for MarkerAddParams {
    fn default() -> Self {
        Self {
            time: Time(0),
            duration: None,
            name: String::new(),
            color: MarkerColor::default(),
            note: String::new(),
        }
    }
}

/// Result of `markers.add`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkerAdded {
    /// Identity of the new marker.
    pub id: animathio_core::MarkerId,
}

/// Arguments for `beats.add_markers`: analysis selection plus marker options.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BeatMarkersParams {
    /// Audio analysis settings.
    #[serde(flatten)]
    pub detect: BeatDetectParams,
    /// Beat subset, colour and naming.
    #[serde(flatten)]
    pub options: BeatMarkerOptions,
}

/// Result of `beats.add_markers`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BeatMarkersResult {
    /// Markers added.
    pub added: usize,
    /// Detected tempo.
    pub bpm: f64,
    /// Tempo confidence, `0..=1`.
    pub confidence: f64,
}

/// Build the command adding one marker, validating it against the project.
pub fn add_marker_command(
    project: &Project,
    params: &MarkerAddParams,
) -> Result<(Command, animathio_core::MarkerId), ControlError> {
    let id = project.next_marker_id();
    let mut marker = Marker::new(id, params.time);
    marker.duration = params.duration;
    marker.name.clone_from(&params.name);
    marker.color = params.color;
    marker.note.clone_from(&params.note);
    let command = Command::AddMarker { marker };
    command.apply(&mut project.clone())?;
    Ok((command, id))
}

/// Add markers at the detected beats as one undo step.
pub fn apply_beat_markers(
    project: &mut Project,
    history: &mut animathio_core::History,
    detection: &BeatDetection,
    options: &BeatMarkerOptions,
) -> Result<BeatMarkersResult, ControlError> {
    let (command, added) = markers_at_beats_command(project, detection, options)?;
    history.execute(project, &command)?;
    Ok(BeatMarkersResult {
        added,
        bpm: detection.bpm,
        confidence: detection.confidence,
    })
}

/// Sync the selected clips' cuts to beats and/or markers as one undo step.
/// `detection` is required when `params.source` includes beats.
pub fn apply_sync_cuts(
    project: &mut Project,
    history: &mut animathio_core::History,
    detection: Option<&BeatDetection>,
    params: &SyncCutsParams,
    limits: &SourceLimits,
) -> Result<SyncCutsResult, ControlError> {
    let beats = match (params.source.needs_beats(), detection) {
        (true, Some(d)) => select_beats(d, params.selection)?
            .into_iter()
            .map(|(_, t)| t)
            .collect(),
        (true, None) => return Err(invalid("beat analysis is required for this source")),
        (false, _) => Vec::new(),
    };
    let (command, result) =
        sync_cuts_command(project, params, &beats, detection.map(|d| d.bpm), limits)?;
    history.execute(project, &command)?;
    Ok(result)
}
