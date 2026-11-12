//! Offline frame-stepped H.264/AAC export using the preview renderer and audio mixer.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use animathio_audio::{AudioPool, Mixer, Pcm};
use animathio_core::{
    AssetId, ClipKind, FrameRate, Project, Rounding, Scene, TICKS_PER_SECOND, Time, TimeRange,
    evaluate,
};
use animathio_media::{
    AudioConfig, EncoderConfig, FilmCraftBackend, MediaBackend, Mp4Encoder, Quality, VideoConfig,
};
use animathio_render::{Frame, RenderCtx, render};
use serde::{Deserialize, Serialize};

/// Offline export failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Invalid range or settings.
    #[error("invalid export: {0}")]
    Invalid(String),
    /// The progress callback requested cancellation.
    #[error("export cancelled")]
    Cancelled,
    /// Rendering failed.
    #[error(transparent)]
    Render(#[from] animathio_render::Error),
    /// Decoding or encoding failed.
    #[error(transparent)]
    Media(#[from] animathio_media::MediaError),
    /// Audio preparation or mixing failed.
    #[error(transparent)]
    Audio(#[from] animathio_audio::Error),
    /// Timeline arithmetic failed.
    #[error(transparent)]
    Model(#[from] animathio_core::Error),
}

/// Serializable output choices. Resolution comes from the project.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Override the project frame rate, including rational NTSC rates.
    pub frame_rate: Option<FrameRate>,
    /// Constant quality, 0..=51 (lower is better).
    pub crf: f32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            frame_rate: None,
            crf: 23.0,
        }
    }
}

/// Frame progress, called before encoding and after every completed frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    /// Fully encoded video frames.
    pub completed: u64,
    /// Total video frames in the requested half-open range.
    pub total: u64,
}

/// Engine resources for one offline job. Construct and use on the export worker.
pub struct Exporter {
    ctx: RenderCtx,
    mixer: Mixer,
}
impl Exporter {
    /// Prepare video/image resources and native audio outside playback.
    pub fn new(project: &Project, assets: &BTreeMap<AssetId, Vec<u8>>) -> Result<Self, Error> {
        let mut pool = AudioPool::default();
        let backend = FilmCraftBackend::new();
        let ids: BTreeSet<_> = project
            .tracks()
            .iter()
            .flat_map(|t| t.clips().values())
            .filter_map(|c| {
                if let ClipKind::Audio(id) = &c.kind {
                    Some(id)
                } else {
                    None
                }
            })
            .collect();
        for id in ids {
            let bytes = assets
                .get(id)
                .ok_or_else(|| Error::Invalid(format!("missing audio asset {id:?}")))?;
            if AssetId::of(bytes) != *id {
                return Err(Error::Invalid("asset hash mismatch".into()));
            }
            let bytes: Arc<[u8]> = bytes.clone().into();
            let info = backend.probe_bytes("media", bytes.clone())?;
            let decoder = backend.open_audio_bytes("media", bytes)?;
            let audio = decoder.info();
            let frames = usize::try_from(
                (i128::from(info.duration.0) * i128::from(audio.sample_rate)
                    + i128::from(TICKS_PER_SECOND)
                    - 1)
                    / i128::from(TICKS_PER_SECOND),
            )
            .map_err(|_| Error::Invalid("audio duration overflow".into()))?;
            let samples = decoder.samples(Time(0), frames)?;
            let pcm = Pcm::new(
                audio.sample_rate,
                u16::try_from(audio.channels)
                    .map_err(|_| Error::Invalid("channel count overflow".into()))?,
                samples.to_vec(),
            )?;
            pool.insert(id.clone(), &pcm)?;
        }
        Ok(Self {
            ctx: RenderCtx::from_assets(assets.clone())?,
            mixer: pool.prepare(48_000, 2)?,
        })
    }

    /// Render exactly the frame used as encoder input. Uses the same evaluation and renderer as preview.
    pub fn render_frame(
        &mut self,
        project: &Project,
        time: Time,
        resolve: &mut impl FnMut(Scene) -> Scene,
    ) -> Result<Frame, Error> {
        let scene = resolve(evaluate(project, time));
        Ok(render(&scene, &mut self.ctx, resolution(project))?)
    }

    /// Render, mix and encode offline. Returning `false` from `progress` cancels the job.
    /// The final partial video frame is included; audio ends at the exact requested end.
    pub fn export(
        &mut self,
        project: &Project,
        range: TimeRange,
        settings: Settings,
        mut progress: impl FnMut(Progress) -> bool,
        mut resolve: impl FnMut(Scene) -> Scene,
    ) -> Result<Vec<u8>, Error> {
        project.validate()?;
        let rate = settings.frame_rate.unwrap_or(project.frame_rate());
        let total = frame_count(project, range, rate)?;
        let (width, height) = resolution(project);
        let mut video = VideoConfig::new(width, height, rate);
        video.quality = Quality::Crf(settings.crf);
        if !progress(Progress {
            completed: 0,
            total,
        }) {
            return Err(Error::Cancelled);
        }
        let mut encoder = Mp4Encoder::new(EncoderConfig::with_audio(
            video,
            AudioConfig::new(48_000, 2, 128_000),
        ))?;
        self.encode_frames(
            project,
            range,
            rate,
            &mut progress,
            &mut resolve,
            |frame, pcm| {
                encoder.encode_video_frame(&frame.pixels)?;
                encoder.encode_audio(pcm)?;
                Ok(())
            },
        )?;
        encoder.finish().map_err(Error::from)
    }

    fn encode_frames(
        &mut self,
        project: &Project,
        range: TimeRange,
        rate: FrameRate,
        progress: &mut impl FnMut(Progress) -> bool,
        resolve: &mut impl FnMut(Scene) -> Scene,
        mut encode: impl FnMut(&Frame, &[f32]) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let total = frame_count(project, range, rate)?;
        for index in 0..total {
            let offset = rate.frame_to_time(
                i64::try_from(index).map_err(|_| Error::Invalid("frame overflow".into()))?,
                Rounding::Nearest,
            )?;
            let time = Time(
                range
                    .start
                    .0
                    .checked_add(offset.0)
                    .ok_or_else(|| Error::Invalid("time overflow".into()))?,
            );
            let frame = self.render_frame(project, time, resolve)?;
            let next = rate.frame_to_time(
                i64::try_from(index + 1).map_err(|_| Error::Invalid("frame overflow".into()))?,
                Rounding::Nearest,
            )?;
            let end = Time(range.start.0.saturating_add(next.0).min(range.end.0));
            let pcm = self.mixer.mix(project, TimeRange::new(time, end)?)?;
            encode(&frame, &pcm)?;
            if !progress(Progress {
                completed: index + 1,
                total,
            }) {
                return Err(Error::Cancelled);
            }
        }
        Ok(())
    }
}

fn frame_count(project: &Project, range: TimeRange, rate: FrameRate) -> Result<u64, Error> {
    if range.start.0 < 0 || range.end <= range.start || range.end > duration(project) {
        return Err(Error::Invalid(
            "range must be nonempty and inside the project".into(),
        ));
    }
    let count = rate.time_to_frame(Time(range.end.0 - range.start.0), Rounding::Ceil)?;
    let count = u64::try_from(count).map_err(|_| Error::Invalid("frame count overflow".into()))?;
    if count > animathio_media::limits::MAX_FRAMES
        || rate.frame_to_time(1, Rounding::Nearest)?.0 <= 0
    {
        return Err(Error::Invalid(
            "frame count/rate exceeds export limits".into(),
        ));
    }
    Ok(count)
}

/// Effective output resolution, mirroring the headless CLI's historical 640×360 fallback when a
/// dimension is unset (`0`).
#[must_use]
pub fn resolution(project: &Project) -> (u32, u32) {
    (
        if project.width() == 0 {
            640
        } else {
            project.width()
        },
        if project.height() == 0 {
            360
        } else {
            project.height()
        },
    )
}

/// Effective duration, deriving it from clips when the project has no explicit duration.
#[must_use]
pub fn duration(project: &Project) -> Time {
    if project.duration().0 > 0 {
        project.duration()
    } else {
        project
            .tracks()
            .iter()
            .flat_map(|t| t.clips().values())
            .map(|c| c.range.end)
            .max()
            .unwrap_or_default()
    }
}

/// Convenience export with unresolved plugins displayed as in headless preview.
/// Call `Exporter::export` with the editor's plugin resolver for installed plugins.
pub fn export(
    project: &Project,
    assets: &BTreeMap<AssetId, Vec<u8>>,
    range: TimeRange,
    settings: Settings,
    progress: impl FnMut(Progress) -> bool,
) -> Result<Vec<u8>, Error> {
    frame_count(
        project,
        range,
        settings.frame_rate.unwrap_or(project.frame_rate()),
    )?;
    Exporter::new(project, assets)?.export(project, range, settings, progress, |scene| scene)
}

#[cfg(test)]
mod tests;
