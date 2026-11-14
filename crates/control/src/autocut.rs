//! Reviewable automatic cuts. Analysis never mutates the project; application is
//! one batch of scoped ripple edits, guarded against a stale project snapshot.

use crate::{ControlError, TrimSilenceResult};
use animathio_core::{
    AssetId, ClipId, ClipKind, Command, History, Project, TICKS_PER_SECOND, Time, TimeRange,
    TrackId,
};
use animathio_transcribe::{ModelSize, TranscribeOptions, Word, prepare::Selection};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, atomic::AtomicBool},
};

const RATE: i64 = 16_000;
pub(crate) const SAMPLE_TICKS: i64 = TICKS_PER_SECOND / RATE;
const WINDOW: usize = 160;
const MAX_CUTS: usize = 1000;

/// Window level used by silence detection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SilenceMetric {
    /// Root mean square energy.
    #[default]
    Rms,
    /// Maximum absolute sample amplitude.
    Peak,
}

/// Analysis options. Times are timeline ticks; no edits happen during analysis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[allow(clippy::struct_excessive_bools)]
pub struct AutocutParams {
    /// Enter silence below this dBFS level.
    pub silence_db: f64,
    /// Leave silence this many dB above the entry level.
    pub hysteresis_db: f64,
    /// RMS or peak, measured over 10 ms windows.
    pub metric: SilenceMetric,
    /// Minimum raw pause length when `keep_short_pauses` is true.
    pub min_silence: Time,
    /// Preserve pauses shorter than `min_silence` (default true).
    pub keep_short_pauses: bool,
    /// Silence retained before the next speech onset.
    pub padding_before: Time,
    /// Silence retained after the preceding speech ends.
    pub padding_after: Time,
    /// Analyse a clip (video selects its linked audio companion).
    pub clip: Option<ClipId>,
    /// Analyse one track's mixed audio.
    pub track: Option<TrackId>,
    /// Restrict analysis to this timeline range.
    pub range: Option<TimeRange>,
    /// Cut all unlocked tracks; otherwise cut the selected tracks and their companions.
    pub all_unlocked_tracks: bool,
    /// Include filler and immediate repeat/false-start suggestions (requires Whisper).
    pub fillers: bool,
    /// Language code; None lets Whisper detect it.
    pub language: Option<String>,
    /// Override language defaults with these filler phrases; empty uses defaults.
    pub filler_words: Vec<String>,
    /// Local Whisper model name.
    pub model: String,
    /// Explicit consent to download a missing model.
    pub download_model: bool,
}
impl Default for AutocutParams {
    fn default() -> Self {
        Self {
            silence_db: -40.0,
            hysteresis_db: 3.0,
            metric: SilenceMetric::Rms,
            min_silence: Time(TICKS_PER_SECOND / 2),
            keep_short_pauses: true,
            padding_before: Time(TICKS_PER_SECOND / 10),
            padding_after: Time(TICKS_PER_SECOND / 10),
            clip: None,
            track: None,
            range: None,
            all_unlocked_tracks: false,
            fillers: false,
            language: None,
            filler_words: Vec::new(),
            model: "base".into(),
            download_model: false,
        }
    }
}

/// Why a range is proposed. Speech confidence is heuristic, not a model probability.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutocutReason {
    /// A silent pause.
    Silence,
    /// A configured filler phrase.
    Filler,
    /// An immediate repetition or a hyphen-marked false start.
    Repeat,
}
/// One reviewable cut in pre-edit timeline coordinates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Autocut {
    /// Interval to remove.
    pub range: TimeRange,
    /// Detection category.
    pub reason: AutocutReason,
    /// Heuristic confidence, 0..=1.
    pub confidence: f64,
    /// Toggle without changing the original coordinates of other cuts.
    pub enabled: bool,
}
/// Snapshot-bound review draft. Clients may toggle cuts before applying.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutocutProposal {
    /// Content hash of the project that was analysed.
    pub revision: String,
    /// Tracks cut and rippled together.
    pub tracks: Vec<TrackId>,
    /// Ascending, nonoverlapping proposed intervals.
    pub cuts: Vec<Autocut>,
    /// Duration saved if every proposal is enabled.
    pub total_saved: Time,
}

fn invalid(message: &str) -> ControlError {
    ControlError::InvalidParams(message.into())
}
pub(crate) fn revision(project: &Project) -> Result<String, ControlError> {
    Ok(AssetId::of(&serde_json::to_vec(project)?)
        .as_str()
        .to_owned())
}
fn validate(params: &AutocutParams) -> Result<(), ControlError> {
    if !params.silence_db.is_finite()
        || !(-120.0..=0.0).contains(&params.silence_db)
        || !params.hysteresis_db.is_finite()
        || !(0.0..=24.0).contains(&params.hysteresis_db)
        || params.min_silence.0 <= 0
        || params.padding_before.0 < 0
        || params.padding_after.0 < 0
        || params.padding_before.0 > TICKS_PER_SECOND * 60
        || params.padding_after.0 > TICKS_PER_SECOND * 60
    {
        return Err(invalid(
            "invalid silence threshold, hysteresis, minimum silence or padding (maximum 60 seconds)",
        ));
    }
    if let Some(range) = params.range {
        TimeRange::new(range.start, range.end)?;
    }
    Ok(())
}

/// Pure deterministic 16 kHz mono detection. Hysteresis avoids chattering near the
/// threshold; padding is applied after measuring each raw silent run.
pub fn detect_silent_samples(
    samples: &[f32],
    origin: Time,
    params: &AutocutParams,
) -> Result<Vec<TimeRange>, ControlError> {
    validate(params)?;
    if origin.0 < 0 || samples.iter().any(|s| !s.is_finite()) {
        return Err(invalid("invalid audio samples or origin"));
    }
    let duration = i64::try_from(samples.len())
        .ok()
        .and_then(|n| n.checked_mul(SAMPLE_TICKS))
        .ok_or_else(|| invalid("audio too long"))?;
    let end = origin
        .0
        .checked_add(duration)
        .ok_or_else(|| invalid("audio too long"))?;
    let enter = 10_f64.powf(params.silence_db / 20.0);
    let leave = 10_f64.powf((params.silence_db + params.hysteresis_db) / 20.0);
    let mut run = None;
    let mut ranges = Vec::new();
    let mut push = |start: i64, finish: i64| {
        if params.keep_short_pauses && finish - start < params.min_silence.0 {
            return;
        }
        let from = start.saturating_add(params.padding_after.0);
        let to = finish.saturating_sub(params.padding_before.0);
        if to > from {
            ranges.push(TimeRange {
                start: Time(from),
                end: Time(to),
            });
        }
    };
    for (i, window) in samples.chunks(WINDOW).enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let level = match params.metric {
            SilenceMetric::Rms => (window.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>()
                / window.len() as f64)
                .sqrt(),
            SilenceMetric::Peak => window
                .iter()
                .map(|s| f64::from(s.abs()))
                .fold(0.0, f64::max),
        };
        let tick = origin.0 + i64::try_from(i * WINDOW).unwrap_or(i64::MAX) * SAMPLE_TICKS;
        if run.is_none() && level < enter {
            run = Some(tick);
        }
        if level >= leave
            && let Some(start) = run.take()
        {
            push(start, tick);
        }
    }
    if let Some(start) = run {
        push(start, end);
    }
    Ok(ranges)
}

fn normalise(text: &str) -> String {
    text.trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase()
}
/// Detect configured multi-word fillers and immediate repeated words / hyphenated
/// false starts from synthetic or recognised word timestamps. No engine is needed.
#[must_use]
pub fn detect_filler_words(words: &[Word], language: &str, configured: &[String]) -> Vec<Autocut> {
    let defaults = match language {
        "hu" => vec!["öö", "ö", "ő", "hát", "izé", "szóval"],
        "en" => vec!["um", "uh", "erm", "like", "you know"],
        _ => Vec::new(),
    };
    let phrases: Vec<Vec<String>> = if configured.is_empty() {
        defaults.iter().map(|s| (*s).to_owned()).collect()
    } else {
        configured.to_vec()
    }
    .iter()
    .map(|s| s.split_whitespace().map(normalise).collect::<Vec<_>>())
    .filter(|p| !p.is_empty() && p.iter().all(|s| !s.is_empty()))
    .collect();
    let tokens: Vec<_> = words.iter().map(|w| normalise(&w.text)).collect();
    let mut cuts = Vec::new();
    for (i, word) in words.iter().enumerate() {
        if word.start.0 < 0 || word.end <= word.start || tokens[i].is_empty() {
            continue;
        }
        let matched = phrases
            .iter()
            .filter(|p| {
                i + p.len() <= words.len()
                    && tokens[i..i + p.len()] == **p
                    && words[i..i + p.len()].windows(2).all(|w| {
                        w[1].start >= w[0].end && w[1].start.0 - w[0].end.0 <= TICKS_PER_SECOND / 2
                    })
            })
            .max_by_key(|p| p.len());
        let repeat = words.get(i + 1).is_some_and(|next| {
            next.start >= word.end
                && next.start.0 - word.end.0 <= TICKS_PER_SECOND / 2
                && (tokens[i] == tokens[i + 1]
                    || (word.text.trim_end().ends_with('-')
                        && tokens[i + 1].starts_with(&tokens[i])))
        });
        let (end, reason, confidence) = if let Some(phrase) = matched {
            (words[i + phrase.len() - 1].end, AutocutReason::Filler, 0.8)
        } else if repeat {
            (word.end, AutocutReason::Repeat, 0.65)
        } else {
            continue;
        };
        if end > word.start {
            cuts.push(Autocut {
                range: TimeRange {
                    start: word.start,
                    end,
                },
                reason,
                confidence,
                enabled: true,
            });
        }
    }
    merge_cuts(cuts)
}

fn merge_cuts(mut cuts: Vec<Autocut>) -> Vec<Autocut> {
    cuts.sort_by_key(|c| (c.range.start, c.range.end));
    let mut merged: Vec<Autocut> = Vec::new();
    for cut in cuts {
        if let Some(last) = merged.last_mut()
            && cut.range.start <= last.range.end
        {
            last.range.end = last.range.end.max(cut.range.end);
            last.confidence = last.confidence.min(cut.confidence);
        } else {
            merged.push(cut);
        }
    }
    merged
}

pub(crate) fn selection(
    project: &Project,
    params: &AutocutParams,
) -> Result<Selection, ControlError> {
    resolve_selection(project, params.clip, params.track, params.range)
}

/// Audio selection shared by the analysers: a video clip resolves to its linked audio.
pub(crate) fn resolve_selection(
    project: &Project,
    clip: Option<ClipId>,
    track: Option<TrackId>,
    range: Option<TimeRange>,
) -> Result<Selection, ControlError> {
    let requested = clip;
    let mut clip = clip;
    if let Some(id) = clip {
        if let Some(track) = track
            && !project
                .tracks()
                .iter()
                .any(|t| t.id() == track && t.clips().contains_key(&id))
        {
            return Err(invalid("clip is not on the selected track"));
        }
        let original = project
            .tracks()
            .iter()
            .flat_map(|t| t.clips().values())
            .find(|c| c.id == id)
            .ok_or_else(|| invalid("clip missing"))?;
        if matches!(original.kind, ClipKind::Video(_)) {
            clip = Some(
                project
                    .tracks()
                    .iter()
                    .flat_map(|t| t.clips().values())
                    .find(|c| {
                        matches!(c.kind, ClipKind::Audio(_))
                            && original.link_group.is_some()
                            && c.link_group == original.link_group
                    })
                    .ok_or_else(|| invalid("video has no linked audio companion"))?
                    .id,
            );
        }
    }
    Ok(Selection {
        clip,
        track: if requested == clip { track } else { None },
        range,
    })
}
fn target_tracks(project: &Project, params: &AutocutParams, selection: Selection) -> Vec<TrackId> {
    let mut tracks: BTreeSet<_> = project
        .tracks()
        .iter()
        .filter(|t| {
            if params.all_unlocked_tracks {
                return !t.is_locked();
            }
            if let Some(id) = selection.clip {
                return t.clips().contains_key(&id);
            }
            selection.track.is_none_or(|id| id == t.id()) && !t.is_locked()
        })
        .map(animathio_core::Track::id)
        .collect();
    // Track-wide ripple can reach other groups later in the track. Close the entire
    // companion graph, not just the group that intersects a proposed cut.
    loop {
        let groups: BTreeSet<_> = project
            .tracks()
            .iter()
            .filter(|t| tracks.contains(&t.id()))
            .flat_map(|t| t.clips().values().filter_map(|c| c.link_group))
            .collect();
        let before = tracks.len();
        for t in project.tracks() {
            if t.clips()
                .values()
                .any(|c| c.link_group.is_some_and(|g| groups.contains(&g)))
            {
                tracks.insert(t.id());
            }
        }
        if tracks.len() == before {
            break;
        }
    }
    tracks.into_iter().collect()
}

/// Analyse the selected mixed audio and return a snapshot-bound review draft.
/// This synchronous operation belongs on a worker thread in a GUI.
pub fn propose_autocuts(
    project: &Project,
    assets: &BTreeMap<AssetId, Vec<u8>>,
    params: &AutocutParams,
) -> Result<AutocutProposal, ControlError> {
    propose_autocuts_cancellable(project, assets, params, &Arc::new(AtomicBool::new(false)))
}

/// Cancellable background analysis for editor review. Cancellation also aborts
/// model downloads and Whisper decoding; audio preparation completes its current mix.
pub fn propose_autocuts_cancellable(
    project: &Project,
    assets: &BTreeMap<AssetId, Vec<u8>>,
    params: &AutocutParams,
    cancel: &animathio_transcribe::Cancel,
) -> Result<AutocutProposal, ControlError> {
    validate(params)?;
    if params.fillers && !animathio_transcribe::engine_available() {
        return Err(animathio_transcribe::Error::EngineUnavailable.into());
    }
    let selection = selection(project, params)?;
    let tracks = target_tracks(project, params, selection);
    if tracks.is_empty()
        || project
            .tracks()
            .iter()
            .any(|t| tracks.contains(&t.id()) && t.is_locked())
    {
        return Err(invalid(
            "autocut needs unlocked tracks, including every linked companion",
        ));
    }
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(animathio_transcribe::Error::Cancelled.into());
    }
    let (samples, origin) = animathio_transcribe::mix_selection(project, assets, selection)?;
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(animathio_transcribe::Error::Cancelled.into());
    }
    let mut cuts: Vec<_> = detect_silent_samples(&samples, origin, params)?
        .into_iter()
        .map(|range| Autocut {
            range,
            reason: AutocutReason::Silence,
            confidence: 1.0,
            enabled: true,
        })
        .collect();
    if params.fillers {
        let model = ModelSize::parse(&params.model)?;
        if params.download_model && !model.is_installed() {
            animathio_transcribe::models::download(model, cancel, &mut |_| {})?;
        }
        let transcript = animathio_transcribe::transcribe(
            &samples,
            origin,
            &TranscribeOptions {
                model,
                language: params.language.clone(),
                threads: Some(3),
            },
            cancel,
            Arc::new(|_| {}),
        )?;
        let words: Vec<_> = transcript
            .segments
            .into_iter()
            .flat_map(|s| s.words)
            .collect();
        cuts.extend(detect_filler_words(
            &words,
            &transcript.language,
            &params.filler_words,
        ));
    }
    // The mixer uses ceiling sample boundaries. Clamp the last fractional sample
    // to the selected timeline end so zero-padding cannot cut beyond the selection.
    let audio_end = project
        .tracks()
        .iter()
        .filter(|t| selection.track.is_none_or(|id| id == t.id()))
        .flat_map(|t| t.clips().values())
        .filter(|c| {
            matches!(c.kind, ClipKind::Audio(_)) && selection.clip.is_none_or(|id| id == c.id)
        })
        .map(|c| c.range.end)
        .max()
        .unwrap_or(origin);
    let end = selection.range.map_or(audio_end, |r| audio_end.min(r.end));
    for cut in &mut cuts {
        cut.range.end = cut.range.end.min(end);
    }
    cuts.retain(|c| c.range.end > c.range.start);
    let cuts = merge_cuts(cuts);
    if cuts.len() > MAX_CUTS {
        return Err(invalid("too many autocuts; analyse a shorter range"));
    }
    let total_saved = Time(cuts.iter().map(|c| c.range.end.0 - c.range.start.0).sum());
    Ok(AutocutProposal {
        revision: revision(project)?,
        tracks,
        cuts,
        total_saved,
    })
}

/// Validate a reviewed proposal and construct one undoable batch. Stale drafts,
/// overlapping or invalid enabled intervals are rejected before any mutation.
pub fn autocut_command(
    project: &Project,
    proposal: &AutocutProposal,
) -> Result<Command, ControlError> {
    if proposal.revision != revision(project)? {
        return Err(invalid(
            "project changed; analyse again before applying autocuts",
        ));
    }
    if proposal.cuts.len() > MAX_CUTS || proposal.tracks.is_empty() {
        return Err(invalid("invalid autocut proposal"));
    }
    let mut ranges: Vec<_> = proposal
        .cuts
        .iter()
        .filter(|c| c.enabled)
        .map(|c| c.range)
        .collect();
    ranges.sort_by_key(|r| r.start);
    for range in &ranges {
        TimeRange::new(range.start, range.end)?;
        if range.end > crate::content_end(project) {
            return Err(invalid("cut exceeds project content"));
        }
    }
    if ranges.windows(2).any(|r| r[0].end > r[1].start) {
        return Err(invalid("autocut ranges overlap"));
    }
    Ok(Command::Batch {
        commands: ranges
            .into_iter()
            .rev()
            .map(|range| Command::RemoveRangeOnTracks {
                range,
                tracks: proposal.tracks.clone(),
                markers: Default::default(),
            })
            .collect(),
    })
}
/// Apply enabled ranges in reverse order, preserving their original coordinates.
pub fn apply_autocuts(
    project: &mut Project,
    history: &mut History,
    proposal: &AutocutProposal,
) -> Result<TrimSilenceResult, ControlError> {
    let command = autocut_command(project, proposal)?;
    let mut removed: Vec<_> = proposal
        .cuts
        .iter()
        .filter(|c| c.enabled)
        .map(|c| c.range)
        .collect();
    removed.sort_by_key(|r| r.start);
    let total_removed = Time(removed.iter().map(|r| r.end.0 - r.start.0).sum());
    if !removed.is_empty() {
        history.execute(project, &command)?;
    }
    Ok(TrimSilenceResult {
        removed,
        total_removed,
    })
}
