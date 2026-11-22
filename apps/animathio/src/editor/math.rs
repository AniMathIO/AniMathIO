//! Math library and a reusable, draft-based object editor. Preview never mutates the project.

use super::assist::{AssistEvent, AssistPanel};
use super::*;
use crate::style::{self, TextRole as _, color, hex};
use animathio_control::{
    LATEX_SNIPPETS, LATEX_TEMPLATES, MathObjectDraft, MathObjectKind, math_object_command,
    math_object_draft, math_object_range,
};
use animathio_core::ParamValue;
use gpui_component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_component::input::{
    Input, InputEvent, InputState, NumberInput, NumberInputEvent, StepAction, Textarea,
    TextareaState,
};
use gpui_component::scroll::Scrollbar;
use gpui_component::switch::Switch;
use gpui_component::{Disableable, WindowExt};

#[derive(Default)]
pub(super) struct MathLibrary {
    started: bool,
    images: BTreeMap<usize, Arc<RenderImage>>,
}

// The preview host is shared across worker jobs, retaining compiled components and
// bounded plugin output caches while drafts change. Never acquire it on the UI thread.
fn render_preview(
    draft: &MathObjectDraft,
    size: (u32, u32),
    thumbnail: bool,
) -> Result<Arc<RenderImage>, String> {
    static HOST: std::sync::OnceLock<std::sync::Mutex<Option<PluginHost>>> =
        std::sync::OnceLock::new();
    let mut guard = HOST
        .get_or_init(|| std::sync::Mutex::new(load_plugins().0))
        .lock()
        .map_err(|e| e.to_string())?;
    let host = guard
        .as_mut()
        .ok_or("Build the first-party plugins to preview math objects")?;
    preview(draft, size, host, thumbnail)
}

/// Drop the host's transport prefixes ("plugin produced an invalid scene: service-failed: ...")
/// so the dialog shows only what is wrong with the input, such as "Invalid LaTeX: ...".
pub(super) fn plain_plugin_error(raw: &str) -> String {
    let mut message = raw;
    for prefix in ["plugin produced an invalid scene: ", "service-failed: "] {
        message = message.strip_prefix(prefix).unwrap_or(message);
    }
    let mut chars = message.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

fn preview(
    draft: &MathObjectDraft,
    size: (u32, u32),
    host: &mut PluginHost,
    thumbnail: bool,
) -> Result<Arc<RenderImage>, String> {
    let (plugin, mut params) = draft.parameters(size).map_err(|e| e.to_string())?;
    if matches!(draft.kind, MathObjectKind::Latex | MathObjectKind::Plot) {
        params["strict_errors"] = serde_json::json!(true);
    }
    if thumbnail && plugin == "org.animathio.shapes" {
        params["stroke_width"] =
            serde_json::json!(f64::from(size.0.min(size.1)) / f64::from(size.0) * 0.014);
    }
    host.render_clip(&plugin, &BTreeMap::new(), &params, 0, size)
        .map_err(|e| plain_plugin_error(&e.to_string()))?;
    let mut project = Project::default();
    Command::SetSettings {
        width: size.0,
        height: size.1,
        frame_rate: project.frame_rate(),
        background: [18, 20, 26, 255],
        duration: Time(0),
    }
    .apply(&mut project)
    .map_err(|e| e.to_string())?;
    if !matches!(
        draft.kind,
        MathObjectKind::Latex
            | MathObjectKind::Text
            | MathObjectKind::Plot
            | MathObjectKind::Cartesian
            | MathObjectKind::Polar
    ) {
        let mut grid = MathObjectDraft::new(MathObjectKind::Cartesian);
        grid.color = "#303640".into();
        if let Ok((command, _)) = math_object_command(
            &project,
            &grid,
            math_object_range(Time(0)).expect("default range"),
            None,
        ) {
            command.apply(&mut project).map_err(|e| e.to_string())?;
        }
    }
    let (command, _) = math_object_command(
        &project,
        draft,
        math_object_range(Time(0)).expect("default range"),
        None,
    )
    .map_err(|e| e.to_string())?;
    command.apply(&mut project).map_err(|e| e.to_string())?;
    if thumbnail && plugin == "org.animathio.shapes" {
        let id = project
            .tracks()
            .last()
            .and_then(|t| t.clips().keys().next())
            .copied()
            .ok_or("missing preview clip")?;
        Command::SetPluginParam {
            id,
            name: "stroke_width".into(),
            value: Some(ParamValue::Number(
                params["stroke_width"].as_f64().unwrap_or(0.01),
            )),
        }
        .apply(&mut project)
        .map_err(|e| e.to_string())?;
    }
    let scene = host.resolve(&evaluate(&project, Time(0)), size);
    let frame = animathio_render::render(&scene, &mut animathio_render::RenderCtx::default(), size)
        .map_err(|e| e.to_string())?;
    let mut rgba =
        image::RgbaImage::from_raw(size.0, size.1, frame.pixels).ok_or("invalid frame")?;
    for p in rgba.pixels_mut() {
        p.0.swap(0, 2);
    }
    Ok(Arc::new(RenderImage::new([image::Frame::new(rgba)])))
}

/// Cards per row in the Library's Elements tab for a dock width in logical pixels.
pub(super) fn element_card_columns(rem: gpui::Pixels, dock_width: f32) -> usize {
    super::media_pool::library_columns(rem, dock_width, 7.).min(3)
}

pub(super) fn element_group(kind: MathObjectKind) -> &'static str {
    match kind {
        MathObjectKind::Latex | MathObjectKind::Text => "Text & formulas",
        MathObjectKind::Plot | MathObjectKind::Cartesian | MathObjectKind::Polar => {
            "Graphs & grids"
        }
        _ => "Shapes & geometry",
    }
}

impl Editor {
    pub(super) fn add_math_default(
        &mut self,
        kind: MathObjectKind,
        start: Time,
        cx: &mut Context<Self>,
    ) {
        let Some(range) = math_object_range(start) else {
            return;
        };
        match math_object_command(&self.project, &MathObjectDraft::new(kind), range, None) {
            Ok((command, added)) => {
                if self.try_execute(&command, cx) {
                    self.select_only(added.clip_id, cx);
                }
            }
            Err(e) => {
                self.error = Some(e.to_string());
                cx.notify();
            }
        }
    }
    pub(super) fn open_math(
        &mut self,
        kind: MathObjectKind,
        edit: Option<ClipId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_pending_inspector_inputs(cx);
        let draft = edit
            .and_then(|id| {
                self.find_clip(id)
                    .and_then(|clip| math_object_draft(clip, project_size(&self.project)))
            })
            .unwrap_or_else(|| MathObjectDraft::new(kind));
        let range = edit
            .and_then(|id| self.find_clip(id).map(|c| c.range))
            .or_else(|| math_object_range(self.position));
        let Some(range) = range else {
            return;
        };
        let editor = cx.entity().downgrade();
        let session = self.project_session;
        let size = project_size(&self.project);
        let panel = cx
            .new(|cx| MathEditor::new(draft, editor, session, range, edit, true, size, window, cx));
        // The large dialog width, clamped to the viewport; the LaTeX palette sits beside the
        // form when the window allows.
        let width = style::dialog_width::LARGE
            .min((f32::from(window.viewport_size().width) - 32.) / style::ui_scale());
        let title = if edit.is_some() {
            "Edit math object"
        } else {
            "Add math object"
        };
        super::dialogs::open_dialog(window, cx, move |dialog, _, _| {
            super::dialogs::dialog_frame(dialog, width, IconName::Sigma, title, None)
                .child(panel.clone())
                .on_ok({
                    let panel = panel.clone();
                    move |_, window, cx| {
                        panel.update(cx, |panel, cx| panel.apply(window, cx));
                        false
                    }
                })
        });
    }
    pub(super) fn edit_math(
        &mut self,
        id: ClipId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(draft) = self
            .find_clip(id)
            .and_then(|clip| math_object_draft(clip, project_size(&self.project)))
        {
            self.select_only(id, cx);
            self.open_math(draft.kind, Some(id), window, cx);
            true
        } else {
            false
        }
    }
    pub(super) fn math_cards(&mut self, rem: gpui::Pixels, cx: &mut Context<Self>) -> Div {
        if !self.math_library.started {
            self.math_library.started = true;
            cx.spawn(async move |this, cx| {
                let images = cx
                    .background_spawn(async move {
                        MathObjectKind::ALL
                            .into_iter()
                            .enumerate()
                            .filter_map(|(i, k)| {
                                let mut draft = MathObjectDraft::new(k);
                                if matches!(k, MathObjectKind::Latex | MathObjectKind::Text) {
                                    draft.fields.insert("size".into(), "28".into());
                                }
                                if k == MathObjectKind::Text {
                                    draft.fields.insert("source".into(), "Text".into());
                                }
                                if k == MathObjectKind::Point {
                                    draft.fields.insert("radius".into(), "0.35".into());
                                }
                                render_preview(&draft, (180, 120), true)
                                    .ok()
                                    .map(|p| (i, p))
                            })
                            .collect()
                    })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.math_library.images = images;
                    cx.notify();
                });
            })
            .detach();
        }
        // Cards use the dock's width: one column when it is very narrow, three when it is wide.
        let columns = element_card_columns(rem, self.dock.left_width);
        let mut cards = div().flex().flex_wrap().gap_2().p_2();
        // The hint costs a row of the short 720p panel; narrow docks get it as a card tooltip.
        if columns >= 3 {
            cards = cards.child(
                div()
                    .w_full()
                    .text_size(style::type_size::LABEL)
                    .text_color(hex(color::TEXT_SECONDARY))
                    .child("Click to configure, + adds, drag to place."),
            );
        }
        let mut previous_group = "";
        for (i, kind) in self.element_kinds() {
            let group = element_group(kind);
            if group != previous_group {
                cards = cards.child(
                    div()
                        .w_full()
                        .pt_2()
                        .text_size(style::type_size::LABEL)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(hex(color::TEXT_SECONDARY))
                        .child(group),
                );
                previous_group = group;
            }
            let mut thumbnail = div()
                .h(px(if columns > 2 { 72. } else { 50. }))
                .w_full()
                .bg(hex(color::BG))
                .flex()
                .items_center()
                .justify_center();
            if let Some(image) = self.math_library.images.get(&i) {
                thumbnail = thumbnail.child(
                    img(image.clone())
                        .size_full()
                        .object_fit(ObjectFit::Contain),
                );
            } else {
                thumbnail = thumbnail.child(icons::icon(IconName::Shapes));
            }
            cards = cards.child(
                div()
                    .id(("math-card", i))
                    .debug_selector(move || format!("element-card-{i}"))
                    .w(relative(match columns {
                        3.. => 0.31,
                        2 => 0.48,
                        _ => 1.,
                    }))
                    .min_w_0()
                    .rounded(px(style::radius::CARD))
                    .overflow_hidden()
                    .border_1()
                    .border_color(hex(color::BORDER))
                    .cursor_pointer()
                    .focusable()
                    .tab_index(0)
                    .tooltip(|window, cx| {
                        gpui_component::tooltip::Tooltip::new(
                            "Click to configure, + adds defaults, drag to place",
                        )
                        .build(window, cx)
                    })
                    .hover(|s| s.border_color(hex(color::ACCENT)).bg(hex(color::HOVER)))
                    .focus_visible(|s| s.border_color(hex(color::ACCENT)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.pool_drag = None;
                        this.open_math(kind, None, window, cx);
                    }))
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.open_math(kind, None, window, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .child(
                        div()
                            .id(("math-thumbnail", i))
                            .child(thumbnail)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    this.arm_pool_drag(PoolDrag::Math(kind), cx);
                                }),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .px_2()
                            .py_1()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(style::type_size::LABEL)
                                    .child(kind.label()),
                            )
                            .child(
                                Button::new(("math-default", i))
                                    .debug_selector(move || format!("element-default-{i}"))
                                    .ghost()
                                    .compact()
                                    .icon(icons::icon(IconName::Plus))
                                    .tooltip("Add with defaults at playhead")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.pool_drag = None;
                                        this.add_math_default(kind, this.position, cx);
                                        cx.stop_propagation();
                                    })),
                            ),
                    ),
            );
        }
        cards
    }
}

/// Window width from which the Add math object dialog puts the LaTeX palette beside the form.
pub(super) const MATH_TWO_COLUMN_MIN_WIDTH: f32 = 960.;
/// Width of the palette column in that layout.
const MATH_PALETTE_WIDTH: f32 = 264.;
/// Height of the live preview stage.
const PREVIEW_HEIGHT: f32 = 160.;

pub(super) struct MathEditor {
    draft: MathObjectDraft,
    dialog: bool,
    fields: BTreeMap<String, Entity<InputState>>,
    source: Option<Entity<TextareaState>>,
    assist: Option<Entity<AssistPanel>>,
    vertices: Vec<[Entity<InputState>; 2]>,
    color: Entity<ColorPickerState>,
    subscriptions: Vec<Subscription>,
    editor: WeakEntity<Editor>,
    session: u64,
    range: TimeRange,
    edit: Option<ClipId>,
    size: (u32, u32),
    generation: u64,
    image: Option<Arc<RenderImage>>,
    error: Option<String>,
    busy: bool,
    focused: bool,
    form_scroll: ScrollHandle,
    /// Keyboard focus of the snippet and template chips (snippets first, then templates).
    chip_focus: Vec<FocusHandle>,
    palette_scroll: ScrollHandle,
}
impl MathEditor {
    pub(super) fn new(
        draft: MathObjectDraft,
        editor: WeakEntity<Editor>,
        session: u64,
        range: TimeRange,
        edit: Option<ClipId>,
        dialog: bool,
        size: (u32, u32),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut fields = BTreeMap::new();
        let mut subscriptions = Vec::new();
        let mut source = None;
        for spec in draft.specs() {
            if spec.id == "vertices" {
                continue;
            }
            let value = draft
                .fields
                .get(spec.id)
                .cloned()
                .unwrap_or_else(|| spec.default.into());
            if spec.id == "source" {
                let state = cx.new(|cx| {
                    TextareaState::new(window, cx)
                        .submit_on_enter(dialog)
                        .auto_grow(3, 5)
                        .default_value(value)
                });
                subscriptions.push(cx.subscribe(&state, |this, state, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.draft
                            .fields
                            .insert("source".into(), state.read(cx).value().to_string());
                        this.schedule_preview(cx);
                    }
                }));
                source = Some(state);
            } else {
                let state = cx.new(|cx| InputState::new(window, cx).default_value(value));
                let key = spec.id;
                subscriptions.push(cx.subscribe_in(
                    &state,
                    window,
                    move |this, state, event: &InputEvent, _window, cx| {
                        if matches!(event, InputEvent::Change) {
                            this.draft
                                .fields
                                .insert(key.into(), state.read(cx).value().to_string());
                            this.schedule_preview(cx);
                        }
                    },
                ));
                if spec.numeric {
                    subscriptions.push(cx.subscribe_in(
                        &state,
                        window,
                        move |this, state, event: &NumberInputEvent, window, cx| {
                            let NumberInputEvent::Step(action) = event;
                            let current = state.read(cx).value().parse::<f64>().unwrap_or(0.);
                            let delta = if matches!(action, StepAction::Increment) {
                                0.1
                            } else {
                                -0.1
                            };
                            let value = fields::format_number(current + delta);
                            state.update(cx, |s, cx| s.set_value(value.clone(), window, cx));
                            this.draft.fields.insert(key.into(), value);
                            this.schedule_preview(cx);
                        },
                    ));
                }
                fields.insert(key.into(), state);
            }
        }
        let rgba =
            animathio_control::parse_caption_color(&draft.color).unwrap_or([255, 255, 255, 255]);
        let color = cx.new(|cx| {
            let mut state = ColorPickerState::new(window, cx);
            state.set_value(fields::to_hsla(rgba), window, cx);
            state
        });
        subscriptions.push(
            cx.subscribe(&color, |this, _, event: &ColorPickerEvent, cx| {
                let ColorPickerEvent::Change(Some(hsla)) = event else {
                    return;
                };
                let rgba = Rgba::from(*hsla);
                let channel = |value: f32| (value.clamp(0., 1.) * 255.).round() as u8;
                this.draft.color = format!(
                    "#{:02x}{:02x}{:02x}{:02x}",
                    channel(rgba.r),
                    channel(rgba.g),
                    channel(rgba.b),
                    channel(rgba.a)
                );
                this.schedule_preview(cx);
            }),
        );
        // LaTeX only: describe a formula in words and convert it (opt-in, see `assist`).
        let assist = (draft.kind == MathObjectKind::Latex).then(|| {
            let panel = cx.new(|cx| AssistPanel::new(window, cx));
            subscriptions.push(cx.subscribe_in(
                &panel,
                window,
                |this, _, event: &AssistEvent, window, cx| {
                    let AssistEvent::Converted(latex) = event;
                    // The formula field takes the result; the live preview then validates it.
                    this.snippet(latex, true, window, cx);
                },
            ));
            panel
        });
        let mut this = Self {
            draft,
            dialog,
            fields,
            source,
            assist,
            vertices: Vec::new(),
            color,
            subscriptions,
            editor,
            session,
            range,
            edit,
            size,
            generation: 0,
            image: None,
            error: None,
            busy: false,
            focused: false,
            form_scroll: ScrollHandle::new(),
            chip_focus: (0..LATEX_SNIPPETS.len() + LATEX_TEMPLATES.len())
                .map(|_| cx.focus_handle())
                .collect(),
            palette_scroll: ScrollHandle::new(),
        };
        if this.draft.kind == MathObjectKind::Polygon {
            let vertices = this
                .draft
                .fields
                .get("vertices")
                .cloned()
                .unwrap_or_default();
            for pair in vertices.split(';').take(64) {
                if let Some((x, y)) = pair.trim().split_once(',') {
                    this.add_vertex([x.trim(), y.trim()], window, cx);
                }
            }
        }
        this.schedule_preview(cx);
        this
    }
    /// The AI assist panel of a LaTeX editor (UI tests drive Convert through it).
    #[cfg(test)]
    pub(super) fn assist_panel(&self) -> Option<Entity<AssistPanel>> {
        self.assist.clone()
    }
    /// The LaTeX source in the draft, as the preview and Add see it.
    #[cfg(test)]
    pub(super) fn draft_source(&self) -> String {
        self.draft.fields.get("source").cloned().unwrap_or_default()
    }
    fn add_vertex(&mut self, values: [&str; 2], window: &mut Window, cx: &mut Context<Self>) {
        let states =
            values.map(|v| cx.new(|cx| InputState::new(window, cx).default_value(v.to_owned())));
        for state in &states {
            self.subscriptions
                .push(cx.subscribe(state, |this, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.sync_vertices(cx);
                    }
                }));
            self.subscriptions.push(cx.subscribe_in(
                state,
                window,
                |this, state, event: &NumberInputEvent, window, cx| {
                    let NumberInputEvent::Step(action) = event;
                    let value = state.read(cx).value().parse::<f64>().unwrap_or(0.)
                        + if matches!(action, StepAction::Increment) {
                            0.1
                        } else {
                            -0.1
                        };
                    state.update(cx, |s, cx| {
                        s.set_value(fields::format_number(value), window, cx);
                    });
                    this.sync_vertices(cx);
                },
            ));
        }
        self.vertices.push(states);
    }
    fn sync_vertices(&mut self, cx: &mut Context<Self>) {
        let value = self
            .vertices
            .iter()
            .map(|states| {
                format!(
                    "{},{}",
                    states[0].read(cx).value(),
                    states[1].read(cx).value()
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        self.draft.fields.insert("vertices".into(), value);
        self.schedule_preview(cx);
    }
    fn schedule_preview(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        self.busy = true;
        self.error = None;
        let draft = self.draft.clone();
        let factor = 480. / f64::from(self.size.0.max(self.size.1));
        let size = (
            (f64::from(self.size.0) * factor).round().max(1.) as u32,
            (f64::from(self.size.1) * factor).round().max(1.) as u32,
        );
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(250))
                .await;
            if !this
                .read_with(cx, |this, _| this.generation == generation)
                .unwrap_or(false)
            {
                return;
            }
            let result = cx
                .background_spawn(async move { render_preview(&draft, size, false) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.generation == generation {
                    this.busy = false;
                    match result {
                        Ok(image) => {
                            this.image = Some(image);
                            this.error = None;
                        }
                        Err(e) => {
                            this.image = None;
                            this.error = Some(e);
                        }
                    }
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }
    /// The keyboard focus handle of a snippet or template chip, by its label (UI tests).
    #[cfg(test)]
    pub(super) fn chip_focus_for(&self, label: &str) -> Option<FocusHandle> {
        let at = LATEX_SNIPPETS
            .iter()
            .chain(LATEX_TEMPLATES)
            .position(|(name, _)| *name == label)?;
        self.chip_focus.get(at).cloned()
    }
    fn snippet(&mut self, value: &str, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(source) = &self.source {
            source.update(cx, |state, cx| {
                if replace {
                    state.replace_all(value.to_owned(), window, cx);
                } else {
                    state.insert(value.to_owned(), window, cx);
                }
            });
            self.draft
                .fields
                .insert("source".into(), source.read(cx).value().to_string());
            self.schedule_preview(cx);
        }
    }
    /// Commit the draft: add the object (or update the edited one) as one undo step.
    /// The dialog's Add button and Enter submit here; UI tests drive it directly.
    pub(super) fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.error.is_some() {
            return;
        }
        let draft = self.draft.clone();
        let range = self.range;
        let edit = self.edit;
        let session = self.session;
        let result = self.editor.update(cx, |editor, cx| -> Result<(), String> {
            if editor.project_session != session {
                return Err("The project changed. Reopen the math editor.".into());
            }
            let (command, added) = math_object_command(&editor.project, &draft, range, edit)
                .map_err(|e| e.to_string())?;
            let mut commands = vec![command];
            if draft.write_on && edit.is_none() {
                let mut candidate = editor.project.clone();
                commands[0]
                    .apply(&mut candidate)
                    .map_err(|e| e.to_string())?;
                let (animation, _) = animathio_control::animation_preset_command(
                    &candidate,
                    editor.plugin_host.as_ref(),
                    &animathio_control::AnimationApplyParams {
                        clip: added.clip_id,
                        preset: "typewriter-in".into(),
                        plugin: None,
                        duration: Some(Time(TICKS_PER_SECOND)),
                        options: BTreeMap::new(),
                    },
                )
                .map_err(|e| e.to_string())?;
                commands.push(animation);
            }
            if editor.try_execute(&Command::Batch { commands }, cx) {
                editor.select_only(added.clip_id, cx);
                Ok(())
            } else {
                Err(editor
                    .error
                    .clone()
                    .unwrap_or_else(|| "Could not apply object".into()))
            }
        });
        match result {
            Ok(Ok(())) => {
                if self.dialog {
                    window.close_dialog(cx);
                }
            }
            Ok(Err(e)) => self.error = Some(e),
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }
}
impl Render for MathEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Type straight away: focus the first field once the dialog has taken focus.
        if self.dialog && !self.focused {
            self.focused = true;
            let handle = self
                .source
                .as_ref()
                .map(|state| state.read(cx).focus_handle(cx))
                .or_else(|| {
                    self.draft.specs().first().and_then(|spec| {
                        self.fields
                            .get(spec.id)
                            .map(|state| state.read(cx).focus_handle(cx))
                    })
                })
                .or_else(|| {
                    self.vertices
                        .first()
                        .map(|pair| pair[0].read(cx).focus_handle(cx))
                });
            if let Some(handle) = handle {
                window.focus(&handle, cx);
            }
        }
        // The dialog keeps the LaTeX palette beside the form when the window is wide enough, so
        // nothing sits below a scroll at 1280x720. Narrower windows and the inspector stack it.
        let two_columns = self.dialog
            && self.draft.kind == MathObjectKind::Latex
            && f32::from(window.viewport_size().width)
                >= MATH_TWO_COLUMN_MIN_WIDTH * style::ui_scale();
        let form_max_height = if self.dialog {
            // Leave room for the preview and footer.
            (f32::from(window.viewport_size().height) - 380. * style::ui_scale())
                .clamp(120., 440. * style::ui_scale())
        } else {
            360.
        };
        let mut form = div()
            .id("math-editor-form")
            .debug_selector(|| "math-form".into())
            .max_h(px(form_max_height))
            .overflow_y_scroll()
            .track_scroll(&self.form_scroll)
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .text_size(style::type_size::BODY)
            // The dialog title already names the object; the inspector keeps the heading.
            .when(!self.dialog, |form| {
                form.child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(self.draft.kind.label()),
                )
            });
        for spec in self.draft.specs() {
            form = form.child(
                div()
                    .text_color(hex(color::TEXT_SECONDARY))
                    .child(spec.label),
            );
            if spec.id == "vertices" {
                for (i, states) in self.vertices.iter().enumerate() {
                    form = form.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(format!("{}", i + 1))
                            .child(
                                div()
                                    .flex_1()
                                    .child("X (units)")
                                    .child(NumberInput::new(&states[0]).small()),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .child("Y (units)")
                                    .child(NumberInput::new(&states[1]).small()),
                            ),
                    );
                }
                form = form.child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            Button::new("math-vertex-add")
                                .small()
                                .label("Add vertex")
                                .disabled(self.vertices.len() >= 64)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.add_vertex(["0", "0"], window, cx);
                                    this.sync_vertices(cx);
                                })),
                        )
                        .child(
                            Button::new("math-vertex-remove")
                                .small()
                                .label("Remove last")
                                .disabled(self.vertices.len() <= 3)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.vertices.pop();
                                    this.sync_vertices(cx);
                                })),
                        ),
                );
            } else if spec.id == "source" {
                if let Some(source) = &self.source {
                    // LaTeX source is code: set it in the mono face.
                    form = form.child(
                        div()
                            .font_family(cx.theme().mono_font_family.clone())
                            .child(Textarea::new(source)),
                    );
                }
            } else if let Some(state) = self.fields.get(spec.id) {
                form = if spec.numeric {
                    form.child(NumberInput::new(state).small())
                } else {
                    form.child(Input::new(state).small())
                };
            }
        }
        // Colour and Write on come before the palette so they are never the part that scrolls
        // out of view; one compact row keeps them discoverable.
        form = form.child(
            div()
                .debug_selector(|| "math-colour-row".into())
                .flex()
                .flex_wrap()
                .items_center()
                .gap_x_4()
                .gap_y_2()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().text_color(hex(color::TEXT_SECONDARY)).child("Color"))
                        .child(super::inspector_units::framed_swatch(
                            ColorPicker::new(&self.color),
                            false,
                            cx,
                        ))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(self.draft.color.clone()),
                        ),
                )
                .when(
                    self.edit.is_none() && self.draft.kind == MathObjectKind::Latex,
                    |row| {
                        row.child(
                            div().debug_selector(|| "math-write-on".into()).child(
                                Switch::new("math-write")
                                    .label("Write on (1 second)")
                                    .checked(self.draft.write_on)
                                    .on_click(cx.listener(|this, checked, _, cx| {
                                        this.draft.write_on = *checked;
                                        cx.notify();
                                    })),
                            ),
                        )
                    },
                ),
        );
        // The assist panel is tall, so it comes after Colour and Write on: it is what scrolls.
        if let Some(assist) = &self.assist {
            form = form.child(assist.clone());
        }
        let mut palette = div()
            .debug_selector(|| "math-palette".into())
            .flex()
            .flex_col()
            .gap_2();
        if self.draft.kind == MathObjectKind::Latex {
            let mut chip = 0;
            for (title, items, replace) in [
                ("Snippets", LATEX_SNIPPETS, false),
                ("Templates", LATEX_TEMPLATES, true),
            ] {
                palette = palette.child(
                    div()
                        .text_role(style::Role::Heading)
                        .text_color(hex(color::TEXT))
                        .child(title),
                );
                let mut buttons = div().flex().flex_wrap().gap_1();
                for &(label, value) in items {
                    // A real button, so a chip takes Tab focus and Enter/Space inserts it.
                    let focus = self.chip_focus[chip].clone();
                    chip += 1;
                    buttons = buttons.child(
                        Button::new(SharedString::from(format!("{title}-{label}")))
                            .track_focus(&focus)
                            .debug_selector(move || format!("math-chip-{label}"))
                            .small()
                            .outline()
                            .label(label)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.snippet(value, replace, window, cx);
                            })),
                    );
                }
                palette = palette.child(buttons);
            }
        }
        let has_palette = self.draft.kind == MathObjectKind::Latex;
        if has_palette && !two_columns {
            form = form.child(palette.into_any_element());
            palette = div();
        }
        let form = div()
            .relative()
            .child(form)
            .child(Scrollbar::vertical(&self.form_scroll));
        // The stage has the canvas aspect, so the preview shows what the frame will frame.
        let aspect = self.size.0.max(1) as f32 / self.size.1.max(1) as f32;
        let mut stage = div()
            .h(px(PREVIEW_HEIGHT))
            .w(px(PREVIEW_HEIGHT * aspect))
            .max_w_full()
            .flex_shrink_0()
            .bg(hex(color::STAGE))
            .flex()
            .items_center()
            .justify_center();
        if let Some(preview) = &self.image {
            stage = stage.child(
                img(preview.clone())
                    .max_w_full()
                    .max_h_full()
                    .object_fit(ObjectFit::Contain),
            );
        }
        let image = div().w_full().flex().justify_center().child(stage);
        let left = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_2()
            .child(form)
            .child(image)
            .child(
                div()
                    .id("math-preview-caption")
                    .flex()
                    .justify_center()
                    .text_role(style::Role::Label)
                    .text_color(hex(color::TEXT_SECONDARY))
                    .child(if self.busy {
                        "Updating preview…"
                    } else {
                        "Live preview"
                    })
                    .tooltip(|window, cx| {
                        gpui_component::tooltip::Tooltip::new(
                            "Ten units across the short edge, positive Y upward",
                        )
                        .build(window, cx)
                    }),
            );
        let mut result = div().flex().flex_col().gap_2().child(
            div().flex().items_start().gap_3().child(left).when(
                has_palette && two_columns,
                |row| {
                    row.child(
                        div()
                            .relative()
                            .w(px(MATH_PALETTE_WIDTH * style::ui_scale()))
                            .flex_shrink_0()
                            .child(
                                div()
                                    .id("math-palette-scroll")
                                    .max_h(px(form_max_height + 190.))
                                    .overflow_y_scroll()
                                    .track_scroll(&self.palette_scroll)
                                    .p_2()
                                    .text_size(style::type_size::BODY)
                                    .child(palette),
                            )
                            .child(Scrollbar::vertical(&self.palette_scroll)),
                    )
                },
            ),
        );
        if let Some(error) = &self.error {
            result = result.child(div().text_color(cx.theme().danger).child(error.clone()));
        }
        result.child(
            div()
                .flex()
                .justify_end()
                .gap_2()
                .when(self.dialog, |row| {
                    row.child(
                        style::secondary_button("math-cancel", cx)
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                })
                .child(
                    style::primary_button("math-apply", cx)
                        .label(if self.edit.is_some() { "Apply" } else { "Add" })
                        .disabled(self.busy || self.error.is_some())
                        .on_click(cx.listener(Self::apply_click)),
                ),
        )
    }
}
impl MathEditor {
    fn apply_click(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.apply(window, cx);
    }
}
