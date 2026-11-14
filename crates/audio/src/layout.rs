//! Channel layouts and downmix/upmix matrices.
//!
//! Decoded audio carries a [`ChannelLayout`] (the speaker of every interleaved channel). Mapping
//! to the mixer's output channel count goes through a [`ChannelMatrix`]: identical layouts copy,
//! mono duplicates, stereo averages to mono, and surround layouts fold down to stereo with the
//! ITU-R BS.775 coefficients (centre and surrounds at -3 dB, LFE dropped, no normalisation).

use crate::Error;
use std::f32::consts::FRAC_1_SQRT_2;
use symphonia::core::audio::Channels;

/// One loudspeaker position.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Speaker {
    /// Front left.
    FrontLeft,
    /// Front right.
    FrontRight,
    /// Front centre.
    FrontCentre,
    /// Low-frequency effects.
    Lfe,
    /// Rear (back) left.
    BackLeft,
    /// Rear (back) right.
    BackRight,
    /// Rear centre.
    BackCentre,
    /// Side left.
    SideLeft,
    /// Side right.
    SideRight,
    /// Front left-of-centre.
    FrontLeftCentre,
    /// Front right-of-centre.
    FrontRightCentre,
    /// Any position without a downmix rule (height, wide, ...). Dropped when folding down.
    Other,
}

/// The ordered speakers of an interleaved frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelLayout(Vec<Speaker>);
impl ChannelLayout {
    /// The usual layout for `channels` interleaved channels: mono, stereo, 3.0 (L R C),
    /// quad (L R BL BR), 5.0, 5.1 (L R C LFE BL BR), 6.1 and 7.1 (5.1 plus SL SR). Counts above
    /// eight are positions without rules ([`Speaker::Other`]).
    #[must_use]
    pub fn default_for(channels: u16) -> Self {
        use Speaker::{
            BackCentre, BackLeft, BackRight, FrontCentre, FrontLeft, FrontRight, Lfe, Other,
            SideLeft, SideRight,
        };
        let list: &[Speaker] = match channels {
            1 => &[FrontCentre],
            2 => &[FrontLeft, FrontRight],
            3 => &[FrontLeft, FrontRight, FrontCentre],
            4 => &[FrontLeft, FrontRight, BackLeft, BackRight],
            5 => &[FrontLeft, FrontRight, FrontCentre, BackLeft, BackRight],
            6 => &[FrontLeft, FrontRight, FrontCentre, Lfe, BackLeft, BackRight],
            7 => &[
                FrontLeft,
                FrontRight,
                FrontCentre,
                Lfe,
                BackCentre,
                SideLeft,
                SideRight,
            ],
            8 => &[
                FrontLeft,
                FrontRight,
                FrontCentre,
                Lfe,
                BackLeft,
                BackRight,
                SideLeft,
                SideRight,
            ],
            n => return Self(vec![Other; usize::from(n)]),
        };
        Self(list.to_vec())
    }
    /// The layout of Vorbis/Opus channel mapping family 1 (`L C R ... LFE` order).
    #[must_use]
    pub fn vorbis(channels: u16) -> Self {
        use Speaker::{
            BackCentre, BackLeft, BackRight, FrontCentre, FrontLeft, FrontRight, Lfe, SideLeft,
            SideRight,
        };
        let list: &[Speaker] = match channels {
            3 => &[FrontLeft, FrontCentre, FrontRight],
            5 => &[FrontLeft, FrontCentre, FrontRight, BackLeft, BackRight],
            6 => &[FrontLeft, FrontCentre, FrontRight, BackLeft, BackRight, Lfe],
            7 => &[
                FrontLeft,
                FrontCentre,
                FrontRight,
                SideLeft,
                SideRight,
                BackCentre,
                Lfe,
            ],
            8 => &[
                FrontLeft,
                FrontCentre,
                FrontRight,
                SideLeft,
                SideRight,
                BackLeft,
                BackRight,
                Lfe,
            ],
            n => return Self::default_for(n),
        };
        Self(list.to_vec())
    }
    /// Speakers in interleave order, from Symphonia's channel flags (ascending bit order is the
    /// interleave order of every Symphonia decoder).
    #[must_use]
    pub fn from_symphonia(channels: Channels) -> Self {
        let mut list = Vec::new();
        for flag in channels.iter() {
            list.push(match flag {
                Channels::FRONT_LEFT => Speaker::FrontLeft,
                Channels::FRONT_RIGHT => Speaker::FrontRight,
                Channels::FRONT_CENTRE => Speaker::FrontCentre,
                Channels::LFE1 | Channels::LFE2 => Speaker::Lfe,
                Channels::REAR_LEFT => Speaker::BackLeft,
                Channels::REAR_RIGHT => Speaker::BackRight,
                Channels::REAR_CENTRE => Speaker::BackCentre,
                Channels::SIDE_LEFT => Speaker::SideLeft,
                Channels::SIDE_RIGHT => Speaker::SideRight,
                Channels::FRONT_LEFT_CENTRE => Speaker::FrontLeftCentre,
                Channels::FRONT_RIGHT_CENTRE => Speaker::FrontRightCentre,
                _ => Speaker::Other,
            });
        }
        Self(list)
    }
    /// Speakers in interleave order.
    #[must_use]
    pub fn speakers(&self) -> &[Speaker] {
        &self.0
    }
    /// Channel count.
    #[must_use]
    pub fn channels(&self) -> usize {
        self.0.len()
    }
    fn has(&self, speaker: Speaker) -> bool {
        self.0.contains(&speaker)
    }
}

/// Output-by-input gain matrix mapping interleaved source frames to output frames.
#[derive(Clone, Debug, PartialEq)]
pub struct ChannelMatrix {
    inputs: usize,
    outputs: usize,
    /// Row-major `outputs x inputs`.
    gains: Vec<f32>,
}
impl ChannelMatrix {
    /// Build the matrix mapping `source` to `outputs` channels.
    ///
    /// Rules: equal channel counts copy; mono duplicates to every output; stereo-or-wider to
    /// mono is the mean of the stereo fold-down; wider to stereo is the BS.775 fold-down;
    /// stereo to a wider layout places left/right on the matching front speakers of the default
    /// layout for that count, silencing the rest. Any other combination is `Unsupported`.
    pub fn new(source: &ChannelLayout, outputs: u16) -> Result<Self, Error> {
        let inputs = source.channels();
        let out = usize::from(outputs);
        let mut matrix = Self {
            inputs,
            outputs: out,
            gains: vec![0.0; inputs * out],
        };
        let unsupported = || {
            Error::Unsupported(format!(
                "cannot map {inputs}-channel audio to {out}-channel output"
            ))
        };
        if inputs == out {
            for channel in 0..inputs {
                matrix.set(channel, channel, 1.0);
            }
        } else if inputs == 1 {
            for row in 0..out {
                matrix.set(row, 0, 1.0);
            }
        } else if out == 2 {
            matrix.fold_to_stereo(source)?;
        } else if out == 1 {
            let mut stereo = Self {
                inputs,
                outputs: 2,
                gains: vec![0.0; inputs * 2],
            };
            stereo.fold_to_stereo(source)?;
            for column in 0..inputs {
                matrix.set(
                    0,
                    column,
                    f32::midpoint(stereo.get(0, column), stereo.get(1, column)),
                );
            }
        } else if inputs == 2 {
            let target = ChannelLayout::default_for(outputs);
            for (row, speaker) in target.speakers().iter().enumerate() {
                match speaker {
                    Speaker::FrontLeft => matrix.set(row, 0, 1.0),
                    Speaker::FrontRight => matrix.set(row, 1, 1.0),
                    _ => {}
                }
            }
            if !target.has(Speaker::FrontLeft) {
                return Err(unsupported());
            }
        } else {
            return Err(unsupported());
        }
        Ok(matrix)
    }
    fn fold_to_stereo(&mut self, source: &ChannelLayout) -> Result<(), Error> {
        let both_rears = source.has(Speaker::SideLeft) && source.has(Speaker::BackLeft);
        let surround = if both_rears { 0.5 } else { FRAC_1_SQRT_2 };
        let mut any = false;
        for (column, speaker) in source.speakers().iter().enumerate() {
            let (left, right) = match speaker {
                Speaker::FrontLeft => (1.0, 0.0),
                Speaker::FrontRight => (0.0, 1.0),
                Speaker::FrontCentre => (FRAC_1_SQRT_2, FRAC_1_SQRT_2),
                Speaker::FrontLeftCentre => (FRAC_1_SQRT_2, 0.0),
                Speaker::FrontRightCentre => (0.0, FRAC_1_SQRT_2),
                Speaker::BackLeft | Speaker::SideLeft => (surround, 0.0),
                Speaker::BackRight | Speaker::SideRight => (0.0, surround),
                Speaker::BackCentre => (0.5, 0.5),
                Speaker::Lfe | Speaker::Other => (0.0, 0.0),
            };
            any |= left != 0.0 || right != 0.0;
            self.set(0, column, left);
            self.set(1, column, right);
        }
        if any {
            Ok(())
        } else {
            Err(Error::Unsupported(format!(
                "no front or surround speakers to fold {}-channel audio to stereo",
                self.inputs
            )))
        }
    }
    fn set(&mut self, row: usize, column: usize, gain: f32) {
        self.gains[row * self.inputs + column] = gain;
    }
    /// Gain of source channel `column` in output channel `row`.
    #[must_use]
    pub fn get(&self, row: usize, column: usize) -> f32 {
        self.gains[row * self.inputs + column]
    }
    /// Source channel count.
    #[must_use]
    pub fn inputs(&self) -> usize {
        self.inputs
    }
    /// Output channel count.
    #[must_use]
    pub fn outputs(&self) -> usize {
        self.outputs
    }
    /// Map one interleaved source frame. Zero gains are skipped and the first term is stored
    /// unaccumulated, so identity, duplication and the stereo mean reproduce plain copies and
    /// `(a + b) / 2` bit for bit.
    #[allow(clippy::float_cmp)] // Exact 0.0 / 1.0 are structural markers, not computed values.
    pub fn apply(&self, frame: &[f32], out: &mut [f32]) {
        for (row, slot) in out.iter_mut().enumerate().take(self.outputs) {
            let gains = &self.gains[row * self.inputs..(row + 1) * self.inputs];
            let mut acc: Option<f32> = None;
            for (gain, sample) in gains.iter().zip(frame) {
                if *gain != 0.0 {
                    let term = if *gain == 1.0 { *sample } else { sample * gain };
                    acc = Some(acc.map_or(term, |sum| sum + term));
                }
            }
            *slot = acc.unwrap_or(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(channels: u16, outputs: u16, frame: &[f32]) -> Vec<f32> {
        let matrix = ChannelMatrix::new(&ChannelLayout::default_for(channels), outputs).unwrap();
        let mut out = vec![0.0; usize::from(outputs)];
        matrix.apply(frame, &mut out);
        out
    }
    fn close(a: &[f32], b: &[f32]) {
        assert!(
            a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6),
            "{a:?} != {b:?}"
        );
    }

    #[test]
    fn copies_duplicates_and_averages_exactly() {
        assert_eq!(map(2, 2, &[0.25, -0.5]), [0.25, -0.5]);
        assert_eq!(map(1, 2, &[0.75]), [0.75, 0.75]);
        assert_eq!(map(1, 3, &[0.75]), [0.75; 3]);
        assert_eq!(map(2, 1, &[0.25, 0.5]), [0.375]);
        assert_eq!(map(2, 1, &[-0.0, -0.0])[0].to_bits(), (-0.0_f32).to_bits());
    }

    #[test]
    fn surround_folds_to_stereo_with_bs775_coefficients() {
        let h = FRAC_1_SQRT_2;
        // 5.1 (L R C LFE BL BR): the centre and surrounds enter at -3 dB, LFE is dropped.
        close(&map(6, 2, &[1., 0., 0., 0., 0., 0.]), &[1., 0.]);
        close(&map(6, 2, &[0., 0., 1., 0., 0., 0.]), &[h, h]);
        close(&map(6, 2, &[0., 0., 0., 1., 0., 0.]), &[0., 0.]);
        close(&map(6, 2, &[0., 0., 0., 0., 1., 0.]), &[h, 0.]);
        close(&map(6, 2, &[0., 0., 0., 0., 0., 1.]), &[0., h]);
        // Quad (L R BL BR).
        close(&map(4, 2, &[0., 1., 0., 0.]), &[0., 1.]);
        close(&map(4, 2, &[0., 0., 1., 1.]), &[h, h]);
        // 7.1 (L R C LFE BL BR SL SR): both rear pairs share the surround gain at -6 dB.
        close(&map(8, 2, &[0., 0., 0., 0., 1., 0., 1., 0.]), &[1., 0.]);
        close(&map(8, 2, &[0., 0., 0., 0., 0., 1., 0., 1.]), &[0., 1.]);
        close(&map(8, 2, &[0., 0., 1., 0., 0., 0., 0., 0.]), &[h, h]);
        // 5.1 to mono is the mean of the stereo fold-down.
        close(&map(6, 1, &[1., 0., 0., 0., 0., 0.]), &[0.5]);
        close(&map(6, 1, &[0., 0., 1., 0., 0., 0.]), &[h]);
    }

    #[test]
    fn stereo_upmix_and_unsupported_pairs() {
        // 5.1 output: left and right land on the front pair, nothing else sounds.
        assert_eq!(map(2, 6, &[0.5, -0.5]), [0.5, -0.5, 0., 0., 0., 0.]);
        // Layouts without a rule are rejected instead of guessed.
        assert!(ChannelMatrix::new(&ChannelLayout::default_for(10), 2).is_err());
        assert!(ChannelMatrix::new(&ChannelLayout::default_for(6), 4).is_err());
        assert!(ChannelMatrix::new(&ChannelLayout::default_for(3), 5).is_err());
    }

    #[test]
    fn vorbis_order_folds_correctly() {
        let h = FRAC_1_SQRT_2;
        // Vorbis 5.1 is L C R BL BR LFE.
        let matrix = ChannelMatrix::new(&ChannelLayout::vorbis(6), 2).unwrap();
        let mut out = [0.0; 2];
        matrix.apply(&[0., 1., 0., 0., 0., 0.], &mut out);
        close(&out, &[h, h]);
        matrix.apply(&[0., 0., 0., 0., 0., 1.], &mut out);
        close(&out, &[0., 0.]);
    }
}
