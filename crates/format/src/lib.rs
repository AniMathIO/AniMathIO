//! Versioned project JSON and content-addressed `.amio` ZIP containers.
//! Reader/writer APIs leave filesystem policy (atomic replacement, autosave) to callers.
use animathio_core::{AssetId, Project};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    io::{Read, Seek, Write},
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

/// Current project schema. Future migrations advance one version at a time.
pub const SCHEMA_VERSION: u32 = 2;

/// Persistence errors, including malformed or unsupported project data.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Reader/writer failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// ZIP container failure.
    #[error(transparent)]
    Zip(#[from] zip::result::ZipError),
    /// JSON syntax or data-model error.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Domain validation failure.
    #[error(transparent)]
    Model(#[from] animathio_core::Error),
    /// Unsupported schema or invalid archive structure.
    #[error("{0}")]
    Invalid(String),
}
fn invalid(message: &str) -> Error {
    Error::Invalid(message.to_owned())
}

#[derive(Serialize)]
struct Document {
    format: String,
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    #[serde(flatten)]
    project: Project,
}

/// Resource limits for in-memory loading. Applications may adjust for their media corpus.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Maximum uncompressed project.json bytes.
    pub project_bytes: u64,
    /// Maximum uncompressed bytes per asset.
    pub asset_bytes: u64,
    /// Maximum cumulative uncompressed archive bytes.
    pub total_bytes: u64,
    /// Maximum number of distinct indexed ZIP entry names.
    pub entries: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            project_bytes: 16 * 1024 * 1024,
            asset_bytes: 1024 * 1024 * 1024,
            total_bytes: 4 * 1024 * 1024 * 1024,
            entries: 10_000,
        }
    }
}

/// Write validated human-readable project.json, including format and schemaVersion.
pub fn write_json(writer: impl Write, project: &Project) -> Result<(), Error> {
    project.validate()?;
    let document = Document {
        format: "animathio".to_owned(),
        schema_version: SCHEMA_VERSION,
        project: project.clone(),
    };
    serde_json::to_writer_pretty(writer, &document)?;
    Ok(())
}

/// Read the current schema, migrating the known historic schema (v1) first.
pub fn read_json(reader: impl Read) -> Result<Project, Error> {
    read_json_with_migration(reader, migrate)
}

/// Migration hook: transform version N JSON into version N+1 JSON, updating
/// schemaVersion. Migrations run before domain deserialization and validation.
/// Future versions are rejected rather than silently losing unknown data.
pub fn read_json_with_migration(
    reader: impl Read,
    step: impl FnMut(u32, Value) -> Result<Value, Error>,
) -> Result<Project, Error> {
    let bytes = read_limited(reader, Limits::default().project_bytes)?;
    decode_json(serde_json::from_slice(&bytes)?, step)
}

fn decode_json(
    mut value: Value,
    mut step: impl FnMut(u32, Value) -> Result<Value, Error>,
) -> Result<Project, Error> {
    loop {
        let version = value
            .get("schemaVersion")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| invalid("missing or invalid schemaVersion"))?;
        if version > SCHEMA_VERSION {
            return Err(invalid("project schema is newer than supported"));
        }
        if version == SCHEMA_VERSION {
            break;
        }
        value = step(version, value)?;
        if value.get("schemaVersion").and_then(Value::as_u64) != Some(u64::from(version) + 1) {
            return Err(invalid("migration must advance exactly one schema version"));
        }
    }
    if value.get("format").and_then(Value::as_str) != Some("animathio") {
        return Err(invalid("unrecognized project format"));
    }
    // Deserialize the project directly. serde's flattened buffering would lose
    // JSON's numeric-map-key adapter for ClipId keys inside Track::clips.
    Ok(serde_json::from_value(value)?)
}

/// Built-in schema migrations. v1 introduced the first production schema (a
/// top-level `frameRate` and no project settings); v2 moved the frame rate into
/// `ProjectSettings` and added output dimensions and a background colour.
fn migrate(version: u32, mut value: Value) -> Result<Value, Error> {
    match version {
        1 => {
            let object = value
                .as_object_mut()
                .ok_or_else(|| invalid("project must be an object"))?;
            let frame_rate = object
                .remove("frame_rate")
                .unwrap_or_else(|| serde_json::json!({ "numerator": 30, "denominator": 1 }));
            object.insert(
                "settings".to_owned(),
                serde_json::json!({
                    "width": 1920,
                    "height": 1080,
                    "frame_rate": frame_rate,
                    "background": [0, 0, 0, 255],
                }),
            );
            object.insert("schemaVersion".to_owned(), serde_json::json!(2));
            Ok(value)
        }
        other => Err(invalid(&format!(
            "no migration registered for schema {other}"
        ))),
    }
}

/// Fully loaded archive. Runtime media resources are created elsewhere.
#[derive(Clone, Debug, PartialEq)]
pub struct Bundle {
    /// Validated project model.
    pub project: Project,
    /// Deduplicated content bytes, keyed by verified BLAKE3 hash.
    pub assets: BTreeMap<AssetId, Vec<u8>>,
}

/// Write project.json and assets/<blake3>. Assets must exactly match project metadata.
/// No filesystem paths are extracted or followed by the container API.
pub fn write_amio<W: Write + Seek>(
    writer: W,
    project: &Project,
    assets: &BTreeMap<AssetId, Vec<u8>>,
) -> Result<W, Error> {
    project.validate()?;
    check_assets(project, assets)?;
    let mut archive = ZipWriter::new(writer);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    archive.start_file("project.json", options)?;
    write_json(&mut archive, project)?;
    for (id, bytes) in assets {
        archive.start_file(format!("assets/{}", id.as_str()), options)?;
        archive.write_all(bytes)?;
    }
    Ok(archive.finish()?)
}

/// Load an archive with conservative, configurable in-memory resource limits.
/// Unknown entries, missing media and hash mismatches are rejected.
/// Duplicate ZIP names follow the ZIP library's last-entry-wins semantics.
pub fn read_amio<R: Read + Seek>(reader: R, limits: Limits) -> Result<Bundle, Error> {
    let mut archive = ZipArchive::new(reader)?;
    if archive.len() > limits.entries {
        return Err(invalid("archive entry limit exceeded"));
    }
    let mut project_json = None;
    let mut assets = BTreeMap::new();
    let mut total = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let name = entry.name().to_owned();
        let limit = if name == "project.json" {
            limits.project_bytes
        } else {
            limits.asset_bytes
        };
        let remaining = limits.total_bytes - total;
        if entry.size() > limit || entry.size() > remaining {
            return Err(invalid("archive byte limit exceeded"));
        }
        let bytes = read_limited(&mut entry, limit.min(remaining))?;
        total += u64::try_from(bytes.len()).map_err(|_| invalid("archive size overflow"))?;
        if name == "project.json" {
            if project_json.replace(bytes).is_some() {
                return Err(invalid("duplicate project.json"));
            }
        } else if let Some(hash) = name.strip_prefix("assets/") {
            let id = AssetId::try_from(hash.to_owned())?;
            if AssetId::of(&bytes) != id {
                return Err(invalid("asset hash mismatch"));
            }
            if assets.insert(id, bytes).is_some() {
                return Err(invalid("duplicate asset"));
            }
        } else {
            return Err(invalid("unknown archive entry"));
        }
    }
    let json = project_json.ok_or_else(|| invalid("project.json missing"))?;
    let project = decode_json(serde_json::from_slice(&json)?, migrate)?;
    check_assets(&project, &assets)?;
    Ok(Bundle { project, assets })
}

fn check_assets(project: &Project, assets: &BTreeMap<AssetId, Vec<u8>>) -> Result<(), Error> {
    if !project.assets().keys().eq(assets.keys()) {
        return Err(invalid("asset manifest does not match container"));
    }
    for (id, bytes) in assets {
        if AssetId::of(bytes) != *id {
            return Err(invalid("asset hash mismatch"));
        }
    }
    Ok(())
}
fn read_limited(reader: impl Read, limit: u64) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(invalid("uncompressed byte limit exceeded"));
    }
    Ok(bytes)
}
