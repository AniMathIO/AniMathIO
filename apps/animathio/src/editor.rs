use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use animathio_core::{
    AssetId, Clip, ClipId, ClipKind, Command, History, Project, TICKS_PER_SECOND, Time, TrackId,
    Transform, TransformProperty,
};
use gpui::{prelude::*, *};

use crate::playback::{Clock, due_frame, make_clock};
use crate::render_worker::{RenderRequest, RenderWorker};
use crate::timeline::{self, TimelineLayout};

const PREVIEW_SIZE: (u32, u32) = (1280, 720);
const RULER_H: f32 = 28.0;
const ROW_H: f32 = 44.0;
const TIMELINE_H: f32 = 260.0;

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
    duration: i64,
    grab_offset: i64,
    current_start: Time,
}

enum TimelineGesture {
    Scrub,
    Drag(ClipDrag),
}

struct ClipPaint {
    row: usize,
    start: Time,
    end: Time,
    selected: bool,
}

struct TimelineSnapshot {
    duration: i64,
    rows: usize,
    clips: Vec<ClipPaint>,
    position: Time,
}

pub(crate) struct Editor {
    project: Project,
    history: History,
    assets: BTreeMap<AssetId, Vec<u8>>,
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
    gesture: Option<TimelineGesture>,
    scrub: Option<Scrub>,
    editing: Option<NumberEdit>,
    error: Option<String>,
    focus_handle: FocusHandle,
}

impl Editor {
    pub(crate) fn new(
        cx: &mut Context<Self>,
        window: &mut Window,
        project: Project,
        assets: BTreeMap<AssetId, Vec<u8>>,
        path: Option<PathBuf>,
    ) -> Self {
        let clock = make_clock(&project);
        let worker = RenderWorker::spawn();
        worker.load(assets.clone());
        let mut this = Self {
            project,
            history: History::default(),
            assets,
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
            gesture: None,
            scrub: None,
            editing: None,
            error: None,
            focus_handle: cx.focus_handle(),
        };
        this.request_render(Time(0));
        window.focus(&this.focus_handle);
        cx.notify();
        this
    }

    fn request_render(&mut self, time: Time) {
        self.request_generation = self.request_generation.wrapping_add(1);
        self.last_requested = Some(time);
        self.worker.request(RenderRequest {
            generation: self.request_generation,
            project: self.project.clone(),
            time,
            size: PREVIEW_SIZE,
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

    fn current_layout(&self) -> TimelineLayout {
        TimelineLayout {
            width: f32::from(self.timeline_bounds.size.width).max(1.0),
            ruler_height: RULER_H,
            row_height: ROW_H,
            duration_ticks: timeline::project_duration(&self.project),
        }
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

    fn after_edit(&mut self, cx: &mut Context<Self>) {
        self.selected = self.selected.filter(|id| self.find_clip(*id).is_some());
        let time = self.current_render_time();
        self.request_render(time);
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
        match self.history.execute(&mut self.project, &command) {
            Ok(()) => self.after_edit(cx),
            Err(error) => self.error = Some(error.0),
        }
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
        match self
            .history
            .execute(&mut self.project, &Command::RemoveClip { id })
        {
            Ok(()) => {
                self.selected = None;
                self.selected_field = None;
                self.after_edit(cx);
            }
            Err(error) => self.error = Some(error.0),
        }
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
            Ok(()) => self.path = Some(path),
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
        self.open_path(path, cx);
    }

    fn open_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        match read_project(&path) {
            Ok((project, assets)) => {
                self.project = project;
                self.history = History::default();
                self.assets = assets.clone();
                self.path = Some(path);
                self.selected = None;
                self.selected_field = None;
                self.position = Time(0);
                self.playing = false;
                self.clock = make_clock(&self.project);
                self.worker.load(assets);
                self.request_render(Time(0));
            }
            Err(error) => self.error = Some(error),
        }
        cx.notify();
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
        if let Some(hit) = timeline::hit_test(&layout, &self.project, x, y) {
            self.selected = Some(hit.clip);
            self.selected_field = None;
            if let Some(clip) = self.find_clip(hit.clip).cloned() {
                let grab_offset = timeline::x_to_time(&layout, x).0 - clip.range.start.0;
                self.gesture = Some(TimelineGesture::Drag(ClipDrag {
                    clip_id: clip.id,
                    origin_track: hit.track,
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
        let (x, _y) = self.timeline_relative(event.position);
        let layout = self.current_layout();
        let mut scrubbing = false;
        if let Some(TimelineGesture::Drag(drag)) = &mut self.gesture {
            drag.current_start = timeline::drag_start(&layout, drag.grab_offset, x);
            cx.notify();
        } else if self
            .gesture
            .as_ref()
            .is_some_and(|gesture| matches!(gesture, TimelineGesture::Scrub))
        {
            scrubbing = true;
        }
        if scrubbing {
            self.scrub_to(x, cx);
        }
    }

    fn on_timeline_mouse_up(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(TimelineGesture::Drag(drag)) = self.gesture.take() {
            let command = Command::MoveClip {
                id: drag.clip_id,
                track: drag.origin_track,
                start: drag.current_start,
            };
            match self.history.execute(&mut self.project, &command) {
                Ok(()) => self.after_edit(cx),
                Err(error) => self.error = Some(error.0),
            }
        }
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
                } else {
                    (clip.range.start, clip.range.end)
                };
                clips.push(ClipPaint {
                    row,
                    start,
                    end,
                    selected: self.selected == Some(clip.id),
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
            .child(button("undo", "Undo", cx.listener(Self::on_undo_click)))
            .child(button("redo", "Redo", cx.listener(Self::on_redo_click)))
            .child(button("play", play_label, cx.listener(Self::on_play_click)))
            .child(
                div()
                    .px_2()
                    .child(format!("{:.3} s", seconds(self.position))),
            )
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
            return pane.child("No clip selected");
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
        div()
            .h(px(24.))
            .w_full()
            .flex()
            .flex_row()
            .justify_between()
            .px_2()
            .bg(rgb(0x1f2430))
            .child(format!(
                "tracks {} | clips {}",
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
            .child(
                canvas(
                    move |bounds, _window, cx| {
                        let _ = weak.update(cx, |this, _| this.timeline_bounds = bounds);
                    },
                    move |bounds, (), window, _cx| paint_timeline(bounds, &snapshot, window),
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

fn paint_timeline(bounds: Bounds<Pixels>, snapshot: &TimelineSnapshot, window: &mut Window) {
    let layout = TimelineLayout {
        width: f32::from(bounds.size.width).max(1.0),
        ruler_height: RULER_H,
        row_height: ROW_H,
        duration_ticks: snapshot.duration,
    };
    window.paint_quad(fill(bounds, rgb(0x12161e)));
    let width = f32::from(bounds.size.width);
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
    }
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

fn write_project(
    path: &Path,
    project: &Project,
    assets: &BTreeMap<AssetId, Vec<u8>>,
) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    animathio_format::write_amio(file, project, assets).map_err(|e| e.to_string())?;
    Ok(())
}
