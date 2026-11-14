//! Downsampled waveform peaks for timeline display. Peaks are derived from the
//! pool's decoded PCM, never stored in the project.

use crate::{AudioPool, Error, Pcm, PcmReader};
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

/// Native frames read per step of a streaming waveform pass.
const WAVEFORM_CHUNK_FRAMES: usize = 1 << 16;

/// Accumulates peaks over interleaved frames pushed in order, in memory independent of length.
struct PeakBuilder {
    channels: usize,
    total_frames: usize,
    peaks: Vec<f32>,
    frame: usize,
}
impl PeakBuilder {
    fn new(channels: usize, total_frames: usize, buckets: usize) -> Self {
        let buckets = buckets
            .clamp(1, MAX_WAVEFORM_BUCKETS)
            .min(total_frames.max(1));
        Self {
            channels,
            total_frames,
            peaks: vec![0.0; buckets],
            frame: 0,
        }
    }
    /// Fold interleaved `samples` (whole frames) into the buckets they fall in. Bucket `b`
    /// holds the frames `ceil(b * total / buckets) .. ceil((b + 1) * total / buckets)`, which is
    /// exactly `floor(frame * buckets / total) == b`, so each bucket costs two divisions
    /// instead of one per frame.
    fn push(&mut self, samples: &[f32]) {
        let buckets = self.peaks.len() as u128;
        let total = self.total_frames as u128;
        let frames = samples.len() / self.channels;
        let mut at = 0;
        while at < frames && self.frame < self.total_frames {
            let bucket = (self.frame as u128 * buckets / total) as usize;
            let bucket_end = ((bucket as u128 + 1) * total).div_ceil(buckets) as usize;
            let run = (bucket_end - self.frame).min(frames - at);
            let peak = samples[at * self.channels..(at + run) * self.channels]
                .iter()
                .fold(0.0_f32, |acc, s| acc.max(s.abs()));
            if peak > self.peaks[bucket] {
                self.peaks[bucket] = peak.min(1.0);
            }
            at += run;
            self.frame += run;
        }
        // Frames beyond the announced length (a source longer than it said) are ignored.
    }
    fn finish(self, rate: u32) -> Waveform {
        let duration_ticks = i64::try_from(
            self.total_frames as u128 * TICKS_PER_SECOND as u128 / u128::from(rate.max(1)),
        )
        .unwrap_or(i64::MAX);
        Waveform {
            duration_ticks,
            peaks: self.peaks,
        }
    }
}

impl Waveform {
    /// Downsample `pcm` to at most `buckets` peaks (at least one).
    #[must_use]
    pub fn from_pcm(pcm: &Pcm, buckets: usize) -> Self {
        let mut builder = PeakBuilder::new(usize::from(pcm.channels()), pcm.frames(), buckets);
        builder.push(pcm.samples());
        builder.finish(pcm.rate())
    }

    /// Downsample a whole stream to at most `buckets` peaks in one bounded-memory pass; the
    /// result equals [`Waveform::from_pcm`] on the fully decoded asset.
    pub fn from_reader(reader: &mut dyn PcmReader, buckets: usize) -> Result<Self, Error> {
        let info = reader.info().clone();
        let total = usize::try_from(info.frames).unwrap_or(usize::MAX);
        let mut builder = PeakBuilder::new(usize::from(info.channels), total, buckets);
        let mut chunk = Vec::with_capacity(WAVEFORM_CHUNK_FRAMES * usize::from(info.channels));
        let mut position = 0_u64;
        while position < info.frames {
            chunk.clear();
            let got = reader.read_at(position, WAVEFORM_CHUNK_FRAMES, &mut chunk)?;
            if got == 0 {
                break;
            }
            builder.push(&chunk);
            position += got as u64;
        }
        Ok(builder.finish(info.rate))
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
    /// Waveform peaks of a pooled asset, computed in one streaming pass (bounded memory), or
    /// `None` when it is not in the pool or cannot be read.
    #[must_use]
    pub fn waveform(&self, id: &AssetId, buckets: usize) -> Option<Waveform> {
        let asset = self.sources.get(id)?;
        let info = asset.info.clone();
        let total = usize::try_from(info.frames).unwrap_or(usize::MAX);
        let mut builder = PeakBuilder::new(usize::from(info.channels), total, buckets);
        let mut chunk = Vec::new();
        let mut position = 0_u64;
        while position < info.frames {
            chunk.clear();
            let got = asset
                .read(position, WAVEFORM_CHUNK_FRAMES, &mut chunk)
                .ok()?;
            if got == 0 {
                break;
            }
            builder.push(&chunk);
            position += got as u64;
        }
        Some(builder.finish(info.rate))
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
