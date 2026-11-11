use crate::manifest::{ManifestError, PluginManifestToml};
use std::io::{Cursor, Read, Seek, Write};
use thiserror::Error;
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

/// File name of the manifest inside an `.amplugin` package.
pub const MANIFEST_FILENAME: &str = "plugin.toml";
/// File name of the WebAssembly component inside an `.amplugin` package.
pub const WASM_FILENAME: &str = "plugin.wasm";

/// Error creating, reading or extracting an `.amplugin` package.
#[derive(Debug, Error)]
pub enum PackageError {
    /// IO error reading or writing zip stream.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// Zip archive processing error.
    #[error("zip archive error: {0}")]
    Zip(#[from] zip::result::ZipError),
    /// Manifest parsing or validation error.
    #[error("manifest error: {0}")]
    Manifest(#[from] ManifestError),
    /// Missing required file inside the package.
    #[error("missing archive entry: {0}")]
    MissingEntry(&'static str),
    /// Corrupted wasm or empty component.
    #[error("invalid wasm binary in package")]
    InvalidWasm,
}

/// An `.amplugin` bundle containing metadata and compiled WebAssembly component bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginPackage {
    /// Manifest parsed from `plugin.toml`.
    pub manifest: PluginManifestToml,
    /// Precompiled or guest `.wasm` component bytes.
    pub wasm_bytes: Vec<u8>,
}

impl PluginPackage {
    /// Construct a package in memory from manifest and wasm binary bytes.
    pub fn new(manifest: PluginManifestToml, wasm_bytes: Vec<u8>) -> Result<Self, PackageError> {
        manifest.validate()?;
        if wasm_bytes.is_empty() {
            return Err(PackageError::InvalidWasm);
        }
        Ok(Self {
            manifest,
            wasm_bytes,
        })
    }

    /// Read an `.amplugin` package from any reader implementing `Read + Seek`.
    pub fn read<R: Read + Seek>(reader: R) -> Result<Self, PackageError> {
        let mut archive = ZipArchive::new(reader)?;
        let mut manifest_str = String::new();
        {
            let mut manifest_file = archive
                .by_name(MANIFEST_FILENAME)
                .map_err(|_| PackageError::MissingEntry(MANIFEST_FILENAME))?;
            manifest_file.read_to_string(&mut manifest_str)?;
        }
        let manifest = PluginManifestToml::parse(&manifest_str)?;

        let mut wasm_bytes = Vec::new();
        {
            let mut wasm_file = archive
                .by_name(WASM_FILENAME)
                .map_err(|_| PackageError::MissingEntry(WASM_FILENAME))?;
            wasm_file.read_to_end(&mut wasm_bytes)?;
        }

        if wasm_bytes.is_empty() {
            return Err(PackageError::InvalidWasm);
        }

        Ok(Self {
            manifest,
            wasm_bytes,
        })
    }

    /// Read an `.amplugin` package from a byte buffer.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, PackageError> {
        Self::read(Cursor::new(bytes))
    }

    /// Write this package to a writer implementing `Write + Seek`.
    pub fn write<W: Write + Seek>(&self, writer: W) -> Result<(), PackageError> {
        let mut zip = ZipWriter::new(writer);
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o644);

        // Write plugin.toml
        zip.start_file(MANIFEST_FILENAME, options)?;
        let toml_str = self.manifest.to_toml_string()?;
        zip.write_all(toml_str.as_bytes())?;

        // Write plugin.wasm
        zip.start_file(WASM_FILENAME, options)?;
        zip.write_all(&self.wasm_bytes)?;

        zip.finish()?;
        Ok(())
    }

    /// Package the manifest and wasm bytes into an in-memory `.amplugin` zip buffer.
    pub fn create_bytes(&self) -> Result<Vec<u8>, PackageError> {
        let mut cursor = Cursor::new(Vec::new());
        self.write(&mut cursor)?;
        Ok(cursor.into_inner())
    }
}
