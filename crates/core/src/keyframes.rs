use crate::Time;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Interpolation from this keyframe to the next keyframe.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Interpolation {
    /// Constant until the next keyframe.
    Hold,
    /// Constant velocity.
    #[default]
    Linear,
    /// Cubic smoothstep, easing in and out.
    EaseInOut,
}

/// One clip-local animated value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Keyframe<T> {
    /// Value at the keyframe's time.
    pub value: T,
    /// Interpolation toward the following keyframe.
    pub interpolation: Interpolation,
}

/// Values supporting interpolation. Time itself never uses this floating-point path.
pub trait Interpolate: Clone {
    /// Blend with another value by a fraction in [0, 1].
    #[must_use]
    fn interpolate(&self, other: &Self, fraction: f64) -> Self;
}
impl Interpolate for f64 {
    fn interpolate(&self, other: &Self, fraction: f64) -> Self {
        // Weighted sum avoids overflowing the difference of two finite endpoints.
        self * (1.0 - fraction) + other * fraction
    }
}

/// Ordered clip-local keyframes; values outside the keyed interval clamp to endpoints.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Keyframes<T>(pub(crate) BTreeMap<Time, Keyframe<T>>);

impl<T> Default for Keyframes<T> {
    fn default() -> Self {
        Self(BTreeMap::new())
    }
}
impl<T> Keyframes<T> {
    /// Read the ordered keys. Mutate project keys through commands.
    #[must_use]
    pub fn keys(&self) -> &BTreeMap<Time, Keyframe<T>> {
        &self.0
    }
}
impl<T: Interpolate> Keyframes<T> {
    /// Sample an animation without state; an empty curve returns `None`.
    #[allow(clippy::cast_precision_loss)] // Relative integer ticks become a dimensionless blend only.
    #[must_use]
    pub fn sample(&self, time: Time) -> Option<T> {
        let Some((left_time, left)) = self.0.range(..=time).next_back() else {
            return self.0.first_key_value().map(|(_, key)| key.value.clone());
        };
        let Some((right_time, right)) = self
            .0
            .range((std::ops::Bound::Excluded(time), std::ops::Bound::Unbounded))
            .next()
        else {
            return Some(left.value.clone());
        };
        let fraction = (i128::from(time.0) - i128::from(left_time.0)) as f64
            / (i128::from(right_time.0) - i128::from(left_time.0)) as f64;
        let fraction = match left.interpolation {
            Interpolation::Hold => 0.0,
            Interpolation::Linear => fraction,
            Interpolation::EaseInOut => fraction * fraction * (3.0 - 2.0 * fraction),
        };
        Some(left.value.interpolate(&right.value, fraction))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::float_cmp)]
    fn interpolation_avoids_overflow_between_opposite_finite_extremes() {
        assert_eq!((-f64::MAX).interpolate(&f64::MAX, 0.5), 0.0);
        assert_eq!(f64::MAX.interpolate(&f64::MAX, 0.5), f64::MAX);
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn sampling_supports_the_entire_signed_tick_domain() {
        let keys = Keyframes(BTreeMap::from([
            (
                Time(i64::MIN),
                Keyframe {
                    value: 0.0,
                    interpolation: Interpolation::Linear,
                },
            ),
            (
                Time(i64::MAX),
                Keyframe {
                    value: 1.0,
                    interpolation: Interpolation::Linear,
                },
            ),
        ]));
        assert_eq!(keys.sample(Time(0)), Some(0.5));
    }
}
