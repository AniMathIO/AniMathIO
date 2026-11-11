use serde::{Deserialize, Serialize};

use crate::{Error, invalid};

/// Ticks per second. Divisible by 24/25/30/50/60/120, NTSC numerators
/// 24000/30000/60000/120000, and audio rates 44100/48000/96000/192000.
/// Thus both frame and sample boundaries are exact, including 1001 denominators.
/// The i64 range still spans over 400 years in either direction.
pub const TICKS_PER_SECOND: i64 = 705_600_000;

/// Signed integer timeline ticks; negative values are useful for intermediate math.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Time(pub i64);

/// Explicit policy used whenever rational frame boundaries fall between ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rounding {
    /// Toward negative infinity.
    Floor,
    /// Toward positive infinity.
    Ceil,
    /// Nearest integer, with ties away from zero.
    Nearest,
}

/// Positive, reduced rational frames per second.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Rate", into = "Rate")]
pub struct FrameRate {
    numerator: u32,
    denominator: u32,
}

#[derive(Serialize, Deserialize)]
struct Rate {
    numerator: u32,
    denominator: u32,
}

impl FrameRate {
    /// Construct a rate, rejecting zero and reducing equivalent fractions.
    pub fn new(numerator: u32, denominator: u32) -> Result<Self, Error> {
        if numerator == 0 || denominator == 0 {
            return Err(invalid("frame rate must be positive"));
        }
        let (mut a, mut b) = (numerator, denominator);
        while b != 0 {
            (a, b) = (b, a % b);
        }
        Ok(Self {
            numerator: numerator / a,
            denominator: denominator / a,
        })
    }

    /// Reduced numerator.
    #[must_use]
    pub const fn numerator(self) -> u32 {
        self.numerator
    }
    /// Reduced denominator.
    #[must_use]
    pub const fn denominator(self) -> u32 {
        self.denominator
    }

    /// Convert a signed frame index to ticks, detecting overflow.
    pub fn frame_to_time(self, frame: i64, rounding: Rounding) -> Result<Time, Error> {
        rounded(
            i128::from(frame) * i128::from(TICKS_PER_SECOND) * i128::from(self.denominator),
            i128::from(self.numerator),
            rounding,
        )
        .map(Time)
    }

    /// Convert ticks to a signed frame index, detecting overflow.
    pub fn time_to_frame(self, time: Time, rounding: Rounding) -> Result<i64, Error> {
        rounded(
            i128::from(time.0) * i128::from(self.numerator),
            i128::from(TICKS_PER_SECOND) * i128::from(self.denominator),
            rounding,
        )
    }
}

fn rounded(n: i128, d: i128, rounding: Rounding) -> Result<i64, Error> {
    let floor = n.div_euclid(d);
    let remainder = n.rem_euclid(d);
    let result = match rounding {
        Rounding::Floor => floor,
        Rounding::Ceil => floor + i128::from(remainder != 0),
        Rounding::Nearest => floor + i128::from(remainder * 2 > d || (remainder * 2 == d && n > 0)),
    };
    i64::try_from(result).map_err(|_| invalid("timeline conversion overflow"))
}

impl TryFrom<Rate> for FrameRate {
    type Error = Error;
    fn try_from(value: Rate) -> Result<Self, Error> {
        Self::new(value.numerator, value.denominator)
    }
}
impl From<FrameRate> for Rate {
    fn from(value: FrameRate) -> Self {
        Self {
            numerator: value.numerator,
            denominator: value.denominator,
        }
    }
}
impl Default for FrameRate {
    fn default() -> Self {
        Self {
            numerator: 30,
            denominator: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rational_rounding_handles_negative_non_ties_and_exact_values() {
        assert_eq!(rounded(-4, 3, Rounding::Floor).unwrap(), -2);
        assert_eq!(rounded(-4, 3, Rounding::Ceil).unwrap(), -1);
        assert_eq!(rounded(-4, 3, Rounding::Nearest).unwrap(), -1);
        assert_eq!(rounded(-5, 3, Rounding::Nearest).unwrap(), -2);
        for rounding in [Rounding::Floor, Rounding::Ceil, Rounding::Nearest] {
            assert_eq!(rounded(-6, 3, rounding).unwrap(), -2);
        }
    }
}
