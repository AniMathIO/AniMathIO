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
#[derive(Clone)]
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

/// User library, outside the project archive.
pub fn template_library_dir() -> Result<std::path::PathBuf, ControlError> {
    directories::ProjectDirs::from("org", "animathio", "animathio")
        .map(|d| d.data_dir().join("templates"))
        .ok_or_else(|| ControlError::InvalidParams("user data directory unavailable".into()))
}

/// Bundled and valid user templates. Broken user entries do not hide the catalog.
pub fn template_catalog() -> Result<Vec<ProjectTemplate>, ControlError> {
    let mut templates = project_templates()?;
    if let Ok(entries) = std::fs::read_dir(template_library_dir()?) {
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|t| t.is_dir())
                && let Ok(template) = ProjectTemplate::load(&entry.path())
                && !templates
                    .iter()
                    .any(|t| t.manifest.id == template.manifest.id)
            {
                templates.push(template);
            }
        }
    }
    Ok(templates)
}

/// Store a snapshot under a generated identity; names never become paths.
pub fn save_user_template(template: ProjectTemplate) -> Result<String, ControlError> {
    save_user_template_at(template, &template_library_dir()?)
}

fn save_user_template_at(
    mut template: ProjectTemplate,
    directory: &Path,
) -> Result<String, ControlError> {
    std::fs::create_dir_all(directory)?;
    let mut random = [0u8; 16];
    getrandom::fill(&mut random).map_err(|e| ControlError::InvalidParams(e.to_string()))?;
    let id = format!("user-{}", &AssetId::of(&random).as_str()[..32]);
    template.manifest.id.clone_from(&id);
    template.save(&directory.join(&id))?;
    Ok(id)
}

/// Delete only generated library entries, never bundled templates or arbitrary paths.
pub fn delete_user_template(id: &str) -> Result<(), ControlError> {
    if !id.starts_with("user-") || id.len() != 37 || !id[5..].bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(ControlError::InvalidParams(
            "not a user template identity".into(),
        ));
    }
    let path = template_library_dir()?.join(id);
    if std::fs::symlink_metadata(&path)?.file_type().is_symlink() {
        return Err(ControlError::InvalidParams(
            "symlink template refused".into(),
        ));
    }
    std::fs::remove_dir_all(path)?;
    Ok(())
}

impl ProjectTemplate {
    /// Replace a placeholder source in one undo step, retaining its identity and edits.
    /// Bytes are returned separately for the runtime asset pool.
    pub fn fill_media(
        &self,
        id: ClipId,
        path: &Path,
        cover: bool,
    ) -> Result<(AssetId, Vec<u8>, Command), ControlError> {
        self.manifest.validate(&self.project)?;
        if !self.manifest.placeholders.iter().any(|p| p.clip == id) {
            return Err(ControlError::InvalidParams("unknown placeholder".into()));
        }
        let (asset, bytes, import, _) = crate::prepare_media_import(&self.project, path)?;
        let Command::Batch { commands } = import else {
            return Err(ControlError::InvalidParams(
                "invalid media import command".into(),
            ));
        };
        let metadata = commands
            .iter()
            .find(|c| matches!(c, Command::AddAsset { .. }))
            .cloned()
            .ok_or_else(|| ControlError::InvalidParams("missing import asset".into()))?;
        let kind = commands
            .iter()
            .find_map(|c| {
                if let Command::AddClip { clip, .. } = c {
                    Some(clip.kind.clone())
                } else {
                    None
                }
            })
            .ok_or_else(|| ControlError::InvalidParams("missing import clip".into()))?;
        if matches!(kind, ClipKind::Audio(_)) {
            return Err(ControlError::InvalidParams(
                "a placeholder needs an image or video, not audio".into(),
            ));
        }
        let track = self
            .project
            .tracks()
            .iter()
            .find(|t| t.clips().contains_key(&id))
            .ok_or_else(|| ControlError::InvalidParams("missing placeholder".into()))?;
        let mut clip = track.clips()[&id].clone();
        clip.kind = kind;
        clip.fit = if cover {
            animathio_core::Fit::Cover
        } else {
            animathio_core::Fit::Contain
        };
        let mut commands = Vec::new();
        if !self.project.assets().contains_key(&asset) {
            commands.push(metadata);
        }
        // Swap the source in place. Remove + add would also delete the clip's linked
        // companions (title presets link their text layers) and lose its z-order.
        commands.push(Command::RestoreClips {
            track: track.id(),
            ids: vec![id],
            clips: vec![clip],
        });
        Ok((asset, bytes, Command::Batch { commands }))
    }
}

/// A media replacement for one placeholder.
#[derive(Clone, Debug)]
pub struct MediaFill {
    /// Local image or video file.
    pub path: std::path::PathBuf,
    /// Crop to fill the placeholder box (true) or fit the whole source inside it (false).
    pub cover: bool,
}

/// All replacements for a template as one undoable edit plus the media bytes it needs.
#[derive(Clone)]
pub struct TemplateFill {
    /// One `Command::Batch` applying every replacement.
    pub command: Command,
    /// Bytes for newly referenced assets, for the runtime asset pool.
    pub assets: BTreeMap<AssetId, Vec<u8>>,
}

impl ProjectTemplate {
    /// Whether a placeholder is a text/LaTeX clip that accepts typed text.
    #[must_use]
    pub fn accepts_text(&self, id: ClipId) -> bool {
        self.project
            .tracks()
            .iter()
            .find_map(|t| t.clips().get(&id))
            .is_some_and(|c| {
                matches!(&c.kind, ClipKind::Plugin { plugin_id, .. }
                    if matches!(plugin_id.as_str(), "org.animathio.text" | "org.animathio.latex"))
            })
    }

    /// Validate every text and media replacement and combine them into one `Command::Batch`.
    /// Nothing is applied; a placeholder may be filled with text or media, not both.
    pub fn fill(
        &self,
        text: &BTreeMap<ClipId, String>,
        media: &BTreeMap<ClipId, MediaFill>,
    ) -> Result<TemplateFill, ControlError> {
        if let Some(id) = text.keys().find(|id| media.contains_key(id)) {
            return Err(ControlError::InvalidParams(format!(
                "placeholder {} supplied as both text and media",
                id.0
            )));
        }
        let mut draft = self.clone();
        let mut commands = Vec::new();
        let mut assets = BTreeMap::new();
        for (id, item) in media {
            let (asset, bytes, command) = draft.fill_media(*id, &item.path, item.cover)?;
            command.apply(&mut draft.project)?;
            commands.push(command);
            assets.insert(asset, bytes);
        }
        if !text.is_empty() {
            commands.push(self.fill_text(text)?);
        }
        Ok(TemplateFill {
            command: Command::Batch { commands },
            assets,
        })
    }
}

#[cfg(test)]
mod user_library_tests {
    #[test]
    fn saved_user_template_round_trips_without_changing_the_project() {
        let directory = tempfile::tempdir().unwrap();
        let mut template = super::project_templates().unwrap().remove(0);
        template.manifest.name = "My lesson".into();
        template.manifest.category = "Lessons".into();
        let project = template.project.clone();
        let id = super::save_user_template_at(template, directory.path()).unwrap();
        assert_eq!(id.len(), 37);
        let loaded = super::ProjectTemplate::load(&directory.path().join(&id)).unwrap();
        assert_eq!(loaded.manifest.id, id);
        assert_eq!(loaded.manifest.name, "My lesson");
        assert_eq!(loaded.manifest.category, "Lessons");
        assert_eq!(loaded.project, project);
        assert!(!loaded.manifest.placeholders.is_empty());
    }
}
