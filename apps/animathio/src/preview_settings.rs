//! Persisted preview renderer preference ("GPU preview" in the View menu).
//! `ANIMATHIO_RENDER=cpu|gpu` overrides the stored setting for one run.

use std::path::PathBuf;

use animathio_render::BackendKind;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct PreviewSettings {
    /// Render the preview with the GPU backend when an adapter is available.
    pub(crate) gpu: bool,
}

impl Default for PreviewSettings {
    fn default() -> Self {
        Self { gpu: true }
    }
}

impl PreviewSettings {
    fn config_path() -> Option<PathBuf> {
        crate::editor_env::config_file("preview.json")
    }

    /// Stored setting, then the `ANIMATHIO_RENDER` override.
    pub(crate) fn load() -> Self {
        let stored = Self::config_path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Self::with_env(stored, std::env::var("ANIMATHIO_RENDER").ok().as_deref())
    }

    fn with_env(stored: Self, env: Option<&str>) -> Self {
        match env.and_then(BackendKind::parse) {
            Some(kind) => Self {
                gpu: kind == BackendKind::Gpu,
            },
            None => stored,
        }
    }

    /// Persist the setting (best effort, like the dock layout).
    pub(crate) fn save(self) {
        if let Some(path) = Self::config_path() {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(text) = serde_json::to_string(&self) {
                let _ = std::fs::write(path, text);
            }
        }
    }

    pub(crate) fn backend(self) -> BackendKind {
        if self.gpu {
            BackendKind::Gpu
        } else {
            BackendKind::Cpu
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_overrides_stored_setting() {
        let on = PreviewSettings { gpu: true };
        let off = PreviewSettings { gpu: false };
        assert_eq!(PreviewSettings::with_env(on, Some("cpu")), off);
        assert_eq!(PreviewSettings::with_env(off, Some("GPU")), on);
        assert_eq!(PreviewSettings::with_env(off, Some("bogus")), off);
        assert_eq!(PreviewSettings::with_env(on, None), on);
    }

    #[test]
    fn missing_fields_default_to_gpu() {
        let parsed: PreviewSettings = serde_json::from_str("{}").unwrap();
        assert!(parsed.gpu);
        assert_eq!(parsed.backend(), BackendKind::Gpu);
    }
}
