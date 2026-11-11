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
    if std::env::args().any(|arg| arg == "--version" || arg == "-V") {
        println!("animathio {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let (project, assets, path, recovered) = initial_project();
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
                    cx.new(|cx| editor::Editor::new(cx, window, project, assets, path, recovered))
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

/// `animathio [file.amio]` opens a project; no argument opens the shared sample.
fn initial_project() -> (Project, BTreeMap<AssetId, Vec<u8>>, Option<PathBuf>, bool) {
    let Some(arg) = std::env::args().nth(1) else {
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
