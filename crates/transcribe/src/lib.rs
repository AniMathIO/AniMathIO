//! Local automatic subtitles: speech recognition, model management and subtitle formats.
//!
//! * [`engine`] runs whisper.cpp (cargo feature `whisper`) over 16 kHz mono audio.
//! * [`models`] downloads, verifies and locates the Whisper model files on demand; nothing
//!   is bundled with the editor or the repository.
//! * [`prepare`] turns a project (clip, track or range) or a media file into the 16 kHz
//!   mono samples the engine wants, reusing `animathio-audio`.
//! * [`cues`] splits recognised segments into readable subtitle cues (line length, line
//!   count, duration and overlap rules). It is pure and needs no model.
//! * [`formats`] reads and writes SRT and `WebVTT`.
//!
//! Everything except the engine is available without the `whisper` feature, so default
//! builds stay light; without the feature, [`engine::transcribe`] returns
//! [`Error::EngineUnavailable`].

pub mod cues;
pub mod engine;
pub mod formats;
pub mod models;
pub mod prepare;

pub use cues::{Cue, Segment, SplitOptions, Word, split_segments};
pub use engine::{Cancel, TranscribeOptions, Transcript, engine_available, transcribe};
pub use formats::{Format, format_cues, parse_cues};
pub use models::{ModelInfo, ModelSize};
pub use prepare::{SAMPLE_RATE, mix_selection, mono_16k_from_bytes};

/// Transcription, model or subtitle-format failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The requested model has not been downloaded and downloading was not requested.
    #[error(
        "Whisper model `{model}` is not downloaded ({size_mb} MB). Download it first (editor: \
         Generate subtitles, CLI: `animathio-cli models --download {model}`, MCP: \
         `download_model: true`) or place `{file}` in {dir}"
    )]
    ModelMissing {
        /// Model name.
        model: String,
        /// Expected file name.
        file: String,
        /// Download size in MB.
        size_mb: u64,
        /// Directory searched.
        dir: String,
    },
    /// Downloading the model failed (network down, server error, short read).
    #[error("model download failed: {0}")]
    Download(String),
    /// A downloaded or existing model file does not match its pinned checksum.
    #[error("model checksum mismatch: expected {expected}, got {actual}")]
    Checksum {
        /// Pinned SHA-256.
        expected: String,
        /// Computed SHA-256.
        actual: String,
    },
    /// The work was cancelled.
    #[error("cancelled")]
    Cancelled,
    /// This build has no speech engine.
    #[error(
        "this build has no speech recognition engine; rebuild with the `whisper` cargo feature \
         (needs cmake, a C++ compiler and libclang)"
    )]
    EngineUnavailable,
    /// The speech engine failed.
    #[error("speech recognition failed: {0}")]
    Engine(String),
    /// Audio decoding or mixing failed.
    #[error("audio: {0}")]
    Audio(String),
    /// Invalid arguments or malformed subtitle text.
    #[error("{0}")]
    Invalid(String),
    /// Filesystem failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
