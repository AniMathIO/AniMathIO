//! Production media runtime: probing, decoding, pooling and MP4 export.
//!
//! FilmCraft is the primary backend; the [`MediaBackend`] trait is ready for a future FFmpeg
//! fallback. The [`MediaPool`] is the engine-owned resource pool keyed by [`AssetId`]: it owns
//! decoders and an LRU frame cache, decodes on worker threads, and prefetches the next frames
//! during sequential playback.

#![allow(
    // Media code is conversion-heavy; these paths use checked/clamped `as` casts.
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    // Pixel loops conventionally use short (`x`, `y`, `u`, `v`) bindings.
    clippy::many_single_char_names,
    clippy::similar_names,
    // Product/format names read better without backticks.
    clippy::doc_markdown,
    // Public APIs take small ids by value for ergonomics.
    clippy::needless_pass_by_value,
)]

mod backend;
mod color;
mod encoder;
mod frame;
mod info;
pub mod limits;
mod pool;
mod time;

pub use backend::{AudioDecoder, FilmCraftBackend, MediaBackend, VideoDecoder};
pub use color::{ColorInfo, ColorRange, Matrix, Primaries, Transfer};
pub use encoder::{AudioConfig, EncoderConfig, Mp4Encoder, Quality, VideoConfig};
pub use frame::Frame;
pub use info::{AudioStreamInfo, MediaInfo, MediaKind, VideoStreamInfo};
pub use pool::{MediaPool, MediaPoolConfig};

use std::sync::{Mutex, MutexGuard, PoisonError};

use animathio_core::AssetId;

/// Lock a [`Mutex`] even after poisoning.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Media errors.
#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    /// File or resource I/O failure.
    #[error("I/O error: {0}")]
    Io(String),
    /// Unsupported, malformed or unrecognised media.
    #[error("media format error: {0}")]
    Format(String),
    /// Decoder or codec failure.
    #[error("decode error: {0}")]
    Decode(String),
    /// The requested stream is absent.
    #[error("no {0} stream")]
    NoStream(&'static str),
    /// The pool does not know this asset.
    #[error("unknown asset {0:?}")]
    UnknownAsset(AssetId),
    /// An encoder argument is invalid.
    #[error("invalid argument: {0}")]
    Invalid(String),
    /// The worker pool has shut down.
    #[error("media pool shut down")]
    Shutdown,
}

/// Media result alias.
pub type Result<T> = std::result::Result<T, MediaError>;
