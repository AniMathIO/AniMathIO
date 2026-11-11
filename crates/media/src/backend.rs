//! The pluggable media backend trait and the FilmCraft implementation.
//!
//! The FFmpeg fallback backend is intentionally not implemented yet; the trait is shaped so it
//! can be added later without touching callers.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

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
    /// Decode `len` interleaved f32 samples starting at `media_time`, at the stream's sample rate.
    fn samples(&self, media_time: Time, len: usize) -> Result<Arc<[f32]>>;
}

/// A media backend: probes files and opens decoders. Implementations are cheap, shared handles;
/// decoded resources are owned by the [`MediaPool`](crate::MediaPool).
pub trait MediaBackend: Send + Sync {
    /// Read static metadata without opening a decoder.
    fn probe(&self, path: &Path) -> Result<MediaInfo>;
    /// Open a video decoder for `path`.
    fn open_video(&self, path: &Path) -> Result<Box<dyn VideoDecoder>>;
    /// Open an audio decoder for `path`.
    fn open_audio(&self, path: &Path) -> Result<Box<dyn AudioDecoder>>;
}

/// The FilmCraft backend (MP4/MOV/Matroska/WebM/Ogg/MPEG-TS and standalone audio, via
/// `filmcraft-codecs`).
///
/// File bytes are cached by canonical path so `open_video` and `open_audio` on the same file
/// share one in-memory copy. Decoding uses FilmCraft's GOP-aware seek.
#[derive(Default)]
pub struct FilmCraftBackend {
    bytes: Mutex<HashMap<PathBuf, Arc<[u8]>>>,
}

impl FilmCraftBackend {
    /// Create an empty backend.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn load(&self, path: &Path) -> Result<Arc<[u8]>> {
        let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let mut map = crate::lock(&self.bytes);
        if let Some(bytes) = map.get(&canon) {
            return Ok(bytes.clone());
        }
        let bytes: Arc<[u8]> = std::fs::read(path)
            .map_err(|e| MediaError::Io(format!("{}: {e}", path.display())))?
            .into();
        map.insert(canon, bytes.clone());
        Ok(bytes)
    }

    fn source(&self, path: &Path) -> Result<SharedSource> {
        let bytes = self.load(path)?;
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("media")
            .to_owned();
        open_source(&name, bytes)
    }
}

fn open_source(name: &str, bytes: Arc<[u8]>) -> Result<SharedSource> {
    filmcraft_codecs::open_bytes(name, bytes).map_err(|e| match e {
        filmcraft_media::MediaError::Unsupported(msg) | filmcraft_media::MediaError::Io(msg) => {
            MediaError::Format(msg)
        }
        other => MediaError::Decode(other.to_string()),
    })
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
        let frame = self
            .source
            .video_frame(FrameRequest::full(to_filmcraft(media_time)))
            .map_err(map_media_error)?;
        let rgba = color::frame_to_rgba8(&frame, self.color);
        Ok(Arc::new(Frame {
            width: frame.width,
            height: frame.height,
            pixels: Arc::from(rgba.into_boxed_slice()),
            pts: media_time,
            color: self.color,
        }))
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
        let sample_rate = self.info.sample_rate;
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
        let bytes = self.load(path)?;
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("media")
            .to_owned();
        let source = open_source(&name, bytes.clone())?;
        Ok(media_info(&source, &bytes))
    }

    fn open_video(&self, path: &Path) -> Result<Box<dyn VideoDecoder>> {
        let bytes = self.load(path)?;
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("media")
            .to_owned();
        let source = open_source(&name, bytes.clone())?;
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
        let source = self.source(path)?;
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
