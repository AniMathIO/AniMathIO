//! A [`ControlHandler`] backed entirely by an in-memory project, so the CLI
//! and tests work without a GUI. Assets are kept as content-addressed bytes
//! and fed to a fresh [`animathio_render::RenderCtx`] per frame render.

use std::{
    collections::BTreeMap,
    fs,
    io::BufReader,
    path::{Path, PathBuf},
};

use animathio_core::{AssetId, History, Project, Scene, Time, evaluate};
use animathio_format::{Bundle, Limits, read_amio, read_json, save_amio, save_json};
use animathio_plugin_host::PluginHost;
use animathio_render::{RenderCtx, render};
use base64::Engine as _;
use serde_json::Value;

use crate::{
    Call, ControlError, ControlHandler,
    method::{
        AssetInfo, ProjectState, RenderFrameParams, RenderFrameResult, Timeline, media_type_of,
        timeline_of,
    },
    path::{read_bounded, validate_existing_file, validate_output_file},
};

/// A headless [`ControlHandler`] over an in-memory project.
///
/// Plugin clips are resolved through a lazily loaded [`PluginHost`]; LaTeX is
/// validated up front through `animathio_typeset::TypesetEngine`.
#[derive(Default)]
pub struct HeadlessHandler {
    project: Project,
    history: History,
    assets: BTreeMap<AssetId, Vec<u8>>,
    path: Option<PathBuf>,
    playhead: Time,
    playing: bool,
    plugins: Option<PluginHost>,
}

impl HeadlessHandler {
    /// Empty project at the default frame rate, with no plugins loaded.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a handler from an already-loaded bundle, with no plugins loaded.
    #[must_use]
    pub fn from_bundle(bundle: Bundle, path: Option<PathBuf>) -> Self {
        Self {
            project: bundle.project,
            history: History::default(),
            assets: bundle.assets,
            path,
            playhead: Time(0),
            playing: false,
            plugins: None,
        }
    }

    /// Load a `.amio` archive or a `project.json` with adjacent assets, plus
    /// plugins from the shared trusted discovery locations.
    pub fn load(path: &Path) -> Result<Self, ControlError> {
        let (project, assets) = load_project(path)?;
        Ok(Self {
            project,
            assets,
            path: Some(path.to_owned()),
            ..Self::default()
        })
    }

    /// Read access to the current project.
    #[must_use]
    pub fn project(&self) -> &Project {
        &self.project
    }

    /// Read access to the loaded asset bytes.
    #[must_use]
    pub fn assets(&self) -> &BTreeMap<AssetId, Vec<u8>> {
        &self.assets
    }

    fn state(&self) -> ProjectState {
        ProjectState {
            project: self.project.clone(),
            path: self.path.as_ref().map(|p| p.display().to_string()),
            playhead: self.playhead,
            playing: self.playing,
        }
    }

    fn timeline(&self) -> Timeline {
        timeline_of(&self.project)
    }

    fn import_asset(&mut self, path: &Path) -> Result<AssetInfo, ControlError> {
        let canonical = validate_existing_file(path)?;
        let bytes = read_bounded(&canonical, Limits::default().asset_bytes)?;
        let id = AssetId::of(&bytes);
        let name = canonical
            .file_name()
            .map_or_else(|| "asset".to_owned(), |n| n.to_string_lossy().into_owned());
        let media_type = media_type_of(&name).to_owned();
        self.assets.insert(id.clone(), bytes);
        Ok(AssetInfo {
            id,
            name,
            media_type,
        })
    }

    fn render_frame_inner(
        &mut self,
        params: RenderFrameParams,
    ) -> Result<RenderFrameResult, ControlError> {
        let mut ctx = RenderCtx::from_assets(self.assets.clone())?;
        let scene = self.resolve_scene(
            evaluate(&self.project, params.time),
            (params.width, params.height),
        );
        if let Some(host) = &self.plugins {
            ctx.set_effect_processor(host.effect_processor());
        }
        let frame = render(&scene, &mut ctx, (params.width, params.height))?;
        if let Some(output) = params.output {
            let canonical = validate_output_file(&output)?;
            frame.save_png(&canonical)?;
            Ok(RenderFrameResult {
                size: frame.size,
                base64: None,
                path: Some(canonical.display().to_string()),
            })
        } else {
            let mut encoded = Vec::new();
            frame.write_png(&mut encoded)?;
            Ok(RenderFrameResult {
                size: frame.size,
                base64: Some(base64::engine::general_purpose::STANDARD.encode(encoded)),
                path: None,
            })
        }
    }

    /// Load the plugin host lazily when the project contains plugin clips.
    fn ensure_plugins(&mut self) {
        let needs_plugins = self.project.tracks().iter().any(|track| {
            !track.audio_effects().is_empty()
                || track.clips().values().any(|clip| {
                    matches!(clip.kind, animathio_core::ClipKind::Plugin { .. })
                        || !clip.effects.is_empty()
                        || !clip.audio_effects.is_empty()
                })
        });
        if self.plugins.is_none() && needs_plugins {
            self.plugins = load_plugins();
        }
    }

    fn effects_list(&mut self) -> Vec<crate::EffectInfo> {
        self.ensure_effect_plugins();
        crate::effect_infos(self.plugins.as_ref().map_or_default(PluginHost::effects))
    }

    fn audio_effects_list(&mut self) -> Vec<crate::EffectInfo> {
        self.ensure_effect_plugins();
        crate::effect_infos(
            self.plugins
                .as_ref()
                .map_or_default(PluginHost::audio_effects),
        )
    }

    fn audio_effects_add(
        &mut self,
        params: &crate::AudioEffectAddParams,
    ) -> Result<crate::AudioEffectAdded, ControlError> {
        self.ensure_effect_plugins();
        let descriptors = self
            .plugins
            .as_ref()
            .map_or_default(PluginHost::audio_effects);
        let (command, added) =
            crate::add_audio_effect_command(&self.project, &descriptors, params)?;
        self.history.execute(&mut self.project, &command)?;
        Ok(added)
    }

    fn effects_add(
        &mut self,
        params: &crate::EffectAddParams,
    ) -> Result<crate::EffectAdded, ControlError> {
        self.ensure_effect_plugins();
        let descriptors = self.plugins.as_ref().map_or_default(PluginHost::effects);
        let (command, added) = crate::add_effect_command(&self.project, &descriptors, params)?;
        self.history.execute(&mut self.project, &command)?;
        Ok(added)
    }

    fn command_apply(&mut self, command: &animathio_core::Command) -> Result<Value, ControlError> {
        self.check_effect_params(command)?;
        self.history.execute(&mut self.project, command)?;
        Ok(serde_json::to_value(())?)
    }

    /// Reject commands that would store effect parameters outside the plugin's
    /// declared ranges (core only validates names and finiteness).
    fn check_effect_params(
        &mut self,
        command: &animathio_core::Command,
    ) -> Result<(), ControlError> {
        if !animathio_plugin_host::mentions_effect_params(command) {
            return Ok(());
        }
        self.ensure_effect_plugins();
        match &self.plugins {
            Some(host) => host
                .validate_effect_command(&self.project, command)
                .map_err(ControlError::InvalidParams),
            None => Ok(()),
        }
    }

    /// Load the plugin host even when the project has no plugin clips or effects yet, so
    /// effect listing and adding work on an empty project.
    fn ensure_effect_plugins(&mut self) {
        if self.plugins.is_none() {
            self.plugins = load_plugins();
        }
    }

    /// Resolve plugin clips in `scene` at `size`, leaving the scene unchanged
    /// when no plugins are installed (matching headless preview placeholders).
    fn resolve_scene(&mut self, scene: Scene, size: (u32, u32)) -> Scene {
        self.ensure_plugins();
        match &mut self.plugins {
            Some(host) => host.resolve(&scene, size),
            None => scene,
        }
    }

    fn save(&mut self, path: Option<PathBuf>) -> Result<String, ControlError> {
        let target = match path.or_else(|| self.path.clone()) {
            Some(path) => validate_output_file(&path)?,
            None => {
                return Err(ControlError::Unsupported(
                    "project has never been saved; supply a path".to_owned(),
                ));
            }
        };
        self.project.validate()?;
        if target
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("json"))
        {
            save_json(&target, &self.project)?;
            let assets_dir = target.parent().unwrap_or(Path::new(".")).join("assets");
            fs::create_dir_all(&assets_dir)?;
            for (id, bytes) in &self.assets {
                fs::write(assets_dir.join(id.as_str()), bytes)?;
            }
        } else {
            save_amio(&target, &self.project, &self.assets)?;
        }
        self.path = Some(target.clone());
        Ok(target.display().to_string())
    }

    fn open(&mut self, path: &Path) -> Result<(), ControlError> {
        let (project, assets) = load_project(path)?;
        self.project = project;
        self.history = History::default();
        self.assets = assets;
        self.path = Some(path.to_owned());
        self.playhead = Time(0);
        self.playing = false;
        Ok(())
    }

    fn import_v1(&mut self, path: &Path) -> Result<(), ControlError> {
        let canonical = validate_existing_file(path)?;
        let bytes = read_bounded(&canonical, animathio_import_v1_limits())?;
        let (project, assets, report) = animathio_import_v1::import_v1_with_report(&bytes)?;
        for warning in report.warnings() {
            eprintln!("warning: {warning}");
        }
        self.assets = assets
            .into_iter()
            .map(|asset| (asset.id, asset.bytes))
            .collect();
        self.project = project;
        self.history = History::default();
        self.path = None;
        self.playhead = Time(0);
        self.playing = false;
        Ok(())
    }
}

impl ControlHandler for HeadlessHandler {
    #[allow(clippy::too_many_lines)] // One arm per control method.
    fn handle(&mut self, call: Call) -> Result<Value, ControlError> {
        Ok(match call {
            Call::Export { params } => {
                let output = validate_output_file(&params.output)?;
                let range = animathio_core::TimeRange::new(
                    params.from,
                    params
                        .to
                        .unwrap_or_else(|| animathio_export::duration(&self.project)),
                )?;
                // Resolve plugin clips through the plugin host, mirroring the
                // headless `render.frame` path so MCP `export_video` works with
                // latex/graph clips when their plugins are installed.
                self.ensure_plugins();
                let (width, height) = animathio_export::resolution(&self.project);
                let project = &self.project;
                let assets = &self.assets;
                let plugins = &mut self.plugins;
                let mut exporter = animathio_export::Exporter::new(project, assets)
                    .map_err(|e| ControlError::Other(e.to_string()))?;
                if let Some(host) = plugins.as_ref() {
                    exporter.set_effect_processor(host.effect_processor());
                    exporter.set_audio_effect_processor(host.audio_effect_processor());
                }
                let destination = animathio_export::Destination {
                    path: output,
                    overwrite: params.overwrite,
                };
                let written = exporter
                    .export_to_file(
                        project,
                        range,
                        params.settings,
                        &destination,
                        |_| true,
                        |scene| match plugins {
                            Some(host) => host.resolve(&scene, (width, height)),
                            None => scene,
                        },
                    )
                    .map_err(ControlError::from)?;
                serde_json::to_value(written.display().to_string())?
            }
            Call::TitlesList => serde_json::to_value(crate::title_presets())?,
            Call::TitlesAdd { params } => {
                let (command, added) = crate::title_preset_command(&self.project, &params)?;
                self.history.execute(&mut self.project, &command)?;
                serde_json::to_value(added)?
            }
            Call::EffectsList => serde_json::to_value(self.effects_list())?,
            Call::AudioEffectsList => serde_json::to_value(self.audio_effects_list())?,
            Call::AudioEffectsAdd { params } => {
                serde_json::to_value(self.audio_effects_add(&params)?)?
            }
            Call::EffectsAdd { params } => serde_json::to_value(self.effects_add(&params)?)?,
            Call::ProjectGet => serde_json::to_value(self.state())?,
            Call::ProjectOpen { path } => {
                self.open(&path)?;
                serde_json::to_value(())?
            }
            Call::ProjectSave { path } => serde_json::to_value(self.save(path)?)?,
            Call::CommandApply { command } => self.command_apply(&command)?,
            Call::HistoryUndo => serde_json::to_value(self.history.undo(&mut self.project)?)?,
            Call::HistoryRedo => serde_json::to_value(self.history.redo(&mut self.project)?)?,
            Call::RenderFrame { params } => serde_json::to_value(self.render_frame_inner(params)?)?,
            Call::PlaybackSeek { time } => {
                self.playhead = time;
                self.playing = false;
                serde_json::to_value(())?
            }
            Call::PlaybackPlay => {
                self.playing = true;
                serde_json::to_value(())?
            }
            Call::PlaybackPause => {
                self.playing = false;
                serde_json::to_value(())?
            }
            Call::AssetsImport { path } => serde_json::to_value(self.import_asset(&path)?)?,
            Call::TimelineList => serde_json::to_value(self.timeline())?,
            Call::ProjectImportV1 { path } => {
                self.import_v1(&path)?;
                serde_json::to_value(())?
            }
            Call::ProposeAutocuts { params } => serde_json::to_value(crate::propose_autocuts(
                &self.project,
                &self.assets,
                &params,
            )?)?,
            Call::ApplyAutocuts { proposal } => serde_json::to_value(crate::apply_autocuts(
                &mut self.project,
                &mut self.history,
                &proposal,
            )?)?,
            Call::TrimSilence { params } => serde_json::to_value(crate::trim_silence(
                &mut self.project,
                &mut self.history,
                &self.assets,
                &params,
            )?)?,
            Call::SubtitlesGenerate { params } => serde_json::to_value(crate::generate_subtitles(
                &mut self.project,
                &mut self.history,
                &self.assets,
                &params,
                &std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                &(std::sync::Arc::new(|_| {}) as crate::ProgressFn),
            )?)?,
            Call::SubtitlesExport { params } => {
                serde_json::to_value(crate::export_subtitles(&self.project, &params)?)?
            }
            Call::SubtitlesImport { params } => serde_json::to_value(crate::import_subtitles(
                &mut self.project,
                &mut self.history,
                &params,
            )?)?,
            call @ (Call::MathAdd { .. } | Call::GraphAdd { .. } | Call::ImportManim { .. }) => {
                let replace = matches!(&call, Call::ImportManim { params } if params.replace);
                let result = crate::apply_authoring_call(
                    &mut self.project,
                    &mut self.history,
                    &mut self.assets,
                    &call,
                )?;
                if replace {
                    self.path = None;
                    self.playhead = Time(0);
                    self.playing = false;
                }
                result
            }
        })
    }
}

/// Load a `.amio` archive or a `project.json` with `assets/<hash>` beside it.
fn load_project(path: &Path) -> Result<(Project, BTreeMap<AssetId, Vec<u8>>), ControlError> {
    let canonical = validate_existing_file(path)?;
    let reader = BufReader::new(fs::File::open(&canonical)?);
    if canonical
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("amio"))
    {
        let bundle = read_amio(reader, Limits::default())?;
        return Ok((bundle.project, bundle.assets));
    }
    let project: Project = read_json(reader)?;
    let limits = Limits::default();
    let mut total = 0_u64;
    let mut assets = BTreeMap::new();
    for id in project.assets().keys() {
        let asset_path = canonical
            .parent()
            .unwrap_or(Path::new("."))
            .join("assets")
            .join(id.as_str());
        let bytes = read_bounded(
            &asset_path,
            limits.asset_bytes.min(limits.total_bytes - total),
        )?;
        total += u64::try_from(bytes.len())
            .map_err(|_| ControlError::InvalidPath("asset size overflow".to_owned()))?;
        if AssetId::of(&bytes) != *id {
            return Err(ControlError::InvalidPath(format!(
                "asset hash mismatch: {}",
                asset_path.display()
            )));
        }
        assets.insert(id.clone(), bytes);
    }
    Ok((project, assets))
}

fn animathio_import_v1_limits() -> u64 {
    animathio_import_v1::ImportLimits::default().inflated_bytes
}

fn plugin_cache_dir() -> PathBuf {
    directories::BaseDirs::new().map_or_else(
        || std::env::temp_dir().join("animathio-plugin-cache"),
        |base| base.cache_dir().join("animathio").join("plugins"),
    )
}

/// Load bundles from explicit, executable-relative and per-user locations.
fn load_plugins() -> Option<PluginHost> {
    let dirs = animathio_plugin_host::plugin_search_dirs();
    let has_plugins = dirs.iter().any(|dir| {
        fs::read_dir(&dir.path).is_ok_and(|entries| {
            entries.filter_map(Result::ok).any(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|ext| ext == "amplugin")
            })
        })
    });
    if !has_plugins {
        eprintln!(
            "no plugin bundles found; plugin clips render as placeholders and fail to export \
             (run scripts/build-plugins.sh or set ANIMATHIO_PLUGIN_DIR)"
        );
        return None;
    }
    let mut host = match PluginHost::new(plugin_cache_dir()) {
        Ok(host) => host,
        Err(error) => {
            eprintln!("plugin host unavailable: {error}");
            return None;
        }
    };
    for problem in host.load_discovered_plugins() {
        eprintln!("plugin load problem: {problem}");
    }
    Some(host)
}

#[cfg(test)]
mod tests {
    use super::*;
    use animathio_core::{Clip, ClipId, Command, TICKS_PER_SECOND, TimeRange, Track, TrackId};

    #[test]
    fn command_apply_and_history_round_trip() {
        let mut handler = HeadlessHandler::new();
        let track = Track::new(TrackId(1), "Text");
        handler
            .handle(Call::CommandApply {
                command: Command::AddTrack { track, index: 0 },
            })
            .unwrap();
        let clip = Clip::new(
            ClipId(1),
            animathio_plugin_api::sources::text("hi".to_owned(), 0.08, [255, 255, 255, 255]),
            TimeRange::new(Time(0), Time(TICKS_PER_SECOND)).unwrap(),
        );
        handler
            .handle(Call::CommandApply {
                command: Command::AddClip {
                    track: TrackId(1),
                    clip,
                },
            })
            .unwrap();
        assert_eq!(handler.project().tracks().len(), 1);

        let undone: bool =
            serde_json::from_value(handler.handle(Call::HistoryUndo).unwrap()).unwrap();
        assert!(undone);
        assert!(handler.project().tracks()[0].clips().is_empty());

        let redone: bool =
            serde_json::from_value(handler.handle(Call::HistoryRedo).unwrap()).unwrap();
        assert!(redone);
        assert_eq!(handler.project().tracks()[0].clips().len(), 1);
    }

    #[test]
    fn timeline_lists_clips_with_kind_labels() {
        let mut handler = HeadlessHandler::new();
        let track = Track::new(TrackId(1), "Text");
        handler
            .handle(Call::CommandApply {
                command: Command::AddTrack { track, index: 0 },
            })
            .unwrap();
        let timeline: Timeline =
            serde_json::from_value(handler.handle(Call::TimelineList).unwrap()).unwrap();
        assert_eq!(timeline.tracks.len(), 1);
        assert_eq!(timeline.tracks[0].name, "Text");
    }

    #[test]
    fn render_frame_returns_png_bytes() {
        let mut handler = HeadlessHandler::new();
        handler
            .handle(Call::CommandApply {
                command: Command::AddTrack {
                    track: Track::new(TrackId(1), "Text"),
                    index: 0,
                },
            })
            .unwrap();
        let result: RenderFrameResult = serde_json::from_value(
            handler
                .handle(Call::RenderFrame {
                    params: RenderFrameParams {
                        time: Time(0),
                        width: 32,
                        height: 32,
                        output: None,
                    },
                })
                .unwrap(),
        )
        .unwrap();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(result.base64.unwrap())
            .unwrap();
        assert_eq!(&decoded[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(result.size, (32, 32));
    }

    #[test]
    fn import_asset_registers_bytes_and_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("image.png");
        // A minimal 1x1 transparent PNG.
        let png: &[u8] = &[
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, b'I', b'H', b'D', b'R',
            0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1f, 0x15, 0xc4, 0x89, 0, 0, 0, 0x0a, b'I',
            b'D', b'A', b'T', 0x08, 0xd7, 0x63, 0x60, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x0a, b'I',
            b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82,
        ];
        std::fs::write(&file, png).unwrap();
        let info: AssetInfo =
            serde_json::from_value(handler_import(&mut HeadlessHandler::new(), &file).unwrap())
                .unwrap();
        assert_eq!(info.media_type, "image/png");
        assert_eq!(info.name, "image.png");
    }

    fn handler_import(handler: &mut HeadlessHandler, path: &Path) -> Result<Value, ControlError> {
        handler.handle(Call::AssetsImport {
            path: path.to_owned(),
        })
    }

    fn text_project() -> HeadlessHandler {
        let mut handler = HeadlessHandler::new();
        handler.plugins = Some(animathio_plugin_host::test_support::host());
        let range = TimeRange::new(Time(0), Time(TICKS_PER_SECOND / 5)).unwrap();
        for command in [
            Command::SetSettings {
                width: 64,
                height: 36,
                frame_rate: animathio_core::FrameRate::new(30, 1).unwrap(),
                background: [0, 0, 0, 255],
                duration: Time(TICKS_PER_SECOND / 5),
            },
            Command::AddTrack {
                track: Track::new(TrackId(1), "Text"),
                index: 0,
            },
            Command::AddClip {
                track: TrackId(1),
                clip: Clip::new(
                    ClipId(1),
                    animathio_plugin_api::sources::text(
                        "export".to_owned(),
                        0.2,
                        [255, 255, 255, 255],
                    ),
                    range,
                ),
            },
        ] {
            handler.handle(Call::CommandApply { command }).unwrap();
        }
        handler
    }

    #[test]
    fn export_fails_when_a_plugin_clip_cannot_be_resolved() {
        let mut handler = text_project();
        // A host that loaded no plugins, as when the bundles were not built.
        let cache = tempfile::tempdir().unwrap();
        handler.plugins = Some(PluginHost::new(cache.path()).unwrap());
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("clip.mp4");
        let error = handler
            .handle(Call::Export {
                params: export_params(output.clone(), false),
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("clip 1"), "{error}");
        assert!(error.contains("org.animathio.text"), "{error}");
        assert!(!output.exists(), "a failed export leaves no file behind");
    }

    #[test]
    fn effect_parameters_are_range_checked_on_apply() {
        let mut handler = text_project();
        let add = |handler: &mut HeadlessHandler, amount: f64| {
            handler.handle(Call::EffectsAdd {
                params: crate::EffectAddParams {
                    clip: ClipId(1),
                    plugin: "org.animathio.effects-basic".into(),
                    effect: "grayscale".into(),
                    params: BTreeMap::from([("amount".to_owned(), amount)]),
                    index: None,
                },
            })
        };
        assert!(add(&mut handler, 5.0).is_err(), "override above max");
        assert!(add(&mut handler, 0.5).is_ok());
        let set = |handler: &mut HeadlessHandler, value: f64| {
            handler.handle(Call::CommandApply {
                command: Command::SetEffectParam {
                    id: ClipId(1),
                    effect: animathio_core::EffectId(0),
                    name: "amount".into(),
                    value: Some(value),
                },
            })
        };
        assert!(set(&mut handler, 5.0).is_err());
        assert!(set(&mut handler, 1.0).is_ok());
        let keyframe = |value: f64| Command::AddKeyframe {
            id: ClipId(1),
            target: animathio_core::Target::Effect {
                effect: animathio_core::EffectId(0),
                param: "amount".into(),
            },
            time: Time(0),
            keyframe: animathio_core::Keyframe {
                value: value.into(),
                interpolation: animathio_core::Interpolation::default(),
            },
        };
        assert!(
            handler
                .handle(Call::CommandApply {
                    command: keyframe(5.0)
                })
                .is_err()
        );
        assert!(
            handler
                .handle(Call::CommandApply {
                    command: keyframe(0.25)
                })
                .is_ok()
        );
    }

    fn export_params(output: PathBuf, overwrite: bool) -> crate::method::ExportParams {
        crate::method::ExportParams {
            output,
            from: Time(0),
            to: None,
            settings: animathio_export::Settings::default(),
            overwrite,
        }
    }

    #[test]
    fn export_creates_missing_parents_and_requires_overwrite() {
        let mut handler = text_project();
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("missing/sub/clip.mp4");

        let written: String = serde_json::from_value(
            handler
                .handle(Call::Export {
                    params: export_params(output.clone(), false),
                })
                .unwrap(),
        )
        .unwrap();
        // The control surface returns the canonicalized parent with the
        // requested tail, which equals the file's canonical path.
        assert_eq!(Path::new(&written), output.canonicalize().unwrap());
        assert!(output.is_file());
        let entries: Vec<_> = fs::read_dir(output.parent().unwrap()).unwrap().collect();
        assert_eq!(
            entries.len(),
            1,
            "no temporary file survives a successful export"
        );

        let rejected = handler
            .handle(Call::Export {
                params: export_params(output.clone(), false),
            })
            .unwrap_err();
        assert!(
            // Compare canonical forms: Windows reports `\\?\` or 8.3 short paths.
            matches!(
                rejected,
                ControlError::AlreadyExists(ref path)
                    if path.canonicalize().ok() == output.canonicalize().ok()
            ),
            "{rejected:?}"
        );

        handler
            .handle(Call::Export {
                params: export_params(output.clone(), true),
            })
            .unwrap();
        assert!(output.is_file());
    }
}
