//! Window-level UI tests: the real [`Editor`] view in a headless gpui window.
//!
//! `gpui::test` runs on gpui's test platform (no display, no GPU, no audio device), so these run
//! in plain `cargo test` on Linux and Windows. [`Ui::open`] builds the editor on a headless
//! [`EditorEnv`] (null audio sink, CPU preview, built plugins or none, private config
//! directory), mounts it in a `Root` like `main` does, and returns a handle that dispatches
//! actions, drives the editor and reads its state. Assert on the project model and on view state
//! (`marker_panel`, `autocut_review`, `selected`, ...), not on pixels.
//!
//! ```ignore
//! #[gpui::test]
//! fn markers(cx: &mut TestAppContext) {
//!     let mut ui = Ui::open(cx, project, assets);
//!     ui.dispatch(actions::AddMarker);
//!     assert_eq!(ui.read(|e| e.project.markers().len()), 1);
//! }
//! ```
//!
//! Plugin-backed scenarios call [`require_plugins`] and skip when `scripts/build-plugins.sh`
//! has not produced `target/plugins` (CI always builds it first).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::OnceLock;

use gpui::{Action, TestAppContext, VisualTestContext};

use super::*;
use crate::editor_env::{PluginSource, built_plugins_dir};

mod fixtures;
mod scenarios;

/// A private config directory (settings, recent files, plugin cache), wiped once per process, so
/// tests never read or write the user's real settings.
fn config_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/ui-test-config")
            .join(std::process::id().to_string());
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create ui-test config dir");
        dir
    })
    .clone()
}

/// The headless environment: the built first-party plugins when present, else no plugins.
pub(super) fn test_env() -> EditorEnv {
    let plugins = built_plugins_dir().map_or(PluginSource::None, PluginSource::Dir);
    EditorEnv::headless(plugins, config_dir())
}

/// Whether plugin-backed scenarios can run; logs why not.
pub(super) fn require_plugins(test: &str) -> bool {
    let ok = built_plugins_dir().is_some();
    if !ok {
        eprintln!("skipping {test}: run scripts/build-plugins.sh to build target/plugins");
    }
    ok
}

/// A running editor window plus the context that drives it.
pub(super) struct Ui {
    pub(super) editor: Entity<Editor>,
    pub(super) cx: &'static mut VisualTestContext,
}

impl Ui {
    /// Open the editor on `project` in a fresh headless window.
    pub(super) fn open(
        cx: &mut TestAppContext,
        project: Project,
        assets: BTreeMap<AssetId, Vec<u8>>,
    ) -> Self {
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::style::install_theme(cx, true, true);
            actions::init(cx);
        });
        let slot: Rc<RefCell<Option<Entity<Editor>>>> = Rc::default();
        let capture = slot.clone();
        let window = cx.add_window(move |window, cx| {
            let editor = cx.new(|cx| {
                Editor::with_env(
                    cx,
                    window,
                    project,
                    assets,
                    None,
                    false,
                    None,
                    false,
                    test_env(),
                )
            });
            *capture.borrow_mut() = Some(editor.clone());
            gpui_component::Root::new(editor, window, cx)
        });
        let cx = VisualTestContext::from_window(*window, cx).into_mut();
        cx.run_until_parked();
        let editor = slot.borrow_mut().take().expect("editor was built");
        Self { editor, cx }
    }

    /// Read editor state.
    pub(super) fn read<R>(&self, f: impl FnOnce(&Editor) -> R) -> R {
        self.editor.read_with(self.cx, |editor, _| f(editor))
    }

    /// Call an editor method (with window and context), then let pending work and redraws run.
    pub(super) fn update<R>(
        &mut self,
        f: impl FnOnce(&mut Editor, &mut Window, &mut Context<Editor>) -> R,
    ) -> R {
        let result = self.editor.update_in(self.cx, f);
        self.cx.run_until_parked();
        result
    }

    /// Dispatch an action to the focused element, like a menu item or shortcut would.
    pub(super) fn dispatch(&mut self, action: impl Action) {
        self.cx.dispatch_action(action);
        self.cx.run_until_parked();
    }

    /// Press keys through the real keymap (`"m"`, `"secondary-z"`).
    pub(super) fn press(&mut self, keys: &str) {
        self.cx.simulate_keystrokes(keys);
        self.cx.run_until_parked();
    }

    /// Let background work (analysis, audio preparation) and redraws finish.
    pub(super) fn settle(&mut self) {
        self.cx.run_until_parked();
    }
}
