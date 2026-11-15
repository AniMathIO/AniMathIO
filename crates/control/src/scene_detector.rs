//! Shared deterministic sampled-luminance hard-cut detector.
use animathio_core::Time;
use animathio_media::Frame;
pub(crate) const W: usize = 96;
pub(crate) const H: usize = 54;
// Analysis runs at 96x54, regardless of decoded source resolution. FilmCraft currently
// decodes a full frame first; no full-resolution analysis or retained frame sequence.
#[allow(clippy::cast_possible_truncation)]
pub(crate) fn thumbnail(frame: &Frame) -> Vec<f64> {
    let mut out = Vec::with_capacity(W * H);
    for y in 0..H {
        for x in 0..W {
            let sx = (x as u64 * u64::from(frame.width) / W as u64) as usize;
            let sy = (y as u64 * u64::from(frame.height) / H as u64) as usize;
            let i = (sy * frame.width as usize + sx) * 4;
            let p = &frame.pixels[i..i + 3];
            out.push(
                (0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2]))
                    / 255.,
            );
        }
    }
    out
}

#[derive(Default)]
pub(crate) struct Detector {
    previous: Vec<f64>,
    difference: f64,
    last_cut: Time,
}
impl Detector {
    pub(crate) fn sample(
        &mut self,
        luma: &[f64],
        time: Time,
        threshold: f64,
        minimum: Time,
    ) -> bool {
        #[allow(clippy::cast_precision_loss)]
        let diff = if self.previous.is_empty() {
            0.
        } else {
            luma.iter()
                .zip(&self.previous)
                .map(|(a, b)| (a - b).abs())
                .sum::<f64>()
                / luma.len() as f64
        };
        let cut = !self.previous.is_empty()
            && diff > threshold
            && diff > self.difference * 3.
            && time.0 - self.last_cut.0 >= minimum.0;
        self.previous = luma.to_vec();
        self.difference = if cut {
            0.
        } else {
            self.difference * 0.8 + diff * 0.2
        };
        if cut {
            self.last_cut = time;
        }
        cut
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hard_cuts_fade_motion_and_minimum() {
        let mut d = Detector::default();
        assert!(!d.sample(&[0.; 64], Time(0), 0.18, Time(3)));
        assert!(d.sample(&[1.; 64], Time(4), 0.18, Time(3)));
        assert!(!d.sample(&[0.; 64], Time(5), 0.18, Time(3)));
        let mut d = Detector::default();
        for i in 0..30 {
            let mut frame = vec![f64::from(i) / 30.; 64];
            frame[i as usize % 64] = 1.;
            assert!(!d.sample(&frame, Time(i64::from(i)), 0.18, Time(0)));
        }
    }
}
