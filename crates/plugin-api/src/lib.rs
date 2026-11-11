//! Plugin API definitions, manifest format, package bundle (.amplugin) and guest SDK.

pub mod guest;
pub mod manifest;
pub mod package;
pub mod schema;

pub use manifest::{ManifestError, PluginKind, PluginManifestToml};
pub use package::{MANIFEST_FILENAME, PackageError, PluginPackage, WASM_FILENAME};
pub use schema::{
    ColorValue, ControlWidget, InspectorParam, InspectorSchema, PointValue, SliderControl,
};

/// Absolute path to the WIT definitions directory packaged with this crate.
pub const WIT_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/wit");
