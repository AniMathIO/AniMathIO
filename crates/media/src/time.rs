//! Exact time conversion between the core [`Time`] domain and FilmCraft's [`Tick`] domain.
//!
//! The two scales share an integer ratio: `core::TICKS_PER_SECOND * 360 ==
//! filmcraft_time::TICKS_PER_SECOND`. Every core tick is exactly 360 FilmCraft ticks, so no
//! floating point enters the conversion.

use animathio_core::{TICKS_PER_SECOND, Time};
use filmcraft_time::Tick;

/// FilmCraft ticks per core tick.
const FC_TICKS_PER_CORE_TICK: i64 = filmcraft_time::TICKS_PER_SECOND / TICKS_PER_SECOND;

/// Convert a core timeline time to a FilmCraft tick (exact).
#[must_use]
pub(crate) fn to_filmcraft(time: Time) -> Tick {
    let ticks = i128::from(time.0) * i128::from(FC_TICKS_PER_CORE_TICK);
    Tick(ticks.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64)
}

/// Convert a FilmCraft tick to a core timeline time, flooring to the nearest core tick.
#[must_use]
pub(crate) fn from_filmcraft(tick: Tick) -> Time {
    let ticks = i128::from(tick.0).div_euclid(i128::from(FC_TICKS_PER_CORE_TICK));
    Time(ticks.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions_round_trip_exactly() {
        for t in [
            0,
            1,
            -1,
            TICKS_PER_SECOND,
            -TICKS_PER_SECOND,
            TICKS_PER_SECOND * 100,
        ] {
            let core = Time(t);
            assert_eq!(from_filmcraft(to_filmcraft(core)), core);
        }
    }

    #[test]
    fn ratio_is_exact() {
        assert_eq!(FC_TICKS_PER_CORE_TICK, 360);
    }
}
