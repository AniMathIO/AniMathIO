//! Downsampled waveform peaks for timeline display. Peaks are derived from the
//! pool's decoded PCM, never stored in the project.

use crate::{AudioPool, Pcm};
use animathio_core::{AssetId, TICKS_PER_SECOND};

/// Largest bucket count a caller may request; bounds memory for cached peaks.
pub const MAX_WAVEFORM_BUCKETS: usize = 16_384;

/// Absolute peak per equal-width bucket across the whole asset, folded over
/// channels, plus the asset's duration in ticks.
#[derive(Clone, Debug, PartialEq)]
pub struct Waveform {
    /// Source duration in ticks.
    pub duration_ticks: i64,
    /// Peak magnitude in `0.0..=1.0` for each bucket.
    pub peaks: Vec<f32>,
}

impl Waveform {
    /// Downsample `pcm` to at most `buckets` peaks (at least one).
    #[must_use]
    pub fn from_pcm(pcm: &Pcm, buckets: usize) -> Self {
        let frames = pcm.frames();
        let channels = usize::from(pcm.channels());
        let buckets = buckets.clamp(1, MAX_WAVEFORM_BUCKETS).min(frames.max(1));
        let mut peaks = vec![0.0_f32; buckets];
        if frames > 0 {
            for (frame, samples) in pcm.samples().chunks_exact(channels).enumerate() {
                let bucket = (frame as u128 * buckets as u128 / frames as u128) as usize;
                let peak = samples.iter().fold(0.0_f32, |acc, s| acc.max(s.abs()));
                if peak > peaks[bucket] {
                    peaks[bucket] = peak.min(1.0);
                }
            }
        }
        let duration_ticks = i64::try_from(
            frames as u128 * TICKS_PER_SECOND as u128 / u128::from(pcm.rate().max(1)),
        )
        .unwrap_or(i64::MAX);
        Self {
            duration_ticks,
            peaks,
        }
    }

    /// Peak for the source-time interval `[start, end)` in ticks, or `0.0`
    /// when it lies outside the asset.
    #[must_use]
    pub fn peak_between(&self, start: i64, end: i64) -> f32 {
        if self.duration_ticks <= 0
            || self.peaks.is_empty()
            || end <= start
            || start < 0
            || start >= self.duration_ticks
        {
            return 0.0;
        }
        let len = self.peaks.len() as i128;
        let duration = i128::from(self.duration_ticks);
        let first =
            usize::try_from((i128::from(start) * len / duration).clamp(0, len - 1)).unwrap_or(0);
        let last =
            usize::try_from(((i128::from(end) * len + duration - 1) / duration).clamp(1, len))
                .unwrap_or(1);
        self.peaks[first..last.max(first + 1)]
            .iter()
            .fold(0.0_f32, |acc, p| acc.max(*p))
    }
}

impl AudioPool {
    /// Waveform peaks of a decoded asset, or `None` when it is not in the pool.
    #[must_use]
    pub fn waveform(&self, id: &AssetId, buckets: usize) -> Option<Waveform> {
        self.sources
            .get(id)
            .map(|pcm| Waveform::from_pcm(pcm, buckets))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peaks_fold_channels_and_bound_buckets() {
        // Stereo: frame 1 has the loudest sample on the right channel.
        let pcm = Pcm::new(4, 2, vec![0.1, -0.2, 0.5, -0.9, 0.0, 0.0, 0.3, 0.3]).unwrap();
        let wave = Waveform::from_pcm(&pcm, 4);
        assert_eq!(wave.peaks, vec![0.2, 0.9, 0.0, 0.3]);
        assert_eq!(wave.duration_ticks, TICKS_PER_SECOND);
        // Asking for more buckets than frames never invents data.
        assert_eq!(Waveform::from_pcm(&pcm, 1000).peaks.len(), 4);
        // A bucket request of zero still yields one bucket.
        assert_eq!(Waveform::from_pcm(&pcm, 0).peaks, vec![0.9]);
        // Sub-range queries read the matching buckets only.
        let quarter = TICKS_PER_SECOND / 4;
        assert!((wave.peak_between(quarter, 2 * quarter) - 0.9).abs() < 1e-6);
        assert_eq!(wave.peak_between(-1, quarter), 0.0);
        assert_eq!(
            wave.peak_between(5 * TICKS_PER_SECOND, 6 * TICKS_PER_SECOND),
            0.0
        );
    }

    #[test]
    fn pool_serves_cached_decoded_assets() {
        let mut pool = AudioPool::default();
        let id = AssetId::of(b"a");
        assert!(pool.waveform(&id, 8).is_none());
        pool.insert(id.clone(), &Pcm::new(2, 1, vec![0.5, -0.25]).unwrap())
            .unwrap();
        assert_eq!(pool.waveform(&id, 8).unwrap().peaks, vec![0.5, 0.25]);
    }
}
