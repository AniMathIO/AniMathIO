//! Effect parameter ranges. Core only knows names and finiteness, so range checks live
//! where the plugin descriptors are: command producers call [`validate_effect_command`]
//! before applying a command, and the renderer clamps whatever is stored.

use crate::effects::{EffectDescriptor, EffectParam};
use animathio_core::{ClipId, Command, EffectId, ParamValue, Project, Target};
use std::collections::BTreeMap;

impl EffectParam {
    /// `value` limited to this parameter's declared range.
    #[must_use]
    pub fn clamp(&self, value: f64) -> f64 {
        value.clamp(self.min, self.max)
    }

    fn check(&self, effect: &str, value: f64) -> Result<(), String> {
        if (self.min..=self.max).contains(&value) {
            Ok(())
        } else {
            Err(format!(
                "effect {effect} parameter {:?} = {value} is outside its range {}..={}",
                self.id, self.min, self.max
            ))
        }
    }
}

impl EffectDescriptor {
    /// The declared parameter called `name`.
    #[must_use]
    pub fn param(&self, name: &str) -> Option<&EffectParam> {
        self.params.iter().find(|param| param.id == name)
    }

    fn check_param(&self, name: &str, value: f64) -> Result<(), String> {
        self.param(name)
            .ok_or_else(|| format!("effect {} has no parameter {name:?}", self.effect_id))?
            .check(&self.effect_id, value)
    }

    /// Reject any parameter that is unknown or outside its declared range.
    pub fn validate_params(&self, params: &BTreeMap<String, f64>) -> Result<(), String> {
        params
            .iter()
            .try_for_each(|(name, value)| self.check_param(name, *value))
    }

    /// `params` with every declared parameter clamped into its range. Names the effect
    /// does not declare are passed through untouched.
    #[must_use]
    pub fn clamped_params(&self, params: &BTreeMap<String, f64>) -> BTreeMap<String, f64> {
        params
            .iter()
            .map(|(name, value)| {
                (
                    name.clone(),
                    self.param(name).map_or(*value, |param| param.clamp(*value)),
                )
            })
            .collect()
    }
}

/// Check the effect parameters a user-facing command would store (new effect
/// instances, `SetEffectParam`, effect keyframes) against the plugin descriptors.
///
/// Effects whose plugin is not loaded cannot be checked and pass. `Exact` commands (undo
/// and redo) pass so a stored out-of-range value never blocks history. Call it before
/// applying commands from the editor, the control socket and MCP.
pub fn validate_effect_command(
    project: &Project,
    command: &Command,
    descriptors: &dyn Fn() -> Vec<EffectDescriptor>,
) -> Result<(), String> {
    if !mentions_effect_params(command) {
        return Ok(());
    }
    let descriptors = descriptors();
    let mut added = BTreeMap::new();
    validate(project, command, &descriptors, &mut added)
}

/// Whether `command` stores effect parameters, i.e. whether
/// [`validate_effect_command`] needs the plugin descriptors at all.
#[must_use]
pub fn mentions_effect_params(command: &Command) -> bool {
    match command {
        Command::AddEffect { .. } | Command::SetEffectParam { .. } => true,
        Command::AddKeyframe { target, .. } => matches!(target, Target::Effect { .. }),
        Command::Batch { commands } => commands.iter().any(mentions_effect_params),
        _ => false,
    }
}

type Added = BTreeMap<(ClipId, EffectId), (String, String)>;

fn validate(
    project: &Project,
    command: &Command,
    descriptors: &[EffectDescriptor],
    added: &mut Added,
) -> Result<(), String> {
    let find = |plugin: &str, effect: &str| {
        descriptors
            .iter()
            .find(|d| d.plugin_id == plugin && d.effect_id == effect)
    };
    let identity = |clip: ClipId, effect: EffectId, added: &Added| {
        added.get(&(clip, effect)).cloned().or_else(|| {
            project
                .tracks()
                .iter()
                .find_map(|track| track.clips().get(&clip))?
                .effects
                .iter()
                .find(|instance| instance.id == effect)
                .map(|instance| (instance.plugin.clone(), instance.effect.clone()))
        })
    };
    match command {
        Command::Batch { commands } => {
            for command in commands {
                validate(project, command, descriptors, added)?;
            }
        }
        Command::AddEffect { id, effect, .. } => {
            added.insert(
                (*id, effect.id),
                (effect.plugin.clone(), effect.effect.clone()),
            );
            if let Some(descriptor) = find(&effect.plugin, &effect.effect) {
                descriptor.validate_params(&effect.params)?;
            }
        }
        Command::SetEffectParam {
            id,
            effect,
            name,
            value: Some(value),
        } => {
            if let Some((plugin, filter)) = identity(*id, *effect, added)
                && let Some(descriptor) = find(&plugin, &filter)
            {
                descriptor.check_param(name, *value)?;
            }
        }
        Command::AddKeyframe {
            id,
            target: Target::Effect { effect, param },
            keyframe,
            ..
        } => {
            if let Some((plugin, filter)) = identity(*id, *effect, added)
                && let Some(descriptor) = find(&plugin, &filter)
            {
                match &keyframe.value {
                    ParamValue::Number(value) => descriptor.check_param(param, *value)?,
                    _ => {
                        return Err(format!(
                            "effect parameter {param:?} keyframes must be numbers"
                        ));
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}
