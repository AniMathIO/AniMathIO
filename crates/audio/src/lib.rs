//! Engine-owned PCM, deterministic offline mixing, and device-master playback.
//! No decoder or stream is stored in the serializable project.
mod mixer;
mod player;
mod source;
mod waveform;

pub use mixer::*;
pub use player::*;
pub use source::*;
pub use waveform::*;

/// Audio decoding, configuration or playback failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Unsupported container, codec, or device sample format.
    #[error("unsupported audio: {0}")]
    Unsupported(String),
    /// Malformed input or invalid configuration.
    #[error("invalid audio: {0}")]
    Invalid(String),
    /// A required asset has not been loaded into the resource pool.
    #[error("audio asset missing from pool: {0}")]
    MissingAsset(String),
    /// Decoder or resampler failure.
    #[error("audio decode/resample: {0}")]
    Decode(String),
    /// Device or worker failure.
    #[error("audio playback: {0}")]
    Playback(String),
    /// Filesystem failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
