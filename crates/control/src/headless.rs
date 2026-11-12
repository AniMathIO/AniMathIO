//! A [`ControlHandler`] backed entirely by an in-memory project, so the CLI
//! and tests work without a GUI. Assets are kept as content-addressed bytes
//! and fed to a fresh [`animathio_render::RenderCtx`] per frame render.

use std::{
    collections::BTreeMap,
    fs,
    io::BufReader,
    path::{Path, PathBuf},
};

use animathio_core::{AssetId, History, Project, Time, evaluate};
use animathio_format::{Bundle, Limits, read_amio, read_json, write_amio, write_json};
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
#[derive(Clone, Debug)]
pub struct HeadlessHandler {
    project: Project,
    history: History,
    assets: BTreeMap<AssetId, Vec<u8>>,
    path: Option<PathBuf>,
    playhead: Time,
    playing: bool,
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
        }
    }
}

impl HeadlessHandler {
    /// Empty project at the default frame rate.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a handler from an already-loaded bundle.
    #[must_use]
    pub fn from_bundle(bundle: Bundle, path: Option<PathBuf>) -> Self {
        Self {
            project: bundle.project,
            history: History::default(),
            assets: bundle.assets,
            path,
            playhead: Time(0),
            playing: false,
        }
    }

    /// Load a `.amio` archive or a `project.json` with adjacent assets.
    pub fn load(path: &Path) -> Result<Self, ControlError> {
        let (project, assets) = load_project(path)?;
        Ok(Self {
            project,
            history: History::default(),
            assets,
            path: Some(path.to_owned()),
            playhead: Time(0),
            playing: false,
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
        &self,
        params: RenderFrameParams,
    ) -> Result<RenderFrameResult, ControlError> {
        let mut ctx = RenderCtx::from_assets(self.assets.clone())?;
        let scene = evaluate(&self.project, params.time);
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
}

impl ControlHandler for HeadlessHandler {
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
                let bytes = animathio_export::export(
                    &self.project,
                    &self.assets,
                    range,
                    params.settings,
                    |_| true,
                )
                .map_err(|e| ControlError::Other(e.to_string()))?;
                fs::write(&output, bytes)?;
                serde_json::to_value(output.display().to_string())?
            }
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
