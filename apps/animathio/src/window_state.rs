//! Persisted main-window geometry: size, position and maximized flag.
//!
//! Stored as `window-state.json` through [`crate::editor_env::config_file`], so tests get a
//! private file via [`crate::editor_env::EditorEnv::headless`]. The first run has no stored
//! state and opens maximized (with a 90%-of-display restore size, clamped to the display);
//! later runs restore the saved geometry on the display it overlaps, clamped to that
//! display's usable area, so the window never opens off-screen after a monitor change.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use gpui::{Bounds, Pixels, Size, point, px, size};

/// The smallest main window we allow: the floor when clamping restored geometry and the
/// default native minimum. The effective minimum shrinks on smaller displays, see
/// [`min_size_for_display`].
pub(crate) const MIN_WINDOW_SIZE: Size<Pixels> = size(px(1280.), px(720.));

pub(crate) const FILE_NAME: &str = "window-state.json";

/// How long the window geometry must stay still before the debounced save writes.
pub(crate) const SAVE_DEBOUNCE: Duration = Duration::from_millis(500);

/// Counter making every write's temporary file unique, so concurrent writers never share
/// one temp file (a truncate-then-rename race would corrupt the destination).
static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

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

    fn read_from(path: &std::path::Path) -> Option<Self> {
        let bytes = std::fs::read(path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    fn write_to(&self, path: &std::path::Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        // A unique temporary per write: the debounced save, the close path and the quit
        // hook can overlap, and writers sharing one temp name would truncate each other's
        // file and rename a half-written state over the destination.
        let temporary = path.with_extension(format!(
            "json.tmp.{}.{}",
            std::process::id(),
            TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let written =
            std::fs::write(&temporary, &bytes).and_then(|()| std::fs::rename(&temporary, path));
        if written.is_err() {
            // Do not litter the config directory with dead temporaries.
            std::fs::remove_file(&temporary).ok();
        }
        written.map_err(|e| e.to_string())
    }

    /// The saved geometry as bounds.
    pub(crate) fn bounds(&self) -> Bounds<Pixels> {
        Bounds {
            origin: point(px(self.x), px(self.y)),
            size: size(px(self.width), px(self.height)),
        }
    }
}

/// Tracks the main window's geometry so it can be persisted.
///
/// The window backends disagree about maximized windows (pinned gpui 0.3.12): macOS
/// `Window::window_bounds()` answers `Windowed` even for maximized windows, so the
/// maximized flag must come from `Window::is_maximized()`; X11 answers `Maximized` carrying
/// the maximized bounds, so the restore geometry must come from the last observation where
/// the window was *not* maximized. The tracker therefore observes the window continuously
/// (window-bounds callbacks, not only at close) and keeps the last non-maximized bounds
/// separately from the maximized flag.
#[derive(Default)]
pub(crate) struct BoundsTracker {
    /// Last bounds observed while the window was neither maximized nor fullscreen: the
    /// restore geometry a maximized window should reopen with.
    last_windowed: Option<Bounds<Pixels>>,
    /// Bounds of the latest observation, whatever the window state.
    last_observed: Option<Bounds<Pixels>>,
    maximized: bool,
}

impl BoundsTracker {
    /// Record the current geometry of `window`.
    pub(crate) fn observe_window(&mut self, window: &gpui::Window) {
        self.observe(
            window_is_maximized(window),
            window.window_bounds().get_bounds(),
        );
    }

    /// Record one geometry observation (pure, so tests can replay backend quirks).
    pub(crate) fn observe(&mut self, maximized: bool, bounds: Bounds<Pixels>) {
        self.maximized = maximized;
        self.last_observed = Some(bounds);
        if !maximized {
            self.last_windowed = Some(bounds);
        }
    }

    /// The state to persist: the maximized flag from the platform (`is_maximized`,
    /// not `window_bounds`, which is unreliable on macOS) and, when maximized, the last
    /// non-maximized bounds instead of the maximized size (which X11 reports through
    /// `window_bounds`).
    pub(crate) fn state(&self) -> WindowState {
        let bounds = if self.maximized {
            self.last_windowed.or(self.last_observed)
        } else {
            self.last_observed
        }
        .unwrap_or_default();
        WindowState {
            width: f32::from(bounds.size.width),
            height: f32::from(bounds.size.height),
            x: f32::from(bounds.origin.x),
            y: f32::from(bounds.origin.y),
            maximized: self.maximized,
        }
    }

    /// Store the state for the next start. Best effort: failures are reported, not fatal.
    pub(crate) fn persist(&self) {
        if let Err(error) = self.state().save() {
            eprintln!("warning: could not save window state: {error}");
        }
    }
}

/// Whether the window is currently maximized or fullscreen. `window_bounds` alone is not
/// enough: the macOS backend answers `Windowed` for maximized windows.
fn window_is_maximized(window: &gpui::Window) -> bool {
    window.is_maximized() || window.is_fullscreen()
}

/// The native minimum window size for a display: never larger than the display's usable
/// area, so a small display still gets a resizable window instead of an unmovable one
/// bigger than the screen.
pub(crate) fn min_size_for_display(min: Size<Pixels>, display: Bounds<Pixels>) -> Size<Pixels> {
    size(
        min.width.min(display.size.width),
        min.height.min(display.size.height),
    )
}

/// The index of the display holding the largest part of `bounds` (`None` when the bounds
/// are on no connected display). Touching an edge is not an overlap.
pub(crate) fn largest_overlap(
    bounds: Bounds<Pixels>,
    displays: &[Bounds<Pixels>],
) -> Option<usize> {
    let right = bounds.origin.x + bounds.size.width;
    let bottom = bounds.origin.y + bounds.size.height;
    let mut best: Option<(usize, f64)> = None;
    for (index, display) in displays.iter().enumerate() {
        let width = right.min(display.origin.x + display.size.width)
            - bounds.origin.x.max(display.origin.x);
        let height = bottom.min(display.origin.y + display.size.height)
            - bounds.origin.y.max(display.origin.y);
        if width <= px(0.) || height <= px(0.) {
            continue;
        }
        let area = f64::from(f32::from(width)) * f64::from(f32::from(height));
        if best.is_none_or(|(_, best_area)| area > best_area) {
            best = Some((index, area));
        }
    }
    best.map(|(index, _)| index)
}

/// Clamp `bounds` so the window fits on `display`: never smaller than `min` (itself
/// shrunk to the display), never larger than the display, and positioned fully inside it
/// (pinned to the display origin when the display cannot even fit the minimum).
pub(crate) fn clamp_bounds(
    bounds: Bounds<Pixels>,
    display: Bounds<Pixels>,
    min: Size<Pixels>,
) -> Bounds<Pixels> {
    let min = min_size_for_display(min, display);
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
/// state restores its geometry, clamped to the given display (the one its saved bounds
/// overlap, else the primary), keeping the maximized flag. Both paths are clamped, so a
/// small display never receives oversized, off-screen geometry.
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
        let centered = Bounds {
            origin: point(
                display.origin.x + (display.size.width - requested.width) / 2.,
                display.origin.y + (display.size.height - requested.height) / 2.,
            ),
            size: requested,
        };
        // The 90% request floored at the minimum can still exceed a small display
        // (an 800x600 display floors 720x540 up to 1280x720), so clamp like any restore.
        clamp_bounds(centered, display, min)
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
    fn min_size_never_exceeds_the_display() {
        // The native minimum shrinks on small displays instead of making the window
        // unresizable (or unmovable) there.
        assert_eq!(
            min_size_for_display(MIN, bounds(0., 0., 800., 600.)),
            size(px(800.), px(600.))
        );
        assert_eq!(
            min_size_for_display(MIN, bounds(0., 0., 1024., 600.)),
            size(px(1024.), px(600.))
        );
        assert_eq!(min_size_for_display(MIN, bounds(0., 0., 2560., 1440.)), MIN);
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
    fn first_run_fits_a_small_display() {
        // An 800x600 display: the 90% request floored at the 1280x720 minimum must clamp
        // down to the display instead of opening oversized at a negative origin.
        let display = bounds(0., 0., 800., 600.);
        let gpui::WindowBounds::Maximized(restore) = initial_window_bounds(None, display, MIN)
        else {
            panic!("first run should open maximized");
        };
        assert_eq!(restore.size, size(px(800.), px(600.)));
        assert_eq!(restore.origin, point(px(0.), px(0.)));
        assert!(restore.origin.x >= display.origin.x && restore.origin.y >= display.origin.y);
    }

    #[test]
    fn first_run_fits_a_mid_display_with_off_origin() {
        // Off-origin displays (a lone secondary at x=2560) get in-display geometry too.
        let display = bounds(2560., 0., 1920., 1080.);
        let gpui::WindowBounds::Maximized(restore) = initial_window_bounds(None, display, MIN)
        else {
            panic!("first run should open maximized");
        };
        assert_eq!(restore.size, size(px(1728.), px(972.)));
        assert_eq!(restore.origin, point(px(2560. + 96.), px(54.)));
    }

    #[test]
    fn picks_the_display_the_bounds_overlap() {
        let primary = bounds(0., 0., 2560., 1440.);
        let secondary = bounds(2560., 0., 1920., 1080.);
        let displays = [primary, secondary];
        assert_eq!(
            largest_overlap(bounds(3000., 100., 1400., 900.), &displays),
            Some(1)
        );
        assert_eq!(
            largest_overlap(bounds(100., 100., 1400., 900.), &displays),
            Some(0)
        );
        // A window straddling both restores on the one holding most of it.
        assert_eq!(
            largest_overlap(bounds(2400., 100., 1400., 900.), &displays),
            Some(1)
        );
        // Fully off every display: no match (the caller falls back to the primary).
        assert_eq!(
            largest_overlap(bounds(9000., 9000., 1400., 900.), &displays),
            None
        );
    }

    #[test]
    fn secondary_monitor_position_is_kept() {
        // A saved window on the secondary display keeps its position when that display is
        // the restore target (clamping to the primary used to discard valid positions).
        let secondary = bounds(2560., 0., 1920., 1080.);
        let state = WindowState {
            width: 1400.,
            height: 900.,
            x: 3000.,
            y: 100.,
            maximized: false,
        };
        let gpui::WindowBounds::Windowed(restore) =
            initial_window_bounds(Some(state), secondary, MIN)
        else {
            panic!("windowed state should restore windowed");
        };
        assert_eq!(restore, bounds(3000., 100., 1400., 900.));
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
    fn maximized_window_keeps_its_last_normal_geometry() {
        // X11 reports the maximized size through `window_bounds`; the tracker must save
        // the last normal bounds instead.
        let mut tracker = BoundsTracker::default();
        tracker.observe(false, bounds(100., 100., 1400., 900.));
        tracker.observe(true, bounds(0., 0., 2560., 1440.));
        let state = tracker.state();
        assert!(state.maximized);
        assert_eq!((state.width, state.height), (1400., 900.));
        assert_eq!((state.x, state.y), (100., 100.));

        // Back to normal: the new geometry is captured again.
        tracker.observe(false, bounds(200., 200., 1200., 800.));
        let state = tracker.state();
        assert!(!state.maximized);
        assert_eq!((state.width, state.height), (1200., 800.));
        assert_eq!((state.x, state.y), (200., 200.));
    }

    #[test]
    fn first_observation_of_a_maximized_window_still_saves() {
        // Started maximized with no prior normal bounds: fall back to the observed
        // bounds (restored clamped, and the maximized flag re-maximizes anyway).
        let mut tracker = BoundsTracker::default();
        tracker.observe(true, bounds(0., 0., 2560., 1440.));
        let state = tracker.state();
        assert!(state.maximized);
        assert_eq!((state.width, state.height), (2560., 1440.));
    }

    #[test]
    fn concurrent_writers_leave_a_valid_file() {
        let dir = std::env::temp_dir().join(format!(
            "animathio-window-state-stress-{}",
            std::process::id()
        ));
        let path = dir.join(FILE_NAME);
        std::fs::remove_dir_all(&dir).ok();

        const WRITERS: usize = 8;
        const WRITES: usize = 25;
        let states: Vec<WindowState> = (0..WRITERS)
            .map(|writer| WindowState {
                width: 800. + writer as f32,
                height: 600. + writer as f32,
                x: writer as f32,
                y: writer as f32,
                maximized: writer % 2 == 0,
            })
            .collect();

        std::thread::scope(|scope| {
            for state in &states {
                let path = &path;
                scope.spawn(move || {
                    for _ in 0..WRITES {
                        state.write_to(path).expect("concurrent write");
                    }
                });
            }
        });

        let bytes = std::fs::read(&path).expect("destination exists");
        let loaded: WindowState = serde_json::from_slice(&bytes).expect("file is valid JSON");
        assert!(
            states.contains(&loaded),
            "the file holds one whole written state, got {loaded:?}"
        );
        let leftovers: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp."))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temporary files left behind: {leftovers:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
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
