use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use animathio_audio::{AudioPool, Pcm, Speed, SymphoniaSource, Waveform};
use animathio_control::{
    AssetInfo, Call, ControlError, ProjectState, RenderFrameParams, RenderFrameResult,
    media_type_of, read_bounded, timeline_of, validate_existing_file, validate_output_file,
};
use animathio_core::{
    Asset, AssetId, AudioChain, Clip, ClipId, ClipKind, Command, EffectId, FrameRate, History,
    Interpolation, MarkerId, Project, Rounding, Scene, TICKS_PER_SECOND, Target, Time, TimeRange,
    Track, TrackId, Transform, TransformProperty, evaluate,
};
use animathio_plugin_api::PluginKind;
use animathio_plugin_host::PluginHost;
use base64::Engine as _;
use gpui::{prelude::*, *};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::{ActiveTheme, Selectable, Sizable};
use serde_json::{Value, json};

use crate::clipboard::Clipboard;
use crate::control::{ControlMsg, ControlRuntime};
use crate::editing::{self, InOut, Shuttle, Tool};
use crate::fades::{self, FadeEdge};
use crate::icons::{self, IconName};
use crate::inspector::{self, ParamModel, PluginDescriptor};
use crate::layout::DockLayout;
use crate::playback::{Clock, due_frame, make_clock};
use crate::render_worker::{RenderRequest, RenderWorker};
use crate::ripple::{self, RippleScope};
use crate::selection;
use crate::timecode;
use crate::timeline::{self, TimelineLayout, TrimEdge};

pub(crate) mod actions;
mod audio_effects;
mod autocut;
mod caption_styles;
mod clip_ops;
mod color_edit;
mod commands;
mod control_bridge;
mod dialogs;
mod export_panel;
mod fields;
mod inspector_view;
mod markers;
mod media_pool;
mod menus;
mod playback_glue;
mod project_io;
mod reframe;
mod start_screen;
mod subtitles;
mod support;
mod templates;
mod timeline_thumbnails;
mod timeline_view;
mod titles;
mod transport;
mod viewer;
mod workspace;

use dialogs::ReplaceProject;
use support::*;
pub(crate) use support::{
    color_from_json, load_plugins, point_from_json, read_project_with_recovery,
};

const RULER_H: f32 = 28.0;
const HEADER_W: f32 = 240.0;
const MAX_RECENT: usize = 10;
const DEFAULT_MEDIA_TICKS: i64 = TICKS_PER_SECOND * 2;
/// Peaks kept per decoded audio asset for timeline waveforms.
const WAVEFORM_BUCKETS: usize = 4096;
/// Distance of a fade handle's centre from its clip edge and top, in pixels.
const FADE_HANDLE_INSET: f32 = 7.0;

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

/// A context-menu action on one clip.
type ClipAction = fn(&mut Editor, ClipId, &mut Context<Editor>);

/// Tabs in the left media-pool panel.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LeftTab {
    Titles,
    Assets,
    Plugins,
    Effects,
}

/// Viewer zoom modes for the preview image.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ViewerFit {
    Contain,
    Fill,
    Actual,
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
enum InputTarget {
    /// Builtin transform/param numeric field.
    Field {
        id: ClipId,
        field: Field,
    },
    /// Numeric parameter of one of a clip's audio effects.
    AudioEffectParam {
        chain: AudioChain,
        effect: EffectId,
        name: String,
    },
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
    /// One axis of a plugin point parameter.
    PluginPoint {
        id: ClipId,
        name: String,
        axis: usize,
    },
    Timecode,
    /// A transform/parameter field applied to every clip of the selection it
    /// was created for (not whatever is selected when it commits on blur).
    SelectionField {
        field: Field,
        ids: BTreeSet<ClipId>,
    },
}

impl InputTarget {
    fn is_numeric(&self) -> bool {
        matches!(
            self,
            Self::Field { .. }
                | Self::SelectionField { .. }
                | Self::PluginNumber { .. }
                | Self::PluginPoint { .. }
                | Self::AudioEffectParam { .. }
        )
    }
}

struct ClipDrag {
    /// Every dragged clip (the whole selection, or just `clip_id`).
    members: BTreeSet<ClipId>,
    origin_track: TrackId,
    target_track: TrackId,
    origin_start: Time,
    /// Start of the earliest member, so the group stops at time zero together.
    group_start: i64,
    /// First and last track row occupied by members.
    rows: (usize, usize),
    /// Track rows the group has moved, already clamped to the stack.
    row_delta: isize,
    grab_offset: i64,
    current_start: Time,
    /// A plain click on a member of a multi-selection keeps the selection for the
    /// drag; releasing without moving collapses it onto this clip.
    collapse_to: Option<ClipId>,
}

/// Rubber-band selection on empty track space.
struct MarqueeDrag {
    /// Press point: x in content pixels (scroll-independent), y canvas-relative.
    origin: (f32, f32),
    /// Pointer, canvas-relative.
    current: (f32, f32),
    /// Selection the band extends (empty unless Shift/Ctrl was held).
    base: BTreeSet<ClipId>,
    /// The pointer left the click tolerance, so this is a band, not a click.
    moved: bool,
}

/// An inline track-name edit in the header.
struct TrackRename {
    id: TrackId,
    input: Entity<gpui_component::input::InputState>,
    _subscription: Subscription,
}

/// A track header being dragged to reorder tracks.
#[derive(Clone)]
struct TrackDrag {
    id: TrackId,
    name: SharedString,
}

impl Render for TrackDrag {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_3()
            .py_1()
            .rounded_sm()
            .bg(cx.theme().popover)
            .border_1()
            .border_color(cx.theme().ring)
            .text_color(cx.theme().foreground)
            .child(self.name.clone())
    }
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
    ripple: bool,
}

/// Dragging a ruler marker; `current` is the (snapped) time under the pointer.
struct MarkerDrag {
    id: MarkerId,
    origin: Time,
    current: Time,
}

/// A marker as painted on the ruler (with any drag already applied).
struct MarkerPaint {
    time: Time,
    end: Option<Time>,
    color: Hsla,
    name: String,
}

/// Dragging a fade handle: `current` is the fade length in ticks.
struct FadeDrag {
    clip_id: ClipId,
    edge: FadeEdge,
    current: i64,
}

enum TimelineGesture {
    Pan { x: f32, scroll: f32 },
    Overview,
    Scrub,
    Drag(ClipDrag),
    Trim(TrimDrag),
    Fade(FadeDrag),
    Marquee(MarqueeDrag),
    Marker(MarkerDrag),
}

/// A right-click context menu target on the timeline.
#[derive(Clone, PartialEq)]
enum ContextMenu {
    Clip { id: ClipId },
    Track { id: TrackId },
    Keyframe(KeyframeSelection),
}

/// An item being dragged from the media pool onto the timeline.
#[derive(Clone, PartialEq)]
enum PoolDrag {
    Title(String),
    Asset(AssetId),
    Plugin(String),
    /// A plugin effect, by plugin id and effect id.
    Effect {
        plugin: String,
        effect: String,
    },
}

struct KeyframePaint {
    time: Time,
    selected: bool,
}

struct ClipPaint {
    effect_warning: bool,
    thumbnail_first: usize,
    row: usize,
    start: Time,
    end: Time,
    selected: bool,
    hovered: bool,
    label: SharedString,
    kind: animathio_core::ClipKind,
    icon: IconName,
    keyframes: Vec<KeyframePaint>,
    /// Downsampled peaks for audio clips, once the asset is decoded.
    waveform: Option<Arc<Waveform>>,
    thumbnails: Vec<Option<Arc<RenderImage>>>,
    preview: SharedString,
    source_offset: i64,
    fade_levels: [Vec<f32>; 2],
    fade_in: i64,
    fade_out: i64,
    /// The clip's track is locked.
    locked: bool,
    /// The clip's track is muted or silenced by another track's solo.
    silenced: bool,
}

struct TransitionPaint {
    row: usize,
    range: TimeRange,
}

struct ZoomMotion {
    started: Instant,
    from: (f32, f32),
    to: (f32, f32),
}

struct GainDrag {
    id: TrackId,
    x: f32,
    original: f64,
    current: f64,
}

struct TimelineSnapshot {
    duration: i64,
    rows: usize,
    clips: Vec<ClipPaint>,
    overview: Vec<(usize, TimeRange, Hsla)>,
    transitions: Vec<TransitionPaint>,
    scrubbing: bool,
    waveform_paths: WavePathCache,
    position: Time,
    frame_rate: FrameRate,
    row_height: f32,
    marks: InOut,
    autocuts: Vec<(TimeRange, usize)>,
    markers: Vec<MarkerPaint>,
    snap_line: Option<Time>,
    /// Rubber band in canvas coordinates: `(left, top, right, bottom)`.
    marquee: Option<(f32, f32, f32, f32)>,
    theme: gpui_component::Theme,
}

/// A small on-disk list of recently opened projects, kept in the XDG config dir.
struct RecentFiles {
    paths: Vec<PathBuf>,
    modified: BTreeMap<PathBuf, SystemTime>,
}
impl RecentFiles {
    fn config_path() -> Option<PathBuf> {
        directories::ProjectDirs::from("org", "AniMathIO", "AniMathIO")
            .map(|dirs| dirs.config_dir().join("recent.json"))
    }
    fn load() -> Self {
        let paths: Vec<PathBuf> = Self::config_path()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let modified = paths
            .iter()
            .filter_map(|path| {
                std::fs::metadata(path)
                    .ok()?
                    .modified()
                    .ok()
                    .map(|time| (path.clone(), time))
            })
            .collect();
        Self { paths, modified }
    }
    fn add(&mut self, path: &Path) {
        self.paths.retain(|existing| existing != path);
        self.paths.insert(0, path.to_path_buf());
        if let Ok(time) = std::fs::metadata(path).and_then(|meta| meta.modified()) {
            self.modified.insert(path.to_path_buf(), time);
        }
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
    assets: Arc<BTreeMap<AssetId, Vec<u8>>>,
    audio_generation: u64,
    /// When a live parameter drag last handed the playback mixer a fresh project snapshot.
    last_live_audio_sync: Option<Instant>,
    save_pending: bool,
    pool: AudioPool,
    audio_ids: BTreeSet<AssetId>,
    path: Option<PathBuf>,
    /// Bumped whenever the project is replaced (new, open, v1/Manim replace), so
    /// background results started on an older project are discarded.
    project_session: u64,
    clock: Box<dyn Clock>,
    playing: bool,
    position: Time,
    /// The primary selected clip: what the inspector, keyframes and fades act on.
    /// Always a member of [`Editor::selected_ids`] when set.
    selected: Option<ClipId>,
    audio_inspector_track: Option<TrackId>,
    /// Every selected clip; see [`Editor::selected_ids`] for the reconciled view.
    selection: BTreeSet<ClipId>,
    clipboard: Option<Clipboard>,
    ripple_scope: RippleScope,
    track_rename: Option<TrackRename>,
    selected_field: Option<Field>,
    worker: RenderWorker,
    image: Option<Arc<RenderImage>>,
    request_generation: u64,
    last_requested: Option<Time>,
    timeline_bounds: Bounds<Pixels>,
    timeline_zoom: f32,
    zoom_motion: Option<ZoomMotion>,
    gain_drag: Option<GainDrag>,
    thumbnails: std::cell::RefCell<timeline_thumbnails::Thumbnails>,
    timeline_scroll: f32,
    gesture: Option<TimelineGesture>,
    plugin_host: Option<PluginHost>,
    plugins: Vec<PluginDescriptor>,
    tool: Tool,
    marks: InOut,
    shuttle: Shuttle,
    /// Whether K is currently held, so J/L jog at half speed.
    k_held: bool,
    snap_line: Option<Time>,
    preferences: crate::preferences::Preferences,
    /// Persisted "GPU preview" toggle.
    preview: crate::preview_settings::PreviewSettings,
    export_queue: crate::export_queue::ExportQueue,
    export_queue_open: bool,
    /// The automatic-subtitle job, running or finished (see `editor/subtitles.rs`).
    subtitle_job: Option<crate::subtitle_job::SubtitleJob>,
    /// Remembered caption look: used for new subtitles and by the caption inspector.
    caption_look: animathio_control::CaptionStyleParams,
    subtitle_panel_open: bool,
    autocut_review: Option<autocut::Review>,
    /// The markers and beat-sync panel (see `editor/markers.rs`).
    marker_panel: Option<markers::Panel>,
    reframe_panel: Option<reframe::ReframePanel>,
    /// A queued-export spec waiting for the user to confirm replacing its file.
    pending_export: Option<(crate::export_queue::JobSpec, bool)>,
    last_export_tick: Instant,
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
    previous_left_tab: LeftTab,
    tab_changed: Instant,
    hovered_preset: Option<usize>,
    previous_hovered_preset: Option<usize>,
    preset_hover_changed: Instant,
    app_menu: Entity<gpui_component::menu::AppMenuBar>,
    menu_signature: String,
    panel_focus: [FocusHandle; 4],
    timeline_scroll_handle: ScrollHandle,
    pointer_down: bool,
    color_edit: Option<color_edit::ColorEdit>,
    notices: Vec<gpui_component::notification::Notification>,
    loop_playback: bool,
    snapping: bool,
    hovered_clip: Option<ClipId>,
    viewer_fit: ViewerFit,
    viewer_state: viewer::ViewerState,
    effect_warning: Option<String>,
    failed_effect_clips: BTreeSet<ClipId>,
    media_view: media_pool::MediaView,
    title_library: titles::TitleLibrary,
    safe_areas: bool,
    viewer_menu_open: bool,
    closed_sections: BTreeSet<String>,
    context_menu: Option<ContextMenu>,
    context_menu_pos: Option<Point<Pixels>>,
    pool_drag: Option<PoolDrag>,
    pool_drag_pos: Point<Pixels>,
    fields: fields::FieldPool,
    waveforms: std::cell::RefCell<BTreeMap<AssetId, Arc<Waveform>>>,
    waveform_paths: WavePathCache,
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
        if let Some(host) = &plugin_host {
            crate::playback::install_audio_effects(Arc::new(host.audio_effect_processor()));
        }
        let audio_ids = audio_asset_ids(&project);
        let initial_next_id = next_id(&project);
        let clock = make_clock(&project, &pool);
        let worker = RenderWorker::spawn();
        let preview = crate::preview_settings::PreviewSettings::load();
        worker.set_backend(preview.backend());
        worker.load(Arc::new(assets.clone()));
        let mut this = Self {
            project,
            history: History::default(),
            assets: Arc::new(assets),
            audio_generation: 0,
            last_live_audio_sync: None,
            save_pending: false,
            pool,
            audio_ids,
            path,
            project_session: 0,
            clock,
            playing: false,
            position: Time(0),
            selected: None,
            audio_inspector_track: None,
            selection: BTreeSet::new(),
            clipboard: None,
            ripple_scope: RippleScope::default(),
            track_rename: None,
            selected_field: None,
            worker,
            image: None,
            request_generation: 0,
            last_requested: None,
            timeline_bounds: Bounds::default(),
            timeline_zoom: 1.0,
            zoom_motion: None,
            gain_drag: None,
            thumbnails: std::cell::RefCell::default(),
            timeline_scroll: 0.0,
            gesture: None,
            plugin_host,
            plugins,
            tool: Tool::default(),
            marks: InOut::default(),
            shuttle: Shuttle::default(),
            k_held: false,
            snap_line: None,
            preferences: crate::preferences::Preferences::load(),
            preview,
            export_queue: crate::export_queue::ExportQueue::default(),
            export_queue_open: false,
            subtitle_job: None,
            caption_look: animathio_control::CaptionStyleParams::default(),
            subtitle_panel_open: false,
            autocut_review: None,
            marker_panel: None,
            reframe_panel: None,
            pending_export: None,
            last_export_tick: Instant::now(),
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
            previous_left_tab: LeftTab::Assets,
            tab_changed: Instant::now()
                .checked_sub(Duration::from_secs(1))
                .unwrap_or_else(Instant::now),
            hovered_preset: None,
            previous_hovered_preset: None,
            preset_hover_changed: Instant::now()
                .checked_sub(Duration::from_secs(1))
                .unwrap_or_else(Instant::now),
            app_menu: gpui_component::menu::AppMenuBar::new(cx),
            menu_signature: String::new(),
            panel_focus: std::array::from_fn(|_| cx.focus_handle()),
            timeline_scroll_handle: ScrollHandle::new(),
            pointer_down: false,
            color_edit: None,
            notices: Vec::new(),
            loop_playback: false,
            snapping: true,
            hovered_clip: None,
            viewer_fit: ViewerFit::Contain,
            viewer_state: viewer::ViewerState::default(),
            effect_warning: None,
            failed_effect_clips: BTreeSet::new(),
            media_view: media_pool::MediaView::default(),
            title_library: titles::TitleLibrary::default(),
            safe_areas: false,
            viewer_menu_open: false,
            closed_sections: BTreeSet::new(),
            context_menu: None,
            context_menu_pos: None,
            pool_drag: None,
            pool_drag_pos: Point::default(),
            fields: fields::FieldPool::default(),
            waveforms: std::cell::RefCell::default(),
            waveform_paths: std::rc::Rc::default(),
        };
        if recovered {
            this.notices
                .push(crate::style::Toast::info("Recovered from autosave"));
        }
        this.request_render(Time(0));
        // A window-manager close asks before dropping unsaved work.
        let close_view = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            close_view
                .update(cx, |this, cx| {
                    if this.dirty {
                        this.replace_project(ReplaceProject::CloseWindow, window, cx);
                    }
                    !this.dirty
                })
                .unwrap_or(true)
        });
        window.focus(
            if this.is_start_screen() {
                &this.focus_handle
            } else {
                &this.panel_focus[0]
            },
            cx,
        );
        // Drain worker/control mailboxes even when there are no pointer events.
        // Only completed work invalidates the view; an idle editor need not redraw.
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(25))
                    .await;
                if this
                    .update_in(cx, |this, window, cx| {
                        if this.thumbnails.borrow_mut().poll() {
                            cx.notify();
                        }
                        if this.pump_results(window) {
                            cx.notify();
                        }
                        this.drain_control(cx);
                        this.pump_subtitle_job(cx);
                        if this.clock.has_error() {
                            this.error =
                                Some("Audio playback failed; pause and play to retry".into());
                            cx.notify();
                        }
                        this.autosave_tick(cx);
                        // Background-job progress is redrawn at 4 Hz, not every frame.
                        if (this.pump_export_queue()
                            || (this.export_queue.is_busy()
                                || this.reframe_panel.as_ref().is_some_and(|p| p.busy))
                                && this.last_export_tick.elapsed() >= Duration::from_millis(250))
                            && !this.is_start_screen()
                        {
                            this.last_export_tick = Instant::now();
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
        this
    }
}

impl Focusable for Editor {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.advance_timeline_zoom(window);
        self.pump_results(window);
        self.drain_control(cx);
        if self.bench.is_some() {
            self.bench_tick(window, cx);
        }
        self.pump_export_queue();
        self.flush_notices(window, cx);

        if self.playing {
            let duration = animathio_export::duration(&self.project).0.max(1);
            let range = if self.loop_playback {
                self.marks.range(duration)
            } else {
                None
            }
            .unwrap_or_else(|| TimeRange::new(Time(0), Time(duration)).expect("positive duration"));
            let raw = self.clock.now();
            let (now, stopped) =
                editing::playback_position(raw, range, self.shuttle.0, self.loop_playback);
            if now != raw {
                let _ = self.clock.seek(now);
            }
            if stopped {
                let _ = self.clock.pause();
                self.playing = false;
                self.shuttle = Shuttle::default();
            }
            self.position = now;
            let due = due_frame(&self.project, now);
            if due != self.last_requested
                && let Some(due) = due
            {
                self.request_render(due);
            }
            window.request_animation_frame();
        }

        self.refresh_menus(cx);
        let workspace = if self.is_start_screen() {
            self.start_screen(cx)
        } else {
            self.workspace(window, cx)
        };
        let gesture_view = cx.entity().downgrade();
        let mut root = div()
            .child(
                canvas(
                    |_, _, _| {},
                    move |_, (), window, _| {
                        Self::capture_color_gesture(gesture_view.clone(), window);
                    },
                )
                .absolute()
                .size_full(),
            )
            .size_full()
            .relative()
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if this.pool_drag.is_some() {
                    this.pool_drag_pos = event.position;
                    cx.notify();
                }
            }))
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .text_size(cx.theme().font_size)
            .font_family(cx.theme().font_family.clone())
            .key_context("Editor")
            .on_key_up(cx.listener(|this, event: &KeyUpEvent, _, _| {
                if event.keystroke.key == "k" {
                    this.k_held = false;
                }
            }))
            .track_focus(&self.focus_handle)
            .child(self.menu_bar(window, cx))
            .child(workspace);
        root = root.child(self.status_line(cx));
        if let Some(panel) = self.export_queue_panel(cx) {
            root = root.child(panel);
        }
        if let Some(panel) = self.reframe_view(cx) {
            root = root.child(panel);
        }
        if let Some(panel) = self.autocut_panel(cx) {
            root = root.child(panel);
        }
        if let Some(panel) = self.subtitle_panel(cx) {
            root = root.child(panel);
        }
        if let Some(panel) = self.marker_panel(cx) {
            root = root.child(panel);
        }
        if let Some(menu) = self.context_menu_dropdown(cx) {
            root = root.child(menu);
        }
        if let Some(ghost) = self.pool_drag_ghost(cx) {
            root = root.child(ghost);
        }
        actions::attach(root, cx)
    }
}
