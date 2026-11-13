//! Delivery presets shared by the CLI, MCP and editor.
use crate::ControlError;
use animathio_core::{Command, FrameRate, Project};
use serde::{Deserialize, Serialize};

/// Delivery presets for `set_export_preset`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportPreset {
    /// 1920x1080 at 30 fps.
    #[serde(rename = "youtube_1080p")]
    Youtube1080p,
    /// 3840x2160 at 30 fps.
    #[serde(rename = "youtube_4k")]
    Youtube4k,
    /// 1080x1920 vertical at 30 fps (`YouTube` Shorts, Reels, `TikTok`).
    ShortsVertical,
    /// 1080x1080 at 30 fps.
    Square,
    /// 1080x1350 portrait at 30 fps.
    Portrait,
}

impl ExportPreset {
    /// Shared delivery preset table for the editor, CLI and MCP.
    pub const ALL: [Self; 5] = [
        Self::Youtube1080p,
        Self::Youtube4k,
        Self::ShortsVertical,
        Self::Square,
        Self::Portrait,
    ];

    /// Stable name used in JSON and user-facing preset selectors.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Youtube1080p => "youtube_1080p",
            Self::Youtube4k => "youtube_4k",
            Self::ShortsVertical => "shorts_vertical",
            Self::Square => "square",
            Self::Portrait => "portrait",
        }
    }

    /// Human-readable delivery choice.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Youtube1080p => "YouTube 1920x1080 30fps",
            Self::Youtube4k => "4K 3840x2160 30fps",
            Self::ShortsVertical => "Shorts/Reels/TikTok 1080x1920 30fps",
            Self::Square => "Square 1080x1080 30fps",
            Self::Portrait => "Portrait 4:5 1080x1350 30fps",
        }
    }

    /// Resolve the stable name shared by CLI, MCP and editor.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|preset| preset.name() == name)
    }

    /// Output `(width, height)`.
    #[must_use]
    pub const fn resolution(self) -> (u32, u32) {
        match self {
            Self::Youtube1080p => (1920, 1080),
            Self::Youtube4k => (3840, 2160),
            Self::ShortsVertical => (1080, 1920),
            Self::Square => (1080, 1080),
            Self::Portrait => (1080, 1350),
        }
    }

    /// Encoder settings for this preset (H.264 CRF 18, the project frame rate).
    #[must_use]
    pub fn export_settings(self) -> animathio_export::Settings {
        animathio_export::Settings {
            frame_rate: None,
            crf: 18.0,
        }
    }

    /// The `SetSettings` command applying this preset's resolution at 30 fps,
    /// keeping the background and explicit duration.
    pub fn command(self, project: &Project) -> Result<Command, ControlError> {
        let (width, height) = self.resolution();
        Ok(Command::SetSettings {
            width,
            height,
            frame_rate: FrameRate::new(30, 1)?,
            background: project.background(),
            duration: project.duration(),
        })
    }
}
