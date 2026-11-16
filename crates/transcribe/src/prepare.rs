//! Audio preparation: 16 kHz mono `f32` samples for the speech engine, from a project
//! selection or a media file. Decoding and resampling reuse `animathio-audio`, so what is
//! transcribed is exactly what the mixer would play (clip gain, mute and track gain
//! applied; effect chains ignored, like silence detection).

use crate::Error;
use animathio_audio::{AudioPool, Pcm, SymphoniaSource};
use animathio_core::{
    AssetId, ClipId, ClipKind, Command, Project, Time, TimeRange, Track, TrackId,
};
use std::collections::BTreeMap;

/// Whisper's required sample rate.
pub const SAMPLE_RATE: u32 = 16_000;

/// Which part of a project to transcribe.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    /// Only this clip (its audio, over its timeline range).
    pub clip: Option<ClipId>,
    /// Only this track's audio.
    pub track: Option<TrackId>,
    /// Restrict to this timeline interval (defaults to the span of the selected audio).
    pub range: Option<TimeRange>,
}

fn audio_error(error: &animathio_audio::Error) -> Error {
    Error::Audio(error.to_string())
}

/// Mix the selected audio to 16 kHz mono. Returns the samples and the timeline tick of the
/// first sample, so recognised times can be mapped back onto the timeline.
pub fn mix_selection(
    project: &Project,
    assets: &BTreeMap<AssetId, Vec<u8>>,
    selection: Selection,
) -> Result<(Vec<f32>, Time), Error> {
    let mut analysis = project.clone();
    let invalid = |m: String| Error::Invalid(m);
    if let Some(clip) = selection.clip {
        let owner = analysis
            .tracks()
            .iter()
            .find(|t| t.clips().contains_key(&clip))
            .map(Track::id)
            .ok_or_else(|| invalid(format!("clip {} missing", clip.0)))?;
        if let Some(track) = selection.track
            && track != owner
        {
            return Err(invalid(format!(
                "clip {} is not on track {}",
                clip.0, track.0
            )));
        }
        // Keep only that clip and its track.
        let others: Vec<(TrackId, Vec<ClipId>)> = analysis
            .tracks()
            .iter()
            .map(|t| {
                let drop = if t.id() == owner {
                    t.clips().keys().copied().filter(|c| *c != clip).collect()
                } else {
                    Vec::new()
                };
                (t.id(), drop)
            })
            .collect();
        for (track, clips) in others {
            if track == owner {
                for id in clips {
                    Command::RemoveClip { id }
                        .apply(&mut analysis)
                        .map_err(|e| invalid(e.to_string()))?;
                }
            } else {
                Command::RemoveTrack { id: track }
                    .apply(&mut analysis)
                    .map_err(|e| invalid(e.to_string()))?;
            }
        }
    } else if let Some(keep) = selection.track {
        if !project.tracks().iter().any(|t| t.id() == keep) {
            return Err(invalid(format!("track {} missing", keep.0)));
        }
        let others: Vec<TrackId> = project
            .tracks()
            .iter()
            .map(Track::id)
            .filter(|id| *id != keep)
            .collect();
        for id in others {
            Command::RemoveTrack { id }
                .apply(&mut analysis)
                .map_err(|e| invalid(e.to_string()))?;
        }
    }
    let mut pool = AudioPool::default();
    let mut span: Option<(Time, Time)> = None;
    for clip in analysis.tracks().iter().flat_map(|t| t.clips().values()) {
        let ClipKind::Audio(id) = &clip.kind else {
            continue;
        };
        span = Some(span.map_or((clip.range.start, clip.range.end), |(s, e)| {
            (s.min(clip.range.start), e.max(clip.range.end))
        }));
        if pool.contains(id) {
            continue;
        }
        let bytes = assets
            .get(id)
            .ok_or_else(|| Error::Audio(format!("audio asset {} not loaded", id.as_str())))?;
        let source = SymphoniaSource::from_bytes(bytes.clone()).map_err(|e| audio_error(&e))?;
        pool.insert(id.clone(), &source)
            .map_err(|e| audio_error(&e))?;
    }
    let Some((mut start, mut end)) = span else {
        return Err(invalid("no audio clips to transcribe".into()));
    };
    if let Some(range) = selection.range {
        start = start.max(range.start);
        end = end.min(range.end);
        if end.0 <= start.0 {
            return Err(invalid("the selected range contains no audio".into()));
        }
    }
    let samples = pool
        .prepare(SAMPLE_RATE, 1)
        .map_err(|e| audio_error(&e))?
        .dry()
        .mix(
            &analysis,
            TimeRange::new(start, end).map_err(|e| invalid(e.to_string()))?,
        )
        .map_err(|e| audio_error(&e))?;
    // The mixer rounds the range start up to the sample lattice.
    let tick_per_sample = animathio_core::TICKS_PER_SECOND / i64::from(SAMPLE_RATE);
    let first = (start.0 + tick_per_sample - 1) / tick_per_sample * tick_per_sample;
    Ok((samples, Time(first)))
}

/// Decode a media file's first audio track to 16 kHz mono samples.
pub fn mono_16k_from_bytes(bytes: Vec<u8>) -> Result<Vec<f32>, Error> {
    let source = SymphoniaSource::from_bytes(bytes).map_err(|e| audio_error(&e))?;
    let (frames, rate) = (source.info().frames, source.info().rate);
    resample_source(&source, frames, rate)
}

/// Resample decoded PCM to 16 kHz mono through the shared mixer path.
pub fn resample_pcm(pcm: &Pcm) -> Result<Vec<f32>, Error> {
    resample_source(pcm, pcm.frames() as u64, pcm.rate())
}

/// Resample a (possibly streaming) source of `frames` frames at `rate` Hz to 16 kHz mono.
fn resample_source(
    source: &impl animathio_audio::AudioSource,
    frames: u64,
    rate: u32,
) -> Result<Vec<f32>, Error> {
    if frames == 0 {
        return Ok(Vec::new());
    }
    let id = AssetId::of(b"transcribe-input");
    let mut pool = AudioPool::default();
    pool.insert(id.clone(), source)
        .map_err(|e| audio_error(&e))?;
    let ticks = i64::try_from(
        i128::from(animathio_core::TICKS_PER_SECOND) * i128::from(frames) / i128::from(rate),
    )
    .map_err(|_| Error::Audio("media too long".into()))?;
    let range =
        TimeRange::new(Time(0), Time(ticks.max(1))).map_err(|e| Error::Audio(e.to_string()))?;
    let mut project = Project::default();
    Command::AddTrack {
        track: Track::new(TrackId(1), "audio"),
        index: 0,
    }
    .apply(&mut project)
    .map_err(|e| Error::Audio(e.to_string()))?;
    Command::AddAsset {
        id: id.clone(),
        asset: animathio_core::Asset {
            name: "input".into(),
            media_type: "audio/wav".into(),
        },
    }
    .apply(&mut project)
    .map_err(|e| Error::Audio(e.to_string()))?;
    Command::AddClip {
        track: TrackId(1),
        clip: animathio_core::Clip::new(ClipId(2), ClipKind::Audio(id), range),
    }
    .apply(&mut project)
    .map_err(|e| Error::Audio(e.to_string()))?;
    pool.prepare(SAMPLE_RATE, 1)
        .map_err(|e| audio_error(&e))?
        .dry()
        .mix(&project, range)
        .map_err(|e| audio_error(&e))
}
