use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use animathio_audio::{AudioPool, Pcm, SymphoniaSource};
use animathio_control::{
    AssetInfo, Call, ControlError, ProjectState, RenderFrameParams, RenderFrameResult,
    media_type_of, read_bounded, timeline_of, validate_existing_file, validate_output_file,
};
use animathio_core::{
    Asset, AssetId, Clip, ClipId, ClipKind, Command, History, Project, TICKS_PER_SECOND, Time,
    TimeRange, Track, TrackId, Transform, TransformProperty,
};
use base64::Engine as _;
use gpui::{prelude::*, *};
use serde_json::Value;

use crate::control::{ControlMsg, ControlRuntime};
use crate::playback::{Clock, due_frame, make_clock};
use crate::render_worker::{RenderRequest, RenderWorker};
use crate::timeline::{self, TimelineLayout, TrimEdge};

const RULER_H: f32 = 28.0;
const ROW_H: f32 = 44.0;
const TIMELINE_H: f32 = 260.0;
const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(60);
const MAX_RECENT: usize = 10;
const DEFAULT_MEDIA_TICKS: i64 = TICKS_PER_SECOND * 2;

/// A project loaded from disk together with its asset bytes and the autosave
/// recovery flag.
type LoadedProject = (Project, BTreeMap<AssetId, Vec<u8>>, bool);

/// An editable value on the selected clip: a transform component or a named
/// numeric parameter.
#[derive(Clone, PartialEq)]
enum Field {
    Transform(TransformProperty),
    Param(String),
}

struct Scrub {
    field: Field,
    start_x: f32,
    start_value: f64,
    current: f64,
}

struct NumberEdit {
    field: Field,
    buffer: String,
}

struct ClipDrag {
    clip_id: ClipId,
    origin_track: TrackId,
    target_track: TrackId,
    origin_start: Time,
    duration: i64,
    grab_offset: i64,
    current_start: Time,
}

struct TrimDrag {
    clip_id: ClipId,
    edge: TrimEdge,
    origin_start: Time,
    origin_end: Time,
    origin_source_offset: Time,
    current: Time,
}

enum TimelineGesture {
    Scrub,
    Drag(ClipDrag),
    Trim(TrimDrag),
}

struct ClipPaint {
    row: usize,
    start: Time,
    end: Time,
    selected: bool,
    label: SharedString,
}

struct TimelineSnapshot {
    duration: i64,
    rows: usize,
    clips: Vec<ClipPaint>,
    position: Time,
}

/// A small on-disk list of recently opened projects, kept in the XDG config dir.
struct RecentFiles {
    paths: Vec<PathBuf>,
}
impl RecentFiles {
    fn config_path() -> Option<PathBuf> {
        directories::ProjectDirs::from("org", "AniMathIO", "AniMathIO")
            .map(|dirs| dirs.config_dir().join("recent.json"))
    }
    fn load() -> Self {
        let paths = Self::config_path()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self { paths }
    }
    fn add(&mut self, path: &Path) {
        self.paths.retain(|existing| existing != path);
        self.paths.insert(0, path.to_path_buf());
        self.paths.truncate(MAX_RECENT);
        self.save();
    }
    fn save(&self) {
        let Some(path) = Self::config_path() else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let Ok(bytes) = serde_json::to_vec_pretty(&self.paths) else {
            return;
        };
        let _ = std::fs::write(path, bytes);
    }
}

pub(crate) struct Editor {
    project: Project,
    history: History,
    assets: BTreeMap<AssetId, Vec<u8>>,
    pool: AudioPool,
    audio_ids: BTreeSet<AssetId>,
    path: Option<PathBuf>,
    clock: Box<dyn Clock>,
    playing: bool,
    position: Time,
    selected: Option<ClipId>,
    selected_field: Option<Field>,
    worker: RenderWorker,
    image: Option<Arc<RenderImage>>,
    request_generation: u64,
    last_requested: Option<Time>,
    timeline_bounds: Bounds<Pixels>,
    timeline_zoom: f32,
    timeline_scroll: f32,
    gesture: Option<TimelineGesture>,
    scrub: Option<Scrub>,
    editing: Option<NumberEdit>,
    error: Option<String>,
    focus_handle: FocusHandle,
    control: Option<ControlRuntime>,
    dirty: bool,
    last_autosave: Instant,
    next_id: u64,
    recent: RecentFiles,
}

impl Editor {
    pub(crate) fn new(
        cx: &mut Context<Self>,
        window: &mut Window,
        project: Project,
        assets: BTreeMap<AssetId, Vec<u8>>,
        path: Option<PathBuf>,
        recovered: bool,
        control: Option<ControlRuntime>,
    ) -> Self {
        let mut pool = AudioPool::default();
        let mut error = None;
        if let Err(message) = load_audio(&mut pool, &project, &assets) {
            error = Some(message);
        }
        let audio_ids = audio_asset_ids(&project);
        let initial_next_id = next_id(&project);
        let clock = make_clock(&project, &pool);
        let worker = RenderWorker::spawn();
        worker.load(assets.clone());
        let mut this = Self {
            project,
            history: History::default(),
            assets,
            pool,
            audio_ids,
            path,
            clock,
            playing: false,
            position: Time(0),
            selected: None,
            selected_field: None,
            worker,
            image: None,
            request_generation: 0,
            last_requested: None,
            timeline_bounds: Bounds::default(),
            timeline_zoom: 1.0,
            timeline_scroll: 0.0,
            gesture: None,
            scrub: None,
            editing: None,
            error,
            focus_handle: cx.focus_handle(),
            control,
            dirty: recovered,
            last_autosave: Instant::now(),
            next_id: initial_next_id,
            recent: RecentFiles::load(),
        };
        if recovered {
            this.error = Some("recovered from autosave".into());
        }
        this.request_render(Time(0));
        window.focus(&this.focus_handle);
        cx.spawn(async move |this, cx| {
            loop {
                let timer = cx.background_executor().timer(Duration::from_secs(1));
                timer.await;
                if this.update(cx, Editor::autosave_tick).is_err() {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
        this
    }

    fn request_render(&mut self, time: Time) {
        self.request_generation = self.request_generation.wrapping_add(1);
        self.last_requested = Some(time);
        let size = project_size(&self.project);
        self.worker.request(RenderRequest {
            generation: self.request_generation,
            project: self.project.clone(),
            time,
            size,
        });
    }

    fn pump_results(&mut self, window: &mut Window) {
        while let Some(result) = self.worker.try_result() {
            if result.generation == self.request_generation
                && let Some(old) = self.image.replace(result.image)
            {
                let _ = window.drop_image(old);
            }
        }
    }

    fn drain_control(&mut self, cx: &mut Context<Self>) {
        loop {
            let next = {
                let Some(control) = self.control.as_mut() else {
                    return;
                };
                control.rx.try_recv()
            };
            let Ok(ControlMsg::Invoke(call, reply)) = next else {
                return;
            };
            let result = self.handle_control_call(call, cx);
            let _ = reply.send(result);
        }
    }

    fn handle_control_call(
        &mut self,
        call: Call,
        cx: &mut Context<Self>,
    ) -> Result<Value, ControlError> {
        Ok(match call {
            Call::ProjectGet => serde_json::to_value(ProjectState {
                project: self.project.clone(),
                path: self.path.as_ref().map(|p| p.display().to_string()),
                playhead: self.position,
                playing: self.playing,
            })?,
            Call::ProjectOpen { path } => {
                self.error = None;
                self.open_path(&path, cx);
                if self.path.as_deref() != Some(path.as_path()) {
                    return Err(ControlError::Other(
                        self.error
                            .take()
                            .unwrap_or_else(|| "open failed".to_owned()),
                    ));
                }
                serde_json::to_value(())?
            }
            Call::ProjectSave { path } => {
                let target = path.or_else(|| self.path.clone()).ok_or_else(|| {
                    ControlError::Unsupported("project has never been saved".to_owned())
                })?;
                let target = validate_output_file(&target)?;
                write_project(&target, &self.project, &self.assets).map_err(ControlError::Other)?;
                self.path = Some(target.clone());
                self.dirty = false;
                self.last_autosave = Instant::now();
                self.recent.add(&target);
                serde_json::to_value(target.display().to_string())?
            }
            Call::CommandApply { command } => {
                self.history
                    .execute(&mut self.project, &command)
                    .map_err(|error| ControlError::Other(error.0))?;
                self.after_edit(cx);
                serde_json::to_value(())?
            }
            Call::HistoryUndo => {
                let undone = self
                    .history
                    .undo(&mut self.project)
                    .map_err(|error| ControlError::Other(error.0))?;
                if undone {
                    self.after_edit(cx);
                }
                serde_json::to_value(undone)?
            }
            Call::HistoryRedo => {
                let redone = self
                    .history
                    .redo(&mut self.project)
                    .map_err(|error| ControlError::Other(error.0))?;
                if redone {
                    self.after_edit(cx);
                }
                serde_json::to_value(redone)?
            }
            Call::RenderFrame { params } => {
                serde_json::to_value(self.render_control_frame(params)?)?
            }
            Call::PlaybackSeek { time } => {
                self.seek_to(time, cx);
                serde_json::to_value(())?
            }
            Call::PlaybackPlay => {
                self.playing = true;
                if let Err(error) = self.clock.play() {
                    self.error = Some(error);
                }
                cx.notify();
                serde_json::to_value(())?
            }
            Call::PlaybackPause => {
                self.playing = false;
                if let Err(error) = self.clock.pause() {
                    self.error = Some(error);
                }
                cx.notify();
                serde_json::to_value(())?
            }
            Call::AssetsImport { path } => serde_json::to_value(self.import_control_asset(&path)?)?,
            Call::TimelineList => serde_json::to_value(timeline_of(&self.project))?,
            Call::ProjectImportV1 { path } => {
                self.import_control_v1(&path)?;
                self.after_edit(cx);
                serde_json::to_value(())?
            }
        })
    }

    fn render_control_frame(
        &self,
        params: RenderFrameParams,
    ) -> Result<RenderFrameResult, ControlError> {
        let mut ctx = animathio_render::RenderCtx::default();
        for (id, bytes) in &self.assets {
            ctx.resources.insert(id.clone(), bytes.clone())?;
        }
        let scene = animathio_core::evaluate(&self.project, params.time);
        let frame = animathio_render::render(&scene, &mut ctx, (params.width, params.height))?;
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

    fn import_control_asset(&mut self, path: &Path) -> Result<AssetInfo, ControlError> {
        let canonical = validate_existing_file(path)?;
        let bytes = read_bounded(&canonical, animathio_format::Limits::default().asset_bytes)?;
        let id = AssetId::of(&bytes);
        let name = canonical
            .file_name()
            .map_or_else(|| "asset".to_owned(), |n| n.to_string_lossy().into_owned());
        self.assets.insert(id.clone(), bytes);
        self.worker.load(self.assets.clone());
        Ok(AssetInfo {
            id,
            media_type: media_type_of(&name).to_owned(),
            name,
        })
    }

    fn import_control_v1(&mut self, path: &Path) -> Result<(), ControlError> {
        let canonical = validate_existing_file(path)?;
        let bytes = read_bounded(
            &canonical,
            animathio_import_v1::ImportLimits::default().inflated_bytes,
        )?;
        let (project, assets, report) =
            animathio_import_v1::import_v1_with_report(&bytes).map_err(ControlError::Import)?;
        for warning in report.warnings() {
            eprintln!("warning: {warning}");
        }
        self.project = project;
        self.history = History::default();
        self.assets = assets.into_iter().map(|a| (a.id, a.bytes)).collect();
        self.path = None;
        self.position = Time(0);
        self.playing = false;
        self.selected = None;
        self.selected_field = None;
        self.pool = AudioPool::default();
        if let Err(error) = load_audio(&mut self.pool, &self.project, &self.assets) {
            self.error = Some(error);
        }
        self.audio_ids = audio_asset_ids(&self.project);
        self.clock = make_clock(&self.project, &self.pool);
        self.next_id = next_id(&self.project);
        self.dirty = true;
        self.worker.load(self.assets.clone());
        Ok(())
    }

    fn current_layout(&self) -> TimelineLayout {
        let width = f32::from(self.timeline_bounds.size.width).max(1.0);
        TimelineLayout::zoomed(
            width,
            RULER_H,
            ROW_H,
            timeline::project_duration(&self.project),
            self.timeline_zoom,
            self.timeline_scroll,
        )
    }

    fn timeline_relative(&self, position: Point<Pixels>) -> (f32, f32) {
        (
            f32::from(position.x) - f32::from(self.timeline_bounds.origin.x),
            f32::from(position.y) - f32::from(self.timeline_bounds.origin.y),
        )
    }

    fn find_clip(&self, id: ClipId) -> Option<&Clip> {
        self.project
            .tracks()
            .iter()
            .flat_map(|t| t.clips().values())
            .find(|c| c.id == id)
    }

    fn current_render_time(&self) -> Time {
        if self.playing {
            due_frame(&self.project, self.clock.now()).unwrap_or(self.position)
        } else {
            self.position
        }
    }

    fn next_clip_id(&mut self) -> ClipId {
        let id = ClipId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    fn execute(&mut self, command: &Command, cx: &mut Context<Self>) {
        match self.history.execute(&mut self.project, command) {
            Ok(()) => self.after_edit(cx),
            Err(error) => self.error = Some(error.0),
        }
    }

    fn after_edit(&mut self, cx: &mut Context<Self>) {
        self.dirty = true;
        self.selected = self.selected.filter(|id| self.find_clip(*id).is_some());
        self.sync_audio();
        let time = self.current_render_time();
        self.request_render(time);
        cx.notify();
    }

    /// Keep the engine-owned audio pool and the player's snapshots in lock-step
    /// with the project. The project snapshot is swapped every edit; the mixer is
    /// only rebuilt when the set of decoded audio assets actually changed.
    fn sync_audio(&mut self) {
        self.pool.collect(&self.project);
        let live = audio_asset_ids(&self.project);
        let Some((rate, channels)) = self.clock.mixer_format() else {
            self.clock.set_project(Arc::new(self.project.clone()));
            return;
        };
        let mut changed = false;
        for id in &live {
            if !self.pool.contains(id) {
                if let Err(message) = load_audio(&mut self.pool, &self.project, &self.assets) {
                    self.error = Some(message);
                }
                changed = true;
                break;
            }
        }
        if changed || live != self.audio_ids {
            match self.pool.prepare(rate, channels) {
                Ok(mixer) => {
                    self.clock.set_mixer(Arc::new(mixer));
                    self.audio_ids = live;
                }
                Err(error) => self.error = Some(error.to_string()),
            }
        }
        self.clock.set_project(Arc::new(self.project.clone()));
    }

    fn autosave_tick(&mut self, cx: &mut Context<Self>) {
        if !self.dirty || self.path.is_none() || self.last_autosave.elapsed() < AUTOSAVE_INTERVAL {
            return;
        }
        let path = autosave_path(self.path.as_ref().expect("checked above"));
        match write_project(&path, &self.project, &self.assets) {
            Ok(()) => self.last_autosave = Instant::now(),
            Err(error) => self.error = Some(format!("autosave failed: {error}")),
        }
        cx.notify();
    }

    pub(crate) fn handle_keystroke(&mut self, event: &KeystrokeEvent, cx: &mut Context<Self>) {
        if self.editing.is_some() {
            self.handle_number_key(event, cx);
            cx.stop_propagation();
            return;
        }

        let key = event.keystroke.key.as_str();
        let modifiers = &event.keystroke.modifiers;
        let handled = match (modifiers.control, modifiers.shift, modifiers.alt, key) {
            (true, false, false, "z") => {
                self.undo(cx);
                true
            }
            (true, true, false, "z") => {
                self.redo(cx);
                true
            }
            (true, false, false, "s") => {
                self.save();
                true
            }
            (true, false, false, "o") => {
                self.open(cx);
                true
            }
            (false, false, false, "space") => {
                self.toggle_play(cx);
                true
            }
            (false, false, false, "delete" | "backspace") => {
                self.delete_selected(cx);
                true
            }
            (false, false, false, "s") => {
                self.split_at_playhead(cx);
                true
            }
            (false, false, false, "up") => {
                self.nudge(1.0, cx);
                true
            }
            (false, false, false, "down") => {
                self.nudge(-1.0, cx);
                true
            }
            _ => false,
        };
        if handled {
            cx.stop_propagation();
        }
    }

    fn handle_number_key(&mut self, event: &KeystrokeEvent, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        match key {
            "enter" => self.commit_editing(cx),
            "escape" => {
                self.editing = None;
                cx.notify();
            }
            "backspace" => {
                if let Some(edit) = &mut self.editing {
                    edit.buffer.pop();
                }
                cx.notify();
            }
            _ => {
                let digit = key.len() == 1 && key.chars().all(|c| c.is_ascii_digit());
                if digit || key == "." || key == "-" {
                    if let Some(edit) = &mut self.editing {
                        edit.buffer.push_str(key);
                    }
                    cx.notify();
                }
            }
        }
    }

    fn commit_editing(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.editing.take() else {
            return;
        };
        let Some(id) = self.selected else {
            return;
        };
        match edit.buffer.trim().parse::<f64>() {
            Ok(value) => self.commit_field(id, &edit.field, value, cx),
            Err(_) => self.error = Some("invalid number".into()),
        }
        cx.notify();
    }

    fn commit_field(&mut self, id: ClipId, field: &Field, value: f64, cx: &mut Context<Self>) {
        if !value.is_finite() {
            self.error = Some("non-finite value".into());
            return;
        }
        let value = match field {
            Field::Transform(TransformProperty::Opacity) => value.clamp(0.0, 1.0),
            _ => value,
        };
        let Some(command) = self.field_command(id, field, value) else {
            return;
        };
        self.execute(&command, cx);
    }

    fn field_command(&self, id: ClipId, field: &Field, value: f64) -> Option<Command> {
        match field {
            Field::Transform(property) => {
                let clip = self.find_clip(id)?;
                Some(Command::SetTransform {
                    id,
                    transform: with_transform(clip.transform, *property, value),
                })
            }
            Field::Param(name) => Some(Command::SetParam {
                id,
                name: name.clone(),
                value: Some(value),
            }),
        }
    }

    fn nudge(&mut self, direction: f64, cx: &mut Context<Self>) {
        let Some(id) = self.selected else {
            return;
        };
        let Some(field) = self.selected_field.clone() else {
            return;
        };
        let Some(clip) = self.find_clip(id).cloned() else {
            return;
        };
        let value = field_value(&clip, &field) + direction * field_step(&field);
        self.commit_field(id, &field, value, cx);
    }

    fn toggle_play(&mut self, cx: &mut Context<Self>) {
        if self.playing {
            if let Err(error) = self.clock.pause() {
                self.error = Some(error);
            }
            self.playing = false;
            self.position = self.clock.now();
        } else {
            if let Err(error) = self.clock.seek(self.position) {
                self.error = Some(error);
            }
            if let Err(error) = self.clock.play() {
                self.error = Some(error);
            }
            self.playing = true;
        }
        cx.notify();
    }

    fn seek_to(&mut self, time: Time, cx: &mut Context<Self>) {
        self.playing = false;
        if let Err(error) = self.clock.pause() {
            self.error = Some(error);
        }
        if let Err(error) = self.clock.seek(time) {
            self.error = Some(error);
        }
        self.position = time;
        self.request_render(time);
        cx.notify();
    }

    fn scrub_to(&mut self, x: f32, cx: &mut Context<Self>) {
        let layout = self.current_layout();
        let time = timeline::x_to_time(&layout, x);
        self.seek_to(time, cx);
    }

    fn undo(&mut self, cx: &mut Context<Self>) {
        match self.history.undo(&mut self.project) {
            Ok(true) => self.after_edit(cx),
            Ok(false) => {}
            Err(error) => self.error = Some(error.0),
        }
    }

    fn redo(&mut self, cx: &mut Context<Self>) {
        match self.history.redo(&mut self.project) {
            Ok(true) => self.after_edit(cx),
            Ok(false) => {}
            Err(error) => self.error = Some(error.0),
        }
    }

    fn delete_selected(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected else {
            return;
        };
        self.execute(&Command::RemoveClip { id }, cx);
        self.selected = None;
        self.selected_field = None;
    }

    fn split_at_playhead(&mut self, cx: &mut Context<Self>) {
        let position = self.position;
        let selected = self.selected.filter(|id| {
            self.find_clip(*id)
                .is_some_and(|c| c.range.start < position && position < c.range.end)
        });
        let target = selected.or_else(|| {
            self.project
                .tracks()
                .iter()
                .flat_map(|t| t.clips().values())
                .find(|c| c.range.start < position && position < c.range.end)
                .map(|c| c.id)
        });
        let Some(id) = target else {
            return;
        };
        let right_id = self.next_clip_id();
        self.execute(
            &Command::SplitClip {
                id,
                right_id,
                at: position,
            },
            cx,
        );
    }

    fn save(&mut self) {
        let path = if let Some(path) = &self.path {
            path.clone()
        } else {
            let Some(path) = rfd::FileDialog::new()
                .add_filter("AniMathIO project", &["amio"])
                .set_file_name("project.amio")
                .save_file()
            else {
                return;
            };
            path
        };
        match write_project(&path, &self.project, &self.assets) {
            Ok(()) => {
                self.path = Some(path.clone());
                self.dirty = false;
                self.last_autosave = Instant::now();
                self.recent.add(&path);
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn open(&mut self, cx: &mut Context<Self>) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("AniMathIO project", &["amio"])
            .pick_file()
        else {
            return;
        };
        self.open_path(&path, cx);
    }

    fn open_path(&mut self, path: &Path, cx: &mut Context<Self>) {
        match read_project_with_recovery(path) {
            Ok((project, assets, recovered)) => {
                self.project = project;
                self.history = History::default();
                self.assets = assets.clone();
                self.path = Some(path.to_path_buf());
                self.dirty = recovered;
                self.last_autosave = Instant::now();
                self.pool = AudioPool::default();
                if let Err(error) = load_audio(&mut self.pool, &self.project, &self.assets) {
                    self.error = Some(error);
                }
                self.audio_ids = audio_asset_ids(&self.project);
                self.clock = make_clock(&self.project, &self.pool);
                self.selected = None;
                self.selected_field = None;
                self.position = Time(0);
                self.playing = false;
                self.next_id = next_id(&self.project);
                self.worker.load(assets);
                self.request_render(Time(0));
                self.recent.add(path);
                if recovered {
                    self.error = Some("recovered from autosave".into());
                }
            }
            Err(error) => self.error = Some(error),
        }
        cx.notify();
    }

    fn import_media(&mut self, cx: &mut Context<Self>) {
        let Some(paths) = rfd::FileDialog::new()
            .add_filter(
                "Media",
                &[
                    "png", "jpg", "jpeg", "webp", "bmp", "mp3", "wav", "flac", "aac", "m4a", "ogg",
                    "mp4", "mov", "mkv", "webm",
                ],
            )
            .pick_files()
        else {
            return;
        };
        let mut changed = false;
        for path in paths {
            match self.add_media_file(&path) {
                Ok(()) => changed = true,
                Err(error) => self.error = Some(error),
            }
        }
        if changed {
            self.after_edit(cx);
        }
    }

    /// Add one media file as an asset and a clip starting at the playhead. Audio
    /// is decoded up front (unsupported audio falls back to a silent source); the
    /// actual clip length is taken from decoded audio when available.
    fn add_media_file(&mut self, path: &Path) -> Result<(), String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let id = AssetId::of(&bytes);
        let media_type = media_type_for(path);
        let name = path
            .file_name()
            .map_or_else(|| "media".into(), |s| s.to_string_lossy().into_owned());
        if !self.project.assets().contains_key(&id) {
            self.history
                .execute(
                    &mut self.project,
                    &Command::AddAsset {
                        id: id.clone(),
                        asset: Asset {
                            name,
                            media_type: media_type.to_owned(),
                        },
                    },
                )
                .map_err(|e| e.0)?;
        }
        self.assets.insert(id.clone(), bytes.clone());
        let (kind, duration) = if media_type.starts_with("audio/") {
            if let Ok(source) = SymphoniaSource::from_bytes(bytes.clone()) {
                let duration = frames_to_ticks(source.pcm().frames(), source.pcm().rate())
                    .max(TICKS_PER_SECOND);
                self.pool
                    .insert(id.clone(), &source)
                    .map_err(|e| e.to_string())?;
                (ClipKind::Audio(id.clone()), duration)
            } else {
                let silent = Pcm::new(48_000, 1, vec![0.0]).map_err(|e| e.to_string())?;
                self.pool
                    .insert(id.clone(), &silent)
                    .map_err(|e| e.to_string())?;
                (ClipKind::Audio(id.clone()), DEFAULT_MEDIA_TICKS)
            }
        } else if media_type.starts_with("image/") {
            (ClipKind::Image(id.clone()), DEFAULT_MEDIA_TICKS)
        } else {
            (ClipKind::Video(id.clone()), DEFAULT_MEDIA_TICKS)
        };
        let start = self.position;
        let end = start
            .0
            .checked_add(duration)
            .ok_or_else(|| "clip end overflow".to_owned())?;
        let track = self
            .project
            .tracks()
            .first()
            .map(Track::id)
            .ok_or_else(|| "no track to place the clip".to_owned())?;
        let clip = Clip::new(
            self.next_clip_id(),
            kind,
            TimeRange::new(start, Time(end)).map_err(|e| e.0)?,
        );
        self.history
            .execute(&mut self.project, &Command::AddClip { track, clip })
            .map_err(|e| e.0)?;
        Ok(())
    }

    fn on_timeline_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (x, y) = self.timeline_relative(event.position);
        let layout = self.current_layout();
        if y < layout.ruler_height {
            self.gesture = Some(TimelineGesture::Scrub);
            self.scrub_to(x, cx);
            return;
        }
        if let Some((hit, edge)) = timeline::hit_trim_edge(&layout, &self.project, x, y)
            && let Some(clip) = self.find_clip(hit.clip).cloned()
        {
            self.selected = Some(hit.clip);
            self.selected_field = None;
            self.gesture = Some(TimelineGesture::Trim(TrimDrag {
                clip_id: hit.clip,
                edge,
                origin_start: clip.range.start,
                origin_end: clip.range.end,
                origin_source_offset: clip.source_offset,
                current: match edge {
                    TrimEdge::Left => clip.range.start,
                    TrimEdge::Right => clip.range.end,
                },
            }));
            cx.notify();
            return;
        }
        if let Some(hit) = timeline::hit_test(&layout, &self.project, x, y) {
            self.selected = Some(hit.clip);
            self.selected_field = None;
            if let Some(clip) = self.find_clip(hit.clip).cloned() {
                let grab_offset = timeline::x_to_time(&layout, x).0 - clip.range.start.0;
                self.gesture = Some(TimelineGesture::Drag(ClipDrag {
                    clip_id: clip.id,
                    origin_track: hit.track,
                    target_track: hit.track,
                    origin_start: clip.range.start,
                    duration: clip.range.end.0 - clip.range.start.0,
                    grab_offset,
                    current_start: clip.range.start,
                }));
            }
        } else {
            self.selected = None;
            self.selected_field = None;
            self.gesture = Some(TimelineGesture::Scrub);
            self.scrub_to(x, cx);
        }
        cx.notify();
    }

    fn on_timeline_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !event.dragging() {
            return;
        }
        let (x, y) = self.timeline_relative(event.position);
        let layout = self.current_layout();
        match &mut self.gesture {
            Some(TimelineGesture::Drag(drag)) => {
                let raw = timeline::drag_start(&layout, drag.grab_offset, x);
                drag.current_start =
                    timeline::snap_start(&layout, &self.project, drag.clip_id, self.position, raw);
                let max_row = self.project.tracks().len().saturating_sub(1) as f32;
                let row = ((y - layout.ruler_height) / layout.row_height.max(1.0))
                    .clamp(0.0, max_row) as usize;
                if let Some(track) = self.project.tracks().get(row) {
                    drag.target_track = track.id();
                }
                cx.notify();
            }
            Some(TimelineGesture::Trim(trim)) => {
                let raw = timeline::x_to_time(&layout, x);
                let snapped =
                    timeline::snap_start(&layout, &self.project, trim.clip_id, self.position, raw);
                trim.current = match trim.edge {
                    TrimEdge::Left => Time(snapped.0.clamp(
                        (trim.origin_start.0 - trim.origin_source_offset.0).max(0),
                        trim.origin_end.0.saturating_sub(1),
                    )),
                    TrimEdge::Right => Time(
                        snapped
                            .0
                            .clamp(trim.origin_start.0.saturating_add(1), i64::MAX),
                    ),
                };
                cx.notify();
            }
            Some(TimelineGesture::Scrub) => self.scrub_to(x, cx),
            None => {}
        }
    }

    fn on_timeline_mouse_up(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.gesture.take() {
            Some(TimelineGesture::Drag(drag)) => {
                if drag.target_track == drag.origin_track && drag.current_start == drag.origin_start
                {
                    return;
                }
                self.execute(
                    &Command::MoveClip {
                        id: drag.clip_id,
                        track: drag.target_track,
                        start: drag.current_start,
                    },
                    cx,
                );
            }
            Some(TimelineGesture::Trim(trim)) => {
                let (range, source_offset) = match trim.edge {
                    TrimEdge::Left => (
                        TimeRange::new(trim.current, trim.origin_end),
                        Time(trim.origin_source_offset.0 + (trim.current.0 - trim.origin_start.0)),
                    ),
                    TrimEdge::Right => (
                        TimeRange::new(trim.origin_start, trim.current),
                        trim.origin_source_offset,
                    ),
                };
                match range {
                    Ok(range) => self.execute(
                        &Command::TrimClip {
                            id: trim.clip_id,
                            range,
                            source_offset,
                        },
                        cx,
                    ),
                    Err(error) => self.error = Some(error.0),
                }
            }
            _ => {}
        }
    }

    fn on_timeline_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (x, _y) = self.timeline_relative(event.position);
        let (dx, dy) = match event.delta {
            ScrollDelta::Pixels(point) => (f32::from(point.x), f32::from(point.y)),
            ScrollDelta::Lines(point) => (point.x * 20.0, point.y * 20.0),
        };
        if event.control {
            let layout = self.current_layout();
            let anchor = timeline::x_to_time(&layout, x);
            let factor = (dy * 0.002).exp();
            self.timeline_zoom = (self.timeline_zoom * factor).clamp(1.0, 64.0);
            let new_layout = self.current_layout();
            let new_x = timeline::time_to_x(&new_layout, anchor);
            self.timeline_scroll += new_x - x;
        } else {
            self.timeline_scroll -= dy;
            if dx != 0.0 {
                self.timeline_scroll -= dx;
            }
        }
        let layout = self.current_layout();
        self.timeline_scroll = self
            .timeline_scroll
            .clamp(0.0, timeline::max_scroll(&layout));
        cx.notify();
    }

    fn on_field_mouse_down(
        &mut self,
        id: ClipId,
        field: &Field,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        let value = self.find_clip(id).map_or(0.0, |c| field_value(c, field));
        self.selected = Some(id);
        self.selected_field = Some(field.clone());
        if event.click_count >= 2 {
            self.editing = Some(NumberEdit {
                field: field.clone(),
                buffer: format!("{value}"),
            });
            self.scrub = None;
        } else {
            self.scrub = Some(Scrub {
                field: field.clone(),
                start_x: f32::from(event.position.x),
                start_value: value,
                current: value,
            });
            self.editing = None;
        }
        cx.notify();
    }

    fn on_field_mouse_move(
        &mut self,
        _id: ClipId,
        field: &Field,
        event: &MouseMoveEvent,
        cx: &mut Context<Self>,
    ) {
        if !event.dragging() {
            return;
        }
        if let Some(scrub) = &mut self.scrub
            && &scrub.field == field
        {
            let delta = f32::from(event.position.x) - scrub.start_x;
            scrub.current = scrub.start_value + f64::from(delta) * field_scrub_scale(field);
            cx.notify();
        }
    }

    fn on_field_mouse_up(
        &mut self,
        _id: ClipId,
        field: &Field,
        _event: &MouseUpEvent,
        cx: &mut Context<Self>,
    ) {
        if let Some(scrub) = self.scrub.take()
            && &scrub.field == field
            && let Some(id) = self.selected
        {
            self.commit_field(id, field, scrub.current, cx);
        }
        cx.notify();
    }

    fn on_open_click(&mut self, _event: &ClickEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.open(cx);
    }

    fn on_save_click(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        self.save();
    }

    fn on_import_click(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.import_media(cx);
    }

    fn on_undo_click(&mut self, _event: &ClickEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.undo(cx);
    }

    fn on_redo_click(&mut self, _event: &ClickEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.redo(cx);
    }

    fn on_play_click(&mut self, _event: &ClickEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_play(cx);
    }

    fn build_timeline_snapshot(&self) -> TimelineSnapshot {
        let duration = timeline::project_duration(&self.project);
        let mut clips = Vec::new();
        for (row, track) in self.project.tracks().iter().enumerate() {
            for clip in track.clips().values() {
                let (start, end) = if let Some(TimelineGesture::Drag(drag)) = &self.gesture
                    && drag.clip_id == clip.id
                {
                    (
                        drag.current_start,
                        Time(drag.current_start.0 + drag.duration),
                    )
                } else if let Some(TimelineGesture::Trim(trim)) = &self.gesture
                    && trim.clip_id == clip.id
                {
                    match trim.edge {
                        TrimEdge::Left => (trim.current, trim.origin_end),
                        TrimEdge::Right => (trim.origin_start, trim.current),
                    }
                } else {
                    (clip.range.start, clip.range.end)
                };
                clips.push(ClipPaint {
                    row,
                    start,
                    end,
                    selected: self.selected == Some(clip.id),
                    label: clip_label(&self.project, clip).into(),
                });
            }
        }
        TimelineSnapshot {
            duration,
            rows: self.project.tracks().len(),
            clips,
            position: self.position,
        }
    }

    fn field_rows(&self, id: ClipId, cx: &mut Context<Self>) -> Vec<Div> {
        let Some(clip) = self.find_clip(id) else {
            return Vec::new();
        };
        let transform = [
            (Field::Transform(TransformProperty::X), "x"),
            (Field::Transform(TransformProperty::Y), "y"),
            (Field::Transform(TransformProperty::ScaleX), "scale x"),
            (Field::Transform(TransformProperty::ScaleY), "scale y"),
            (Field::Transform(TransformProperty::Rotation), "rotation"),
            (Field::Transform(TransformProperty::Opacity), "opacity"),
        ];
        let mut rows = Vec::new();
        for (field, label) in transform {
            let value = field_value(clip, &field);
            rows.push(self.field_row(id, &field, label, value, cx));
        }
        for (name, value) in &clip.params {
            let field = Field::Param(name.clone());
            rows.push(self.field_row(id, &field, name, *value, cx));
        }
        rows
    }

    fn field_row(
        &self,
        id: ClipId,
        field: &Field,
        label: &str,
        value: f64,
        cx: &mut Context<Self>,
    ) -> Div {
        let display = if let Some(edit) = &self.editing
            && &edit.field == field
        {
            edit.buffer.clone()
        } else if let Some(scrub) = &self.scrub
            && &scrub.field == field
        {
            format!("{:.4}", scrub.current)
        } else {
            format!("{value:.4}")
        };

        let down = field.clone();
        let move_field = field.clone();
        let up = field.clone();
        div()
            .flex()
            .flex_row()
            .justify_between()
            .px_2()
            .py_1()
            .child(label.to_owned())
            .child(
                div()
                    .cursor(CursorStyle::ResizeLeftRight)
                    .child(display)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            this.on_field_mouse_down(id, &down, event, cx);
                        }),
                    )
                    .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                        this.on_field_mouse_move(id, &move_field, event, cx);
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseUpEvent, _, cx| {
                            this.on_field_mouse_up(id, &up, event, cx);
                        }),
                    ),
            )
    }

    fn top_bar(&self, cx: &mut Context<Self>) -> Div {
        let play_label = if self.playing { "Pause" } else { "Play" };
        let dirty = if self.dirty { " ●" } else { "" };
        div()
            .h(px(44.))
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_2()
            .bg(rgb(0x1f2430))
            .child(button("open", "Open", cx.listener(Self::on_open_click)))
            .child(button("save", "Save", cx.listener(Self::on_save_click)))
            .child(button(
                "import",
                "Add media…",
                cx.listener(Self::on_import_click),
            ))
            .child(button("undo", "Undo", cx.listener(Self::on_undo_click)))
            .child(button("redo", "Redo", cx.listener(Self::on_redo_click)))
            .child(button("play", play_label, cx.listener(Self::on_play_click)))
            .child(
                div()
                    .px_2()
                    .child(format!("{:.3} s", seconds(self.position))),
            )
            .child(div().text_color(rgb(0x8a94a6)).child(dirty))
    }

    fn inspector(&self, cx: &mut Context<Self>) -> Div {
        let mut pane = div()
            .w(px(260.))
            .h_full()
            .flex()
            .flex_col()
            .p_2()
            .gap_2()
            .bg(rgb(0x151a22));
        let Some(id) = self.selected else {
            pane = pane.child("No clip selected");
            if !self.recent.paths.is_empty() {
                pane = pane.child("Recent projects:");
                for path in self.recent.paths.clone() {
                    let open = path.clone();
                    pane = pane.child(button(
                        "recent",
                        path.display().to_string(),
                        cx.listener(move |this, _event: &ClickEvent, _window, cx| {
                            this.open_path(&open, cx);
                        }),
                    ));
                }
            }
            return pane;
        };
        let Some(clip) = self.find_clip(id) else {
            return pane.child("No clip selected");
        };
        let kind = kind_label(&clip.kind);
        pane = pane.child(format!("Clip {}", id.0)).child(kind);
        let rows = self.field_rows(id, cx);
        pane.children(rows)
    }

    fn status_line(&self) -> Div {
        let error = self.error.clone().unwrap_or_default();
        let path = self
            .path
            .as_ref()
            .map_or_else(|| "untitled".into(), |p| p.display().to_string());
        div()
            .h(px(24.))
            .w_full()
            .flex()
            .flex_row()
            .justify_between()
            .px_2()
            .bg(rgb(0x1f2430))
            .child(format!(
                "{path} | tracks {} | clips {}",
                self.project.tracks().len(),
                self.project
                    .tracks()
                    .iter()
                    .map(|t| t.clips().len())
                    .sum::<usize>()
            ))
            .child(div().text_color(rgb(0xff6b6b)).child(error))
    }
}

impl Focusable for Editor {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.pump_results(window);
        self.drain_control(cx);

        if self.playing {
            let now = self.clock.now();
            self.position = now;
            let due = due_frame(&self.project, now);
            if due != self.last_requested
                && let Some(due) = due
            {
                self.request_render(due);
            }
            window.request_animation_frame();
        }

        let image = self.image.clone();
        let snapshot = self.build_timeline_snapshot();
        let zoom = self.timeline_zoom;
        let scroll = self.timeline_scroll;
        let weak = cx.entity().downgrade();

        let viewport = div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(0x0b0e13));
        let viewport = if let Some(image) = image {
            viewport.child(img(image).w_full().h_full().object_fit(ObjectFit::Contain))
        } else {
            viewport.child("rendering…")
        };

        let timeline = div()
            .h(px(TIMELINE_H))
            .w_full()
            .bg(rgb(0x12161e))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_timeline_mouse_down))
            .on_mouse_move(cx.listener(Self::on_timeline_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_timeline_mouse_up))
            .on_scroll_wheel(cx.listener(Self::on_timeline_scroll_wheel))
            .child(
                canvas(
                    move |bounds, _window, cx| {
                        let _ = weak.update(cx, |this, _| this.timeline_bounds = bounds);
                    },
                    move |bounds, (), window, cx| {
                        paint_timeline(bounds, &snapshot, zoom, scroll, window, cx);
                    },
                )
                .size_full(),
            );

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x0b0e13))
            .text_color(rgb(0xffffff))
            .track_focus(&self.focus_handle)
            .child(self.top_bar(cx))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_row()
                    .child(viewport)
                    .child(self.inspector(cx)),
            )
            .child(timeline)
            .child(self.status_line())
    }
}

fn button(
    id: &'static str,
    label: impl Into<SharedString>,
    listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .px_2()
        .py_1()
        .cursor_pointer()
        .bg(rgb(0x2a3342))
        .child(label.into())
        .id(id)
        .on_click(listener)
}

fn paint_timeline(
    bounds: Bounds<Pixels>,
    snapshot: &TimelineSnapshot,
    zoom: f32,
    scroll: f32,
    window: &mut Window,
    cx: &mut App,
) {
    let width = f32::from(bounds.size.width).max(1.0);
    let layout = TimelineLayout::zoomed(width, RULER_H, ROW_H, snapshot.duration, zoom, scroll);
    window.paint_quad(fill(bounds, rgb(0x12161e)));
    let height = f32::from(bounds.size.height);
    let ruler = Bounds::from_corners(
        bounds.origin,
        point(bounds.origin.x + px(width), bounds.origin.y + px(RULER_H)),
    );
    window.paint_quad(fill(ruler, rgb(0x232c3a)));
    for row in 0..snapshot.rows {
        let top = bounds.origin.y + px(RULER_H + row as f32 * ROW_H);
        let row_bounds = Bounds::from_corners(
            point(bounds.origin.x, top),
            point(bounds.origin.x + px(width), top + px(ROW_H)),
        );
        window.paint_quad(fill(
            row_bounds,
            if row % 2 == 0 {
                rgb(0x1b2230)
            } else {
                rgb(0x202838)
            },
        ));
    }
    for clip in &snapshot.clips {
        let x0 = bounds.origin.x + px(timeline::time_to_x(&layout, clip.start));
        let x1 = bounds.origin.x + px(timeline::time_to_x(&layout, clip.end));
        let y0 = bounds.origin.y + px(RULER_H + clip.row as f32 * ROW_H + 3.0);
        let y1 = bounds.origin.y + px(RULER_H + (clip.row as f32 + 1.0) * ROW_H - 3.0);
        let block = Bounds::from_corners(point(x0, y0), point(x1, y1));
        window.paint_quad(fill(
            block,
            if clip.selected {
                rgb(0x37cbbb)
            } else {
                rgb(0x2f6f8f)
            },
        ));
        // Trim handles at the two edges.
        let handle_w = px(3.0);
        for edge_x in [x0, x1] {
            let handle =
                Bounds::from_corners(point(edge_x - handle_w, y0), point(edge_x + handle_w, y1));
            window.paint_quad(fill(handle, rgb(0xe8e8e8)));
        }
        // Clip label, only when the block is wide enough to hold it.
        if x1 - x0 > px(24.0) && !clip.label.is_empty() {
            let run = TextRun {
                len: clip.label.len(),
                font: font(".SystemUIFont"),
                color: rgb(0xffffff).into(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let line = window
                .text_system()
                .shape_line(clip.label.clone(), px(11.0), &[run], None);
            let _ = line.paint(point(x0 + px(4.0), y0 + px(16.0)), px(14.0), window, cx);
        }
    }
    let head_x = bounds.origin.x + px(timeline::time_to_x(&layout, snapshot.position));
    let head = Bounds::from_corners(
        point(head_x, bounds.origin.y),
        point(head_x + px(1.5), bounds.origin.y + px(height)),
    );
    window.paint_quad(fill(head, rgb(0xe8e8e8)));
}

fn field_value(clip: &Clip, field: &Field) -> f64 {
    match field {
        Field::Transform(property) => match property {
            TransformProperty::X => clip.transform.x,
            TransformProperty::Y => clip.transform.y,
            TransformProperty::ScaleX => clip.transform.scale_x,
            TransformProperty::ScaleY => clip.transform.scale_y,
            TransformProperty::Rotation => clip.transform.rotation,
            TransformProperty::Opacity => clip.transform.opacity,
        },
        Field::Param(name) => clip.params.get(name).copied().unwrap_or(0.0),
    }
}

fn with_transform(mut transform: Transform, property: TransformProperty, value: f64) -> Transform {
    match property {
        TransformProperty::X => transform.x = value,
        TransformProperty::Y => transform.y = value,
        TransformProperty::ScaleX => transform.scale_x = value,
        TransformProperty::ScaleY => transform.scale_y = value,
        TransformProperty::Rotation => transform.rotation = value,
        TransformProperty::Opacity => transform.opacity = value,
    }
    transform
}

fn field_step(field: &Field) -> f64 {
    match field {
        Field::Transform(TransformProperty::Rotation | TransformProperty::Opacity) => 0.05,
        Field::Transform(_) => 0.01,
        Field::Param(_) => 0.1,
    }
}

fn field_scrub_scale(field: &Field) -> f64 {
    match field {
        Field::Transform(_) => 0.001,
        Field::Param(_) => 0.01,
    }
}

fn seconds(time: Time) -> f64 {
    time.0 as f64 / TICKS_PER_SECOND as f64
}

fn kind_label(kind: &ClipKind) -> String {
    match kind {
        ClipKind::Video(_) => "video".into(),
        ClipKind::Image(_) => "image".into(),
        ClipKind::Audio(_) => "audio".into(),
        ClipKind::Builtin(builtin) => match builtin {
            animathio_core::builtin::Builtin::Text { .. } => "text".into(),
            animathio_core::builtin::Builtin::Shape { .. } => "shape".into(),
        },
        ClipKind::Plugin { plugin_id, .. } => format!("plugin {plugin_id}"),
        ClipKind::Vector(_) => "vector".into(),
        ClipKind::Error(_) => "error".into(),
    }
}

fn clip_label(project: &Project, clip: &Clip) -> String {
    let kind = match &clip.kind {
        ClipKind::Video(id) | ClipKind::Image(id) | ClipKind::Audio(id) => {
            project.assets().get(id).map(|a| a.name.clone())
        }
        _ => None,
    };
    kind.unwrap_or_else(|| kind_label(&clip.kind))
}

fn audio_asset_ids(project: &Project) -> BTreeSet<AssetId> {
    project
        .tracks()
        .iter()
        .flat_map(|t| t.clips().values())
        .filter_map(|c| match &c.kind {
            ClipKind::Audio(id) => Some(id.clone()),
            _ => None,
        })
        .collect()
}

fn load_audio(
    pool: &mut AudioPool,
    project: &Project,
    assets: &BTreeMap<AssetId, Vec<u8>>,
) -> Result<(), String> {
    for id in audio_asset_ids(project) {
        if pool.contains(&id) {
            continue;
        }
        let bytes = assets
            .get(&id)
            .ok_or_else(|| format!("audio asset missing: {}", id.as_str()))?;
        if let Ok(source) = SymphoniaSource::from_bytes(bytes.clone()) {
            pool.insert(id, &source).map_err(|e| e.to_string())?;
        } else {
            // An undecodable asset plays silence instead of aborting the mix.
            let silent = Pcm::new(48_000, 1, vec![0.0]).map_err(|e| e.to_string())?;
            pool.insert(id, &silent).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn frames_to_ticks(frames: usize, rate: u32) -> i64 {
    let ticks = u128::from(rate.max(1));
    i64::try_from(frames as u128 * TICKS_PER_SECOND as u128 / ticks).unwrap_or(i64::MAX)
}

fn media_type_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        Some("wav") => "audio/wav",
        Some("mp3") => "audio/mpeg",
        Some("flac") => "audio/flac",
        Some("aac") => "audio/aac",
        Some("m4a") => "audio/mp4",
        Some("ogg") => "audio/ogg",
        Some("mp4") => "video/mp4",
        Some("mov") => "video/quicktime",
        Some("mkv") => "video/x-matroska",
        Some("webm") => "video/webm",
        _ => "application/octet-stream",
    }
}

fn project_size(project: &Project) -> (u32, u32) {
    (
        if project.width() == 0 {
            1280
        } else {
            project.width()
        },
        if project.height() == 0 {
            720
        } else {
            project.height()
        },
    )
}

fn next_id(project: &Project) -> u64 {
    project
        .tracks()
        .iter()
        .flat_map(|t| t.clips().keys().map(|c| c.0))
        .chain(project.tracks().iter().map(|t| t.id().0))
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

fn autosave_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".autosave");
    PathBuf::from(name)
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

pub(crate) fn read_project(path: &Path) -> Result<(Project, BTreeMap<AssetId, Vec<u8>>), String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let bundle = animathio_format::read_amio(
        std::io::BufReader::new(file),
        animathio_format::Limits::default(),
    )
    .map_err(|e| e.to_string())?;
    Ok((bundle.project, bundle.assets))
}

/// Read a project, preferring a newer `<file>.amio.autosave` when it exists so a
/// crash mid-edit can be recovered. Returns the recovered flag alongside the data.
pub(crate) fn read_project_with_recovery(path: &Path) -> Result<LoadedProject, String> {
    let autosave = autosave_path(path);
    let use_autosave = match (modified(path), modified(&autosave)) {
        (Some(main), Some(auto)) => auto > main,
        (None, Some(_)) => true,
        _ => false,
    };
    let source = if use_autosave { &autosave } else { path };
    let (project, assets) = read_project(source)?;
    Ok((project, assets, use_autosave))
}

fn write_project(
    path: &Path,
    project: &Project,
    assets: &BTreeMap<AssetId, Vec<u8>>,
) -> Result<(), String> {
    // Only persist bytes still referenced by the model so undo of an asset add
    // cannot leave orphaned entries that fail the manifest check.
    let referenced: BTreeMap<AssetId, Vec<u8>> = assets
        .iter()
        .filter(|(id, _)| project.assets().contains_key(id))
        .map(|(id, bytes)| (id.clone(), bytes.clone()))
        .collect();
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    animathio_format::write_amio(file, project, &referenced).map_err(|e| e.to_string())?;
    Ok(())
}
