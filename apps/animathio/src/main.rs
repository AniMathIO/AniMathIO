//! `AniMathIO` desktop editor shell: a thin GPUI view over the command-only core.

// GPUI elements are ergonomic with wildcard imports and pixel/tick casts; this
// crate is a UI shell, not a reusable library.
#![allow(
    clippy::wildcard_imports,
    clippy::unreadable_literal,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

mod control;
mod editor;
mod playback;
mod render_worker;
mod timeline;

use std::collections::BTreeMap;
use std::path::PathBuf;

use animathio_core::{AssetId, Project};
use animathio_render::sample;
use gpui::{prelude::*, *};

fn main() {
    let (project, assets, path) = initial_project();
    let control = if control_enabled() {
        let (bridge, rx) = control::ControlBridge::new();
        match control::start(bridge) {
            Some(server) => Some(control::ControlRuntime { rx, server }),
            None => None,
        }
    } else {
        None
    };
    Application::new().run(move |cx: &mut App| {
        cx.on_window_closed(|cx| {
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
                        editor::Editor::new(cx, window, project, assets, path, control)
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

/// `animathio [--control] [file.amio]` opens a project; no argument opens the
/// shared sample.
fn initial_project() -> (Project, BTreeMap<AssetId, Vec<u8>>, Option<PathBuf>) {
    let Some(arg) = std::env::args().skip(1).find(|arg| arg != "--control") else {
        let sample = sample::build().expect("build sample project");
        return (sample.project, sample.assets, None);
    };
    let path = PathBuf::from(arg);
    match editor::read_project(&path) {
        Ok((project, assets)) => (project, assets, Some(path)),
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
    }
}
