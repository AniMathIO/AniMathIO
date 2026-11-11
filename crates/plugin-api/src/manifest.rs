use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Error validating or parsing a plugin manifest.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ManifestError {
    /// Required field is empty or missing.
    #[error("missing required field: {0}")]
    MissingField(&'static str),
    /// Invalid API version string.
    #[error("unsupported api version: {0}")]
    UnsupportedApiVersion(String),
    /// Invalid plugin ID (must be alphanumeric with dots/hyphens).
    #[error("invalid plugin id: {0}")]
    InvalidId(String),
    /// Invalid semantic version string.
    #[error("invalid semver version: {0}")]
    InvalidVersion(String),
    /// TOML deserialization error.
    #[error("toml parse error: {0}")]
    ParseError(String),
}

/// Plugin kind declaration matching WIT plugin-kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PluginKind {
    /// Generates vector graphics scene.
    ClipSource,
    /// Transforms an input vector scene.
    Effect,
    /// Evaluates a 1D value animation curve.
    Animation,
}

/// Plugin manifest stored as `plugin.toml` inside `.amplugin` packages.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginManifestToml {
    /// Plugin API version contract, e.g. "0.1.0".
    pub api_version: String,
    /// Reverse-domain plugin identifier, e.g. "org.animathio.shapes".
    pub id: String,
    /// Human-readable plugin name.
    pub name: String,
    /// Plugin semantic version, e.g. "0.1.0".
    pub version: String,
    /// Short description of plugin capabilities.
    #[serde(default)]
    pub description: String,
    /// Plugin kinds implemented by this component.
    pub kinds: Vec<PluginKind>,
    /// Capabilities requested from the host (e.g. "network", "filesystem"). Default is empty.
    #[serde(default)]
    pub capabilities: Vec<String>,
}

impl PluginManifestToml {
    /// Parse and validate a `plugin.toml` string.
    pub fn parse(toml_str: &str) -> Result<Self, ManifestError> {
        let manifest: Self =
            toml::from_str(toml_str).map_err(|e| ManifestError::ParseError(e.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Serialize the manifest to a TOML string.
    pub fn to_toml_string(&self) -> Result<String, ManifestError> {
        self.validate()?;
        toml::to_string_pretty(self).map_err(|e| ManifestError::ParseError(e.to_string()))
    }

    /// Validate fields according to the AniMathIO 2.0 specification.
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.api_version.is_empty() {
            return Err(ManifestError::MissingField("api_version"));
        }
        if !self.api_version.starts_with("0.1") {
            return Err(ManifestError::UnsupportedApiVersion(
                self.api_version.clone(),
            ));
        }
        if self.id.is_empty() {
            return Err(ManifestError::MissingField("id"));
        }
        if !self
            .id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
        {
            return Err(ManifestError::InvalidId(self.id.clone()));
        }
        if self.name.is_empty() {
            return Err(ManifestError::MissingField("name"));
        }
        if self.version.is_empty() {
            return Err(ManifestError::MissingField("version"));
        }
        if self.kinds.is_empty() {
            return Err(ManifestError::MissingField("kinds"));
        }
        Ok(())
    }
}
