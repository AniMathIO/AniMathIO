//! A [`ControlHandler`] backed entirely by an in-memory project, so the CLI
//! and tests work without a GUI. Assets are kept as content-addressed bytes
//! and fed to a fresh [`animathio_render::RenderCtx`] per frame render.

use std::{
    collections::BTreeMap,
    fs,
    io::BufReader,
    path::{Path, PathBuf},
};

use animathio_core::{
    AssetId, Clip, ClipId, ClipKind, Command, History, Interpolation, Keyframe, Project, Target,
    Time, TimeRange, Track, TrackId, Transform, evaluate,
};
use animathio_format::{Bundle, Limits, read_amio, read_json, write_amio, write_json};
use animathio_import_manim::{ImportLimits as ManimLimits, ManimOptions, import_manim_with_limits};
use animathio_plugin_host::PluginHost;
use animathio_render::{RenderCtx, render};
use animathio_typeset::{MathMode, TypesetEngine};
use base64::Engine as _;
use serde_json::{Value, json};

use crate::{
    Call, ControlError, ControlHandler,
    method::{
        AssetInfo, ClipAdded, GraphAddParams, ImportManimParams, ImportManimResult, MathAddParams,
        ProjectState, RenderFrameParams, RenderFrameResult, Timeline, media_type_of, timeline_of,
    },
    path::{read_bounded, validate_existing_file, validate_output_file},
};

const LATEX_PLUGIN_ID: &str = "org.animathio.latex";
const GRAPH_PLUGIN_ID: &str = "org.animathio.graph";

/// A headless [`ControlHandler`] over an in-memory project.
///
/// Plugin clips are resolved through a lazily loaded [`PluginHost`]; LaTeX is
/// validated up front through a lazily created [`TypesetEngine`].
pub struct HeadlessHandler {
    project: Project,
    history: History,
    assets: BTreeMap<AssetId, Vec<u8>>,
    path: Option<PathBuf>,
    playhead: Time,
    playing: bool,
    plugins: Option<PluginHost>,
    typeset: Option<TypesetEngine>,
}

impl Default for HeadlessHandler {
    fn default() -> Self {
        Self {
            project: Project::default(),
            history: History::default(),
            assets: BTreeMap::new(),
            path: None,
            playhead: Time(0),
            playing: false,
            plugins: None,
            typeset: None,
        }
    }
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
            typeset: None,
        }
    }

    /// Load a `.amio` archive or a `project.json` with adjacent assets, plus
    /// any `.amplugin` files found in `ANIMATHIO_PLUGIN_DIR` or `target/plugins`.
    pub fn load(path: &Path) -> Result<Self, ControlError> {
        let (project, assets) = load_project(path)?;
        let mut handler = Self::default();
        handler.project = project;
        handler.assets = assets;
        handler.path = Some(path.to_owned());
        handler.plugins = load_plugins();
        Ok(handler)
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
        let mut ctx = RenderCtx::default();
        for (id, bytes) in &self.assets {
            ctx.resources.insert(id.clone(), bytes.clone())?;
        }
        let scene = evaluate(&self.project, params.time);
        let scene = match &mut self.plugins {
            Some(host) => host.resolve(&scene, (params.width, params.height)),
            None => scene,
        };
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
            let file = fs::File::create(&target)?;
            write_json(file, &self.project)?;
            let assets_dir = target.parent().unwrap_or(Path::new(".")).join("assets");
            fs::create_dir_all(&assets_dir)?;
            for (id, bytes) in &self.assets {
                fs::write(assets_dir.join(id.as_str()), bytes)?;
            }
        } else {
            let file = fs::File::create(&target)?;
            write_amio(file, &self.project, &self.assets)?;
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

    /// Lazily create and return the shared math typeset engine, when available.
    fn typeset_engine(&mut self) -> Option<&TypesetEngine> {
        if self.typeset.is_none() {
            match TypesetEngine::new() {
                Ok(engine) => self.typeset = Some(engine),
                Err(error) => eprintln!("math typesetting unavailable: {error}"),
            }
        }
        self.typeset.as_ref()
    }

    fn math_add(&mut self, params: MathAddParams) -> Result<ClipAdded, ControlError> {
        if let Some(engine) = self.typeset_engine() {
            let mode = match params.mode.as_str() {
                "typst" => MathMode::Typst,
                _ => MathMode::Latex,
            };
            engine
                .typeset(&params.latex, mode, params.size as f32)
                .map_err(ControlError::from)?;
        }

        let base = next_id(&self.project);
        let track_id = TrackId(base);
        let clip_id = ClipId(base + 1);
        let start = params.start;
        let duration = params
            .duration
            .unwrap_or_else(|| project_duration(&self.project));
        let range = clip_range(start, duration)?;

        let reveal = params.reveal_duration.filter(|d| d.0 > 0);
        let mut numeric = BTreeMap::new();
        numeric.insert("reveal".to_owned(), if reveal.is_some() { 0.0 } else { 1.0 });

        let mut clip = Clip::new(
            clip_id,
            ClipKind::Plugin {
                plugin_id: LATEX_PLUGIN_ID.to_owned(),
                params: json!({
                    "source": params.latex,
                    "mode": params.mode,
                    "size": params.size,
                    "color": params.color.unwrap_or([255, 255, 255, 255]),
                }),
            },
            range,
        );
        clip.transform = Transform {
            x: params.x.unwrap_or(0.5),
            y: params.y.unwrap_or(0.5),
            ..Transform::default()
        };
        clip.params = numeric;

        let index = self.project.tracks().len();
        self.history.execute(
            &mut self.project,
            &Command::AddTrack {
                track: Track::new(track_id, "Math"),
                index,
            },
        )?;
        self.history
            .execute(&mut self.project, &Command::AddClip { track: track_id, clip })?;
        if let Some(duration) = reveal {
            self.history.execute(
                &mut self.project,
                &Command::AddKeyframe {
                    id: clip_id,
                    target: Target::Param("reveal".to_owned()),
                    time: Time(0),
                    keyframe: Keyframe {
                        value: 0.0,
                        interpolation: Interpolation::Linear,
                    },
                },
            )?;
            self.history.execute(
                &mut self.project,
                &Command::AddKeyframe {
                    id: clip_id,
                    target: Target::Param("reveal".to_owned()),
                    time: duration,
                    keyframe: Keyframe {
                        value: 1.0,
                        interpolation: Interpolation::Linear,
                    },
                },
            )?;
        }

        Ok(ClipAdded { track_id, clip_id })
    }

    fn graph_add(&mut self, params: GraphAddParams) -> Result<ClipAdded, ControlError> {
        let base = next_id(&self.project);
        let track_id = TrackId(base);
        let clip_id = ClipId(base + 1);
        let start = params.start;
        let duration = params
            .duration
            .unwrap_or_else(|| project_duration(&self.project));
        let range = clip_range(start, duration)?;

        let draw = params.draw_duration.filter(|d| d.0 > 0);
        let mut numeric = BTreeMap::new();
        numeric.insert("draw".to_owned(), if draw.is_some() { 0.0 } else { 1.0 });

        let mut opaque = serde_json::Map::new();
        opaque.insert("items".to_owned(), json!(params.items));
        if let Some(value) = params.x_min {
            opaque.insert("x_min".to_owned(), json!(value));
        }
        if let Some(value) = params.x_max {
            opaque.insert("x_max".to_owned(), json!(value));
        }
        if let Some(value) = params.y_min {
            opaque.insert("y_min".to_owned(), json!(value));
        }
        if let Some(value) = params.y_max {
            opaque.insert("y_max".to_owned(), json!(value));
        }
        if let Some(value) = params.show_grid {
            opaque.insert("show_grid".to_owned(), json!(value));
        }
        if let Some(value) = params.show_axes {
            opaque.insert("show_axes".to_owned(), json!(value));
        }
        if let Some(value) = params.show_labels {
            opaque.insert("show_labels".to_owned(), json!(value));
        }
        opaque.insert(
            "color".to_owned(),
            json!(params.color.unwrap_or([90, 170, 255, 255])),
        );

        let mut clip = Clip::new(
            clip_id,
            ClipKind::Plugin {
                plugin_id: GRAPH_PLUGIN_ID.to_owned(),
                params: Value::Object(opaque),
            },
            range,
        );
        clip.transform = Transform {
            x: params.x.unwrap_or(0.5),
            y: params.y.unwrap_or(0.5),
            ..Transform::default()
        };
        clip.params = numeric;

        let index = self.project.tracks().len();
        self.history.execute(
            &mut self.project,
            &Command::AddTrack {
                track: Track::new(track_id, "Graph"),
                index,
            },
        )?;
        self.history
            .execute(&mut self.project, &Command::AddClip { track: track_id, clip })?;
        if let Some(duration) = draw {
            self.history.execute(
                &mut self.project,
                &Command::AddKeyframe {
                    id: clip_id,
                    target: Target::Param("draw".to_owned()),
                    time: Time(0),
                    keyframe: Keyframe {
                        value: 0.0,
                        interpolation: Interpolation::Linear,
                    },
                },
            )?;
            self.history.execute(
                &mut self.project,
                &Command::AddKeyframe {
                    id: clip_id,
                    target: Target::Param("draw".to_owned()),
                    time: duration,
                    keyframe: Keyframe {
                        value: 1.0,
                        interpolation: Interpolation::Linear,
                    },
                },
            )?;
        }

        Ok(ClipAdded { track_id, clip_id })
    }

    fn import_manim(&mut self, params: ImportManimParams) -> Result<ImportManimResult, ControlError> {
        let (source, script_dir) = match (params.path.as_deref(), params.source.as_deref()) {
            (Some(path), None) => {
                let canonical = validate_existing_file(path)?;
                let bytes = read_bounded(&canonical, ManimLimits::default().source_bytes)?;
                let source = String::from_utf8(bytes).map_err(|_| {
                    ControlError::InvalidPath(format!(
                        "manim script is not valid UTF-8: {}",
                        canonical.display()
                    ))
                })?;
                let dir = params
                    .script_dir
                    .clone()
                    .or_else(|| canonical.parent().map(Path::to_path_buf));
                (source, dir)
            }
            (None, Some(source)) => (source.to_owned(), params.script_dir.clone()),
            (Some(_), Some(_)) => {
                return Err(ControlError::InvalidParams(
                    "`path` and `source` are mutually exclusive".to_owned(),
                ));
            }
            (None, None) => {
                return Err(ControlError::InvalidParams(
                    "either `path` or `source` is required".to_owned(),
                ));
            }
        };

        let options = ManimOptions {
            scene: params.scene.clone(),
            script_dir,
        };
        let (imported, imported_assets, report) =
            import_manim_with_limits(&source, &options, &ManimLimits::default())?;
        let warnings = report.warnings().iter().map(ToString::to_string).collect();
        let infos = report.infos().iter().map(ToString::to_string).collect();
        let tracks = imported.tracks().len();
        let asset_count = imported_assets.len();

        if params.replace {
            self.project = imported;
            self.assets = imported_assets
                .into_iter()
                .map(|asset| (asset.id, asset.bytes))
                .collect();
            self.history = History::default();
            self.path = None;
            self.playhead = Time(0);
            self.playing = false;
        } else {
            self.append_project(&imported, imported_assets)?;
        }

        Ok(ImportManimResult {
            tracks,
            assets: asset_count,
            warnings,
            infos,
        })
    }

    /// Append an imported project's tracks and assets onto the current project,
    /// remapping identities so they cannot collide with existing clips.
    fn append_project(
        &mut self,
        imported: &Project,
        imported_assets: Vec<animathio_import_manim::Asset>,
    ) -> Result<(), ControlError> {
        for asset in &imported_assets {
            if !self.project.assets().contains_key(&asset.id) {
                self.history.execute(
                    &mut self.project,
                    &Command::AddAsset {
                        id: asset.id.clone(),
                        asset: animathio_core::Asset {
                            name: asset.name.clone(),
                            media_type: asset.media_type.clone(),
                        },
                    },
                )?;
            }
            self.assets
                .entry(asset.id.clone())
                .or_insert_with(|| asset.bytes.clone());
        }

        let mut next_track = self
            .project
            .tracks()
            .iter()
            .map(|track| track.id().0)
            .max()
            .map_or(1, |max| max + 1);
        let mut next_clip = self
            .project
            .tracks()
            .iter()
            .flat_map(|track| track.clips().keys())
            .map(|clip| clip.0)
            .max()
            .map_or(1, |max| max + 1);

        for track in imported.tracks() {
            let track_id = TrackId(next_track);
            next_track += 1;
            let index = self.project.tracks().len();
            self.history.execute(
                &mut self.project,
                &Command::AddTrack {
                    track: Track::new(track_id, track.name().to_owned()),
                    index,
                },
            )?;
            if track.gain() != 1.0 {
                self.history.execute(
                    &mut self.project,
                    &Command::SetTrackGain {
                        id: track_id,
                        gain: track.gain(),
                    },
                )?;
            }
            for clip in track.clips().values() {
                let clip_id = ClipId(next_clip);
                next_clip += 1;
                let mut clip = clip.clone();
                clip.id = clip_id;
                self.history
                    .execute(&mut self.project, &Command::AddClip { track: track_id, clip })?;
            }
        }
        Ok(())
    }
}

impl ControlHandler for HeadlessHandler {
    fn handle(&mut self, call: Call) -> Result<Value, ControlError> {
        Ok(match call {
            Call::ProjectGet => serde_json::to_value(self.state())?,
            Call::ProjectOpen { path } => {
                self.open(&path)?;
                serde_json::to_value(())?
            }
            Call::ProjectSave { path } => serde_json::to_value(self.save(path)?)?,
            Call::CommandApply { command } => {
                self.history.execute(&mut self.project, &command)?;
                serde_json::to_value(())?
            }
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
            Call::MathAdd { params } => serde_json::to_value(self.math_add(params)?)?,
            Call::GraphAdd { params } => serde_json::to_value(self.graph_add(params)?)?,
            Call::ImportManim { params } => serde_json::to_value(self.import_manim(params)?)?,
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

/// Smallest unused identity across all tracks and clips.
fn next_id(project: &Project) -> u64 {
    let mut max = 0_u64;
    for track in project.tracks() {
        max = max.max(track.id().0);
        for clip in track.clips().keys() {
            max = max.max(clip.0);
        }
    }
    max.saturating_add(1)
}

/// A sane fallback duration for clips added without an explicit length.
fn project_duration(project: &Project) -> Time {
    if project.duration().0 > 0 {
        project.duration()
    } else {
        Time(animathio_core::TICKS_PER_SECOND * 3)
    }
}

/// A nonempty half-open range starting at `start` and lasting `duration` ticks.
fn clip_range(start: Time, duration: Time) -> Result<TimeRange, ControlError> {
    if duration.0 <= 0 {
        return Err(ControlError::InvalidParams(
            "clip duration must be positive".to_owned(),
        ));
    }
    let end = start
        .0
        .checked_add(duration.0)
        .ok_or_else(|| ControlError::InvalidParams("clip time range overflow".to_owned()))?;
    Ok(TimeRange::new(start, Time(end))?)
}

/// Directory to scan for `.amplugin` bundles.
///
/// `ANIMATHIO_PLUGIN_DIR` wins; otherwise `target/plugins` is located from the
/// current directory upwards so it works both from the workspace root and from
/// an `apps/animathio-cli` integration-test working directory.
fn plugin_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("ANIMATHIO_PLUGIN_DIR") {
        let dir = PathBuf::from(dir);
        if dir.is_dir() {
            return Some(dir);
        }
    }
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let candidate = dir.join("target").join("plugins");
        if candidate.is_dir() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn plugin_cache_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|base| base.cache_dir().join("animathio").join("plugins"))
        .unwrap_or_else(|| std::env::temp_dir().join("animathio-plugin-cache"))
}

/// Load `.amplugin` bundles from [`plugin_dir`], if any exist.
fn load_plugins() -> Option<PluginHost> {
    let dir = plugin_dir()?;
    let has_plugins = fs::read_dir(&dir)
        .ok()?
        .filter_map(Result::ok)
        .any(|entry| entry.path().extension().is_some_and(|ext| ext == "amplugin"));
    if !has_plugins {
        return None;
    }
    let mut host = match PluginHost::new(plugin_cache_dir()) {
        Ok(host) => host,
        Err(error) => {
            eprintln!("plugin host unavailable: {error}");
            return None;
        }
    };
    match host.load_plugins_dir(&dir) {
        Ok(ids) if !ids.is_empty() => {
            eprintln!("loaded plugins from {}: {}", dir.display(), ids.join(", "));
        }
        Ok(_) => eprintln!("no plugins loaded from {}", dir.display()),
        Err(error) => eprintln!("failed to load plugins from {}: {error}", dir.display()),
    }
    Some(host)
}

#[cfg(test)]
mod tests {
    use super::*;
    use animathio_core::{
        Clip, ClipId, Command, TICKS_PER_SECOND, TimeRange, Track, TrackId, builtin::Builtin,
    };

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
            animathio_core::ClipKind::Builtin(Builtin::Text {
                text: "hi".to_owned(),
                font_size: 0.08,
                color: [255, 255, 255, 255],
            }),
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
}
