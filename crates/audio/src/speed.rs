//! Playback speed and the pure block generator behind audible shuttle.
//!
//! Everything here is device-free and allocation-happy by design: it runs on the
//! mixer worker (or the control thread for the prefill), never in the device
//! callback. The callback only ever sees finished interleaved blocks.
#![allow(
    clippy::cast_precision_loss, // Frame positions stay far below 2^53.
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss // Indexes are offsets from the block's own lowest frame.
)]

use crate::Error;

/// Fastest audible forward speed. Faster shuttle keeps the clock but mutes.
pub const MAX_AUDIBLE_FORWARD: i32 = 8;
/// Fastest audible reverse speed.
pub const MAX_AUDIBLE_REVERSE: i32 = 4;
/// Largest accepted magnitude, to keep block spans bounded.
const MAX_SPEED: i64 = 64;

/// A signed rational playback speed (`1/1` is normal, negative is reverse).
/// Rational, so the master clock stays exact at every rate, including `1/2` jog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Speed {
    num: i32,
    den: u32,
}
impl Speed {
    /// Normal forward playback.
    pub const NORMAL: Self = Self { num: 1, den: 1 };
    /// Slow jog, half speed forward.
    pub const HALF: Self = Self { num: 1, den: 2 };
    /// Slow jog, half speed backward.
    pub const HALF_REVERSE: Self = Self { num: -1, den: 2 };

    /// `num / den`. Zero and magnitudes above 64x are rejected.
    pub fn new(num: i32, den: u32) -> Result<Self, Error> {
        if num == 0 || den == 0 || i64::from(num).abs() > MAX_SPEED * i64::from(den) {
            return Err(Error::Invalid("playback speed must be 1/64..=64x".into()));
        }
        let (mut a, mut b) = (num.unsigned_abs(), den);
        while b != 0 {
            (a, b) = (b, a % b);
        }
        Ok(Self {
            num: i32::try_from(i64::from(num) / i64::from(a))
                .map_err(|e| Error::Invalid(e.to_string()))?,
            den: den / a,
        })
    }
    /// A whole-number speed such as `2` or `-4`.
    pub fn whole(speed: i32) -> Result<Self, Error> {
        Self::new(speed, 1)
    }
    /// Signed numerator.
    #[must_use]
    pub fn num(self) -> i32 {
        self.num
    }
    /// Positive denominator.
    #[must_use]
    pub fn den(self) -> u32 {
        self.den
    }
    /// The speed as a float.
    #[must_use]
    pub fn as_f64(self) -> f64 {
        f64::from(self.num) / f64::from(self.den)
    }
    /// Whether the timeline moves backwards.
    #[must_use]
    pub fn is_reverse(self) -> bool {
        self.num < 0
    }
    /// Exactly `1/1`.
    #[must_use]
    pub fn is_normal(self) -> bool {
        self == Self::NORMAL
    }
    /// Forward up to 8x and reverse up to 4x are audible; anything faster is muted.
    #[must_use]
    pub fn is_audible(self) -> bool {
        let limit = if self.is_reverse() {
            MAX_AUDIBLE_REVERSE
        } else {
            MAX_AUDIBLE_FORWARD
        };
        i64::from(self.num).abs() <= i64::from(limit) * i64::from(self.den)
    }
}
impl Default for Speed {
    fn default() -> Self {
        Self::NORMAL
    }
}

/// Render `frames` output frames that read the timeline at `start + speed * k`
/// (fractional timeline frames, linear interpolation). `fetch(first, count)` must
/// return exactly `count` interleaved frames of 1x timeline audio starting at
/// timeline frame `first`, and may be asked for negative frames (silence).
///
/// A negative `speed` reads backwards, so the result is the reversed PCM block.
/// Pitch shifts with speed (varispeed). The caller carries `start + speed *
/// frames` as the next block's start, which keeps interpolation phase continuous:
/// two consecutive blocks equal one block of the combined length.
pub fn varispeed(
    channels: usize,
    start: f64,
    speed: f64,
    frames: usize,
    mut fetch: impl FnMut(i64, usize) -> Result<Vec<f32>, Error>,
) -> Result<Vec<f32>, Error> {
    if channels == 0 || frames == 0 {
        return Ok(Vec::new());
    }
    let at = |k: usize| start + speed * k as f64;
    let (first, last) = (at(0), at(frames - 1));
    #[allow(clippy::cast_possible_truncation)] // Frame positions, bounded by MAX_SPEED * block.
    let (lo, hi) = (
        first.min(last).floor() as i64,
        first.max(last).floor() as i64 + 1,
    );
    let count = usize::try_from(hi - lo + 1).map_err(|e| Error::Invalid(e.to_string()))?;
    let source = fetch(lo, count)?;
    if source.len() != count * channels {
        return Err(Error::Invalid(
            "varispeed source block has wrong length".into(),
        ));
    }
    let mut output = vec![0.0_f32; frames * channels];
    for k in 0..frames {
        let position = at(k);
        let floor = position.floor();
        #[allow(clippy::cast_possible_truncation)]
        let index = (floor as i64 - lo) as usize;
        #[allow(clippy::cast_possible_truncation)]
        let fraction = (position - floor) as f32;
        for channel in 0..channels {
            let a = source[index * channels + channel];
            output[k * channels + channel] = if fraction == 0.0 {
                a
            } else {
                let b = source[(index + 1) * channels + channel];
                a + (b - a) * fraction
            };
        }
    }
    Ok(output)
}

/// Stateful block generator used by the player: owns the fractional timeline
/// position so blocks join without a seam when the speed is constant.
#[derive(Clone, Debug)]
pub struct BlockGenerator {
    speed: Speed,
    channels: usize,
    position: f64,
}
impl BlockGenerator {
    /// Start at timeline frame `position` (at the mixer's sample rate).
    #[must_use]
    pub fn new(speed: Speed, channels: usize, position: f64) -> Self {
        Self {
            speed,
            channels,
            position,
        }
    }
    /// Next timeline frame the generator will read.
    #[must_use]
    pub fn position(&self) -> f64 {
        self.position
    }
    /// The speed this generator renders at.
    #[must_use]
    pub fn speed(&self) -> Speed {
        self.speed
    }
    /// Produce `frames` interleaved output frames. Normal speed reads the timeline
    /// untouched; muted speeds return silence but still advance the position.
    pub fn next_block(
        &mut self,
        frames: usize,
        mut fetch: impl FnMut(i64, usize) -> Result<Vec<f32>, Error>,
    ) -> Result<Vec<f32>, Error> {
        let advance = self.speed.as_f64() * frames as f64;
        let block = if !self.speed.is_audible() {
            vec![0.0; frames * self.channels]
        } else if self.speed.is_normal() {
            #[allow(clippy::cast_possible_truncation)]
            fetch(self.position.round() as i64, frames)?
        } else {
            varispeed(
                self.channels,
                self.position,
                self.speed.as_f64(),
                frames,
                fetch,
            )?
        };
        self.position += advance;
        Ok(block)
    }
}

#[cfg(test)]
#[allow(clippy::cast_possible_wrap, clippy::unnecessary_wraps)]
mod tests {
    use super::*;

    /// A mono "timeline": sample at frame `f` is `f` (zero for negative frames).
    fn ramp(first: i64, count: usize) -> Result<Vec<f32>, Error> {
        Ok((0..count as i64)
            .map(|i| {
                if first + i < 0 {
                    0.0
                } else {
                    (first + i) as f32
                }
            })
            .collect())
    }

    #[test]
    fn speed_is_reduced_validated_and_audibility_limits_hold() {
        assert_eq!(Speed::new(2, 4).unwrap(), Speed::HALF);
        assert!(Speed::new(0, 1).is_err());
        assert!(Speed::new(1, 0).is_err());
        assert!(Speed::whole(65).is_err());
        for forward in [1, 2, 4, 8] {
            assert!(Speed::whole(forward).unwrap().is_audible(), "{forward}x");
        }
        assert!(!Speed::whole(16).unwrap().is_audible());
        for reverse in [-1, -2, -4] {
            assert!(Speed::whole(reverse).unwrap().is_audible(), "{reverse}x");
        }
        assert!(!Speed::whole(-8).unwrap().is_audible());
        assert!(Speed::HALF_REVERSE.is_audible() && Speed::HALF_REVERSE.is_reverse());
    }

    #[test]
    fn forward_rates_pick_every_nth_frame() {
        let two = varispeed(1, 10.0, 2.0, 5, ramp).unwrap();
        assert_eq!(two, [10.0, 12.0, 14.0, 16.0, 18.0]);
        let eight = varispeed(1, 0.0, 8.0, 3, ramp).unwrap();
        assert_eq!(eight, [0.0, 8.0, 16.0]);
    }

    #[test]
    fn half_speed_interpolates_between_frames() {
        let out = varispeed(1, 4.0, 0.5, 4, ramp).unwrap();
        assert_eq!(out, [4.0, 4.5, 5.0, 5.5]);
    }

    #[test]
    fn reverse_plays_the_pcm_block_backwards() {
        let one = varispeed(1, 9.0, -1.0, 4, ramp).unwrap();
        assert_eq!(one, [9.0, 8.0, 7.0, 6.0]);
        let four = varispeed(1, 20.0, -4.0, 4, ramp).unwrap();
        assert_eq!(four, [20.0, 16.0, 12.0, 8.0]);
        // Running off the start of the timeline is silence, not an error.
        let edge = varispeed(1, 1.0, -1.0, 4, ramp).unwrap();
        assert_eq!(edge, [1.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn stereo_channels_stay_interleaved() {
        let stereo = |first: i64, count: usize| {
            Ok((0..count as i64)
                .flat_map(|i| [(first + i) as f32, -((first + i) as f32)])
                .collect())
        };
        let out = varispeed(2, 3.0, -1.0, 3, stereo).unwrap();
        assert_eq!(out, [3.0, -3.0, 2.0, -2.0, 1.0, -1.0]);
    }

    #[test]
    fn consecutive_blocks_equal_one_long_block_at_every_rate() {
        for speed in [
            Speed::HALF,
            Speed::HALF_REVERSE,
            Speed::NORMAL,
            Speed::whole(2).unwrap(),
            Speed::whole(-1).unwrap(),
            Speed::whole(-4).unwrap(),
            Speed::new(3, 2).unwrap(),
            Speed::whole(8).unwrap(),
        ] {
            let start = if speed.is_reverse() { 5000.0 } else { 100.0 };
            let mut split = BlockGenerator::new(speed, 1, start);
            let mut joined = split.next_block(300, ramp).unwrap();
            joined.extend(split.next_block(212, ramp).unwrap());
            let mut whole = BlockGenerator::new(speed, 1, start);
            let expected = whole.next_block(512, ramp).unwrap();
            for (a, b) in joined.iter().zip(&expected) {
                assert!((a - b).abs() < 1e-3, "{speed:?}: {a} vs {b}");
            }
            assert!((split.position() - whole.position()).abs() < 1e-6);
        }
    }

    #[test]
    fn muted_speeds_are_silent_but_keep_the_position_moving() {
        let mut fast = BlockGenerator::new(Speed::whole(16).unwrap(), 2, 0.0);
        let block = fast
            .next_block(100, |_, _| panic!("a muted block must not mix"))
            .unwrap();
        assert_eq!(block.len(), 200);
        assert!(block.iter().all(|s| *s == 0.0));
        assert_eq!(fast.position(), 1600.0);
        let mut back = BlockGenerator::new(Speed::whole(-8).unwrap(), 1, 1600.0);
        back.next_block(100, |_, _| panic!("muted")).unwrap();
        assert_eq!(back.position(), 800.0);
    }

    #[test]
    fn normal_speed_reads_the_timeline_untouched() {
        let mut normal = BlockGenerator::new(Speed::NORMAL, 1, 48.0);
        let block = normal.next_block(4, ramp).unwrap();
        assert_eq!(block, [48.0, 49.0, 50.0, 51.0]);
        assert_eq!(normal.position(), 52.0);
    }
}
