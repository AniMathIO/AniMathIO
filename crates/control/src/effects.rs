//! Clip effects offered by loaded effect plugins: listing and adding.
//!
//! Which effects exist is decided by the plugins the handler loaded, never by a table in
//! this crate. `effects.list` reports each filter with its parameter schema, and
//! `effects.add` builds the `Command::AddEffect` from the plugin's own defaults.

use crate::ControlError;
use animathio_core::{ClipId, Command, EffectId, Project};
use animathio_plugin_host::EffectDescriptor;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One numeric effect parameter, as declared by its plugin.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectParamInfo {
    /// Parameter id (the key in `EffectInstance::params`).
    pub id: String,
    /// Display label.
    pub label: String,
    /// Smallest sensible value.
    pub min: f64,
    /// Largest sensible value.
    pub max: f64,
    /// Value used by `effects.add` when the parameter is not given.
    pub default: f64,
}

/// A filter available from a loaded effect plugin.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectInfo {
    /// Providing plugin id.
    pub plugin: String,
    /// Filter id within the plugin.
    pub effect: String,
    /// Display name.
    pub name: String,
    /// One-line description.
    pub description: String,
    /// Parameters in display order.
    pub params: Vec<EffectParamInfo>,
}

/// Arguments for `effects.add`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectAddParams {
    /// Clip to add the effect to.
    pub clip: ClipId,
    /// Plugin id, as listed by `effects.list`.
    pub plugin: String,
    /// Filter id, as listed by `effects.list`.
    pub effect: String,
    /// Parameter overrides; unlisted parameters keep the plugin's defaults.
    #[serde(default)]
    pub params: BTreeMap<String, f64>,
    /// Position in the clip's chain; `None` appends.
    #[serde(default)]
    pub index: Option<usize>,
}

/// Identity of a newly added effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectAdded {
    /// Clip the effect was added to.
    pub clip: ClipId,
    /// New effect identity within the clip.
    pub effect_id: EffectId,
}

/// Convert host descriptors to the wire form.
#[must_use]
pub fn effect_infos(descriptors: Vec<EffectDescriptor>) -> Vec<EffectInfo> {
    descriptors
        .into_iter()
        .map(|d| EffectInfo {
            plugin: d.plugin_id,
            effect: d.effect_id,
            name: d.name,
            description: d.description,
            params: d
                .params
                .into_iter()
                .map(|p| EffectParamInfo {
                    id: p.id,
                    label: p.label,
                    min: p.min,
                    max: p.max,
                    default: p.default,
                })
                .collect(),
        })
        .collect()
}

/// Build the command that adds the requested effect with plugin defaults plus overrides.
pub fn add_effect_command(
    project: &Project,
    descriptors: &[EffectDescriptor],
    params: &EffectAddParams,
) -> Result<(Command, EffectAdded), ControlError> {
    let descriptor = descriptors
        .iter()
        .find(|d| d.plugin_id == params.plugin && d.effect_id == params.effect)
        .ok_or_else(|| {
            ControlError::InvalidParams(format!(
                "effect {}/{} is not provided by a loaded plugin; call effects.list",
                params.plugin, params.effect
            ))
        })?;
    let clip = project
        .tracks()
        .iter()
        .find_map(|track| track.clips().get(&params.clip))
        .ok_or_else(|| ControlError::InvalidParams(format!("clip {} not found", params.clip.0)))?;
    let mut instance_params = descriptor.default_params();
    for (name, value) in &params.params {
        if !instance_params.contains_key(name) {
            return Err(ControlError::InvalidParams(format!(
                "effect {} has no parameter {name:?}",
                descriptor.effect_id
            )));
        }
        instance_params.insert(name.clone(), *value);
    }
    descriptor
        .validate_params(&instance_params)
        .map_err(ControlError::InvalidParams)?;
    let id = EffectId(
        clip.effects
            .iter()
            .map(|e| e.id.0)
            .max()
            .map_or(Some(0), |max| max.checked_add(1))
            .ok_or_else(|| ControlError::InvalidParams("effect id overflow".into()))?,
    );
    let index = params.index.unwrap_or(clip.effects.len());
    let instance = animathio_core::EffectInstance::new(
        id,
        descriptor.plugin_id.clone(),
        descriptor.effect_id.clone(),
        instance_params,
    );
    Ok((
        Command::AddEffect {
            id: params.clip,
            index,
            effect: instance,
        },
        EffectAdded {
            clip: params.clip,
            effect_id: id,
        },
    ))
}
