//! The pluggable media backend trait and the FilmCraft implementation.
//!
//! The FFmpeg fallback backend is intentionally not implemented yet; the trait is shaped so it
//! can be added later without touching callers.

use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use animathio_core::{FrameRate, TICKS_PER_SECOND, Time};
use filmcraft_media::{FrameRequest, SharedSource};

use crate::color::{self, ColorInfo};
use crate::frame::Frame;
use crate::info::{AudioStreamInfo, MediaInfo, MediaKind, VideoStreamInfo};
use crate::time::{from_filmcraft, to_filmcraft};
use crate::{MediaError, Result};

/// Decodes video frames for one asset.
pub trait VideoDecoder: Send + Sync {
    /// Static metadata for this stream.
    fn info(&self) -> &VideoStreamInfo;
    /// Decode the frame displayed at `media_time` as straight-alpha RGBA8.
    fn frame_at(&self, media_time: Time) -> Result<Arc<Frame>>;
}

/// Decodes PCM ranges for one asset.
pub trait AudioDecoder: Send + Sync {
    /// Static metadata for this stream.
    fn info(&self) -> &AudioStreamInfo;
    /// Decode `len` frames of interleaved f32 samples starting at `media_time`, at the stream's sample rate.
    fn samples(&self, media_time: Time, len: usize) -> Result<Arc<[f32]>>;
}

/// A media backend: probes files and opens decoders. Implementations are cheap, shared handles;
/// decoded resources are owned by the [`MediaPool`](crate::MediaPool).
pub trait MediaBackend: Send + Sync {
    /// Read metadata from in-memory encoded media.
    fn probe_bytes(&self, _name: &str, _bytes: Arc<[u8]>) -> Result<MediaInfo> {
        Err(MediaError::Format("byte loading unavailable".into()))
    }
    /// Open video from verified in-memory asset bytes.
    fn open_video_bytes(&self, _name: &str, _bytes: Arc<[u8]>) -> Result<Box<dyn VideoDecoder>> {
        Err(MediaError::Format("byte loading unavailable".into()))
    }
    /// Open audio from verified in-memory asset bytes.
    fn open_audio_bytes(&self, _name: &str, _bytes: Arc<[u8]>) -> Result<Box<dyn AudioDecoder>> {
        Err(MediaError::Format("byte loading unavailable".into()))
    }
    /// Read static metadata from a file.
    fn probe(&self, path: &Path) -> Result<MediaInfo>;
    /// Open a video decoder for `path`.
    fn open_video(&self, path: &Path) -> Result<Box<dyn VideoDecoder>>;
    /// Open an audio decoder for `path`.
    fn open_audio(&self, path: &Path) -> Result<Box<dyn AudioDecoder>>;
}

/// The FilmCraft backend (MP4/MOV/Matroska/WebM/Ogg/MPEG-TS and standalone audio, via
/// `filmcraft-codecs`).
///
/// Decoding uses FilmCraft's GOP-aware seek. Byte-based loading shares one encoded asset allocation.
#[derive(Default)]
pub struct FilmCraftBackend;

impl FilmCraftBackend {
    /// Create an empty backend.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    fn load(path: &Path) -> Result<Arc<[u8]>> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|e| MediaError::Io(e.to_string()))?
            .take(crate::limits::MAX_INPUT_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| MediaError::Io(e.to_string()))?;
        crate::limits::input(&bytes)?;
        Ok(bytes.into())
    }
}

fn open_source(name: &str, bytes: Arc<[u8]>) -> Result<SharedSource> {
    crate::limits::input(&bytes)?;
    let source = filmcraft_codecs::open_bytes(name, bytes).map_err(|e| match e {
        filmcraft_media::MediaError::Unsupported(msg) | filmcraft_media::MediaError::Io(msg) => {
            MediaError::Format(msg)
        }
        other => MediaError::Decode(other.to_string()),
    })?;
    let info = source.info();
    if info.duration.0 < 0 {
        return Err(crate::limits::invalid());
    }
    if let Some(v) = &info.video {
        crate::limits::frame_bytes(v.width, v.height)?;
        let frames = i128::from(info.duration.0) * i128::from(v.frame_rate.num)
            / i128::from(filmcraft_time::TICKS_PER_SECOND)
            / i128::from(v.frame_rate.den.max(1));
        if frames > i128::from(crate::limits::MAX_FRAMES) {
            return Err(crate::limits::invalid());
        }
    }
    for a in &info.audio_streams {
        crate::limits::audio(a.sample_rate, a.channels as usize, 0)?;
    }
    Ok(source)
}

fn map_media_error(e: filmcraft_media::MediaError) -> MediaError {
    match e {
        filmcraft_media::MediaError::NoStream(kind) => MediaError::NoStream(kind),
        filmcraft_media::MediaError::Unsupported(msg) | filmcraft_media::MediaError::Io(msg) => {
            MediaError::Format(msg)
        }
        other => MediaError::Decode(other.to_string()),
    }
}

fn core_frame_rate(num: i64, den: i64) -> FrameRate {
    let num = u32::try_from(num.max(0)).unwrap_or(30);
    let den = u32::try_from(den.max(0)).unwrap_or(1);
    FrameRate::new(num, den).unwrap_or_default()
}

fn media_info(source: &SharedSource, bytes: &[u8]) -> MediaInfo {
    let mi = source.info();
    let kind = match mi.kind {
        filmcraft_media::MediaKind::Movie => MediaKind::Movie,
        filmcraft_media::MediaKind::AudioOnly => MediaKind::AudioOnly,
        filmcraft_media::MediaKind::Still => MediaKind::Still,
        filmcraft_media::MediaKind::ImageSequence | filmcraft_media::MediaKind::Synthetic => {
            MediaKind::Sequence
        }
    };
    let mut video = None;
    if let Some(v) = mi.video.as_ref() {
        let (color, vfr) = color::resolve(bytes, v.color);
        video = Some(VideoStreamInfo {
            width: v.width,
            height: v.height,
            frame_rate: core_frame_rate(v.frame_rate.num, v.frame_rate.den),
            vfr,
            color,
            codec: v.codec.clone(),
            pixel_format: v.pixel_format.clone(),
            has_alpha: v.has_alpha,
        });
    }
    MediaInfo {
        kind,
        duration: from_filmcraft(mi.duration),
        video,
        audio: mi
            .audio_streams
            .iter()
            .map(|a| AudioStreamInfo {
                sample_rate: a.sample_rate,
                channels: a.channels,
                codec: a.codec.clone(),
            })
            .collect(),
        container: mi.container.clone(),
    }
}

/// A FilmCraft-backed video decoder with explicit BT.601-defaulted colour conversion.
pub struct FilmCraftVideoDecoder {
    source: SharedSource,
    info: VideoStreamInfo,
    color: ColorInfo,
}

impl VideoDecoder for FilmCraftVideoDecoder {
    fn info(&self) -> &VideoStreamInfo {
        &self.info
    }

    fn frame_at(&self, media_time: Time) -> Result<Arc<Frame>> {
        let _timer = animathio_core::timing::Timer::new("decode");
        if media_time.0 < 0 {
            return Err(crate::limits::invalid());
        }
        let demux = animathio_core::timing::Timer::new("decode-codec");
        let frame = self
            .source
            .video_frame(FrameRequest::full(to_filmcraft(media_time)))
            .map_err(map_media_error)?;
        drop(demux);
        crate::limits::frame_bytes(frame.width, frame.height)?;
        let _convert = animathio_core::timing::Timer::new("decode-yuv-to-rgba");
        let rgba = color::frame_to_rgba8(&frame, self.color)?;
        Ok(Arc::new(Frame {
            width: frame.width,
            height: frame.height,
            pixels: Arc::from(rgba.into_boxed_slice()),
            pts: media_time,
            color: self.color,
        }))
    }
}

fn indexed_container(bytes: &[u8]) -> bool {
    bytes.get(4..8) == Some(b"ftyp") || bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3])
}

/// Open audio-only media (WAV, MP3, FLAC, Vorbis, Opus, ...) as a streaming source: the
/// asset is scanned once for its exact length, never held decoded in memory.
fn bounded_audio(bytes: &[u8]) -> Result<animathio_audio::SymphoniaSource> {
    crate::limits::input(bytes)?;
    let source = animathio_audio::SymphoniaSource::from_bytes_with_limits(
        bytes.to_vec(),
        animathio_audio::DecodeLimits {
            max_duration: std::time::Duration::from_hours(4),
            max_samples: animathio_audio::MAX_INTERLEAVED_SAMPLES,
        },
    )
    .map_err(|e| MediaError::Decode(e.to_string()))?;
    let info = source.info();
    crate::limits::audio(info.rate, usize::from(info.channels), 0)?;
    Ok(source)
}

struct BoundedAudioDecoder {
    reader: std::sync::Mutex<Box<dyn animathio_audio::PcmReader>>,
    rate: u32,
    channels: usize,
    info: AudioStreamInfo,
}
impl AudioDecoder for BoundedAudioDecoder {
    fn info(&self) -> &AudioStreamInfo {
        &self.info
    }
    fn samples(&self, media_time: Time, len: usize) -> Result<Arc<[f32]>> {
        crate::limits::audio(self.info.sample_rate, self.info.channels as usize, len)?;
        if media_time.0 < 0 {
            return Err(crate::limits::invalid());
        }
        let start = u64::try_from(sample_index(media_time, self.rate))
            .map_err(|_| crate::limits::invalid())?;
        let mut output = Vec::with_capacity(len * self.channels);
        self.reader
            .lock()
            .map_err(|_| crate::limits::invalid())?
            .read_at(start, len, &mut output)
            .map_err(|e| MediaError::Decode(e.to_string()))?;
        output.resize(len * self.channels, 0.0);
        Ok(output.into())
    }
}

/// A FilmCraft-backed audio decoder returning interleaved f32 PCM.
pub struct FilmCraftAudioDecoder {
    source: SharedSource,
    info: AudioStreamInfo,
}

impl AudioDecoder for FilmCraftAudioDecoder {
    fn info(&self) -> &AudioStreamInfo {
        &self.info
    }

    fn samples(&self, media_time: Time, len: usize) -> Result<Arc<[f32]>> {
        if media_time.0 < 0 {
            return Err(crate::limits::invalid());
        }
        let sample_rate = self.info.sample_rate;
        crate::limits::audio(sample_rate, self.info.channels as usize, len)?;
        if sample_rate == 0 {
            return Err(MediaError::NoStream("audio"));
        }
        let start = sample_index(media_time, sample_rate);
        let buffer = self
            .source
            .audio(start, len, sample_rate)
            .map_err(map_media_error)?;
        Ok(Arc::from(buffer.interleaved().into_boxed_slice()))
    }
}

fn sample_index(time: Time, sample_rate: u32) -> i64 {
    let n = i128::from(time.0) * i128::from(sample_rate);
    let d = i128::from(TICKS_PER_SECOND);
    n.div_euclid(d)
        .clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}

impl MediaBackend for FilmCraftBackend {
    fn probe(&self, path: &Path) -> Result<MediaInfo> {
        self.probe_bytes("media", Self::load(path)?)
    }
    fn probe_bytes(&self, name: &str, bytes: Arc<[u8]>) -> Result<MediaInfo> {
        if !indexed_container(&bytes) {
            let source = bounded_audio(&bytes)?;
            let pcm = source.info();
            return Ok(MediaInfo {
                kind: MediaKind::AudioOnly,
                duration: Time(
                    i64::try_from(
                        i128::from(pcm.frames) * i128::from(TICKS_PER_SECOND)
                            / i128::from(pcm.rate),
                    )
                    .map_err(|_| crate::limits::invalid())?,
                ),
                video: None,
                audio: vec![AudioStreamInfo {
                    sample_rate: pcm.rate,
                    channels: u32::from(pcm.channels),
                    codec: "PCM (Symphonia)".into(),
                }],
                container: "audio".into(),
            });
        }
        let source = open_source(name, bytes.clone())?;
        Ok(media_info(&source, &bytes))
    }

    fn open_video(&self, path: &Path) -> Result<Box<dyn VideoDecoder>> {
        self.open_video_bytes("media", Self::load(path)?)
    }
    fn open_video_bytes(&self, name: &str, bytes: Arc<[u8]>) -> Result<Box<dyn VideoDecoder>> {
        if !indexed_container(&bytes) {
            return Err(MediaError::NoStream("video"));
        }
        let source = open_source(name, bytes.clone())?;
        let mi = source.info();
        let v = mi.video.as_ref().ok_or(MediaError::NoStream("video"))?;
        let (color, vfr) = color::resolve(&bytes, v.color);
        let info = VideoStreamInfo {
            width: v.width,
            height: v.height,
            frame_rate: core_frame_rate(v.frame_rate.num, v.frame_rate.den),
            vfr,
            color,
            codec: v.codec.clone(),
            pixel_format: v.pixel_format.clone(),
            has_alpha: v.has_alpha,
        };
        Ok(Box::new(FilmCraftVideoDecoder {
            source,
            info,
            color,
        }))
    }

    fn open_audio(&self, path: &Path) -> Result<Box<dyn AudioDecoder>> {
        self.open_audio_bytes("media", Self::load(path)?)
    }
    fn open_audio_bytes(&self, name: &str, bytes: Arc<[u8]>) -> Result<Box<dyn AudioDecoder>> {
        if !indexed_container(&bytes) {
            let source = bounded_audio(&bytes)?;
            let stream = source.info();
            let info = AudioStreamInfo {
                sample_rate: stream.rate,
                channels: u32::from(stream.channels),
                codec: "PCM (Symphonia)".into(),
            };
            let reader = animathio_audio::AudioSource::open_reader(&source)
                .map_err(|e| MediaError::Decode(e.to_string()))?;
            return Ok(Box::new(BoundedAudioDecoder {
                reader: std::sync::Mutex::new(reader),
                rate: stream.rate,
                channels: usize::from(stream.channels),
                info,
            }));
        }
        let source = open_source(name, bytes)?;
        let mi = source.info();
        let a = mi.audio().ok_or(MediaError::NoStream("audio"))?;
        let info = AudioStreamInfo {
            sample_rate: a.sample_rate,
            channels: a.channels,
            codec: a.codec.clone(),
        };
        Ok(Box::new(FilmCraftAudioDecoder { source, info }))
    }
}
