//! The devices, plugins and on-disk state an [`Editor`](crate::editor::Editor) depends on.
//!
//! Production runs with [`EditorEnv::production`]: the default audio device, the stored GPU
//! preview preference, plugin discovery and the user's XDG directories. Tests build the same
//! editor with [`EditorEnv::headless`] (see `ui_tests`): a null audio sink, the CPU preview
//! backend, plugins from one directory (or none) and a private config directory, so no test
//! touches a sound card, a GPU or the user's settings.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

#[cfg(test)]
use animathio_assist::{AssistError, CancelToken, Endpoint};
use animathio_assist::{Converter, HttpConverter};
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
    /// Whether this environment may check for a newer release at startup
    /// (`update_check`); the headless test harness never touches the network.
    pub(crate) updates: bool,
    /// Whether a first run in a short window starts with small track rows. Off in the headless
    /// harness so every scenario sees the same default layout.
    pub(crate) adaptive_density: bool,
    /// The AI assist client behind the Math dialog's Convert button. Production uses the real
    /// HTTP client; tests inject a mock, and the headless default never touches the network.
    pub(crate) assist: Arc<dyn Converter>,
}

/// The headless default for [`EditorEnv::assist`]: every conversion fails as offline.
#[cfg(test)]
#[derive(Debug)]
pub(crate) struct OfflineConverter;

#[cfg(test)]
impl Converter for OfflineConverter {
    fn complete(
        &self,
        _endpoint: &Endpoint,
        _prose: &str,
        _cancel: &CancelToken,
    ) -> Result<String, AssistError> {
        Err(AssistError::Network("offline test environment".into()))
    }
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
            updates: true,
            adaptive_density: true,
            assist: Arc::new(HttpConverter::new()),
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
            updates: false,
            adaptive_density: false,
            assist: Arc::new(OfflineConverter),
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

/// The private configuration directory, when there is one: the installed environment's, or, in
/// a test binary that has not installed one yet, a per-process directory under `target/`. Test
/// code can therefore never resolve to the user's real XDG directories (`~/.config/animathio`).
fn private_config_dir() -> Option<PathBuf> {
    match PROCESS.get() {
        Some((_, Some(dir))) => Some(dir.clone()),
        _ => test_fallback_dir(),
    }
}

#[cfg(test)]
#[allow(clippy::unnecessary_wraps)] // mirrors the non-test stub, which has no fallback
fn test_fallback_dir() -> Option<PathBuf> {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    let dir = DIR.get_or_init(|| {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/ui-test-config")
            .join(format!("{}-fallback", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test fallback config dir");
        dir
    });
    Some(dir.clone())
}

#[cfg(not(test))]
fn test_fallback_dir() -> Option<PathBuf> {
    None
}

/// A settings file (`name`) in the config directory.
pub(crate) fn config_file(name: &str) -> Option<PathBuf> {
    match private_config_dir() {
        Some(dir) => Some(dir.join(name)),
        None => directories::ProjectDirs::from("org", "AniMathIO", "AniMathIO")
            .map(|dirs| dirs.config_dir().join(name)),
    }
}

/// The community plugin store (per-user plugin folder and trust settings).
///
/// The settings live in the configuration directory (`community-plugins.json`), shared with
/// `animathio-cli` and the control handler. An environment with a private configuration
/// directory (the headless test harness) also gets a private plugin folder inside it, so tests
/// never read or write the user's real plugins.
pub(crate) fn community_store() -> Option<animathio_plugin_host::CommunityStore> {
    let config = private_config_dir();
    let settings = animathio_plugin_host::settings_path(config.as_deref())?;
    let dir = match &config {
        Some(dir) => dir.join("community-plugins"),
        None => animathio_plugin_host::user_plugin_dir()?,
    };
    Some(animathio_plugin_host::CommunityStore::new(dir, settings))
}

/// Whether the global community plugins switch is on (off when the settings are missing).
pub(crate) fn community_plugins_enabled() -> bool {
    community_store().is_some_and(|store| store.settings().enabled)
}

/// The compiled-plugin cache directory.
pub(crate) fn plugin_cache_dir() -> PathBuf {
    match private_config_dir() {
        Some(dir) => dir.join("plugin-cache"),
        None => directories::BaseDirs::new().map_or_else(
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Whatever state the process is in (no environment installed yet, or a headless one), the
    /// settings, community store and plugin cache stay inside `target/`, never in `~/.config`.
    #[test]
    fn test_paths_never_resolve_to_the_user_directories() {
        let target = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        let target = target.canonicalize().expect("target dir exists");
        let inside = |path: PathBuf| {
            let parent = path.parent().expect("has a parent").to_path_buf();
            std::fs::create_dir_all(&parent).expect("create parent");
            let canonical = parent.canonicalize().expect("canonicalize");
            assert!(
                canonical.starts_with(&target),
                "{} escapes {}",
                path.display(),
                target.display()
            );
        };
        inside(config_file("x").expect("config file"));
        inside(plugin_cache_dir().join("x"));
        let store = community_store().expect("community store");
        inside(store.dir().join("x"));
        inside(store.settings_path().to_path_buf());
    }
}
