//! Schema → inspector model mapping and the command construction rules shared by
//! the plugin inspector and the keyframe UI. Everything here is pure data so the
//! mapping and edit rules are unit-testable without a GPUI context.

use std::collections::BTreeMap;

use animathio_core::{
    Clip, Command, Interpolation, Keyframe, Target, Time, Transform, TransformProperty,
};
use animathio_plugin_api::{
    ColorValue, ControlWidget, InspectorParam, InspectorSchema, PointValue,
};
use serde_json::json;

/// A resolved, UI-agnostic description of one inspectable plugin parameter.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ParamModel {
    /// Continuous numeric value; the only kind the core can keyframe today.
    Number {
        id: String,
        label: String,
        min: f64,
        max: f64,
        step: f64,
        default: f64,
    },
    Color {
        id: String,
        label: String,
        default: ColorValue,
    },
    Text {
        id: String,
        label: String,
        default: String,
    },
    Enum {
        id: String,
        label: String,
        options: Vec<String>,
        default: String,
    },
    Bool {
        id: String,
        label: String,
        default: bool,
    },
    Point {
        id: String,
        label: String,
        default: PointValue,
    },
    Asset {
        id: String,
        label: String,
        media_type: String,
        default: Option<String>,
    },
}

impl ParamModel {
    pub(crate) fn id(&self) -> &str {
        match self {
            Self::Number { id, .. }
            | Self::Color { id, .. }
            | Self::Text { id, .. }
            | Self::Enum { id, .. }
            | Self::Bool { id, .. }
            | Self::Point { id, .. }
            | Self::Asset { id, .. } => id,
        }
    }

    pub(crate) fn label(&self) -> &str {
        match self {
            Self::Number { label, .. }
            | Self::Color { label, .. }
            | Self::Text { label, .. }
            | Self::Enum { label, .. }
            | Self::Bool { label, .. }
            | Self::Point { label, .. }
            | Self::Asset { label, .. } => label,
        }
    }

    /// Whether the model maps onto the numeric, keyframe-capable parameter path.
    /// Non-numeric keyframes need a core model extension and are deferred.
    pub(crate) fn is_keyframeable(&self) -> bool {
        matches!(self, Self::Number { .. })
    }
}

/// A clip-source plugin the Add menu and inspector can render.
#[derive(Clone, Debug)]
pub(crate) struct PluginDescriptor {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) schema: InspectorSchema,
}

impl PluginDescriptor {
    pub(crate) fn models(&self) -> Vec<ParamModel> {
        models_from_schema(&self.schema)
    }
}

/// Map a plugin inspector schema to the editor's control models.
pub(crate) fn models_from_schema(schema: &InspectorSchema) -> Vec<ParamModel> {
    schema.params.iter().map(model_from_param).collect()
}

fn model_from_param(param: &InspectorParam) -> ParamModel {
    match &param.widget {
        ControlWidget::Slider(slider) => ParamModel::Number {
            id: param.id.clone(),
            label: param.label.clone(),
            min: slider.min,
            max: slider.max,
            step: slider.step,
            default: slider.default,
        },
        ControlWidget::ColorPicker { default } => ParamModel::Color {
            id: param.id.clone(),
            label: param.label.clone(),
            default: *default,
        },
        ControlWidget::TextInput { default } => ParamModel::Text {
            id: param.id.clone(),
            label: param.label.clone(),
            default: default.clone(),
        },
        ControlWidget::Dropdown { options, default } => ParamModel::Enum {
            id: param.id.clone(),
            label: param.label.clone(),
            options: options.clone(),
            default: default.clone(),
        },
        ControlWidget::Toggle { default } => ParamModel::Bool {
            id: param.id.clone(),
            label: param.label.clone(),
            default: *default,
        },
        ControlWidget::Point2D { default } => ParamModel::Point {
            id: param.id.clone(),
            label: param.label.clone(),
            default: *default,
        },
        ControlWidget::AssetPicker {
            media_type,
            default_asset,
        } => ParamModel::Asset {
            id: param.id.clone(),
            label: param.label.clone(),
            media_type: media_type.clone(),
            default: default_asset.clone(),
        },
    }
}

/// Default numeric base params and opaque JSON params for a freshly inserted clip.
pub(crate) fn default_params(models: &[ParamModel]) -> (BTreeMap<String, f64>, serde_json::Value) {
    let mut numeric = BTreeMap::new();
    let mut opaque = serde_json::Map::new();
    for model in models {
        match model {
            ParamModel::Number { id, default, .. } => {
                numeric.insert(id.clone(), *default);
            }
            ParamModel::Color { id, default, .. } => {
                opaque.insert(
                    id.clone(),
                    json!([default.r, default.g, default.b, default.a]),
                );
            }
            ParamModel::Text { id, default, .. } | ParamModel::Enum { id, default, .. } => {
                opaque.insert(id.clone(), json!(default));
            }
            ParamModel::Bool { id, default, .. } => {
                opaque.insert(id.clone(), json!(default));
            }
            ParamModel::Point { id, default, .. } => {
                opaque.insert(id.clone(), json!([default.x, default.y]));
            }
            ParamModel::Asset { id, default, .. } => {
                opaque.insert(
                    id.clone(),
                    default
                        .as_ref()
                        .map_or(serde_json::Value::Null, |value| json!(value)),
                );
            }
        }
    }
    (numeric, serde_json::Value::Object(opaque))
}

/// The base numeric value for a target without applying keyframes.
pub(crate) fn numeric_base(clip: &Clip, target: &Target) -> f64 {
    match target {
        Target::Param(name) => clip.params.get(name).copied().unwrap_or(0.0),
        Target::Transform(property) => transform_value(&clip.transform, *property),
    }
}

/// The sampled numeric value for a target at `playhead`, falling back to `base`.
pub(crate) fn sampled_numeric(clip: &Clip, target: &Target, playhead: Time, base: f64) -> f64 {
    let local = clip_local_time(clip, playhead);
    for animation in &clip.animations {
        if &animation.target == target
            && let Some(value) = animation.keys.sample(local)
        {
            return value;
        }
    }
    base
}

/// Whether a keyframe exists for `target` at the playhead.
pub(crate) fn keyframed(clip: &Clip, target: &Target, playhead: Time) -> bool {
    let local = clip_local_time(clip, playhead);
    clip.animations
        .iter()
        .any(|animation| &animation.target == target && animation.keys.keys().contains_key(&local))
}

/// The interpolation of the keyframe at `local`, if any.
pub(crate) fn keyframe_interpolation(
    clip: &Clip,
    target: &Target,
    local: Time,
) -> Option<Interpolation> {
    clip.animations.iter().find_map(|animation| {
        (animation.target == *target)
            .then(|| {
                animation
                    .keys
                    .keys()
                    .get(&local)
                    .map(|key| key.interpolation)
            })
            .flatten()
    })
}

/// Build the command for toggling a keyframe at the playhead. Toggling on keeps
/// the current sampled value so there is no jump; toggling off removes it.
pub(crate) fn keyframe_toggle_command(clip: &Clip, target: Target, playhead: Time) -> Command {
    let local = clip_local_time(clip, playhead);
    if keyframed(clip, &target, playhead) {
        Command::RemoveKeyframe {
            id: clip.id,
            target,
            time: local,
        }
    } else {
        let value = sampled_numeric(clip, &target, playhead, numeric_base(clip, &target));
        Command::AddKeyframe {
            id: clip.id,
            target,
            time: local,
            keyframe: Keyframe {
                value,
                interpolation: Interpolation::Linear,
            },
        }
    }
}

/// Build the command for editing a numeric value. When the target is keyframed at
/// the playhead the edit updates or creates that keyframe; otherwise it sets the
/// base value.
pub(crate) fn numeric_edit_command(
    clip: &Clip,
    target: Target,
    value: f64,
    playhead: Time,
) -> Command {
    let local = clip_local_time(clip, playhead);
    if keyframed(clip, &target, playhead) {
        let interpolation =
            keyframe_interpolation(clip, &target, local).unwrap_or(Interpolation::Linear);
        Command::AddKeyframe {
            id: clip.id,
            target,
            time: local,
            keyframe: Keyframe {
                value,
                interpolation,
            },
        }
    } else {
        match &target {
            Target::Param(name) => Command::SetParam {
                id: clip.id,
                name: name.clone(),
                value: Some(value),
            },
            Target::Transform(property) => Command::SetTransform {
                id: clip.id,
                transform: with_transform(clip.transform, *property, value),
            },
        }
    }
}

/// Build the command that sets a keyframe's outgoing interpolation in place.
pub(crate) fn interpolation_command(
    clip: &Clip,
    target: Target,
    local: Time,
    interpolation: Interpolation,
) -> Option<Command> {
    let value = clip.animations.iter().find_map(|animation| {
        (animation.target == target)
            .then(|| animation.keys.keys().get(&local).map(|key| key.value))
            .flatten()
    })?;
    Some(Command::AddKeyframe {
        id: clip.id,
        target,
        time: local,
        keyframe: Keyframe {
            value,
            interpolation,
        },
    })
}

/// Clip-local time for a timeline playhead, clamped to nonnegative ticks.
pub(crate) fn clip_local_time(clip: &Clip, playhead: Time) -> Time {
    Time(playhead.0.saturating_sub(clip.range.start.0).max(0))
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use animathio_core::{ClipId, ClipKind, TimeRange, Track, TrackId};
    use animathio_plugin_api::InspectorSchema;

    fn clip_with_params() -> Clip {
        let mut clip = Clip::new(
            ClipId(1),
            ClipKind::Plugin {
                plugin_id: "org.animathio.test".into(),
                params: json!({ "align": "center", "opaque": true }),
            },
            TimeRange::new(Time(0), Time(100)).unwrap(),
        );
        clip.params.insert("radius".into(), 5.0);
        clip
    }

    fn number_model() -> ParamModel {
        ParamModel::Number {
            id: "radius".into(),
            label: "Radius".into(),
            min: 0.0,
            max: 100.0,
            step: 1.0,
            default: 10.0,
        }
    }

    #[test]
    fn maps_every_schema_widget_to_a_model() {
        let schema = InspectorSchema::new()
            .with_number("n", "N", 0.0, 1.0, 0.5, 0.1)
            .with_color(
                "c",
                "C",
                ColorValue {
                    r: 1,
                    g: 2,
                    b: 3,
                    a: 255,
                },
            )
            .with_string("s", "S", "hello")
            .with_enum("e", "E", vec!["a".into(), "b".into()], "a")
            .with_bool("b", "B", true)
            .with_point("p", "P", PointValue { x: 1.0, y: 2.0 })
            .with_asset_ref("a", "A", "image/*", None);

        let models = models_from_schema(&schema);
        assert_eq!(models.len(), 7);
        assert!(matches!(&models[0], ParamModel::Number { id, min: 0.0, .. } if id == "n"));
        assert!(matches!(&models[1], ParamModel::Color { id, .. } if id == "c"));
        assert!(
            matches!(&models[2], ParamModel::Text { id, default, .. } if id == "s" && default == "hello")
        );
        assert!(
            matches!(&models[3], ParamModel::Enum { id, options, .. } if id == "e" && options.len() == 2)
        );
        assert!(
            matches!(&models[4], ParamModel::Bool { id, default: true, label: _ } if id == "b")
        );
        assert!(matches!(&models[5], ParamModel::Point { id, .. } if id == "p"));
        assert!(matches!(&models[6], ParamModel::Asset { id, .. } if id == "a"));
    }

    #[test]
    fn default_params_split_numeric_and_opaque_values() {
        let models = vec![
            number_model(),
            ParamModel::Text {
                id: "text".into(),
                label: "Text".into(),
                default: "hi".into(),
            },
        ];
        let (numeric, opaque) = default_params(&models);
        assert_eq!(numeric.get("radius"), Some(&10.0));
        assert_eq!(opaque["text"], json!("hi"));
        assert!(opaque.get("radius").is_none());
    }

    #[test]
    fn keyframe_toggle_and_edit_map_to_the_right_commands() {
        let clip = clip_with_params();
        let target = Target::Param("radius".into());
        let playhead = Time(50);

        // Not keyframed: editing sets the base param; toggling adds a keyframe.
        let edit = numeric_edit_command(&clip, target.clone(), 20.0, playhead);
        assert_eq!(
            edit,
            Command::SetParam {
                id: ClipId(1),
                name: "radius".into(),
                value: Some(20.0),
            }
        );
        let toggle = keyframe_toggle_command(&clip, target.clone(), playhead);
        assert_eq!(
            toggle,
            Command::AddKeyframe {
                id: ClipId(1),
                target: target.clone(),
                time: Time(50),
                keyframe: Keyframe {
                    value: 5.0,
                    interpolation: Interpolation::Linear,
                },
            }
        );

        // Applying the toggle makes the target keyframed at the playhead.
        let mut project = animathio_core::Project::default();
        Command::AddTrack {
            track: Track::new(TrackId(0), "t"),
            index: 0,
        }
        .apply(&mut project)
        .unwrap();
        Command::AddClip {
            track: TrackId(0),
            clip: clip.clone(),
        }
        .apply(&mut project)
        .unwrap();
        toggle.apply(&mut project).unwrap();
        let clip = &project.tracks()[0].clips()[&ClipId(1)];
        assert!(keyframed(clip, &target, playhead));

        // Editing now updates the keyframe at the playhead.
        let edit = numeric_edit_command(clip, target.clone(), 30.0, playhead);
        assert_eq!(
            edit,
            Command::AddKeyframe {
                id: ClipId(1),
                target: target.clone(),
                time: Time(50),
                keyframe: Keyframe {
                    value: 30.0,
                    interpolation: Interpolation::Linear,
                },
            }
        );
    }
}
