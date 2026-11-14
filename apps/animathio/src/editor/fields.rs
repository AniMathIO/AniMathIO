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

use gpui_component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_component::input::{
    Input, InputEvent, InputState, NumberInput, NumberInputEvent, StepAction,
};
use gpui_component::select::{SearchableVec, Select, SelectEvent, SelectItem, SelectState};
use gpui_component::slider::{Slider, SliderEvent, SliderState};
use gpui_component::switch::Switch;
use gpui_component::{Disableable, IndexPath};

use super::*;
use crate::style::{color, hex};

const CONTROL_W: f32 = 120.0;

type EnumState = SelectState<SearchableVec<String>>;

/// A text or number input plus the text it last held from the model.
struct InputField {
    state: Entity<InputState>,
    /// Model text last written to (or committed from) the input.
    synced: String,
    target: InputTarget,
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

#[derive(Clone, PartialEq)]
struct AssetChoice {
    id: Option<String>,
    label: String,
}
impl SelectItem for AssetChoice {
    type Value = Option<String>;
    fn title(&self) -> SharedString {
        self.label.clone().into()
    }
    fn value(&self) -> &Self::Value {
        &self.id
    }
}
struct AssetField {
    state: Entity<SelectState<SearchableVec<AssetChoice>>>,
    choices: Vec<AssetChoice>,
    _subscription: Subscription,
}

/// Live widget state for the inspector, keyed by `c<clip>:<kind>:<name>`.
#[derive(Default)]
pub(super) struct FieldPool {
    pub(super) scrub: Option<NumberScrub>,
    inputs: BTreeMap<String, InputField>,
    colors: BTreeMap<String, ColorField>,
    enums: BTreeMap<String, EnumField>,
    assets: BTreeMap<String, AssetField>,
    sliders: BTreeMap<String, (Entity<SliderState>, Subscription)>,
}

pub(super) struct NumberScrub {
    target: InputTarget,
    origin: f32,
    value: f64,
    step: f64,
}

fn scrub_value(value: f64, delta: f32, step: f64, shift: bool, ctrl: bool) -> f64 {
    let sensitivity = if shift {
        0.1
    } else if ctrl {
        10.0
    } else {
        1.0
    };
    value + f64::from(delta) * step * sensitivity
}

impl FieldPool {
    /// Whether a text/number input currently owns keyboard focus, in which case
    /// global editor shortcuts must stay out of the way.
    pub(super) fn any_focused(&self, window: &Window, cx: &App) -> bool {
        self.inputs
            .values()
            .any(|field| field.state.read(cx).focus_handle(cx).is_focused(window))
    }

    pub(super) fn retain_clips(&mut self, clips: &BTreeSet<ClipId>) {
        let keep = |key: &String| {
            !key.starts_with('c')
                || clips
                    .iter()
                    .any(|id| key.starts_with(&format!("c{}:", id.0)))
        };
        self.inputs.retain(|key, _| keep(key));
        self.colors.retain(|key, _| keep(key));
        self.sliders.retain(|key, _| keep(key));
        self.assets.retain(|key, _| keep(key));
        self.enums.retain(|key, _| keep(key));
    }

    /// Drop widgets that belong to clips other than `clip`.
    fn retain_clip(&mut self, clip: ClipId) {
        let prefix = format!("c{}:", clip.0);
        self.inputs
            .retain(|key, _| !key.starts_with('c') || key.starts_with(&prefix));
        self.colors.retain(|key, _| key.starts_with(&prefix));
        self.sliders
            .retain(|key, _| !key.starts_with('c') || key.starts_with(&prefix));
        self.assets.retain(|key, _| key.starts_with(&prefix));
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
    pub(super) fn input_state(
        &mut self,
        key: &str,
        target: InputTarget,
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
                        if commit_key == "timecode" {
                            this.viewer_state.timecode_editing = false;
                            cx.notify();
                        }
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
        self.commit_input_value(target, buffer, cx);
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
        self.commit_input_value(target, text, cx);
    }

    pub(super) fn number_control(
        &mut self,
        key: &str,
        target: InputTarget,
        value: f64,
        step: f64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let scrub_target = target.clone();
        let slider = matches!(
            &target,
            InputTarget::Field {
                field: Field::Transform(TransformProperty::Opacity),
                ..
            } | InputTarget::SelectionField {
                field: Field::Transform(TransformProperty::Opacity)
            }
        );
        let state = self.input_state(key, target, format_number(value), step, window, cx);
        let slider_control = slider.then(|| {
            self.slider_control(
                key,
                Some(self.fields.inputs[key].target.clone()),
                value,
                window,
                cx,
            )
        });
        div()
            .w(px(CONTROL_W))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .id(SharedString::from(format!("scrub-{key}")))
                            .w(px(14.))
                            .h(px(24.))
                            .flex()
                            .items_center()
                            .cursor_ew_resize()
                            .child(
                                icons::icon(IconName::GripVertical)
                                    .xsmall()
                                    .text_color(hex(color::TEXT_MUTED)),
                            )
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                    this.pointer_down = true;
                                    this.fields.scrub = Some(NumberScrub {
                                        target: scrub_target.clone(),
                                        origin: f32::from(event.position.x),
                                        value,
                                        step,
                                    });
                                    cx.stop_propagation();
                                }),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(NumberInput::new(&state).small()),
                    ),
            )
            // Align the slider with the input, past the 14px scrub grip and the gap.
            .children(slider_control.map(|slider| div().pl(px(18.)).pb_1().child(slider)))
    }

    pub(super) fn slider_control(
        &mut self,
        key: &str,
        target: Option<InputTarget>,
        value: f64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        if !self.fields.sliders.contains_key(key) {
            let state = cx.new(|_| {
                SliderState::new()
                    .min(0.)
                    .max(1.)
                    .step(0.01)
                    .default_value(value as f32)
            });
            let sub = cx.subscribe(
                &state,
                move |this, _, event: &SliderEvent, cx| match event {
                    SliderEvent::Change(value) => {
                        if let Some(target) = &target {
                            if let Some(command) = this
                                .numeric_gesture_command(target.clone(), f64::from(value.start()))
                            {
                                this.preview_color(command, cx);
                            }
                        } else {
                            this.set_monitor_volume(f64::from(value.start()), cx);
                        }
                    }
                    SliderEvent::Release(_) => this.finish_color_edit(cx),
                },
            );
            self.fields.sliders.insert(key.to_owned(), (state, sub));
        }
        let state = self.fields.sliders[key].0.clone();
        if (f64::from(state.read(cx).value().start()) - value).abs() > 0.001 {
            state.update(cx, |state, cx| state.set_value(value as f32, window, cx));
        }
        div()
            .h(px(14.))
            .w_full()
            .child(Slider::new(&state).horizontal())
    }

    pub(super) fn move_number_scrub(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(scrub) = &self.fields.scrub else {
            return;
        };
        let value = scrub_value(
            scrub.value,
            f32::from(event.position.x) - scrub.origin,
            scrub.step,
            event.modifiers.shift,
            event.modifiers.control,
        );
        let target = scrub.target.clone();
        let command = self.numeric_gesture_command(target, value);
        if let Some(command) = command {
            self.preview_color(command, cx);
        }
    }

    fn numeric_gesture_command(&self, target: InputTarget, value: f64) -> Option<Command> {
        match target {
            InputTarget::Field { id, field } => self.scrub_field_command(id, &field, value),
            InputTarget::SelectionField { field } => {
                let commands = self
                    .selected_ids()
                    .into_iter()
                    .filter_map(|id| self.scrub_field_command(id, &field, value))
                    .collect();
                Some(Command::Batch { commands })
            }
            InputTarget::AudioEffectParam {
                chain,
                effect,
                name,
            } => self.audio_effect_chain_param_command(chain, effect, &name, value),
            InputTarget::PluginNumber { id, name } => self.plugin_number_command(id, &name, value),
            InputTarget::PluginPoint { id, name, axis } => {
                let mut point = self.plugin_point_value(id, &name);
                point[axis] = value as f32;
                Some(self.plugin_param_command(id, name, json!(point)))
            }
            _ => None,
        }
    }

    fn scrub_field_command(&self, id: ClipId, field: &Field, value: f64) -> Option<Command> {
        let value = field_model_value(field, value);
        let clip = self.find_clip(id)?;
        let (target, value) = match field {
            Field::Transform(property) => (
                Target::Transform(*property),
                if *property == TransformProperty::Opacity {
                    value.clamp(0., 1.)
                } else {
                    value
                },
            ),
            Field::Param(name) => (Target::Param(name.clone()), value),
        };
        Some(inspector::numeric_edit_command(
            clip,
            target,
            value,
            self.position,
        ))
    }

    pub(super) fn text_control(
        &mut self,
        key: &str,
        target: InputTarget,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let state = self.input_state(key, target, value.to_owned(), 0.0, window, cx);
        div().w(px(CONTROL_W)).child(Input::new(&state).small())
    }

    pub(super) fn caption_color_control(
        &mut self,
        key: String,
        accent: bool,
        target: Option<TrackId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let color = if accent {
            self.caption_look.accent.unwrap_or([255, 214, 10, 255])
        } else {
            self.caption_look.color.unwrap_or([255; 4])
        };
        if !self.fields.colors.contains_key(&key) {
            let state = cx.new(|cx| ColorPickerState::new(window, cx));
            let event_key = key.clone();
            let subscription = cx.subscribe_in(
                &state,
                window,
                move |this, _, event: &ColorPickerEvent, _, cx| {
                    let ColorPickerEvent::Change(Some(hsla)) = event else {
                        return;
                    };
                    let rgba = Rgba::from(*hsla);
                    let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                    let next = [
                        channel(rgba.r),
                        channel(rgba.g),
                        channel(rgba.b),
                        channel(rgba.a),
                    ];
                    let Some(field) = this.fields.colors.get_mut(&event_key) else {
                        return;
                    };
                    if field.synced == next {
                        return;
                    }
                    field.synced = next;
                    this.apply_look_change(
                        if accent {
                            caption_styles::LookChange::Accent(next)
                        } else {
                            caption_styles::LookChange::Text(next)
                        },
                        target,
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
        let field = self.fields.colors.get_mut(&key).expect("inserted picker");
        if field.synced != color {
            field.synced = color;
            field
                .state
                .update(cx, |state, cx| state.set_value(to_hsla(color), window, cx));
        }
        div().child(ColorPicker::new(&field.state))
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
                    this.preview_color(
                        this.plugin_param_command(
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

    pub(super) fn plugin_asset_control(
        &mut self,
        id: ClipId,
        name: String,
        media_type: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let key = format!("c{}:asset:{name}", id.0);
        let current = self.plugin_asset_value(id, &name);
        let mut choices = vec![AssetChoice {
            id: None,
            label: "None".into(),
        }];
        choices.extend(
            self.matching_asset_ids(media_type)
                .into_iter()
                .map(|asset| AssetChoice {
                    label: self
                        .project
                        .assets()
                        .get(&asset)
                        .expect("matching asset exists")
                        .name
                        .clone(),
                    id: Some(asset.as_str().to_owned()),
                }),
        );
        if let Some(missing) = &current
            && !choices.iter().any(|choice| choice.id == current)
        {
            choices.push(AssetChoice {
                id: current.clone(),
                label: format!("Missing: {missing}"),
            });
        }
        if !self.fields.assets.contains_key(&key) {
            let selected = choices
                .iter()
                .position(|choice| choice.id == current)
                .map(IndexPath::new);
            let state = cx.new(|cx| {
                SelectState::new(SearchableVec::new(choices.clone()), selected, window, cx)
            });
            let subscription = cx.subscribe_in(
                &state,
                window,
                move |this, _, event: &SelectEvent<SearchableVec<AssetChoice>>, _, cx| {
                    let SelectEvent::Confirm(Some(value)) = event else {
                        return;
                    };
                    this.execute(
                        &this.plugin_param_command(id, name.clone(), json!(value)),
                        cx,
                    );
                },
            );
            self.fields.assets.insert(
                key.clone(),
                AssetField {
                    state,
                    choices: choices.clone(),
                    _subscription: subscription,
                },
            );
        }
        let field = self
            .fields
            .assets
            .get_mut(&key)
            .expect("asset field exists");
        if field.choices != choices {
            field.choices.clone_from(&choices);
            field.state.update(cx, |state, cx| {
                state.set_items(SearchableVec::new(choices), window, cx);
            });
        }
        if field.state.read(cx).selected_value() != Some(&current) {
            field.state.update(cx, |state, cx| {
                state.set_selected_value(&current, window, cx);
            });
        }
        div().w_full().child(Select::new(&field.state).small())
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
    #[allow(clippy::if_not_else)]
    pub(super) fn row_diamond(&self, id: ClipId, target: &Target, cx: &mut Context<Self>) -> Div {
        let active = self
            .find_clip(id)
            .is_some_and(|clip| inspector::keyframed(clip, target, self.position));
        let element_id = SharedString::from(format!("kf-{}-{target:?}", id.0));
        let mut controls = div().flex().items_center().gap_0();
        for (forward, icon) in [
            (false, IconName::ChevronLeft),
            (true, IconName::ChevronRight),
        ] {
            let time = self
                .find_clip(id)
                .and_then(|clip| inspector::adjacent_key(clip, target, self.position, forward));
            if !forward {
                controls = controls.child(
                    Button::new(SharedString::from(format!("prev-{element_id}")))
                        .ghost()
                        .compact()
                        .w(px(14.))
                        .icon(icons::icon(icon).xsmall())
                        .disabled(time.is_none())
                        .tooltip("Previous keyframe")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(time) = time {
                                this.seek_to(time, cx);
                            }
                        })),
                );
                let target = target.clone();
                controls = controls.child(
                    Button::new(element_id.clone())
                        .ghost()
                        .compact()
                        .w(px(18.))
                        .icon(icons::diamond(active).xsmall())
                        .tooltip(if active {
                            "Remove keyframe at playhead"
                        } else {
                            "Add keyframe at playhead"
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.toggle_keyframe(id, target.clone(), cx);
                        })),
                );
            } else {
                controls = controls.child(
                    Button::new(SharedString::from(format!("next-{element_id}")))
                        .ghost()
                        .compact()
                        .w(px(14.))
                        .icon(icons::icon(icon).xsmall())
                        .disabled(time.is_none())
                        .tooltip("Next keyframe")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(time) = time {
                                this.seek_to(time, cx);
                            }
                        })),
                );
            }
        }
        controls
    }

    /// One `[diamond] label ........ control` inspector row.
    pub(super) fn param_row(
        diamond: Option<Div>,
        label: impl IntoElement,
        control: impl IntoElement,
        _cx: &mut Context<Self>,
    ) -> Div {
        div()
            .min_h(px(32.))
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .px_2()
            .child(div().w(px(46.)).flex_shrink_0().children(diamond))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_size(px(11.))
                    .text_color(hex(color::TEXT_SECONDARY))
                    .child(label),
            )
            .child(control)
    }

    /// A row label that scrubs its number when dragged horizontally.
    pub(super) fn scrub_label(
        key: &str,
        text: String,
        target: InputTarget,
        value: f64,
        step: f64,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        div()
            .id(SharedString::from(format!("label-{key}")))
            .cursor_ew_resize()
            .child(text)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.pointer_down = true;
                    this.fields.scrub = Some(NumberScrub {
                        target: target.clone(),
                        origin: f32::from(event.position.x),
                        value,
                        step,
                    });
                    cx.stop_propagation();
                }),
            )
    }

    /// A `label ........ control` row without the keyframe column.
    pub(super) fn param_row_flush(label: impl IntoElement, control: impl IntoElement) -> Div {
        div()
            .min_h(px(32.))
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .px_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_size(px(11.))
                    .text_color(hex(color::TEXT_SECONDARY))
                    .child(label),
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
            InputTarget::Field {
                id,
                field: field.clone(),
            },
            value,
            field_step(&field),
            window,
            cx,
        );
        let diamond = self.row_diamond(id, &target, cx);
        let label = Self::scrub_label(
            &key,
            label.to_owned(),
            InputTarget::Field {
                id,
                field: field.clone(),
            },
            value,
            field_step(&field),
            cx,
        );
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
            (
                Field::Transform(TransformProperty::Rotation),
                "Rotation (deg)",
            ),
        ];
        transform
            .into_iter()
            .map(|(field, label)| {
                let target = match &field {
                    Field::Transform(property) => Target::Transform(*property),
                    Field::Param(name) => Target::Param(name.clone()),
                };
                // Sample in model units (radians), then convert for display.
                let model = inspector::sampled_numeric(
                    &clip,
                    &target,
                    self.position,
                    field_model_value(&field, field_value(&clip, &field)),
                );
                let value = field_display_value(&field, model);
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
                rows.push(Self::param_row(None, label.to_owned(), control, cx));
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
        let mut label = model.label().to_owned().into_any_element();
        let control = match model {
            ParamModel::Number { step, min, max, .. } => {
                let value = self.plugin_number_value(id, &name);
                let unit = (*min - 0.).abs() < f64::EPSILON && (*max - 1.).abs() < f64::EPSILON;
                label = Self::scrub_label(
                    &format!("c{}:pn:{name}", id.0),
                    model.label().to_owned(),
                    InputTarget::PluginNumber {
                        id,
                        name: name.clone(),
                    },
                    value,
                    *step,
                    cx,
                )
                .into_any_element();
                let number = self.number_control(
                    &format!("c{}:pn:{name}", id.0),
                    InputTarget::PluginNumber {
                        id,
                        name: name.clone(),
                    },
                    value,
                    *step,
                    window,
                    cx,
                );
                if unit {
                    let slider = self.slider_control(
                        &format!("c{}:ps:{name}", id.0),
                        Some(InputTarget::PluginNumber {
                            id,
                            name: name.clone(),
                        }),
                        value,
                        window,
                        cx,
                    );
                    number.child(slider)
                } else {
                    number
                }
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
                    InputTarget::PluginText {
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
                        InputTarget::PluginPoint {
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
                .child(self.plugin_asset_control(id, name.clone(), media_type, window, cx)),
        };
        Self::param_row(Some(diamond), label, control, cx)
    }

    /// Inspector for several selected clips: a read-only summary plus the
    /// transform (and, for all-audio selections, gain) fields every clip shares.
    /// A shared field shows the primary clip's value and applies an edit to every
    /// selected clip as one undo step.
    fn multi_inspector(
        &mut self,
        ids: &BTreeSet<ClipId>,
        pane: Div,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let clips: Vec<Clip> = ids
            .iter()
            .filter_map(|id| self.find_clip(*id).cloned())
            .collect();
        let Some(summary) = selection::summarize(&self.project, ids) else {
            return pane.child("No clip selected");
        };
        let rate = self.project.frame_rate();
        let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
        for clip in &clips {
            *kinds.entry(kind_label(&clip.kind)).or_default() += 1;
        }
        let kinds = kinds
            .into_iter()
            .map(|(kind, count)| format!("{count} {kind}"))
            .collect::<Vec<_>>()
            .join(", ");
        let muted = cx.theme().muted_foreground;
        let info = |label: &str, value: String| {
            div()
                .h(px(24.))
                .px_2()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(div().w(px(70.)).text_color(muted).child(label.to_owned()))
                .child(div().flex_1().min_w_0().truncate().child(value))
        };
        let header = div()
            .px_2()
            .py_2()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(cx.theme().sidebar_border)
            .child(icons::icon(IconName::Layers).small())
            .child(
                div()
                    .flex_1()
                    .child(format!("{} clips selected", summary.count)),
            );
        let details = div()
            .flex()
            .flex_col()
            .child(info("Kinds", kinds))
            .child(info(
                "Tracks",
                format!("{} of {}", summary.tracks, self.project.tracks().len()),
            ))
            .child(info(
                "Start",
                timecode::format_timecode(summary.start, rate),
            ))
            .child(info("End", timecode::format_timecode(summary.end, rate)))
            .child(info(
                "Duration",
                timecode::format_timecode(Time(summary.end.0 - summary.start.0), rate),
            ));
        let mut shared = vec![
            (Field::Transform(TransformProperty::X), "Position X"),
            (Field::Transform(TransformProperty::Y), "Position Y"),
            (Field::Transform(TransformProperty::ScaleX), "Scale X"),
            (Field::Transform(TransformProperty::ScaleY), "Scale Y"),
            (
                Field::Transform(TransformProperty::Rotation),
                "Rotation (deg)",
            ),
            (Field::Transform(TransformProperty::Opacity), "Opacity"),
        ];
        let all_audio = clips
            .iter()
            .all(|clip| matches!(clip.kind, ClipKind::Audio(_)));
        if all_audio {
            shared.push((Field::Param("gain".to_owned()), "Gain"));
        }
        let primary = self.selected.and_then(|id| self.find_clip(id).cloned());
        let rows: Vec<Div> = shared
            .into_iter()
            .map(|(field, label)| {
                let values: Vec<f64> = clips.iter().map(|c| field_value(c, &field)).collect();
                let value = primary
                    .as_ref()
                    .map_or(values[0], |clip| field_value(clip, &field));
                let mixed = values.iter().any(|v| (v - value).abs() > f64::EPSILON);
                let key = match &field {
                    Field::Transform(property) => format!("multi:f:{property:?}"),
                    Field::Param(name) => format!("multi:p:{name}"),
                };
                let control = self.number_control(
                    &key,
                    InputTarget::SelectionField {
                        field: field.clone(),
                    },
                    value,
                    field_step(&field),
                    window,
                    cx,
                );
                let label = if mixed {
                    format!("{label} (mixed)")
                } else {
                    label.to_owned()
                };
                Self::param_row_flush(label, control)
            })
            .collect();
        let body = div()
            .id("inspector-scroll")
            .flex_1()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .child(self.section("selection", "Selection", details, cx))
            .child(self.section(
                "shared",
                "Shared properties",
                div().flex().flex_col().children(rows),
                cx,
            ));
        pane.child(header).child(body)
    }

    /// Inspector panel: header, collapsible sections and widgets for the
    /// selected clip.
    pub(super) fn inspector(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let mut pane = div()
            .key_context("Inspector")
            .track_focus(&self.panel_focus[2])
            .w_full()
            .h_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(hex(color::PANEL))
            .border_l_1()
            .border_color(cx.theme().sidebar_border)
            .text_color(hex(color::TEXT))
            .text_size(px(12.));
        let Some(id) = self.selected else {
            if let Some(track) = self.audio_inspector_track
                && let Some(model) = self.project.tracks().iter().find(|t| t.id() == track)
            {
                let title = format!("{} — Audio effects", model.name());
                let rows = self.audio_chain_rows(AudioChain::Track(track), window, cx);
                return pane.child(
                    div()
                        .id("track-audio-inspector")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .child(self.section(
                            "track-audio",
                            &title,
                            div().flex().flex_col().children(rows),
                            cx,
                        )),
                );
            }
            return pane.child(
                div()
                    .p_3()
                    .text_color(cx.theme().muted_foreground)
                    .child("Select a clip to edit its properties"),
            );
        };
        let title_fields = self.title_field_rows(id, window, cx);
        let is_title = title_fields.is_some();
        if title_fields.is_none() {
            self.fields.retain_clip(id);
        }
        let ids = self.selected_ids();
        if ids.len() > 1 {
            if let Some(rows) = title_fields {
                return pane.child(rows);
            }
            return self.multi_inspector(&ids, pane, window, cx);
        }
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
            .p_1()
            .pb_3()
            .bg(hex(color::PANEL))
            .child(transform);
        if let Some(rows) = title_fields {
            body = body.child(rows);
        }
        if let Some(section) = self.caption_style_section(id, window, cx) {
            body = body.child(section);
        }
        let opacity = inspector::sampled_numeric(
            &clip,
            &Target::Transform(TransformProperty::Opacity),
            self.position,
            clip.transform.opacity,
        );
        let opacity_row = self.number_row(
            id,
            Field::Transform(TransformProperty::Opacity),
            "Opacity",
            opacity,
            window,
            cx,
        );
        body = body.child(self.section("opacity", "Compositing", div().child(opacity_row), cx));
        if matches!(clip.kind, ClipKind::Plugin { .. }) && !is_title {
            let rows = self.plugin_rows(id, window, cx);
            body = body.child(self.section(
                "params",
                "Text / Content",
                div().flex().flex_col().children(rows),
                cx,
            ));
        } else {
            let rows = self.numeric_param_rows(id, window, cx);
            if !rows.is_empty() {
                body = body.child(self.section(
                    "params",
                    "Content",
                    div().flex().flex_col().children(rows),
                    cx,
                ));
            }
        }
        if matches!(clip.kind, ClipKind::Audio(_)) {
            let mut rows = self.audio_rows(id, window, cx);
            rows.extend(self.audio_effect_rows(id, window, cx));
            body = body.child(self.section(
                "audio",
                "Audio",
                div().flex().flex_col().children(rows),
                cx,
            ));
        }
        {
            let mut rows = self.effect_rows(id, cx);
            if rows.is_empty() {
                rows.push(
                    div()
                        .px_3()
                        .text_size(px(11.))
                        .text_color(hex(color::TEXT_MUTED))
                        .child("Drop an effect from the media pool"),
                );
            }
            body = body.child(self.section(
                "effects",
                "Effects",
                div().flex().flex_col().children(rows),
                cx,
            ));
        }
        let transitions: Vec<_> = self
            .project
            .tracks()
            .iter()
            .flat_map(Track::transitions)
            .filter(|t| t.from == id || t.to == id)
            .map(|t| {
                div()
                    .px_3()
                    .text_size(px(11.))
                    .text_color(hex(color::TEXT_SECONDARY))
                    .child(format!(
                        "{:?} / {}",
                        t.kind,
                        timecode::format_timecode(t.duration, self.project.frame_rate())
                    ))
            })
            .collect();
        let transition_content = if transitions.is_empty() {
            div()
                .px_3()
                .text_size(px(11.))
                .text_color(hex(color::TEXT_MUTED))
                .child("No transition on this clip")
        } else {
            div().children(transitions)
        };
        body = body.child(self.section("transition", "Transition", transition_content, cx));
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
    fn scrubbing_modifiers_are_predictable() {
        assert!((scrub_value(0.5, 10., 0.01, false, false) - 0.6).abs() < 1e-9);
        assert!((scrub_value(0.5, 10., 0.01, true, false) - 0.51).abs() < 1e-9);
        assert!((scrub_value(0.5, -2., 0.01, false, true) - 0.3).abs() < 1e-9);
    }

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
