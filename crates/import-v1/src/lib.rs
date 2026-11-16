//! One-way importer for v1 (`.animathio`) projects.
//!
//! v1 serialized a project as pako-deflated JSON (the earliest releases used
//! plain JSON) with media embedded as base64 data URLs. This crate inflates,
//! parses leniently, decodes media into content-addressed assets and rebuilds a
//! v2 [`animathio_core::Project`] exclusively through [`animathio_core::Command`]s.

// Media import is full of exact-but-lossy float conversions by design: v1
// stores milliseconds and pixels as f64 while v2 stores integer ticks and
// normalized fractions. Bounds are checked where they matter; the residual
// precision/sign casts are inherent to the translation.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

mod map;
mod model;

use animathio_core::{AssetId, Project};

/// Failures that prevent a v1 project from being imported.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    /// The payload could not be inflated (and was not plain JSON text either).
    #[error("could not inflate v1 project: {0}")]
    Inflate(#[from] std::io::Error),
    /// The deflated JSON exceeds the configured size limit.
    #[error("v1 project inflates to {inflated} bytes, exceeding the {limit} byte limit")]
    InflatedTooLarge {
        /// Decompressed size that was rejected.
        inflated: u64,
        /// Configured maximum decompressed size.
        limit: u64,
    },
    /// The JSON could not be parsed.
    #[error("invalid v1 project JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// One embedded media asset exceeds the configured size limit.
    #[error("embedded media asset is {bytes} bytes, exceeding the {limit} byte limit")]
    AssetTooLarge {
        /// Decoded size that was rejected.
        bytes: u64,
        /// Configured maximum per-asset size.
        limit: u64,
    },
    /// The v1 data is structurally unusable.
    #[error("invalid v1 project: {0}")]
    Invalid(String),
    /// The rebuilt v2 project failed validation.
    #[error(transparent)]
    Model(#[from] animathio_core::Error),
}

/// Resource limits for importing untrusted v1 files.
#[derive(Clone, Copy, Debug)]
pub struct ImportLimits {
    /// Maximum uncompressed project JSON bytes (decompression-bomb guard).
    pub inflated_bytes: u64,
    /// Maximum decoded bytes per embedded media asset.
    pub asset_bytes: u64,
    /// Maximum number of embedded media entries.
    pub media_entries: usize,
}
impl Default for ImportLimits {
    fn default() -> Self {
        Self {
            inflated_bytes: 2 * 1024 * 1024 * 1024, // 2 GiB
            asset_bytes: 1024 * 1024 * 1024,        // 1 GiB
            media_entries: 10_000,
        }
    }
}

/// Non-fatal observations produced while importing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub(crate) warnings: Vec<String>,
}
impl Report {
    /// Warnings, in import order.
    #[must_use]
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }
    /// Whether the import produced no warnings.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.warnings.is_empty()
    }
    /// Number of warnings.
    #[must_use]
    pub fn len(&self) -> usize {
        self.warnings.len()
    }
    pub(crate) fn warn(&mut self, message: impl Into<String>) {
        self.warnings.push(message.into());
    }
}

/// One decoded, content-addressed media asset.
#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    /// BLAKE3 content hash, also registered in the returned project.
    pub id: AssetId,
    /// Human-readable filename hint.
    pub name: String,
    /// Declared media type; decoders must probe the actual bytes.
    pub media_type: String,
    /// Raw decoded bytes.
    pub bytes: Vec<u8>,
}

/// Import a v1 `.animathio` payload, discarding warnings.
///
/// See [`import_v1_with_report`] for the warning-carrying variant.
pub fn import_v1(bytes: &[u8]) -> Result<(Project, Vec<Asset>), ImportError> {
    let (project, assets, _) = import_v1_with_report(bytes)?;
    Ok((project, assets))
}

/// Import a v1 `.animathio` payload, keeping non-fatal warnings.
pub fn import_v1_with_report(bytes: &[u8]) -> Result<(Project, Vec<Asset>, Report), ImportError> {
    map::import_with_report(bytes, &ImportLimits::default())
}

/// Import with explicit resource limits, keeping non-fatal warnings.
pub fn import_v1_with_limits(
    bytes: &[u8],
    limits: &ImportLimits,
) -> Result<(Project, Vec<Asset>, Report), ImportError> {
    map::import_with_report(bytes, limits)
}
