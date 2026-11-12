//! Typed per-clip visual effects.
//!
//! Effects are plain data on a [`Clip`](crate::Clip): an ordered chain of
//! [`EffectInstance`]s with keyframeable numeric parameters. The core owns the
//! model and validation only; pixel processing lives in the `animathio-render`
//! crate (and, later, in an `effects-basic` plugin).

use crate::{Error, invalid};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Stable effect identity within a clip, allocated by the command caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EffectId(pub u64);

/// Builtin visual effect kinds, applied in chain order by the renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EffectKind {
    /// Desaturate toward luminance. Parameter `amount` in `[0, 1]`.
    Grayscale,
    /// Warm sepia tint. Parameter `amount` in `[0, 1]`.
    Sepia,
    /// Invert colors. Parameter `amount` in `[0, 1]`.
    Invert,
    /// Saturation multiplier. Parameter `amount` >= 0 (`1` is identity).
    Saturate,
    /// Brightness multiplier. Parameter `amount` >= 0 (`1` is identity).
    Brightness,
    /// Contrast slope around mid-gray. Parameter `amount` >= 0 (`1` is identity).
    Contrast,
    /// Box blur. Parameter `radius` is a fraction of the canvas height, >= 0.
    Blur,
    /// Multiply straight alpha. Parameter `amount` in `[0, 1]`.
    Opacity,
}

impl EffectKind {
    /// Default parameters. The three multiplier effects and blur default to the
    /// identity, while the four preset effects default to full strength (matching
    /// how the v1 importer emits them).
    #[must_use]
    pub fn default_params(self) -> BTreeMap<String, f64> {
        match self {
            Self::Blur => BTreeMap::from([("radius".to_owned(), 0.0)]),
            Self::Grayscale
            | Self::Sepia
            | Self::Invert
            | Self::Saturate
            | Self::Brightness
            | Self::Contrast
            | Self::Opacity => BTreeMap::from([("amount".to_owned(), 1.0)]),
        }
    }

    /// Parameter names this kind reads. Extra parameters are preserved but unused.
    #[must_use]
    pub fn param_names(self) -> &'static [&'static str] {
        match self {
            Self::Blur => &["radius"],
            Self::Grayscale
            | Self::Sepia
            | Self::Invert
            | Self::Saturate
            | Self::Brightness
            | Self::Contrast
            | Self::Opacity => &["amount"],
        }
    }

    /// Validate one named parameter for this kind.
    pub(crate) fn validate_param(self, name: &str, value: f64) -> Result<(), Error> {
        if !value.is_finite() {
            return Err(invalid("effect parameter must be finite"));
        }
        let valid = match self {
            Self::Grayscale | Self::Sepia | Self::Invert | Self::Opacity => {
                name == "amount" && (0.0..=1.0).contains(&value)
            }
            Self::Saturate | Self::Brightness | Self::Contrast => name == "amount" && value >= 0.0,
            Self::Blur => name == "radius" && value >= 0.0,
        };
        if valid {
            Ok(())
        } else {
            Err(invalid("invalid effect parameter"))
        }
    }
}

/// One ordered effect in a clip's effect chain.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectInstance {
    /// Stable identity, unique within the clip.
    pub id: EffectId,
    /// Which effect to apply.
    pub kind: EffectKind,
    /// Keyframeable base parameters, resolved into the evaluated scene.
    pub params: BTreeMap<String, f64>,
}

impl EffectInstance {
    /// Construct an effect with its default parameters.
    #[must_use]
    pub fn new(id: EffectId, kind: EffectKind) -> Self {
        Self {
            id,
            kind,
            params: kind.default_params(),
        }
    }

    /// Validate required parameters, their ranges and overall finiteness.
    pub(crate) fn validate(&self) -> Result<(), Error> {
        for name in self.kind.param_names() {
            let value = self
                .params
                .get(*name)
                .ok_or_else(|| invalid("effect is missing a required parameter"))?;
            self.kind.validate_param(name, *value)?;
        }
        for value in self.params.values() {
            if !value.is_finite() {
                return Err(invalid("effect parameter must be finite"));
            }
        }
        Ok(())
    }
}
