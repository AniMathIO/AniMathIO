//! `AniMathIO` desktop editor shell: a thin GPUI view over the command-only core.

// GPUI elements are ergonomic with wildcard imports and pixel/tick casts; this
// crate is a UI shell, not a reusable library.
#![allow(
    clippy::wildcard_imports,
    clippy::unreadable_literal,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::struct_excessive_bools,
    clippy::needless_pass_by_value,
    clippy::ref_option,
    clippy::too_many_lines,
    clippy::too_many_arguments
)]

mod clipboard;
mod control;
mod editing;
mod editor;
mod editor_env;
mod export_queue;
mod export_worker;
mod fades;
mod glyph_guard;
mod icons;
mod inspector;
mod layout;
mod marker_edit;
mod playback;
mod preferences;
mod preview_settings;
mod render_worker;
mod ripple;
mod selection;
mod speed_ramp;
mod style;
mod style_guard;
mod subtitle_job;
mod timecode;
mod timeline;
mod window_state;

use std::collections::BTreeMap;
use std::path::PathBuf;

use animathio_core::{AssetId, Project};
use animathio_render::sample;
use gpui::{prelude::*, *};

fn main() {
    if std::env::args().any(|arg| arg == "--version" || arg == "-V") {
        println!("animathio {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let (project, assets, path, recovered) = initial_project();
    let bench = bench_viewport_enabled();
    let control = if control_enabled() {
        let (bridge, rx) = control::ControlBridge::new();
        let (server, error) = match control::start(bridge) {
            Ok(server) => (Some(server), None),
            Err(error) => (None, Some(error)),
        };
        Some(control::ControlRuntime { rx, server, error })
    } else {
        None
    };
    gpui_platform::application()
        .with_assets(style::Assets)
        .run(move |cx: &mut App| {
            style::init_components(cx);
            let preferences = preferences::Preferences::load();
            style::install_theme(cx, preferences.compact, preferences.reduce_motion);
            cx.on_window_closed(|cx, _window_id| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();

            let displays = cx.displays();
            let saved = window_state::WindowState::load();
            let primary = cx.primary_display();
            let primary_index = displays
                .iter()
                .position(|display| {
                    primary
                        .as_ref()
                        .is_some_and(|primary| primary.id() == display.id())
                })
                .unwrap_or(0);
            let ids: Vec<_> = displays
                .iter()
                .map(|display| display.uuid().ok().map(|uuid| *uuid.as_bytes()))
                .collect();
            let index = window_state::restore_display(saved, &ids, primary_index);
            let display = displays.get(index);
            let display_bounds = display.map_or(
                Bounds {
                    origin: point(px(0.), px(0.)),
                    size: size(px(2560.), px(1440.)),
                },
                |display| window_state::native_display_bounds(display.as_ref()),
            );
            // The native minimum never exceeds the selected display's usable area, so a
            // small display still gets a usable window (smaller than 1280x720 there).
            let min_size =
                window_state::min_size_for_display(window_state::MIN_WINDOW_SIZE, display_bounds);
            let window_bounds =
                window_state::initial_window_bounds(saved, display_bounds, min_size);
            editor::actions::init(cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(window_bounds),
                    display_id: display.map(|display| display.id()),
                    window_min_size: Some(min_size),
                    window_decorations: Some(WindowDecorations::Client),
                    ..gpui_component::TitleBar::window_options()
                },
                move |window, cx| {
                    let view = cx.new(|cx| {
                        editor::Editor::new(
                            cx, window, project, assets, path, recovered, control, bench,
                        )
                    });
                    view.update(cx, |editor, cx| {
                        if let Some(panel) = std::env::args().find_map(|arg| {
                            arg.strip_prefix("--screenshot-panel=").map(str::to_owned)
                        }) {
                            editor.open_screenshot_panel(&panel, window, cx);
                        }
                        let mut local = window_bounds.get_bounds();
                        local.origin -= display_bounds.origin;
                        let seed = if matches!(window_bounds, gpui::WindowBounds::Maximized(_)) {
                            gpui::WindowBounds::Maximized(local)
                        } else {
                            gpui::WindowBounds::Windowed(local)
                        };
                        editor.seed_window_tracker(seed, ids.get(index).copied().flatten());
                    });
                    // `Root` hosts the popover layers that gpui-component widgets
                    // (select, colour picker) render into.
                    cx.new(|cx| gpui_component::Root::new(view, window, cx))
                },
            )
            .expect("open editor window");

            cx.activate(true);
        });
}

/// Whether the control server should run: `--control` or `ANIMATHIO_CONTROL=1`.
fn control_enabled() -> bool {
    std::env::args().any(|arg| arg == "--control")
        || std::env::var("ANIMATHIO_CONTROL").is_ok_and(|value| value == "1")
}

/// Whether to run the viewport presentation benchmark: `--bench-viewport`.
fn bench_viewport_enabled() -> bool {
    std::env::args().any(|arg| arg == "--bench-viewport")
}

/// `animathio [--control] [--bench-viewport] [file.amio]` opens a project; no
/// argument opens the shared sample.
fn initial_project() -> (Project, BTreeMap<AssetId, Vec<u8>>, Option<PathBuf>, bool) {
    let Some(arg) = std::env::args().skip(1).find(|arg| !arg.starts_with("--")) else {
        if std::env::args().any(|arg| arg == "--sample" || arg == "--bench-viewport") {
            let sample = sample::build().expect("build sample project");
            return (sample.project, sample.assets, None, false);
        }
        return (
            Project::new(animathio_core::FrameRate::new(30, 1).expect("30 fps")),
            BTreeMap::new(),
            None,
            false,
        );
    };
    let path = PathBuf::from(arg);
    match editor::read_project_with_recovery(&path) {
        Ok((project, assets, recovered)) => (project, assets, Some(path), recovered),
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
    }
}
