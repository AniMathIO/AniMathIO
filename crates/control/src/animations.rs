//! Animation presets offered by loaded animation-preset plugins: listing and applying.
//!
//! `animations.list` reports each preset with its role and option schema;
//! `animations.apply` asks the plugin for curves and applies them as one
//! `Command::Batch` of keyframe edits, so the result is ordinary keyframes and a
//! single undo step restores the previous ones exactly.

use crate::ControlError;
use animathio_core::{ClipId, Command, ParamValue, Project, Time};
use animathio_plugin_host::{PluginHost, PresetDescriptor, PresetParamKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// First-party animation-preset plugin, used when `animations.apply` names no plugin.
pub const ANIMATIONS_PLUGIN_ID: &str = "org.animathio.animations";

/// One preset option, as declared by its plugin.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnimationOptionInfo {
    /// Option id.
    pub id: String,
    /// Display label.
    pub label: String,
    /// `number`, `choice` or `toggle`.
    pub kind: String,
    /// Default value (number, string or bool).
    pub default: Value,
    /// Range of a `number` option.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    /// Range of a `number` option.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// Values of a `choice` option.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
}

/// A preset available from a loaded animation-preset plugin.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnimationPresetInfo {
    /// Providing plugin id.
    pub plugin: String,
    /// Preset id within the plugin.
    pub preset: String,
    /// Display name.
    pub name: String,
    /// One-line description.
    pub description: String,
    /// `in`, `out` or `emphasis`.
    pub role: String,
    /// Clip kind ids the preset applies to; empty means every visual clip.
    pub applies_to: Vec<String>,
    /// Default window length in ticks (emphasis presets default to the whole clip).
    pub default_duration: Time,
    /// Options in display order.
    pub options: Vec<AnimationOptionInfo>,
}

/// Arguments for `animations.apply`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnimationApplyParams {
    /// Clip to animate.
    pub clip: ClipId,
    /// Preset id, as listed by `animations.list`.
    pub preset: String,
    /// Plugin id; defaults to `org.animathio.animations`.
    #[serde(default)]
    pub plugin: Option<String>,
    /// Window length in ticks; defaults to the preset's, clamped to the clip.
    #[serde(default)]
    pub duration: Option<Time>,
    /// Option overrides by id (numbers, strings or booleans).
    #[serde(default)]
    pub options: BTreeMap<String, Value>,
}

/// Result of `animations.apply`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnimationApplied {
    /// Animated clip.
    pub clip: ClipId,
    /// Number of keyframes written.
    pub keyframes: usize,
}

/// Describe loaded presets for the protocol.
#[must_use]
pub fn animation_preset_infos(presets: Vec<PresetDescriptor>) -> Vec<AnimationPresetInfo> {
    presets
        .into_iter()
        .map(|preset| AnimationPresetInfo {
            plugin: preset.plugin_id,
            preset: preset.preset_id,
            name: preset.name,
            description: preset.description,
            role: preset.role.as_str().to_owned(),
            applies_to: preset.applies_to,
            default_duration: preset.default_duration,
            options: preset
                .params
                .into_iter()
                .map(|param| {
                    let (kind, default, min, max, options) = match param.kind {
                        PresetParamKind::Number {
                            min, max, default, ..
                        } => (
                            "number",
                            Value::from(default),
                            Some(min),
                            Some(max),
                            Vec::new(),
                        ),
                        PresetParamKind::Choice { options, default } => {
                            ("choice", Value::from(default), None, None, options)
                        }
                        PresetParamKind::Toggle { default } => {
                            ("toggle", Value::from(default), None, None, Vec::new())
                        }
                    };
                    AnimationOptionInfo {
                        id: param.id,
                        label: param.label,
                        kind: kind.to_owned(),
                        default,
                        min,
                        max,
                        options,
                    }
                })
                .collect(),
        })
        .collect()
}

fn option_value(id: &str, value: &Value) -> Result<ParamValue, ControlError> {
    match value {
        Value::Number(n) => n
            .as_f64()
            .map(ParamValue::Number)
            .ok_or_else(|| ControlError::InvalidParams(format!("option {id} is not finite"))),
        Value::String(s) => Ok(ParamValue::Enum(s.clone())),
        Value::Bool(b) => Ok(ParamValue::Bool(*b)),
        _ => Err(ControlError::InvalidParams(format!(
            "option {id} must be a number, string or boolean"
        ))),
    }
}

/// Build the keyframe batch for `params` without applying it.
pub fn animation_preset_command(
    project: &Project,
    plugins: Option<&PluginHost>,
    params: &AnimationApplyParams,
) -> Result<(Command, AnimationApplied), ControlError> {
    let host = plugins.ok_or_else(|| {
        ControlError::Unsupported(
            "no plugins are loaded; build them with scripts/build-plugins.sh".to_owned(),
        )
    })?;
    let plugin = params.plugin.as_deref().unwrap_or(ANIMATIONS_PLUGIN_ID);
    if !host
        .animation_presets()
        .iter()
        .any(|p| p.plugin_id == plugin && p.preset_id == params.preset)
    {
        return Err(ControlError::InvalidParams(format!(
            "unknown animation preset {plugin}/{}",
            params.preset
        )));
    }
    let options = params
        .options
        .iter()
        .map(|(id, value)| Ok((id.clone(), option_value(id, value)?)))
        .collect::<Result<BTreeMap<_, _>, ControlError>>()?;
    let command = host
        .animation_preset_command(
            project,
            params.clip,
            plugin,
            &params.preset,
            params.duration,
            &options,
        )
        .map_err(|error| ControlError::InvalidParams(error.to_string()))?;
    let keyframes = count_added_keys(&command);
    Ok((
        command,
        AnimationApplied {
            clip: params.clip,
            keyframes,
        },
    ))
}

fn count_added_keys(command: &Command) -> usize {
    match command {
        Command::AddKeyframe { .. } => 1,
        Command::Batch { commands } => commands.iter().map(count_added_keys).sum(),
        _ => 0,
    }
}
