//! First-party `import-manim` plugin: the Manim scene importer
//! (`animathio-import-manim`) running in the API 1.1 `importer-plugin` world.
//!
//! The scene is parsed and translated inside the sandbox; `ImageMobject` files are
//! read only through the host's bounded `read-sibling` import. The rebuilt project
//! leaves as a version-1 command document, which the host validates and replays.

wit_bindgen::generate!({
    path: "../../crates/plugin-api/wit",
    world: "importer-plugin",
});

use std::sync::Arc;

use animathio::plugin::import_files;
use animathio::plugin::types::{
    Diagnostic, ParamEntry, ParamKind, ParamSpec, ParamValue, StringParam,
};
use animathio_import_manim::{ImportLimits, ManimOptions, SiblingFiles};
use exports::animathio::plugin::importer::{
    Guest as Importer, ImportMessage, ImportOutput, ImportSource, ImportedAsset,
    ImporterDescriptor, MessageSeverity,
};

struct ManimImport;

impl Guest for ManimImport {
    fn describe() -> Manifest {
        Manifest {
            name: "Manim Import".to_owned(),
            version: "1.0.0".to_owned(),
            description: "Import Manim Community scene scripts".to_owned(),
            kinds: vec!["importer".into()],
            metadata: vec![],
            params: Vec::new(),
        }
    }
}

/// `ImageMobject` reads go through the host, confined to the granted directory.
struct HostFiles;

impl SiblingFiles for HostFiles {
    fn read(&self, relative: &str, _max_bytes: u64) -> Result<Vec<u8>, String> {
        import_files::read_sibling(relative).map_err(|diagnostic| diagnostic.message)
    }
}

fn diagnostic(code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        code: code.into(),
        message: message.into(),
        metadata: vec![],
    }
}

fn message(
    severity: MessageSeverity,
    line: usize,
    column: usize,
    text: &str,
    snippet: &str,
) -> ImportMessage {
    ImportMessage {
        severity,
        line: u32::try_from(line).unwrap_or(u32::MAX),
        column: u32::try_from(column).unwrap_or(u32::MAX),
        message: text.to_owned(),
        snippet: snippet.to_owned(),
    }
}

impl Importer for ManimImport {
    fn list_importers() -> Vec<ImporterDescriptor> {
        vec![ImporterDescriptor {
            id: "manim".into(),
            name: "Manim scene".into(),
            description: "Manim Community scene script".into(),
            extensions: vec!["py".into()],
            params: vec![ParamSpec {
                id: "scene".into(),
                label: "Scene class (empty for the first)".into(),
                kind: ParamKind::String(StringParam {
                    default: String::new(),
                }),
            }],
        }]
    }

    fn run_import(
        importer: String,
        source: ImportSource,
        params: Vec<ParamEntry>,
    ) -> Result<ImportOutput, Diagnostic> {
        if importer != "manim" {
            return Err(diagnostic(
                "invalid-input",
                format!("unknown importer {importer:?}"),
            ));
        }
        let text = String::from_utf8(source.bytes)
            .map_err(|_| diagnostic("invalid-input", "manim script is not valid UTF-8"))?;
        let scene = params
            .iter()
            .find_map(|entry| match (&entry.value, entry.id.as_str()) {
                (ParamValue::String(scene), "scene") if !scene.is_empty() => Some(scene.clone()),
                _ => None,
            });
        let options = ManimOptions {
            scene,
            script_dir: None,
            files: Some(Arc::new(HostFiles)),
        };
        let (project, assets, report) = animathio_import_manim::import_manim_with_limits(
            &text,
            &options,
            &ImportLimits::default(),
        )
        .map_err(|error| diagnostic("invalid-input", error.to_string()))?;
        let commands = serde_json::json!({
            "version": 1,
            "commands": project.rebuild_commands(),
        })
        .to_string();
        let mut messages: Vec<ImportMessage> = report
            .warnings()
            .iter()
            .map(|w| {
                message(
                    MessageSeverity::Warning,
                    w.line,
                    w.column,
                    &w.message,
                    &w.snippet,
                )
            })
            .collect();
        messages.extend(report.infos().iter().map(|i| {
            message(
                MessageSeverity::Info,
                i.line,
                i.column,
                &i.message,
                &i.snippet,
            )
        }));
        Ok(ImportOutput {
            commands,
            assets: assets
                .into_iter()
                .map(|asset| ImportedAsset {
                    id: asset.id.as_str().to_owned(),
                    name: asset.name,
                    media_type: asset.media_type,
                    bytes: asset.bytes,
                })
                .collect(),
            messages,
        })
    }
}

export!(ManimImport);
