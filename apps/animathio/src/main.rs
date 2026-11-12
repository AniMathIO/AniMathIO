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

mod control;
mod editor;
mod export_worker;
mod inspector;
mod playback;
mod render_worker;
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
    gpui_platform::application().run(move |cx: &mut App| {
        gpui_component::init(cx);
        cx.on_window_closed(|cx, _window_id| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(1400.), px(900.)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                move |window, cx| {
                    cx.new(|cx| {
                        editor::Editor::new(
                            cx, window, project, assets, path, recovered, control, bench,
                        )
                    })
                },
            )
            .expect("open editor window");

        let view = window.update(cx, |_, _, cx| cx.entity()).unwrap();
        cx.intercept_keystrokes(move |event, _window, cx| {
            view.update(cx, |editor, cx| editor.handle_keystroke(event, cx));
        })
        .detach();

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
    let Some(arg) = std::env::args()
        .skip(1)
        .find(|arg| arg != "--control" && arg != "--bench-viewport")
    else {
        let sample = sample::build().expect("build sample project");
        return (sample.project, sample.assets, None, false);
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
