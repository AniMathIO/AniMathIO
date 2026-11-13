//! Versioned project JSON and content-addressed `.amio` ZIP containers.
//! Stream APIs support custom storage; save helpers atomically replace filesystem targets.
mod legacy;

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

/// Read current or schema-1 projects, migrating legacy clip sources automatically.
pub fn read_json(reader: impl Read) -> Result<Project, Error> {
    read_json_with_migration(reader, |_, _| {
        Err(invalid("no migration registered for this schema"))
    })
}

/// Additional migration hook for versions without a built-in migration.
/// Transform version N JSON into N+1 JSON, updating schemaVersion. The built-in
/// schema-1 migration always runs automatically. All migration steps precede
/// domain deserialization and validation; future versions are rejected.
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
        value = if version == 1 {
            legacy::migrate(value)?
        } else {
            step(version, value)?
        };
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

/// Atomically replace a project only after validation and a durable temporary write.
pub fn save_amio(
    path: impl AsRef<std::path::Path>,
    project: &Project,
    assets: &BTreeMap<AssetId, Vec<u8>>,
) -> Result<(), Error> {
    project.validate()?;
    check_assets(project, assets)?;
    atomic_write(path.as_ref(), |file| {
        write_amio(file, project, assets)?;
        Ok(())
    })
}

/// Atomically replace a JSON project.
pub fn save_json(path: impl AsRef<std::path::Path>, project: &Project) -> Result<(), Error> {
    project.validate()?;
    atomic_write(path.as_ref(), |file| write_json(file, project))
}

fn atomic_write(
    path: &std::path::Path,
    write: impl FnOnce(&mut std::fs::File) -> Result<(), Error>,
) -> Result<(), Error> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    write(temporary.as_file_mut())?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|e| e.error)?;
    Ok(())
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
    let project = decode_json(serde_json::from_slice(&json)?, |_, _| {
        Err(invalid("no migration registered for this schema"))
    })?;
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

#[cfg(test)]
mod atomic_tests {
    use super::*;

    #[test]
    fn failed_write_preserves_target() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project.amio");
        std::fs::write(&path, b"original").unwrap();
        let result = atomic_write(&path, |file| {
            file.write_all(b"partial ZIP")?;
            Err(std::io::Error::other("simulated disk full").into())
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"original");
    }

    #[test]
    fn missing_asset_preserves_target() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project.amio");
        std::fs::write(&path, b"original").unwrap();
        let mut bundle = animathio_core::Project::default();
        animathio_core::Command::AddAsset {
            id: AssetId::of(b"missing"),
            asset: animathio_core::Asset {
                name: "missing".into(),
                media_type: "image/png".into(),
            },
        }
        .apply(&mut bundle)
        .unwrap();
        let assets = BTreeMap::new();
        assert!(save_amio(&path, &bundle, &assets).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"original");
    }
}
