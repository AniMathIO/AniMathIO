//! Importer plugins (API 1.1): registry, sandboxed import, bounded sibling reads and
//! validated replay of the returned command document.
//!
//! An importer implements the `importer-plugin` world: `describe`, plus the `importer`
//! interface (`list-importers`, `run-import`). The host hands it the source bytes and,
//! only when the caller grants a directory, bounded reads of files next to the source
//! through the `import-files` host import. The plugin answers with a JSON command
//! document; the host replays every command on an empty project through the normal
//! validating `Command::apply`, checks that each returned asset's id is the BLAKE3 hash
//! of its bytes and is registered, and only then hands the project to the caller.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use animathio_core::{AssetId, Command, FrameRate, ParamValue, Project};
use serde::Deserialize;
use wasmtime::component::{Component as WasmComponent, Linker};
use wasmtime::{Store, StoreLimitsBuilder};
use wasmtime_wasi::WasiCtxBuilder;

use crate::compat::Pre;
use crate::importer_world::animathio::plugin::import_files;
use crate::importer_world::exports::animathio::plugin::importer as wit;
use crate::{HostError, HostState, wit_types};

/// Most formats one importer plugin may list.
const MAX_IMPORTERS: usize = 32;
/// Most messages kept from one import.
const MAX_MESSAGES: usize = 10_000;
/// Smallest memory an importer call gets, whatever `import_memory_bytes` says.
const IMPORT_BASE_MEMORY: usize = 64 * 1024 * 1024;

/// A format offered by a loaded importer plugin.
#[derive(Clone, Debug, PartialEq)]
pub struct ImporterDescriptor {
    /// Providing plugin id, e.g. `org.animathio.import-manim`.
    pub plugin_id: String,
    /// Format id within the plugin, e.g. `manim`.
    pub importer_id: String,
    /// Menu name.
    pub name: String,
    /// One-line description.
    pub description: String,
    /// Lower-case file extensions without the dot.
    pub extensions: Vec<String>,
    /// Ids of the string/number/enum/bool options the importer accepts.
    pub params: Vec<String>,
}

/// One import invocation.
#[derive(Clone, Debug, Default)]
pub struct ImportRequest {
    /// File name hint without directories.
    pub file_name: String,
    /// Raw source bytes.
    pub bytes: Vec<u8>,
    /// Directory the importer may read sibling files from; `None` grants nothing.
    pub sibling_dir: Option<PathBuf>,
    /// Importer options by id.
    pub params: BTreeMap<String, ParamValue>,
}

/// Severity of an [`ImportMessage`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportSeverity {
    /// Informational note (e.g. an embedded file).
    Info,
    /// Something was approximated or skipped.
    Warning,
}

/// One non-fatal observation from an importer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportMessage {
    /// Severity.
    pub severity: ImportSeverity,
    /// One-based line, 0 when unknown.
    pub line: u32,
    /// One-based column, 0 when unknown.
    pub column: u32,
    /// Human-readable description.
    pub message: String,
    /// Offending source snippet, may be empty.
    pub snippet: String,
}

impl std::fmt::Display for ImportMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.line == 0 {
            f.write_str(&self.message)
        } else {
            write!(
                f,
                "{}:{}: {} ({})",
                self.line, self.column, self.message, self.snippet
            )
        }
    }
}

/// One asset of an imported project, with verified id.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportedAsset {
    /// BLAKE3 content hash, registered in the project.
    pub id: AssetId,
    /// Filename hint.
    pub name: String,
    /// Declared media type.
    pub media_type: String,
    /// Raw bytes.
    pub bytes: Vec<u8>,
}

/// A validated import result.
#[derive(Clone, Debug)]
pub struct ImportedProject {
    /// The rebuilt project.
    pub project: Project,
    /// Asset bytes for every returned asset.
    pub assets: Vec<ImportedAsset>,
    /// Non-fatal messages in import order.
    pub messages: Vec<ImportMessage>,
}

impl ImportedProject {
    /// Warning texts, formatted like the native importers' reports.
    #[must_use]
    pub fn warnings(&self) -> Vec<String> {
        self.messages_of(ImportSeverity::Warning)
    }

    /// Informational texts.
    #[must_use]
    pub fn infos(&self) -> Vec<String> {
        self.messages_of(ImportSeverity::Info)
    }

    fn messages_of(&self, severity: ImportSeverity) -> Vec<String> {
        self.messages
            .iter()
            .filter(|message| message.severity == severity)
            .map(ToString::to_string)
            .collect()
    }
}

/// Directory and remaining budget for `read-sibling` during one import call.
pub(crate) struct SiblingGrant {
    dir: Option<PathBuf>,
    max_file: u64,
    remaining_bytes: u64,
    remaining_files: usize,
}

pub(crate) struct ImporterPlugin {
    pre: Pre,
    importers: Vec<ImporterDescriptor>,
    params: BTreeMap<String, Vec<wit_types::ParamSpec>>,
}

type RunImport = wasmtime::component::TypedFunc<
    (String, wit::ImportSource, Vec<wit_types::ParamEntry>),
    (Result<wit::ImportOutput, wit_types::Diagnostic>,),
>;

fn run_import_func(
    instance: &crate::compat::Bindings,
    store: &mut Store<HostState>,
) -> wasmtime::Result<RunImport> {
    let export = instance.export(store, "importer", "run-import")?;
    instance.instance.get_typed_func(store, export)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

impl ImporterPlugin {
    pub(crate) fn register(
        linker: &Linker<HostState>,
        package: &animathio_plugin_api::PluginManifestToml,
        component: &WasmComponent,
        store: &mut Store<HostState>,
    ) -> Result<(Self, wit_types::Manifest), String> {
        let pre = Pre::new(
            linker
                .instantiate_pre(component)
                .map_err(|e| e.to_string())?,
            false,
        );
        let instance = pre.instantiate(&mut *store).map_err(|e| e.to_string())?;
        let manifest = instance
            .describe(&mut *store, false)
            .map_err(|e| e.to_string())?;
        crate::validate_manifest(package, &manifest).map_err(|e| e.to_string())?;
        run_import_func(&instance, store).map_err(|e| e.to_string())?;
        let list = instance
            .export(store, "importer", "list-importers")
            .map_err(|e| e.to_string())?;
        let (listed,) = instance
            .instance
            .get_typed_func::<(), (Vec<wit::ImporterDescriptor>,)>(&mut *store, &list)
            .and_then(|func| func.call(&mut *store, ()))
            .map_err(|e| e.to_string())?;
        if listed.len() > MAX_IMPORTERS {
            return Err(format!("too many importers (maximum {MAX_IMPORTERS})"));
        }
        let mut importers = Vec::new();
        let mut params = BTreeMap::new();
        for descriptor in listed {
            let id = descriptor.id;
            if !valid_id(&id) || params.contains_key(&id) {
                return Err(format!("invalid or duplicate importer id {id:?}"));
            }
            if descriptor.name.trim().is_empty() {
                return Err(format!("importer {id:?} has no name"));
            }
            if descriptor.extensions.is_empty()
                || descriptor.extensions.len() > 16
                || !descriptor.extensions.iter().all(|ext| {
                    !ext.is_empty()
                        && ext.len() <= 16
                        && ext
                            .bytes()
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
                })
            {
                return Err(format!("importer {id:?} has invalid extensions"));
            }
            if descriptor.params.len() > 16 {
                return Err(format!("importer {id:?} declares too many params"));
            }
            crate::validate_params(&descriptor.params)
                .map_err(|e| format!("importer {id:?}: {e}"))?;
            if descriptor.params.iter().any(|param| {
                !matches!(
                    param.kind,
                    wit_types::ParamKind::String(_)
                        | wit_types::ParamKind::Number(_)
                        | wit_types::ParamKind::EnumChoice(_)
                        | wit_types::ParamKind::Bool(_)
                )
            }) {
                return Err(format!(
                    "importer {id:?}: only string, number, enum-choice and bool params are supported"
                ));
            }
            importers.push(ImporterDescriptor {
                plugin_id: package.id.clone(),
                importer_id: id.clone(),
                name: crate::truncate_utf8(&descriptor.name, 256),
                description: crate::truncate_utf8(&descriptor.description, 512),
                extensions: descriptor.extensions,
                params: descriptor.params.iter().map(|p| p.id.clone()).collect(),
            });
            params.insert(id, descriptor.params);
        }
        Ok((
            Self {
                pre,
                importers,
                params,
            },
            manifest,
        ))
    }
}

fn import_params(
    schema: &[wit_types::ParamSpec],
    values: &BTreeMap<String, ParamValue>,
) -> Result<Vec<wit_types::ParamEntry>, String> {
    if let Some(unknown) = values
        .keys()
        .find(|key| !schema.iter().any(|param| &param.id == *key))
    {
        return Err(format!("importer has no option {unknown:?}"));
    }
    schema
        .iter()
        .map(|spec| {
            let value = match (&spec.kind, values.get(&spec.id)) {
                (wit_types::ParamKind::String(s), None) => {
                    wit_types::ParamValue::String(s.default.clone())
                }
                (wit_types::ParamKind::String(_), Some(ParamValue::String(text))) => {
                    if text.len() > 4096 {
                        return Err(format!("option {} is too long", spec.id));
                    }
                    wit_types::ParamValue::String(text.clone())
                }
                (wit_types::ParamKind::Number(range), None) => {
                    wit_types::ParamValue::Number(range.default)
                }
                (wit_types::ParamKind::Number(range), Some(ParamValue::Number(n)))
                    if n.is_finite() && (range.min..=range.max).contains(n) =>
                {
                    wit_types::ParamValue::Number(*n)
                }
                (wit_types::ParamKind::EnumChoice(choice), None) => {
                    wit_types::ParamValue::EnumChoice(choice.default.clone())
                }
                (
                    wit_types::ParamKind::EnumChoice(choice),
                    Some(ParamValue::Enum(value) | ParamValue::String(value)),
                ) if choice.options.contains(value) => {
                    wit_types::ParamValue::EnumChoice(value.clone())
                }
                (wit_types::ParamKind::Bool(toggle), None) => {
                    wit_types::ParamValue::Bool(toggle.default)
                }
                (wit_types::ParamKind::Bool(_), Some(ParamValue::Bool(b))) => {
                    wit_types::ParamValue::Bool(*b)
                }
                _ => return Err(format!("option {} has an invalid value", spec.id)),
            };
            Ok(wit_types::ParamEntry {
                id: spec.id.clone(),
                value,
            })
        })
        .collect()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandDocument {
    version: u32,
    commands: Vec<Command>,
}

/// Replay a version-1 command document on an empty project, validating every step,
/// and verify the returned assets against it.
fn replay(
    output: wit::ImportOutput,
    max_bytes: usize,
    max_commands: usize,
) -> Result<ImportedProject, String> {
    if output.commands.len() > max_bytes {
        return Err(format!(
            "command document is {} bytes (maximum {max_bytes})",
            output.commands.len()
        ));
    }
    let document: CommandDocument = serde_json::from_str(&output.commands)
        .map_err(|e| format!("invalid command document: {e}"))?;
    if document.version != 1 {
        return Err(format!(
            "unsupported command document version {}",
            document.version
        ));
    }
    if document.commands.len() > max_commands {
        return Err(format!(
            "{} commands (maximum {max_commands})",
            document.commands.len()
        ));
    }
    let mut project = Project::new(FrameRate::default());
    for (index, command) in document.commands.iter().enumerate() {
        command
            .apply(&mut project)
            .map_err(|e| format!("command {index}: {e}"))?;
    }
    let mut seen = BTreeSet::new();
    let mut assets = Vec::with_capacity(output.assets.len());
    for asset in output.assets {
        let id = AssetId::of(&asset.bytes);
        if id.as_str() != asset.id {
            return Err(format!("asset {} does not match its bytes", asset.id));
        }
        if !project.assets().contains_key(&id) {
            return Err(format!(
                "asset {} is not registered by any command",
                asset.id
            ));
        }
        if !seen.insert(id.clone()) {
            continue;
        }
        assets.push(ImportedAsset {
            id,
            name: crate::truncate_utf8(&asset.name, 256),
            media_type: crate::truncate_utf8(&asset.media_type, 256),
            bytes: asset.bytes,
        });
    }
    if let Some(missing) = project.assets().keys().find(|id| !seen.contains(*id)) {
        return Err(format!("asset {} has no bytes", missing.as_str()));
    }
    let messages = output
        .messages
        .into_iter()
        .take(MAX_MESSAGES)
        .map(|message| ImportMessage {
            severity: match message.severity {
                wit::MessageSeverity::Info => ImportSeverity::Info,
                wit::MessageSeverity::Warning => ImportSeverity::Warning,
            },
            line: message.line,
            column: message.column,
            message: crate::truncate_utf8(&message.message, 1024),
            snippet: crate::truncate_utf8(&message.snippet, 256),
        })
        .collect();
    Ok(ImportedProject {
        project,
        assets,
        messages,
    })
}

/// Validate a guest-supplied relative path: `/`-separated normal components only.
fn sibling_path(relative: &str) -> Result<PathBuf, &'static str> {
    if relative.is_empty() || relative.len() > 4096 || relative.contains(['\\', '\0']) {
        return Err("invalid relative path");
    }
    let path = Path::new(relative);
    if path.has_root() {
        return Err("absolute paths are not allowed");
    }
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            _ => return Err("`..` and prefix components are not allowed"),
        }
    }
    if clean.as_os_str().is_empty() {
        return Err("invalid relative path");
    }
    Ok(clean)
}

fn diag(code: &str, message: impl Into<String>) -> wit_types::Diagnostic {
    crate::diagnostic(code, message)
}

impl import_files::Host for HostState {
    fn read_sibling(&mut self, relative_path: String) -> Result<Vec<u8>, wit_types::Diagnostic> {
        let Some(grant) = self.import_files.as_mut() else {
            return Err(diag("invalid-input", "no import is running"));
        };
        let Some(dir) = grant.dir.clone() else {
            return Err(diag(
                "invalid-input",
                "no sibling directory was granted for this import",
            ));
        };
        if grant.remaining_files == 0 {
            return Err(diag("limit-exceeded", "too many sibling files"));
        }
        grant.remaining_files -= 1;
        let relative = sibling_path(&relative_path).map_err(|e| diag("invalid-input", e))?;
        // Canonicalize so a symlink pointing outside the directory is refused.
        let candidate = dir
            .join(&relative)
            .canonicalize()
            .map_err(|e| diag("service-failed", format!("not readable: {e}")))?;
        if !candidate.starts_with(&dir) {
            return Err(diag(
                "invalid-input",
                "path resolves outside the granted directory",
            ));
        }
        let metadata = candidate
            .metadata()
            .map_err(|e| diag("service-failed", e.to_string()))?;
        if !metadata.is_file() {
            return Err(diag("invalid-input", "not a regular file"));
        }
        let limit = grant.max_file.min(grant.remaining_bytes);
        if metadata.len() > limit {
            return Err(diag(
                "limit-exceeded",
                "sibling file exceeds the byte limit",
            ));
        }
        let bytes = read_capped(&candidate, limit)
            .map_err(|e| diag("service-failed", e.to_string()))?
            .ok_or_else(|| diag("limit-exceeded", "sibling file exceeds the byte limit"))?;
        grant.remaining_bytes -= bytes.len() as u64;
        Ok(bytes)
    }
}

/// Read at most `limit` bytes; `None` when the file grew past the limit meanwhile.
fn read_capped(path: &Path, limit: u64) -> std::io::Result<Option<Vec<u8>>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    Ok((bytes.len() as u64 <= limit).then_some(bytes))
}

impl crate::PluginHost {
    /// Formats offered by every loaded importer plugin, in plugin-id then listing order.
    #[must_use]
    pub fn importers(&self) -> Vec<ImporterDescriptor> {
        self.importers
            .values()
            .flat_map(|plugin| plugin.importers.iter().cloned())
            .collect()
    }

    /// Run importer `importer_id` of `plugin_id` on `request` under the importer
    /// sandbox budget and return the validated project.
    pub fn import(
        &self,
        plugin_id: &str,
        importer_id: &str,
        request: ImportRequest,
    ) -> Result<ImportedProject, HostError> {
        let plugin = self
            .importers
            .get(plugin_id)
            .ok_or_else(|| HostError::NotFound(plugin_id.to_owned()))?;
        let schema = plugin
            .params
            .get(importer_id)
            .ok_or_else(|| HostError::NotFound(format!("{plugin_id}/{importer_id}")))?;
        let limits = self.limits;
        if request.bytes.len() > limits.max_import_source_bytes {
            return Err(HostError::InvalidOutput(format!(
                "import source is {} bytes (maximum {})",
                request.bytes.len(),
                limits.max_import_source_bytes
            )));
        }
        let params = import_params(schema, &request.params).map_err(HostError::InvalidOutput)?;
        let dir = request
            .sibling_dir
            .as_deref()
            .map(Path::canonicalize)
            .transpose()?;

        // Sibling files, decoded assets and the output all live in the guest, so the
        // whole (lazily committed) importer budget is available to every call.
        let memory = limits.import_memory_bytes.max(IMPORT_BASE_MEMORY);
        let limiter = StoreLimitsBuilder::new()
            .memory_size(memory)
            .table_elements(limits.max_table_elements)
            .instances(8)
            .memories(1)
            .tables(2)
            .trap_on_grow_failure(true)
            .build();
        let fuel = limits.fuel_per_call.saturating_mul(100).saturating_add(
            limits
                .import_fuel_per_byte
                .saturating_mul(request.bytes.len() as u64),
        );
        let mut store = crate::build_store(
            &self.engine,
            WasiCtxBuilder::new().build(),
            self.font_data.clone(),
            std::sync::Arc::clone(&self.typeset),
            limiter,
            (fuel, limits.import_epoch_deadline_ticks),
            limits,
        );
        store.data_mut().import_files = Some(SiblingGrant {
            dir,
            max_file: limits.max_sibling_file_bytes,
            remaining_bytes: limits.max_sibling_total_bytes,
            remaining_files: limits.max_sibling_files,
        });
        let instance = plugin
            .pre
            .instantiate(&mut store)
            .map_err(|e| HostError::Trapped(e.to_string()))?;
        let func = run_import_func(&instance, &mut store)
            .map_err(|e| HostError::Trapped(e.to_string()))?;
        let source = wit::ImportSource {
            file_name: crate::truncate_utf8(&request.file_name, 256),
            bytes: request.bytes,
        };
        let (result,) = func
            .call(&mut store, (importer_id.to_owned(), source, params))
            .map_err(|e| HostError::Trapped(e.to_string()))?;
        let output =
            result.map_err(|e| HostError::InvalidOutput(format!("{}: {}", e.code, e.message)))?;
        replay(
            output,
            limits.max_import_output_bytes,
            limits.max_import_commands,
        )
        .map_err(|e| HostError::InvalidOutput(format!("import {plugin_id}/{importer_id}: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use animathio_core::{Asset, Clip, ClipId, ClipKind, Time, TimeRange, Track, TrackId};

    fn document(commands: &[Command]) -> String {
        serde_json::json!({ "version": 1, "commands": commands }).to_string()
    }

    fn output(commands: String, assets: Vec<wit::ImportedAsset>) -> wit::ImportOutput {
        wit::ImportOutput {
            commands,
            assets,
            messages: vec![wit::ImportMessage {
                severity: wit::MessageSeverity::Warning,
                line: 3,
                column: 1,
                message: "approximated".into(),
                snippet: "x".into(),
            }],
        }
    }

    fn image_project() -> (Vec<Command>, wit::ImportedAsset) {
        let bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        let id = AssetId::of(&bytes);
        let commands = vec![
            Command::AddAsset {
                id: id.clone(),
                asset: Asset {
                    name: "a.png".into(),
                    media_type: "image/png".into(),
                },
            },
            Command::AddTrack {
                track: Track::new(TrackId(1), "V1"),
                index: 0,
            },
            Command::AddClip {
                track: TrackId(1),
                clip: Clip::new(
                    ClipId(1),
                    ClipKind::Image(id.clone()),
                    TimeRange::new(Time(0), Time(10)).unwrap(),
                ),
            },
        ];
        let asset = wit::ImportedAsset {
            id: id.as_str().to_owned(),
            name: "a.png".into(),
            media_type: "image/png".into(),
            bytes,
        };
        (commands, asset)
    }

    #[test]
    fn valid_documents_replay_and_assets_are_verified() {
        let (commands, asset) = image_project();
        let imported = replay(
            output(document(&commands), vec![asset.clone()]),
            1 << 20,
            100,
        )
        .unwrap();
        assert_eq!(imported.project.tracks().len(), 1);
        assert_eq!(imported.assets.len(), 1);
        assert_eq!(imported.warnings(), ["3:1: approximated (x)"]);

        let mut forged = asset.clone();
        forged.bytes.push(0);
        assert!(replay(output(document(&commands), vec![forged]), 1 << 20, 100).is_err());
        assert!(replay(output(document(&commands), vec![]), 1 << 20, 100).is_err());
        assert!(replay(output(document(&commands[1..]), vec![asset]), 1 << 20, 100).is_err());
    }

    #[test]
    fn invalid_documents_are_rejected() {
        let (commands, _) = image_project();
        let bad_clip = vec![commands[1].clone(), commands[2].clone()];
        for doc in [
            "not json".to_owned(),
            r#"{"version": 2, "commands": []}"#.to_owned(),
            r#"{"version": 1, "commands": [], "extra": 1}"#.to_owned(),
            document(&bad_clip),
        ] {
            assert!(replay(output(doc, vec![]), 1 << 20, 100).is_err());
        }
        let many = vec![commands[1].clone(); 2];
        assert!(replay(output(document(&many), vec![]), 1 << 20, 1).is_err());
        assert!(replay(output(document(&[]), vec![]), 4, 100).is_err());
    }

    #[test]
    fn sibling_paths_stay_relative() {
        assert!(sibling_path("images/a.png").is_ok());
        assert!(sibling_path("./a.png").is_ok());
        for bad in [
            "",
            "/etc/passwd",
            "../a.png",
            "a/../../b",
            "a\\b",
            ".",
            "a\0",
        ] {
            assert!(sibling_path(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn sibling_reads_are_confined_and_budgeted() {
        use import_files::Host as _;
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.png"), b"12345").unwrap();
        std::fs::write(dir.path().join("big.bin"), vec![0u8; 64]).unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret"), b"s").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.path().join("secret"), dir.path().join("link")).unwrap();
        let cache = tempfile::tempdir().unwrap();
        let host = crate::PluginHost::new(cache.path()).unwrap();
        let mut store = host
            .create_store(crate::CapabilityGrants::default())
            .unwrap();
        let state = store.data_mut();
        assert!(
            state.read_sibling("a.png".into()).is_err(),
            "no import running"
        );
        state.import_files = Some(SiblingGrant {
            dir: Some(dir.path().canonicalize().unwrap()),
            max_file: 16,
            remaining_bytes: 8,
            remaining_files: 4,
        });
        assert_eq!(state.read_sibling("a.png".into()).unwrap(), b"12345");
        assert_eq!(
            state.read_sibling("big.bin".into()).unwrap_err().code,
            "limit-exceeded"
        );
        assert!(state.read_sibling("../secret".into()).is_err());
        #[cfg(unix)]
        assert!(state.read_sibling("link".into()).is_err());
        // Either the byte budget (3 left) or the file budget is exhausted now.
        assert!(state.read_sibling("a.png".into()).is_err());
        assert_eq!(
            state.read_sibling("a.png".into()).unwrap_err().code,
            "limit-exceeded",
            "file budget exhausted"
        );
        state.import_files = Some(SiblingGrant {
            dir: None,
            max_file: 16,
            remaining_bytes: 16,
            remaining_files: 4,
        });
        assert!(state.read_sibling("a.png".into()).is_err());
    }
}
