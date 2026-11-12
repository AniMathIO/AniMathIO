//! One-way importer for Manim Community scene scripts (`.py`).
//!
//! Parses a supported subset of Manim scenes with tree-sitter + tree-sitter-python
//! (not regex) and rebuilds a v2 [`animathio_core::Project`] exclusively through
//! [`animathio_core::Command`]s. Unsupported Python constructs are reported as
//! warnings, never errors: the importer degrades gracefully rather than failing a
//! whole script because of one construct it does not understand.

// Manim coordinates, colours and seconds are exact-but-lossy float inputs that
// must be mapped to integer ticks and normalized fractions; bounds are checked
// where they matter, and the residual precision/sign casts are inherent to the
// translation.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

mod ast;
mod eval;
mod translate;

use animathio_core::{AssetId, Project};
use std::path::PathBuf;

/// Straight-alpha sRGB colour used throughout the importer.
pub(crate) type Rgba = [u8; 4];

/// Failures that prevent a Manim scene from being imported.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    /// The requested scene class is not defined in the source.
    #[error("scene {0:?} not found in script")]
    SceneNotFound(String),
    /// No `Scene` subclass was found in the source.
    #[error("no Scene subclass found in script")]
    NoScene,
    /// The source is structurally unusable.
    #[error("invalid Manim script: {0}")]
    Invalid(String),
    /// The rebuilt v2 project failed validation.
    #[error(transparent)]
    Model(#[from] animathio_core::Error),
}

/// Resource limits for importing untrusted Manim scripts.
#[derive(Clone, Copy, Debug)]
pub struct ImportLimits {
    /// Maximum source bytes accepted by the importer.
    pub source_bytes: u64,
}
impl Default for ImportLimits {
    fn default() -> Self {
        Self {
            source_bytes: 16 * 1024 * 1024,
        }
    }
}

/// Options controlling Manim import.
#[derive(Clone, Debug, Default)]
pub struct ManimOptions {
    /// Import only this scene class; by default the first `Scene` subclass is used.
    pub scene: Option<String>,
    /// Directory used to resolve `ImageMobject` relative paths (the script's folder).
    pub script_dir: Option<PathBuf>,
}

/// A non-fatal observation with a source location and the offending snippet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    /// One-based line number.
    pub line: usize,
    /// One-based column number.
    pub column: usize,
    /// Human-readable description.
    pub message: String,
    /// The offending source line, trimmed.
    pub snippet: String,
}
impl std::fmt::Display for Warning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}: {} ({})",
            self.line, self.column, self.message, self.snippet
        )
    }
}

/// Non-fatal observations produced while importing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub(crate) warnings: Vec<Warning>,
}
impl Report {
    /// Warnings, in the order they were produced.
    #[must_use]
    pub fn warnings(&self) -> &[Warning] {
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
    pub(crate) fn warn(
        &mut self,
        line: usize,
        column: usize,
        message: impl Into<String>,
        snippet: impl Into<String>,
    ) {
        self.warnings.push(Warning {
            line,
            column,
            message: message.into(),
            snippet: snippet.into(),
        });
    }
}

/// One decoded, content-addressed image asset referenced by the scene.
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

/// Import a Manim scene script, discarding warnings.
///
/// See [`import_manim_with_options`] for the warning-carrying variant.
pub fn import_manim(source: &str) -> Result<(Project, Vec<Asset>), ImportError> {
    import_manim_with_options(source, &ManimOptions::default())
        .map(|(project, assets, _)| (project, assets))
}

/// Import a Manim scene script, keeping non-fatal warnings.
pub fn import_manim_with_options(
    source: &str,
    options: &ManimOptions,
) -> Result<(Project, Vec<Asset>, Report), ImportError> {
    import_manim_with_limits(source, options, &ImportLimits::default())
}

/// Import with explicit resource limits, keeping non-fatal warnings.
pub fn import_manim_with_limits(
    source: &str,
    options: &ManimOptions,
    limits: &ImportLimits,
) -> Result<(Project, Vec<Asset>, Report), ImportError> {
    if u64::try_from(source.len()).unwrap_or(u64::MAX) > limits.source_bytes {
        return Err(ImportError::Invalid(format!(
            "source exceeds the {} byte limit",
            limits.source_bytes
        )));
    }
    translate::translate(source, options)
}
