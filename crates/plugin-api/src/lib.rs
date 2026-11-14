//! Plugin API definitions, manifest format, package bundle (.amplugin) and guest SDK.

pub mod guest;
pub mod manifest;
pub mod package;
pub mod schema;
pub mod sources;

pub use manifest::{ManifestError, PluginKind, PluginManifestToml};
pub use package::{
    MANIFEST_FILENAME, MAX_ENTRIES, MAX_MANIFEST_BYTES, MAX_WASM_BYTES, PackageError,
    PluginPackage, WASM_FILENAME,
};
pub use schema::{
    ColorValue, ControlWidget, InspectorParam, InspectorSchema, PointValue, SliderControl,
};

/// Absolute path to the WIT definitions directory packaged with this crate.
pub const WIT_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/wit");

/// Highest stable component ABI implemented by this release (not the crate version).
pub const API_VERSION: &str = "1.0.0";
