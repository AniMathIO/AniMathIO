//! Static, resource-free media metadata.

use animathio_core::{FrameRate, Time};

use crate::color::ColorInfo;

/// Broad classification of a media asset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    /// Video, possibly with audio.
    Movie,
    /// Audio only.
    AudioOnly,
    /// A still image.
    Still,
    /// An image sequence or synthetic source.
    Sequence,
    /// Anything the backend cannot classify.
    Other,
}

/// Metadata for one video stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoStreamInfo {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Exact rational frame rate (reduced).
    pub frame_rate: FrameRate,
    /// True when sample durations vary (variable frame rate).
    pub vfr: bool,
    /// Resolved colour metadata.
    pub color: ColorInfo,
    /// Codec name (e.g. `h264`).
    pub codec: String,
    /// Pixel format label.
    pub pixel_format: String,
    /// Whether the stream carries an alpha channel.
    pub has_alpha: bool,
}

/// Metadata for one audio stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioStreamInfo {
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Channel count.
    pub channels: u32,
    /// Codec name (e.g. `aac`).
    pub codec: String,
}

/// Static media metadata produced by [`MediaBackend::probe`](crate::MediaBackend::probe).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaInfo {
    /// Media kind.
    pub kind: MediaKind,
    /// Media duration in core ticks.
    pub duration: Time,
    /// Video stream metadata, when present.
    pub video: Option<VideoStreamInfo>,
    /// Audio streams in file order.
    pub audio: Vec<AudioStreamInfo>,
    /// Container name (e.g. `mp4`).
    pub container: String,
}
