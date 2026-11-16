//! Community plugins: the trust model, the per-user plugin folder and install/uninstall.
//!
//! Community plugins are third-party `.amplugin` bundles kept in the per-user folder
//! (`<data dir>/animathio/plugins`, or `ANIMATHIO_USER_PLUGIN_DIR`). They are sandboxed like
//! every plugin, but AniMathIO does not audit them, so nothing in that folder runs until the
//! user opts in twice:
//!
//! 1. the global **community plugins** switch (default off; while off the folder is not even
//!    read), and
//! 2. the plugin's own enabled flag (an installed plugin starts disabled).
//!
//! Both live in one small JSON file (`community-plugins.json` in the config directory, or
//! `ANIMATHIO_PLUGIN_SETTINGS`) that the editor, the CLI and the control handler all read
//! through [`CommunityStore`].
//!
//! Installing verifies a SHA-256 *before* anything is written, validates the package with the
//! normal package reader, manifest validation and ABI check, refuses reserved
//! `org.animathio.*` ids and writes atomically into the folder as `<id>.amplugin`.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use animathio_plugin_api::{PluginPackage, package::MAX_WASM_BYTES};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::HostError;

/// Overrides the settings file location (tests, portable installs).
pub const SETTINGS_ENV: &str = "ANIMATHIO_PLUGIN_SETTINGS";
/// Overrides the per-user plugin folder (tests, portable installs).
pub const USER_DIR_ENV: &str = "ANIMATHIO_USER_PLUGIN_DIR";
/// Settings file name inside the config directory.
pub const SETTINGS_FILE: &str = "community-plugins.json";
/// Largest package accepted from a file or URL (the wasm limit plus archive overhead).
pub const MAX_PACKAGE_BYTES: u64 = MAX_WASM_BYTES + 1024 * 1024;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_REDIRECTS: u32 = 5;

/// The persisted trust decisions.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CommunitySettings {
    /// The global switch. Off (the default): the user plugin folder is not loaded at all.
    pub enabled: bool,
    /// Ids of installed community plugins the user enabled.
    pub plugins: BTreeSet<String>,
}

impl CommunitySettings {
    /// Whether the plugin `id` may be loaded: the switch is on and the plugin is enabled.
    #[must_use]
    pub fn allows(&self, id: &str) -> bool {
        self.enabled && self.plugins.contains(id)
    }
}

/// Errors from the community plugin store.
#[derive(Debug, Error)]
pub enum CommunityError {
    /// Reading or writing the plugin folder or settings failed.
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    /// The settings file could not be written.
    #[error("settings error: {0}")]
    Settings(String),
    /// The configuration or data directory could not be determined.
    #[error("no per-user directory is available on this system")]
    NoDirectory,
    /// The package did not match the expected SHA-256.
    #[error("sha-256 mismatch: expected {expected}, got {actual}")]
    HashMismatch {
        /// The hash the caller required.
        expected: String,
        /// The hash of the bytes actually received.
        actual: String,
    },
    /// The expected hash is not 64 hexadecimal characters.
    #[error("sha-256 must be 64 hexadecimal characters")]
    BadHash,
    /// A download requires an expected SHA-256.
    #[error("downloading a plugin requires an expected sha-256")]
    HashRequired,
    /// Only `https://` downloads are allowed.
    #[error("only https:// URLs are accepted")]
    NotHttps,
    /// The package or download exceeds the size cap.
    #[error("plugin package is larger than {MAX_PACKAGE_BYTES} bytes")]
    TooLarge,
    /// The download failed.
    #[error("download failed: {0}")]
    Download(String),
    /// The bytes are not a valid plugin package (zip, manifest or ABI).
    #[error("invalid plugin package: {0}")]
    Package(#[from] animathio_plugin_api::PackageError),
    /// The package claims a first-party `org.animathio.*` id.
    #[error("plugin id {0} is reserved for first-party plugins")]
    Reserved(String),
    /// The id is not usable as a community plugin id.
    #[error("invalid plugin id: {0}")]
    InvalidId(String),
    /// A plugin with this id is already installed (use force to replace it).
    #[error("plugin {id} {version} is already installed (use --force to replace it)")]
    AlreadyInstalled {
        /// The colliding id.
        id: String,
        /// The installed version.
        version: String,
    },
    /// No installed community plugin has this id.
    #[error("no community plugin with id {0} is installed")]
    NotInstalled(String),
}

/// One installed community plugin, read from the manifest of its package.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InstalledPlugin {
    /// Plugin id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Plugin version.
    pub version: String,
    /// Description from the manifest.
    pub description: String,
    /// Minimum ABI version.
    pub api_version: String,
    /// Plugin kinds (`clip-source`, `effect`, ...).
    pub kinds: Vec<String>,
    /// Capabilities the manifest requests.
    pub capabilities: Vec<String>,
    /// The package file.
    pub path: PathBuf,
    /// Whether the user enabled this plugin (it still loads only while the global switch is on).
    pub enabled: bool,
}

/// What an install did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallOutcome {
    /// The installed plugin.
    pub id: String,
    /// Its display name.
    pub name: String,
    /// Its version.
    pub version: String,
    /// The file written.
    pub path: PathBuf,
    /// The version it replaced when it was an update of an installed plugin.
    pub replaced: Option<String>,
}

/// The per-user plugin folder and trust settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommunityStore {
    dir: PathBuf,
    settings_path: PathBuf,
}

/// The per-user plugin folder: `ANIMATHIO_USER_PLUGIN_DIR`, else `<data dir>/animathio/plugins`.
#[must_use]
pub fn user_plugin_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(USER_DIR_ENV).filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    directories::BaseDirs::new().map(|base| base.data_dir().join("animathio/plugins"))
}

/// The settings file: `ANIMATHIO_PLUGIN_SETTINGS`, else `config_dir/community-plugins.json`,
/// else the platform config directory of the editor.
#[must_use]
pub fn settings_path(config_dir: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(SETTINGS_ENV).filter(|path| !path.is_empty()) {
        return Some(PathBuf::from(path));
    }
    match config_dir {
        Some(dir) => Some(dir.join(SETTINGS_FILE)),
        None => directories::ProjectDirs::from("org", "AniMathIO", "AniMathIO")
            .map(|dirs| dirs.config_dir().join(SETTINGS_FILE)),
    }
}

impl CommunityStore {
    /// A store over explicit locations.
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>, settings_path: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            settings_path: settings_path.into(),
        }
    }

    /// The store at the default per-user locations. `config_dir` is the editor's configuration
    /// directory when it overrides the platform default.
    pub fn at_default_location(config_dir: Option<&Path>) -> Result<Self, CommunityError> {
        let dir = user_plugin_dir().ok_or(CommunityError::NoDirectory)?;
        let settings = settings_path(config_dir).ok_or(CommunityError::NoDirectory)?;
        Ok(Self::new(dir, settings))
    }

    /// The plugin folder.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The settings file.
    #[must_use]
    pub fn settings_path(&self) -> &Path {
        &self.settings_path
    }

    /// The saved trust settings. A missing or unreadable file means the safe default: off.
    #[must_use]
    pub fn settings(&self) -> CommunitySettings {
        fs::read(&self.settings_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Persist `settings` atomically.
    pub fn save_settings(&self, settings: &CommunitySettings) -> Result<(), CommunityError> {
        let parent = self
            .settings_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let bytes = serde_json::to_vec_pretty(settings)
            .map_err(|error| CommunityError::Settings(error.to_string()))?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.write_all(&bytes)?;
        file.persist(&self.settings_path)
            .map_err(|error| CommunityError::Io(error.error))?;
        Ok(())
    }

    /// Turn the global community plugins switch on or off.
    pub fn set_enabled(&self, enabled: bool) -> Result<(), CommunityError> {
        let mut settings = self.settings();
        settings.enabled = enabled;
        self.save_settings(&settings)
    }

    /// Enable or disable one installed plugin.
    pub fn set_plugin_enabled(&self, id: &str, enabled: bool) -> Result<(), CommunityError> {
        if enabled && !self.list().0.iter().any(|plugin| plugin.id == id) {
            return Err(CommunityError::NotInstalled(id.to_owned()));
        }
        let mut settings = self.settings();
        if enabled {
            settings.plugins.insert(id.to_owned());
        } else {
            settings.plugins.remove(id);
        }
        self.save_settings(&settings)
    }

    /// The installed community plugins (sorted by id) and one message per file that could not
    /// be read as a plugin package.
    #[must_use]
    pub fn list(&self) -> (Vec<InstalledPlugin>, Vec<String>) {
        let settings = self.settings();
        let mut plugins = Vec::new();
        let mut problems = Vec::new();
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return (plugins, problems);
        };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "amplugin"))
            .collect();
        paths.sort();
        for path in paths {
            match read_package_file(&path) {
                Ok(package) => {
                    let manifest = package.manifest;
                    plugins.push(InstalledPlugin {
                        enabled: settings.plugins.contains(&manifest.id),
                        kinds: manifest
                            .kinds
                            .iter()
                            .map(|kind| kind.as_str().to_owned())
                            .collect(),
                        id: manifest.id,
                        name: manifest.name,
                        version: manifest.version,
                        description: manifest.description,
                        api_version: manifest.api_version,
                        capabilities: manifest.capabilities,
                        path,
                    });
                }
                Err(error) => problems.push(format!("{}: {error}", path.display())),
            }
        }
        plugins.sort_by(|a, b| a.id.cmp(&b.id));
        (plugins, problems)
    }

    /// Install a package from a local file. `sha256`, when given, must match.
    pub fn install_file(
        &self,
        path: &Path,
        sha256: Option<&str>,
        force: bool,
    ) -> Result<InstallOutcome, CommunityError> {
        let mut bytes = Vec::new();
        fs::File::open(path)?
            .take(MAX_PACKAGE_BYTES + 1)
            .read_to_end(&mut bytes)?;
        self.install_bytes(&bytes, sha256, force)
    }

    /// Download a package over https and install it. The expected SHA-256 is required and is
    /// checked before anything is written.
    pub fn install_url(
        &self,
        url: &str,
        sha256: &str,
        force: bool,
    ) -> Result<InstallOutcome, CommunityError> {
        self.install_url_with(url, Some(sha256), force, download)
    }

    /// [`Self::install_url`] with the transport injected (tests never touch the network).
    pub fn install_url_with(
        &self,
        url: &str,
        sha256: Option<&str>,
        force: bool,
        fetch: impl FnOnce(&str) -> Result<Vec<u8>, CommunityError>,
    ) -> Result<InstallOutcome, CommunityError> {
        if !is_https(url) {
            return Err(CommunityError::NotHttps);
        }
        let sha256 = sha256.ok_or(CommunityError::HashRequired)?;
        // Reject a malformed hash before spending bandwidth.
        parse_sha256(sha256)?;
        let bytes = fetch(url)?;
        self.install_bytes(&bytes, Some(sha256), force)
    }

    /// Verify, validate and write a package. Nothing is written unless every check passes.
    pub fn install_bytes(
        &self,
        bytes: &[u8],
        sha256: Option<&str>,
        force: bool,
    ) -> Result<InstallOutcome, CommunityError> {
        if bytes.len() as u64 > MAX_PACKAGE_BYTES {
            return Err(CommunityError::TooLarge);
        }
        if let Some(expected) = sha256 {
            let expected = parse_sha256(expected)?;
            let actual: [u8; 32] = Sha256::digest(bytes).into();
            if actual != expected {
                return Err(CommunityError::HashMismatch {
                    expected: hex(&expected),
                    actual: hex(&actual),
                });
            }
        }
        let package = PluginPackage::from_bytes(bytes)?;
        let manifest = &package.manifest;
        crate::check_plugin_id(&manifest.id, false).map_err(|error| match error {
            HostError::ReservedId(id) => CommunityError::Reserved(id),
            other => CommunityError::InvalidId(other.to_string()),
        })?;
        if manifest.id.starts_with('.') || is_windows_device_name(&manifest.id) {
            // The id becomes a file name; keep it valid on every platform.
            return Err(CommunityError::InvalidId(manifest.id.clone()));
        }

        fs::create_dir_all(&self.dir)?;
        let existing: Vec<InstalledPlugin> = self
            .list()
            .0
            .into_iter()
            .filter(|plugin| plugin.id == manifest.id)
            .collect();
        if let Some(old) = existing.first()
            && !force
        {
            return Err(CommunityError::AlreadyInstalled {
                id: old.id.clone(),
                version: old.version.clone(),
            });
        }

        let target = self.dir.join(format!("{}.amplugin", manifest.id));
        let mut file = tempfile::NamedTempFile::new_in(&self.dir)?;
        file.write_all(bytes)?;
        file.persist(&target)
            .map_err(|error| CommunityError::Io(error.error))?;
        // A hand-dropped copy under another name would load as a duplicate.
        for old in &existing {
            if old.path != target {
                let _ = fs::remove_file(&old.path);
            }
        }
        if existing.is_empty() {
            // A fresh install starts disabled, whatever an earlier install left behind.
            let mut settings = self.settings();
            if settings.plugins.remove(&manifest.id) {
                self.save_settings(&settings)?;
            }
        }
        Ok(InstallOutcome {
            id: manifest.id.clone(),
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            path: target,
            replaced: existing.first().map(|old| old.version.clone()),
        })
    }

    /// Remove an installed plugin and its enabled flag.
    pub fn uninstall(&self, id: &str) -> Result<(), CommunityError> {
        let (installed, _) = self.list();
        let matches: Vec<&InstalledPlugin> = installed.iter().filter(|p| p.id == id).collect();
        if matches.is_empty() {
            return Err(CommunityError::NotInstalled(id.to_owned()));
        }
        for plugin in matches {
            fs::remove_file(&plugin.path)?;
        }
        let mut settings = self.settings();
        if settings.plugins.remove(id) {
            self.save_settings(&settings)?;
        }
        Ok(())
    }
}

pub(crate) fn read_package_file(path: &Path) -> Result<PluginPackage, CommunityError> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_PACKAGE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_PACKAGE_BYTES {
        return Err(CommunityError::TooLarge);
    }
    Ok(PluginPackage::from_bytes(&bytes)?)
}

/// Whether the file name `<id>.amplugin` would name a Windows device (`CON`, `NUL`, `COM1`, ...),
/// which Windows resolves by the part before the first dot.
fn is_windows_device_name(id: &str) -> bool {
    let stem = id.split('.').next().unwrap_or(id).to_ascii_lowercase();
    matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
        || ((stem.starts_with("com") || stem.starts_with("lpt"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0')
}

fn is_https(url: &str) -> bool {
    url.get(..8)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
}

fn parse_sha256(text: &str) -> Result<[u8; 32], CommunityError> {
    let text = text.trim();
    if text.len() != 64 || !text.is_ascii() {
        return Err(CommunityError::BadHash);
    }
    let mut out = [0u8; 32];
    for (byte, pair) in out.iter_mut().zip(text.as_bytes().chunks(2)) {
        let pair = std::str::from_utf8(pair).map_err(|_| CommunityError::BadHash)?;
        *byte = u8::from_str_radix(pair, 16).map_err(|_| CommunityError::BadHash)?;
    }
    Ok(out)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

/// Fetch `url` over https with timeouts and the package size cap. The request carries a plain
/// `AniMathIO/<version>` user agent and nothing else about the user.
fn download(url: &str) -> Result<Vec<u8>, CommunityError> {
    let net = |error: &dyn std::fmt::Display| CommunityError::Download(error.to_string());
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .user_agent(format!("AniMathIO/{}", env!("CARGO_PKG_VERSION")))
        .https_only(true)
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_global(Some(REQUEST_TIMEOUT))
        .max_redirects(MAX_REDIRECTS)
        .build()
        .into();
    let response = agent.get(url).call().map_err(|e| net(&e))?;
    if response.status().as_u16() != 200 {
        return Err(CommunityError::Download(format!(
            "server answered {}",
            response.status()
        )));
    }
    let mut bytes = Vec::new();
    response
        .into_body()
        .into_reader()
        .take(MAX_PACKAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| net(&e))?;
    if bytes.len() as u64 > MAX_PACKAGE_BYTES {
        return Err(CommunityError::TooLarge);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELLO: &[u8] = include_bytes!("../tests/fixtures/hello-1.0.0.amplugin");

    fn store() -> (tempfile::TempDir, CommunityStore) {
        let root = tempfile::tempdir().unwrap();
        let store = CommunityStore::new(
            root.path().join("plugins"),
            root.path().join("config/community-plugins.json"),
        );
        (root, store)
    }

    fn sha(bytes: &[u8]) -> String {
        hex(&Sha256::digest(bytes))
    }

    fn package(id: &str) -> Vec<u8> {
        let manifest = animathio_plugin_api::PluginManifestToml {
            api_version: "1.0.0".into(),
            id: id.into(),
            name: "Test".into(),
            version: "0.1.0".into(),
            description: String::new(),
            kinds: vec![animathio_plugin_api::PluginKind::ClipSource],
            capabilities: Vec::new(),
        };
        PluginPackage::new(manifest, b"\0asm".to_vec())
            .unwrap()
            .create_bytes()
            .unwrap()
    }

    #[test]
    fn settings_default_to_off_and_round_trip() {
        let (_root, store) = store();
        assert_eq!(store.settings(), CommunitySettings::default());
        assert!(!store.settings().allows("com.example.hello"));
        store.set_enabled(true).unwrap();
        assert!(store.settings().enabled);
        let mut settings = store.settings();
        settings.plugins.insert("com.example.hello".into());
        store.save_settings(&settings).unwrap();
        assert!(store.settings().allows("com.example.hello"));
        std::fs::write(store.settings_path(), b"not json").unwrap();
        assert!(!store.settings().enabled, "corrupt settings are safe: off");
    }

    #[test]
    fn install_file_starts_disabled_and_lists() {
        let (_root, store) = store();
        let outcome = store
            .install_bytes(HELLO, Some(&sha(HELLO)), false)
            .unwrap();
        assert_eq!(outcome.id, "com.example.hello");
        assert!(outcome.path.ends_with("com.example.hello.amplugin"));
        let (plugins, problems) = store.list();
        assert!(problems.is_empty());
        assert_eq!(plugins.len(), 1);
        assert!(!plugins[0].enabled);
        store.set_plugin_enabled("com.example.hello", true).unwrap();
        assert!(store.list().0[0].enabled);
        assert!(matches!(
            store.set_plugin_enabled("com.example.missing", true),
            Err(CommunityError::NotInstalled(_))
        ));
    }

    #[test]
    fn install_rejects_bad_hash_before_writing() {
        let (_root, store) = store();
        let wrong = "0".repeat(64);
        assert!(matches!(
            store.install_bytes(HELLO, Some(&wrong), false),
            Err(CommunityError::HashMismatch { .. })
        ));
        assert!(matches!(
            store.install_bytes(HELLO, Some("abc"), false),
            Err(CommunityError::BadHash)
        ));
        assert!(!store.dir().exists(), "nothing was written");
    }

    #[test]
    fn install_rejects_invalid_packages_and_reserved_ids() {
        let (_root, store) = store();
        let junk = b"not a zip file".to_vec();
        assert!(matches!(
            store.install_bytes(&junk, Some(&sha(&junk)), false),
            Err(CommunityError::Package(_))
        ));
        let reserved = package("org.animathio.text");
        assert!(matches!(
            store.install_bytes(&reserved, Some(&sha(&reserved)), false),
            Err(CommunityError::Reserved(_))
        ));
        for bad in ["..", "nul", "COM1.example", "con.plugin"] {
            let bytes = package(bad);
            assert!(
                matches!(
                    store.install_bytes(&bytes, None, false),
                    Err(CommunityError::InvalidId(_))
                ),
                "{bad}"
            );
        }
        assert!(!is_windows_device_name("com.example.hello"));
        assert!(store.list().0.is_empty());
    }

    #[test]
    fn install_refuses_collisions_unless_forced() {
        let (_root, store) = store();
        store.install_bytes(HELLO, None, false).unwrap();
        store.set_plugin_enabled("com.example.hello", true).unwrap();
        assert!(matches!(
            store.install_bytes(HELLO, None, false),
            Err(CommunityError::AlreadyInstalled { .. })
        ));
        let update = store.install_bytes(HELLO, None, true).unwrap();
        assert_eq!(update.replaced.as_deref(), Some("1.0.0"));
        assert!(
            store.settings().plugins.contains("com.example.hello"),
            "an update keeps its enabled flag"
        );
    }

    #[test]
    fn fresh_install_clears_a_stale_enabled_flag() {
        let (_root, store) = store();
        let mut settings = store.settings();
        settings.plugins.insert("com.example.hello".into());
        store.save_settings(&settings).unwrap();
        store.install_bytes(HELLO, None, false).unwrap();
        assert!(!store.settings().plugins.contains("com.example.hello"));
    }

    #[test]
    fn url_installs_require_https_and_a_hash() {
        let (_root, store) = store();
        let never = |_: &str| -> Result<Vec<u8>, CommunityError> { panic!("fetched") };
        assert!(matches!(
            store.install_url_with(
                "http://example.com/a.amplugin",
                Some(&sha(HELLO)),
                false,
                never
            ),
            Err(CommunityError::NotHttps)
        ));
        assert!(matches!(
            store.install_url_with("https://example.com/a.amplugin", None, false, never),
            Err(CommunityError::HashRequired)
        ));
        assert!(matches!(
            store.install_url_with("https://example.com/a.amplugin", Some("zz"), false, never),
            Err(CommunityError::BadHash)
        ));
        let outcome = store
            .install_url_with(
                "https://example.com/a.amplugin",
                Some(&sha(HELLO)),
                false,
                |url| {
                    assert_eq!(url, "https://example.com/a.amplugin");
                    Ok(HELLO.to_vec())
                },
            )
            .unwrap();
        assert_eq!(outcome.id, "com.example.hello");
        let (_root2, other) = self::store();
        assert!(matches!(
            other.install_url_with(
                "https://example.com/a.amplugin",
                Some(&"1".repeat(64)),
                false,
                |_| Ok(HELLO.to_vec())
            ),
            Err(CommunityError::HashMismatch { .. })
        ));
    }

    #[test]
    fn uninstall_removes_file_and_flag() {
        let (_root, store) = store();
        store.install_bytes(HELLO, None, false).unwrap();
        store.set_plugin_enabled("com.example.hello", true).unwrap();
        store.uninstall("com.example.hello").unwrap();
        assert!(store.list().0.is_empty());
        assert!(store.settings().plugins.is_empty());
        assert!(matches!(
            store.uninstall("com.example.hello"),
            Err(CommunityError::NotInstalled(_))
        ));
    }
}
