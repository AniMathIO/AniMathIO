//! Clip effects offered by loaded effect plugins: listing and adding.
//!
//! Which effects exist is decided by the plugins the handler loaded, never by a table in
//! this crate. `effects.list` reports each filter with its parameter schema, and
//! `effects.add` builds the `Command::AddEffect` from the plugin's own defaults.

use crate::ControlError;
use animathio_core::{AudioChain, ClipId, Command, EffectId, Project, TrackId};
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

/// Arguments for `audio_effects.add`: the clip or the track whose audio chain to extend.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AudioEffectAddParams {
    /// Clip whose own audio-effect chain to extend. Exactly one of `clip` and `track` is set.
    #[serde(default)]
    pub clip: Option<ClipId>,
    /// Track whose audio-effect chain to extend (applied to the sum of its clips).
    #[serde(default)]
    pub track: Option<TrackId>,
    /// Plugin id, as listed by `audio_effects.list`.
    pub plugin: String,
    /// Filter id, as listed by `audio_effects.list`.
    pub effect: String,
    /// Parameter overrides; unlisted parameters keep the plugin's defaults.
    #[serde(default)]
    pub params: BTreeMap<String, f64>,
    /// Position in the chain; `None` appends.
    #[serde(default)]
    pub index: Option<usize>,
}

/// Identity of a newly added audio effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioEffectAdded {
    /// The chain the effect was added to.
    pub chain: AudioChain,
    /// New effect identity within the clip or track.
    pub effect_id: EffectId,
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

/// Build the command that adds the requested audio effect with plugin defaults plus overrides.
///
/// Effect ids are unique within a clip (across its video and audio chains) or within a track.
pub fn add_audio_effect_command(
    project: &Project,
    descriptors: &[EffectDescriptor],
    params: &AudioEffectAddParams,
) -> Result<(Command, AudioEffectAdded), ControlError> {
    let descriptor = descriptors
        .iter()
        .find(|d| d.plugin_id == params.plugin && d.effect_id == params.effect)
        .ok_or_else(|| {
            ControlError::InvalidParams(format!(
                "audio effect {}/{} is not provided by a loaded plugin; call audio_effects.list",
                params.plugin, params.effect
            ))
        })?;
    let (chain, used_ids, len): (AudioChain, Vec<u64>, usize) = match (params.clip, params.track) {
        (Some(id), None) => {
            let clip = project
                .tracks()
                .iter()
                .find_map(|track| track.clips().get(&id))
                .ok_or_else(|| ControlError::InvalidParams(format!("clip {} not found", id.0)))?;
            (
                AudioChain::Clip(id),
                clip.effects
                    .iter()
                    .chain(&clip.audio_effects)
                    .map(|e| e.id.0)
                    .collect(),
                clip.audio_effects.len(),
            )
        }
        (None, Some(id)) => {
            let track = project
                .tracks()
                .iter()
                .find(|track| track.id() == id)
                .ok_or_else(|| ControlError::InvalidParams(format!("track {} not found", id.0)))?;
            (
                AudioChain::Track(id),
                track.audio_effects().iter().map(|e| e.id.0).collect(),
                track.audio_effects().len(),
            )
        }
        _ => {
            return Err(ControlError::InvalidParams(
                "give exactly one of `clip` and `track`".to_owned(),
            ));
        }
    };
    let mut instance_params = descriptor.default_params();
    for (name, value) in &params.params {
        if !instance_params.contains_key(name) {
            return Err(ControlError::InvalidParams(format!(
                "audio effect {} has no parameter {name:?}",
                descriptor.effect_id
            )));
        }
        if let Some(spec) = descriptor.params.iter().find(|p| &p.id == name)
            && !(spec.min..=spec.max).contains(value)
        {
            return Err(ControlError::InvalidParams(format!(
                "audio effect {} parameter {name:?} must be within {}..={}",
                descriptor.effect_id, spec.min, spec.max
            )));
        }
        instance_params.insert(name.clone(), *value);
    }
    let id = EffectId(used_ids.into_iter().max().map_or(0, |max| max + 1));
    let instance = animathio_core::EffectInstance::new(
        id,
        descriptor.plugin_id.clone(),
        descriptor.effect_id.clone(),
        instance_params,
    );
    Ok((
        Command::AddAudioEffect {
            chain,
            index: params.index.unwrap_or(len),
            effect: instance,
        },
        AudioEffectAdded {
            chain,
            effect_id: id,
        },
    ))
}
