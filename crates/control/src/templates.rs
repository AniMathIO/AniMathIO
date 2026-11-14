//! Portable project templates, separate from the versioned project schema.
use crate::{ControlError, TitleAddParams, title_preset_command};
use animathio_core::{
    AssetId, ClipId, ClipKind, Command, FrameRate, Project, TICKS_PER_SECOND, Time, TimeRange,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read,
    path::Path,
};

/// One editable clip in a template.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Placeholder {
    /// Existing clip identity.
    pub clip: ClipId,
    /// Optional user-facing prompt.
    pub label: Option<String>,
}

/// Metadata stored in template.json, never inside the project schema.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TemplateManifest {
    /// Stable catalog identity.
    pub id: String,
    /// Card title.
    pub name: String,
    /// Card description.
    pub description: String,
    /// Human-readable canvas ratio.
    pub aspect: String,
    /// Library category.
    pub category: String,
    /// Optional thumbnail filename; only thumbnail.png is supported.
    pub thumbnail: Option<String>,
    /// Clips that can be filled by the user.
    pub placeholders: Vec<Placeholder>,
}

impl TemplateManifest {
    /// Reject stale references, duplicate placeholders and unsafe thumbnail paths.
    pub fn validate(&self, project: &Project) -> Result<(), ControlError> {
        project.validate()?;
        let mut seen = BTreeSet::new();
        if self.id.is_empty()
            || self.name.trim().is_empty()
            || self
                .thumbnail
                .as_deref()
                .is_some_and(|p| p != "thumbnail.png")
            || self.placeholders.iter().any(|p| {
                !seen.insert(p.clip)
                    || !project
                        .tracks()
                        .iter()
                        .any(|t| t.clips().contains_key(&p.clip))
            })
        {
            return Err(ControlError::InvalidParams(
                "invalid template manifest or placeholder clip".into(),
            ));
        }
        Ok(())
    }
}

/// A normal project bundle paired with template metadata.
pub struct ProjectTemplate {
    /// Resource-free project model.
    pub project: Project,
    /// Content-addressed media bytes.
    pub assets: BTreeMap<AssetId, Vec<u8>>,
    /// Template metadata.
    pub manifest: TemplateManifest,
}

impl ProjectTemplate {
    /// Load a .amiotemplate directory. Archive parsing retains normal format limits.
    pub fn load(directory: &Path) -> Result<Self, ControlError> {
        let manifest: TemplateManifest = serde_json::from_reader(
            File::open(directory.join("template.json"))?.take(1024 * 1024),
        )?;
        let bundle = animathio_format::read_amio(
            File::open(directory.join("project.amio"))?,
            animathio_format::Limits::default(),
        )?;
        manifest.validate(&bundle.project)?;
        Ok(Self {
            project: bundle.project,
            assets: bundle.assets,
            manifest,
        })
    }

    /// Write a new directory, refusing to overwrite an existing template.
    pub fn save(&self, directory: &Path) -> Result<(), ControlError> {
        self.manifest.validate(&self.project)?;
        if self.manifest.thumbnail.is_some() {
            return Err(ControlError::InvalidParams(
                "thumbnail bytes must be provided separately; save without a thumbnail".into(),
            ));
        }
        std::fs::create_dir(directory)?;
        let result = (|| {
            animathio_format::save_amio(
                directory.join("project.amio"),
                &self.project,
                &self.assets,
            )?;
            serde_json::to_writer_pretty(
                File::create(directory.join("template.json"))?,
                &self.manifest,
            )?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_dir_all(directory);
        }
        result
    }

    /// Create an independent project with text placeholders filled in one edit.
    pub fn instantiate(&self, values: &BTreeMap<ClipId, String>) -> Result<Project, ControlError> {
        let command = self.fill_text(values)?;
        let mut project = self.project.clone();
        command.apply(&mut project)?;
        Ok(project)
    }

    /// Validate all replacements and return one undoable edit. Omitted fields retain defaults.
    pub fn fill_text(&self, values: &BTreeMap<ClipId, String>) -> Result<Command, ControlError> {
        self.manifest.validate(&self.project)?;
        let mut commands = Vec::new();
        for (id, value) in values {
            if !self.manifest.placeholders.iter().any(|p| p.clip == *id)
                || value.trim().is_empty()
                || value.len() > 4096
            {
                return Err(ControlError::InvalidParams(
                    "unknown placeholder or invalid text".into(),
                ));
            }
            let clip = self
                .project
                .tracks()
                .iter()
                .find_map(|t| t.clips().get(id))
                .ok_or_else(|| ControlError::InvalidParams("missing placeholder".into()))?;
            let ClipKind::Plugin { plugin_id, .. } = &clip.kind else {
                return Err(ControlError::InvalidParams(
                    "placeholder is not text".into(),
                ));
            };
            let name = match plugin_id.as_str() {
                "org.animathio.text" => "text",
                "org.animathio.latex" => "source",
                _ => {
                    return Err(ControlError::InvalidParams(
                        "placeholder is not text".into(),
                    ));
                }
            };
            commands.push(Command::SetPluginParam {
                id: *id,
                name: name.into(),
                value: Some(animathio_core::ParamValue::String(value.clone())),
            });
        }
        Ok(Command::Batch { commands })
    }
}

/// Five lightweight, media-free first-party templates composed from bundled title presets.
pub fn project_templates() -> Result<Vec<ProjectTemplate>, ControlError> {
    [
        (
            "math_explainer",
            "Math explainer",
            "16:9",
            1920,
            1080,
            "title_subtitle",
        ),
        (
            "vertical_short",
            "Vertical short with captions",
            "9:16",
            1080,
            1920,
            "caption",
        ),
        (
            "interview",
            "Lower-third interview",
            "16:9",
            1920,
            1080,
            "lower_third",
        ),
        ("quote", "Quote card", "1:1", 1080, 1080, "chapter_card"),
        (
            "equation",
            "Equation reveal",
            "16:9",
            1920,
            1080,
            "math_title",
        ),
    ]
    .into_iter()
    .map(|(id, name, aspect, width, height, preset)| {
        let mut project = Project::new(FrameRate::new(30, 1)?);
        Command::SetSettings {
            width,
            height,
            frame_rate: project.frame_rate(),
            background: [18, 20, 28, 255],
            duration: Time(6 * TICKS_PER_SECOND),
        }
        .apply(&mut project)?;
        let (command, _) = title_preset_command(
            &project,
            &TitleAddParams {
                preset: preset.into(),
                fields: BTreeMap::new(),
                range: TimeRange::new(Time(0), Time(6 * TICKS_PER_SECOND))?,
            },
        )?;
        command.apply(&mut project)?;
        let placeholders = project
            .tracks()
            .iter()
            .flat_map(|t| t.clips().values())
            .filter_map(|c| {
                let ClipKind::Plugin { params, .. } = &c.kind else {
                    return None;
                };
                let field = params.get("_title_field")?.as_str()?;
                (!field.is_empty()).then(|| Placeholder {
                    clip: c.id,
                    label: Some(field.into()),
                })
            })
            .collect();
        let manifest = TemplateManifest {
            id: id.into(),
            name: name.into(),
            description: format!("Six-second {name} composition"),
            aspect: aspect.into(),
            category: "Titles".into(),
            thumbnail: None,
            placeholders,
        };
        manifest.validate(&project)?;
        Ok(ProjectTemplate {
            project,
            assets: BTreeMap::new(),
            manifest,
        })
    })
    .collect()
}
