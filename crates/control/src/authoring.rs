//! Math and import edits shared by headless and editor control handlers.

use crate::{
    Call, ClipAdded, ControlError, GraphAddParams, ImportManimParams, ImportManimResult,
    MathAddParams,
    path::{read_bounded, validate_existing_file},
};
use animathio_core::{
    AssetId, Clip, ClipId, ClipKind, Command, History, Interpolation, Keyframe, ParamValue,
    Project, Target, Time, TimeRange, Track, TrackId, Transform,
};
use animathio_plugin_host::{ImportRequest, ImportedAsset, PluginHost};
use animathio_typeset::{MathMode, TypesetEngine};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

const LATEX_PLUGIN_ID: &str = "org.animathio.latex";
const GRAPH_PLUGIN_ID: &str = "org.animathio.graph";
/// First-party importer plugin that runs Manim scene import in the sandbox.
pub const MANIM_IMPORT_PLUGIN_ID: &str = "org.animathio.import-manim";
/// Importer id of the Manim format within [`MANIM_IMPORT_PLUGIN_ID`].
pub const MANIM_IMPORTER_ID: &str = "manim";
/// Largest Manim script read from disk (the plugin host caps sources too).
const MAX_MANIM_SOURCE_BYTES: u64 = 16 * 1024 * 1024;

/// Import a Manim script through the sandboxed `org.animathio.import-manim` plugin.
/// `sibling_dir` grants bounded reads of `ImageMobject` files below it.
pub fn import_manim_with_plugin(
    plugins: Option<&PluginHost>,
    source: &str,
    file_name: &str,
    sibling_dir: Option<std::path::PathBuf>,
    scene: Option<&str>,
) -> Result<animathio_plugin_host::ImportedProject, ControlError> {
    let host = plugins
        .filter(|host| {
            host.importers()
                .iter()
                .any(|i| i.plugin_id == MANIM_IMPORT_PLUGIN_ID && i.importer_id == MANIM_IMPORTER_ID)
        })
        .ok_or_else(|| {
            ControlError::Unsupported(format!(
                "Manim import needs the {MANIM_IMPORT_PLUGIN_ID} plugin; build it with scripts/build-plugins.sh"
            ))
        })?;
    let mut params = BTreeMap::new();
    if let Some(scene) = scene {
        params.insert("scene".to_owned(), ParamValue::String(scene.to_owned()));
    }
    Ok(host.import(
        MANIM_IMPORT_PLUGIN_ID,
        MANIM_IMPORTER_ID,
        ImportRequest {
            file_name: file_name.to_owned(),
            bytes: source.as_bytes().to_vec(),
            sibling_dir,
            params,
        },
    )?)
}

/// Apply `math.add`, `graph.add` or `import.manim` to shared editing state.
/// A rejected call leaves project, assets and history unchanged. Successful
/// edits use the same commands and history as normal timeline edits; replacing
/// a project resets history, like opening a project.
pub fn apply_authoring_call(
    project: &mut Project,
    history: &mut History,
    assets: &mut BTreeMap<AssetId, Vec<u8>>,
    call: &Call,
    plugins: Option<&PluginHost>,
) -> Result<Value, ControlError> {
    let mut candidate = project.clone();
    let mut candidate_history = history.clone();
    let mut candidate_assets = BTreeMap::new();
    let mut session = AuthoringSession {
        project: &mut candidate,
        history: &mut candidate_history,
        assets: &mut candidate_assets,
        plugins,
    };
    let result = match call {
        Call::MathAdd { params } => serde_json::to_value(session.math_add(params)?)?,
        Call::GraphAdd { params } => serde_json::to_value(session.graph_add(params)?)?,
        Call::ImportManim { params } => serde_json::to_value(session.import_manim(params)?)?,
        _ => {
            return Err(ControlError::InvalidParams(
                "expected an authoring call".to_owned(),
            ));
        }
    };
    *project = candidate;
    *history = candidate_history;
    if matches!(call, Call::ImportManim { params } if params.replace) {
        *assets = candidate_assets;
    } else {
        for (id, bytes) in candidate_assets {
            assets.entry(id).or_insert(bytes);
        }
    }
    Ok(result)
}

struct AuthoringSession<'a> {
    project: &'a mut Project,
    history: &'a mut History,
    assets: &'a mut BTreeMap<AssetId, Vec<u8>>,
    plugins: Option<&'a PluginHost>,
}

impl AuthoringSession<'_> {
    fn math_add(&mut self, params: &MathAddParams) -> Result<ClipAdded, ControlError> {
        let mode = match params.mode.as_str() {
            "latex" => MathMode::Latex,
            "typst" => MathMode::Typst,
            _ => {
                return Err(ControlError::InvalidParams(
                    "mode must be latex or typst".to_owned(),
                ));
            }
        };
        validate_placement(params.x, params.y)?;
        validate_animation(params.reveal_duration)?;
        #[allow(clippy::cast_possible_truncation)] // typeset validates the f32 size
        TypesetEngine::new()?.typeset(&params.latex, mode, params.size as f32)?;

        let base = next_id(self.project)?;
        let track_id = TrackId(base);
        let clip_id = ClipId(base + 1);
        let start = params.start;
        let duration = params
            .duration
            .unwrap_or_else(|| project_duration(self.project));
        let range = clip_range(start, duration)?;

        let reveal = params.reveal_duration.filter(|d| d.0 > 0);
        let mut numeric = BTreeMap::new();
        numeric.insert(
            "reveal".to_owned(),
            ParamValue::Number(if reveal.is_some() { 0.0 } else { 1.0 }),
        );

        let mut clip = Clip::new(
            clip_id,
            ClipKind::Plugin {
                plugin_id: LATEX_PLUGIN_ID.to_owned(),
                params: json!({
                    "source": params.latex,
                    "mode": params.mode,
                    "units": "pixels",
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

        self.add_plugin_clip(
            track_id,
            clip,
            "Math",
            reveal.map(|duration| ("reveal", duration)),
        )
    }

    fn graph_add(&mut self, params: &GraphAddParams) -> Result<ClipAdded, ControlError> {
        validate_placement(params.x, params.y)?;
        validate_animation(params.draw_duration)?;
        for (min, max) in [
            (params.x_min.unwrap_or(-5.0), params.x_max.unwrap_or(5.0)),
            (params.y_min.unwrap_or(-5.0), params.y_max.unwrap_or(5.0)),
        ] {
            if !min.is_finite() || !max.is_finite() || min >= max {
                return Err(ControlError::InvalidParams(
                    "graph bounds must be finite and increasing".to_owned(),
                ));
            }
        }
        let base = next_id(self.project)?;
        let track_id = TrackId(base);
        let clip_id = ClipId(base + 1);
        let start = params.start;
        let duration = params
            .duration
            .unwrap_or_else(|| project_duration(self.project));
        let range = clip_range(start, duration)?;

        let draw = params.draw_duration.filter(|d| d.0 > 0);
        let mut numeric = BTreeMap::new();
        numeric.insert(
            "draw".to_owned(),
            ParamValue::Number(if draw.is_some() { 0.0 } else { 1.0 }),
        );

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

        self.add_plugin_clip(
            track_id,
            clip,
            "Graph",
            draw.map(|duration| ("draw", duration)),
        )
    }

    fn add_plugin_clip(
        &mut self,
        track_id: TrackId,
        clip: Clip,
        label: &str,
        animation: Option<(&str, Time)>,
    ) -> Result<ClipAdded, ControlError> {
        let clip_id = clip.id;
        let index = self.project.tracks().len();
        // One batch (new layer, clip and animation keyframes) so undo removes it in one step.
        let mut commands = vec![
            Command::AddTrack {
                track: Track::new(track_id, label),
                index,
            },
            Command::AddClip {
                track: track_id,
                clip,
            },
        ];
        if let Some((param, duration)) = animation {
            for (time, value) in [(Time(0), 0.0), (duration, 1.0)] {
                commands.push(Command::AddKeyframe {
                    id: clip_id,
                    target: Target::Param(param.to_owned()),
                    time,
                    keyframe: Keyframe {
                        value: ParamValue::Number(value),
                        interpolation: Interpolation::Linear,
                    },
                });
            }
        }
        self.history
            .execute(self.project, &Command::Batch { commands })?;
        Ok(ClipAdded { track_id, clip_id })
    }

    fn import_manim(
        &mut self,
        params: &ImportManimParams,
    ) -> Result<ImportManimResult, ControlError> {
        let (source, script_dir, file_name) =
            match (params.path.as_deref(), params.source.as_deref()) {
                (Some(path), None) => {
                    let canonical = validate_existing_file(path)?;
                    let bytes = read_bounded(&canonical, MAX_MANIM_SOURCE_BYTES)?;
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
                    let name = canonical
                        .file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                    (source, dir, name)
                }
                (None, Some(source)) => {
                    (source.to_owned(), params.script_dir.clone(), String::new())
                }
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

        let script_dir = script_dir
            .map(|dir| {
                let canonical = dir.canonicalize().map_err(|error| {
                    ControlError::InvalidPath(format!("{}: {error}", dir.display()))
                })?;
                if !canonical.is_dir() {
                    return Err(ControlError::InvalidPath(format!(
                        "not a directory: {}",
                        dir.display()
                    )));
                }
                Ok(canonical)
            })
            .transpose()?;
        let imported = import_manim_with_plugin(
            self.plugins,
            &source,
            &file_name,
            script_dir,
            params.scene.as_deref(),
        )?;
        let warnings = imported.warnings();
        let infos = imported.infos();
        let imported_assets = imported.assets;
        let imported = imported.project;
        let tracks = imported.tracks().len();
        let asset_count = imported_assets.len();

        if params.replace {
            *self.project = imported;
            *self.assets = imported_assets
                .into_iter()
                .map(|asset| (asset.id, asset.bytes))
                .collect();
            *self.history = History::default();
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
        imported_assets: Vec<ImportedAsset>,
    ) -> Result<(), ControlError> {
        for asset in imported_assets {
            if !self.project.assets().contains_key(&asset.id) {
                self.history.execute(
                    self.project,
                    &Command::AddAsset {
                        id: asset.id.clone(),
                        asset: animathio_core::Asset {
                            name: asset.name.clone(),
                            media_type: asset.media_type.clone(),
                        },
                    },
                )?;
            }
            self.assets.entry(asset.id.clone()).or_insert(asset.bytes);
        }

        let mut next_track = self
            .project
            .tracks()
            .iter()
            .map(|track| track.id().0)
            .max()
            .map_or(Some(1), |max| max.checked_add(1))
            .ok_or_else(|| ControlError::InvalidParams("identity overflow".to_owned()))?;
        let mut next_clip = self
            .project
            .tracks()
            .iter()
            .flat_map(|track| track.clips().keys())
            .map(|clip| clip.0)
            .max()
            .map_or(Some(1), |max| max.checked_add(1))
            .ok_or_else(|| ControlError::InvalidParams("identity overflow".to_owned()))?;

        for track in imported.tracks() {
            let track_id = TrackId(next_track);
            next_track = next_track
                .checked_add(1)
                .ok_or_else(|| ControlError::InvalidParams("identity overflow".to_owned()))?;
            let index = self.project.tracks().len();
            self.history.execute(
                self.project,
                &Command::AddTrack {
                    track: Track::new(track_id, track.name().to_owned()),
                    index,
                },
            )?;
            if track.gain().to_bits() != 1.0_f64.to_bits() {
                self.history.execute(
                    self.project,
                    &Command::SetTrackGain {
                        id: track_id,
                        gain: track.gain(),
                    },
                )?;
            }
            for clip in track.clips().values() {
                let clip_id = ClipId(next_clip);
                next_clip = next_clip
                    .checked_add(1)
                    .ok_or_else(|| ControlError::InvalidParams("identity overflow".to_owned()))?;
                let mut clip = clip.clone();
                clip.id = clip_id;
                self.history.execute(
                    self.project,
                    &Command::AddClip {
                        track: track_id,
                        clip,
                    },
                )?;
            }
        }
        Ok(())
    }
}

/// Smallest unused identity across all tracks and clips.
fn next_id(project: &Project) -> Result<u64, ControlError> {
    let mut max = 0_u64;
    for track in project.tracks() {
        max = max.max(track.id().0);
        for clip in track.clips().keys() {
            max = max.max(clip.0);
        }
    }
    max.checked_add(2)
        .map(|_| max + 1)
        .ok_or_else(|| ControlError::InvalidParams("identity overflow".to_owned()))
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

fn validate_placement(x: Option<f64>, y: Option<f64>) -> Result<(), ControlError> {
    if [x, y].into_iter().flatten().any(|value| !value.is_finite()) {
        return Err(ControlError::InvalidParams(
            "position must be finite".to_owned(),
        ));
    }
    Ok(())
}

fn validate_animation(duration: Option<Time>) -> Result<(), ControlError> {
    if duration.is_some_and(|time| time.0 <= 0) {
        return Err(ControlError::InvalidParams(
            "animation duration must be positive".to_owned(),
        ));
    }
    Ok(())
}
