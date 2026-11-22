//! Persisted dockable panel arrangement. The layout is view-only state: panel
//! sizes and open flags live here, never in the project model.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::editor_env::EditorEnv;

/// Persisted timeline density (logical pixels per track).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackHeight {
    Small,
    #[default]
    Medium,
    Large,
}
impl TrackHeight {
    pub fn pixels(self) -> f32 {
        let height = match self {
            Self::Small => 28.,
            Self::Medium => 44.,
            Self::Large => 72.,
        };
        height * crate::style::ui_scale()
    }
    pub const fn larger(self) -> Self {
        match self {
            Self::Small => Self::Medium,
            _ => Self::Large,
        }
    }
    pub const fn smaller(self) -> Self {
        match self {
            Self::Large => Self::Medium,
            _ => Self::Small,
        }
    }
}

/// The track height a first run starts with: small rows in a short window, so more than a few
/// tracks fit, unless a layout was already saved (then the user's choice stands).
#[must_use]
pub fn first_run_track_height(current: TrackHeight, compact: bool, saved: bool) -> TrackHeight {
    if compact && !saved {
        TrackHeight::Small
    } else {
        current
    }
}

/// Sizes (logical pixels) and open/closed flags for the dockable panels.
/// Sizes are clamped on load so a hand-edited or stale file cannot produce a
/// degenerate layout.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DockLayout {
    #[serde(default)]
    pub track_height: TrackHeight,
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
            track_height: TrackHeight::default(),
            left_width: 260.0,
            right_width: 340.0,
            bottom_height: 300.0,
            left_open: true,
            right_open: true,
            bottom_open: true,
        }
    }
}

impl DockLayout {
    /// Round-trip through the JSON string form used for the on-disk file.
    #[must_use]
    pub fn to_json(self) -> String {
        serde_json::to_string(&self).expect("dock layout serializes")
    }

    /// Parse from the on-disk JSON form, falling back to the default layout.
    #[must_use]
    pub fn from_json(text: &str) -> Self {
        serde_json::from_str(text).unwrap_or_default()
    }

    /// Config file under the XDG config dir, next to the recent-files list.
    #[must_use]
    pub(crate) fn config_path(env: &EditorEnv) -> Option<PathBuf> {
        env.config_file("dock-layout.json")
    }

    /// Load the persisted layout, falling back to the default and clamping.
    #[must_use]
    pub(crate) fn load(env: &EditorEnv) -> Self {
        let mut layout = Self::config_path(env)
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map_or_else(Self::default, |text| Self::from_json(&text));
        layout.clamp();
        layout
    }

    /// Persist the layout, ignoring IO errors (best effort, like autosave).
    pub(crate) fn save(&self, env: &EditorEnv) {
        let Some(path) = Self::config_path(env) else {
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
            track_height: TrackHeight::Large,
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
    fn short_first_runs_start_with_small_tracks_but_saved_layouts_win() {
        use TrackHeight::{Large, Medium, Small};
        assert_eq!(first_run_track_height(Medium, true, false), Small);
        assert_eq!(first_run_track_height(Medium, false, false), Medium);
        assert_eq!(first_run_track_height(Large, true, true), Large);
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
    fn existing_layout_files_default_to_medium_tracks() {
        let mut json = serde_json::to_value(DockLayout::default()).unwrap();
        json.as_object_mut().unwrap().remove("track_height");
        assert_eq!(
            DockLayout::from_json(&json.to_string()).track_height,
            TrackHeight::Medium
        );
    }

    #[test]
    fn invalid_json_falls_back_to_default() {
        assert_eq!(DockLayout::from_json("not json"), DockLayout::default());
        assert_eq!(DockLayout::from_json("{}"), DockLayout::default());
    }
}
