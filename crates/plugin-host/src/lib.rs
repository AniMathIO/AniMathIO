//! WebAssembly Component Model plugin host runtime and registry for AniMathIO 2.0.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use animathio_core::{ClipKind, Scene as CoreScene, VectorScene};
use animathio_plugin_api::{InspectorSchema, PluginKind, PluginManifestToml, PluginPackage};
use animathio_typeset::{MathMode, TypesetEngine, TypesetResult, TypesetVerb};
use lru::LruCache;
use thiserror::Error;
use wasmtime::component::{Component, HasSelf, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

wasmtime::component::bindgen!({
    path: "../plugin-api/wit",
    world: "source-plugin",
});

/// Bindings for the pixel-effect world (`effect-plugin`). Shared interfaces reuse the
/// `source-plugin` world's generated types and host imports, so one linker serves both worlds.
mod effect_world {
    wasmtime::component::bindgen!({
        path: "../plugin-api/wit",
        world: "effect-plugin",
        with: {
            "animathio:plugin/types": crate::animathio::plugin::types,
            "animathio:plugin/host": crate::animathio::plugin::host,
        },
    });
}

/// Bindings for the audio-effect world (`audio-effect-plugin`); see [`effect_world`].
mod audio_world {
    wasmtime::component::bindgen!({
        path: "../plugin-api/wit",
        world: "audio-effect-plugin",
        with: {
            "animathio:plugin/types": crate::animathio::plugin::types,
            "animathio:plugin/host": crate::animathio::plugin::host,
        },
    });
}

mod audio;
mod compat;
mod discovery;
mod effect_params;
mod effects;
pub use audio::AudioEffectHandle;
pub use discovery::{PluginDirectory, plugin_search_dirs};
pub use effect_params::{mentions_effect_params, validate_effect_command};
pub use effects::{EffectDescriptor, EffectHandle, EffectParam};

use animathio::plugin::host::Host;
use animathio::plugin::types as wit_types;

/// Errors encountered in the plugin host runtime.
#[derive(Debug, Error)]
pub enum HostError {
    /// Wasmtime runtime or engine error.
    #[error("wasmtime error: {0}")]
    Wasmtime(#[from] wasmtime::Error),
    /// IO error reading files or disk cache.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// Error in plugin package packaging or extraction.
    #[error("package error: {0}")]
    Package(#[from] animathio_plugin_api::PackageError),
    /// Plugin compilation failure.
    #[error("compilation error: {0}")]
    CompilationFailed(String),
    /// Plugin not registered in the host.
    #[error("plugin not found: {0}")]
    NotFound(String),
    /// Plugin execution trapped or failed.
    #[error("plugin execution trapped: {0}")]
    Trapped(String),
    /// A plugin with this id is already registered.
    #[error("plugin id already registered: {0}")]
    DuplicateId(String),
    /// A user-installed package claimed a reserved first-party id.
    #[error("reserved plugin id is not allowed in the user directory: {0}")]
    ReservedId(String),
    /// The plugin produced a scene that failed validation.
    #[error("plugin produced an invalid scene: {0}")]
    InvalidOutput(String),
}

/// Maximum characters accepted by the host `shape_text` import.
const MAX_SHAPE_TEXT_CHARS: usize = 10_000;
/// Maximum total path verbs returned by the host `shape_text` import.
const MAX_SHAPE_TEXT_VERBS: usize = 200_000;
/// Maximum UTF-8 bytes accepted by the host `svg_to_paths` import.
const MAX_SVG_INPUT_BYTES: usize = 1024 * 1024;
/// Maximum total path verbs returned by the host `svg_to_paths` import.
const MAX_SVG_VERBS: usize = 100_000;
/// Maximum total path verbs returned by the host `typeset_math` import.
const MAX_TYPESET_VERBS: usize = 200_000;
/// Maximum UTF-8 bytes per `log` message.
const MAX_LOG_MESSAGE_BYTES: usize = 4 * 1024;
/// Maximum retained log messages per store.
const MAX_LOG_ENTRIES: usize = 1_000;
/// How often the epoch ticker advances the engine epoch.
const EPOCH_TICK_INTERVAL: Duration = Duration::from_millis(10);

/// Resource limits applied to sandboxed plugin execution.
#[derive(Clone, Copy, Debug)]
pub struct SandboxLimits {
    /// Memory growth limit in bytes (default: 64 MB).
    pub max_memory_bytes: usize,
    /// Maximum table elements (default: 50,000).
    pub max_table_elements: usize,
    /// Fuel units allocated per execution call (default: 50,000,000).
    pub fuel_per_call: u64,
    /// Epoch deadline ticks (default: 100). At a 10 ms tick interval this is a
    /// ~1 s backstop; fuel metering catches most runaway plugins much sooner.
    /// A low deadline races the epoch ticker and can spuriously trap fast
    /// calls such as `describe` during plugin loading.
    pub epoch_deadline_ticks: u64,
    /// Largest frame, in pixels, a pixel effect may process (default: 16 Mpx, above
    /// 4K UHD). Larger frames are rejected before any plugin code runs.
    pub max_effect_pixels: usize,
    /// Ceiling on the memory of one pixel-effect call (default: 1 GiB). The limit
    /// actually applied is 64 MiB plus 48 bytes per frame pixel, capped here, so small
    /// frames get a small sandbox.
    pub effect_memory_bytes: usize,
    /// Extra fuel per frame pixel for pixel effects, added to `fuel_per_call`
    /// (default: 4,096 — roughly 10x the cost of the heaviest first-party filter).
    pub effect_fuel_per_pixel: u64,
    /// Epoch deadline ticks for pixel effects (default: 1,000, a ~10 s backstop).
    pub effect_epoch_deadline_ticks: u64,
    /// Most frames one audio-effect call may process (default: 65,536, about 1.4 s at
    /// 48 kHz). Larger blocks are rejected before any plugin code runs.
    pub max_audio_block_frames: usize,
    /// Largest opaque state blob an audio effect may carry between blocks (default: 64 KiB).
    pub max_audio_state_bytes: usize,
    /// Ceiling on the memory of one audio-effect call (default: 256 MiB). The limit actually
    /// applied is 16 MiB plus 64 bytes per interleaved sample, capped here.
    pub audio_memory_bytes: usize,
    /// Extra fuel per interleaved sample for audio effects, added to `fuel_per_call`
    /// (default: 4,096, about 10x the cost of the heaviest first-party filter).
    pub audio_fuel_per_sample: u64,
    /// Epoch deadline ticks for audio effects (default: 200, a ~2 s backstop; a block is
    /// worth well under 100 ms of real-time audio, so a stalled call is far over budget).
    pub audio_epoch_deadline_ticks: u64,
}

impl Default for SandboxLimits {
    fn default() -> Self {
        Self {
            max_memory_bytes: 64 * 1024 * 1024,
            max_table_elements: 50_000,
            fuel_per_call: 50_000_000,
            epoch_deadline_ticks: 100,
            max_effect_pixels: 16 * 1024 * 1024,
            effect_memory_bytes: 1024 * 1024 * 1024,
            effect_fuel_per_pixel: 4096,
            effect_epoch_deadline_ticks: 1000,
            max_audio_block_frames: 65_536,
            max_audio_state_bytes: 64 * 1024,
            audio_memory_bytes: 256 * 1024 * 1024,
            audio_fuel_per_sample: 4096,
            audio_epoch_deadline_ticks: 200,
        }
    }
}

/// Capabilities granted by the host to a plugin. Default is none.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CapabilityGrants {
    /// Allowed filesystem directories (read-only or read-write).
    pub allowed_directories: Vec<PathBuf>,
    /// Whether network socket access is granted. Default is false.
    pub allow_network: bool,
    /// Environment variables passed to WASI context.
    pub environment_variables: Vec<(String, String)>,
}

/// Build the [`StoreLimits`] applied to every plugin store. A single memory of at most
/// `max_memory_bytes` makes the per-memory limit effectively the aggregate plugin allocation.
/// Instances, memories and tables are capped far below wasmtime's defaults; wasmtime 50's
/// component runtime needs at least three core instances per component, hence the eight here.
fn sandbox_store_limits(limits: &SandboxLimits) -> StoreLimits {
    StoreLimitsBuilder::new()
        .memory_size(limits.max_memory_bytes)
        .table_elements(limits.max_table_elements)
        .instances(8)
        .memories(1)
        .tables(2)
        .trap_on_grow_failure(true)
        .build()
}

/// Background thread that advances the engine epoch on a fixed interval. Without this the
/// `epoch_deadline_ticks` configured on each store would never fire while a plugin runs.
struct EpochTicker {
    shutdown: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl EpochTicker {
    fn start(engine: Engine, interval: Duration) -> Self {
        let shutdown = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&shutdown);
        let handle = std::thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                std::thread::sleep(interval);
                engine.increment_epoch();
            }
        });
        Self {
            shutdown,
            handle: Some(handle),
        }
    }
}

impl Drop for EpochTicker {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Internal host state for WASI and host imports.
pub struct HostState {
    table: ResourceTable,
    wasi: WasiCtx,
    font_data: Vec<u8>,
    typeset: Arc<TypesetEngine>,
    limiter: StoreLimits,
    logs: Vec<String>,
    policy: SandboxLimits,
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl animathio::plugin::types::Host for HostState {}

impl HostState {
    fn legacy_shape_text(&mut self, text: String, font_size: f32) -> Vec<Vec<wit_types::PathVerb>> {
        let char_count = text.chars().count();
        if char_count > MAX_SHAPE_TEXT_CHARS {
            log::warn!(
                "shape_text: rejecting {char_count} characters (max {MAX_SHAPE_TEXT_CHARS})"
            );
            return Vec::new();
        }
        shape_text_skrifa(&self.font_data, &text, font_size, MAX_SHAPE_TEXT_VERBS)
    }

    fn legacy_svg_to_paths(&mut self, svg_xml: String) -> Vec<Vec<wit_types::PathVerb>> {
        if svg_xml.len() > MAX_SVG_INPUT_BYTES {
            log::warn!(
                "svg_to_paths: rejecting {} input bytes (max {MAX_SVG_INPUT_BYTES})",
                svg_xml.len()
            );
            return Vec::new();
        }
        parse_svg_to_paths(&svg_xml, MAX_SVG_VERBS)
    }

    fn legacy_typeset_math(
        &mut self,
        source: String,
        mode: wit_types::MathMode,
        font_size: f32,
    ) -> wit_types::TypesetResult {
        let mode = match mode {
            wit_types::MathMode::Latex => MathMode::Latex,
            wit_types::MathMode::Typst => MathMode::Typst,
        };
        match self.typeset.typeset(&source, mode, font_size) {
            Ok(result) => typeset_to_wit(&result, MAX_TYPESET_VERBS),
            Err(err) => {
                log::warn!("typeset_math: {err}");
                wit_types::TypesetResult {
                    paths: Vec::new(),
                    bbox: wit_types::Bbox {
                        min_x: 0.0,
                        min_y: 0.0,
                        max_x: 0.0,
                        max_y: 0.0,
                    },
                }
            }
        }
    }

    fn legacy_log(&mut self, message: String) {
        let message = truncate_utf8(&message, MAX_LOG_MESSAGE_BYTES);
        log::info!("[plugin log] {}", message);
        if self.logs.len() < MAX_LOG_ENTRIES {
            self.logs.push(message);
        }
    }
}

/// Loaded precompiled plugin entry.
pub struct LoadedPlugin {
    /// Package manifest.
    pub manifest: PluginManifestToml,
    /// Component compiled for wasmtime.
    pub component: Component,
    /// BLAKE3 content hash of the wasm binary.
    pub wasm_hash: [u8; 32],
    /// UI Inspector schema derived from WIT manifest.
    pub inspector_schema: InspectorSchema,
    /// Full WIT manifest returned by describe().
    pub wit_manifest: wit_types::Manifest,
}

/// Disk cache for compiled WebAssembly components.
///
/// A compiled component contains native machine code, so an entry is only
/// deserialized after its BLAKE3 keyed MAC is verified against a per-install
/// 32-byte secret stored in the cache directory. On Unix the cache directory
/// itself must be a private (`0700`, current-uid, non-symlink) directory; if
/// it is not, the disk cache is disabled and components are compiled fresh on
/// every load.
pub struct ComponentDiskCache {
    cache_dir: PathBuf,
    /// `None` disables the disk cache (unsafe directory or unavailable key).
    secret: Option<[u8; 32]>,
}

impl ComponentDiskCache {
    /// Initialize or create a cache directory, loading or generating the MAC key.
    ///
    /// If the cache directory cannot be made private (Unix) or the key cannot
    /// be loaded or created, the cache is disabled and every lookup recompiles.
    pub fn new(cache_dir: impl Into<PathBuf>) -> Self {
        let dir = cache_dir.into();
        if let Err(error) = prepare_cache_dir(&dir) {
            log::warn!("disabling plugin disk cache {}: {error}", dir.display());
            return Self {
                cache_dir: dir,
                secret: None,
            };
        }
        match load_or_create_secret(&dir) {
            Some(secret) => Self {
                cache_dir: dir,
                secret: Some(secret),
            },
            None => {
                log::warn!(
                    "disabling plugin disk cache {}: could not load or create MAC key",
                    dir.display()
                );
                Self {
                    cache_dir: dir,
                    secret: None,
                }
            }
        }
    }

    /// Whether the disk cache is usable.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.secret.is_some()
    }

    /// Compute disk cache key from wasm hash and wasmtime engine version.
    #[must_use]
    pub fn cache_key(wasm_bytes: &[u8]) -> String {
        let mut hasher = blake3::Hasher::new();
        hasher.update(wasm_bytes);
        hasher.update(b"wasmtime-50.0.1");
        hasher.finalize().to_hex().to_string()
    }

    /// Get precompiled component from disk cache, or compile and write to disk cache.
    pub fn get_or_compile(
        &self,
        engine: &Engine,
        wasm_bytes: &[u8],
    ) -> Result<Component, HostError> {
        let key = Self::cache_key(wasm_bytes);
        let cache_file = self.cache_dir.join(format!("{key}.bin"));

        if let Some(secret) = &self.secret
            && let Some(bytes) = read_verified_entry(secret, &cache_file)
        {
            // SAFETY: `bytes` is a serialized component whose 32-byte keyed
            // BLAKE3 MAC was verified in constant time against our per-install
            // secret. On Unix the cache directory was also verified to be
            // private, so an attacker cannot have forged the entry.
            if let Ok(component) = unsafe { Component::deserialize(engine, &bytes) } {
                return Ok(component);
            }
            let _ = fs::remove_file(&cache_file);
        }

        let component = Component::from_binary(engine, wasm_bytes)
            .map_err(|e| HostError::CompilationFailed(e.to_string()))?;

        if let Some(secret) = &self.secret
            && let Ok(serialized) = component.serialize()
            && let Err(error) =
                write_verified_entry(&self.cache_dir, &cache_file, secret, &serialized)
        {
            log::warn!(
                "failed to write plugin cache entry {}: {error}",
                cache_file.display()
            );
        }

        Ok(component)
    }
}

#[cfg(unix)]
fn current_uid() -> u32 {
    rustix::process::getuid().as_raw()
}

#[cfg(unix)]
fn prepare_cache_dir(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true).mode(0o700);
    builder.create(dir)?;
    verify_private_cache_dir(dir)
}

#[cfg(windows)]
fn prepare_cache_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)
}

#[cfg(unix)]
fn verify_private_cache_dir(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let meta = fs::symlink_metadata(dir)?;
    if meta.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "cache directory is a symlink",
        ));
    }
    if !meta.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cache path is not a directory",
        ));
    }
    let uid = current_uid();
    if meta.uid() != uid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("cache directory is owned by uid {}, not {uid}", meta.uid()),
        ));
    }
    if meta.mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "cache directory permissions {:o} allow group or other access",
                meta.mode() & 0o7777
            ),
        ));
    }
    Ok(())
}

fn load_or_create_secret(dir: &Path) -> Option<[u8; 32]> {
    let key_path = dir.join("key");
    if let Some(secret) = read_secret(&key_path) {
        return Some(secret);
    }
    remove_key_file(&key_path);
    write_secret(&key_path)
}

fn remove_key_file(path: &Path) {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() || meta.file_type().is_symlink() => {
            let _ = fs::remove_file(path);
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => {}
    }
}

fn read_secret(path: &Path) -> Option<[u8; 32]> {
    let file = open_secret(path).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.uid() != current_uid() || meta.mode() & 0o077 != 0 {
            return None;
        }
    }
    use std::io::Read;
    let mut bytes = Vec::new();
    file.take(33).read_to_end(&mut bytes).ok()?;
    if bytes.len() != 32 {
        return None;
    }
    let mut secret = [0_u8; 32];
    secret.copy_from_slice(&bytes);
    Some(secret)
}

fn open_secret(path: &Path) -> io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(rustix::fs::OFlags::NOFOLLOW.bits().cast_signed());
    }
    options.open(path)
}

fn write_secret(path: &Path) -> Option<[u8; 32]> {
    let mut secret = [0_u8; 32];
    getrandom::fill(&mut secret).ok()?;

    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
        options.custom_flags(rustix::fs::OFlags::NOFOLLOW.bits().cast_signed());
    }
    let mut file = options.open(path).ok()?;
    use std::io::Write;
    file.write_all(&secret).ok()?;
    file.flush().ok()?;
    Some(secret)
}

fn read_verified_entry(secret: &[u8; 32], path: &Path) -> Option<Vec<u8>> {
    let bytes = fs::read(path).ok()?;
    if bytes.len() <= 32 {
        let _ = fs::remove_file(path);
        return None;
    }
    let (mac, serialized) = bytes.split_at(32);
    let Ok(stored) = blake3::Hash::from_slice(mac) else {
        let _ = fs::remove_file(path);
        return None;
    };
    let expected = blake3::keyed_hash(secret, serialized);
    if stored == expected {
        Some(serialized.to_vec())
    } else {
        let _ = fs::remove_file(path);
        None
    }
}

fn write_verified_entry(
    dir: &Path,
    path: &Path,
    secret: &[u8; 32],
    serialized: &[u8],
) -> io::Result<()> {
    use std::io::Write;
    let mac = blake3::keyed_hash(secret, serialized);
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(mac.as_bytes())?;
    tmp.write_all(serialized)?;
    tmp.flush()?;
    // Atomic rename; replaces any existing entry (including a symlink) without
    // following it.
    tmp.persist(path).map_err(|error| error.error)?;
    Ok(())
}

/// Assemble a sandboxed store: WASI with no ambient authority plus fuel, epoch and
/// memory limits.
fn build_store(
    engine: &Engine,
    wasi: WasiCtx,
    font_data: Vec<u8>,
    typeset: Arc<TypesetEngine>,
    limiter: StoreLimits,
    budget: (u64, u64),
    policy: SandboxLimits,
) -> Store<HostState> {
    let state = HostState {
        table: ResourceTable::new(),
        wasi,
        font_data,
        typeset,
        limiter,
        logs: Vec::new(),
        policy,
    };
    let mut store = Store::new(engine, state);
    let _ = store.set_fuel(budget.0);
    store.set_epoch_deadline(budget.1);
    store.limiter(|s| &mut s.limiter);
    store
}

/// Central registry managing plugin loading, sandboxing, output caching and scene resolution.
pub struct PluginHost {
    engine: Engine,
    linker: Arc<Linker<HostState>>,
    disk_cache: ComponentDiskCache,
    plugins: BTreeMap<String, LoadedPlugin>,
    limits: SandboxLimits,
    font_data: Vec<u8>,
    typeset: Arc<TypesetEngine>,
    output_cache: LruCache<[u8; 32], VectorScene>,
    cache_hits: AtomicUsize,
    cache_misses: AtomicUsize,
    /// Registry and sandboxed runner for pixel-effect plugins, shared with every
    /// [`EffectHandle`] handed to a renderer.
    effects: Arc<effects::EffectRuntime>,
    /// Registry and sandboxed, instance-pooled runner for audio-effect plugins.
    audio_effects: Arc<audio::AudioRuntime>,
    /// Held only so its `Drop` stops and joins the epoch ticker thread.
    _epoch_ticker: Arc<EpochTicker>,
    /// One message per plugin package that was found but could not be loaded.
    load_failures: Vec<String>,
}

impl PluginHost {
    /// Create a new plugin host with default sandbox limits and disk cache in `cache_dir`.
    pub fn new(cache_dir: impl Into<PathBuf>) -> Result<Self, HostError> {
        Self::with_limits(cache_dir, SandboxLimits::default())
    }

    /// Create a plugin host with explicit sandbox limits.
    pub fn with_limits(
        cache_dir: impl Into<PathBuf>,
        limits: SandboxLimits,
    ) -> Result<Self, HostError> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        config.consume_fuel(true);
        config.epoch_interruption(true);

        let engine = Engine::new(&config)?;
        let mut linker: Linker<HostState> = Linker::new(&engine);

        wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
        SourcePlugin::add_to_linker::<HostState, HasSelf<HostState>>(&mut linker, |s| s)?;
        compat::add_to_linker(&mut linker)?;

        let font_data = load_system_font_bytes();
        let typeset = Arc::new(
            TypesetEngine::new().map_err(|e| HostError::CompilationFailed(e.to_string()))?,
        );
        let disk_cache = ComponentDiskCache::new(cache_dir);
        let output_cache = LruCache::new(NonZeroUsize::new(512).unwrap());
        let epoch_ticker = Arc::new(EpochTicker::start(engine.clone(), EPOCH_TICK_INTERVAL));
        let linker = Arc::new(linker);
        let audio_effects = Arc::new(audio::AudioRuntime::new(
            engine.clone(),
            Arc::clone(&linker),
            limits,
            Arc::clone(&typeset),
            Arc::clone(&epoch_ticker),
        ));
        let effects = Arc::new(effects::EffectRuntime::new(
            engine.clone(),
            Arc::clone(&linker),
            limits,
            Arc::clone(&typeset),
            Arc::clone(&epoch_ticker),
        ));

        Ok(Self {
            engine,
            linker,
            effects,
            audio_effects,
            disk_cache,
            plugins: BTreeMap::new(),
            limits,
            font_data,
            typeset,
            output_cache,
            cache_hits: AtomicUsize::new(0),
            cache_misses: AtomicUsize::new(0),
            _epoch_ticker: epoch_ticker,
            load_failures: Vec::new(),
        })
    }

    /// Set font data used for host-side text shaping.
    pub fn set_font_data(&mut self, font_bytes: Vec<u8>) {
        self.font_data = font_bytes;
    }

    /// Number of cache hits recorded.
    pub fn cache_hits(&self) -> usize {
        self.cache_hits.load(Ordering::Relaxed)
    }

    /// Number of cache misses recorded.
    pub fn cache_misses(&self) -> usize {
        self.cache_misses.load(Ordering::Relaxed)
    }

    /// Clear output cache.
    pub fn clear_output_cache(&mut self) {
        self.output_cache.clear();
    }

    /// Load an `.amplugin` file from disk.
    pub fn load_plugin_file(&mut self, path: impl AsRef<Path>) -> Result<String, HostError> {
        let bytes = fs::read(path)?;
        let package = PluginPackage::from_bytes(&bytes)?;
        self.load_plugin_package(package)
    }

    /// Load all `.amplugin` files from a trusted directory.
    ///
    /// For application discovery, use [`Self::load_plugin_directory`] so user
    /// installations cannot claim reserved `org.animathio.*` ids.
    pub fn load_plugins_dir(&mut self, dir: impl AsRef<Path>) -> Result<Vec<String>, HostError> {
        self.load_directory(dir.as_ref(), true)
    }

    /// Packages that were found in a loaded directory but failed to load (bad
    /// manifest, reserved id, compile error), one message each.
    #[must_use]
    pub fn load_failures(&self) -> &[String] {
        &self.load_failures
    }

    /// Load every discovered plugin directory ([`plugin_search_dirs`]) and return one
    /// message per problem: an `ANIMATHIO_PLUGIN_DIR` override that is not a
    /// directory, an unreadable directory, or a package that failed to load. Optional
    /// directories that simply do not exist are not problems. An empty list does not
    /// mean any plugin was found; clips of a missing plugin fail at export.
    pub fn load_discovered_plugins(&mut self) -> Vec<String> {
        let mut problems = Vec::new();
        if let Some(path) = discovery::invalid_override_dir() {
            problems.push(format!(
                "ANIMATHIO_PLUGIN_DIR {} is not a directory",
                path.display()
            ));
        }
        for dir in plugin_search_dirs() {
            let before = self.load_failures.len();
            if let Err(error) = self.load_plugin_directory(&dir) {
                problems.push(format!("plugins in {}: {error}", dir.path.display()));
            }
            problems.extend(self.load_failures[before..].iter().cloned());
        }
        problems
    }

    /// Load a discovered directory, enforcing its reserved-id policy before
    /// compiling or instantiating any component.
    pub fn load_plugin_directory(
        &mut self,
        dir: &PluginDirectory,
    ) -> Result<Vec<String>, HostError> {
        self.load_directory(&dir.path, dir.allow_reserved_ids)
    }

    fn load_directory(
        &mut self,
        dir: &Path,
        allow_reserved: bool,
    ) -> Result<Vec<String>, HostError> {
        let mut ids = Vec::new();
        if !dir.exists() {
            return Ok(ids);
        }
        let mut paths = fs::read_dir(dir)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()?;
        paths.sort();
        for path in paths {
            if !path.extension().is_some_and(|ext| ext == "amplugin") {
                continue;
            }
            let result = (|| {
                let package = PluginPackage::from_bytes(&fs::read(&path)?)?;
                check_plugin_id(&package.manifest.id, allow_reserved)?;
                self.load_plugin_package(package)
            })();
            match result {
                Ok(id) => ids.push(id),
                Err(error) => {
                    log::warn!("plugin {} was not loaded: {error}", path.display());
                    self.load_failures
                        .push(format!("plugin {} was not loaded: {error}", path.display()));
                }
            }
        }
        Ok(ids)
    }

    /// Load a `PluginPackage` directly into the host.
    ///
    /// Plugin ids must be unique within a host. Signing is deferred: an untrusted package may
    /// still claim any id, so callers must only load packages from trusted sources for now.
    pub fn load_plugin_package(&mut self, package: PluginPackage) -> Result<String, HostError> {
        package
            .manifest
            .validate()
            .map_err(|e| HostError::InvalidOutput(e.to_string()))?;
        let legacy = package.manifest.api_version.starts_with("0.");
        let id = package.manifest.id.clone();
        if self.plugins.contains_key(&id) {
            return Err(HostError::DuplicateId(id));
        }

        let wasm_hash = *blake3::hash(&package.wasm_bytes).as_bytes();
        let component = self
            .disk_cache
            .get_or_compile(&self.engine, &package.wasm_bytes)?;

        // Describe plugin to extract WIT manifest and parameter schema
        let mut store = self.create_store(CapabilityGrants::default())?;
        let wit_manifest = if package.manifest.kinds.contains(&PluginKind::AudioEffect) {
            self.audio_effects
                .register(&package.manifest, &component, &mut store, legacy)
                .map_err(|e| {
                    HostError::CompilationFailed(format!("audio effect plugin {id}: {e}"))
                })?
        } else if package.manifest.kinds.contains(&PluginKind::Effect) {
            self.effects
                .register(&package.manifest, &component, &mut store, legacy)
                .map_err(|e| HostError::CompilationFailed(format!("effect plugin {id}: {e}")))?
        } else {
            let pre = compat::Pre::new(self.linker.instantiate_pre(&component)?, legacy);
            let instance = pre.instantiate(&mut store)?;
            instance.validate_source(&mut store)?;
            let manifest = instance.describe(&mut store, false)?;
            validate_manifest(&package.manifest, &manifest)?;
            manifest
        };

        let inspector_schema = wit_manifest_to_inspector(&wit_manifest);

        self.plugins.insert(
            id.clone(),
            LoadedPlugin {
                manifest: package.manifest,
                component,
                wasm_hash,
                inspector_schema,
                wit_manifest,
            },
        );

        Ok(id)
    }

    /// Get inspector schema for a loaded plugin.
    pub fn get_inspector_schema(&self, plugin_id: &str) -> Option<&InspectorSchema> {
        self.plugins.get(plugin_id).map(|p| &p.inspector_schema)
    }

    /// Get loaded plugin information.
    pub fn get_plugin(&self, plugin_id: &str) -> Option<&LoadedPlugin> {
        self.plugins.get(plugin_id)
    }

    /// List all loaded plugin IDs.
    pub fn loaded_plugin_ids(&self) -> Vec<String> {
        self.plugins.keys().cloned().collect()
    }

    fn create_store(&self, capabilities: CapabilityGrants) -> Result<Store<HostState>, HostError> {
        let mut wasi_builder = WasiCtxBuilder::new();
        for (k, v) in &capabilities.environment_variables {
            wasi_builder.env(k, v);
        }
        let wasi = wasi_builder.build();
        Ok(build_store(
            &self.engine,
            wasi,
            self.font_data.clone(),
            Arc::clone(&self.typeset),
            sandbox_store_limits(&self.limits),
            (self.limits.fuel_per_call, self.limits.epoch_deadline_ticks),
            self.limits,
        ))
    }

    /// Pixel effects offered by every loaded effect plugin, in plugin-id then listing order.
    #[must_use]
    pub fn effects(&self) -> Vec<EffectDescriptor> {
        self.effects.descriptors()
    }

    /// Audio filters offered by every loaded audio-effect plugin, in plugin-id then listing order.
    #[must_use]
    pub fn audio_effects(&self) -> Vec<EffectDescriptor> {
        self.audio_effects.descriptors()
    }

    /// A cheap handle that runs audio-effect plugins for the mixer. It shares this host's
    /// plugin registry and warm-instance pools, so plugins loaded later become available to
    /// existing handles.
    #[must_use]
    pub fn audio_effect_processor(&self) -> AudioEffectHandle {
        AudioEffectHandle::new(Arc::clone(&self.audio_effects))
    }

    /// Check the effect parameters `command` would store against the loaded plugins'
    /// declared ranges (see [`validate_effect_command`]).
    pub fn validate_effect_command(
        &self,
        project: &animathio_core::Project,
        command: &animathio_core::Command,
    ) -> Result<(), String> {
        validate_effect_command(project, command, &|| self.effects())
    }

    /// A cheap handle that runs effect plugins for a renderer. It shares this host's
    /// plugin registry, so plugins loaded later become available to existing handles.
    #[must_use]
    pub fn effect_processor(&self) -> EffectHandle {
        EffectHandle::new(Arc::clone(&self.effects))
    }

    /// Execute a clip-source plugin under sandbox protection and return a VectorScene.
    pub fn render_clip(
        &mut self,
        plugin_id: &str,
        params: &BTreeMap<String, animathio_core::ParamValue>,
        opaque_params: &serde_json::Value,
        time_ticks: i64,
        canvas_size: (u32, u32),
    ) -> Result<VectorScene, HostError> {
        let plugin = self
            .plugins
            .get(plugin_id)
            .ok_or_else(|| HostError::NotFound(plugin_id.to_string()))?;

        // 1. Check output cache
        let cache_key = compute_cache_key(
            &plugin.wasm_hash,
            time_ticks,
            canvas_size,
            params,
            opaque_params,
        );

        if let Some(cached) = self.output_cache.get(&cache_key) {
            self.cache_hits.fetch_add(1, Ordering::Relaxed);
            return Ok(cached.clone());
        }

        self.cache_misses.fetch_add(1, Ordering::Relaxed);

        // 2. Prepare WIT parameters (typed against the plugin's declared schema)
        let wit_params = build_wit_params(&plugin.wit_manifest, params, opaque_params);
        let size = wit_types::Size {
            width: canvas_size.0,
            height: canvas_size.1,
        };

        // 3. Sandboxed execution with fuel and epoch limit
        let mut store = self.create_store(CapabilityGrants::default())?;
        let pre = compat::Pre::new(
            self.linker.instantiate_pre(&plugin.component)?,
            plugin.manifest.api_version.starts_with("0."),
        );
        let instance = pre
            .instantiate(&mut store)
            .map_err(|e| HostError::Trapped(e.to_string()))?;

        let wit_scene = instance
            .render(&mut store, &wit_params, time_ticks, size)
            .map_err(|e| HostError::Trapped(e.to_string()))?
            .map_err(|e| HostError::InvalidOutput(format!("{}: {}", e.code, e.message)))?;

        let core_scene = wit_scene_to_core(wit_scene);
        core_scene
            .validate()
            .map_err(|e| HostError::InvalidOutput(e.to_string()))?;

        // 4. Cache output
        self.output_cache.put(cache_key, core_scene.clone());

        Ok(core_scene)
    }

    /// Resolve an evaluated scene by running all plugin clips and replacing them with vector scenes.
    /// Render can then draw it without knowing about plugins.
    /// Trapped or missing plugins are converted to `ClipKind::Error` without crashing the host.
    pub fn resolve(&mut self, scene: &CoreScene, canvas_size: (u32, u32)) -> CoreScene {
        let mut resolved_scene = CoreScene {
            clips: Vec::with_capacity(scene.clips.len()),
        };

        for clip in &scene.clips {
            let mut resolved_clip = clip.clone();

            if let ClipKind::Plugin {
                ref plugin_id,
                ref params,
            } = clip.kind
            {
                match self.render_clip(
                    plugin_id,
                    &clip.params,
                    params,
                    clip.source_time.0,
                    canvas_size,
                ) {
                    Ok(vector_scene) => {
                        resolved_clip.kind = ClipKind::Vector(vector_scene);
                    }
                    Err(err) => {
                        log::warn!("Plugin '{}' failed to evaluate: {}", plugin_id, err);
                        resolved_clip.kind =
                            ClipKind::Error(format!("Plugin '{plugin_id}' error: {err}"));
                    }
                }
            }

            resolved_scene.clips.push(resolved_clip);
        }

        resolved_scene
    }
}

fn compute_cache_key(
    plugin_hash: &[u8; 32],
    time_ticks: i64,
    size: (u32, u32),
    params: &BTreeMap<String, animathio_core::ParamValue>,
    opaque_params: &serde_json::Value,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(plugin_hash);
    hasher.update(&time_ticks.to_le_bytes());
    hasher.update(&size.0.to_le_bytes());
    hasher.update(&size.1.to_le_bytes());

    // Length-delimited structured encoding includes the variant and avoids ambiguous strings.
    hasher.update(&serde_json::to_vec(params).expect("serializable parameter values"));

    if !opaque_params.is_null()
        && let Ok(s) = serde_json::to_string(opaque_params)
    {
        hasher.update(s.as_bytes());
    }

    *hasher.finalize().as_bytes()
}

fn build_wit_params(
    manifest: &wit_types::Manifest,
    typed_params: &BTreeMap<String, animathio_core::ParamValue>,
    opaque: &serde_json::Value,
) -> Vec<wit_types::ParamEntry> {
    let mut entries = Vec::new();
    let mut schema = BTreeMap::new();
    for spec in &manifest.params {
        schema.insert(spec.id.as_str(), &spec.kind);
    }

    if let serde_json::Value::Object(map) = opaque {
        for (k, v) in map {
            if typed_params.contains_key(k) {
                continue; // evaluated typed param overrides opaque base data
            }
            if let Some(value) = json_to_param_value(v, schema.get(k.as_str()).copied()) {
                entries.push(wit_types::ParamEntry {
                    id: k.clone(),
                    value,
                });
            }
        }
    }

    for (k, v) in typed_params {
        entries.push(wit_types::ParamEntry {
            id: k.clone(),
            value: typed_to_param_value(v),
        });
    }

    entries
}

// Typed values already carry their identity; only legacy JSON needs schema inference.
fn typed_to_param_value(value: &animathio_core::ParamValue) -> wit_types::ParamValue {
    use animathio_core::ParamValue as CoreValue;
    use wit_types::ParamValue as WitValue;
    match value {
        CoreValue::Number(v) => WitValue::Number(*v),
        CoreValue::Color(_) => {
            WitValue::Color(json_to_color(&value.to_plugin_json()).expect("RGBA bytes"))
        }
        CoreValue::Point(v) => WitValue::Point(wit_types::Point {
            x: v[0] as f32,
            y: v[1] as f32,
        }),
        CoreValue::Bool(v) => WitValue::Bool(*v),
        CoreValue::Enum(v) => WitValue::EnumChoice(v.clone()),
        CoreValue::String(v) => WitValue::String(v.clone()),
        CoreValue::AssetRef(v) => WitValue::AssetRef(
            v.as_ref()
                .map_or_else(String::new, |id| id.as_str().to_owned()),
        ),
    }
}

fn json_to_param_value(
    value: &serde_json::Value,
    kind: Option<&wit_types::ParamKind>,
) -> Option<wit_types::ParamValue> {
    match kind {
        Some(wit_types::ParamKind::EnumChoice(_)) => value
            .as_str()
            .map(|s| wit_types::ParamValue::EnumChoice(s.to_string())),
        Some(wit_types::ParamKind::Bool(_)) => value.as_bool().map(wit_types::ParamValue::Bool),
        Some(wit_types::ParamKind::Color(_)) => {
            json_to_color(value).map(wit_types::ParamValue::Color)
        }
        Some(wit_types::ParamKind::Point(_)) => {
            json_to_point(value).map(wit_types::ParamValue::Point)
        }
        Some(wit_types::ParamKind::AssetRef(_)) => value
            .as_str()
            .map(|s| wit_types::ParamValue::AssetRef(s.to_string())),
        Some(wit_types::ParamKind::Number(_)) => value.as_f64().map(wit_types::ParamValue::Number),
        Some(wit_types::ParamKind::String(_)) => value
            .as_str()
            .map(|s| wit_types::ParamValue::String(s.to_string())),
        None => match value {
            serde_json::Value::Number(n) => n.as_f64().map(wit_types::ParamValue::Number),
            serde_json::Value::Bool(b) => Some(wit_types::ParamValue::Bool(*b)),
            serde_json::Value::String(s) => Some(wit_types::ParamValue::String(s.clone())),
            serde_json::Value::Array(arr) if arr.len() == 4 => {
                json_to_color(value).map(wit_types::ParamValue::Color)
            }
            serde_json::Value::Array(arr) if arr.len() == 2 => {
                json_to_point(value).map(wit_types::ParamValue::Point)
            }
            _ => None,
        },
    }
}

fn json_to_color(value: &serde_json::Value) -> Option<wit_types::Color> {
    if let serde_json::Value::Array(arr) = value
        && arr.len() == 4
    {
        return Some(wit_types::Color {
            r: arr[0].as_u64().unwrap_or(0) as u8,
            g: arr[1].as_u64().unwrap_or(0) as u8,
            b: arr[2].as_u64().unwrap_or(0) as u8,
            a: arr[3].as_u64().unwrap_or(255) as u8,
        });
    }
    if let serde_json::Value::Object(map) = value {
        return Some(wit_types::Color {
            r: map
                .get("r")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as u8,
            g: map
                .get("g")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as u8,
            b: map
                .get("b")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as u8,
            a: map
                .get("a")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(255) as u8,
        });
    }
    None
}

fn json_to_point(value: &serde_json::Value) -> Option<wit_types::Point> {
    if let serde_json::Value::Array(arr) = value
        && arr.len() == 2
    {
        return Some(wit_types::Point {
            x: arr[0].as_f64().unwrap_or(0.0) as f32,
            y: arr[1].as_f64().unwrap_or(0.0) as f32,
        });
    }
    if let serde_json::Value::Object(map) = value {
        return Some(wit_types::Point {
            x: map
                .get("x")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0) as f32,
            y: map
                .get("y")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0) as f32,
        });
    }
    None
}

fn wit_scene_to_core(scene: wit_types::Scene) -> VectorScene {
    let mut elements = Vec::with_capacity(scene.elements.len());

    for cmd in scene.elements {
        match cmd {
            wit_types::DrawCommand::FillPath((verbs, color)) => {
                elements.push(animathio_core::VectorElement::FillPath {
                    path: verbs.into_iter().map(wit_verb_to_core).collect(),
                    color: animathio_core::Color::rgba(color.r, color.g, color.b, color.a),
                });
            }
            wit_types::DrawCommand::StrokePath((verbs, style)) => {
                let cap = match style.cap {
                    wit_types::StrokeCap::Butt => animathio_core::StrokeCap::Butt,
                    wit_types::StrokeCap::Round => animathio_core::StrokeCap::Round,
                    wit_types::StrokeCap::Square => animathio_core::StrokeCap::Square,
                };
                let join = match style.join {
                    wit_types::StrokeJoin::Miter => animathio_core::StrokeJoin::Miter,
                    wit_types::StrokeJoin::Round => animathio_core::StrokeJoin::Round,
                    wit_types::StrokeJoin::Bevel => animathio_core::StrokeJoin::Bevel,
                };
                elements.push(animathio_core::VectorElement::StrokePath {
                    path: verbs.into_iter().map(wit_verb_to_core).collect(),
                    style: animathio_core::StrokeStyle {
                        width: style.width as f64,
                        color: animathio_core::Color::rgba(
                            style.color.r,
                            style.color.g,
                            style.color.b,
                            style.color.a,
                        ),
                        cap,
                        join,
                    },
                });
            }
            wit_types::DrawCommand::TextRun(text_elem) => {
                elements.push(animathio_core::VectorElement::TextRun(
                    animathio_core::TextElement {
                        align: match text_elem.align {
                            wit_types::TextAlignment::Left => animathio_core::TextAlignment::Left,
                            wit_types::TextAlignment::Center => {
                                animathio_core::TextAlignment::Center
                            }
                            wit_types::TextAlignment::Right => animathio_core::TextAlignment::Right,
                        },
                        text: text_elem.text,
                        font_size: text_elem.font_size as f64,
                        x: text_elem.x as f64,
                        y: text_elem.y as f64,
                        color: animathio_core::Color::rgba(
                            text_elem.color.r,
                            text_elem.color.g,
                            text_elem.color.b,
                            text_elem.color.a,
                        ),
                    },
                ));
            }
        }
    }

    VectorScene {
        coordinate_space: match scene.coordinate_space {
            wit_types::CoordinateSpace::Canvas => animathio_core::CoordinateSpace::Canvas,
            wit_types::CoordinateSpace::Normalized => animathio_core::CoordinateSpace::Normalized,
        },
        elements,
    }
}

fn wit_verb_to_core(v: wit_types::PathVerb) -> animathio_core::PathVerb {
    match v {
        wit_types::PathVerb::MoveTo(p) => {
            animathio_core::PathVerb::MoveTo(animathio_core::Point::new(p.x as f64, p.y as f64))
        }
        wit_types::PathVerb::LineTo(p) => {
            animathio_core::PathVerb::LineTo(animathio_core::Point::new(p.x as f64, p.y as f64))
        }
        wit_types::PathVerb::QuadTo((c, to)) => animathio_core::PathVerb::QuadTo(
            animathio_core::Point::new(c.x as f64, c.y as f64),
            animathio_core::Point::new(to.x as f64, to.y as f64),
        ),
        wit_types::PathVerb::CubicTo((c1, c2, to)) => animathio_core::PathVerb::CubicTo(
            animathio_core::Point::new(c1.x as f64, c1.y as f64),
            animathio_core::Point::new(c2.x as f64, c2.y as f64),
            animathio_core::Point::new(to.x as f64, to.y as f64),
        ),
        wit_types::PathVerb::Close => animathio_core::PathVerb::Close,
    }
}

fn wit_manifest_to_inspector(manifest: &wit_types::Manifest) -> InspectorSchema {
    let mut schema = InspectorSchema::new();

    for param in &manifest.params {
        let widget = match &param.kind {
            wit_types::ParamKind::Number(n) => {
                animathio_plugin_api::ControlWidget::Slider(animathio_plugin_api::SliderControl {
                    min: n.min,
                    max: n.max,
                    default: n.default,
                    step: n.step,
                })
            }
            wit_types::ParamKind::Color(c) => animathio_plugin_api::ControlWidget::ColorPicker {
                default: animathio_plugin_api::ColorValue {
                    r: c.default.r,
                    g: c.default.g,
                    b: c.default.b,
                    a: c.default.a,
                },
            },
            wit_types::ParamKind::String(s) => animathio_plugin_api::ControlWidget::TextInput {
                default: s.default.clone(),
            },
            wit_types::ParamKind::EnumChoice(e) => animathio_plugin_api::ControlWidget::Dropdown {
                options: e.options.clone(),
                default: e.default.clone(),
            },
            wit_types::ParamKind::Bool(b) => {
                animathio_plugin_api::ControlWidget::Toggle { default: b.default }
            }
            wit_types::ParamKind::Point(p) => animathio_plugin_api::ControlWidget::Point2D {
                default: animathio_plugin_api::PointValue {
                    x: p.default.x,
                    y: p.default.y,
                },
            },
            wit_types::ParamKind::AssetRef(a) => animathio_plugin_api::ControlWidget::AssetPicker {
                media_type: a.media_type.clone(),
                default_asset: a.default_asset.clone(),
            },
        };

        schema.params.push(animathio_plugin_api::InspectorParam {
            id: param.id.clone(),
            label: param.label.clone(),
            keyframeable: true,
            widget,
        });
    }

    schema
}

fn load_system_font_bytes() -> Vec<u8> {
    const CANDIDATES: &[&str] = &[
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/usr/share/fonts/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/TTF/OpenSans-Regular.ttf",
        "/usr/share/fonts/TTF/Lato-Regular.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        // Windows fallbacks (CI runs the host tests on windows-latest too).
        "C:\\Windows\\Fonts\\arial.ttf",
        "C:\\Windows\\Fonts\\segoeui.ttf",
    ];

    for path in CANDIDATES {
        if let Ok(bytes) = fs::read(path) {
            return bytes;
        }
    }

    log::warn!("No system font found; host text shaping will return empty contours");
    Vec::new()
}

/// Truncate a string to at most `max` bytes on a UTF-8 character boundary.
fn truncate_utf8(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_owned();
    }
    let mut end = max;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

fn shape_text_skrifa(
    font_bytes: &[u8],
    text: &str,
    font_size: f32,
    max_verbs: usize,
) -> Vec<Vec<wit_types::PathVerb>> {
    if font_bytes.is_empty() || text.is_empty() {
        return Vec::new();
    }

    let Ok(font) = skrifa::FontRef::new(font_bytes) else {
        return Vec::new();
    };

    use skrifa::MetadataProvider;
    let outlines = font.outline_glyphs();
    let charmap = font.charmap();
    let metrics = font.glyph_metrics(
        skrifa::instance::Size::new(font_size),
        skrifa::instance::LocationRef::default(),
    );

    struct OutlineCollector {
        paths: Vec<Vec<wit_types::PathVerb>>,
        current: Vec<wit_types::PathVerb>,
        cursor_x: f32,
    }

    impl skrifa::outline::OutlinePen for OutlineCollector {
        fn move_to(&mut self, x: f32, y: f32) {
            if !self.current.is_empty() {
                self.paths.push(std::mem::take(&mut self.current));
            }
            self.current
                .push(wit_types::PathVerb::MoveTo(wit_types::Point {
                    x: self.cursor_x + x,
                    y: -y,
                }));
        }

        fn line_to(&mut self, x: f32, y: f32) {
            self.current
                .push(wit_types::PathVerb::LineTo(wit_types::Point {
                    x: self.cursor_x + x,
                    y: -y,
                }));
        }

        fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
            self.current.push(wit_types::PathVerb::QuadTo((
                wit_types::Point {
                    x: self.cursor_x + cx0,
                    y: -cy0,
                },
                wit_types::Point {
                    x: self.cursor_x + x,
                    y: -y,
                },
            )));
        }

        fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
            self.current.push(wit_types::PathVerb::CubicTo((
                wit_types::Point {
                    x: self.cursor_x + cx0,
                    y: -cy0,
                },
                wit_types::Point {
                    x: self.cursor_x + cx1,
                    y: -cy1,
                },
                wit_types::Point {
                    x: self.cursor_x + x,
                    y: -y,
                },
            )));
        }

        fn close(&mut self) {
            self.current.push(wit_types::PathVerb::Close);
            if !self.current.is_empty() {
                self.paths.push(std::mem::take(&mut self.current));
            }
        }
    }

    let mut collector = OutlineCollector {
        paths: Vec::new(),
        current: Vec::new(),
        cursor_x: 0.0,
    };

    for ch in text.chars() {
        if ch == ' ' {
            collector.cursor_x += font_size * 0.32;
            continue;
        }

        let glyph_id = charmap.map(ch).unwrap_or_default();
        if let Some(glyph) = outlines.get(glyph_id) {
            let draw_settings = skrifa::outline::DrawSettings::unhinted(
                skrifa::instance::Size::new(font_size),
                skrifa::instance::LocationRef::default(),
            );
            let _ = glyph.draw(draw_settings, &mut collector);
        }
        let advance = metrics.advance_width(glyph_id).unwrap_or(font_size * 0.6);
        collector.cursor_x += advance;
    }

    if !collector.current.is_empty() {
        collector.paths.push(collector.current);
    }

    let total_verbs: usize = collector.paths.iter().map(Vec::len).sum();
    if total_verbs > max_verbs {
        log::warn!("shape_text: rejecting {total_verbs} path verbs (max {max_verbs})");
        return Vec::new();
    }

    collector.paths
}

fn parse_svg_to_paths(svg: &str, max_verbs: usize) -> Vec<Vec<wit_types::PathVerb>> {
    let mut results = Vec::new();
    let trimmed = svg.trim();
    if trimmed.is_empty() {
        return results;
    }

    if !trimmed.starts_with('<') {
        if let Ok(bez) = kurbo::BezPath::from_svg(trimmed) {
            let verbs = bez_to_wit_verbs(&bez);
            if !verbs.is_empty() {
                results.push(verbs);
            }
        }
        return results;
    }

    let mut cursor = 0;
    while let Some(d_idx) = svg[cursor..].find("d=\"") {
        let start = cursor + d_idx + 3;
        if let Some(end_offset) = svg[start..].find('"') {
            let d_str = &svg[start..start + end_offset];
            if let Ok(bez) = kurbo::BezPath::from_svg(d_str) {
                let verbs = bez_to_wit_verbs(&bez);
                if !verbs.is_empty() {
                    results.push(verbs);
                }
            }
            cursor = start + end_offset + 1;
        } else {
            break;
        }
    }

    let total_verbs: usize = results.iter().map(Vec::len).sum();
    if total_verbs > max_verbs {
        log::warn!("svg_to_paths: rejecting {total_verbs} path verbs (max {max_verbs})");
        return Vec::new();
    }

    results
}

fn bez_to_wit_verbs(bez: &kurbo::BezPath) -> Vec<wit_types::PathVerb> {
    let mut verbs = Vec::new();
    for el in bez.elements() {
        match el {
            kurbo::PathEl::MoveTo(p) => verbs.push(wit_types::PathVerb::MoveTo(wit_types::Point {
                x: p.x as f32,
                y: p.y as f32,
            })),
            kurbo::PathEl::LineTo(p) => verbs.push(wit_types::PathVerb::LineTo(wit_types::Point {
                x: p.x as f32,
                y: p.y as f32,
            })),
            kurbo::PathEl::QuadTo(p1, p2) => verbs.push(wit_types::PathVerb::QuadTo((
                wit_types::Point {
                    x: p1.x as f32,
                    y: p1.y as f32,
                },
                wit_types::Point {
                    x: p2.x as f32,
                    y: p2.y as f32,
                },
            ))),
            kurbo::PathEl::CurveTo(p1, p2, p3) => verbs.push(wit_types::PathVerb::CubicTo((
                wit_types::Point {
                    x: p1.x as f32,
                    y: p1.y as f32,
                },
                wit_types::Point {
                    x: p2.x as f32,
                    y: p2.y as f32,
                },
                wit_types::Point {
                    x: p3.x as f32,
                    y: p3.y as f32,
                },
            ))),
            kurbo::PathEl::ClosePath => verbs.push(wit_types::PathVerb::Close),
        }
    }
    verbs
}

/// Convert a host typesetting result into WIT types, enforcing the output cap.
fn typeset_to_wit(result: &TypesetResult, max_verbs: usize) -> wit_types::TypesetResult {
    let total_verbs: usize = result.paths.iter().map(Vec::len).sum();
    if total_verbs > max_verbs {
        log::warn!("typeset_math: rejecting {total_verbs} path verbs (max {max_verbs})");
        return wit_types::TypesetResult {
            paths: Vec::new(),
            bbox: wit_types::Bbox {
                min_x: 0.0,
                min_y: 0.0,
                max_x: 0.0,
                max_y: 0.0,
            },
        };
    }

    let paths = result
        .paths
        .iter()
        .map(|path| path.iter().map(typeset_verb_to_wit).collect())
        .collect();

    wit_types::TypesetResult {
        paths,
        bbox: wit_types::Bbox {
            min_x: result.bbox.min_x as f32,
            min_y: result.bbox.min_y as f32,
            max_x: result.bbox.max_x as f32,
            max_y: result.bbox.max_y as f32,
        },
    }
}

fn typeset_verb_to_wit(verb: &TypesetVerb) -> wit_types::PathVerb {
    let point = |p: &animathio_typeset::TypesetPoint| wit_types::Point {
        x: p.x as f32,
        y: p.y as f32,
    };
    match *verb {
        TypesetVerb::MoveTo(p) => wit_types::PathVerb::MoveTo(point(&p)),
        TypesetVerb::LineTo(p) => wit_types::PathVerb::LineTo(point(&p)),
        TypesetVerb::QuadTo(c, p) => wit_types::PathVerb::QuadTo((point(&c), point(&p))),
        TypesetVerb::CubicTo(c1, c2, p) => {
            wit_types::PathVerb::CubicTo((point(&c1), point(&c2), point(&p)))
        }
        TypesetVerb::Close => wit_types::PathVerb::Close,
    }
}

fn check_plugin_id(id: &str, allow_reserved: bool) -> Result<(), HostError> {
    if !allow_reserved && id.starts_with("org.animathio.") {
        return Err(HostError::ReservedId(id.to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasmtime::{Instance, Module};

    #[test]
    #[allow(clippy::float_cmp)]
    fn typed_parameters_map_through_every_schema_kind() {
        use animathio_core::ParamValue as CoreValue;
        use wit_types::*;
        let cases = vec![
            (
                "n",
                ParamKind::Number(NumberRange {
                    min: 0.0,
                    max: 10.0,
                    default: 0.0,
                    step: 1.0,
                }),
                CoreValue::Number(2.5),
            ),
            (
                "c",
                ParamKind::Color(ColorParam {
                    default: Color {
                        r: 0,
                        g: 0,
                        b: 0,
                        a: 255,
                    },
                }),
                CoreValue::Color([0.5, 0.5, 0.5, 1.0]),
            ),
            (
                "p",
                ParamKind::Point(PointParam {
                    default: Point { x: 0.0, y: 0.0 },
                }),
                CoreValue::Point([0.25, 0.75]),
            ),
            (
                "b",
                ParamKind::Bool(BoolParam { default: false }),
                CoreValue::Bool(true),
            ),
            (
                "s",
                ParamKind::String(StringParam {
                    default: String::new(),
                }),
                CoreValue::String("hello".into()),
            ),
            (
                "e",
                ParamKind::EnumChoice(EnumParam {
                    options: vec!["a".into(), "b".into()],
                    default: "a".into(),
                }),
                CoreValue::Enum("b".into()),
            ),
            (
                "a",
                ParamKind::AssetRef(AssetRefParam {
                    media_type: "image/*".into(),
                    default_asset: None,
                }),
                CoreValue::AssetRef(Some(animathio_core::AssetId::of(b"image"))),
            ),
        ];
        let manifest = Manifest {
            name: "test".into(),
            version: "1".into(),
            description: String::new(),
            kinds: vec![],
            metadata: vec![],
            params: cases
                .iter()
                .map(|(id, kind, _)| ParamSpec {
                    id: (*id).into(),
                    label: (*id).into(),
                    kind: kind.clone(),
                })
                .collect(),
        };
        let params = cases
            .into_iter()
            .map(|(id, _, value)| (id.to_owned(), value))
            .collect();
        let entries = build_wit_params(
            &manifest,
            &params,
            &serde_json::json!({"c":[255,0,0,255], "e":"a", "legacy": true}),
        );
        assert_eq!(entries.len(), 8);
        let get = |id: &str| &entries.iter().find(|e| e.id == id).unwrap().value;
        assert!(matches!(get("n"), ParamValue::Number(v) if *v == 2.5));
        assert!(matches!(get("c"), ParamValue::Color(v) if (v.r,v.g,v.b,v.a) == (188,188,188,255)));
        assert!(matches!(get("p"), ParamValue::Point(v) if v.x == 0.25 && v.y == 0.75));
        assert!(matches!(get("b"), ParamValue::Bool(true)));
        assert!(matches!(get("s"), ParamValue::String(v) if v == "hello"));
        assert!(matches!(get("e"), ParamValue::EnumChoice(v) if v == "b"));
        assert!(
            matches!(get("a"), ParamValue::AssetRef(v) if v == animathio_core::AssetId::of(b"image").as_str())
        );
        assert!(matches!(get("legacy"), ParamValue::Bool(true)));
        let cleared = BTreeMap::from([("a".to_owned(), CoreValue::AssetRef(None))]);
        assert!(
            matches!(&build_wit_params(&manifest, &cleared, &serde_json::Value::Null)[0].value, ParamValue::AssetRef(v) if v.is_empty())
        );

        // A schema can change or omit a parameter; typed values keep their identity.
        let mut without_schema = manifest;
        without_schema.params.clear();
        let entries = build_wit_params(&without_schema, &params, &serde_json::Value::Null);
        assert_eq!(entries.len(), 7);
        let get = |id: &str| &entries.iter().find(|e| e.id == id).unwrap().value;
        assert!(matches!(get("e"), ParamValue::EnumChoice(v) if v == "b"));
        assert!(
            matches!(get("a"), ParamValue::AssetRef(v) if v == animathio_core::AssetId::of(b"image").as_str())
        );
        assert!(matches!(get("s"), ParamValue::String(v) if v == "hello"));
    }

    struct LimiterState {
        limiter: StoreLimits,
    }

    #[test]
    fn memory_growth_beyond_limit_traps() {
        let engine = Engine::default();
        let module = Module::new(
            &engine,
            r#"(module
                (memory 1)
                (func (export "grow") (param i32) (result i32)
                    local.get 0
                    memory.grow))"#,
        )
        .unwrap();
        let limits = SandboxLimits {
            max_memory_bytes: 64 * 1024,
            ..SandboxLimits::default()
        };
        let mut store = Store::new(
            &engine,
            LimiterState {
                limiter: sandbox_store_limits(&limits),
            },
        );
        store.limiter(|s| &mut s.limiter);
        let instance = Instance::new(&mut store, &module, &[]).unwrap();
        let grow = instance
            .get_typed_func::<i32, i32>(&mut store, "grow")
            .unwrap();
        assert!(grow.call(&mut store, 1024).is_err());
    }

    #[test]
    fn multiple_memories_are_rejected() {
        let engine = Engine::default();
        let module = Module::new(&engine, r#"(module (memory 1) (memory 1))"#).unwrap();
        let mut store = Store::new(
            &engine,
            LimiterState {
                limiter: sandbox_store_limits(&SandboxLimits::default()),
            },
        );
        store.limiter(|s| &mut s.limiter);
        assert!(Instance::new(&mut store, &module, &[]).is_err());
    }

    #[test]
    fn nan_vector_scene_fails_validation() {
        let wit_scene = wit_types::Scene {
            coordinate_space: wit_types::CoordinateSpace::Canvas,
            elements: vec![wit_types::DrawCommand::FillPath((
                vec![wit_types::PathVerb::MoveTo(wit_types::Point {
                    x: f32::NAN,
                    y: 0.0,
                })],
                wit_types::Color {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 255,
                },
            ))],
        };
        let core_scene = wit_scene_to_core(wit_scene);
        assert!(core_scene.validate().is_err());
    }

    #[test]
    fn oversized_log_messages_are_truncated() {
        assert_eq!(truncate_utf8("hello", 10), "hello");
        let long = "é".repeat(100);
        let truncated = truncate_utf8(&long, 4);
        assert!(truncated.len() <= 4);
        assert!(truncated.is_char_boundary(truncated.len()));
    }

    #[test]
    fn oversized_svg_input_is_rejected() {
        let svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0 {}\"/></svg>",
            "L1 1 ".repeat(MAX_SVG_INPUT_BYTES / 5)
        );
        assert!(svg.len() > MAX_SVG_INPUT_BYTES);
        assert!(parse_svg_to_paths(&svg, MAX_SVG_VERBS).is_empty());
    }

    fn component_engine() -> Engine {
        let mut config = Config::new();
        config.wasm_component_model(true);
        Engine::new(&config).unwrap()
    }

    fn empty_component() -> Vec<u8> {
        wat::parse_str("(component)").unwrap()
    }

    fn private_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        }
        dir
    }

    #[test]
    fn tampered_cache_entry_is_rejected_and_recompiled() {
        let tmp = private_dir();
        let cache = ComponentDiskCache::new(tmp.path());
        assert!(cache.is_enabled());

        let engine = component_engine();
        let wasm = empty_component();
        let entry = tmp
            .path()
            .join(format!("{}.bin", ComponentDiskCache::cache_key(&wasm)));

        cache.get_or_compile(&engine, &wasm).unwrap();
        assert!(entry.exists());

        // Flip one byte of the serialized payload without updating the MAC.
        let mut tampered = fs::read(&entry).unwrap();
        *tampered.last_mut().unwrap() ^= 0xff;
        fs::write(&entry, &tampered).unwrap();

        // The tampered entry must not be deserialized; it is recompiled and
        // replaced with a freshly MAC'd entry.
        cache.get_or_compile(&engine, &wasm).unwrap();
        let rewritten = fs::read(&entry).unwrap();
        assert_ne!(rewritten, tampered);

        let secret = read_secret(&tmp.path().join("key")).unwrap();
        assert!(read_verified_entry(&secret, &entry).is_some());
    }

    #[test]
    fn missing_or_invalid_key_is_regenerated() {
        let tmp = private_dir();
        let key_path = tmp.path().join("key");

        // A missing key is generated on first use.
        let cache = ComponentDiskCache::new(tmp.path());
        assert!(cache.is_enabled());
        let first = read_secret(&key_path).unwrap();
        assert_eq!(first.len(), 32);

        // An invalid (truncated) key is replaced with a fresh 32-byte key.
        fs::write(&key_path, b"short").unwrap();
        let cache = ComponentDiskCache::new(tmp.path());
        assert!(cache.is_enabled());
        let regenerated = read_secret(&key_path).unwrap();
        assert_eq!(regenerated.len(), 32);
        assert_ne!(regenerated, first);

        // The regenerated key still protects entries end-to-end.
        let engine = component_engine();
        let wasm = empty_component();
        cache.get_or_compile(&engine, &wasm).unwrap();
        let entry = tmp
            .path()
            .join(format!("{}.bin", ComponentDiskCache::cache_key(&wasm)));
        assert!(read_verified_entry(&regenerated, &entry).is_some());
    }

    #[cfg(unix)]
    #[test]
    fn group_or_world_writable_cache_dir_disables_cache() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        fs::set_permissions(tmp.path(), fs::Permissions::from_mode(0o777)).unwrap();

        let cache = ComponentDiskCache::new(tmp.path());
        assert!(!cache.is_enabled());

        // The unsafe directory is never written to; components still compile.
        let engine = component_engine();
        let wasm = empty_component();
        cache.get_or_compile(&engine, &wasm).unwrap();
        let entry = tmp
            .path()
            .join(format!("{}.bin", ComponentDiskCache::cache_key(&wasm)));
        assert!(!entry.exists());
        assert!(!tmp.path().join("key").exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_cache_dir_is_refused() {
        let base = tempfile::tempdir().unwrap();
        let real = base.path().join("real");
        fs::create_dir(&real).unwrap();
        let link = base.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let cache = ComponentDiskCache::new(&link);
        assert!(!cache.is_enabled());
        assert!(!real.join("key").exists());
    }
}

#[cfg(feature = "test-support")]
pub mod test_support;

impl SandboxLimits {
    /// Stable advisory keys exposed to guests. Unknown future keys are ignored.
    fn wire_limits(self) -> Vec<(String, u64)> {
        vec![
            ("ticks-per-second", animathio_core::TICKS_PER_SECOND as u64),
            ("max-memory-bytes", self.max_memory_bytes as u64),
            ("max-table-elements", self.max_table_elements as u64),
            ("fuel-per-call", self.fuel_per_call),
            ("epoch-interval-ms", EPOCH_TICK_INTERVAL.as_millis() as u64),
            ("epoch-deadline-ticks", self.epoch_deadline_ticks),
            ("max-effect-pixels", self.max_effect_pixels as u64),
            ("effect-memory-bytes", self.effect_memory_bytes as u64),
            ("effect-fuel-per-pixel", self.effect_fuel_per_pixel),
            (
                "effect-epoch-deadline-ticks",
                self.effect_epoch_deadline_ticks,
            ),
            ("max-audio-block-frames", self.max_audio_block_frames as u64),
            ("max-audio-state-bytes", self.max_audio_state_bytes as u64),
            ("audio-memory-bytes", self.audio_memory_bytes as u64),
            ("audio-fuel-per-sample", self.audio_fuel_per_sample),
            (
                "audio-epoch-deadline-ticks",
                self.audio_epoch_deadline_ticks,
            ),
            ("max-audio-channels", u64::from(audio::MAX_CHANNELS)),
            ("max-audio-sample-rate", u64::from(audio::MAX_SAMPLE_RATE)),
            ("max-shape-text-chars", MAX_SHAPE_TEXT_CHARS as u64),
            ("max-shape-text-verbs", MAX_SHAPE_TEXT_VERBS as u64),
            ("max-svg-input-bytes", MAX_SVG_INPUT_BYTES as u64),
            ("max-svg-verbs", MAX_SVG_VERBS as u64),
            ("max-typeset-verbs", MAX_TYPESET_VERBS as u64),
            (
                "max-typeset-source-bytes",
                animathio_typeset::MAX_SOURCE_BYTES as u64,
            ),
            (
                "max-typeset-font-size-px",
                animathio_typeset::MAX_FONT_SIZE as u64,
            ),
            (
                "typeset-timeout-ms",
                animathio_typeset::TYPESET_TIMEOUT.as_millis() as u64,
            ),
            ("max-log-message-bytes", MAX_LOG_MESSAGE_BYTES as u64),
            ("max-log-entries", MAX_LOG_ENTRIES as u64),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
    }
}

fn validate_kinds(manifest: &wit_types::Manifest, expected: &[&str]) -> Result<(), HostError> {
    let mut kinds = std::collections::BTreeSet::new();
    for kind in &manifest.kinds {
        if kind.trim().is_empty() || !kinds.insert(kind.as_str()) {
            return Err(HostError::InvalidOutput(
                "empty or duplicate describe kind".into(),
            ));
        }
    }
    // Open wire strings allow advisory classifications. Known execution kinds
    // must match exactly, and unknown strings do not enable new execution paths.
    let known = ["clip-source", "animation", "effect", "audio-effect"];
    if known
        .iter()
        .any(|kind| kinds.contains(kind) != expected.contains(kind))
    {
        return Err(HostError::InvalidOutput(
            "describe kinds disagree with plugin.toml".into(),
        ));
    }
    Ok(())
}

fn validate_manifest(
    package: &PluginManifestToml,
    manifest: &wit_types::Manifest,
) -> Result<(), HostError> {
    // 0.2.x components were accepted without schema or consistency checks. Keeping
    // them loadable means not retroactively rejecting what they describe; the
    // typed export checks and sandbox still apply.
    if package.api_version.starts_with("0.") {
        return Ok(());
    }
    validate_kinds(
        manifest,
        &package.kinds.iter().map(|k| k.as_str()).collect::<Vec<_>>(),
    )?;
    if manifest.version != package.version {
        return Err(HostError::InvalidOutput(
            "describe version disagrees with plugin.toml".into(),
        ));
    }
    let mut keys = std::collections::BTreeSet::new();
    if manifest
        .metadata
        .iter()
        .any(|(key, _)| key.is_empty() || !keys.insert(key))
    {
        return Err(HostError::InvalidOutput(
            "empty or duplicate metadata key".into(),
        ));
    }
    validate_params(&manifest.params)
}

fn validate_params(params: &[wit_types::ParamSpec]) -> Result<(), HostError> {
    let mut seen = std::collections::BTreeSet::new();
    for param in params {
        let valid = match &param.kind {
            wit_types::ParamKind::Number(r) => {
                [r.min, r.max, r.default, r.step]
                    .iter()
                    .all(|n| n.is_finite())
                    && r.min <= r.max
                    && (r.min..=r.max).contains(&r.default)
                    && r.step > 0.0
            }
            wit_types::ParamKind::EnumChoice(e) => {
                !e.options.is_empty()
                    && e.options.contains(&e.default)
                    && e.options.iter().all(|o| !o.is_empty())
                    && e.options
                        .iter()
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        == e.options.len()
            }
            wit_types::ParamKind::Point(p) => p.default.x.is_finite() && p.default.y.is_finite(),
            _ => true,
        };
        if param.id.is_empty() || param.label.trim().is_empty() || !seen.insert(&param.id) || !valid
        {
            return Err(HostError::InvalidOutput(format!(
                "invalid parameter schema {:?}",
                param.id
            )));
        }
    }
    Ok(())
}

fn diagnostic(code: &str, message: impl Into<String>) -> wit_types::Diagnostic {
    wit_types::Diagnostic {
        code: code.into(),
        message: message.into(),
        metadata: Vec::new(),
    }
}

impl Host for HostState {
    fn limits(&mut self) -> Vec<(String, u64)> {
        self.policy.wire_limits()
    }

    fn shape_text(
        &mut self,
        text: String,
        font_size: f32,
    ) -> Result<Vec<Vec<wit_types::PathVerb>>, wit_types::Diagnostic> {
        if !font_size.is_finite() || font_size <= 0.0 {
            return Err(diagnostic(
                "invalid-input",
                "font size must be finite and positive",
            ));
        }
        if text.chars().count() > MAX_SHAPE_TEXT_CHARS {
            return Err(diagnostic("limit-exceeded", "text exceeds character limit"));
        }
        let paths = shape_text_skrifa(&self.font_data, &text, font_size, usize::MAX);
        if paths.iter().map(Vec::len).sum::<usize>() > MAX_SHAPE_TEXT_VERBS {
            return Err(diagnostic("limit-exceeded", "text exceeds path verb limit"));
        }
        Ok(paths)
    }

    fn typeset_math(
        &mut self,
        source: String,
        mode: wit_types::MathMode,
        font_size: f32,
    ) -> Result<wit_types::TypesetResult, wit_types::Diagnostic> {
        if !font_size.is_finite() || font_size <= 0.0 {
            return Err(diagnostic(
                "invalid-input",
                "font size must be finite and positive",
            ));
        }
        if source.trim().is_empty() {
            return Err(diagnostic("invalid-input", "math source is empty"));
        }
        if source.len() > animathio_typeset::MAX_SOURCE_BYTES {
            return Err(diagnostic(
                "limit-exceeded",
                "math exceeds source byte limit",
            ));
        }
        if font_size > animathio_typeset::MAX_FONT_SIZE {
            return Err(diagnostic("limit-exceeded", "math exceeds font size limit"));
        }
        let mode = match mode {
            wit_types::MathMode::Latex => MathMode::Latex,
            wit_types::MathMode::Typst => MathMode::Typst,
        };
        let result = self
            .typeset
            .typeset(&source, mode, font_size)
            .map_err(|e| diagnostic("service-failed", e.to_string()))?;
        let result = typeset_to_wit(&result, usize::MAX);
        if result.paths.iter().map(Vec::len).sum::<usize>() > MAX_TYPESET_VERBS {
            return Err(diagnostic("limit-exceeded", "math exceeds path verb limit"));
        }
        Ok(result)
    }

    fn svg_to_paths(
        &mut self,
        svg_xml: String,
    ) -> Result<Vec<Vec<wit_types::PathVerb>>, wit_types::Diagnostic> {
        if svg_xml.len() > MAX_SVG_INPUT_BYTES {
            return Err(diagnostic("limit-exceeded", "SVG exceeds input byte limit"));
        }
        let paths = parse_svg_to_paths(&svg_xml, usize::MAX);
        if paths.iter().map(Vec::len).sum::<usize>() > MAX_SVG_VERBS {
            return Err(diagnostic("limit-exceeded", "SVG exceeds path verb limit"));
        }
        if paths.is_empty() && !svg_xml.trim().is_empty() {
            return Err(diagnostic(
                "invalid-input",
                "SVG contains no supported paths",
            ));
        }
        Ok(paths)
    }

    fn log(&mut self, message: String) {
        self.legacy_log(message);
    }
}

#[cfg(test)]
mod abi_tests {
    use super::*;

    fn manifest() -> wit_types::Manifest {
        wit_types::Manifest {
            name: "test".into(),
            version: "1.0.0".into(),
            description: String::new(),
            kinds: vec!["audio-effect".into(), "com.example.experimental".into()],
            metadata: Vec::new(),
            params: Vec::new(),
        }
    }

    #[test]
    fn open_wire_kinds_do_not_enable_unknown_execution_worlds() {
        let mut m = manifest();
        assert!(validate_kinds(&m, &["audio-effect"]).is_ok());
        assert!(validate_kinds(&m, &["effect"]).is_err());
        m.kinds.push("effect".into());
        assert!(validate_kinds(&m, &["audio-effect"]).is_err());
        m.kinds = vec!["audio-effect".into(), "audio-effect".into()];
        assert!(validate_kinds(&m, &["audio-effect"]).is_err());
    }

    #[test]
    fn malformed_parameter_schemas_are_rejected() {
        let mut spec = wit_types::ParamSpec {
            id: "amount".into(),
            label: "Amount".into(),
            kind: wit_types::ParamKind::Number(wit_types::NumberRange {
                min: 0.0,
                max: 1.0,
                default: 0.5,
                step: 0.1,
            }),
        };
        assert!(validate_params(&[spec.clone()]).is_ok());
        assert!(validate_params(&[spec.clone(), spec.clone()]).is_err());
        for step in [0.0, -1.0, f64::NAN] {
            let wit_types::ParamKind::Number(r) = &mut spec.kind else {
                unreachable!()
            };
            r.step = step;
            assert!(validate_params(&[spec.clone()]).is_err());
        }
        spec.kind = wit_types::ParamKind::EnumChoice(wit_types::EnumParam {
            options: vec!["a".into()],
            default: "b".into(),
        });
        assert!(validate_params(&[spec.clone()]).is_err());
        spec.kind = wit_types::ParamKind::Point(wit_types::PointParam {
            default: wit_types::Point {
                x: f32::NAN,
                y: 0.0,
            },
        });
        assert!(validate_params(&[spec]).is_err());
    }

    #[test]
    fn v1_services_report_errors_while_legacy_keeps_empty_results() {
        let cache = tempfile::tempdir().unwrap();
        let limits = SandboxLimits {
            max_audio_block_frames: 123,
            ..SandboxLimits::default()
        };
        let host = PluginHost::with_limits(cache.path(), limits).unwrap();
        let mut store = host.create_store(CapabilityGrants::default()).unwrap();
        let state = store.data_mut();
        assert!(Host::limits(state).contains(&("max-audio-block-frames".into(), 123)));
        let text = "a".repeat(MAX_SHAPE_TEXT_CHARS + 1);
        assert_eq!(
            Host::shape_text(state, text.clone(), 12.0)
                .unwrap_err()
                .code,
            "limit-exceeded"
        );
        assert!(state.legacy_shape_text(text, 12.0).is_empty());
        assert_eq!(
            Host::shape_text(state, "x".into(), f32::NAN)
                .unwrap_err()
                .code,
            "invalid-input"
        );
        assert_eq!(
            Host::svg_to_paths(state, "garbage".into())
                .unwrap_err()
                .code,
            "invalid-input"
        );
        assert!(state.legacy_svg_to_paths("garbage".into()).is_empty());
        assert!(
            Host::shape_text(state, String::new(), 12.0)
                .unwrap()
                .is_empty()
        );
    }
}
