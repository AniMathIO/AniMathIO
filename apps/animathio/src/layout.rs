//! Persisted dockable panel arrangement. The layout is view-only state: panel
//! sizes and open flags live here, never in the project model.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Sizes (logical pixels) and open/closed flags for the dockable panels.
/// Sizes are clamped on load so a hand-edited or stale file cannot produce a
/// degenerate layout.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DockLayout {
    /// Left panel (media pool) width in logical pixels.
    pub left_width: f32,
    /// Right panel (inspector) width in logical pixels.
    pub right_width: f32,
    /// Bottom panel (timeline) height in logical pixels.
    pub bottom_height: f32,
    /// Whether the left panel is currently open.
    pub left_open: bool,
    /// Whether the right panel is currently open.
    pub right_open: bool,
    /// Whether the bottom panel is currently open.
    pub bottom_open: bool,
}

impl Default for DockLayout {
    fn default() -> Self {
        Self {
            left_width: 260.0,
            right_width: 300.0,
            bottom_height: 240.0,
            left_open: true,
            right_open: true,
            bottom_open: true,
        }
    }
}

impl DockLayout {
    /// Round-trip through the JSON string form used for the on-disk file.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("dock layout serializes")
    }

    /// Parse from the on-disk JSON form, falling back to the default layout.
    #[must_use]
    pub fn from_json(text: &str) -> Self {
        serde_json::from_str(text).unwrap_or_default()
    }

    /// Config file under the XDG config dir, next to the recent-files list.
    #[must_use]
    pub fn config_path() -> Option<PathBuf> {
        directories::ProjectDirs::from("org", "AniMathIO", "AniMathIO")
            .map(|dirs| dirs.config_dir().join("dock-layout.json"))
    }

    /// Load the persisted layout, falling back to the default and clamping.
    #[must_use]
    pub fn load() -> Self {
        let mut layout = Self::config_path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map_or_else(Self::default, |text| Self::from_json(&text));
        layout.clamp();
        layout
    }

    /// Persist the layout, ignoring IO errors (best effort, like autosave).
    pub fn save(&self) {
        let Some(path) = Self::config_path() else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, self.to_json());
    }

    /// Clamp panel sizes to sane bounds.
    pub fn clamp(&mut self) {
        self.left_width = self.left_width.clamp(160.0, 640.0);
        self.right_width = self.right_width.clamp(200.0, 720.0);
        self.bottom_height = self.bottom_height.clamp(120.0, 600.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_round_trips_through_json() {
        let layout = DockLayout::default();
        let text = layout.to_json();
        let parsed = DockLayout::from_json(&text);
        assert_eq!(parsed, layout);
    }

    #[test]
    fn custom_layout_round_trips_through_json() {
        let layout = DockLayout {
            left_width: 320.0,
            right_width: 480.0,
            bottom_height: 300.0,
            left_open: false,
            right_open: true,
            bottom_open: false,
        };
        let parsed = DockLayout::from_json(&layout.to_json());
        assert_eq!(parsed, layout);
    }

    #[test]
    fn clamps_degenerate_sizes() {
        let mut layout = DockLayout {
            left_width: 1.0,
            right_width: 10_000.0,
            bottom_height: -5.0,
            ..DockLayout::default()
        };
        layout.clamp();
        assert_eq!(layout.left_width, 160.0);
        assert_eq!(layout.right_width, 720.0);
        assert_eq!(layout.bottom_height, 120.0);
    }

    #[test]
    fn invalid_json_falls_back_to_default() {
        assert_eq!(DockLayout::from_json("not json"), DockLayout::default());
        assert_eq!(DockLayout::from_json("{}"), DockLayout::default());
    }
}
