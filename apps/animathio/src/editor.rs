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
    Asset, AssetId, Clip, ClipId, ClipKind, Command, FrameRate, History, Interpolation, Project,
    Rounding, Scene, TICKS_PER_SECOND, Target, Time, TimeRange, Track, TrackId, Transform,
    TransformProperty, evaluate,
};
use animathio_plugin_api::PluginKind;
use animathio_plugin_host::PluginHost;
use base64::Engine as _;
use gpui::{prelude::*, *};
use gpui_component::button::{Button, ButtonVariants};
use serde_json::{Value, json};

use crate::control::{ControlMsg, ControlRuntime};
use crate::inspector::{self, ParamModel, PluginDescriptor};
use crate::layout::DockLayout;
use crate::playback::{Clock, due_frame, make_clock};
use crate::render_worker::{RenderRequest, RenderWorker};
use crate::timecode;
use crate::timeline::{self, TimelineLayout, TrimEdge};

const RULER_H: f32 = 28.0;
const ROW_H: f32 = 44.0;
const TIMELINE_H: f32 = 260.0;
const HEADER_W: f32 = 150.0;
const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(60);
const MAX_RECENT: usize = 10;
const DEFAULT_MEDIA_TICKS: i64 = TICKS_PER_SECOND * 2;

/// Viewport benchmark: warm up for two seconds, then scrub to a deterministic
/// random tick at a 60 Hz request cadence for ten seconds and report the p50/p95
/// cadence of delivered frames before quitting.
const BENCH_WARMUP: Duration = Duration::from_secs(2);
const BENCH_DURATION: Duration = Duration::from_secs(10);
const BENCH_SIZE: (u32, u32) = (1920, 1080);

/// State for the `--bench-viewport` flag. It drives the render worker with a
/// deterministic xorshift sequence and measures the cadence of frames that make
/// it through the full presentation path (worker render → BGRA → `RenderImage`
/// upload → draw).
struct ViewportBench {
    rng: u64,
    warmup_started: Instant,
    active_start: Option<Instant>,
    last_delivery: Option<Instant>,
    next_request: Option<Instant>,
    cadence_ms: Vec<f64>,
    done: bool,
}

impl ViewportBench {
    fn new() -> Self {
        Self {
            rng: 0x9e37_79b9_7f4a_7c15,
            warmup_started: Instant::now(),
            active_start: None,
            last_delivery: None,
            next_request: None,
            cadence_ms: Vec::new(),
            done: false,
        }
    }

    /// Deterministic xorshift64* tick in `[0, duration_ticks)`.
    fn next_tick(&mut self, duration_ticks: i64) -> i64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        let span = duration_ticks.max(1) as u64;
        i64::try_from(x % span).expect("tick modulus fits in i64")
    }
}

/// Return the value at the nearest-rank percentile `p` (in `[0, 1]`).
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

/// A project loaded from disk together with its asset bytes and the autosave
/// recovery flag.
type LoadedProject = (Project, BTreeMap<AssetId, Vec<u8>>, bool);

/// A menu-bar action: a closure mutating the editor in its window context.
type MenuAction = Box<dyn Fn(&mut Editor, &mut Context<Editor>)>;

/// Tabs in the left media-pool panel.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LeftTab {
    Assets,
    Plugins,
    Effects,
}

/// Top menu-bar menus.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuId {
    File,
    Edit,
    View,
    Clip,
    Help,
}

/// Dockable panel sides, mirroring [`DockLayout`] flags.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PanelSide {
    Left,
    Right,
    Bottom,
}

/// An editable value on the selected clip: a transform component or a named
/// numeric parameter.
#[derive(Clone, PartialEq)]
enum Field {
    Transform(TransformProperty),
    Param(String),
}

/// Where a typed edit commits once the user presses Enter.
#[derive(Clone, PartialEq)]
enum EditTarget {
    /// Builtin transform/param numeric field.
    Field(Field),
    /// Plugin numeric parameter.
    PluginNumber {
        id: ClipId,
        name: String,
    },
    /// Plugin string parameter.
    PluginText {
        id: ClipId,
        name: String,
    },
    /// Plugin color parameter, edited as `#RRGGBB` / `#RRGGBBAA`.
    PluginColor {
        id: ClipId,
        name: String,
    },
    /// One axis of a plugin point parameter.
    PluginPoint {
        id: ClipId,
        name: String,
        axis: usize,
    },
    SettingsWidth,
    SettingsHeight,
    SettingsFps,
    SettingsBackground,
}

impl EditTarget {
    fn is_numeric(&self) -> bool {
        matches!(
            self,
            Self::Field(_)
                | Self::PluginNumber { .. }
                | Self::PluginPoint { .. }
                | Self::SettingsWidth
                | Self::SettingsHeight
                | Self::SettingsFps
        )
    }
}

#[derive(Clone, Copy)]
enum SettingsField {
    Width,
    Height,
    Fps,
    Background,
}

struct Scrub {
    field: Field,
    start_x: f32,
    start_value: f64,
    current: f64,
}

struct PluginScrub {
    id: ClipId,
    name: String,
    start_x: f32,
    start_value: f64,
    current: f64,
}

struct TextEdit {
    target: EditTarget,
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

struct KeyframeDrag {
    clip_id: ClipId,
    target: Target,
    clip_start: Time,
    origin: Time,
    current: Time,
}

#[derive(Clone, PartialEq)]
struct KeyframeSelection {
    clip_id: ClipId,
    target: Target,
    time: Time,
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

struct KeyframePaint {
    time: Time,
    selected: bool,
}

struct ClipPaint {
    row: usize,
    start: Time,
    end: Time,
    selected: bool,
    label: SharedString,
    error: bool,
    keyframes: Vec<KeyframePaint>,
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
    plugin_scrub: Option<PluginScrub>,
    editing: Option<TextEdit>,
    plugin_host: Option<PluginHost>,
    plugins: Vec<PluginDescriptor>,
    settings_open: bool,
    export_task: Option<crate::export_worker::ExportTask>,
    confirm_overwrite: Option<PathBuf>,
    keyframe_drag: Option<KeyframeDrag>,
    selected_keyframe: Option<KeyframeSelection>,
    error: Option<String>,
    focus_handle: FocusHandle,
    control: Option<ControlRuntime>,
    bench: Option<ViewportBench>,
    dirty: bool,
    last_autosave: Instant,
    next_id: u64,
    recent: RecentFiles,
    dock: DockLayout,
    left_tab: LeftTab,
    menu_open: Option<MenuId>,
    loop_playback: bool,
    snapping: bool,
    timecode_buffer: Option<String>,
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
        bench: bool,
    ) -> Self {
        let mut pool = AudioPool::default();
        let mut error = None;
        if let Err(message) = load_audio(&mut pool, &project, &assets) {
            error = Some(message);
        }
        let (plugin_host, plugins, plugin_error) = load_plugins();
        if error.is_none() {
            error = plugin_error;
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
            plugin_scrub: None,
            editing: None,
            plugin_host,
            plugins,
            settings_open: false,
            export_task: None,
            confirm_overwrite: None,
            keyframe_drag: None,
            selected_keyframe: None,
            error,
            focus_handle: cx.focus_handle(),
            control,
            bench: bench.then(ViewportBench::new),
            dirty: recovered,
            last_autosave: Instant::now(),
            next_id: initial_next_id,
            recent: RecentFiles::load(),
            dock: DockLayout::load(),
            left_tab: LeftTab::Assets,
            menu_open: None,
            loop_playback: false,
            snapping: true,
            timecode_buffer: None,
        };
        if recovered {
            this.error = Some("recovered from autosave".into());
        }
        this.request_render(Time(0));
        window.focus(&this.focus_handle, cx);
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
        let size = if self.bench.is_some() {
            BENCH_SIZE
        } else {
            project_size(&self.project)
        };
        self.request_render_at(time, size);
    }

    fn request_render_at(&mut self, time: Time, size: (u32, u32)) {
        self.request_generation = self.request_generation.wrapping_add(1);
        self.last_requested = Some(time);
        let scene = evaluate(&self.project, time);
        let scene = self.resolve_scene(scene, size);
        self.worker.request(RenderRequest {
            generation: self.request_generation,
            scene,
            size,
        });
    }

    /// Run plugin clips through the plugin host so the render worker only ever
    /// sees media, builtin, vector and error clips.
    fn resolve_scene(&mut self, scene: Scene, size: (u32, u32)) -> Scene {
        match &mut self.plugin_host {
            Some(host) => host.resolve(&scene, size),
            None => scene,
        }
    }

    fn pump_results(&mut self, window: &mut Window) {
        while let Some(result) = self.worker.try_result() {
            if result.generation == self.request_generation {
                if let Some(bench) = &mut self.bench {
                    let now = Instant::now();
                    if let Some(last) = bench.last_delivery.replace(now) {
                        bench.cadence_ms.push((now - last).as_secs_f64() * 1000.0);
                    }
                }
                if let Some(old) = self.image.replace(result.image) {
                    let _ = window.drop_image(old);
                }
            }
        }
    }

    /// Drive the viewport benchmark when `--bench-viewport` is set: warm up,
    /// then scrub at 60 Hz for ten seconds and report the delivered-frame
    /// cadence before quitting.
    fn bench_tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let bench = self
            .bench
            .as_mut()
            .expect("bench tick only runs with a bench");
        if bench.done {
            return;
        }
        let now = Instant::now();
        if bench.active_start.is_none() {
            if now.duration_since(bench.warmup_started) < BENCH_WARMUP {
                window.request_animation_frame();
                return;
            }
            bench.active_start = Some(now);
            bench.last_delivery = Some(now);
            bench.next_request = Some(now);
        }
        let elapsed = now.duration_since(bench.active_start.expect("active start set"));
        if elapsed >= BENCH_DURATION {
            bench.done = true;
            let mut samples = std::mem::take(&mut bench.cadence_ms);
            samples.sort_by(f64::total_cmp);
            let p50 = percentile(&samples, 0.50);
            let p95 = percentile(&samples, 0.95);
            eprintln!(
                "viewport bench: samples={} p50={:.3}ms p95={:.3}ms ({:.2} fps p50)",
                samples.len(),
                p50,
                p95,
                if p50 > 0.0 { 1000.0 / p50 } else { 0.0 }
            );
            cx.quit();
            return;
        }
        // Request a new deterministic random frame at a 60 Hz cadence so the
        // worker (latest-request-wins) is neither starved nor flooded.
        let due = {
            let bench = self
                .bench
                .as_mut()
                .expect("bench tick only runs with a bench");
            if bench.next_request.is_some_and(|next| now >= next) {
                let duration = timeline::project_duration(&self.project).max(1);
                let tick = bench.next_tick(duration);
                bench.next_request = Some(now + Duration::from_micros(16_667));
                Some(tick)
            } else {
                None
            }
        };
        if let Some(tick) = due {
            self.request_render_at(Time(tick), BENCH_SIZE);
        }
        window.request_animation_frame();
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
            if let Call::Export { params } = call {
                if self.export_task.is_some() {
                    let _ = reply.send(Err(ControlError::Other(
                        "an export is already running".into(),
                    )));
                } else {
                    match crate::export_worker::ExportTask::spawn(
                        self.project.clone(),
                        self.assets.clone(),
                        params,
                        Some(reply.clone()),
                    ) {
                        Ok(task) => self.export_task = Some(task),
                        Err(error) => {
                            let _ = reply.send(Err(ControlError::Other(error)));
                        }
                    }
                }
                cx.notify();
                continue;
            }
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
            Call::Export { .. } => {
                return Err(ControlError::Other("export must run on its worker".into()));
            }
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
            call @ (Call::MathAdd { .. } | Call::GraphAdd { .. } | Call::ImportManim { .. }) => {
                let replace = matches!(&call, Call::ImportManim { params } if params.replace);
                let importing = matches!(&call, Call::ImportManim { .. });
                let result = animathio_control::apply_authoring_call(
                    &mut self.project,
                    &mut self.history,
                    &mut self.assets,
                    &call,
                )?;
                if replace {
                    self.path = None;
                    self.selected = None;
                    self.selected_field = None;
                    self.seek_to(Time(0), cx);
                }
                self.next_id = next_id(&self.project);
                if importing {
                    self.worker.load(self.assets.clone());
                }
                self.after_edit(cx);
                result
            }
        })
    }

    fn render_control_frame(
        &mut self,
        params: RenderFrameParams,
    ) -> Result<RenderFrameResult, ControlError> {
        let mut ctx = animathio_render::RenderCtx::from_assets(self.assets.clone())?;
        let scene = animathio_core::evaluate(&self.project, params.time);
        let scene = self.resolve_scene(scene, (params.width, params.height));
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
        if self.timecode_buffer.is_some() {
            self.handle_timecode_key(event, cx);
            cx.stop_propagation();
            return;
        }
        if self.editing.is_some() {
            self.handle_text_key(event, cx);
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

    fn handle_timecode_key(&mut self, event: &KeystrokeEvent, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        match key {
            "enter" => self.commit_timecode(cx),
            "escape" => {
                self.timecode_buffer = None;
                cx.notify();
            }
            "backspace" => {
                if let Some(buffer) = &mut self.timecode_buffer {
                    buffer.pop();
                }
                cx.notify();
            }
            _ => {
                let allowed = key.len() == 1
                    && (key
                        .chars()
                        .all(|c| c.is_ascii_digit() || c == ':' || c == '.'));
                if allowed && let Some(buffer) = &mut self.timecode_buffer {
                    buffer.push_str(key);
                    cx.notify();
                }
            }
        }
    }

    fn handle_text_key(&mut self, event: &KeystrokeEvent, cx: &mut Context<Self>) {
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
                let allowed = match self.editing.as_ref().map(|edit| &edit.target) {
                    Some(target) if target.is_numeric() => {
                        key.len() == 1
                            && (key.chars().all(|c| c.is_ascii_digit()) || key == "." || key == "-")
                    }
                    Some(_) => key.len() == 1,
                    None => false,
                };
                if allowed && let Some(edit) = &mut self.editing {
                    edit.buffer.push_str(key);
                    cx.notify();
                }
            }
        }
    }

    fn commit_editing(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.editing.take() else {
            return;
        };
        match edit.target {
            EditTarget::Field(field) => {
                let Some(id) = self.selected else {
                    return;
                };
                match edit.buffer.trim().parse::<f64>() {
                    Ok(value) => self.commit_field(id, &field, value, cx),
                    Err(_) => self.error = Some("invalid number".into()),
                }
            }
            EditTarget::PluginNumber { id, name } => {
                let Ok(value) = edit.buffer.trim().parse::<f64>() else {
                    self.error = Some("invalid number".into());
                    cx.notify();
                    return;
                };
                if !value.is_finite() {
                    self.error = Some("non-finite value".into());
                    cx.notify();
                    return;
                }
                let value = self
                    .plugin_model(id, &name)
                    .and_then(|model| match model {
                        ParamModel::Number { min, max, .. } => Some(value.clamp(min, max)),
                        _ => None,
                    })
                    .unwrap_or(value);
                if let Some(command) = self.plugin_number_command(id, &name, value) {
                    self.execute(&command, cx);
                }
            }
            EditTarget::PluginText { id, name } => {
                let value = edit.buffer;
                self.execute(&self.plugin_param_command(id, name, json!(value)), cx);
            }
            EditTarget::PluginColor { id, name } => match parse_hex_color(edit.buffer.trim()) {
                Some(color) => self.execute(
                    &self.plugin_param_command(
                        id,
                        name,
                        json!([color[0], color[1], color[2], color[3]]),
                    ),
                    cx,
                ),
                None => self.error = Some("invalid color (use #RRGGBB or #RRGGBBAA)".into()),
            },
            EditTarget::PluginPoint { id, name, axis } => {
                let Ok(value) = edit.buffer.trim().parse::<f64>() else {
                    self.error = Some("invalid number".into());
                    cx.notify();
                    return;
                };
                let mut point = self.plugin_point_value(id, &name);
                point[axis] = value as f32;
                self.execute(
                    &self.plugin_param_command(id, name, json!([point[0], point[1]])),
                    cx,
                );
            }
            EditTarget::SettingsWidth => {
                self.commit_settings_edit(&edit.buffer, SettingsField::Width, cx);
            }
            EditTarget::SettingsHeight => {
                self.commit_settings_edit(&edit.buffer, SettingsField::Height, cx);
            }
            EditTarget::SettingsFps => {
                self.commit_settings_edit(&edit.buffer, SettingsField::Fps, cx);
            }
            EditTarget::SettingsBackground => {
                self.commit_settings_edit(&edit.buffer, SettingsField::Background, cx);
            }
        }
        cx.notify();
    }

    fn plugin_number_command(&self, id: ClipId, name: &str, value: f64) -> Option<Command> {
        let value = self
            .plugin_model(id, name)
            .and_then(|model| match model {
                ParamModel::Number { min, max, .. } => Some(value.clamp(min, max)),
                _ => None,
            })
            .unwrap_or(value);
        let clip = self.find_clip(id)?;
        Some(inspector::numeric_edit_command(
            clip,
            Target::Param(name.to_owned()),
            value,
            self.position,
        ))
    }

    fn plugin_number_value(&self, id: ClipId, name: &str) -> f64 {
        self.plugin_opaque(id, name)
            .and_then(|value| value.as_f64())
            .unwrap_or(0.0)
    }

    fn editing_display(&self, target: &EditTarget) -> Option<String> {
        self.editing
            .as_ref()
            .filter(|edit| &edit.target == target)
            .map(|edit| edit.buffer.clone())
    }

    fn plugin_model(&self, id: ClipId, name: &str) -> Option<ParamModel> {
        let clip = self.find_clip(id)?;
        let ClipKind::Plugin { plugin_id, .. } = &clip.kind else {
            return None;
        };
        self.plugins
            .iter()
            .find(|plugin| &plugin.id == plugin_id)?
            .models()
            .into_iter()
            .find(|model| model.id() == name)
    }

    fn plugin_param_command(&self, id: ClipId, name: String, json: Value) -> Command {
        let model = self
            .plugin_model(id, &name)
            .expect("plugin schema parameter");
        let value = inspector::model_value(&model, Some(&json));
        inspector::param_edit_command(
            self.find_clip(id).expect("selected clip"),
            name,
            value,
            self.position,
        )
    }

    fn plugin_opaque(&self, id: ClipId, name: &str) -> Option<Value> {
        let clip = self.find_clip(id)?;
        let ClipKind::Plugin { params, .. } = &clip.kind else {
            return None;
        };
        let model = self.plugin_model(id, name)?;
        let base = inspector::model_value(&model, params.get(name));
        Some(inspector::sampled_param(clip, name, self.position, base).to_plugin_json())
    }

    fn plugin_text_value(&self, id: ClipId, name: &str, default: &str) -> String {
        self.plugin_opaque(id, name)
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| default.to_owned())
    }

    fn plugin_bool_value(&self, id: ClipId, name: &str, default: bool) -> bool {
        self.plugin_opaque(id, name)
            .and_then(|value| value.as_bool())
            .unwrap_or(default)
    }

    fn plugin_color_value(&self, id: ClipId, name: &str, default: [u8; 4]) -> [u8; 4] {
        self.plugin_opaque(id, name)
            .and_then(|value| color_from_json(&value))
            .unwrap_or(default)
    }

    fn plugin_point_value(&self, id: ClipId, name: &str) -> [f32; 2] {
        self.plugin_opaque(id, name)
            .and_then(|value| point_from_json(&value))
            .unwrap_or([0.0, 0.0])
    }

    fn plugin_asset_value(&self, id: ClipId, name: &str) -> Option<String> {
        self.plugin_opaque(id, name).and_then(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .filter(|value| self.project.assets().contains_key(&asset_id_from(value)))
        })
    }

    fn commit_settings_edit(&mut self, buffer: &str, field: SettingsField, cx: &mut Context<Self>) {
        let mut width = self.project.width();
        let mut height = self.project.height();
        let mut frame_rate = self.project.frame_rate();
        let mut background = self.project.background();
        match field {
            SettingsField::Width => match buffer.trim().parse::<u32>() {
                Ok(value) if value <= 65_535 => width = value,
                _ => {
                    self.error = Some("invalid width".into());
                    return;
                }
            },
            SettingsField::Height => match buffer.trim().parse::<u32>() {
                Ok(value) if value <= 65_535 => height = value,
                _ => {
                    self.error = Some("invalid height".into());
                    return;
                }
            },
            SettingsField::Fps => match parse_fps(buffer.trim()) {
                Ok(value) => frame_rate = value,
                Err(message) => {
                    self.error = Some(message);
                    return;
                }
            },
            SettingsField::Background => {
                if let Some(value) = parse_hex_color(buffer.trim()) {
                    background = value;
                } else {
                    self.error = Some("invalid color (use #RRGGBB or #RRGGBBAA)".into());
                    return;
                }
            }
        }
        self.execute(
            &Command::SetSettings {
                width,
                height,
                frame_rate,
                background,
                duration: self.project.duration(),
            },
            cx,
        );
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

    /// Step the playhead by a whole number of frames.
    fn step_frame(&mut self, delta: i64, cx: &mut Context<Self>) {
        let rate = self.project.frame_rate();
        let Some(frame) = rate
            .time_to_frame(self.position, Rounding::Nearest)
            .ok()
            .and_then(|frame| frame.checked_add(delta))
            .map(|frame| frame.max(0))
        else {
            return;
        };
        let Some(time) = rate.frame_to_time(frame, Rounding::Nearest).ok() else {
            return;
        };
        self.seek_to(time, cx);
    }

    fn jump_to_start(&mut self, cx: &mut Context<Self>) {
        self.seek_to(Time(0), cx);
    }

    fn jump_to_end(&mut self, cx: &mut Context<Self>) {
        let end = timeline::project_duration(&self.project);
        self.seek_to(Time(end), cx);
    }

    fn toggle_loop(&mut self, cx: &mut Context<Self>) {
        self.loop_playback = !self.loop_playback;
        cx.notify();
    }

    fn toggle_snapping(&mut self, cx: &mut Context<Self>) {
        self.snapping = !self.snapping;
        cx.notify();
    }

    fn toggle_panel(&mut self, side: PanelSide, cx: &mut Context<Self>) {
        match side {
            PanelSide::Left => self.dock.left_open = !self.dock.left_open,
            PanelSide::Right => self.dock.right_open = !self.dock.right_open,
            PanelSide::Bottom => self.dock.bottom_open = !self.dock.bottom_open,
        }
        self.dock.save();
        cx.notify();
    }

    fn select_left_tab(&mut self, tab: LeftTab, cx: &mut Context<Self>) {
        self.left_tab = tab;
        cx.notify();
    }

    fn toggle_menu(&mut self, menu: MenuId, cx: &mut Context<Self>) {
        self.menu_open = if self.menu_open == Some(menu) {
            None
        } else {
            Some(menu)
        };
        cx.notify();
    }

    fn begin_timecode_edit(&mut self, cx: &mut Context<Self>) {
        self.timecode_buffer = Some(timecode::format_timecode(
            self.position,
            self.project.frame_rate(),
        ));
        cx.notify();
    }

    fn commit_timecode(&mut self, cx: &mut Context<Self>) {
        let Some(buffer) = self.timecode_buffer.take() else {
            return;
        };
        if let Some(time) = timecode::parse_timecode(&buffer, self.project.frame_rate()) {
            self.seek_to(time, cx);
        } else {
            self.error = Some("invalid timecode (HH:MM:SS:FF)".into());
            cx.notify();
        }
    }

    fn nudge_track_gain(&mut self, track: TrackId, delta: f64, cx: &mut Context<Self>) {
        let Some(current) = self
            .project
            .tracks()
            .iter()
            .find(|t| t.id() == track)
            .map(Track::gain)
        else {
            return;
        };
        let gain = (current + delta).clamp(0.0, 2.0);
        self.execute(&Command::SetTrackGain { id: track, gain }, cx);
    }

    /// Place an already-registered asset onto the first track at the playhead.
    fn add_asset_at_playhead(&mut self, asset_id: AssetId, cx: &mut Context<Self>) {
        let Some(asset) = self.project.assets().get(&asset_id).cloned() else {
            return;
        };
        let kind = if asset.media_type.starts_with("video/") {
            ClipKind::Video(asset_id.clone())
        } else if asset.media_type.starts_with("image/") {
            ClipKind::Image(asset_id.clone())
        } else if asset.media_type.starts_with("audio/") {
            ClipKind::Audio(asset_id.clone())
        } else {
            self.error = Some(format!("cannot place {}", asset.media_type));
            cx.notify();
            return;
        };
        let Some(track) = self.project.tracks().first().map(Track::id) else {
            self.error = Some("no track to place the clip".into());
            cx.notify();
            return;
        };
        let start = self.position;
        let Some(end) = start.0.checked_add(DEFAULT_MEDIA_TICKS) else {
            return;
        };
        let Ok(range) = TimeRange::new(start, Time(end)) else {
            return;
        };
        let clip = Clip::new(self.next_clip_id(), kind, range);
        self.execute(&Command::AddClip { track, clip }, cx);
    }

    /// Add a built-in effect to the selected clip's chain.
    fn add_effect_to_selected(&mut self, kind: animathio_core::EffectKind, cx: &mut Context<Self>) {
        let Some(id) = self.selected else {
            self.error = Some("select a clip to add an effect".into());
            cx.notify();
            return;
        };
        let index = self.find_clip(id).map_or(0, |clip| clip.effects.len());
        let effect =
            animathio_core::EffectInstance::new(animathio_core::EffectId(self.next_id), kind);
        self.next_id = self.next_id.saturating_add(1);
        self.execute(&Command::AddEffect { id, index, effect }, cx);
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
            self.selected_keyframe = None;
            self.gesture = Some(TimelineGesture::Scrub);
            self.scrub_to(x, cx);
            return;
        }
        if let Some(selection) = self.hit_keyframe(&layout, x, y) {
            let clip = self.find_clip(selection.clip_id).cloned();
            self.selected = Some(selection.clip_id);
            self.selected_field = None;
            self.selected_keyframe = Some(selection.clone());
            if let Some(clip) = clip {
                self.keyframe_drag = Some(KeyframeDrag {
                    clip_id: selection.clip_id,
                    target: selection.target.clone(),
                    clip_start: clip.range.start,
                    origin: selection.time,
                    current: selection.time,
                });
            }
            cx.notify();
            return;
        }
        self.selected_keyframe = None;
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

    fn hit_keyframe(&self, layout: &TimelineLayout, x: f32, y: f32) -> Option<KeyframeSelection> {
        let selected = self.selected?;
        if y < layout.ruler_height {
            return None;
        }
        let row = ((y - layout.ruler_height) / layout.row_height.max(1.0)) as usize;
        let mut found = None;
        for (clip_row, clip) in self
            .project
            .tracks()
            .iter()
            .enumerate()
            .flat_map(|(row, track)| track.clips().values().map(move |clip| (row, clip)))
        {
            if clip.id != selected {
                continue;
            }
            if clip_row != row {
                return None;
            }
            for animation in &clip.animations {
                for local in animation.keys.keys().keys() {
                    let time = Time(clip.range.start.0 + local.0);
                    let kx = timeline::time_to_x(layout, time);
                    if (x - kx).abs() <= 6.0 {
                        found = Some(KeyframeSelection {
                            clip_id: selected,
                            target: animation.target.clone(),
                            time: *local,
                        });
                    }
                }
            }
        }
        found
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
        if let Some(drag) = &mut self.keyframe_drag {
            let raw = timeline::x_to_time(&layout, x);
            drag.current = Time((raw.0 - drag.clip_start.0).max(0));
            cx.notify();
            return;
        }
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
        if let Some(drag) = self.keyframe_drag.take() {
            self.selected_keyframe = Some(KeyframeSelection {
                clip_id: drag.clip_id,
                target: drag.target.clone(),
                time: drag.current,
            });
            if drag.current != drag.origin {
                self.execute(
                    &Command::MoveKeyframe {
                        id: drag.clip_id,
                        target: drag.target,
                        from: drag.origin,
                        to: drag.current,
                    },
                    cx,
                );
            }
            cx.notify();
            return;
        }
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
        event: &ScrollEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (x, _y) = self.timeline_relative(event.position);
        let (dx, dy) = match event.delta {
            ScrollDelta::Pixels(point) => (f32::from(point.x), f32::from(point.y)),
            ScrollDelta::Lines(point) => (point.x * 20.0, point.y * 20.0),
        };
        if event.modifiers.control {
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
            self.editing = Some(TextEdit {
                target: EditTarget::Field(field.clone()),
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

    fn add_plugin_clip(&mut self, descriptor: &PluginDescriptor, cx: &mut Context<Self>) {
        let models = descriptor.models();
        let (numeric, opaque) = inspector::default_params(&models);
        let Some(track) = self.project.tracks().first() else {
            self.error = Some("no track to place the clip".into());
            cx.notify();
            return;
        };
        let track = track.id();
        let start = self.position;
        let duration = DEFAULT_MEDIA_TICKS;
        let Some(end) = start.0.checked_add(duration) else {
            self.error = Some("clip end overflow".into());
            cx.notify();
            return;
        };
        let Ok(range) = TimeRange::new(start, Time(end)) else {
            self.error = Some("invalid clip range".into());
            cx.notify();
            return;
        };
        let mut clip = Clip::new(
            self.next_clip_id(),
            ClipKind::Plugin {
                plugin_id: descriptor.id.clone(),
                params: opaque,
            },
            range,
        );
        clip.params = numeric;
        self.execute(&Command::AddClip { track, clip }, cx);
    }

    fn add_plugin_clip_by_id(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(descriptor) = self.plugins.iter().find(|plugin| plugin.id == id) else {
            return;
        };
        let descriptor = descriptor.clone();
        self.add_plugin_clip(&descriptor, cx);
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
                let mut keyframes = Vec::new();
                if self.selected == Some(clip.id) {
                    for animation in &clip.animations {
                        for local in animation.keys.keys().keys() {
                            let time = if let Some(drag) = &self.keyframe_drag
                                && drag.clip_id == clip.id
                                && drag.target == animation.target
                                && drag.origin == *local
                            {
                                drag.current
                            } else {
                                *local
                            };
                            keyframes.push(KeyframePaint {
                                time: Time(clip.range.start.0 + time.0),
                                selected: self.selected_keyframe.as_ref().is_some_and(
                                    |selection| {
                                        selection.clip_id == clip.id
                                            && selection.target == animation.target
                                            && selection.time == time
                                    },
                                ),
                            });
                        }
                    }
                }
                clips.push(ClipPaint {
                    row,
                    start,
                    end,
                    selected: self.selected == Some(clip.id),
                    label: clip_label(&self.project, clip).into(),
                    error: matches!(clip.kind, ClipKind::Error(_)),
                    keyframes,
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

    fn transform_rows(&self, id: ClipId, cx: &mut Context<Self>) -> Vec<Div> {
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
        rows
    }

    fn numeric_param_rows(&self, id: ClipId, cx: &mut Context<Self>) -> Vec<Div> {
        let Some(clip) = self.find_clip(id) else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        for (name, value) in &clip.params {
            let field = Field::Param(name.clone());
            if let Some(value) = value.as_number() {
                rows.push(self.field_row(id, &field, name, value, cx));
            }
        }
        rows
    }

    fn plugin_rows(&self, id: ClipId, cx: &mut Context<Self>) -> Vec<Div> {
        let Some(clip) = self.find_clip(id) else {
            return Vec::new();
        };
        let ClipKind::Plugin { plugin_id, .. } = &clip.kind else {
            return Vec::new();
        };
        let Some(descriptor) = self.plugins.iter().find(|plugin| &plugin.id == plugin_id) else {
            return vec![div().px_2().child("plugin not loaded")];
        };
        let models = descriptor.models();
        let mut rows = Vec::new();
        for model in &models {
            rows.push(self.plugin_row(id, model, cx));
        }
        rows
    }

    fn plugin_row(&self, id: ClipId, model: &ParamModel, cx: &mut Context<Self>) -> Div {
        let name = model.id().to_owned();
        let diamond = self.keyframe_diamond(id, Target::Param(name.clone()), cx);
        let control = match model {
            ParamModel::Number { .. } => self.plugin_number_control(id, name.clone(), cx),
            ParamModel::Color { default, .. } => self.plugin_color_control(
                id,
                name.clone(),
                [default.r, default.g, default.b, default.a],
                cx,
            ),
            ParamModel::Text { default, .. } => {
                self.plugin_text_control(id, name.clone(), default, cx)
            }
            ParamModel::Enum {
                options, default, ..
            } => self.plugin_enum_control(id, name.clone(), options.clone(), default, cx),
            ParamModel::Bool { default, .. } => {
                self.plugin_bool_control(id, name.clone(), *default, cx)
            }
            ParamModel::Point { default, .. } => {
                self.plugin_point_control(id, name.clone(), *default, cx)
            }
            ParamModel::Asset { media_type, .. } => {
                self.plugin_asset_control(id, name.clone(), media_type, cx)
            }
        };
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .child(diamond)
            .child(div().flex_1().child(model.label().to_owned()))
            .child(control)
    }

    fn keyframe_diamond(
        &self,
        id: ClipId,
        target: Target,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let active = self
            .find_clip(id)
            .is_some_and(|clip| inspector::keyframed(clip, &target, self.position));
        let label = if active { "◆" } else { "◇" };
        div()
            .w(px(14.))
            .h(px(14.))
            .cursor_pointer()
            .child(label)
            .id("keyframe-diamond")
            .on_click(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
                this.toggle_keyframe(id, target.clone(), cx);
            }))
    }

    fn toggle_keyframe(&mut self, id: ClipId, target: Target, cx: &mut Context<Self>) {
        let Some(clip) = self.find_clip(id) else {
            return;
        };
        let command = if let Target::Param(name) = &target
            && let Some(model) = self.plugin_model(id, name)
        {
            let raw = if let ClipKind::Plugin { params, .. } = &clip.kind {
                params.get(name)
            } else {
                None
            };
            inspector::param_toggle_command(
                clip,
                name.clone(),
                self.position,
                inspector::model_value(&model, raw),
            )
        } else {
            inspector::keyframe_toggle_command(clip, target, self.position)
        };
        self.execute(&command, cx);
    }

    fn interpolation_command(
        &self,
        selection: &KeyframeSelection,
        interpolation: Interpolation,
    ) -> Option<Command> {
        let clip = self.find_clip(selection.clip_id)?;
        inspector::interpolation_command(
            clip,
            selection.target.clone(),
            selection.time,
            interpolation,
        )
    }

    fn keyframe_menu(
        &self,
        selection: &KeyframeSelection,
        cx: &mut Context<Self>,
    ) -> Vec<Stateful<Div>> {
        let mut rows = vec![
            div()
                .px_2()
                .child("Keyframe interpolation")
                .id("keyframe-menu-title"),
        ];
        let current = self.find_clip(selection.clip_id).and_then(|clip| {
            inspector::keyframe_interpolation(clip, &selection.target, selection.time)
        });
        for (label, interpolation) in [
            ("linear", Interpolation::Linear),
            ("hold", Interpolation::Hold),
            ("ease", Interpolation::EaseInOut),
        ] {
            let active = current == Some(interpolation);
            let selection = selection.clone();
            rows.push(
                div()
                    .px_2()
                    .cursor_pointer()
                    .bg(if active { rgb(0x2a3342) } else { rgb(0x151a22) })
                    .child(label)
                    .id("keyframe-interp")
                    .on_click(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
                        if let Some(command) = this.interpolation_command(&selection, interpolation)
                        {
                            this.execute(&command, cx);
                        }
                    })),
            );
        }
        rows
    }

    fn plugin_number_control(
        &self,
        id: ClipId,
        name: String,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let value = self.plugin_number_value(id, &name);
        let target = EditTarget::PluginNumber {
            id,
            name: name.clone(),
        };
        let display = if let Some(buffer) = self.editing_display(&target) {
            buffer
        } else if let Some(scrub) = &self.plugin_scrub
            && scrub.id == id
            && scrub.name == name
        {
            format!("{:.4}", scrub.current)
        } else {
            format!("{value:.4}")
        };

        let down = name.clone();
        let move_name = name.clone();
        let up = name.clone();
        div()
            .cursor(CursorStyle::ResizeLeftRight)
            .child(display)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                    this.on_plugin_number_down(id, &down, event, cx);
                }),
            )
            .on_mouse_move(
                cx.listener(move |this, event: &MouseMoveEvent, _window, cx| {
                    this.on_plugin_number_move(id, &move_name, event, cx);
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseUpEvent, _window, cx| {
                    this.on_plugin_number_up(id, &up, event, cx);
                }),
            )
            .id("plugin-number")
    }

    fn on_plugin_number_down(
        &mut self,
        id: ClipId,
        name: &str,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        let value = self.plugin_number_value(id, name);
        if event.click_count >= 2 {
            self.editing = Some(TextEdit {
                target: EditTarget::PluginNumber {
                    id,
                    name: name.to_owned(),
                },
                buffer: format!("{value}"),
            });
            self.plugin_scrub = None;
        } else {
            self.plugin_scrub = Some(PluginScrub {
                id,
                name: name.to_owned(),
                start_x: f32::from(event.position.x),
                start_value: value,
                current: value,
            });
            self.editing = None;
        }
        cx.notify();
    }

    fn on_plugin_number_move(
        &mut self,
        id: ClipId,
        name: &str,
        event: &MouseMoveEvent,
        cx: &mut Context<Self>,
    ) {
        if !event.dragging() {
            return;
        }
        let step = self
            .plugin_model(id, name)
            .and_then(|model| match model {
                ParamModel::Number { step, .. } => Some(step),
                _ => None,
            })
            .unwrap_or(0.1);
        let Some(scrub) = &mut self.plugin_scrub else {
            return;
        };
        if scrub.id != id || scrub.name != name {
            return;
        }
        let delta = f64::from(f32::from(event.position.x) - scrub.start_x);
        scrub.current = scrub.start_value + delta * step * 0.05;
        cx.notify();
    }

    fn on_plugin_number_up(
        &mut self,
        id: ClipId,
        name: &str,
        _event: &MouseUpEvent,
        cx: &mut Context<Self>,
    ) {
        if let Some(scrub) = self.plugin_scrub.take()
            && scrub.id == id
            && scrub.name == name
            && let Some(command) = self.plugin_number_command(id, name, scrub.current)
        {
            self.execute(&command, cx);
        }
        cx.notify();
    }

    fn plugin_text_control(
        &self,
        id: ClipId,
        name: String,
        default: &str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let value = self.plugin_text_value(id, &name, default);
        let target = EditTarget::PluginText {
            id,
            name: name.clone(),
        };
        let display = self
            .editing_display(&target)
            .unwrap_or_else(|| value.clone());
        let current = value.clone();
        div()
            .cursor_pointer()
            .child(display)
            .id("plugin-text")
            .on_click(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
                this.editing = Some(TextEdit {
                    target: EditTarget::PluginText {
                        id,
                        name: name.clone(),
                    },
                    buffer: current.clone(),
                });
                cx.notify();
            }))
    }

    fn plugin_enum_control(
        &self,
        id: ClipId,
        name: String,
        options: Vec<String>,
        default: &str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let current = self.plugin_text_value(id, &name, default);
        let options_for_click = options.clone();
        div()
            .cursor_pointer()
            .child(current.clone())
            .id("plugin-enum")
            .on_click(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
                let next = cycle_option(&options_for_click, &current);
                this.execute(
                    &this.plugin_param_command(id, name.clone(), json!(next)),
                    cx,
                );
            }))
    }

    fn plugin_bool_control(
        &self,
        id: ClipId,
        name: String,
        default: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let value = self.plugin_bool_value(id, &name, default);
        let label = if value { "on" } else { "off" };
        div()
            .cursor_pointer()
            .child(label)
            .id("plugin-bool")
            .on_click(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
                this.execute(
                    &this.plugin_param_command(id, name.clone(), json!(!value)),
                    cx,
                );
            }))
    }

    fn plugin_color_control(
        &self,
        id: ClipId,
        name: String,
        default: [u8; 4],
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let color = self.plugin_color_value(id, &name, default);
        let hex = format!(
            "#{:02x}{:02x}{:02x}{:02x}",
            color[0], color[1], color[2], color[3]
        );
        let target = EditTarget::PluginColor {
            id,
            name: name.clone(),
        };
        let display = self.editing_display(&target).unwrap_or_else(|| hex.clone());
        let swatch =
            rgb(u32::from(color[0]) << 16 | u32::from(color[1]) << 8 | u32::from(color[2]));
        let name_for_click = name.clone();
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .child(div().w(px(16.)).h(px(12.)).bg(swatch))
            .child(
                div()
                    .cursor_pointer()
                    .child(display)
                    .id("plugin-color")
                    .on_click(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
                        this.editing = Some(TextEdit {
                            target: EditTarget::PluginColor {
                                id,
                                name: name.clone(),
                            },
                            buffer: hex.clone(),
                        });
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .cursor_pointer()
                    .child("↻")
                    .id("plugin-color-cycle")
                    .on_click(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
                        let next = cycle_color(color);
                        this.execute(
                            &this.plugin_param_command(
                                id,
                                name_for_click.clone(),
                                json!([next[0], next[1], next[2], next[3]]),
                            ),
                            cx,
                        );
                    })),
            )
            .id("plugin-color-row")
    }

    fn plugin_point_control(
        &self,
        id: ClipId,
        name: String,
        default: animathio_plugin_api::PointValue,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let point = self.plugin_point_value(id, &name);
        let x_name = name.clone();
        let y_name = name.clone();
        div()
            .flex()
            .flex_row()
            .gap_1()
            .child(
                div()
                    .cursor_pointer()
                    .child(format!("x {:.2}", point[0]))
                    .id("plugin-point-x")
                    .on_click(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
                        this.editing = Some(TextEdit {
                            target: EditTarget::PluginPoint {
                                id,
                                name: x_name.clone(),
                                axis: 0,
                            },
                            buffer: format!("{:.2}", point[0]),
                        });
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .cursor_pointer()
                    .child(format!("y {:.2}", point[1]))
                    .id("plugin-point-y")
                    .on_click(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
                        this.editing = Some(TextEdit {
                            target: EditTarget::PluginPoint {
                                id,
                                name: y_name.clone(),
                                axis: 1,
                            },
                            buffer: format!("{:.2}", point[1]),
                        });
                        cx.notify();
                    })),
            )
            .child(div().px_1().child(format!("({} {})", default.x, default.y)))
            .id("plugin-point-row")
    }

    fn plugin_asset_control(
        &self,
        id: ClipId,
        name: String,
        media_type: &str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let current = self.plugin_asset_value(id, &name);
        let label = current.as_ref().map_or_else(
            || "none".to_owned(),
            |asset| {
                self.project
                    .assets()
                    .get(&asset_id_from(asset))
                    .map_or_else(|| "unknown".to_owned(), |meta| meta.name.clone())
            },
        );
        let media_type = media_type.to_owned();
        div()
            .cursor_pointer()
            .child(label)
            .id("plugin-asset")
            .on_click(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
                let ids = this.matching_asset_ids(&media_type);
                let next = current
                    .as_deref()
                    .and_then(|current| {
                        let index = ids.iter().position(|asset| asset.as_str() == current)?;
                        ids.get(index + 1).or_else(|| ids.first())
                    })
                    .or_else(|| ids.first())
                    .cloned();
                this.execute(
                    &this.plugin_param_command(
                        id,
                        name.clone(),
                        next.map_or(Value::Null, |asset| json!(asset.as_str())),
                    ),
                    cx,
                );
            }))
    }

    fn settings_dialog(&self, cx: &mut Context<Self>) -> Div {
        let width = self.project.width();
        let height = self.project.height();
        let fps = frame_rate_label(self.project.frame_rate());
        let background = self.project.background();
        let hex = format!(
            "#{:02x}{:02x}{:02x}{:02x}",
            background[0], background[1], background[2], background[3]
        );
        let swatch = rgb(u32::from(background[0]) << 16
            | u32::from(background[1]) << 8
            | u32::from(background[2]));
        div()
            .absolute()
            .top(px(52.))
            .right(px(12.))
            .w(px(280.))
            .bg(rgb(0x1f2430))
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .child(self.settings_field("Width", width.to_string(), EditTarget::SettingsWidth, cx))
            .child(self.settings_field(
                "Height",
                height.to_string(),
                EditTarget::SettingsHeight,
                cx,
            ))
            .child(self.settings_field("Frame rate", fps, EditTarget::SettingsFps, cx))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(div().w(px(16.)).h(px(12.)).bg(swatch))
                    .child(self.settings_field(
                        "Background",
                        hex,
                        EditTarget::SettingsBackground,
                        cx,
                    )),
            )
            .child(
                div()
                    .cursor_pointer()
                    .px_2()
                    .child("Close")
                    .id("settings-close")
                    .on_click(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
                        this.settings_open = false;
                        cx.notify();
                    })),
            )
    }

    fn settings_field(
        &self,
        label: &str,
        value: String,
        target: EditTarget,
        cx: &mut Context<Self>,
    ) -> Div {
        let display = if let Some(edit) = &self.editing
            && edit.target == target
        {
            edit.buffer.clone()
        } else {
            value.clone()
        };
        div()
            .flex()
            .flex_row()
            .justify_between()
            .px_2()
            .py_1()
            .child(label.to_owned())
            .child(
                div()
                    .cursor_pointer()
                    .child(display)
                    .id("settings-field")
                    .on_click(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
                        this.editing = Some(TextEdit {
                            target: target.clone(),
                            buffer: value.clone(),
                        });
                        cx.notify();
                    })),
            )
    }

    fn matching_asset_ids(&self, media_type: &str) -> Vec<AssetId> {
        let prefix = media_type.strip_suffix("/*").unwrap_or(media_type);
        let mut ids: Vec<AssetId> = self
            .project
            .assets()
            .iter()
            .filter(|(_, asset)| asset.media_type.starts_with(prefix))
            .map(|(id, _)| id.clone())
            .collect();
        ids.sort();
        ids
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
            && edit.target == EditTarget::Field(field.clone())
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

    fn open_export_dialog(&mut self, cx: &mut Context<Self>) {
        if self.export_task.is_some() {
            return;
        }
        let Some(output) = rfd::FileDialog::new()
            .add_filter("MP4 video", &["mp4"])
            .set_file_name("video.mp4")
            .save_file()
        else {
            return;
        };
        // Export never overwrites silently: ask before replacing an existing file.
        if output.exists() {
            self.confirm_overwrite = Some(output);
            cx.notify();
            return;
        }
        self.start_export(output, false, cx);
    }

    /// Spawn the export worker; the export dialog shows progress and carries the
    /// confirm-overwrite prompt.
    fn start_export(&mut self, output: PathBuf, overwrite: bool, cx: &mut Context<Self>) {
        if self.export_task.is_some() {
            return;
        }
        let params = animathio_control::ExportParams {
            output,
            from: Time(0),
            to: None,
            settings: animathio_export::Settings::default(),
            overwrite,
        };
        match crate::export_worker::ExportTask::spawn(
            self.project.clone(),
            self.assets.clone(),
            params,
            None,
        ) {
            Ok(task) => self.export_task = Some(task),
            Err(error) => self.error = Some(error),
        }
        cx.notify();
    }

    fn toggle_settings(&mut self, cx: &mut Context<Self>) {
        self.settings_open = !self.settings_open;
        cx.notify();
    }

    fn export_dialog(&self, cx: &mut Context<Self>) -> Div {
        use std::sync::atomic::Ordering;
        let Some(task) = &self.export_task else {
            let Some(path) = &self.confirm_overwrite else {
                return div();
            };
            return div()
                .absolute()
                .top(px(60.))
                .left(px(30.))
                .w(px(280.))
                .p_3()
                .bg(rgb(0x1f2430))
                .flex()
                .flex_col()
                .gap_2()
                .child("Replace existing file?")
                .child(
                    div()
                        .text_color(rgb(0x8a94a6))
                        .child(path.display().to_string()),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .child(button(
                            "confirm-overwrite",
                            "Replace",
                            cx.listener(|this, _: &ClickEvent, _, cx| {
                                if let Some(path) = this.confirm_overwrite.take() {
                                    this.start_export(path, true, cx);
                                }
                                cx.notify();
                            }),
                        ))
                        .child(button(
                            "cancel-overwrite",
                            "Cancel",
                            cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.confirm_overwrite = None;
                                cx.notify();
                            }),
                        )),
                );
        };
        let completed = task.completed.load(Ordering::Relaxed);
        let total = task.total.load(Ordering::Relaxed);
        let width = if total == 0 {
            0.0
        } else {
            240.0 * completed as f32 / total as f32
        };
        div()
            .absolute()
            .top(px(60.))
            .left(px(30.))
            .w(px(280.))
            .p_3()
            .bg(rgb(0x1f2430))
            .flex()
            .flex_col()
            .gap_2()
            .child("Exporting MP4")
            .child(format!("{completed} / {total} frames"))
            .child(
                div()
                    .w(px(240.))
                    .h(px(8.))
                    .bg(rgb(0x394354))
                    .child(div().w(px(width)).h(px(8.)).bg(rgb(0x5eabff))),
            )
            .child(button(
                "cancel-export",
                "Cancel",
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    if let Some(task) = &this.export_task {
                        task.cancel.store(true, Ordering::Relaxed);
                    }
                    cx.notify();
                }),
            ))
    }

    fn menu_bar(&self, cx: &mut Context<Self>) -> Div {
        let dirty = if self.dirty { " ●" } else { "" };
        let title = self.path.as_ref().map_or_else(
            || "Untitled".to_owned(),
            |p| {
                p.file_name().map_or_else(
                    || p.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                )
            },
        );
        div()
            .h(px(34.))
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .px_1()
            .bg(rgb(0x171c26))
            .child(
                Button::new("menu-file")
                    .ghost()
                    .compact()
                    .label("File")
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_menu(MenuId::File, cx))),
            )
            .child(
                Button::new("menu-edit")
                    .ghost()
                    .compact()
                    .label("Edit")
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_menu(MenuId::Edit, cx))),
            )
            .child(
                Button::new("menu-view")
                    .ghost()
                    .compact()
                    .label("View")
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_menu(MenuId::View, cx))),
            )
            .child(
                Button::new("menu-clip")
                    .ghost()
                    .compact()
                    .label("Clip")
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_menu(MenuId::Clip, cx))),
            )
            .child(
                Button::new("menu-help")
                    .ghost()
                    .compact()
                    .label("Help")
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_menu(MenuId::Help, cx))),
            )
            .child(div().flex_1())
            .child(
                div()
                    .text_color(rgb(0xc6cedb))
                    .child(format!("{title}{dirty}")),
            )
    }

    /// A dropdown listing the actions of the open menu, or `None` when closed.
    fn menu_dropdown(&self, cx: &mut Context<Self>) -> Option<Div> {
        let menu = self.menu_open?;
        let items: Vec<(&str, MenuAction)> = match menu {
            MenuId::File => vec![
                ("Open… Ctrl+O", Box::new(Self::open)),
                ("Save Ctrl+S", Box::new(|this, _cx| this.save())),
                ("Import media…", Box::new(Self::import_media)),
                ("Export…", Box::new(Self::open_export_dialog)),
                ("Settings", Box::new(Self::toggle_settings)),
            ],
            MenuId::Edit => vec![
                ("Undo Ctrl+Z", Box::new(Self::undo)),
                ("Redo Ctrl+Shift+Z", Box::new(Self::redo)),
                ("Delete selection Del", Box::new(Self::delete_selected)),
            ],
            MenuId::View => vec![
                (
                    "Toggle media pool",
                    Box::new(|this, cx| this.toggle_panel(PanelSide::Left, cx)),
                ),
                (
                    "Toggle inspector",
                    Box::new(|this, cx| this.toggle_panel(PanelSide::Right, cx)),
                ),
                (
                    "Toggle timeline",
                    Box::new(|this, cx| this.toggle_panel(PanelSide::Bottom, cx)),
                ),
            ],
            MenuId::Clip => vec![
                ("Split at playhead S", Box::new(Self::split_at_playhead)),
                (
                    "Add effect",
                    Box::new(|this, cx| this.select_left_tab(LeftTab::Effects, cx)),
                ),
            ],
            MenuId::Help => vec![("AniMathIO 2.0 — DaVinci-style editor", Box::new(|_, _| {}))],
        };
        let mut dropdown = div()
            .absolute()
            .top(px(34.))
            .left(px(2.))
            .w(px(220.))
            .bg(rgb(0x1f2633))
            .p_1()
            .flex()
            .flex_col()
            .shadow_lg();
        for (label, action) in items {
            dropdown = dropdown.child(
                div()
                    .px_2()
                    .py_1()
                    .cursor_pointer()
                    .hover(|this| this.bg(rgb(0x2a3446)))
                    .child(label)
                    .id("menu-item")
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.menu_open = None;
                        action(this, cx);
                    })),
            );
        }
        Some(dropdown)
    }

    fn media_pool(&self, cx: &mut Context<Self>) -> Div {
        let tabs = [
            (LeftTab::Assets, "Assets"),
            (LeftTab::Plugins, "Plugins"),
            (LeftTab::Effects, "Effects"),
        ];
        let mut tab_row = div().flex().flex_row().w_full().bg(rgb(0x12161e));
        for (tab, label) in tabs {
            let selected = self.left_tab == tab;
            tab_row = tab_row.child(
                div()
                    .px_2()
                    .py_1()
                    .cursor_pointer()
                    .text_color(if selected {
                        rgb(0xffffff)
                    } else {
                        rgb(0x8a94a6)
                    })
                    .bg(if selected {
                        rgb(0x232c3a)
                    } else {
                        rgb(0x12161e)
                    })
                    .child(label)
                    .id("media-tab")
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.select_left_tab(tab, cx);
                    })),
            );
        }

        let mut body = div()
            .flex_1()
            .w_full()
            .p_1()
            .id("media-body")
            .overflow_y_scroll();
        match self.left_tab {
            LeftTab::Assets => {
                if self.project.assets().is_empty() {
                    body = body.child(empty_state("Drop media here"));
                }
                for (id, asset) in self.project.assets() {
                    let asset_id = id.clone();
                    let name = asset.name.clone();
                    let kind = asset_kind_label(&asset.media_type);
                    body = body.child(
                        div()
                            .px_2()
                            .py_1()
                            .cursor_pointer()
                            .hover(|this| this.bg(rgb(0x232c3a)))
                            .flex()
                            .flex_row()
                            .justify_between()
                            .child(name)
                            .child(div().text_color(rgb(0x8a94a6)).child(kind))
                            .id("asset-item")
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.add_asset_at_playhead(asset_id.clone(), cx);
                            })),
                    );
                }
            }
            LeftTab::Plugins => {
                if self.plugins.is_empty() {
                    body = body.child(empty_state("No clip plugins loaded"));
                }
                for descriptor in &self.plugins {
                    let id = descriptor.id.clone();
                    let name = descriptor.name.clone();
                    body = body.child(
                        div()
                            .px_2()
                            .py_1()
                            .cursor_pointer()
                            .hover(|this| this.bg(rgb(0x232c3a)))
                            .child(name)
                            .id("plugin-item")
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.add_plugin_clip_by_id(&id, cx);
                            })),
                    );
                }
            }
            LeftTab::Effects => {
                let kinds = [
                    animathio_core::EffectKind::Grayscale,
                    animathio_core::EffectKind::Sepia,
                    animathio_core::EffectKind::Invert,
                    animathio_core::EffectKind::Saturate,
                    animathio_core::EffectKind::Brightness,
                    animathio_core::EffectKind::Contrast,
                    animathio_core::EffectKind::Blur,
                    animathio_core::EffectKind::Opacity,
                ];
                for kind in kinds {
                    body = body.child(
                        div()
                            .px_2()
                            .py_1()
                            .cursor_pointer()
                            .hover(|this| this.bg(rgb(0x232c3a)))
                            .child(effect_label(kind))
                            .id("effect-item")
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.add_effect_to_selected(kind, cx);
                            })),
                    );
                }
            }
        }
        div()
            .w(px(self.dock.left_width))
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(0x151a22))
            .child(tab_row)
            .child(body)
    }

    fn viewer(&self, cx: &mut Context<Self>) -> Div {
        let image = self.image.clone();
        let viewport = div()
            .flex_1()
            .w_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(0x0b0e13))
            .relative();
        let viewport = if let Some(image) = image {
            viewport
                .child(img(image).w_full().h_full().object_fit(ObjectFit::Contain))
                .child(canvas(
                    move |_bounds, _window, _cx| {},
                    move |bounds, (), window, cx| paint_safe_areas(bounds, window, cx),
                ))
        } else {
            viewport.child(empty_state("rendering…"))
        };
        div()
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(0x0b0e13))
            .child(viewport)
            .child(self.transport_controls(cx))
    }

    fn transport_controls(&self, cx: &mut Context<Self>) -> Div {
        let play_label = if self.playing { "⏸" } else { "▶" };
        let loop_label = if self.loop_playback {
            "🔁 on"
        } else {
            "🔁 off"
        };
        let timecode_text = self
            .timecode_buffer
            .clone()
            .unwrap_or_else(|| timecode::format_timecode(self.position, self.project.frame_rate()));
        let timecode_editing = self.timecode_buffer.is_some();
        div()
            .h(px(36.))
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .px_1()
            .bg(rgb(0x151a22))
            .child(button(
                "jump-start",
                "⏮",
                cx.listener(|this, _: &ClickEvent, _, cx| this.jump_to_start(cx)),
            ))
            .child(button(
                "step-back",
                "◀",
                cx.listener(|this, _: &ClickEvent, _, cx| this.step_frame(-1, cx)),
            ))
            .child(button("play", play_label, cx.listener(Self::on_play_click)))
            .child(button(
                "step-fwd",
                "▶",
                cx.listener(|this, _: &ClickEvent, _, cx| this.step_frame(1, cx)),
            ))
            .child(button(
                "jump-end",
                "⏭",
                cx.listener(|this, _: &ClickEvent, _, cx| this.jump_to_end(cx)),
            ))
            .child(button(
                "loop",
                loop_label,
                cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_loop(cx)),
            ))
            .child(
                div()
                    .px_2()
                    .py_1()
                    .cursor_text()
                    .bg(rgb(0x232c3a))
                    .text_color(if timecode_editing {
                        rgb(0xffd166)
                    } else {
                        rgb(0xffffff)
                    })
                    .child(timecode_text)
                    .id("timecode")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.begin_timecode_edit(cx);
                    })),
            )
    }

    fn timeline_panel(&self, cx: &mut Context<Self>) -> Div {
        let snapshot = self.build_timeline_snapshot();
        let zoom = self.timeline_zoom;
        let scroll = self.timeline_scroll;
        let weak = cx.entity().downgrade();
        let snapping = self.snapping;
        let snap_label = if snapping { "Snap: on" } else { "Snap: off" };

        div()
            .h(px(TIMELINE_H))
            .w_full()
            .flex()
            .flex_col()
            .bg(rgb(0x12161e))
            .child(
                div()
                    .h(px(30.))
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .bg(rgb(0x171c26))
                    .child(button(
                        "snap",
                        snap_label,
                        cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_snapping(cx)),
                    ))
                    .child(button(
                        "zoom-out",
                        "−",
                        cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.timeline_zoom = (this.timeline_zoom / 1.25).clamp(1.0, 64.0);
                            cx.notify();
                        }),
                    ))
                    .child(div().text_color(rgb(0x8a94a6)).child(format!("{zoom:.1}×")))
                    .child(button(
                        "zoom-in",
                        "+",
                        cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.timeline_zoom = (this.timeline_zoom * 1.25).clamp(1.0, 64.0);
                            cx.notify();
                        }),
                    )),
            )
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .flex()
                    .flex_row()
                    .child(self.track_header(cx))
                    .child(
                        div()
                            .flex_1()
                            .h_full()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(Self::on_timeline_mouse_down),
                            )
                            .on_mouse_move(cx.listener(Self::on_timeline_mouse_move))
                            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_timeline_mouse_up))
                            .on_scroll(cx.listener(Self::on_timeline_scroll_wheel))
                            .child(
                                canvas(
                                    move |bounds, _window, cx| {
                                        let _ = weak
                                            .update(cx, |this, _| this.timeline_bounds = bounds);
                                    },
                                    move |bounds, (), window, cx| {
                                        paint_timeline(bounds, &snapshot, zoom, scroll, window, cx);
                                    },
                                )
                                .size_full(),
                            ),
                    ),
            )
    }

    fn track_header(&self, cx: &mut Context<Self>) -> Div {
        let mut header = div()
            .w(px(HEADER_W))
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(0x151a22));
        header = header.child(
            div()
                .h(px(RULER_H))
                .w_full()
                .border_b_1()
                .border_color(rgb(0x2a3342)),
        );
        for track in self.project.tracks() {
            let id = track.id();
            let name = track.name().to_owned();
            let gain = track.gain();
            header = header.child(
                div()
                    .h(px(ROW_H))
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .px_1()
                    .border_b_1()
                    .border_color(rgb(0x2a3342))
                    .child(div().flex_1().child(name))
                    .child(div().text_color(rgb(0x8a94a6)).child(format!("{gain:.1}")))
                    .child(button(
                        "gain-",
                        "−",
                        cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.nudge_track_gain(id, -0.1, cx);
                        }),
                    ))
                    .child(button(
                        "gain+",
                        "+",
                        cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.nudge_track_gain(id, 0.1, cx);
                        }),
                    )),
            );
        }
        header
    }

    fn inspector(&self, cx: &mut Context<Self>) -> Div {
        let mut pane = div()
            .w(px(self.dock.right_width))
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
        if let ClipKind::Error(message) = &clip.kind {
            pane = pane.child(
                div()
                    .text_color(rgb(0xff6b6b))
                    .child(format!("⚠ {message}")),
            );
        }
        pane = pane.children(self.transform_rows(id, cx));
        if matches!(clip.kind, ClipKind::Plugin { .. }) {
            pane = pane.children(self.plugin_rows(id, cx));
        } else {
            pane = pane.children(self.numeric_param_rows(id, cx));
        }
        if let Some(selection) = &self.selected_keyframe
            && selection.clip_id == id
        {
            pane = pane.children(self.keyframe_menu(selection, cx));
        }
        pane
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
        if self.bench.is_some() {
            self.bench_tick(window, cx);
        }
        if let Some(task) = &self.export_task {
            match task.result.try_recv() {
                Ok(result) => {
                    self.error = Some(match result {
                        Ok(path) => format!("Exported {}", path.display()),
                        Err(error) => error,
                    });
                    self.export_task = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.error = Some("export worker stopped unexpectedly".into());
                    self.export_task = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => window.request_animation_frame(),
            }
        }

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

        let mut middle = div().flex_1().flex().flex_row();
        if self.dock.left_open {
            middle = middle.child(self.media_pool(cx));
        }
        middle = middle.child(self.viewer(cx));
        if self.dock.right_open {
            middle = middle.child(self.inspector(cx));
        }

        let mut root = div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(rgb(0x0b0e13))
            .text_color(rgb(0xffffff))
            .track_focus(&self.focus_handle)
            .child(self.menu_bar(cx))
            .child(middle);
        if self.dock.bottom_open {
            root = root.child(self.timeline_panel(cx));
        }
        root = root.child(self.status_line());
        if let Some(dropdown) = self.menu_dropdown(cx) {
            root = root.child(dropdown);
        }
        if self.settings_open {
            root = root.child(self.settings_dialog(cx));
        }
        if self.export_task.is_some() || self.confirm_overwrite.is_some() {
            root = root.child(self.export_dialog(cx));
        }
        root
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
            let _ = line.paint(
                point(x0 + px(4.0), y0 + px(16.0)),
                px(14.0),
                TextAlign::Left,
                None,
                window,
                cx,
            );
        }
        if clip.error {
            let badge = Bounds::from_corners(
                point(x0 + px(2.0), y0 + px(2.0)),
                point(x0 + px(12.0), y0 + px(12.0)),
            );
            window.paint_quad(fill(badge, rgb(0xe04040)));
            let run = TextRun {
                len: 1,
                font: font(".SystemUIFont"),
                color: rgb(0xffffff).into(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let line = window
                .text_system()
                .shape_line("!".into(), px(9.0), &[run], None);
            let _ = line.paint(
                point(x0 + px(3.0), y0 + px(2.0)),
                px(9.0),
                TextAlign::Left,
                None,
                window,
                cx,
            );
        }
        for keyframe in &clip.keyframes {
            let kx = bounds.origin.x + px(timeline::time_to_x(&layout, keyframe.time));
            let cy = y0 + px((ROW_H - 6.0) / 2.0);
            let half = px(4.0);
            let mut path = gpui::Path::new(point(kx, cy - half));
            path.line_to(point(kx + half, cy));
            path.line_to(point(kx, cy + half));
            path.line_to(point(kx - half, cy));
            path.line_to(point(kx, cy - half));
            window.paint_path(
                path,
                if keyframe.selected {
                    rgb(0xffffff)
                } else {
                    rgb(0xf2c14e)
                },
            );
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
        Field::Param(name) => clip
            .params
            .get(name)
            .and_then(animathio_core::ParamValue::as_number)
            .unwrap_or(0.0),
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

fn frame_rate_label(rate: FrameRate) -> String {
    if rate.denominator() == 1 {
        format!("{} fps", rate.numerator())
    } else {
        format!(
            "{:.3} fps",
            f64::from(rate.numerator()) / f64::from(rate.denominator())
        )
    }
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

/// Human-readable media-pool label for an asset's declared media type.
fn asset_kind_label(media_type: &str) -> &'static str {
    if media_type.starts_with("video/") {
        "video"
    } else if media_type.starts_with("image/") {
        "image"
    } else if media_type.starts_with("audio/") {
        "audio"
    } else {
        "asset"
    }
}

/// Human-readable effect name for the effects tab and inspector.
fn effect_label(kind: animathio_core::EffectKind) -> &'static str {
    match kind {
        animathio_core::EffectKind::Grayscale => "Grayscale",
        animathio_core::EffectKind::Sepia => "Sepia",
        animathio_core::EffectKind::Invert => "Invert",
        animathio_core::EffectKind::Saturate => "Saturate",
        animathio_core::EffectKind::Brightness => "Brightness",
        animathio_core::EffectKind::Contrast => "Contrast",
        animathio_core::EffectKind::Blur => "Blur",
        animathio_core::EffectKind::Opacity => "Opacity",
    }
}

/// Dim placeholder text used for empty panels and unloaded viewports.
fn empty_state(label: &'static str) -> Div {
    div()
        .w_full()
        .py_4()
        .flex()
        .justify_center()
        .text_color(rgb(0x5a6474))
        .child(label)
}

/// Paint title-safe and action-safe guides over the viewer.
fn paint_safe_areas(bounds: Bounds<Pixels>, window: &mut Window, _cx: &mut App) {
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    let origin = bounds.origin;
    let mut paint_frame = |inset: f32| {
        let left = origin.x + px(width * inset);
        let top = origin.y + px(height * inset);
        let frame_bounds = Bounds::from_corners(
            point(left, top),
            point(
                origin.x + px(width * (1.0 - inset)),
                origin.y + px(height * (1.0 - inset)),
            ),
        );
        window.paint_quad(outline(frame_bounds, rgb(0x3a4658), BorderStyle::Solid));
    };
    paint_frame(0.05); // action-safe
    paint_frame(0.10); // title-safe
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

fn plugins_cache_dir() -> PathBuf {
    directories::BaseDirs::new().map_or_else(
        || std::env::temp_dir().join("animathio-plugin-cache"),
        |base| base.cache_dir().join("animathio").join("plugin-cache"),
    )
}

/// Load every clip-source plugin from the bundled build output and the user
/// plugins directory. Descriptors are plain data for the UI; the host itself is
/// kept on the UI thread and drives both preview and control rendering.
pub(crate) fn load_plugins() -> (Option<PluginHost>, Vec<PluginDescriptor>, Option<String>) {
    let mut host = match PluginHost::new(plugins_cache_dir()) {
        Ok(host) => host,
        Err(error) => {
            return (
                None,
                Vec::new(),
                Some(format!("plugin host unavailable: {error}")),
            );
        }
    };
    let mut error = None;
    for dir in animathio_plugin_host::plugin_search_dirs() {
        match host.load_plugin_directory(&dir) {
            Ok(_) => {}
            Err(message) => {
                eprintln!("plugin load failed from {}: {message}", dir.path.display());
                error.get_or_insert_with(|| format!("plugin load failed: {message}"));
            }
        }
    }
    let mut descriptors = Vec::new();
    for id in host.loaded_plugin_ids() {
        let Some(plugin) = host.get_plugin(&id) else {
            continue;
        };
        if plugin.manifest.kinds.contains(&PluginKind::ClipSource) {
            descriptors.push(PluginDescriptor {
                id: id.clone(),
                name: plugin.manifest.name.clone(),
                schema: plugin.inspector_schema.clone(),
            });
        }
    }
    descriptors.sort_by(|left, right| left.name.cmp(&right.name));
    (Some(host), descriptors, error)
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

fn parse_hex_color(value: &str) -> Option<[u8; 4]> {
    let hex = value.strip_prefix('#').unwrap_or(value);
    let (rgb, alpha) = match hex.len() {
        6 => (hex, 255),
        8 => (&hex[..6], u8::from_str_radix(&hex[6..], 16).ok()?),
        _ => return None,
    };
    let value = u32::from_str_radix(rgb, 16).ok()?;
    Some([
        ((value >> 16) & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
        alpha,
    ])
}

fn parse_fps(value: &str) -> Result<FrameRate, String> {
    let fps = value
        .parse::<f64>()
        .map_err(|_| "invalid frame rate".to_owned())?;
    if !fps.is_finite() || fps <= 0.0 {
        return Err("invalid frame rate".into());
    }
    // Integer rates keep denominator 1; fractional rates use the NTSC 1001
    // denominator so 23.976/29.97/59.94 round-trip exactly.
    let (numerator, denominator) = if fps.fract() == 0.0 {
        (fps as u32, 1)
    } else {
        ((fps * 1001.0).round() as u32, 1001)
    };
    FrameRate::new(numerator.max(1), denominator).map_err(|e| e.0)
}

pub(crate) fn color_from_json(value: &Value) -> Option<[u8; 4]> {
    if let Some(array) = value.as_array()
        && array.len() >= 3
    {
        return Some([
            array.first().and_then(Value::as_u64).unwrap_or(0) as u8,
            array.get(1).and_then(Value::as_u64).unwrap_or(0) as u8,
            array.get(2).and_then(Value::as_u64).unwrap_or(0) as u8,
            array.get(3).and_then(Value::as_u64).unwrap_or(255) as u8,
        ]);
    }
    if let Some(object) = value.as_object() {
        return Some([
            object.get("r").and_then(Value::as_u64).unwrap_or(0) as u8,
            object.get("g").and_then(Value::as_u64).unwrap_or(0) as u8,
            object.get("b").and_then(Value::as_u64).unwrap_or(0) as u8,
            object.get("a").and_then(Value::as_u64).unwrap_or(255) as u8,
        ]);
    }
    None
}

pub(crate) fn point_from_json(value: &Value) -> Option<[f32; 2]> {
    if let Some(array) = value.as_array()
        && array.len() >= 2
    {
        return Some([
            array.first().and_then(Value::as_f64).unwrap_or(0.0) as f32,
            array.get(1).and_then(Value::as_f64).unwrap_or(0.0) as f32,
        ]);
    }
    if let Some(object) = value.as_object() {
        return Some([
            object.get("x").and_then(Value::as_f64).unwrap_or(0.0) as f32,
            object.get("y").and_then(Value::as_f64).unwrap_or(0.0) as f32,
        ]);
    }
    None
}

fn asset_id_from(value: &str) -> AssetId {
    AssetId::try_from(value.to_owned()).unwrap_or_else(|_| AssetId::of(value.as_bytes()))
}

fn cycle_option(options: &[String], current: &str) -> String {
    options
        .iter()
        .position(|option| option == current)
        .and_then(|index| options.get(index + 1).or_else(|| options.first()))
        .cloned()
        .unwrap_or_else(|| current.to_owned())
}

const COLOR_PRESETS: [[u8; 4]; 8] = [
    [255, 255, 255, 255],
    [0, 0, 0, 255],
    [239, 68, 68, 255],
    [34, 197, 94, 255],
    [59, 130, 246, 255],
    [234, 179, 8, 255],
    [168, 85, 247, 255],
    [90, 170, 255, 255],
];

fn cycle_color(current: [u8; 4]) -> [u8; 4] {
    COLOR_PRESETS
        .iter()
        .position(|color| color[0..3] == current[0..3])
        .and_then(|index| {
            COLOR_PRESETS
                .get(index + 1)
                .or_else(|| COLOR_PRESETS.first())
        })
        .copied()
        .unwrap_or(COLOR_PRESETS[0])
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
