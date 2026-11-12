//! Resource-free parameter values. Colours are straight-alpha, linear-light RGBA.
use crate::{AssetId, Interpolate};
use serde::{Deserialize, Serialize};

/// A keyframeable parameter. Discrete variants always hold until the next key.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "CompatibleValue")]
pub enum ParamValue {
    /// Continuous scalar.
    Number(f64),
    /// Straight-alpha linear-light RGBA, each channel in `[0, 1]`.
    Color([f32; 4]),
    /// Resolution-independent coordinates.
    Point([f64; 2]),
    /// Boolean switch.
    Bool(bool),
    /// One declared enum choice.
    Enum(String),
    /// Free text.
    String(String),
    /// Optional content-addressed asset reference.
    AssetRef(Option<AssetId>),
}

// Accept the untagged numbers written before typed parameters, including keyframes.
#[derive(Deserialize)]
#[serde(untagged)]
enum CompatibleValue {
    Legacy(f64),
    Typed(TypedValue),
}
#[derive(Deserialize)]
enum TypedValue {
    Number(f64),
    Color([f32; 4]),
    Point([f64; 2]),
    Bool(bool),
    Enum(String),
    String(String),
    AssetRef(Option<AssetId>),
}
impl From<CompatibleValue> for ParamValue {
    fn from(value: CompatibleValue) -> Self {
        match value {
            CompatibleValue::Legacy(v) | CompatibleValue::Typed(TypedValue::Number(v)) => {
                Self::Number(v)
            }
            CompatibleValue::Typed(TypedValue::Color(v)) => Self::Color(v),
            CompatibleValue::Typed(TypedValue::Point(v)) => Self::Point(v),
            CompatibleValue::Typed(TypedValue::Bool(v)) => Self::Bool(v),
            CompatibleValue::Typed(TypedValue::Enum(v)) => Self::Enum(v),
            CompatibleValue::Typed(TypedValue::String(v)) => Self::String(v),
            CompatibleValue::Typed(TypedValue::AssetRef(v)) => Self::AssetRef(v),
        }
    }
}
impl From<f64> for ParamValue {
    fn from(value: f64) -> Self {
        Self::Number(value)
    }
}
impl ParamValue {
    /// Numeric value, when this is a scalar.
    #[must_use]
    pub fn as_number(&self) -> Option<f64> {
        if let Self::Number(v) = self {
            Some(*v)
        } else {
            None
        }
    }
    /// Whether two values belong to the same parameter type.
    #[must_use]
    pub fn same_type(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
    pub(crate) fn valid(&self) -> bool {
        match self {
            Self::Number(v) => v.is_finite(),
            Self::Point(v) => v.iter().all(|x| x.is_finite()),
            Self::Color(v) => v.iter().all(|x| x.is_finite() && (0.0..=1.0).contains(x)),
            _ => true,
        }
    }
    /// Convert byte sRGB colour to the linear-light animation domain.
    #[must_use]
    pub fn from_srgba(value: [u8; 4]) -> Self {
        let mut rgba = value.map(|x| f32::from(x) / 255.0);
        for x in &mut rgba[..3] {
            *x = if *x <= 0.04045 {
                *x / 12.92
            } else {
                ((*x + 0.055) / 1.055).powf(2.4)
            };
        }
        Self::Color(rgba)
    }
    /// Convert to the existing plugin JSON representation, encoding colours as sRGB bytes.
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn to_plugin_json(&self) -> serde_json::Value {
        match self {
            Self::Number(v) => serde_json::json!(v),
            Self::Point(v) => serde_json::json!(v),
            Self::Color(v) => {
                let mut rgba = *v;
                for x in &mut rgba[..3] {
                    *x = if *x <= 0.003_130_8 {
                        *x * 12.92
                    } else {
                        1.055 * x.powf(1.0 / 2.4) - 0.055
                    };
                }
                serde_json::json!(rgba.map(|x| (x.clamp(0.0, 1.0) * 255.0).round() as u8))
            }
            Self::Bool(v) => serde_json::json!(v),
            Self::Enum(v) | Self::String(v) => serde_json::json!(v),
            Self::AssetRef(v) => serde_json::json!(v),
        }
    }
}
impl Interpolate for ParamValue {
    #[allow(clippy::cast_possible_truncation)] // A convex blend of finite f32 channels remains bounded.
    fn interpolate(&self, other: &Self, fraction: f64) -> Self {
        // Returning endpoints verbatim also avoids roundoff at exact key times.
        if fraction <= 0.0 {
            return self.clone();
        }
        match (self, other) {
            (Self::Number(a), Self::Number(b)) => Self::Number(a.interpolate(b, fraction)),
            (Self::Point(a), Self::Point(b)) => {
                Self::Point(std::array::from_fn(|i| a[i].interpolate(&b[i], fraction)))
            }
            (Self::Color(a), Self::Color(b)) => Self::Color(std::array::from_fn(|i| {
                f64::from(a[i]).interpolate(&f64::from(b[i]), fraction) as f32
            })),
            _ => self.clone(),
        }
    }
}
