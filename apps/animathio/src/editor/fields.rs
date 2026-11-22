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
use gpui_component::input::{Input, InputEvent, InputState, StepAction, Textarea, TextareaState};
use gpui_component::select::{SearchableVec, Select, SelectEvent, SelectItem, SelectState};
use gpui_component::slider::{SliderEvent, SliderState};
use gpui_component::switch::Switch;
use gpui_component::{Disableable, IndexPath, Sizable};

use super::inspector_units::{
    SizeUnits, format_degrees, format_hex, framed_swatch, is_animatable, is_font_size_param,
    size_to_display, size_to_model,
};
use super::*;
use crate::style::{Role, TextRole, color, hex, size};

/// Width of the value field next to a slider, in rems (the field holds the
/// exact value with its unit suffix, which 4rem clipped).
const FIELD_W: f32 = 4.5;
/// Share of a row taken by the label column; a pair row (two fields) gives the
/// fields more, since its labels (Position, Scale, Size) are short.
const LABEL_FRACTION: f32 = 0.4;
const PAIR_LABEL_FRACTION: f32 = 0.3;
/// Width of the keyframe column, in rems: diamond plus hover-revealed arrows.
const KF_COL: f32 = 3.5;
/// Widths of the keyframe controls, in rems.
const KF_ARROW_W: f32 = 0.933;
const KF_DIAMOND_W: f32 = 1.2;
/// Gain slider bounds in dB (the field shows dB; the model stays linear).
const GAIN_DB_RANGE: (f64, f64, f64) = (-60., 24., 0.5);
/// The lane the overlay scrollbar of the inspector occupies; rows end left of it
/// so no field or border runs under the thumb.
pub(super) const SCROLLBAR_GUTTER: gpui::Pixels = gpui::px(14.);
/// Geometry of the "Add effect" menu, in rems.
const MENU_W_REMS: gpui::Rems = gpui::Rems(14.);
const MENU_MAX_H_REMS: gpui::Rems = gpui::Rems(16.);

type EnumState = SelectState<SearchableVec<EnumChoice>>;

/// What a row shows in its label column and, for plain text, the tooltip that
/// carries the whole label when the column truncates it.
pub(super) struct RowLabel {
    element: AnyElement,
    tooltip: Option<SharedString>,
}

impl RowLabel {
    /// A text label; its tooltip repeats the full text.
    pub(super) fn text(text: impl Into<SharedString>) -> Self {
        let text: SharedString = text.into();
        Self {
            element: text.clone().into_any_element(),
            tooltip: Some(text),
        }
    }

    /// A shortened `text` whose tooltip carries the `full` label.
    pub(super) fn shortened(text: impl Into<SharedString>, full: impl Into<SharedString>) -> Self {
        Self {
            element: text.into().into_any_element(),
            tooltip: Some(full.into()),
        }
    }
}

impl From<&str> for RowLabel {
    fn from(text: &str) -> Self {
        Self::text(text.to_owned())
    }
}

impl From<String> for RowLabel {
    fn from(text: String) -> Self {
        Self::text(text)
    }
}

impl From<SharedString> for RowLabel {
    fn from(text: SharedString) -> Self {
        Self::text(text)
    }
}

/// An element label brings its own tooltip (scrub labels) or none.
impl From<Stateful<Div>> for RowLabel {
    fn from(element: Stateful<Div>) -> Self {
        Self {
            element: element.into_any_element(),
            tooltip: None,
        }
    }
}

impl From<Div> for RowLabel {
    fn from(element: Div) -> Self {
        Self {
            element: element.into_any_element(),
            tooltip: None,
        }
    }
}

/// The label a plugin parameter shows in the narrow label column: the
/// explanatory parenthesis and redundant prefixes are dropped (the tooltip keeps
/// the full label).
fn short_label(label: &str) -> String {
    let base = label.split(" (").next().unwrap_or(label);
    match base {
        "Axes and labels color" => "Axes color".to_owned(),
        "Plot stroke width" => "Stroke width".to_owned(),
        "Coordinate scale" => "Coordinates".to_owned(),
        other => other.replace("Character slide", "Slide"),
    }
}

/// One enum choice: the raw stored value plus what the row displays.
#[derive(Clone, PartialEq)]
struct EnumChoice {
    value: String,
    label: String,
}
impl SelectItem for EnumChoice {
    type Value = String;
    fn title(&self) -> SharedString {
        self.label.clone().into()
    }
    fn value(&self) -> &Self::Value {
        &self.value
    }
}

/// A text or number input plus the text it last held from the model.
struct InputField {
    state: Entity<InputState>,
    /// Model text last written to (or committed from) the input.
    synced: String,
    target: InputTarget,
    step: f64,
    _subscriptions: Vec<Subscription>,
}

/// A multi-line plugin text parameter (LaTeX source, graph items). Typing previews live and
/// the whole focus session commits as one undo step.
struct TextareaField {
    state: Entity<TextareaState>,
    target: (ClipId, String),
    /// Model text last written to (or committed from) the editor.
    synced: String,
    _subscription: Subscription,
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

/// The static option labels and focus handles of one segmented enum control.
struct SegState {
    handles: Vec<gpui::FocusHandle>,
    choices: Vec<crate::style::SegmentedOption>,
    items: Vec<(String, String)>,
    element_id: &'static str,
}

/// A `&'static str` for a UI id or label: the segmented control wants static
/// strings, and the set of distinct ones is bounded by the clips and
/// parameters the user selects, so each is allocated once.
fn intern(text: String) -> &'static str {
    use std::collections::HashSet;
    use std::sync::{Mutex, OnceLock};
    static POOL: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let mut pool = POOL
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .expect("intern pool");
    if let Some(found) = pool.get(text.as_str()) {
        return found;
    }
    let leaked: &'static str = Box::leak(text.into_boxed_str());
    pool.insert(leaked);
    leaked
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
    textareas: BTreeMap<String, TextareaField>,
    colors: BTreeMap<String, ColorField>,
    enums: BTreeMap<String, EnumField>,
    assets: BTreeMap<String, AssetField>,
    sliders: BTreeMap<String, (Entity<SliderState>, Subscription)>,
    /// One focus handle per option of every segmented enum control.
    segmented: BTreeMap<String, SegState>,
    /// The inspector's "Add effect" menu that is open, if any: the video and
    /// the audio sections have their own.
    pub(super) add_menu: Option<EffectTarget>,
    /// The LaTeX snippets disclosure is open.
    pub(super) snippets_open: bool,
    /// The LaTeX "More" group (units, alignment) is open.
    pub(super) more_open: bool,
    /// The last strict typeset check of a formula or graph clip: the parameters
    /// it ran with and its error.
    typeset: BTreeMap<ClipId, (String, Option<String>)>,
}

#[cfg(test)]
impl FieldPool {
    /// The text currently in the input registered under `key` (UI tests).
    pub(super) fn input_text(&self, key: &str, cx: &App) -> Option<String> {
        self.inputs
            .get(key)
            .map(|field| field.state.read(cx).value().to_string())
    }
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
            || self
                .textareas
                .values()
                .any(|field| field.state.read(cx).focus_handle(cx).is_focused(window))
    }

    /// Put the model text back into the focused single-line input, dropping whatever was typed
    /// but not committed. Returns whether an input was focused. A focused multi-line field has
    /// nothing to revert (it applies live) but still counts, so Escape can leave it.
    pub(super) fn revert_focused_input(&mut self, window: &mut Window, cx: &mut App) -> bool {
        if self
            .textareas
            .values()
            .any(|field| field.state.read(cx).focus_handle(cx).is_focused(window))
        {
            return true;
        }
        let Some(field) = self
            .inputs
            .values()
            .find(|field| field.state.read(cx).focus_handle(cx).is_focused(window))
        else {
            return false;
        };
        let text = field.synced.clone();
        field
            .state
            .update(cx, |state, cx| state.set_value(text, window, cx));
        true
    }

    /// The first live single-line input, for UI tests that drive keyboard focus.
    #[cfg(test)]
    pub(super) fn first_input(&self) -> Option<Entity<InputState>> {
        self.inputs.values().next().map(|field| field.state.clone())
    }

    /// The first live multi-line field (LaTeX source, text content), for UI tests.
    #[cfg(test)]
    pub(super) fn first_textarea(&self) -> Option<Entity<TextareaState>> {
        self.textareas
            .values()
            .next()
            .map(|field| field.state.clone())
    }

    pub(super) fn retain_clips(&mut self, clips: &BTreeSet<ClipId>) {
        let keep = |key: &String| {
            !key.starts_with('c')
                || clips
                    .iter()
                    .any(|id| key.starts_with(&format!("c{}:", id.0)))
        };
        self.inputs.retain(|key, _| keep(key));
        self.textareas.retain(|key, _| keep(key));
        self.colors.retain(|key, _| keep(key));
        self.sliders.retain(|key, _| keep(key));
        self.assets.retain(|key, _| keep(key));
        self.enums.retain(|key, _| keep(key));
        self.segmented.retain(|key, _| keep(key));
        self.typeset.retain(|id, _| clips.contains(id));
    }

    /// Drop multi-selection widgets other than those keyed with `prefix`.
    fn retain_multi(&mut self, prefix: &str) {
        let keep = |key: &String| !key.starts_with("multi:") || key.starts_with(prefix);
        self.inputs.retain(|key, _| keep(key));
        self.sliders.retain(|key, _| keep(key));
    }

    /// Drop widgets that belong to clips other than `clip`.
    fn retain_clip(&mut self, clip: ClipId) {
        let prefix = format!("c{}:", clip.0);
        self.inputs
            .retain(|key, _| !key.starts_with('c') || key.starts_with(&prefix));
        self.textareas.retain(|key, _| key.starts_with(&prefix));
        self.colors.retain(|key, _| key.starts_with(&prefix));
        self.sliders
            .retain(|key, _| !key.starts_with('c') || key.starts_with(&prefix));
        self.assets.retain(|key, _| key.starts_with(&prefix));
        self.enums.retain(|key, _| key.starts_with(&prefix));
        self.segmented.retain(|key, _| key.starts_with(&prefix));
        self.typeset.retain(|id, _| *id == clip);
    }
}

/// Plugin text parameters that hold several lines (formula source, item lists, subtitles).
fn is_multiline_param(name: &str, default: &str, value: &str) -> bool {
    matches!(name, "source" | "items" | "text") || default.contains('\n') || value.contains('\n')
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

/// The factor between what a field shows and what the model stores: opacity,
/// position and scale are canvas fractions shown as percents (`100` for a
/// stored `1`), everything else is unchanged (gains convert through dB, see
/// below).
fn field_display_scale(target: &InputTarget) -> f64 {
    if is_percent_target(target) { 100. } else { 1. }
}

/// How far one pixel of a label drag moves a field, in what the field shows:
/// the model step times the display scale (a 0.05 opacity step is 5 percent).
fn scrub_step(target: &InputTarget, step: f64) -> f64 {
    step * field_display_scale(target)
}

/// The weight step (100..900) nearest to a stored weight, so a value between two
/// steps shows a name.
fn nearest_weight(value: f64) -> f64 {
    (value / 100.).round().clamp(1., 9.) * 100.
}

/// Whether the field is a transform component the inspector shows as a
/// percent: opacity, position and scale.
fn is_percent_target(target: &InputTarget) -> bool {
    let (InputTarget::Field { field, .. } | InputTarget::SelectionField { field, .. }) = target
    else {
        return false;
    };
    matches!(
        field,
        Field::Transform(
            TransformProperty::Opacity
                | TransformProperty::X
                | TransformProperty::Y
                | TransformProperty::ScaleX
                | TransformProperty::ScaleY
        )
    )
}

/// Whether the field shows a linear gain as decibels (the model stays linear).
fn is_gain_target(target: &InputTarget) -> bool {
    matches!(
        target,
        InputTarget::Field {
            field: Field::Param(name),
            ..
        } | InputTarget::SelectionField {
            field: Field::Param(name),
            ..
        }
        if name == "gain"
    )
}

/// What the field shows for a model value: gains become decibels, opacities
/// percents, everything else passes through (rotation is already in degrees
/// here, converted by `field_display_value` before the row is built).
fn field_to_display(target: &InputTarget, value: f64) -> f64 {
    if is_gain_target(target) {
        super::support::gain_db(value)
    } else {
        value * field_display_scale(target)
    }
}

/// The model value for what the field shows.
fn field_from_display(target: &InputTarget, value: f64) -> f64 {
    if is_gain_target(target) {
        10f64.powf(value / 20.)
    } else {
        value / field_display_scale(target)
    }
}

/// How a display value is formatted: gains with one decimal (or `-inf`),
/// rotation with at most one, everything else compactly.
fn format_display(target: &InputTarget, value: f64) -> String {
    if is_gain_target(target) {
        return if value.is_finite() {
            format!("{value:.1}")
        } else {
            "-inf".to_owned()
        };
    }
    match target {
        InputTarget::Field {
            field: Field::Transform(TransformProperty::Rotation),
            ..
        }
        | InputTarget::SelectionField {
            field: Field::Transform(TransformProperty::Rotation),
            ..
        } => format_degrees(value),
        _ => format_number(value),
    }
}

/// Sentence-case a raw identifier for display: `word-highlight` becomes
/// `Word highlight`.
fn sentence_case(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for (i, word) in value.split(['-', '_']).enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    out
}

/// What an enum parameter's value reads like in the inspector: proper names
/// for first-party values (docs/design.md: no raw identifiers as text),
/// sentence-casing for everything else.
pub(super) fn display_label(plugin_id: &str, param: &str, value: &str) -> String {
    const MAP: &[(&str, &str, &str, &str)] = &[
        ("*", "*", "off", "Off"),
        ("*", "*", "normalized", "Normalized"),
        ("*", "*", "percent", "Percent"),
        ("*", "*", "pixels", "Pixels"),
        ("*", "*", "uniform", "Uniform"),
        ("*", "*", "stretch", "Stretch"),
        ("*", "*", "left", "Left"),
        ("*", "*", "center", "Center"),
        ("*", "*", "right", "Right"),
        ("org.animathio.text", "font", "dejavu", "DejaVu Sans"),
        ("org.animathio.text", "font", "inter", "Inter"),
        ("org.animathio.text", "caption_style", "classic", "Classic"),
        (
            "org.animathio.text",
            "caption_style",
            "word-highlight",
            "Word highlight",
        ),
        (
            "org.animathio.text",
            "caption_style",
            "one-word",
            "One word",
        ),
        (
            "org.animathio.text",
            "caption_style",
            "bold-outline",
            "Bold outline",
        ),
        ("org.animathio.shapes", "shape", "rect", "Rectangle"),
        ("org.animathio.shapes", "shape", "ellipse", "Ellipse"),
        ("org.animathio.shapes", "shape", "polygon", "Polygon"),
        ("org.animathio.shapes", "shape", "star", "Star"),
        ("org.animathio.shapes", "shape", "arrow", "Arrow"),
        ("org.animathio.shapes", "shape", "path", "Path"),
        ("org.animathio.latex", "mode", "latex", "LaTeX"),
        ("org.animathio.latex", "mode", "typst", "Typst"),
    ];
    for (plugin, name, raw, label) in MAP {
        let plugin_matches = *plugin == "*" || *plugin == plugin_id;
        let name_matches = *name == "*" || *name == param;
        if plugin_matches && name_matches && *raw == value {
            return (*label).to_owned();
        }
    }
    sentence_case(value)
}

/// Whether two consecutive plugin parameters share one row (`X/Y`, width and
/// height, a `min`/`max` range) and what the row is called.
pub(super) fn plugin_param_pair(a: &str, b: &str) -> Option<&'static str> {
    match (a, b) {
        ("x", "y") => Some("Center"),
        ("width", "height") => Some("Size"),
        ("fit_width", "fit_height") => Some("Fit"),
        ("x_min", "x_max") => Some("X range"),
        ("y_min", "y_max") => Some("Y range"),
        _ => None,
    }
}

/// One-letter prefixes for a pair of fields: axes as `X`/`Y`, sizes as `W`/`H`,
/// ranges as `Min`/`Max`.
fn axis_prefixes(a: &str, b: &str) -> (&'static str, &'static str) {
    match (a, b) {
        ("width", "height") | ("fit_width", "fit_height") => ("W", "H"),
        // A range reads as "min, max" by position; the words would not fit.
        ("x_min", "x_max") | ("y_min", "y_max") => ("", ""),
        _ => ("X", "Y"),
    }
}

/// Parameters that are switches for tooling rather than things to edit by
/// hand: the strict-error flag of the math plugins and the LaTeX plugin's unit
/// and alignment, which the Add dialog never offered either.
fn technical_param(plugin_id: &str, param: &str) -> bool {
    param == "strict_errors"
        || (plugin_id == "org.animathio.latex" && matches!(param, "units" | "align"))
}

/// Whether an enum is short enough for a segmented control in the narrow
/// control column: two to four options whose labels total at most 12 letters
/// (`LaTeX`/`Typst`); longer ones use a select.
fn segmented_fits(plugin_id: &str, param: &str, options: &[String]) -> bool {
    (2..=4).contains(&options.len())
        && options
            .iter()
            .map(|option| display_label(plugin_id, param, option).chars().count())
            .sum::<usize>()
            <= 12
}

/// Whether a numeric parameter is a declared font weight the inspector shows
/// as named choices: 100-900 in whole hundreds (docs/design.md, no raw values).
fn is_font_weight_param(plugin_id: &str, name: &str, min: f64, max: f64) -> bool {
    plugin_id == "org.animathio.text" && name == "font_weight" && min <= 100. && max >= 900.
}

/// The text plugin's per-character slide directions, stored as 1 to 4.
const SLIDE_DIRECTIONS: [(f64, &str); 4] =
    [(1., "Left"), (2., "Right"), (3., "Top"), (4., "Bottom")];

fn is_slide_direction_param(plugin_id: &str, name: &str) -> bool {
    plugin_id == "org.animathio.text" && name == "char_slide_direction"
}

/// Whether a plugin parameter is caption-only and the clip's captions are off:
/// the values stay, the inspector just hides rows that cannot do anything
/// until a caption style is chosen (docs/design.md "Inspector").
fn caption_param_hidden(
    plugin_id: &str,
    param: &str,
    value_of: &impl Fn(&str) -> Option<String>,
) -> bool {
    plugin_id == "org.animathio.text"
        && matches!(
            param,
            "words" | "accent" | "outline" | "outline_width" | "max_words"
        )
        && value_of("caption_style").is_none_or(|style| {
            style == "off"
                || !animathio_control::caption_styles()
                    .iter()
                    .any(|known| known.id == style)
        })
}

/// Sentence-case kind of a clip for the inspector header: "Image", "Shape",
/// "LaTeX", "Graph" \u{2014} never the lowercase "math" of the timeline label.
pub(super) fn kind_display(kind: &ClipKind) -> String {
    match kind {
        ClipKind::Video(_) => "Video".into(),
        ClipKind::Image(_) => "Image".into(),
        ClipKind::Audio(_) => "Audio".into(),
        ClipKind::Plugin { plugin_id, .. } => match plugin_id.as_str() {
            "org.animathio.text" => "Text".into(),
            "org.animathio.shapes" => "Shape".into(),
            "org.animathio.latex" => "LaTeX".into(),
            "org.animathio.graph" => "Graph".into(),
            other => sentence_case(other.rsplit('.').next().unwrap_or(other)),
        },
        ClipKind::Vector(_) => "Vector".into(),
        ClipKind::Error(_) => "Error".into(),
    }
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
                move |this, _state, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        this.commit_input(&commit_key, cx);
                        if commit_key == "timecode" {
                            this.viewer_state.timecode_editing = false;
                            cx.notify();
                        }
                    }
                    // Enter commits and hands the keyboard back to the timeline, so Space,
                    // Delete and the tool keys work again without clicking away first.
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        window.focus(&this.panel_focus[0], cx);
                    }
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
    /// when it differs from what the model last showed. The buffer is what the
    /// field displays (decibels, percents); the model gets what it stores.
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
        let model = buffer
            .trim()
            .parse::<f64>()
            .ok()
            .map(|value| format_number(field_from_display(&target, value)));
        self.commit_input_value(target, model.unwrap_or(buffer), cx);
    }

    /// Flush before selection changes can drop widgets and their blur subscriptions.
    /// Each input carries its original clip ids, never the current selection.
    pub(super) fn commit_pending_inspector_inputs(&mut self, cx: &mut Context<Self>) {
        let keys: Vec<_> = self
            .fields
            .inputs
            .iter()
            .filter(|(_, field)| !matches!(field.target, InputTarget::Timecode))
            .map(|(key, _)| key.clone())
            .collect();
        for key in keys {
            self.commit_input(&key, cx);
        }
        // Flush a final textarea change even if its Change/Blur event is still queued.
        let textareas: Vec<_> = self
            .fields
            .textareas
            .values_mut()
            .filter_map(|field| {
                let text = field.state.read(cx).value().to_string();
                if text == field.synced {
                    return None;
                }
                field.synced.clone_from(&text);
                Some((field.target.clone(), text))
            })
            .collect();
        for ((id, name), text) in textareas {
            if let Some(command) = self.plugin_param_command(id, name, json!(text)) {
                self.preview_live(command, cx);
            }
        }
        self.finish_color_edit(cx);
    }

    /// Step a numeric field by Up/Down (`step_input`), by the field's step in
    /// display units (percent steps, decibels).
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
        let target = field.target.clone();
        let current = field
            .state
            .read(cx)
            .value()
            .trim()
            .parse::<f64>()
            .unwrap_or_else(|_| field_to_display(&target, 0.));
        let scale = field_display_scale(&target);
        let delta = match action {
            StepAction::Increment => field.step * scale,
            StepAction::Decrement => -field.step * scale,
        };
        let display = current + delta;
        let text = format_display(&target, display);
        field
            .state
            .update(cx, |state, cx| state.set_value(text.clone(), window, cx));
        field.synced.clone_from(&text);
        let model = format_number(field_from_display(&target, display));
        self.commit_input_value(target, model, cx);
    }

    /// One numeric field with its unit suffix, `tnum` digits and Up/Down
    /// stepping. No grip and no +/- stepper buttons: the label drags (scrub)
    /// and the focused field steps with the arrow keys, that is all the drag
    /// and keyboard affordance a row needs (docs/design.md "Inspector").
    fn number_field_inner(
        &mut self,
        key: &str,
        target: InputTarget,
        value: f64,
        step: f64,
        prefix: Option<&'static str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let display = field_to_display(&target, value);
        let text = format_display(&target, display);
        let suffix = self.number_suffix(&target).unwrap_or("");
        let state = self.input_state(key, target.clone(), text, step, window, cx);
        let step_key = key.to_owned();
        let mut field = Input::new(&state).small();
        if let Some(prefix) = prefix.filter(|prefix| !prefix.is_empty()) {
            field = field.prefix(
                div()
                    .pl_1()
                    .text_role(Role::Label)
                    .text_color(hex(color::TEXT_MUTED))
                    .child(prefix),
            );
        }
        if !suffix.is_empty() {
            field = field.suffix(
                div()
                    .pr_1()
                    .text_role(Role::Label)
                    .text_color(hex(color::TEXT_MUTED))
                    .child(suffix),
            );
        }
        div()
            .flex_1()
            .min_w_0()
            .tabular()
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                let action = match event.keystroke.key.as_str() {
                    "up" => Some(StepAction::Increment),
                    "down" => Some(StepAction::Decrement),
                    _ => None,
                };
                if let Some(action) = action {
                    this.step_input(&step_key, action, window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(field)
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
        self.number_field_inner(key, target, value, step, None, window, cx)
    }

    /// A numeric field that shares a pair row with its neighbour (`X`/`Y`),
    /// with the axis letter inside the field.
    fn number_control_flex(
        &mut self,
        key: &str,
        target: InputTarget,
        value: f64,
        step: f64,
        prefix: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        self.number_field_inner(key, target, value, step, Some(prefix), window, cx)
    }

    /// The unit shown after a number input: degrees for rotation, percent for
    /// font sizes, percent for opacity, decibels for gain. Canvas fractions
    /// (position and scale) stay bare, as the model shows them.
    fn number_suffix(&self, target: &InputTarget) -> Option<&'static str> {
        match target {
            InputTarget::Field { field, .. } | InputTarget::SelectionField { field, .. } => {
                match field {
                    Field::Transform(TransformProperty::Rotation) => Some("\u{b0}"),
                    Field::Transform(
                        TransformProperty::Opacity
                        | TransformProperty::X
                        | TransformProperty::Y
                        | TransformProperty::ScaleX
                        | TransformProperty::ScaleY,
                    ) => Some("%"),
                    Field::Param(name) if name == "gain" => Some("dB"),
                    _ => None,
                }
            }
            InputTarget::PluginNumber { id, name } => self
                .font_size_units(*id, name)
                .map(|_| "%")
                .or_else(|| self.shape_number_suffix(*id, name)),
            _ => None,
        }
    }

    /// The unit of a shape parameter: pixels for lengths while the shape is in
    /// pixel units, degrees for its rotation. Normalised lengths have no unit.
    fn shape_number_suffix(&self, id: ClipId, name: &str) -> Option<&'static str> {
        let ClipKind::Plugin { plugin_id, .. } = &self.find_clip(id)?.kind else {
            return None;
        };
        if plugin_id != "org.animathio.shapes" {
            return None;
        }
        match name {
            "rotation" => Some("\u{b0}"),
            "x" | "y" | "width" | "height" | "radius" | "stroke_width"
                if self.plugin_text_value(id, "units", "pixels") == "pixels" =>
            {
                Some("px")
            }
            _ => None,
        }
    }

    /// The stored unit of a plugin font size parameter, when `name` is one.
    pub(super) fn font_size_units(&self, id: ClipId, name: &str) -> Option<SizeUnits> {
        let ClipKind::Plugin { plugin_id, .. } = &self.find_clip(id)?.kind else {
            return None;
        };
        if !is_font_size_param(plugin_id, name) {
            return None;
        }
        let units = self.plugin_text_value(id, "units", "pixels");
        Some(SizeUnits::parse(plugin_id, &units))
    }

    /// The stored value for what the inspector shows or the user typed (font sizes are
    /// presented as a percentage of the short edge).
    pub(super) fn plugin_number_from_display(&self, id: ClipId, name: &str, value: f64) -> f64 {
        match self.font_size_units(id, name) {
            Some(units) => size_to_model(value, units, project_size(&self.project)),
            None => value,
        }
    }

    pub(super) fn plugin_number_display_value(&self, id: ClipId, name: &str) -> f64 {
        let value = self.plugin_number_value(id, name);
        match self.font_size_units(id, name) {
            Some(units) => size_to_display(value, units, project_size(&self.project)),
            None => value,
        }
    }

    pub(super) fn slider_control(
        &mut self,
        key: &str,
        target: Option<InputTarget>,
        value: f64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        self.range_slider_control(key, target, value, (0., 1., 0.01), window, cx)
    }

    /// A slider over `(min, max, step)`. The range is fixed when the slider for `key` is
    /// first created, so callers key it by something that determines the range.
    pub(super) fn range_slider_control(
        &mut self,
        key: &str,
        target: Option<InputTarget>,
        value: f64,
        (min, max, step): (f64, f64, f64),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        if !self.fields.sliders.contains_key(key) {
            let state = cx.new(|_| {
                SliderState::new()
                    .min(min as f32)
                    .max(max as f32)
                    .step(step as f32)
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
            .child(tool_panel::slider(&state, &self.focus_handle))
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
            InputTarget::SelectionField { field, ids } => {
                let commands = ids
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
            InputTarget::VideoEffectParam { id, effect, name } => {
                self.video_effect_param_command(id, effect, &name, value)
            }
            InputTarget::PluginNumber { id, name } => {
                let value = self.plugin_number_from_display(id, &name, value);
                self.plugin_number_command(id, &name, value)
            }
            InputTarget::PluginPoint { id, name, axis } => {
                let mut point = self.plugin_point_value(id, &name);
                point[axis] = value as f32;
                self.plugin_param_command(id, name, json!(point))
            }
            _ => None,
        }
    }

    fn scrub_field_command(&self, id: ClipId, field: &Field, value: f64) -> Option<Command> {
        // Scrubs and sliders carry display units (degrees came in as degrees;
        // gains as decibels, opacities as percents).
        let display_target = InputTarget::Field {
            id,
            field: field.clone(),
        };
        let value = field_from_display(&display_target, value);
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

    /// A multi-line editor for plugin text that is naturally several lines (LaTeX source,
    /// graph items). Edits preview live and commit as one undo step when focus leaves.
    pub(super) fn textarea_control(
        &mut self,
        id: ClipId,
        name: &str,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let key = format!("c{}:pta:{name}", id.0);
        if !self.fields.textareas.contains_key(&key) {
            let initial = value.to_owned();
            let state = cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(3, 8)
                    .default_value(initial)
            });
            let param = name.to_owned();
            let field_key = key.clone();
            let subscription = cx.subscribe_in(
                &state,
                window,
                move |this, state, event: &InputEvent, _window, cx| match event {
                    InputEvent::Change => {
                        let text = state.read(cx).value().to_string();
                        let Some(field) = this.fields.textareas.get_mut(&field_key) else {
                            return;
                        };
                        if field.synced == text {
                            return;
                        }
                        field.synced.clone_from(&text);
                        if let Some(command) =
                            this.plugin_param_command(id, param.clone(), json!(text))
                        {
                            this.preview_live(command, cx);
                        }
                    }
                    InputEvent::Blur => this.finish_color_edit(cx),
                    _ => {}
                },
            );
            self.fields.textareas.insert(
                key.clone(),
                TextareaField {
                    state,
                    target: (id, name.to_owned()),
                    synced: value.to_owned(),
                    _subscription: subscription,
                },
            );
        }
        let field = self
            .fields
            .textareas
            .get_mut(&key)
            .expect("textarea field was just inserted");
        let focused = field.state.read(cx).focus_handle(cx).is_focused(window);
        if !focused && field.state.read(cx).value().as_ref() != value {
            field.state.update(cx, |state, cx| {
                state.set_value(value.to_owned(), window, cx);
            });
        }
        if !focused {
            value.clone_into(&mut field.synced);
        }
        let state = field.state.clone();
        div().w_full().child(Textarea::new(&state))
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
        div()
            .flex_1()
            .min_w_0()
            .tabular()
            .child(Input::new(&state).small())
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
            state.update(cx, |state, cx| {
                state.set_value(picker_color(color), window, cx);
            });
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
            field.state.update(cx, |state, cx| {
                state.set_value(picker_color(color), window, cx);
            });
        }
        framed_swatch(ColorPicker::new(&field.state), color[3] < 255, cx)
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
                    if let Some(command) = this.plugin_param_command(
                        id,
                        param.clone(),
                        json!([next[0], next[1], next[2], next[3]]),
                    ) {
                        this.preview_color(command, cx);
                    }
                },
            );
            state.update(cx, |state, cx| {
                state.set_value(picker_color(color), window, cx);
            });
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
            field.state.update(cx, |state, cx| {
                state.set_value(picker_color(color), window, cx);
            });
        }
        let hex_text = format_hex(color);
        let swatch = framed_swatch(ColorPicker::new(&field.state), color[3] < 255, cx);
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .child(swatch)
            .child(
                div()
                    .text_role(Role::Label)
                    .tabular()
                    .text_color(hex(color::TEXT_MUTED))
                    .child(hex_text),
            )
    }

    /// A segmented control over `items` (`(debug id, label)` pairs) whose
    /// option strings and focus handles live in the field pool and are rebuilt
    /// only when the items change (docs/design.md "Tab strips and segmented
    /// controls"). `on_select` gets the chosen index.
    fn segmented_pooled(
        &mut self,
        key: String,
        items: Vec<(String, String)>,
        selected: usize,
        on_select: impl Fn(usize, &mut Window, &mut App) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> Div {
        let stale = self
            .fields
            .segmented
            .get(&key)
            .is_none_or(|entry| entry.items != items);
        if stale {
            let choices = items
                .iter()
                .map(|(id, label)| {
                    crate::style::SegmentedOption::new(
                        intern(id.clone()),
                        intern(label.clone()),
                        None,
                    )
                })
                .collect();
            let element_id = intern(format!("{key}:segmented"));
            self.fields.segmented.insert(
                key.clone(),
                SegState {
                    handles: items.iter().map(|_| cx.focus_handle()).collect(),
                    choices,
                    items,
                    element_id,
                },
            );
        }
        let entry = &self.fields.segmented[&key];
        let control = crate::style::segmented(
            entry.element_id,
            &entry.choices,
            &entry.handles,
            selected,
            None,
            true,
            on_select,
        );
        // `segmented` frames itself with 8 px padding for panel use; inside a
        // row that padding is cancelled so the row keeps its height.
        div().w_full().mx(px(-8.)).my(px(-8.)).child(control)
    }

    /// A short enum as a segmented control.
    fn segmented_control(
        &mut self,
        id: ClipId,
        plugin_id: &str,
        name: &str,
        options: &[String],
        current: &str,
        cx: &mut Context<Self>,
    ) -> Div {
        let items = options
            .iter()
            .map(|option| {
                (
                    format!("enum-{}-{name}-{option}", id.0),
                    display_label(plugin_id, name, option),
                )
            })
            .collect();
        // A value that is none of the options selects none of them.
        let selected = options
            .iter()
            .position(|option| option == current)
            .unwrap_or(usize::MAX);
        let editor = cx.entity();
        let param = name.to_owned();
        let values = options.to_vec();
        self.segmented_pooled(
            format!("c{}:seg:{name}", id.0),
            items,
            selected,
            move |index, _, cx| {
                if let Some(value) = values.get(index) {
                    editor.update(cx, |this, cx| {
                        this.execute_plugin_param(id, param.clone(), json!(value), cx);
                    });
                }
            },
            cx,
        )
    }

    /// An enum choice that shows a display label but stores the raw value.
    fn enum_control(
        &mut self,
        id: ClipId,
        plugin_id: &str,
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
            let items = SearchableVec::new(
                options
                    .iter()
                    .map(|option| EnumChoice {
                        value: option.clone(),
                        label: display_label(plugin_id, name, option),
                    })
                    .collect::<Vec<_>>(),
            );
            let state = cx.new(|cx| SelectState::new(items, selected, window, cx));
            let param = name.to_owned();
            let subscription = cx.subscribe_in(
                &state,
                window,
                move |this, _state, event: &SelectEvent<SearchableVec<EnumChoice>>, _window, cx| {
                    let SelectEvent::Confirm(Some(value)) = event else {
                        return;
                    };
                    this.execute_plugin_param(id, param.clone(), json!(value), cx);
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
        div().w_full().child(Select::new(&field.state).small())
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
                    this.execute_plugin_param(id, name.clone(), json!(value), cx);
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
        div().flex().justify_end().child(
            Switch::new(SharedString::from(format!("switch-{}-{name}", id.0)))
                .checked(value)
                .on_click(cx.listener(move |this, checked: &bool, _window, cx| {
                    this.execute_plugin_param(id, param.clone(), json!(*checked), cx);
                })),
        )
    }

    /// Keyframe diamond for one animatable property: filled when a keyframe sits
    /// at the playhead. The previous/next arrows show when the property has
    /// keyframes, and on row hover (docs/design.md "Inspector").
    #[allow(clippy::if_not_else)]
    pub(super) fn row_diamond(&self, id: ClipId, target: &Target, cx: &mut Context<Self>) -> Div {
        let clip = self.find_clip(id);
        let active = clip.is_some_and(|clip| inspector::keyframed(clip, target, self.position));
        let has_keys = clip.is_some_and(|clip| {
            clip.animations
                .iter()
                .any(|animation| animation.target == *target)
        });
        let element_id = SharedString::from(format!("kf-{}-{target:?}", id.0));
        let mut controls = div().flex().items_center().gap_0();
        for (forward, icon) in [
            (false, IconName::ChevronLeft),
            (true, IconName::ChevronRight),
        ] {
            let time =
                clip.and_then(|clip| inspector::adjacent_key(clip, target, self.position, forward));
            let side = if forward { "next" } else { "prev" };
            controls = controls.child(
                Button::new(SharedString::from(format!("{side}-{element_id}")))
                    .ghost()
                    .xsmall()
                    .w(gpui::rems(KF_ARROW_W))
                    .icon(icons::icon(icon).xsmall())
                    .disabled(time.is_none())
                    .tooltip(if forward {
                        "Next keyframe"
                    } else {
                        "Previous keyframe"
                    })
                    // Arrows are hover-revealed unless the property has keyframes.
                    .opacity(f32::from(u8::from(has_keys)))
                    .group_hover("insp-row", |revealed| revealed.opacity(1.))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(time) = time {
                            this.seek_to(time, cx);
                        }
                    })),
            );
            if !forward {
                let target = target.clone();
                controls = controls.child(
                    Button::new(element_id.clone())
                        .ghost()
                        .xsmall()
                        .w(gpui::rems(KF_DIAMOND_W))
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
            }
        }
        controls
    }

    /// One keyframe control for a pair of properties that share a row: the
    /// diamond keys (or clears) both at once, the arrows seek the nearest
    /// keyframe of either.
    pub(super) fn row_diamond_pair(
        &self,
        id: ClipId,
        targets: &[Target],
        label: &str,
        cx: &mut Context<Self>,
    ) -> Div {
        let clip = self.find_clip(id);
        let active = clip.is_some_and(|clip| {
            targets
                .iter()
                .all(|target| inspector::keyframed(clip, target, self.position))
        });
        let any = clip.is_some_and(|clip| {
            targets
                .iter()
                .any(|target| inspector::keyframed(clip, target, self.position))
        });
        let element_id = SharedString::from(format!("kf-{}-{}-{label}", id.0, targets.len()));
        let mut controls = div().flex().items_center().gap_0();
        for (forward, icon) in [
            (false, IconName::ChevronLeft),
            (true, IconName::ChevronRight),
        ] {
            let time = clip.and_then(|clip| {
                targets
                    .iter()
                    .filter_map(|target| {
                        inspector::adjacent_key(clip, target, self.position, forward)
                    })
                    .min_by_key(|time| (time.0 - self.position.0).abs())
            });
            let side = if forward { "next" } else { "prev" };
            controls = controls.child(
                Button::new(SharedString::from(format!("{side}-{element_id}")))
                    .ghost()
                    .xsmall()
                    .w(gpui::rems(KF_ARROW_W))
                    .icon(icons::icon(icon).xsmall())
                    .disabled(time.is_none())
                    .tooltip(if forward {
                        "Next keyframe"
                    } else {
                        "Previous keyframe"
                    })
                    .opacity(f32::from(u8::from(any)))
                    .group_hover("insp-row", |revealed| revealed.opacity(1.))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(time) = time {
                            this.seek_to(time, cx);
                        }
                    })),
            );
            if !forward {
                let all = targets.to_vec();
                controls = controls.child(
                    Button::new(element_id.clone())
                        .ghost()
                        .xsmall()
                        .w(gpui::rems(KF_DIAMOND_W))
                        .icon(icons::diamond(active).xsmall())
                        .tooltip(if active {
                            format!("Remove {label} keyframes at playhead")
                        } else {
                            format!("Add {label} keyframes at playhead")
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            for target in all.clone() {
                                this.toggle_keyframe(id, target.clone(), cx);
                            }
                        })),
                );
            }
        }
        controls
    }

    /// One inspector row: keyframe column, 40% label column (truncating, the
    /// label scrubs when it can), control filling the rest (docs/design.md
    /// "Inspector"). Rows join the `insp-row` hover group, which reveals the
    /// keyframe arrows.
    pub(super) fn param_row(
        diamond: Option<Div>,
        label: impl Into<RowLabel>,
        control: impl IntoElement,
        _cx: &mut Context<Self>,
    ) -> Div {
        Self::row_shell(diamond, label, control)
    }

    fn row_shell(
        diamond: Option<Div>,
        label: impl Into<RowLabel>,
        control: impl IntoElement,
    ) -> Div {
        Self::row_shell_with(diamond, label, control, LABEL_FRACTION)
    }

    /// [`Self::row_shell`] with the label column at `label_fraction` of the width.
    fn row_shell_with(
        diamond: Option<Div>,
        label: impl Into<RowLabel>,
        control: impl IntoElement,
        label_fraction: f32,
    ) -> Div {
        let RowLabel { element, tooltip } = label.into();
        let label_cell = div()
            .w(gpui::relative(label_fraction))
            .flex_shrink_0()
            .min_w_0()
            .truncate()
            .text_role(Role::Label);
        let label_cell = match tooltip {
            Some(text) => label_cell
                .id(SharedString::from(format!("row-label-{text}")))
                .tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(text.clone()).build(window, cx)
                })
                .child(element)
                .into_any_element(),
            None => label_cell.child(element).into_any_element(),
        };
        div()
            .group("insp-row")
            .min_h(size::ROW)
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .px_2()
            .child(
                div()
                    .w(gpui::rems(KF_COL))
                    .flex_shrink_0()
                    .children(diamond),
            )
            // The label is 40% of what is left after the keyframe column, so the
            // control keeps its width in the narrow default inspector.
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .child(label_cell)
                    .child(div().flex_1().min_w_0().child(control)),
            )
    }

    /// A row label that scrubs its number when dragged horizontally
    /// (Shift coarse, Ctrl fine). Its tooltip names the whole label next to the
    /// gesture, since the column truncates long labels.
    pub(super) fn scrub_label(
        key: &str,
        text: String,
        target: InputTarget,
        value: f64,
        step: f64,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let full = text.clone();
        Self::scrub_label_full(key, text, &full, target, value, step, cx)
    }

    /// [`Self::scrub_label`] showing a shortened `text` whose tooltip carries
    /// the `full` label.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn scrub_label_full(
        key: &str,
        text: String,
        full: &str,
        target: InputTarget,
        value: f64,
        step: f64,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let tooltip = SharedString::from(format!("{full}. Drag to adjust"));
        div()
            .id(SharedString::from(format!("label-{key}")))
            .cursor_ew_resize()
            .truncate()
            .child(text)
            .tooltip(move |window, cx| {
                gpui_component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
            })
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

    /// A row that cannot be keyframed: the keyframe column stays (empty) so its
    /// label lines up with the other rows.
    pub(super) fn param_row_flush(label: impl Into<RowLabel>, control: impl IntoElement) -> Div {
        Self::row_shell(None, label, control)
    }

    /// Content that sits in the label column of the rows above it (a switch, a
    /// caption, a row of buttons): the keyframe column stays empty so it lines
    /// up with the labels.
    pub(super) fn label_column(content: impl IntoElement) -> Div {
        div()
            .px_2()
            .flex()
            .flex_row()
            .gap_1()
            .child(div().w(gpui::rems(KF_COL)).flex_shrink_0())
            .child(div().flex_1().min_w_0().child(content))
    }

    /// Two controls that belong together (`X`/`Y`, width and height, a min/max
    /// range) share one row, each field half of the control column with its
    /// one-letter prefix inside the field.
    pub(super) fn param_row_pair(
        diamond: Option<Div>,
        label: impl Into<RowLabel>,
        left: impl IntoElement,
        right: impl IntoElement,
        _cx: &mut Context<Self>,
    ) -> Div {
        let pair = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .child(div().flex_1().min_w_0().child(left))
            .child(div().flex_1().min_w_0().child(right));
        Self::row_shell_with(diamond, label, pair, PAIR_LABEL_FRACTION)
    }

    /// Slider and field on one row (docs/design.md: a slider is never on a
    /// second line). The slider takes the remaining width, the field stays 4rem.
    pub(super) fn slider_field_row(slider: Option<Div>, field: impl IntoElement) -> Div {
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .children(slider.map(|slider| div().flex_1().min_w_0().child(slider)))
            .child(div().w(gpui::rems(FIELD_W)).flex_shrink_0().child(field))
    }

    /// A numeric builtin field (transform component or numeric parameter).
    /// `value` is what the model stores; the field shows it in display units
    /// (degrees for rotation, percents, decibels for gain).
    fn number_row(
        &mut self,
        id: ClipId,
        field: Field,
        label: &str,
        value: f64,
        slider: Option<(&'static str, f64, f64, f64)>,
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
        let input_target = InputTarget::Field {
            id,
            field: field.clone(),
        };
        // Gain steps in whole decibels; every other field in its model step.
        let step = if is_gain_target(&input_target) {
            1.0
        } else {
            field_step(&field)
        };
        let control = self.number_control(&key, input_target.clone(), value, step, window, cx);
        let control = match slider {
            Some((slider_key, min, max, step)) => {
                let slider = self.range_slider_control(
                    &format!("{key}:{slider_key}"),
                    Some(input_target.clone()),
                    field_to_display(&input_target, value),
                    (min, max, step),
                    window,
                    cx,
                );
                Self::slider_field_row(Some(slider), control)
            }
            None => control,
        };
        let diamond = self.row_diamond(id, &target, cx);
        let selector_label = label.to_owned();
        let label = Self::scrub_label(
            &key,
            label.to_owned(),
            input_target.clone(),
            field_to_display(&input_target, value),
            scrub_step(&input_target, step),
            cx,
        );
        Self::param_row(Some(diamond), label, control, cx)
            .debug_selector(move || format!("param-{selector_label}"))
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
        // Sample in model units (radians), then convert for display.
        let position = self.position;
        let sampled = |field: &Field| {
            let target = match field {
                Field::Transform(property) => Target::Transform(*property),
                Field::Param(name) => Target::Param(name.clone()),
            };
            inspector::sampled_numeric(
                &clip,
                &target,
                position,
                field_model_value(field, field_value(&clip, field)),
            )
        };
        let mut rows: Vec<Div> = Vec::new();
        if matches!(clip.kind, ClipKind::Image(_) | ClipKind::Video(_)) {
            rows.push(self.fit_row(id, clip.fit, cx));
        }
        // Position and scale share one row per pair (docs/design.md "Inspector").
        for (label, a, b, targets) in [
            (
                "Position",
                Field::Transform(TransformProperty::X),
                Field::Transform(TransformProperty::Y),
                [
                    Target::Transform(TransformProperty::X),
                    Target::Transform(TransformProperty::Y),
                ],
            ),
            (
                "Scale",
                Field::Transform(TransformProperty::ScaleX),
                Field::Transform(TransformProperty::ScaleY),
                [
                    Target::Transform(TransformProperty::ScaleX),
                    Target::Transform(TransformProperty::ScaleY),
                ],
            ),
        ] {
            let (value_a, value_b) = (
                field_display_value(&a, sampled(&a)),
                field_display_value(&b, sampled(&b)),
            );
            let key = |field: &Field| match field {
                Field::Transform(property) => format!("c{}:f:{property:?}", id.0),
                Field::Param(name) => format!("c{}:p:{name}", id.0),
            };
            let prefixes = ("X", "Y");
            let left = self.number_control_flex(
                &key(&a),
                InputTarget::Field {
                    id,
                    field: a.clone(),
                },
                value_a,
                field_step(&a),
                prefixes.0,
                window,
                cx,
            );
            let right = self.number_control_flex(
                &key(&b),
                InputTarget::Field {
                    id,
                    field: b.clone(),
                },
                value_b,
                field_step(&b),
                prefixes.1,
                window,
                cx,
            );
            let diamond = self.row_diamond_pair(id, &targets, label, cx);
            rows.push(
                Self::param_row_pair(Some(diamond), label, left, right, cx)
                    .debug_selector(move || format!("param-{label}")),
            );
        }
        let rotation = Field::Transform(TransformProperty::Rotation);
        rows.push(self.number_row(
            id,
            rotation.clone(),
            "Rotation",
            field_display_value(&rotation, sampled(&rotation)),
            None,
            window,
            cx,
        ));
        rows
    }

    pub(super) fn set_clip_fit(&mut self, id: ClipId, fit: Fit, cx: &mut Context<Self>) {
        if self.find_clip(id).is_some_and(|clip| clip.fit != fit) {
            self.execute(&Command::SetClipFit { id, fit }, cx);
        }
    }

    /// Fit / Fill / Stretch / 1:1 choice for an image or video clip.
    fn fit_row(&mut self, id: ClipId, current: Fit, cx: &mut Context<Self>) -> Div {
        let items = Fit::ALL
            .iter()
            .map(|fit| {
                (
                    format!("fit-{}-{}", id.0, fit.name()),
                    fit_label(*fit).to_owned(),
                )
            })
            .collect();
        let selected = Fit::ALL
            .iter()
            .position(|fit| *fit == current)
            .unwrap_or(usize::MAX);
        let editor = cx.entity();
        let control = self.segmented_pooled(
            format!("c{}:fit", id.0),
            items,
            selected,
            move |index, _, cx| {
                if let Some(fit) = Fit::ALL.get(index) {
                    editor.update(cx, |this, cx| this.set_clip_fit(id, *fit, cx));
                }
            },
            cx,
        );
        // Four options need the whole row width, so the control sits under its
        // label (in the label column) instead of beside it.
        div()
            .flex()
            .flex_col()
            .child(Self::label_column(
                div().pt_1().text_role(Role::Label).child("Fit"),
            ))
            .child(
                div()
                    .min_h(size::ROW)
                    .flex()
                    .items_center()
                    .px_2()
                    .child(div().flex_1().min_w_0().child(control)),
            )
            .debug_selector(|| "param-Fit".to_owned())
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
                rows.push(self.number_row(id, field, name, value, None, window, cx));
            }
        }
        rows
    }

    /// Gain and mute rows for audio clips. Gain shows in decibels (the model
    /// stays linear) with its slider; mute is a switch row.
    pub(super) fn audio_rows(
        &mut self,
        id: ClipId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(clip) = self.find_clip(id).cloned() else {
            return Vec::new();
        };
        let mut rows: Vec<AnyElement> = Vec::new();
        let gain = Field::Param("gain".to_owned());
        let value = field_value(&clip, &gain);
        rows.push(
            self.number_row(
                id,
                gain.clone(),
                "Gain",
                value,
                Some(("slider", GAIN_DB_RANGE.0, GAIN_DB_RANGE.1, GAIN_DB_RANGE.2)),
                window,
                cx,
            )
            .into_any_element(),
        );
        let editor = cx.entity();
        rows.push(
            Self::label_column(crate::style::switch_row(
                "audio-muted",
                "Muted",
                None,
                field_value(&clip, &Field::Param("muted".to_owned())) != 0.0,
                move |checked, _, cx| {
                    editor.update(cx, |this, cx| {
                        this.commit_field(
                            id,
                            &Field::Param("muted".to_owned()),
                            f64::from(u8::from(*checked)),
                            cx,
                        );
                    });
                },
            ))
            .into_any_element(),
        );
        rows
    }

    pub(super) fn plugin_rows(
        &mut self,
        id: ClipId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        // Formulas and graphs draw a failure inline; the inspector wants it as text.
        let typeset_error = self.typeset_error(id);
        let Some(clip) = self.find_clip(id) else {
            return Vec::new();
        };
        let ClipKind::Plugin { plugin_id, .. } = &clip.kind else {
            return Vec::new();
        };
        let Some(descriptor) = self.plugins.iter().find(|plugin| &plugin.id == plugin_id) else {
            return vec![div().px_2().child("Plugin not loaded").into_any_element()];
        };
        let plugin_id = plugin_id.clone();
        let models = descriptor.models();
        let value_of = |name: &str| {
            self.plugin_opaque(id, name)
                .and_then(|value| value.as_str().map(str::to_owned))
        };
        let mut visible: Vec<&ParamModel> = models
            .iter()
            .filter(|model| {
                plugin_param_visible(&plugin_id, model.id(), value_of)
                    && !caption_param_hidden(&plugin_id, model.id(), &value_of)
                    && !technical_param(&plugin_id, model.id())
            })
            .collect();
        // Rarely changed formula options stay reachable in a collapsed group.
        let more: Vec<&ParamModel> = models
            .iter()
            .filter(|model| {
                plugin_id == "org.animathio.latex"
                    && technical_param(&plugin_id, model.id())
                    && model.id() != "strict_errors"
                    && plugin_param_visible(&plugin_id, model.id(), value_of)
            })
            .collect();
        // The formula or function leads its section, then what changes how it looks.
        let leading: &[&str] = match plugin_id.as_str() {
            "org.animathio.latex" => &["source", "size", "color", "mode", "reveal"],
            "org.animathio.graph" => &["items", "x_min", "x_max", "y_min", "y_max", "color"],
            _ => &[],
        };
        if !leading.is_empty() {
            let rank = |name: &str| {
                leading
                    .iter()
                    .position(|first| *first == name)
                    .unwrap_or(usize::MAX)
            };
            visible.sort_by_key(|model| rank(model.id()));
        }
        // The parameter whose failure the typeset error belongs under.
        let error_after = match plugin_id.as_str() {
            "org.animathio.latex" => "source",
            "org.animathio.graph" => "items",
            _ => "",
        };
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut index = 0;
        while index < visible.len() {
            let model = visible[index];
            let next = visible.get(index + 1);

            if let (ParamModel::Number { .. }, Some(pair_model)) = (model, next)
                && let Some(label) = plugin_param_pair(model.id(), pair_model.id())
                && let ParamModel::Number { .. } = pair_model
            {
                rows.push(
                    self.plugin_pair_row(id, label, model, pair_model, window, cx)
                        .into_any_element(),
                );
                index += 2;
            } else {
                rows.push(
                    self.plugin_row(id, &plugin_id, model, window, cx)
                        .into_any_element(),
                );
                // The failure is shown right below the field it belongs to.
                if model.id() == error_after
                    && let Some(message) = &typeset_error
                {
                    rows.push(Self::typeset_error_row(message).into_any_element());
                }
                index += 1;
            }
        }
        if !more.is_empty() {
            let open = self.fields.more_open;
            rows.push(
                Self::disclosure_row("latex-more", "More", open, cx, |fields| {
                    fields.more_open = !fields.more_open;
                })
                .into_any_element(),
            );
            if open {
                for model in more {
                    rows.push(
                        self.plugin_row(id, &plugin_id, model, window, cx)
                            .into_any_element(),
                    );
                }
            }
        }
        rows
    }

    /// Two plugin numbers that belong together (`Center X/Y`, `Width/Height`,
    /// a `min`/`max` range) share one row with one keyframe control for both.
    #[allow(clippy::too_many_arguments)]
    fn plugin_pair_row(
        &mut self,
        id: ClipId,
        label: &'static str,
        a: &ParamModel,
        b: &ParamModel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let (name_a, name_b) = (a.id().to_owned(), b.id().to_owned());
        let (ParamModel::Number { step: step_a, .. }, ParamModel::Number { step: step_b, .. }) =
            (a, b)
        else {
            return div();
        };
        let value_a = self.plugin_number_display_value(id, &name_a);
        let value_b = self.plugin_number_display_value(id, &name_b);
        let prefixes = axis_prefixes(&name_a, &name_b);
        let left = self.number_control_flex(
            &format!("c{}:pn:{name_a}", id.0),
            InputTarget::PluginNumber {
                id,
                name: name_a.clone(),
            },
            value_a,
            *step_a,
            prefixes.0,
            window,
            cx,
        );
        let right = self.number_control_flex(
            &format!("c{}:pn:{name_b}", id.0),
            InputTarget::PluginNumber {
                id,
                name: name_b.clone(),
            },
            value_b,
            *step_b,
            prefixes.1,
            window,
            cx,
        );
        let targets = [Target::Param(name_a.clone()), Target::Param(name_b.clone())];
        let keyed = self.find_clip(id).is_some_and(|clip| {
            targets
                .iter()
                .any(|target| clip.animations.iter().any(|a| &a.target == target))
        });
        let diamond =
            (is_animatable(a) || keyed).then(|| self.row_diamond_pair(id, &targets, label, cx));
        Self::param_row_pair(diamond, label, left, right, cx)
            .debug_selector(move || format!("param-{label}"))
    }

    fn plugin_row(
        &mut self,
        id: ClipId,
        plugin_id: &str,
        model: &ParamModel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let name = model.id().to_owned();
        // Only values that interpolate get keyframe controls; text, choices and switches do not.
        // A param that already has keys keeps its controls, since edits then add keys.
        let target = Target::Param(name.clone());
        let has_keys = self
            .find_clip(id)
            .is_some_and(|clip| clip.animations.iter().any(|a| a.target == target));
        let diamond = (is_animatable(model) || has_keys).then(|| self.row_diamond(id, &target, cx));
        let short = short_label(model.label());
        let mut label = RowLabel::shortened(short.clone(), model.label().to_owned());
        let control = match model {
            ParamModel::Number { step, min, max, .. } => {
                let value = self.plugin_number_display_value(id, &name);
                let step = if self.font_size_units(id, &name).is_some() {
                    &0.5
                } else {
                    step
                };
                let unit = (*min - 0.).abs() < f64::EPSILON && (*max - 1.).abs() < f64::EPSILON;
                // A declared 100..900 weight becomes a named choice instead of
                // a bare number (docs/design.md: no raw identifiers as text).
                if is_font_weight_param(plugin_id, &name, *min, *max) {
                    self.weight_control(id, &name, value, window, cx)
                } else if is_slide_direction_param(plugin_id, &name) {
                    self.named_number_control(
                        id,
                        &name,
                        value.round().clamp(1., 4.),
                        &SLIDE_DIRECTIONS,
                        window,
                        cx,
                    )
                } else {
                    label = Self::scrub_label_full(
                        &format!("c{}:pn:{name}", id.0),
                        short.clone(),
                        model.label(),
                        InputTarget::PluginNumber {
                            id,
                            name: name.clone(),
                        },
                        value,
                        *step,
                        cx,
                    )
                    .into();
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
                        Self::slider_field_row(Some(slider), number)
                    } else {
                        number
                    }
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
                if is_multiline_param(&name, default, &value) {
                    // Label on its own line so the editor can use the full inspector width.
                    let editor = self.textarea_control(id, &name, &value, window, cx);
                    return div()
                        .flex()
                        .flex_col()
                        .debug_selector(move || format!("param-{name}"))
                        .child(Self::param_row(diamond, label, div(), cx))
                        .child(div().px_2().pb_1().w_full().child(editor));
                }
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
                if segmented_fits(plugin_id, &name, options) {
                    self.segmented_control(id, plugin_id, &name, options, &current, cx)
                } else {
                    self.enum_control(id, plugin_id, &name, options, &current, window, cx)
                }
            }
            ParamModel::Bool { default, .. } => {
                let value = self.plugin_bool_value(id, &name, *default);
                Self::bool_control(id, &name, value, cx)
            }
            ParamModel::Point { .. } => {
                let point = self.plugin_point_value(id, &name);
                let axes = [0usize, 1].map(|axis| {
                    self.number_control_flex(
                        &format!("c{}:pp{axis}:{name}", id.0),
                        InputTarget::PluginPoint {
                            id,
                            name: name.clone(),
                            axis,
                        },
                        f64::from(point[axis]),
                        0.1,
                        if axis == 0 { "X" } else { "Y" },
                        window,
                        cx,
                    )
                });
                let [x, y] = axes;
                return Self::param_row_pair(diamond, label, x, y, cx)
                    .debug_selector(move || format!("param-{name}"));
            }
            ParamModel::Asset { media_type, .. } => {
                self.plugin_asset_control(id, name.clone(), media_type, window, cx)
            }
        };
        Self::param_row(diamond, label, control, cx).debug_selector(move || format!("param-{name}"))
    }

    /// A declared font weight (100-900 in steps of 100) as a named select:
    /// Thin to Black.
    fn weight_control(
        &mut self,
        id: ClipId,
        name: &str,
        value: f64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        // The text plugin's full 100-900 range, so a clip at any weight (the
        // title presets use 600 and 800) shows its name.
        const WEIGHTS: [(f64, &str); 9] = [
            (100., "Thin"),
            (200., "Extra light"),
            (300., "Light"),
            (400., "Regular"),
            (500., "Medium"),
            (600., "Semibold"),
            (700., "Bold"),
            (800., "Extra bold"),
            (900., "Black"),
        ];
        // A weight between two steps (a variable-font value) shows the nearest name.
        self.named_number_control(id, name, nearest_weight(value), &WEIGHTS, window, cx)
    }

    /// A numeric plugin parameter whose whole-number values have names, as a select
    /// (docs/design.md: no raw numbers where the value is a choice).
    fn named_number_control(
        &mut self,
        id: ClipId,
        name: &str,
        value: f64,
        options: &[(f64, &str)],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let key = format!("c{}:named:{name}", id.0);
        if !self.fields.enums.contains_key(&key) {
            let selected = options
                .iter()
                .position(|(weight, _)| (*weight - value).abs() < f64::EPSILON)
                .map(IndexPath::new);
            let items = SearchableVec::new(
                options
                    .iter()
                    .map(|(weight, label)| EnumChoice {
                        value: format!("{weight:.0}"),
                        label: (*label).to_owned(),
                    })
                    .collect::<Vec<_>>(),
            );
            let state = cx.new(|cx| SelectState::new(items, selected, window, cx));
            let param = name.to_owned();
            let subscription = cx.subscribe_in(
                &state,
                window,
                move |this, _state, event: &SelectEvent<SearchableVec<EnumChoice>>, _window, cx| {
                    let SelectEvent::Confirm(Some(text)) = event else {
                        return;
                    };
                    if let Ok(weight) = text.parse::<f64>() {
                        this.execute_plugin_param(id, param.clone(), json!(weight), cx);
                    }
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
        let field = self.fields.enums.get(&key).expect("named select inserted");
        let wanted = format!("{value:.0}");
        if field.state.read(cx).selected_value() != Some(&wanted) {
            field.state.update(cx, |state, cx| {
                state.set_selected_value(&wanted, window, cx);
            });
        }
        div().w_full().child(Select::new(&field.state).small())
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
            *kinds.entry(kind_display(&clip.kind)).or_default() += 1;
        }
        let kinds = kinds
            .into_iter()
            .map(|(kind, count)| format!("{count} {kind}"))
            .collect::<Vec<_>>()
            .join(", ");
        let info = |label: &'static str, value: String| {
            div()
                .min_h(size::ROW)
                .px_2()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .w(gpui::relative(0.4))
                        .flex_shrink_0()
                        .truncate()
                        .text_role(Role::Label)
                        .child(label),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_role(Role::Label)
                        .text_color(hex(color::TEXT))
                        .tabular()
                        .child(value),
                )
        };
        let header = div()
            .px_2()
            .py_2()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(hex(color::BORDER))
            .child(icons::icon(IconName::Layers).small())
            .child(
                div()
                    .flex_1()
                    .text_role(Role::Heading)
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
            (Field::Transform(TransformProperty::Rotation), "Rotation"),
            (Field::Transform(TransformProperty::Opacity), "Opacity"),
        ];
        let all_audio = clips
            .iter()
            .all(|clip| matches!(clip.kind, ClipKind::Audio(_)));
        if all_audio {
            shared.push((Field::Param("gain".to_owned()), "Gain"));
        }
        // Shared fields are keyed by the selection, so a value typed for one
        // selection can never commit into the next one.
        let prefix = format!(
            "multi:{}:",
            ids.iter()
                .map(|id| id.0.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        self.fields.retain_multi(&prefix);
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
                    Field::Transform(property) => format!("{prefix}f:{property:?}"),
                    Field::Param(name) => format!("{prefix}p:{name}"),
                };
                let input_target = InputTarget::SelectionField {
                    field: field.clone(),
                    ids: ids.clone(),
                };
                // Gain steps in whole decibels, like the single-clip row.
                let step = if is_gain_target(&input_target) {
                    1.0
                } else {
                    field_step(&field)
                };
                let control = self.number_control(&key, input_target, value, step, window, cx);
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
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.inspector_scroll)
            .flex()
            .flex_col()
            .children(sectioned(vec![
                self.section("selection", "Selection", details, cx),
                self.section(
                    "shared",
                    "Shared properties",
                    div().flex().flex_col().children(rows),
                    cx,
                ),
            ]));
        pane.child(header).child(self.scrolled(body))
    }

    /// An inspector scroll body with an always-visible scrollbar, so overflow is discoverable.
    fn scrolled(&self, body: Stateful<Div>) -> Div {
        div()
            .flex_1()
            .min_h_0()
            .relative()
            .child(body.pr(SCROLLBAR_GUTTER))
            .child(gpui_component::scroll::Scrollbar::vertical(
                &self.inspector_scroll,
            ))
    }

    /// Inspector panel: header, collapsible sections and widgets for the
    /// selected clip.
    pub(super) fn inspector(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        // An Add effect menu belongs to the selection that opened it.
        if let Some(target) = self.fields.add_menu
            && !target.belongs_to(self.selected, self.audio_inspector_track)
        {
            self.fields.add_menu = None;
        }
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
            .border_color(hex(color::BORDER))
            .text_color(hex(color::TEXT))
            .text_size(crate::style::type_size::BODY);
        let Some(id) = self.selected else {
            if let Some(track) = self.audio_inspector_track
                && let Some(model) = self.project.tracks().iter().find(|t| t.id() == track)
            {
                let title = format!("{} — Audio effects", model.name());
                let mut rows: Vec<AnyElement> = vec![
                    self.add_effect_control(
                        EffectTarget::Audio(AudioChain::Track(track)),
                        window,
                        cx,
                    )
                    .into_any_element(),
                ];
                rows.extend(
                    self.audio_chain_rows(AudioChain::Track(track), window, cx)
                        .into_iter()
                        .map(gpui::IntoElement::into_any_element),
                );
                return pane.child(
                    div()
                        .id("track-audio-inspector")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .children(sectioned(vec![self.section(
                            "track-audio",
                            &title,
                            div().flex().flex_col().children(rows),
                            cx,
                        )])),
                );
            }
            // The one empty state (docs/design.md): centred, icon and one line.
            return pane.child(div().flex_1().flex().flex_col().justify_center().child(
                crate::style::empty_state(
                    Some(IconName::MousePointerClick),
                    "Select a clip to edit it.",
                    None,
                ),
            ));
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
        // Clip header: the name in Heading, the kind as a sentence-case Label.
        pane = pane.child(
            div()
                .px_2()
                .py_2()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .border_b_1()
                .border_color(hex(color::BORDER))
                .child(icons::icon(icons::kind_icon(&clip.kind)).small())
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_role(Role::Heading)
                        .child(clip_title(&self.project, &clip, 0.0)),
                )
                .child(
                    div()
                        .text_role(Role::Label)
                        .flex_shrink_0()
                        .child(kind_display(&clip.kind)),
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
                    .text_role(Role::Label)
                    .text_color(hex(color::DANGER))
                    .child(icons::icon(IconName::TriangleAlert).small())
                    .child(message.clone()),
            );
        }
        // Audio has no picture: transform, compositing and visual effects do not
        // apply, and its gain/mute controls live in the Audio section.
        let visual = !matches!(clip.kind, ClipKind::Audio(_));
        // Generated clips (text, formulas, shapes, graphs, titles) lead with their own content;
        // timed media (video, audio) leads with Speed, which matters for it and little for the rest.
        let timed_media = matches!(clip.kind, ClipKind::Video(_) | ClipKind::Audio(_));
        let speed_rows = self.speed_rows(id, cx);
        let mut speed = Some(self.section("speed", "Speed", speed_rows, cx));
        let mut title_rows = title_fields;
        let mut content: Option<Div> = None;
        if matches!(clip.kind, ClipKind::Plugin { .. }) && !is_title {
            // LaTeX and graph clips render through the standard sections too:
            // their source and ranges are plugin parameters like any other.
            let rows = self.plugin_rows(id, window, cx);
            let (title, icon) = plugin_section_identity(&clip.kind);
            let mut content_column = div().flex().flex_col().children(rows);
            if plugin_id_of(&clip.kind).as_deref() == Some("org.animathio.latex") {
                content_column = content_column.children(self.latex_extras(id, window, cx));
            }
            content = Some(self.section_with_icon("params", title, Some(icon), content_column, cx));
        } else if visual {
            let rows = self.numeric_param_rows(id, window, cx);
            if !rows.is_empty() {
                content = Some(self.section(
                    "params",
                    "Content",
                    div().flex().flex_col().children(rows),
                    cx,
                ));
            }
        }
        let is_audio = matches!(clip.kind, ClipKind::Audio(_));
        let mut sections: Vec<Div> = Vec::new();
        if is_audio {
            // Audio clips lead with what matters for them: gain and mute, then
            // their effect chain in its own Effects section, then Speed.
            let rows = self.audio_rows(id, window, cx);
            sections.push(self.section(
                "audio",
                "Audio",
                div().flex().flex_col().children(rows),
                cx,
            ));
            let mut effects: Vec<AnyElement> = vec![
                self.add_effect_control(EffectTarget::Audio(AudioChain::Clip(id)), window, cx)
                    .into_any_element(),
            ];
            effects.extend(
                self.audio_effect_rows(id, window, cx)
                    .into_iter()
                    .map(gpui::IntoElement::into_any_element),
            );
            sections.push(self.section(
                "effects",
                "Effects",
                div().flex().flex_col().children(effects),
                cx,
            ));
            sections.push(speed.take().expect("speed section"));
        } else if timed_media {
            sections.push(speed.take().expect("speed section"));
        } else {
            if let Some(rows) = title_rows.take() {
                sections.push(rows);
            }
            if let Some(content) = content.take() {
                sections.push(content);
            }
        }
        if visual {
            let transform_rows = self.transform_rows(id, window, cx);
            sections.push(self.section(
                "transform",
                "Transform",
                div().flex().flex_col().children(transform_rows),
                cx,
            ));
        }
        if let Some(rows) = title_rows.take() {
            sections.push(rows);
        }
        if let Some(section) = self.caption_style_section(id, window, cx) {
            sections.push(section);
        }
        if visual {
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
                Some(("slider", 0., 1., 0.01)),
                window,
                cx,
            );
            sections.push(self.section("opacity", "Compositing", div().child(opacity_row), cx));
        }
        if let Some(content) = content.take() {
            sections.push(content);
        }
        if let Some(speed) = speed.take() {
            sections.push(speed);
        }
        if visual {
            let mut rows: Vec<AnyElement> = vec![
                self.add_effect_control(EffectTarget::Video(id), window, cx)
                    .into_any_element(),
            ];
            rows.extend(
                self.video_effect_rows(id, window, cx)
                    .into_iter()
                    .map(gpui::IntoElement::into_any_element),
            );
            sections.push(self.section(
                "effects",
                "Effects",
                div().flex().flex_col().children(rows),
                cx,
            ));
        }
        if !is_audio {
            let rows = self.animation_rows(id, cx);
            sections.push(self.section(
                "animations",
                "Animations",
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
                    .px_2()
                    .text_role(Role::Label)
                    .text_color(hex(color::TEXT_SECONDARY))
                    .tabular()
                    .child(format!(
                        "{:?} / {}",
                        t.kind,
                        timecode::format_timecode(t.duration, self.project.frame_rate())
                    ))
            })
            .collect();
        let transition_content = if transitions.is_empty() {
            div()
                .px_2()
                .text_role(Role::Label)
                .text_color(hex(color::TEXT_MUTED))
                .child("No transition on this clip")
        } else {
            div().children(transitions)
        };
        sections.push(self.section("transition", "Transition", transition_content, cx));
        if let Some(selection) = self.selected_keyframe.clone()
            && selection.clip_id == id
        {
            sections.push(
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
        let body = div()
            .id("inspector-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.inspector_scroll)
            .flex()
            .flex_col()
            .pb_3()
            .bg(hex(color::PANEL))
            .children(sectioned(sections));
        pane.child(self.scrolled(body))
    }

    /// A "Show/Hide" disclosure row in the label column: the label, then a ghost
    /// button that flips `toggle` on the field pool.
    fn disclosure_row(
        button_id: &'static str,
        label: &'static str,
        open: bool,
        cx: &mut Context<Self>,
        toggle: fn(&mut FieldPool),
    ) -> Div {
        Self::param_row_flush(
            label,
            div().flex().child(
                Button::new(button_id)
                    .ghost()
                    .xsmall()
                    .icon(
                        icons::icon(if open {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .xsmall(),
                    )
                    .label(if open { "Hide" } else { "Show" })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        toggle(&mut this.fields);
                        cx.notify();
                    })),
            ),
        )
    }

    /// LaTeX-only extras under the Formula section: the snippets disclosure.
    fn latex_extras(
        &mut self,
        id: ClipId,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut extras: Vec<AnyElement> = Vec::new();
        let open = self.fields.snippets_open;
        extras.push(
            Self::disclosure_row("latex-snippets", "Snippets", open, cx, |fields| {
                fields.snippets_open = !fields.snippets_open;
            })
            .into_any_element(),
        );
        if open {
            let chips = div()
                .px_2()
                .pb_1()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap_1()
                .children(
                    animathio_control::LATEX_SNIPPETS
                        .iter()
                        .map(|(name, snippet)| {
                            let snippet = *snippet;
                            Button::new(SharedString::from(format!(
                                "latex-snippet-{}-{name}",
                                id.0
                            )))
                            .ghost()
                            .xsmall()
                            .label(*name)
                            .tooltip(format!("Insert {snippet} into the source"))
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.insert_latex_snippet(id, snippet, window, cx);
                                },
                            ))
                        }),
                );
            extras.push(chips.into_any_element());
        }
        extras
    }

    /// The failure of a formula or graph that does not typeset, as a `DANGER`
    /// row under its source field.
    fn typeset_error_row(message: &str) -> Div {
        div()
            .px_2()
            .py_1()
            .flex()
            .flex_row()
            .items_start()
            .gap_1()
            .debug_selector(|| "typeset-error".to_owned())
            .text_role(Role::Label)
            .text_color(hex(color::DANGER))
            .child(icons::icon(IconName::TriangleAlert).small())
            .child(message.to_owned())
    }

    /// Why a formula or graph clip does not typeset, or `None` when it does. The
    /// plugins draw an inline error unless asked to be strict, so the clip is
    /// checked with `strict_errors` on (as the Math dialog preview does), once
    /// per change of its parameters, and the result belongs to that clip alone.
    fn typeset_error(&mut self, id: ClipId) -> Option<String> {
        let ClipKind::Plugin { plugin_id, .. } = &self.find_clip(id)?.kind else {
            return None;
        };
        if !matches!(
            plugin_id.as_str(),
            "org.animathio.latex" | "org.animathio.graph"
        ) {
            return None;
        }
        let plugin_id = plugin_id.clone();
        let models = self
            .plugins
            .iter()
            .find(|plugin| plugin.id == plugin_id)?
            .models();
        let mut params = serde_json::Map::new();
        for model in &models {
            if let Some(value) = self.plugin_opaque(id, model.id()) {
                params.insert(model.id().to_owned(), value);
            }
        }
        params.insert("strict_errors".into(), json!(true));
        let params = Value::Object(params);
        let key = params.to_string();
        if let Some((cached, error)) = self.fields.typeset.get(&id)
            && *cached == key
        {
            return error.clone();
        }
        let size = project_size(&self.project);
        let error = self.plugin_host.as_mut().and_then(|host| {
            host.render_clip(&plugin_id, &BTreeMap::new(), &params, 0, size)
                .err()
        });
        let error = error.map(|error| super::math::plain_plugin_error(&error.to_string()));
        self.fields.typeset.insert(id, (key, error.clone()));
        error
    }

    /// Insert a snippet at the LaTeX source editor's caret; the editor's
    /// Change event previews the render and keeps the undo session.
    fn insert_latex_snippet(
        &mut self,
        id: ClipId,
        snippet: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = format!("c{}:pta:source", id.0);
        let Some(field) = self.fields.textareas.get_mut(&key) else {
            return;
        };
        let state = field.state.clone();
        let glue = if state
            .read(cx)
            .value()
            .chars()
            .last()
            .is_some_and(char::is_alphanumeric)
        {
            " "
        } else {
            ""
        };
        state.update(cx, |state, cx| {
            state.insert(format!("{glue}{snippet}"), window, cx);
        });
        cx.notify();
    }

    /// One "Add effect" button with a menu of everything the loaded plugins
    /// offer (docs/design.md: at the top of the Effects section). `target`
    /// selects the pixel effects of a clip or the audio effects of a chain. The
    /// menu is a popover (placed under the button, or above it when there is no
    /// room, never clipped by the scroll area); it closes on a click away, on
    /// Escape, on a choice and when the selection changes.
    fn add_effect_control(
        &mut self,
        target: EffectTarget,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let audio = matches!(target, EffectTarget::Audio(_));
        let available = if audio {
            self.available_audio_effects()
        } else {
            self.available_effects()
        };
        let empty = available.is_empty();
        let open = !empty && self.fields.add_menu == Some(target);
        let editor = cx.entity();
        let button_id = if audio {
            "add-audio-effect"
        } else {
            "add-video-effect"
        };
        let choices: Vec<(String, String, String)> = available
            .into_iter()
            .map(|descriptor| (descriptor.plugin_id, descriptor.effect_id, descriptor.name))
            .collect();
        let toggle = editor.clone();
        let popover = gpui_component::popover::Popover::new(SharedString::from(format!(
            "{button_id}-menu-{}",
            target.key()
        )))
        .anchor(gpui::Anchor::TopLeft)
        .open(open)
        .on_open_change(move |open, _, cx| {
            toggle.update(cx, |this, cx| {
                if *open {
                    this.fields.add_menu = Some(target);
                } else if this.fields.add_menu == Some(target) {
                    this.fields.add_menu = None;
                }
                cx.notify();
            });
        })
        .p_1()
        .trigger(
            Button::new(button_id)
                .outline()
                .small()
                .icon(icons::icon(IconName::Plus).small())
                .label("Add effect")
                .tooltip("Add an effect from the loaded plugins"),
        )
        .content(move |_, _, _| {
            let mut menu = div()
                .id("add-effect-menu")
                .debug_selector(|| "add-effect-menu".to_owned())
                .w(MENU_W_REMS)
                .max_h(MENU_MAX_H_REMS)
                .overflow_y_scroll()
                .flex()
                .flex_col();
            for (plugin, effect, name) in &choices {
                let (plugin, effect) = (plugin.clone(), effect.clone());
                let editor = editor.clone();
                menu = menu.child(
                    div()
                        .id(SharedString::from(format!("add-effect-{name}")))
                        .px_2()
                        .h(size::ROW)
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .cursor_pointer()
                        .rounded(px(crate::style::radius::CONTROL))
                        .text_role(Role::Body)
                        .hover(|item| item.bg(hex(color::HOVER)))
                        .child(icons::icon(IconName::Plus).small())
                        .child(name.clone())
                        .on_click(move |_, _, cx| {
                            editor.update(cx, |this, cx| {
                                this.fields.add_menu = None;
                                match target {
                                    EffectTarget::Audio(chain) => {
                                        this.add_audio_effect_to_chain(chain, &plugin, &effect, cx);
                                    }
                                    EffectTarget::Video(id) => {
                                        this.add_effect_to_clip(id, &plugin, &effect, cx);
                                    }
                                }
                            });
                        }),
                );
            }
            menu
        });
        let mut control = div().px_2().pb_1().child(div().flex().child(popover));
        if empty {
            control = control.child(
                div()
                    .pt_1()
                    .text_role(Role::Label)
                    .text_color(hex(color::TEXT_MUTED))
                    .child(if audio {
                        "No audio effect plugins loaded"
                    } else {
                        "No effect plugins loaded"
                    }),
            );
        }
        control
    }
}

/// What the Add effect menu adds to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum EffectTarget {
    Video(ClipId),
    Audio(AudioChain),
}

impl EffectTarget {
    /// A stable key for element ids.
    fn key(self) -> String {
        match self {
            Self::Video(id) => format!("v{}", id.0),
            Self::Audio(AudioChain::Clip(id)) => format!("ac{}", id.0),
            Self::Audio(AudioChain::Track(track)) => format!("at{}", track.0),
        }
    }

    /// Whether the menu of this target belongs to the current selection.
    fn belongs_to(self, clip: Option<ClipId>, track: Option<TrackId>) -> bool {
        match self {
            Self::Video(id) | Self::Audio(AudioChain::Clip(id)) => clip == Some(id),
            Self::Audio(AudioChain::Track(owner)) => track == Some(owner),
        }
    }
}

/// Sections separated by 1px `BORDER` dividers: every section but the first
/// draws a hairline above it (docs/design.md "Inspector").
fn sectioned(sections: Vec<Div>) -> Vec<AnyElement> {
    let mut out: Vec<AnyElement> = Vec::new();
    for (index, section) in sections.into_iter().enumerate() {
        if index > 0 {
            out.push(
                div()
                    .h(gpui::px(1.))
                    .w_full()
                    .flex_shrink_0()
                    .bg(hex(color::BORDER))
                    .into_any_element(),
            );
        }
        out.push(section.into_any_element());
    }
    out
}

/// The plugin id of a clip, when it is a plugin clip.
fn plugin_id_of(kind: &ClipKind) -> Option<String> {
    match kind {
        ClipKind::Plugin { plugin_id, .. } => Some(plugin_id.clone()),
        _ => None,
    }
}

/// A straight-alpha sRGB colour as the colour picker's value.
pub(super) fn picker_color(color: [u8; 4]) -> Hsla {
    Hsla::from(Rgba {
        r: f32::from(color[0]) / 255.0,
        g: f32::from(color[1]) / 255.0,
        b: f32::from(color[2]) / 255.0,
        a: f32::from(color[3]) / 255.0,
    })
}

/// The name the dialogs and the Math form already call it by.
pub(super) use self::picker_color as to_hsla;

/// Short inspector label for a fit mode.
pub(super) fn fit_label(fit: Fit) -> &'static str {
    match fit {
        Fit::Contain => "Fit",
        Fit::Cover => "Fill",
        Fit::Stretch => "Stretch",
        Fit::Original => "1:1",
    }
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

    fn field_target(property: TransformProperty) -> InputTarget {
        InputTarget::Field {
            id: ClipId(1),
            field: Field::Transform(property),
        }
    }

    /// A percent field scrubs in percent per pixel: the 0.05 model step of an opacity is five
    /// percent, so a drag moves the shown value, not 1/100 of it.
    #[core::prelude::v1::test]
    fn percent_fields_scrub_in_their_display_units() {
        let opacity = field_target(TransformProperty::Opacity);
        let step = scrub_step(
            &opacity,
            field_step(&Field::Transform(TransformProperty::Opacity)),
        );
        assert!((step - 5.).abs() < 1e-9);
        assert!((scrub_value(50., 2., step, false, false) - 60.).abs() < 1e-9);
        let position = field_target(TransformProperty::X);
        assert!((scrub_step(&position, 0.01) - 1.).abs() < 1e-9);
        // Degrees and decibels already show in model steps.
        let rotation = field_target(TransformProperty::Rotation);
        assert!((scrub_step(&rotation, 1.) - 1.).abs() < 1e-9);
    }

    #[core::prelude::v1::test]
    fn position_and_scale_show_as_percent_of_the_canvas() {
        for property in [
            TransformProperty::X,
            TransformProperty::Y,
            TransformProperty::ScaleX,
            TransformProperty::ScaleY,
        ] {
            let target = field_target(property);
            assert!((field_to_display(&target, 0.24) - 24.).abs() < 1e-9);
            assert!((field_from_display(&target, 24.) - 0.24).abs() < 1e-9);
        }
        assert!(
            (field_to_display(&field_target(TransformProperty::Rotation), 30.) - 30.).abs() < 1e-9
        );
    }

    #[core::prelude::v1::test]
    fn long_labels_shorten_and_weights_snap() {
        assert_eq!(
            short_label("Character slide direction (1 left, 2 right, 3 top, 4 bottom)"),
            "Slide direction"
        );
        assert_eq!(short_label("Character slide (1 is at rest)"), "Slide");
        assert_eq!(short_label("Plot stroke width (540p px)"), "Stroke width");
        assert_eq!(short_label("Axes and labels color"), "Axes color");
        assert_eq!(short_label("Font size"), "Font size");
        assert!((nearest_weight(450.) - 500.).abs() < 1e-9);
        assert!((nearest_weight(849.) - 800.).abs() < 1e-9);
        assert!((nearest_weight(0.) - 100.).abs() < 1e-9);
        assert!((nearest_weight(1200.) - 900.).abs() < 1e-9);
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
