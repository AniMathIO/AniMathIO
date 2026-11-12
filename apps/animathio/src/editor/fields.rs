//! Inspector controls built from gpui-component widgets.
//!
//! Numbers use [`NumberInput`], text uses [`Input`], colours use
//! [`ColorPicker`], enums use [`Select`] and booleans use [`Switch`]. Every
//! committed edit still goes through [`Command`] via the editor, so undo/redo,
//! the control protocol and keyframing behave exactly as before.
//!
//! The widgets are stateful entities, so they live in a [`FieldPool`] keyed by
//! `clip id + parameter`. A control is created the first time it is rendered and
//! re-synchronised from the project on every frame it is not being edited.

use gpui_component::IndexPath;
use gpui_component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_component::input::{
    Input, InputEvent, InputState, NumberInput, NumberInputEvent, StepAction,
};
use gpui_component::select::{SearchableVec, Select, SelectEvent, SelectState};
use gpui_component::switch::Switch;

use super::*;

const CONTROL_W: f32 = 120.0;

type EnumState = SelectState<SearchableVec<String>>;

/// A text or number input plus the text it last held from the model.
struct InputField {
    state: Entity<InputState>,
    /// Model text last written to (or committed from) the input.
    synced: String,
    target: EditTarget,
    step: f64,
    _subscriptions: Vec<Subscription>,
}

struct ColorField {
    state: Entity<ColorPickerState>,
    synced: [u8; 4],
    _subscription: Subscription,
}

struct EnumField {
    state: Entity<EnumState>,
    _subscription: Subscription,
}

/// Live widget state for the inspector, keyed by `c<clip>:<kind>:<name>`.
#[derive(Default)]
pub(super) struct FieldPool {
    inputs: BTreeMap<String, InputField>,
    colors: BTreeMap<String, ColorField>,
    enums: BTreeMap<String, EnumField>,
}

impl FieldPool {
    /// Whether a text/number input currently owns keyboard focus, in which case
    /// global editor shortcuts must stay out of the way.
    pub(super) fn any_focused(&self, window: &Window, cx: &App) -> bool {
        self.inputs
            .values()
            .any(|field| field.state.read(cx).focus_handle(cx).is_focused(window))
    }

    /// Drop widgets that belong to clips other than `clip`.
    fn retain_clip(&mut self, clip: ClipId) {
        let prefix = format!("c{}:", clip.0);
        self.inputs.retain(|key, _| key.starts_with(&prefix));
        self.colors.retain(|key, _| key.starts_with(&prefix));
        self.enums.retain(|key, _| key.starts_with(&prefix));
    }
}

/// Shortest decimal rendering of `value` with at most four fractional digits.
pub(super) fn format_number(value: f64) -> String {
    let text = format!("{value:.4}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    match text {
        "" | "-" | "-0" => "0".to_owned(),
        other => other.to_owned(),
    }
}

/// Whether `text` could still become a number while the user is typing it.
fn is_numeric_prefix(text: &str) -> bool {
    text.chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E'))
}

impl Editor {
    /// Get (or create) the input for `key`, keeping it in step with the model.
    fn input_state(
        &mut self,
        key: &str,
        target: EditTarget,
        text: String,
        step: f64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if !self.fields.inputs.contains_key(key) {
            let numeric = target.is_numeric();
            let initial = text.clone();
            let state = cx.new(|cx| {
                let state = InputState::new(window, cx).default_value(initial);
                if numeric {
                    state.validate(|value, _| is_numeric_prefix(value))
                } else {
                    state
                }
            });
            let mut subscriptions = Vec::new();
            let commit_key = key.to_owned();
            subscriptions.push(cx.subscribe_in(
                &state,
                window,
                move |this, _state, event: &InputEvent, _window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        this.commit_input(&commit_key, cx);
                    }
                },
            ));
            let step_key = key.to_owned();
            subscriptions.push(cx.subscribe_in(
                &state,
                window,
                move |this, _state, event: &NumberInputEvent, window, cx| {
                    let NumberInputEvent::Step(action) = event;
                    this.step_input(&step_key, *action, window, cx);
                },
            ));
            self.fields.inputs.insert(
                key.to_owned(),
                InputField {
                    state,
                    synced: text.clone(),
                    target: target.clone(),
                    step,
                    _subscriptions: subscriptions,
                },
            );
        }
        let field = self
            .fields
            .inputs
            .get_mut(key)
            .expect("input field was just inserted");
        field.target = target;
        field.step = step;
        let focused = field.state.read(cx).focus_handle(cx).is_focused(window);
        if !focused && field.state.read(cx).value().as_ref() != text {
            field
                .state
                .update(cx, |state, cx| state.set_value(text.clone(), window, cx));
        }
        if !focused {
            field.synced = text;
        }
        field.state.clone()
    }

    /// Commit the typed text of an input through the shared edit path, but only
    /// when it differs from what the model last showed.
    fn commit_input(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(field) = self.fields.inputs.get_mut(key) else {
            return;
        };
        let buffer = field.state.read(cx).value().to_string();
        if buffer == field.synced {
            return;
        }
        field.synced.clone_from(&buffer);
        let target = field.target.clone();
        self.editing = Some(TextEdit { target, buffer });
        self.commit_editing(cx);
    }

    /// The +/- buttons and the Up/Down keys of a [`NumberInput`].
    fn step_input(
        &mut self,
        key: &str,
        action: StepAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(field) = self.fields.inputs.get_mut(key) else {
            return;
        };
        let current = field
            .state
            .read(cx)
            .value()
            .trim()
            .parse::<f64>()
            .unwrap_or(0.0);
        let delta = match action {
            StepAction::Increment => field.step,
            StepAction::Decrement => -field.step,
        };
        let text = format_number(current + delta);
        field
            .state
            .update(cx, |state, cx| state.set_value(text.clone(), window, cx));
        field.synced.clone_from(&text);
        let target = field.target.clone();
        self.editing = Some(TextEdit {
            target,
            buffer: text,
        });
        self.commit_editing(cx);
    }

    fn number_control(
        &mut self,
        key: &str,
        target: EditTarget,
        value: f64,
        step: f64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let state = self.input_state(key, target, format_number(value), step, window, cx);
        div()
            .w(px(CONTROL_W))
            .child(NumberInput::new(&state).small())
    }

    fn text_control(
        &mut self,
        key: &str,
        target: EditTarget,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let state = self.input_state(key, target, value.to_owned(), 0.0, window, cx);
        div().w(px(CONTROL_W)).child(Input::new(&state).small())
    }

    fn color_control(
        &mut self,
        id: ClipId,
        name: &str,
        color: [u8; 4],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let key = format!("c{}:color:{name}", id.0);
        if !self.fields.colors.contains_key(&key) {
            let state = cx.new(|cx| ColorPickerState::new(window, cx));
            let param = name.to_owned();
            let subscription = cx.subscribe_in(
                &state,
                window,
                move |this, _state, event: &ColorPickerEvent, _window, cx| {
                    let ColorPickerEvent::Change(Some(hsla)) = event else {
                        return;
                    };
                    let rgba = Rgba::from(*hsla);
                    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
                    let next = [
                        channel(rgba.r),
                        channel(rgba.g),
                        channel(rgba.b),
                        channel(rgba.a),
                    ];
                    let key = format!("c{}:color:{param}", id.0);
                    let Some(field) = this.fields.colors.get_mut(&key) else {
                        return;
                    };
                    if field.synced == next {
                        return;
                    }
                    field.synced = next;
                    this.execute(
                        &this.plugin_param_command(
                            id,
                            param.clone(),
                            json!([next[0], next[1], next[2], next[3]]),
                        ),
                        cx,
                    );
                },
            );
            state.update(cx, |state, cx| state.set_value(to_hsla(color), window, cx));
            self.fields.colors.insert(
                key.clone(),
                ColorField {
                    state,
                    synced: color,
                    _subscription: subscription,
                },
            );
        }
        let field = self
            .fields
            .colors
            .get_mut(&key)
            .expect("color field was just inserted");
        if field.synced != color {
            field.synced = color;
            field
                .state
                .update(cx, |state, cx| state.set_value(to_hsla(color), window, cx));
        }
        let hex = format!(
            "#{:02x}{:02x}{:02x}{:02x}",
            color[0], color[1], color[2], color[3]
        );
        div()
            .w(px(CONTROL_W))
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .child(ColorPicker::new(&field.state))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(hex),
            )
    }

    fn enum_control(
        &mut self,
        id: ClipId,
        name: &str,
        options: &[String],
        current: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let key = format!("c{}:enum:{name}", id.0);
        if !self.fields.enums.contains_key(&key) {
            let selected = options
                .iter()
                .position(|option| option == current)
                .map(IndexPath::new);
            let items = SearchableVec::new(options.to_vec());
            let state = cx.new(|cx| SelectState::new(items, selected, window, cx));
            let param = name.to_owned();
            let subscription = cx.subscribe_in(
                &state,
                window,
                move |this, _state, event: &SelectEvent<SearchableVec<String>>, _window, cx| {
                    let SelectEvent::Confirm(Some(value)) = event else {
                        return;
                    };
                    this.execute(
                        &this.plugin_param_command(id, param.clone(), json!(value)),
                        cx,
                    );
                },
            );
            self.fields.enums.insert(
                key.clone(),
                EnumField {
                    state,
                    _subscription: subscription,
                },
            );
        }
        let field = self
            .fields
            .enums
            .get(&key)
            .expect("enum field was just inserted");
        let wanted = current.to_owned();
        if field.state.read(cx).selected_value() != Some(&wanted) {
            field.state.update(cx, |state, cx| {
                state.set_selected_value(&wanted, window, cx);
            });
        }
        div().w(px(CONTROL_W)).child(Select::new(&field.state))
    }

    fn bool_control(id: ClipId, name: &str, value: bool, cx: &mut Context<Self>) -> Div {
        let param = name.to_owned();
        div().w(px(CONTROL_W)).child(
            Switch::new(SharedString::from(format!("switch-{}-{name}", id.0)))
                .checked(value)
                .on_click(cx.listener(move |this, checked: &bool, _window, cx| {
                    this.execute(
                        &this.plugin_param_command(id, param.clone(), json!(*checked)),
                        cx,
                    );
                })),
        )
    }

    /// Keyframe diamond for one animatable property: filled when a keyframe sits
    /// at the playhead.
    fn row_diamond(&self, id: ClipId, target: &Target, cx: &mut Context<Self>) -> Button {
        let active = self
            .find_clip(id)
            .is_some_and(|clip| inspector::keyframed(clip, target, self.position));
        let element_id = SharedString::from(format!("kf-{}-{target:?}", id.0));
        let target = target.clone();
        Button::new(element_id)
            .ghost()
            .compact()
            .icon(icons::diamond(active))
            .tooltip(if active {
                "Remove keyframe at playhead"
            } else {
                "Add keyframe at playhead"
            })
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.toggle_keyframe(id, target.clone(), cx);
            }))
    }

    /// One `[diamond] label ........ control` inspector row.
    fn param_row(
        diamond: Option<Button>,
        label: &str,
        control: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .h(px(30.))
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .px_2()
            .child(div().w(px(26.)).children(diamond))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_color(cx.theme().sidebar_foreground)
                    .child(label.to_owned()),
            )
            .child(control)
    }

    /// A numeric builtin field (transform component or numeric parameter).
    fn number_row(
        &mut self,
        id: ClipId,
        field: Field,
        label: &str,
        value: f64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let key = match &field {
            Field::Transform(property) => format!("c{}:f:{property:?}", id.0),
            Field::Param(name) => format!("c{}:p:{name}", id.0),
        };
        let target = match &field {
            Field::Transform(property) => Target::Transform(*property),
            Field::Param(name) => Target::Param(name.clone()),
        };
        let control = self.number_control(
            &key,
            EditTarget::Field(field.clone()),
            value,
            field_step(&field),
            window,
            cx,
        );
        let diamond = self.row_diamond(id, &target, cx);
        Self::param_row(Some(diamond), label, control, cx)
    }

    pub(super) fn transform_rows(
        &mut self,
        id: ClipId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        let Some(clip) = self.find_clip(id).cloned() else {
            return Vec::new();
        };
        let transform = [
            (Field::Transform(TransformProperty::X), "Position X"),
            (Field::Transform(TransformProperty::Y), "Position Y"),
            (Field::Transform(TransformProperty::ScaleX), "Scale X"),
            (Field::Transform(TransformProperty::ScaleY), "Scale Y"),
            (Field::Transform(TransformProperty::Rotation), "Rotation"),
            (Field::Transform(TransformProperty::Opacity), "Opacity"),
        ];
        transform
            .into_iter()
            .map(|(field, label)| {
                let value = field_value(&clip, &field);
                self.number_row(id, field, label, value, window, cx)
            })
            .collect()
    }

    pub(super) fn numeric_param_rows(
        &mut self,
        id: ClipId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        let Some(clip) = self.find_clip(id).cloned() else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        for (name, value) in &clip.params {
            if let Some(value) = value.as_number() {
                let field = Field::Param(name.clone());
                rows.push(self.number_row(id, field, name, value, window, cx));
            }
        }
        rows
    }

    /// Gain and mute rows for audio clips.
    pub(super) fn audio_rows(
        &mut self,
        id: ClipId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        let Some(clip) = self.find_clip(id).cloned() else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        for (name, label) in [("gain", "Gain"), ("muted", "Muted")] {
            let field = Field::Param(name.to_owned());
            let value = field_value(&clip, &field);
            if name == "muted" {
                let control = div().w(px(CONTROL_W)).child(
                    Switch::new(SharedString::from(format!("muted-{}", id.0)))
                        .checked(value != 0.0)
                        .on_click(cx.listener(move |this, checked: &bool, _window, cx| {
                            this.commit_field(
                                id,
                                &Field::Param("muted".to_owned()),
                                f64::from(u8::from(*checked)),
                                cx,
                            );
                        })),
                );
                rows.push(Self::param_row(None, label, control, cx));
            } else {
                rows.push(self.number_row(id, field, label, value, window, cx));
            }
        }
        rows
    }

    pub(super) fn plugin_rows(
        &mut self,
        id: ClipId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        let Some(clip) = self.find_clip(id) else {
            return Vec::new();
        };
        let ClipKind::Plugin { plugin_id, .. } = &clip.kind else {
            return Vec::new();
        };
        let Some(descriptor) = self.plugins.iter().find(|plugin| &plugin.id == plugin_id) else {
            return vec![div().px_2().child("Plugin not loaded")];
        };
        let models = descriptor.models();
        models
            .iter()
            .map(|model| self.plugin_row(id, model, window, cx))
            .collect()
    }

    fn plugin_row(
        &mut self,
        id: ClipId,
        model: &ParamModel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let name = model.id().to_owned();
        let diamond = self.row_diamond(id, &Target::Param(name.clone()), cx);
        let control = match model {
            ParamModel::Number { step, .. } => {
                let value = self.plugin_number_value(id, &name);
                self.number_control(
                    &format!("c{}:pn:{name}", id.0),
                    EditTarget::PluginNumber {
                        id,
                        name: name.clone(),
                    },
                    value,
                    *step,
                    window,
                    cx,
                )
            }
            ParamModel::Color { default, .. } => {
                let color = self.plugin_color_value(
                    id,
                    &name,
                    [default.r, default.g, default.b, default.a],
                );
                self.color_control(id, &name, color, window, cx)
            }
            ParamModel::Text { default, .. } => {
                let value = self.plugin_text_value(id, &name, default);
                self.text_control(
                    &format!("c{}:pt:{name}", id.0),
                    EditTarget::PluginText {
                        id,
                        name: name.clone(),
                    },
                    &value,
                    window,
                    cx,
                )
            }
            ParamModel::Enum {
                options, default, ..
            } => {
                let current = self.plugin_text_value(id, &name, default);
                self.enum_control(id, &name, options, &current, window, cx)
            }
            ParamModel::Bool { default, .. } => {
                let value = self.plugin_bool_value(id, &name, *default);
                Self::bool_control(id, &name, value, cx)
            }
            ParamModel::Point { .. } => {
                let point = self.plugin_point_value(id, &name);
                let axes = [0usize, 1].map(|axis| {
                    self.number_control(
                        &format!("c{}:pp{axis}:{name}", id.0),
                        EditTarget::PluginPoint {
                            id,
                            name: name.clone(),
                            axis,
                        },
                        f64::from(point[axis]),
                        0.1,
                        window,
                        cx,
                    )
                });
                let [x, y] = axes;
                div()
                    .flex()
                    .flex_row()
                    .gap_1()
                    .child(x.w(px(CONTROL_W / 2.0 + 24.0)))
                    .child(y.w(px(CONTROL_W / 2.0 + 24.0)))
            }
            ParamModel::Asset { media_type, .. } => div()
                .w(px(CONTROL_W))
                .child(self.plugin_asset_control(id, name.clone(), media_type, cx)),
        };
        Self::param_row(Some(diamond), model.label(), control, cx)
    }

    /// Inspector panel: header, collapsible sections and widgets for the
    /// selected clip.
    pub(super) fn inspector(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let mut pane = div()
            .w(px(self.dock.right_width))
            .h_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(cx.theme().sidebar)
            .border_l_1()
            .border_color(cx.theme().sidebar_border)
            .text_color(cx.theme().sidebar_foreground);
        let Some(id) = self.selected else {
            let recent = div()
                .flex()
                .flex_col()
                .children(self.recent.paths.iter().map(|path| {
                    let open = path.clone();
                    div()
                        .px_2()
                        .py_1()
                        .cursor_pointer()
                        .hover(|this| this.bg(cx.theme().list_hover))
                        .child(path.display().to_string())
                        .id("recent-item")
                        .on_click(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
                            this.open_path(&open, cx);
                        }))
                }));
            return pane
                .child(
                    div()
                        .px_2()
                        .py_2()
                        .text_color(cx.theme().muted_foreground)
                        .child("Select a clip to edit its properties"),
                )
                .child(self.section("recent", "Recent projects", recent, cx));
        };
        self.fields.retain_clip(id);
        let Some(clip) = self.find_clip(id).cloned() else {
            return pane.child("No clip selected");
        };
        pane = pane.child(
            div()
                .px_2()
                .py_2()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .border_b_1()
                .border_color(cx.theme().sidebar_border)
                .child(icons::icon(icons::kind_icon(&clip.kind)).small())
                .child(
                    div()
                        .flex_1()
                        .overflow_hidden()
                        .child(clip_label(&self.project, &clip)),
                )
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(kind_label(&clip.kind)),
                ),
        );
        if let ClipKind::Error(message) = &clip.kind {
            pane = pane.child(
                div()
                    .px_2()
                    .py_1()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .text_color(cx.theme().danger)
                    .child(icons::icon(IconName::TriangleAlert).small())
                    .child(message.clone()),
            );
        }
        let transform_rows = self.transform_rows(id, window, cx);
        let transform = self.section(
            "transform",
            "Transform",
            div().flex().flex_col().children(transform_rows),
            cx,
        );
        let mut body = div()
            .id("inspector-scroll")
            .flex_1()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .child(transform);
        if matches!(clip.kind, ClipKind::Plugin { .. }) {
            let rows = self.plugin_rows(id, window, cx);
            body = body.child(self.section(
                "params",
                "Plugin parameters",
                div().flex().flex_col().children(rows),
                cx,
            ));
        } else {
            let rows = self.numeric_param_rows(id, window, cx);
            if !rows.is_empty() {
                body = body.child(self.section(
                    "params",
                    "Parameters",
                    div().flex().flex_col().children(rows),
                    cx,
                ));
            }
        }
        if matches!(clip.kind, ClipKind::Audio(_)) {
            let rows = self.audio_rows(id, window, cx);
            body = body.child(self.section(
                "audio",
                "Audio",
                div().flex().flex_col().children(rows),
                cx,
            ));
        }
        if !clip.effects.is_empty() {
            let rows = self.effect_rows(id, cx);
            body = body.child(self.section(
                "effects",
                "Effects",
                div().flex().flex_col().children(rows),
                cx,
            ));
        }
        if let Some(selection) = self.selected_keyframe.clone()
            && selection.clip_id == id
        {
            body = body.child(
                self.section(
                    "keyframe",
                    "Keyframe",
                    div()
                        .flex()
                        .flex_col()
                        .children(self.keyframe_menu(&selection, cx)),
                    cx,
                ),
            );
        }
        pane.child(body)
    }
}

pub(super) fn to_hsla(color: [u8; 4]) -> Hsla {
    Hsla::from(Rgba {
        r: f32::from(color[0]) / 255.0,
        g: f32::from(color[1]) / 255.0,
        b: f32::from(color[2]) / 255.0,
        a: f32::from(color[3]) / 255.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[core::prelude::v1::test]
    fn numbers_format_compactly() {
        assert_eq!(format_number(1.0), "1");
        assert_eq!(format_number(0.5), "0.5");
        assert_eq!(format_number(-0.00001), "0");
        assert_eq!(format_number(12.34567), "12.3457");
        assert_eq!(format_number(-3.0), "-3");
    }

    #[core::prelude::v1::test]
    fn numeric_inputs_accept_partial_numbers_only() {
        for ok in ["", "-", "1.", "-0.5", "1e3"] {
            assert!(is_numeric_prefix(ok), "{ok}");
        }
        for bad in ["a", "1 2", "0x1", "1,5"] {
            assert!(!is_numeric_prefix(bad), "{bad}");
        }
    }
}
