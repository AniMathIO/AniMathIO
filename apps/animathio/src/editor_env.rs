//! The devices, plugins and on-disk state an [`Editor`](crate::editor::Editor) depends on.
//!
//! Production runs with [`EditorEnv::production`]: the default audio device, the stored GPU
//! preview preference, plugin discovery and the user's XDG directories. Tests build the same
//! editor with [`EditorEnv::headless`] (see `ui_tests`): a null audio sink, the CPU preview
//! backend, plugins from one directory (or none) and a private config directory, so no test
//! touches a sound card, a GPU or the user's settings.

use std::path::PathBuf;
use std::sync::OnceLock;

use animathio_audio::AudioPool;
use animathio_core::Project;
use animathio_render::BackendKind;

use crate::playback::{Clock, MonotonicClock, make_clock};

/// Where playback audio goes.
// `Null` and `Dir` are constructed by the headless test environment only.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AudioOutput {
    /// The default output device (falls back to a wall clock when none opens).
    Device,
    /// No device: a monotonic clock drives playback and nothing is played.
    Null,
}

/// Where plugin bundles come from.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PluginSource {
    /// `ANIMATHIO_PLUGIN_DIR`, the executable-relative bundles and the user directory.
    Discover,
    /// Every `.amplugin` in one directory (first-party ids allowed).
    Dir(PathBuf),
    /// No plugins: clips of plugin kinds render as placeholders.
    None,
}

/// The injectable environment of one editor process.
#[derive(Clone, Debug)]
pub(crate) struct EditorEnv {
    pub(crate) audio: AudioOutput,
    /// Preview renderer; `None` follows the stored "GPU preview" preference.
    pub(crate) backend: Option<BackendKind>,
    pub(crate) plugins: PluginSource,
    /// Directory for settings, recent files and caches; `None` uses the XDG directories.
    pub(crate) config_dir: Option<PathBuf>,
}

/// Process-wide part of the environment. Plugin loading and settings files are reached from
/// worker threads and free functions, so the first installed value wins for the process.
static PROCESS: OnceLock<(PluginSource, Option<PathBuf>)> = OnceLock::new();

impl EditorEnv {
    /// The real devices, plugin discovery and user directories.
    pub(crate) fn production() -> Self {
        Self {
            audio: AudioOutput::Device,
            backend: None,
            plugins: PluginSource::Discover,
            config_dir: None,
        }
    }

    /// A test environment: null audio, CPU preview, the given plugin source and a private
    /// config directory.
    #[cfg(test)]
    pub(crate) fn headless(plugins: PluginSource, config_dir: PathBuf) -> Self {
        Self {
            audio: AudioOutput::Null,
            backend: Some(BackendKind::Cpu),
            plugins,
            config_dir: Some(config_dir),
        }
    }

    /// Publish the plugin source and config directory for code that has no editor handle.
    /// Idempotent: the first call in a process decides.
    pub(crate) fn install(&self) {
        let _ = PROCESS.set((self.plugins.clone(), self.config_dir.clone()));
    }

    /// A fresh playback clock on this environment's audio output.
    pub(crate) fn make_clock(&self, project: &Project, pool: &AudioPool) -> Box<dyn Clock> {
        match self.audio {
            AudioOutput::Device => make_clock(project, pool),
            AudioOutput::Null => Box::new(MonotonicClock::new()),
        }
    }
}

/// The installed plugin source ([`PluginSource::Discover`] until an editor installs one).
pub(crate) fn plugin_source() -> PluginSource {
    PROCESS
        .get()
        .map_or(PluginSource::Discover, |(plugins, _)| plugins.clone())
}

/// A settings file (`name`) in the config directory.
pub(crate) fn config_file(name: &str) -> Option<PathBuf> {
    match PROCESS.get() {
        Some((_, Some(dir))) => Some(dir.join(name)),
        _ => directories::ProjectDirs::from("org", "AniMathIO", "AniMathIO")
            .map(|dirs| dirs.config_dir().join(name)),
    }
}

/// The compiled-plugin cache directory.
pub(crate) fn plugin_cache_dir() -> PathBuf {
    match PROCESS.get() {
        Some((_, Some(dir))) => dir.join("plugin-cache"),
        _ => directories::BaseDirs::new().map_or_else(
            || std::env::temp_dir().join("animathio-plugin-cache"),
            |base| base.cache_dir().join("animathio").join("plugin-cache"),
        ),
    }
}

/// The built plugin bundles (`target/plugins`, from `scripts/build-plugins.sh`) when present and
/// non-empty; tests use them so plugin-backed panels run against the real first-party plugins.
#[cfg(test)]
pub(crate) fn built_plugins_dir() -> Option<PathBuf> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/plugins");
    let has_bundle = std::fs::read_dir(&dir).ok()?.flatten().any(|entry| {
        entry
            .path()
            .extension()
            .is_some_and(|ext| ext == "amplugin")
    });
    has_bundle.then_some(dir)
}
