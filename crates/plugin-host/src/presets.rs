//! Animation-preset plugins (API 1.1): registry, sandboxed generation, and the
//! conversion of generated curves into one keyframe [`Command::Batch`].
//!
//! A preset plugin implements the `animation-preset-plugin` world: `describe`, plus
//! the `animation-presets` interface (`list-presets`, `generate`). Generation runs
//! once, when a preset is applied; the result is ordinary keyframes, so rendering and
//! export never depend on the plugin and undo restores the previous keys exactly.

use std::collections::{BTreeMap, BTreeSet};

use animathio_core::{
    Clip, ClipId, ClipKind, Command, Interpolation, Keyframe, ParamValue, Project, Rounding,
    Target, Time, Transform, TransformProperty,
};
use wasmtime::Store;
use wasmtime::component::{Component, Linker};

use crate::compat::Pre;
use crate::preset_world::exports::animathio::plugin::animation_presets as wit;
use crate::{HostError, HostState, wit_types};

/// Most presets one plugin may list.
const MAX_PRESETS: usize = 128;
/// Most curves one generate call may return.
const MAX_CURVES: usize = 16;
/// Largest edit list in one batch level (core's `MAX_BATCH`).
const BATCH_CHUNK: usize = 1024;

/// Where a preset sits in a clip's lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PresetRole {
    /// Window at the clip start ("In").
    Entrance,
    /// Window at the clip end ("Out").
    Exit,
    /// Window centred on the clip.
    Emphasis,
}

impl PresetRole {
    /// Stable id used by the control protocol: `in`, `out` or `emphasis`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Entrance => "in",
            Self::Exit => "out",
            Self::Emphasis => "emphasis",
        }
    }
}

/// Type and range of one preset option.
#[derive(Clone, Debug, PartialEq)]
pub enum PresetParamKind {
    /// Finite number within a range.
    Number {
        /// Smallest accepted value.
        min: f64,
        /// Largest accepted value.
        max: f64,
        /// Default value.
        default: f64,
        /// UI step.
        step: f64,
    },
    /// One of a fixed set of strings.
    Choice {
        /// Accepted values, in display order.
        options: Vec<String>,
        /// Default value.
        default: String,
    },
    /// Boolean switch.
    Toggle {
        /// Default value.
        default: bool,
    },
}

/// One preset option.
#[derive(Clone, Debug, PartialEq)]
pub struct PresetParam {
    /// Option id.
    pub id: String,
    /// Fallback label.
    pub label: String,
    /// Type, range and default.
    pub kind: PresetParamKind,
}

/// A preset offered by a loaded animation-preset plugin.
#[derive(Clone, Debug, PartialEq)]
pub struct PresetDescriptor {
    /// Providing plugin id, e.g. `org.animathio.animations`.
    pub plugin_id: String,
    /// Preset id within the plugin, e.g. `fade-in`.
    pub preset_id: String,
    /// Menu name.
    pub name: String,
    /// One-line description.
    pub description: String,
    /// Window placement.
    pub role: PresetRole,
    /// Clip kind ids the preset applies to (see [`clip_kind_id`]); empty means every
    /// visual clip.
    pub applies_to: Vec<String>,
    /// Default window length.
    pub default_duration: Time,
    /// Options in display order.
    pub params: Vec<PresetParam>,
}

impl PresetDescriptor {
    /// Whether the preset can animate a clip of kind id `kind`.
    #[must_use]
    pub fn applies_to_kind(&self, kind: &str) -> bool {
        if self.applies_to.is_empty() {
            kind != "audio"
        } else {
            self.applies_to.iter().any(|k| k == kind)
        }
    }
}

/// Stable kind id of a clip for preset matching: `video`, `image`, `audio`,
/// `vector`, `error` or `plugin:<plugin-id>`.
#[must_use]
pub fn clip_kind_id(kind: &ClipKind) -> String {
    match kind {
        ClipKind::Video(_) => "video".into(),
        ClipKind::Image(_) => "image".into(),
        ClipKind::Audio(_) => "audio".into(),
        ClipKind::Vector(_) => "vector".into(),
        ClipKind::Error(_) => "error".into(),
        ClipKind::Plugin { plugin_id, .. } => format!("plugin:{plugin_id}"),
    }
}

/// Clip-local `(start, end)` of a preset window of length `window` (clamped to the
/// clip) on a clip lasting `clip_duration`.
#[must_use]
pub fn preset_window(role: PresetRole, clip_duration: Time, window: Time) -> (Time, Time) {
    let total = clip_duration.0.max(0);
    let window = window.0.clamp(0, total);
    match role {
        PresetRole::Entrance => (Time(0), Time(window)),
        PresetRole::Exit => (Time(total - window), Time(total)),
        PresetRole::Emphasis => {
            let start = (total - window) / 2;
            (Time(start), Time(start + window))
        }
    }
}

pub(crate) struct PresetPlugin {
    pre: Pre,
    presets: Vec<PresetDescriptor>,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

impl PresetPlugin {
    /// Describe `component`, list and validate its presets.
    pub(crate) fn register(
        linker: &Linker<HostState>,
        package: &animathio_plugin_api::PluginManifestToml,
        component: &Component,
        store: &mut Store<HostState>,
    ) -> Result<(Self, wit_types::Manifest), String> {
        let pre = Pre::new(
            linker
                .instantiate_pre(component)
                .map_err(|e| e.to_string())?,
            false,
        );
        let instance = pre.instantiate(&mut *store).map_err(|e| e.to_string())?;
        let manifest = instance
            .describe(&mut *store, false)
            .map_err(|e| e.to_string())?;
        crate::validate_manifest(package, &manifest).map_err(|e| e.to_string())?;
        // Check the generate signature up front so a malformed export fails loading.
        let generate = instance
            .export(store, "animation-presets", "generate")
            .map_err(|e| e.to_string())?;
        generate_func(&instance, store, &generate).map_err(|e| e.to_string())?;
        let list = instance
            .export(store, "animation-presets", "list-presets")
            .map_err(|e| e.to_string())?;
        let (listed,) = instance
            .instance
            .get_typed_func::<(), (Vec<wit::PresetDescriptor>,)>(&mut *store, &list)
            .and_then(|func| func.call(&mut *store, ()))
            .map_err(|e| e.to_string())?;
        if listed.len() > MAX_PRESETS {
            return Err(format!("too many presets (maximum {MAX_PRESETS})"));
        }
        let mut seen = BTreeSet::new();
        let presets = listed
            .into_iter()
            .map(|descriptor| {
                if !seen.insert(descriptor.id.clone()) {
                    return Err(format!("duplicate preset id {:?}", descriptor.id));
                }
                convert_descriptor(&package.id, descriptor)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok((Self { pre, presets }, manifest))
    }

    pub(crate) fn presets(&self) -> &[PresetDescriptor] {
        &self.presets
    }
}

/// Typed `generate` export.
type GenerateFunc = wasmtime::component::TypedFunc<
    (String, wit::ClipInfo, i64, Vec<wit_types::ParamEntry>),
    (Result<Vec<wit::Curve>, wit_types::Diagnostic>,),
>;

fn generate_func(
    instance: &crate::compat::Bindings,
    store: &mut Store<HostState>,
    export: &wasmtime::component::ComponentExportIndex,
) -> wasmtime::Result<GenerateFunc> {
    instance.instance.get_typed_func(store, export)
}

fn convert_descriptor(
    plugin_id: &str,
    descriptor: wit::PresetDescriptor,
) -> Result<PresetDescriptor, String> {
    let id = descriptor.id;
    if !valid_id(&id) {
        return Err(format!("invalid preset id {id:?}"));
    }
    if descriptor.name.trim().is_empty() {
        return Err(format!("preset {id:?} has no name"));
    }
    if descriptor.default_duration_ticks <= 0 {
        return Err(format!("preset {id:?} needs a positive default duration"));
    }
    if descriptor.applies_to.len() > 16 || descriptor.applies_to.iter().any(String::is_empty) {
        return Err(format!("preset {id:?} has an invalid applies-to list"));
    }
    if descriptor.params.len() > 16 {
        return Err(format!(
            "preset {id:?} declares too many params (maximum 16)"
        ));
    }
    crate::validate_params(&descriptor.params).map_err(|e| format!("preset {id:?}: {e}"))?;
    let params = descriptor
        .params
        .into_iter()
        .map(|param| {
            let kind = match param.kind {
                wit_types::ParamKind::Number(range) => PresetParamKind::Number {
                    min: range.min,
                    max: range.max,
                    default: range.default,
                    step: range.step,
                },
                wit_types::ParamKind::EnumChoice(choice) => PresetParamKind::Choice {
                    options: choice.options,
                    default: choice.default,
                },
                wit_types::ParamKind::Bool(toggle) => PresetParamKind::Toggle {
                    default: toggle.default,
                },
                _ => {
                    return Err(format!(
                        "preset {id:?} param {:?}: only number, enum-choice and bool are supported",
                        param.id
                    ));
                }
            };
            Ok(PresetParam {
                id: crate::truncate_utf8(&param.id, 64),
                label: crate::truncate_utf8(&param.label, 256),
                kind,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PresetDescriptor {
        plugin_id: plugin_id.to_owned(),
        role: match descriptor.role {
            wit::PresetRole::Entrance => PresetRole::Entrance,
            wit::PresetRole::Exit => PresetRole::Exit,
            wit::PresetRole::Emphasis => PresetRole::Emphasis,
        },
        preset_id: id,
        name: crate::truncate_utf8(&descriptor.name, 256),
        description: crate::truncate_utf8(&descriptor.description, 512),
        applies_to: descriptor.applies_to,
        default_duration: Time(descriptor.default_duration_ticks),
        params,
    })
}

/// Validate caller-supplied options against the schema and fill defaults.
fn preset_params(
    descriptor: &PresetDescriptor,
    values: &BTreeMap<String, ParamValue>,
) -> Result<Vec<wit_types::ParamEntry>, String> {
    if let Some(unknown) = values
        .keys()
        .find(|key| !descriptor.params.iter().any(|param| &param.id == *key))
    {
        return Err(format!(
            "preset {} has no option {unknown:?}",
            descriptor.preset_id
        ));
    }
    descriptor
        .params
        .iter()
        .map(|param| {
            let value = values.get(&param.id);
            let value = match (&param.kind, value) {
                (PresetParamKind::Number { default, .. }, None) => {
                    wit_types::ParamValue::Number(*default)
                }
                (PresetParamKind::Number { min, max, .. }, Some(ParamValue::Number(n))) => {
                    if !n.is_finite() || n < min || n > max {
                        return Err(format!(
                            "option {} must be between {min} and {max}",
                            param.id
                        ));
                    }
                    wit_types::ParamValue::Number(*n)
                }
                (PresetParamKind::Choice { default, .. }, None) => {
                    wit_types::ParamValue::EnumChoice(default.clone())
                }
                (
                    PresetParamKind::Choice { options, .. },
                    Some(ParamValue::Enum(choice) | ParamValue::String(choice)),
                ) => {
                    if !options.contains(choice) {
                        return Err(format!(
                            "option {} must be one of {}",
                            param.id,
                            options.join(", ")
                        ));
                    }
                    wit_types::ParamValue::EnumChoice(choice.clone())
                }
                (PresetParamKind::Toggle { default }, None) => {
                    wit_types::ParamValue::Bool(*default)
                }
                (PresetParamKind::Toggle { .. }, Some(ParamValue::Bool(b))) => {
                    wit_types::ParamValue::Bool(*b)
                }
                _ => return Err(format!("option {} has the wrong type", param.id)),
            };
            Ok(wit_types::ParamEntry {
                id: param.id.clone(),
                value,
            })
        })
        .collect()
}

const TRANSFORM_TARGETS: [(&str, TransformProperty); 6] = [
    ("opacity", TransformProperty::Opacity),
    ("x", TransformProperty::X),
    ("y", TransformProperty::Y),
    ("scale-x", TransformProperty::ScaleX),
    ("scale-y", TransformProperty::ScaleY),
    ("rotation", TransformProperty::Rotation),
];

fn transform_value(transform: &Transform, property: TransformProperty) -> f64 {
    match property {
        TransformProperty::X => transform.x,
        TransformProperty::Y => transform.y,
        TransformProperty::ScaleX => transform.scale_x,
        TransformProperty::ScaleY => transform.scale_y,
        TransformProperty::Rotation => transform.rotation,
        TransformProperty::Opacity => transform.opacity,
    }
}

fn sampled(clip: &Clip, target: &Target, time: Time) -> Option<f64> {
    clip.animations
        .iter()
        .find(|animation| &animation.target == target)
        .and_then(|animation| animation.keys.sample(time))
        .and_then(|value| value.as_number())
}

/// Current numeric value of every animatable target at `time`: the six transform
/// components plus numeric clip parameters (and, for plugin clips, the numeric
/// defaults of the plugin's schema).
fn rest_values(clip: &Clip, time: Time, schema: &[wit_types::ParamSpec]) -> Vec<(String, f64)> {
    let mut rest: Vec<(String, f64)> = TRANSFORM_TARGETS
        .iter()
        .map(|(id, property)| {
            let value = sampled(clip, &Target::Transform(*property), time)
                .unwrap_or_else(|| transform_value(&clip.transform, *property));
            ((*id).to_owned(), value)
        })
        .collect();
    let mut params: BTreeMap<String, f64> = schema
        .iter()
        .filter_map(|spec| match &spec.kind {
            wit_types::ParamKind::Number(range) => Some((spec.id.clone(), range.default)),
            _ => None,
        })
        .collect();
    for (name, value) in &clip.params {
        match value.as_number() {
            Some(number) => {
                params.insert(name.clone(), number);
            }
            None => {
                params.remove(name);
            }
        }
    }
    for (name, value) in &mut params {
        if let Some(number) = sampled(clip, &Target::Param(name.clone()), time) {
            *value = number;
        }
    }
    rest.extend(
        params
            .into_iter()
            .map(|(name, value)| (format!("param:{name}"), value)),
    );
    rest
}

fn parse_target(clip: &Clip, target: &str) -> Result<Target, String> {
    if let Some((_, property)) = TRANSFORM_TARGETS.iter().find(|(id, _)| *id == target) {
        return Ok(Target::Transform(*property));
    }
    let Some(name) = target.strip_prefix("param:") else {
        return Err(format!("unknown curve target {target:?}"));
    };
    if name.is_empty() || name.len() > 64 {
        return Err(format!("invalid parameter target {target:?}"));
    }
    match clip.params.get(name) {
        Some(value) if value.as_number().is_none() => {
            Err(format!("parameter {name:?} is not numeric"))
        }
        Some(_) => Ok(Target::Param(name.to_owned())),
        None if matches!(clip.kind, ClipKind::Plugin { .. }) => Ok(Target::Param(name.to_owned())),
        None => Err(format!("clip has no parameter {name:?}")),
    }
}

/// Nest `commands` into batches of at most [`BATCH_CHUNK`] edits per level.
fn batch(mut commands: Vec<Command>) -> Command {
    while commands.len() > BATCH_CHUNK {
        commands = commands
            .chunks(BATCH_CHUNK)
            .map(|chunk| Command::Batch {
                commands: chunk.to_vec(),
            })
            .collect();
    }
    Command::Batch { commands }
}

/// Turn generated curves into one batch: for each animated target, remove its
/// existing keys inside the window, then add the generated keys (window-relative
/// times shifted to clip-local time).
fn curves_to_command(
    clip: &Clip,
    window: (Time, Time),
    curves: Vec<wit::Curve>,
    max_keys: usize,
) -> Result<Command, String> {
    if curves.is_empty() {
        return Err("preset produced no keyframes".into());
    }
    if curves.len() > MAX_CURVES {
        return Err(format!(
            "preset produced too many curves (maximum {MAX_CURVES})"
        ));
    }
    let total_keys: usize = curves.iter().map(|curve| curve.keys.len()).sum();
    if total_keys > max_keys {
        return Err(format!(
            "preset produced {total_keys} keyframes (maximum {max_keys})"
        ));
    }
    let (start, end) = window;
    let length = end.0 - start.0;
    let mut removals = Vec::new();
    let mut additions = Vec::new();
    let mut targets = BTreeSet::new();
    for curve in curves {
        let target = parse_target(clip, &curve.target)?;
        if !targets.insert(target.clone()) {
            return Err(format!("duplicate curve target {:?}", curve.target));
        }
        if curve.keys.is_empty() {
            return Err(format!("curve {:?} has no keys", curve.target));
        }
        if let Some(animation) = clip.animations.iter().find(|a| a.target == target) {
            removals.extend(animation.keys.keys().range(start..=end).map(|(time, _)| {
                Command::RemoveKeyframe {
                    id: clip.id,
                    target: target.clone(),
                    time: *time,
                }
            }));
        }
        let mut previous = None;
        for key in curve.keys {
            if key.time_ticks < 0 || key.time_ticks > length {
                return Err(format!(
                    "curve {:?} key at {} is outside the window",
                    curve.target, key.time_ticks
                ));
            }
            if previous.is_some_and(|p| key.time_ticks <= p) {
                return Err(format!(
                    "curve {:?} keys must have strictly increasing times",
                    curve.target
                ));
            }
            previous = Some(key.time_ticks);
            if !key.value.is_finite() {
                return Err(format!("curve {:?} has a non-finite value", curve.target));
            }
            let value = if target == Target::Transform(TransformProperty::Opacity) {
                key.value.clamp(0.0, 1.0)
            } else {
                key.value
            };
            additions.push(Command::AddKeyframe {
                id: clip.id,
                target: target.clone(),
                time: Time(start.0 + key.time_ticks),
                keyframe: Keyframe {
                    value: ParamValue::Number(value),
                    interpolation: match key.interpolation {
                        wit::CurveInterpolation::Hold => Interpolation::Hold,
                        wit::CurveInterpolation::Linear => Interpolation::Linear,
                        wit::CurveInterpolation::EaseInOut => Interpolation::EaseInOut,
                    },
                },
            });
        }
    }
    removals.extend(additions);
    Ok(batch(removals))
}

impl crate::PluginHost {
    /// Presets offered by every loaded animation-preset plugin, in plugin-id then
    /// listing order.
    #[must_use]
    pub fn animation_presets(&self) -> Vec<PresetDescriptor> {
        self.presets
            .values()
            .flat_map(|plugin| plugin.presets().iter().cloned())
            .collect()
    }

    /// Generate preset `preset_id` of `plugin_id` for `clip` and return the keyframe
    /// edits as one [`Command::Batch`] (not applied). `duration` overrides the
    /// preset's default window and is clamped to the clip; `params` are validated
    /// against the preset's options, missing ones take their defaults.
    pub fn animation_preset_command(
        &self,
        project: &Project,
        clip: ClipId,
        plugin_id: &str,
        preset_id: &str,
        duration: Option<Time>,
        params: &BTreeMap<String, ParamValue>,
    ) -> Result<Command, HostError> {
        let invalid = |message: String| HostError::InvalidOutput(message);
        let plugin = self
            .presets
            .get(plugin_id)
            .ok_or_else(|| HostError::NotFound(plugin_id.to_owned()))?;
        let descriptor = plugin
            .presets()
            .iter()
            .find(|preset| preset.preset_id == preset_id)
            .ok_or_else(|| HostError::NotFound(format!("{plugin_id}/{preset_id}")))?;
        let clip = project
            .tracks()
            .iter()
            .find_map(|track| track.clips().get(&clip))
            .ok_or_else(|| HostError::NotFound(format!("clip {}", clip.0)))?;
        let kind = clip_kind_id(&clip.kind);
        if !descriptor.applies_to_kind(&kind) {
            return Err(invalid(format!(
                "preset {preset_id} does not apply to {kind} clips"
            )));
        }
        let clip_duration = Time(clip.range.end.0 - clip.range.start.0);
        let requested = duration.unwrap_or(descriptor.default_duration);
        if requested.0 <= 0 {
            return Err(invalid("preset duration must be positive".into()));
        }
        let window = preset_window(descriptor.role, clip_duration, requested);
        let length = window.1.0 - window.0.0;
        if length <= 0 {
            return Err(invalid("clip is too short for a preset".into()));
        }
        let rest_time = match descriptor.role {
            PresetRole::Entrance => window.1,
            PresetRole::Exit | PresetRole::Emphasis => window.0,
        };
        let schema = match &clip.kind {
            ClipKind::Plugin { plugin_id, .. } => self
                .plugins
                .get(plugin_id)
                .map_or_default(|plugin| plugin.wit_manifest.params.clone()),
            _ => Vec::new(),
        };
        let frame_ticks = project
            .frame_rate()
            .frame_to_time(1, Rounding::Nearest)
            .map_or(1, |time| time.0.max(1));
        let info = wit::ClipInfo {
            kind,
            duration_ticks: clip_duration.0,
            frame_ticks,
            rest: rest_values(clip, rest_time, &schema),
        };
        let wit_params = preset_params(descriptor, params).map_err(invalid)?;

        let mut store = self.create_store(crate::CapabilityGrants::default())?;
        let instance = plugin
            .pre
            .instantiate(&mut store)
            .map_err(|e| HostError::Trapped(e.to_string()))?;
        let export = instance
            .export(&mut store, "animation-presets", "generate")
            .map_err(|e| HostError::Trapped(e.to_string()))?;
        let func = generate_func(&instance, &mut store, &export)
            .map_err(|e| HostError::Trapped(e.to_string()))?;
        let (result,) = func
            .call(&mut store, (preset_id.to_owned(), info, length, wit_params))
            .map_err(|e| HostError::Trapped(e.to_string()))?;
        let curves =
            result.map_err(|e| HostError::InvalidOutput(format!("{}: {}", e.code, e.message)))?;
        curves_to_command(clip, window, curves, self.limits.max_preset_keys).map_err(invalid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use animathio_core::{FrameRate, TimeRange, Track, TrackId};

    fn key(time: i64, value: f64) -> wit::CurveKey {
        wit::CurveKey {
            time_ticks: time,
            value,
            interpolation: wit::CurveInterpolation::Linear,
        }
    }

    fn project() -> Project {
        let mut project = Project::new(FrameRate::default());
        Command::AddTrack {
            track: Track::new(TrackId(1), "V1"),
            index: 0,
        }
        .apply(&mut project)
        .unwrap();
        Command::AddClip {
            track: TrackId(1),
            clip: Clip::new(
                ClipId(1),
                animathio_plugin_api::sources::text("Hi".into(), 0.1, [255; 4]),
                TimeRange::new(Time(0), Time(1000)).unwrap(),
            ),
        }
        .apply(&mut project)
        .unwrap();
        project
    }

    fn clip(project: &Project) -> &Clip {
        &project.tracks()[0].clips()[&ClipId(1)]
    }

    #[test]
    fn windows_follow_the_role() {
        let d = Time(1000);
        assert_eq!(
            preset_window(PresetRole::Entrance, d, Time(200)),
            (Time(0), Time(200))
        );
        assert_eq!(
            preset_window(PresetRole::Exit, d, Time(200)),
            (Time(800), Time(1000))
        );
        assert_eq!(
            preset_window(PresetRole::Emphasis, d, Time(200)),
            (Time(400), Time(600))
        );
        assert_eq!(
            preset_window(PresetRole::Exit, d, Time(5000)),
            (Time(0), Time(1000))
        );
    }

    #[test]
    fn curves_become_one_exactly_undoable_batch() {
        let mut project = project();
        // An existing key inside the window is replaced; one outside survives.
        for (time, value) in [(100, 0.3), (900, 0.7)] {
            Command::AddKeyframe {
                id: ClipId(1),
                target: Target::Transform(TransformProperty::Opacity),
                time: Time(time),
                keyframe: Keyframe {
                    value: ParamValue::Number(value),
                    interpolation: Interpolation::Linear,
                },
            }
            .apply(&mut project)
            .unwrap();
        }
        let before = project.clone();
        let rest = rest_values(clip(&project), Time(200), &[]);
        assert!(rest.iter().any(|(k, _)| k == "opacity"));
        let command = curves_to_command(
            clip(&project),
            (Time(0), Time(200)),
            vec![
                wit::Curve {
                    target: "opacity".into(),
                    keys: vec![key(0, -0.5), key(200, 1.0)],
                },
                wit::Curve {
                    target: "param:reveal".into(),
                    keys: vec![key(0, 0.0), key(200, 1.0)],
                },
            ],
            100,
        )
        .unwrap();
        let inverse = command.apply(&mut project).unwrap();
        let opacity = clip(&project)
            .animations
            .iter()
            .find(|a| a.target == Target::Transform(TransformProperty::Opacity))
            .unwrap();
        let times: Vec<_> = opacity.keys.keys().keys().map(|t| t.0).collect();
        assert_eq!(times, [0, 200, 900]);
        assert_eq!(
            opacity.keys.keys()[&Time(0)].value,
            ParamValue::Number(0.0),
            "opacity is clamped"
        );
        inverse.apply(&mut project).unwrap();
        assert_eq!(project, before);
    }

    #[test]
    fn malformed_curves_are_rejected() {
        let project = project();
        let clip = clip(&project);
        let window = (Time(0), Time(200));
        for curves in [
            vec![],
            vec![wit::Curve {
                target: "bogus".into(),
                keys: vec![key(0, 0.0)],
            }],
            vec![wit::Curve {
                target: "x".into(),
                keys: vec![key(0, 0.0), key(201, 1.0)],
            }],
            vec![wit::Curve {
                target: "x".into(),
                keys: vec![key(10, 0.0), key(10, 1.0)],
            }],
            vec![wit::Curve {
                target: "x".into(),
                keys: vec![key(0, f64::NAN)],
            }],
            vec![
                wit::Curve {
                    target: "x".into(),
                    keys: vec![key(0, 0.0)],
                },
                wit::Curve {
                    target: "x".into(),
                    keys: vec![key(0, 0.0)],
                },
            ],
        ] {
            assert!(curves_to_command(clip, window, curves, 100).is_err());
        }
        let many = vec![wit::Curve {
            target: "x".into(),
            keys: (0..=200).map(|t| key(t, 0.0)).collect(),
        }];
        assert!(curves_to_command(clip, window, many, 100).is_err());
    }

    #[test]
    fn options_are_validated_and_defaulted() {
        let descriptor = PresetDescriptor {
            plugin_id: "p".into(),
            preset_id: "slide".into(),
            name: "Slide".into(),
            description: String::new(),
            role: PresetRole::Entrance,
            applies_to: Vec::new(),
            default_duration: Time(10),
            params: vec![
                PresetParam {
                    id: "direction".into(),
                    label: "Direction".into(),
                    kind: PresetParamKind::Choice {
                        options: vec!["left".into(), "right".into()],
                        default: "left".into(),
                    },
                },
                PresetParam {
                    id: "amount".into(),
                    label: "Amount".into(),
                    kind: PresetParamKind::Number {
                        min: 0.0,
                        max: 1.0,
                        default: 0.5,
                        step: 0.1,
                    },
                },
            ],
        };
        let entries = preset_params(&descriptor, &BTreeMap::new()).unwrap();
        assert_eq!(entries.len(), 2);
        let bad = [
            ("direction", ParamValue::Enum("up".into())),
            ("amount", ParamValue::Number(2.0)),
            ("amount", ParamValue::Bool(true)),
            ("other", ParamValue::Number(0.0)),
        ];
        for (key, value) in bad {
            let values = BTreeMap::from([(key.to_owned(), value)]);
            assert!(preset_params(&descriptor, &values).is_err(), "{key}");
        }
        assert!(descriptor.applies_to_kind("video"));
        assert!(!descriptor.applies_to_kind("audio"));
    }
}
