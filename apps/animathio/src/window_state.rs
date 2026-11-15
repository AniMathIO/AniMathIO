//! Persisted main-window geometry: size, position and maximized flag.
//!
//! Stored as `window-state.json` through [`crate::editor_env::config_file`], so tests get a
//! private file via [`crate::editor_env::EditorEnv::headless`]. The first run has no stored
//! state and opens maximized (with a 90%-of-display restore size); later runs restore the
//! saved geometry, clamped to the display that is actually present so the window never opens
//! off-screen after a monitor change.

use serde::{Deserialize, Serialize};

use gpui::{Bounds, Pixels, Size, point, px, size};

/// The smallest main window we allow, also the floor when clamping restored geometry.
pub(crate) const MIN_WINDOW_SIZE: Size<Pixels> = size(px(1280.), px(720.));

pub(crate) const FILE_NAME: &str = "window-state.json";

/// The window geometry saved when the main window closes and restored on the next start.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct WindowState {
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) maximized: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            width: 1280.,
            height: 720.,
            x: 0.,
            y: 0.,
            maximized: false,
        }
    }
}

impl WindowState {
    /// Capture the state a window should reopen with next time.
    pub(crate) fn from_window(window: &gpui::Window) -> Self {
        let bounds = window.window_bounds().get_bounds();
        Self {
            width: f32::from(bounds.size.width),
            height: f32::from(bounds.size.height),
            x: f32::from(bounds.origin.x),
            y: f32::from(bounds.origin.y),
            maximized: matches!(
                window.window_bounds(),
                gpui::WindowBounds::Maximized(_) | gpui::WindowBounds::Fullscreen(_)
            ),
        }
    }

    /// The stored state, if any.
    pub(crate) fn load() -> Option<Self> {
        let path = crate::editor_env::config_file(FILE_NAME)?;
        Self::read_from(&path)
    }

    /// Store the state for the next start. Best effort: failures are reported, not fatal.
    pub(crate) fn save(&self) -> Result<(), String> {
        let path = crate::editor_env::config_file(FILE_NAME)
            .ok_or("configuration directory unavailable")?;
        self.write_to(&path)
    }

    /// Capture the current window geometry and store it for the next start. Best effort.
    pub(crate) fn persist(window: &gpui::Window) {
        if let Err(error) = WindowState::from_window(window).save() {
            eprintln!("warning: could not save window state: {error}");
        }
    }

    fn read_from(path: &std::path::Path) -> Option<Self> {
        let bytes = std::fs::read(path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    fn write_to(&self, path: &std::path::Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, bytes)
            .and_then(|()| std::fs::rename(temporary, path))
            .map_err(|e| e.to_string())
    }

    fn bounds(&self) -> Bounds<Pixels> {
        Bounds {
            origin: point(px(self.x), px(self.y)),
            size: size(px(self.width), px(self.height)),
        }
    }
}

/// Clamp `bounds` so the window fits on `display`: never smaller than `min`, never larger
/// than the display, and positioned fully inside it (pinned to the display origin when the
/// display cannot even fit the minimum).
pub(crate) fn clamp_bounds(
    bounds: Bounds<Pixels>,
    display: Bounds<Pixels>,
    min: Size<Pixels>,
) -> Bounds<Pixels> {
    let min = size(
        min.width.min(display.size.width),
        min.height.min(display.size.height),
    );
    let size = size(
        bounds.size.width.max(min.width).min(display.size.width),
        bounds.size.height.max(min.height).min(display.size.height),
    );
    let max_origin = point(
        display.origin.x + display.size.width - size.width,
        display.origin.y + display.size.height - size.height,
    );
    let origin = point(
        bounds.origin.x.max(display.origin.x).min(max_origin.x),
        bounds.origin.y.max(display.origin.y).min(max_origin.y),
    );
    Bounds { origin, size }
}

/// The `WindowBounds` the main window opens with.
///
/// No stored state (first run) opens maximized with a 90%-of-display restore size. A stored
/// state restores its geometry, clamped to the current display, keeping the maximized flag.
pub(crate) fn initial_window_bounds(
    state: Option<WindowState>,
    display: Bounds<Pixels>,
    min: Size<Pixels>,
) -> gpui::WindowBounds {
    let maximized = state.is_none_or(|state| state.maximized);
    let restore = if let Some(state) = state {
        clamp_bounds(state.bounds(), display, min)
    } else {
        let requested = size(display.size.width * 0.9, display.size.height * 0.9);
        let restore_size = size(
            requested.width.max(min.width),
            requested.height.max(min.height),
        );
        Bounds {
            origin: point(
                display.origin.x + (display.size.width - restore_size.width) / 2.,
                display.origin.y + (display.size.height - restore_size.height) / 2.,
            ),
            size: restore_size,
        }
    };
    if maximized {
        gpui::WindowBounds::Maximized(restore)
    } else {
        gpui::WindowBounds::Windowed(restore)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds {
            origin: point(px(x), px(y)),
            size: size(px(w), px(h)),
        }
    }

    const MIN: Size<Pixels> = size(px(1280.), px(720.));

    #[test]
    fn clamps_oversized_origin_back_onscreen() {
        let display = bounds(0., 0., 2560., 1440.);
        let clamped = clamp_bounds(bounds(2400., 1300., 1400., 900.), display, MIN);
        assert_eq!(clamped, bounds(2560. - 1400., 1440. - 900., 1400., 900.));
    }

    #[test]
    fn clamps_negative_origin_back_onscreen() {
        let display = bounds(0., 0., 2560., 1440.);
        let clamped = clamp_bounds(bounds(-300., -100., 1400., 900.), display, MIN);
        assert_eq!(clamped.origin, point(px(0.), px(0.)));
        assert_eq!(clamped.size, size(px(1400.), px(900.)));
    }

    #[test]
    fn enforces_minimum_size() {
        let display = bounds(0., 0., 2560., 1440.);
        let clamped = clamp_bounds(bounds(100., 100., 400., 300.), display, MIN);
        assert_eq!(clamped.size, MIN);
        assert_eq!(clamped.origin, point(px(100.), px(100.)));
    }

    #[test]
    fn clamps_to_secondary_display_origin() {
        let display = bounds(2560., 0., 1920., 1080.);
        let clamped = clamp_bounds(bounds(100., 100., 1400., 900.), display, MIN);
        assert_eq!(clamped, bounds(2560., 100., 1400., 900.));
    }

    #[test]
    fn shrinks_window_larger_than_display() {
        let display = bounds(0., 0., 1920., 1080.);
        let clamped = clamp_bounds(bounds(0., 0., 3000., 1600.), display, MIN);
        assert_eq!(clamped.size, size(px(1920.), px(1080.)));
    }

    #[test]
    fn tiny_display_still_gets_minimum() {
        let display = bounds(0., 0., 800., 600.);
        let clamped = clamp_bounds(bounds(0., 0., 800., 600.), display, MIN);
        assert_eq!(clamped.size, size(px(800.), px(600.)));
        assert_eq!(clamped.origin, point(px(0.), px(0.)));
    }

    #[test]
    fn first_run_opens_maximized_with_large_restore_size() {
        let display = bounds(0., 0., 2560., 1440.);
        let gpui::WindowBounds::Maximized(restore) = initial_window_bounds(None, display, MIN)
        else {
            panic!("first run should open maximized");
        };
        assert_eq!(restore.size, size(px(2304.), px(1296.)));
        assert_eq!(restore.origin, point(px(128.), px(72.)));
    }

    #[test]
    fn restores_windowed_state_clamped() {
        let display = bounds(0., 0., 2560., 1440.);
        let state = WindowState {
            width: 1600.,
            height: 1000.,
            x: 5000.,
            y: 20.,
            maximized: false,
        };
        let gpui::WindowBounds::Windowed(restore) =
            initial_window_bounds(Some(state), display, MIN)
        else {
            panic!("windowed state should restore windowed");
        };
        assert_eq!(restore.size, size(px(1600.), px(1000.)));
        assert_eq!(restore.origin, point(px(2560. - 1600.), px(20.)));
    }

    #[test]
    fn restores_maximized_flag() {
        let display = bounds(0., 0., 2560., 1440.);
        let state = WindowState {
            width: 1600.,
            height: 1000.,
            x: 100.,
            y: 100.,
            maximized: true,
        };
        assert!(matches!(
            initial_window_bounds(Some(state), display, MIN),
            gpui::WindowBounds::Maximized(_)
        ));
    }

    #[test]
    fn persistence_round_trip() {
        let dir = std::env::temp_dir().join(format!(
            "animathio-window-state-test-{}",
            std::process::id()
        ));
        let path = dir.join(FILE_NAME);
        std::fs::remove_dir_all(&dir).ok();
        assert!(WindowState::read_from(&path).is_none());

        let state = WindowState {
            width: 1706.6,
            height: 1012.3,
            x: 42.5,
            y: -7.25,
            maximized: true,
        };
        state.write_to(&path).expect("write state");
        let loaded = WindowState::read_from(&path).expect("reload state");
        assert_eq!(loaded, state);

        // A garbage file loads as no state instead of failing to start.
        std::fs::write(&path, b"not json").unwrap();
        assert!(WindowState::read_from(&path).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
