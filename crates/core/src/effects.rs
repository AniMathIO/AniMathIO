//! Typed per-clip visual effects.
//!
//! Effects are plain data on a [`Clip`](crate::Clip): an ordered chain of
//! [`EffectInstance`]s with keyframeable numeric parameters. The core owns the
//! model only; pixel processing lives in sandboxed plugins (the first-party
//! `org.animathio.effects-basic`) and reaches the renderer through the
//! [`EffectProcessor`] port.

use crate::{Error, invalid};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Stable effect identity within a clip, allocated by the command caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EffectId(pub u64);

/// Plugin id of the first-party basic effects (grayscale, sepia, blur, ...).
pub const BASIC_EFFECTS_PLUGIN: &str = "org.animathio.effects-basic";

/// One ordered effect in a clip's effect chain.
///
/// An effect is a *pixel filter exported by a plugin*: `plugin` names the
/// plugin package (for example [`BASIC_EFFECTS_PLUGIN`]) and `effect` names one
/// of the filters that plugin lists. Parameter names, ranges and defaults are
/// owned by the plugin, so the core only checks that values are finite.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectInstance {
    /// Stable identity, unique within the clip.
    pub id: EffectId,
    /// Plugin id providing the filter, e.g. `org.animathio.effects-basic`.
    pub plugin: String,
    /// Filter id within that plugin, e.g. `grayscale`.
    pub effect: String,
    /// Keyframeable base parameters, resolved into the evaluated scene.
    pub params: BTreeMap<String, f64>,
}

impl EffectInstance {
    /// Construct an effect from explicit parameters (usually the defaults the
    /// plugin reports for the filter).
    #[must_use]
    pub fn new(
        id: EffectId,
        plugin: impl Into<String>,
        effect: impl Into<String>,
        params: BTreeMap<String, f64>,
    ) -> Self {
        Self {
            id,
            plugin: plugin.into(),
            effect: effect.into(),
            params,
        }
    }

    /// Construct a first-party basic effect with its documented default
    /// parameters: `blur` defaults to radius `0`, every other filter to
    /// `amount = 1`. Hosts that load the plugin should prefer the defaults the
    /// plugin itself reports.
    #[must_use]
    pub fn basic(id: EffectId, effect: &str) -> Self {
        let params = if effect == "blur" {
            BTreeMap::from([("radius".to_owned(), 0.0)])
        } else {
            BTreeMap::from([("amount".to_owned(), 1.0)])
        };
        Self::new(id, BASIC_EFFECTS_PLUGIN, effect, params)
    }

    /// Validate identity strings and finiteness. Parameter ranges belong to
    /// the plugin and are checked when the effect runs.
    pub(crate) fn validate(&self) -> Result<(), Error> {
        let valid_name = |name: &str| {
            !name.is_empty()
                && name.len() <= 128
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        };
        if !valid_name(&self.plugin) || !valid_name(&self.effect) {
            return Err(invalid("invalid effect plugin or effect id"));
        }
        for (name, value) in &self.params {
            if name.is_empty() {
                return Err(invalid("effect parameter name is empty"));
            }
            if !value.is_finite() {
                return Err(invalid("effect parameter must be finite"));
            }
        }
        Ok(())
    }
}

/// Runs plugin pixel effects for the renderer.
///
/// The render crate owns frames but not plugins, and the plugin host owns
/// plugins but not rendering; this port connects them. Implementations must be
/// deterministic functions of `(effect, time, pixels)`.
pub trait EffectProcessor {
    /// Apply `effect` at `time` to straight-alpha sRGB RGBA8 `pixels`
    /// (`size.0 * size.1 * 4` bytes, row-major), replacing them with the result.
    ///
    /// # Errors
    /// A human-readable reason when the plugin is missing, rejects the
    /// parameters, exceeds its sandbox limits or returns a malformed frame.
    fn apply(
        &mut self,
        effect: &EffectInstance,
        time: crate::Time,
        size: (u32, u32),
        pixels: &mut Vec<u8>,
    ) -> Result<(), String>;
}
