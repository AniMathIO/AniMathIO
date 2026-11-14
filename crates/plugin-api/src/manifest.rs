use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Error validating or parsing a plugin manifest.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
#[non_exhaustive]
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
    /// Inconsistent plugin kinds.
    #[error("invalid plugin kinds: {0}")]
    InvalidKinds(String),
    /// TOML deserialization error.
    #[error("toml parse error: {0}")]
    ParseError(String),
}

/// Host-supported execution kinds. The WIT uses open string ids; unknown wire
/// kinds are advisory, but an unknown package execution kind is rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum PluginKind {
    /// Generates vector graphics scene.
    ClipSource,
    /// Pixel filter exported through the `effect-plugin` world (`frame-effects`).
    /// An effect plugin implements only that world, so it declares exactly this kind.
    Effect,
    /// Legacy classification; no animation execution world is implemented in 1.0.
    Animation,
    /// Audio filter exported through the `audio-effect-plugin` world (`audio-effects`).
    /// Like `effect`, an audio-effect plugin declares exactly this kind.
    AudioEffect,
}

/// Plugin manifest stored as `plugin.toml` inside `.amplugin` packages.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginManifestToml {
    /// Minimum ABI version required by the component. Stable 1.0.0 and the
    /// explicitly supported legacy releases 0.2.0, 0.2.1 and 0.2.2 are accepted.
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
        let api = semver::Version::parse(&self.api_version)
            .map_err(|_| ManifestError::UnsupportedApiVersion(self.api_version.clone()))?;
        if !api.pre.is_empty()
            || !api.build.is_empty()
            || !matches!((api.major, api.minor, api.patch), (0, 2, 0..=2) | (1, 0, 0))
        {
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
        if semver::Version::parse(&self.version).is_err() {
            return Err(ManifestError::InvalidVersion(self.version.clone()));
        }
        if self
            .kinds
            .iter()
            .enumerate()
            .any(|(i, kind)| self.kinds[..i].contains(kind))
        {
            return Err(ManifestError::InvalidKinds("duplicate kind".into()));
        }
        if self.kinds.is_empty() {
            return Err(ManifestError::MissingField("kinds"));
        }
        if api.major == 1 && self.kinds.contains(&PluginKind::Animation) {
            return Err(ManifestError::InvalidKinds(
                "animation execution has no 1.0 world; use native keyframes".into(),
            ));
        }
        if self.kinds.contains(&PluginKind::Effect) {
            if self.kinds.len() != 1 {
                return Err(ManifestError::InvalidKinds(
                    "an effect plugin implements only the effect-plugin world".to_owned(),
                ));
            }
            if self.api_version == "0.2.0" {
                return Err(ManifestError::UnsupportedApiVersion(
                    "effect plugins require api_version 0.2.1".to_owned(),
                ));
            }
        }
        if self.kinds.contains(&PluginKind::AudioEffect) {
            if self.kinds.len() != 1 {
                return Err(ManifestError::InvalidKinds(
                    "an audio-effect plugin implements only the audio-effect-plugin world"
                        .to_owned(),
                ));
            }
            if matches!(self.api_version.as_str(), "0.2.0" | "0.2.1") {
                return Err(ManifestError::UnsupportedApiVersion(
                    "audio-effect plugins require api_version 0.2.2".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

impl PluginKind {
    /// Stable WIT string identifier for this execution kind.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ClipSource => "clip-source",
            Self::Effect => "effect",
            Self::Animation => "animation",
            Self::AudioEffect => "audio-effect",
        }
    }
}
