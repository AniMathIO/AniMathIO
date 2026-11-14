//! `AniMathIO` desktop editor shell: a thin GPUI view over the command-only core.

// GPUI elements are ergonomic with wildcard imports and pixel/tick casts; this
// crate is a UI shell, not a reusable library.
// `#[gpui::test]` (src/editor/ui_tests) needs more macro-expansion recursion than
// the default 128.
#![recursion_limit = "256"]
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
mod playback;
mod preferences;
mod preview_settings;
mod render_worker;
mod ripple;
mod selection;
mod style;
mod subtitle_job;
mod timecode;
mod timeline;

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
        control::start(bridge).map(|server| control::ControlRuntime { rx, server })
    } else {
        None
    };
    gpui_platform::application()
        .with_assets(style::Assets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            let preferences = preferences::Preferences::load();
            style::install_theme(cx, preferences.compact, preferences.reduce_motion);
            cx.on_window_closed(|cx, _window_id| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();

            let bounds = Bounds::centered(None, size(px(1400.), px(900.)), cx);
            editor::actions::init(cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(900.), px(600.))),
                    window_decorations: Some(WindowDecorations::Client),
                    ..gpui_component::TitleBar::window_options()
                },
                move |window, cx| {
                    let view = cx.new(|cx| {
                        editor::Editor::new(
                            cx, window, project, assets, path, recovered, control, bench,
                        )
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
