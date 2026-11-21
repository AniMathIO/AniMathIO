//! Offline frame-stepped export (H.264/AAC MP4 or VP9/Opus `WebM`) using the preview renderer and
//! audio mixer.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use animathio_audio::{AudioPool, Mixer};
use animathio_core::{
    AssetId, ClipKind, FrameRate, Project, Rounding, Scene, TICKS_PER_SECOND, Time, TimeRange,
    evaluate,
};
use animathio_media::{
    AudioConfig, EncoderConfig, FilmCraftBackend, MediaBackend, Mp4Encoder, Quality, VideoConfig,
    WebmEncoder,
};
use animathio_render::{Frame, RenderCtx, render};
use serde::{Deserialize, Serialize};

mod audio_source;
mod chapters;
mod output;
mod plugin_check;

pub use chapters::{ChapterError, MAX_CHAPTERS, embed_chapters};

pub use output::AtomicOutput;

/// Output container and codecs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// MP4 with H.264 video and AAC audio (the default). Can embed chapters.
    #[default]
    Mp4,
    /// `WebM` with VP9 video and Opus audio. No chapters.
    Webm,
}

impl Format {
    /// Every format, in selector order.
    pub const ALL: [Self; 2] = [Self::Mp4, Self::Webm];

    /// Stable lowercase name used in JSON, the CLI and MCP (`mp4`, `webm`); also the file extension.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Webm => "webm",
        }
    }

    /// File extension without the dot.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        self.name()
    }

    /// Human-readable choice for selectors.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Mp4 => "MP4 (H.264 + AAC)",
            Self::Webm => "WebM (VP9 + Opus)",
        }
    }

    /// Whether timeline markers can be embedded as chapters in this container.
    #[must_use]
    pub const fn supports_chapters(self) -> bool {
        matches!(self, Self::Mp4)
    }

    /// The format a file name asks for, from its extension (`.mp4`, `.webm`, any case).
    #[must_use]
    pub fn for_path(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?;
        Self::ALL
            .into_iter()
            .find(|format| format.extension().eq_ignore_ascii_case(extension))
    }

    /// Resolve an optional explicit choice against the output path: an explicit format wins and
    /// must agree with a `.mp4`/`.webm` extension, otherwise the extension picks the format and
    /// MP4 is the default.
    pub fn resolve(explicit: Option<Self>, path: &Path) -> Result<Self, Error> {
        match explicit {
            Some(format) => {
                format.check_path(path)?;
                Ok(format)
            }
            None => Ok(Self::for_path(path).unwrap_or_default()),
        }
    }

    /// Fail when `path` ends in the extension of the other format.
    pub fn check_path(self, path: &Path) -> Result<(), Error> {
        match Self::for_path(path) {
            Some(other) if other != self => Err(Error::Invalid(format!(
                "the output file ends in .{} but the export format is {}; use a .{} file name or choose {}",
                other.extension(),
                self.name(),
                self.extension(),
                other.name(),
            ))),
            _ => Ok(()),
        }
    }
}

impl std::fmt::Display for Format {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl std::str::FromStr for Format {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self, Error> {
        Self::ALL
            .into_iter()
            .find(|format| format.name().eq_ignore_ascii_case(value.trim()))
            .ok_or_else(|| {
                Error::Invalid(format!(
                    "unknown export format '{value}'; supported formats are mp4 and webm"
                ))
            })
    }
}

/// Where an offline export writes its video file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Destination {
    /// Final output path; missing parent directories are created.
    pub path: PathBuf,
    /// Replace an existing file instead of failing with [`Error::AlreadyExists`].
    pub overwrite: bool,
}
impl Destination {
    /// Destination that refuses to replace an existing file.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            overwrite: false,
        }
    }

    /// Same destination, replacing an existing file.
    #[must_use]
    pub fn overwriting(mut self) -> Self {
        self.overwrite = true;
        self
    }
}

/// Offline export failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Invalid range or settings.
    #[error("invalid export: {0}")]
    Invalid(String),
    /// The output exists and overwriting was not requested.
    #[error("output already exists (pass --overwrite to replace it): {0}")]
    AlreadyExists(PathBuf),
    /// The output file could not be created or written.
    #[error(transparent)]
    Io(#[from] std::io::Error),
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
    /// A plugin clip in the export range is missing or failed to render.
    #[error("clip {clip} (plugin '{plugin}') cannot be exported: {message}")]
    Plugin {
        /// Clip whose plugin could not run.
        clip: u64,
        /// Plugin id the clip refers to.
        plugin: String,
        /// Why it could not be resolved.
        message: String,
    },
    /// The finished MP4 could not take the chapter list.
    #[error("cannot embed chapters: {0}")]
    Chapters(#[from] ChapterError),
}

/// Serializable output choices. Resolution comes from the project.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Override the project frame rate, including rational NTSC rates.
    pub frame_rate: Option<FrameRate>,
    /// Constant quality, 0..=51 (lower is better). The same scale for both formats.
    pub crf: f32,
    /// Embed the timeline markers inside the exported range as a Nero `chpl` chapter list
    /// (at most [`MAX_CHAPTERS`]). Nothing is written when no marker is in range. MP4 only:
    /// `WebM` output carries no chapters and ignores this.
    #[serde(default = "default_embed_chapters")]
    pub embed_chapters: bool,
    /// Output container and codecs.
    #[serde(default)]
    pub format: Format,
}

fn default_embed_chapters() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            frame_rate: None,
            crf: 23.0,
            embed_chapters: true,
            format: Format::Mp4,
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
            let audio = decoder.info().clone();
            let frames = usize::try_from(
                (i128::from(info.duration.0) * i128::from(audio.sample_rate)
                    + i128::from(TICKS_PER_SECOND)
                    - 1)
                    / i128::from(TICKS_PER_SECOND),
            )
            .map_err(|_| Error::Invalid("audio duration overflow".into()))?;
            let source = audio_source::DecoderSource::new(
                decoder,
                audio.sample_rate,
                u16::try_from(audio.channels)
                    .map_err(|_| Error::Invalid("channel count overflow".into()))?,
                frames as u64,
            );
            pool.insert(id.clone(), &source)?;
        }
        Ok(Self {
            ctx: RenderCtx::from_assets(assets.clone())?,
            mixer: pool.prepare(48_000, 2)?,
        })
    }

    /// Install the plugin runner for clip effects (see
    /// `PluginHost::effect_processor`). Exporting a project whose clips have effects
    /// fails without one.
    pub fn set_effect_processor(
        &mut self,
        processor: impl animathio_core::EffectProcessor + 'static,
    ) {
        self.ctx.set_effect_processor(processor);
    }

    /// Install the plugin runner for clip and track audio effects (see
    /// `PluginHost::audio_effect_processor`). Exporting a project that has active audio effects
    /// fails without one.
    pub fn set_audio_effect_processor(
        &mut self,
        processor: impl animathio_core::AudioEffectProcessor + 'static,
    ) {
        self.mixer = self
            .mixer
            .clone()
            .with_effect_processor(Arc::new(processor));
    }

    /// Render exactly the frame used as encoder input. Uses the same evaluation and renderer as preview.
    pub fn render_frame(
        &mut self,
        project: &Project,
        time: Time,
        resolve: &mut impl FnMut(Scene) -> Scene,
    ) -> Result<Frame, Error> {
        let scene = evaluate(project, time);
        let pending = plugin_check::pending_plugins(&scene);
        let resolve_timer = animathio_core::timing::Timer::new("resolve-plugins");
        let scene = resolve(scene);
        drop(resolve_timer);
        plugin_check::check_resolved(&pending, &scene)?;
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
        let mut encoder = FormatEncoder::new(settings.format, video)?;
        // One queued frame bounds memory while rendering overlaps encoding. The
        // worker receives owned buffers, preserving frame/audio order without copies.
        std::thread::scope(|scope| {
            let (tx, rx) = std::sync::mpsc::sync_channel::<(Frame, Vec<f32>)>(1);
            let (done_tx, done_rx) = std::sync::mpsc::channel();
            let worker = scope.spawn(move || -> Result<Vec<u8>, Error> {
                for (frame, pcm) in rx {
                    encoder.encode_video_frame(&frame.pixels)?;
                    encoder.encode_audio(&pcm)?;
                    if done_tx.send(()).is_err() {
                        return Err(Error::Cancelled);
                    }
                }
                encoder.finish().map_err(Error::from)
            });
            let mut submitted = 0;
            let mut completed = 0;
            let produced = self.encode_frames(
                project,
                range,
                rate,
                &mut |_| true,
                &mut resolve,
                |frame, pcm| {
                    tx.send((frame, pcm)).map_err(|_| Error::Cancelled)?;
                    submitted += 1;
                    if submitted > 1 {
                        done_rx.recv().map_err(|_| Error::Cancelled)?;
                        completed += 1;
                        if !progress(Progress { completed, total }) {
                            return Err(Error::Cancelled);
                        }
                    }
                    Ok(())
                },
            );
            drop(tx);
            let worker_result = worker
                .join()
                .map_err(|_| Error::Invalid("encoder worker panicked".into()))?;
            // Prefer the encoder's concrete error over a disconnected channel.
            let bytes = worker_result?;
            produced?;
            if !progress(Progress {
                completed: total,
                total,
            }) {
                return Err(Error::Cancelled);
            }
            if settings.embed_chapters && settings.format.supports_chapters() {
                Ok(embed_chapters(bytes, project.markers(), range)?)
            } else {
                Ok(bytes)
            }
        })
    }

    /// Render, mix and encode into `destination` atomically.
    ///
    /// Missing parent directories are created and an existing file is only
    /// replaced when `destination.overwrite` is set; otherwise the job fails
    /// with [`Error::AlreadyExists`]. A `.mp4` or `.webm` destination must match
    /// `settings.format` (see [`Format::resolve`]). The file is written to a temporary file
    /// next to the destination and renamed into place on success, so a failed
    /// or cancelled export leaves no partial output (and no temp file) behind.
    pub fn export_to_file(
        &mut self,
        project: &Project,
        range: TimeRange,
        settings: Settings,
        destination: &Destination,
        progress: impl FnMut(Progress) -> bool,
        resolve: impl FnMut(Scene) -> Scene,
    ) -> Result<PathBuf, Error> {
        settings.format.check_path(&destination.path)?;
        let mut output = AtomicOutput::create(&destination.path, destination.overwrite)?;
        let bytes = self.export(project, range, settings, progress, resolve)?;
        output.write_all(&bytes)?;
        output.commit()
    }

    fn encode_frames(
        &mut self,
        project: &Project,
        range: TimeRange,
        rate: FrameRate,
        progress: &mut impl FnMut(Progress) -> bool,
        resolve: &mut impl FnMut(Scene) -> Scene,
        mut encode: impl FnMut(Frame, Vec<f32>) -> Result<(), Error>,
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
            let mix_timer = animathio_core::timing::Timer::new("audio-mix");
            let pcm = self.mixer.mix(project, TimeRange::new(time, end)?)?;
            drop(mix_timer);
            encode(frame, pcm)?;
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

/// The selected container's encoder behind one interface.
enum FormatEncoder {
    Mp4(Box<Mp4Encoder>),
    Webm(Box<WebmEncoder>),
}

impl FormatEncoder {
    /// 48 kHz stereo audio at 128 kbit/s in both containers.
    fn new(format: Format, video: VideoConfig) -> Result<Self, Error> {
        let config = EncoderConfig::with_audio(video, AudioConfig::new(48_000, 2, 128_000));
        Ok(match format {
            Format::Mp4 => Self::Mp4(Box::new(Mp4Encoder::new(config)?)),
            Format::Webm => Self::Webm(Box::new(WebmEncoder::new(config)?)),
        })
    }

    fn encode_video_frame(&mut self, rgba: &[u8]) -> Result<(), animathio_media::MediaError> {
        match self {
            Self::Mp4(encoder) => encoder.encode_video_frame(rgba),
            Self::Webm(encoder) => encoder.encode_video_frame(rgba),
        }
    }

    fn encode_audio(&mut self, interleaved: &[f32]) -> Result<(), animathio_media::MediaError> {
        match self {
            Self::Mp4(encoder) => encoder.encode_audio(interleaved),
            Self::Webm(encoder) => encoder.encode_audio(interleaved),
        }
    }

    fn finish(self) -> Result<Vec<u8>, animathio_media::MediaError> {
        match self {
            Self::Mp4(encoder) => encoder.finish(),
            Self::Webm(encoder) => encoder.finish(),
        }
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

/// Convenience export without a plugin resolver: it fails with [`Error::Plugin`] if the
/// project has plugin clips in range. Call `Exporter::export` with a plugin resolver for those.
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

/// Convenience atomic export without a plugin resolver (see [`export`]).
/// The returned path is the destination, which is only created on success.
pub fn export_to_file(
    project: &Project,
    assets: &BTreeMap<AssetId, Vec<u8>>,
    range: TimeRange,
    settings: Settings,
    destination: &Destination,
    progress: impl FnMut(Progress) -> bool,
) -> Result<PathBuf, Error> {
    frame_count(
        project,
        range,
        settings.frame_rate.unwrap_or(project.frame_rate()),
    )?;
    Exporter::new(project, assets)?.export_to_file(
        project,
        range,
        settings,
        destination,
        progress,
        |scene| scene,
    )
}

#[cfg(test)]
mod tests;
