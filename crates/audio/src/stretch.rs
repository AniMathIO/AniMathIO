//! Bounded, seek-independent WSOLA. Grains advance at the native sample rate;
//! their source anchors follow the exact clip clock. Correlation moves each anchor
//! by at most 5 ms, using the same offset for every channel (stereo stays coherent).
//! Each 64-hop segment replays eight warm-up grains from a canonical entry point.
//! Adjacent segments overlap for one hop and crossfade their independent histories.
//! The cache changes cost only: every segment can be reconstructed on a cold seek.
#![allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
use crate::{Error, prepared::Cursor};
use animathio_core::{Clip, ClipSpeed, TICKS_PER_SECOND, Time};
use std::{collections::HashMap, sync::Arc};

const HOPS: usize = 64;
const WARM: i128 = 8;
const CACHE_BYTES: usize = 16 * 1024 * 1024;

pub(crate) fn active(clip: &Clip) -> bool {
    clip.maintain_pitch
        && !clip.time_map.hold
        && if clip.time_map.ramps.is_empty() {
            clip.time_map.speed != ClipSpeed::default()
        } else {
            clip.time_map
                .ramps
                .iter()
                .any(|p| p.speed != ClipSpeed::default())
        }
}

#[derive(Default)]
pub(crate) struct Cache {
    entries: HashMap<(u64, usize), Arc<Vec<f32>>>,
    bytes: usize,
}
impl Cache {
    pub(crate) fn get(&self, key: u64, segment: usize) -> Option<Arc<Vec<f32>>> {
        self.entries.get(&(key, segment)).cloned()
    }
    pub(crate) fn insert(&mut self, key: u64, segment: usize, samples: Arc<Vec<f32>>) {
        // A simple bounded generation cache; eviction never changes the result.
        if self.bytes + samples.len() * 4 > CACHE_BYTES {
            self.entries.clear();
            self.bytes = 0;
        }
        if samples.len() * 4 <= CACHE_BYTES {
            self.bytes += samples.len() * 4;
            if let Some(old) = self.entries.insert((key, segment), samples) {
                self.bytes -= old.len() * 4;
            }
        }
    }
}

/// Retain the two adjacent segments for a mix loop, even when one segment is
/// larger than the shared cache budget at extreme rates/channel counts.
#[derive(Default)]
pub(crate) struct Segments([Option<(usize, Arc<Vec<f32>>)>; 2]);
impl Segments {
    pub(crate) fn get(&self, index: usize) -> Option<Arc<Vec<f32>>> {
        self.0[index % 2]
            .as_ref()
            .filter(|(n, _)| *n == index)
            .map(|(_, v)| Arc::clone(v))
    }
    pub(crate) fn insert(&mut self, index: usize, entry: Arc<Vec<f32>>) {
        self.0[index % 2] = Some((index, entry));
    }
}

pub(crate) fn hop(rate: u32) -> usize {
    (rate as usize / 100).max(1)
}
pub(crate) fn segment_frames(rate: u32) -> usize {
    hop(rate) * HOPS
}

/// Render one segment, including the following hop used to blend the next reset.
pub(crate) fn render(
    clip: &Clip,
    rate: u32,
    channels: usize,
    segment: usize,
    cursor: &mut Cursor<'_>,
) -> Result<Vec<f32>, Error> {
    let hop = hop(rate);
    let search = (rate as usize / 200).max(1);
    let base = (segment * HOPS) as i128;
    let mut out = vec![0.0; (HOPS + 1) * hop * channels];
    let mut previous: Option<Vec<f32>> = None;
    for grain in base - WARM..=base + HOPS as i128 {
        let frame = grain * hop as i128;
        let local = frame * i128::from(TICKS_PER_SECOND)
            - i128::from(clip.range.start.0) * i128::from(rate);
        let time = Time(
            i64::try_from(local.div_euclid(i128::from(rate)))
                .map_err(|e| Error::Invalid(e.to_string()))?,
        );
        let mut position = i128::from(clip.source_time(time).0) * i128::from(rate);
        if clip.time_map.reverse {
            position -= i128::from(TICKS_PER_SECOND) - i128::from(rate);
        }
        let anchor = position.div_euclid(i128::from(TICKS_PER_SECOND));
        let direction = if clip.time_map.reverse { -1 } else { 1 };
        let first =
            i128::from(clip.source_offset.0) * i128::from(rate) / i128::from(TICKS_PER_SECOND);
        let length = clip
            .time_map
            .source_length
            .map_or_else(|| clip.source_bounds().1.0 - clip.source_offset.0, |t| t.0);
        let end = (i128::from(clip.source_offset.0) + i128::from(length)) * i128::from(rate)
            / i128::from(TICKS_PER_SECOND);
        // Fetch the candidate region once. All indexing is in playback order,
        // so the same overlap/search code also handles reverse clips.
        let mut region = vec![0.0; (2 * hop + 2 * search) * channels];
        for (i, dst) in region.chunks_exact_mut(channels).enumerate() {
            let position = anchor + direction * (i as i128 - search as i128);
            if position >= first
                && position < end
                && let Ok(index) = usize::try_from(position)
                && let Some(src) = cursor.frame(index)?
            {
                dst.copy_from_slice(src);
            }
        }
        let offset = previous.as_ref().map_or(search, |prev| {
            best_overlap(&prev[hop * channels..], &region, channels, hop, search)
        });
        let current = region[offset * channels..(offset + 2 * hop) * channels].to_vec();
        for i in 0..2 * hop {
            let output = frame - base * hop as i128 + i as i128;
            if let Ok(output) = usize::try_from(output)
                && output < (HOPS + 1) * hop
            {
                let weight = if i < hop {
                    i as f32 / hop as f32
                } else {
                    (2 * hop - i) as f32 / hop as f32
                };
                for channel in 0..channels {
                    out[output * channels + channel] += current[i * channels + channel] * weight;
                }
            }
        }
        previous = Some(current);
    }
    Ok(out)
}

fn best_overlap(
    previous: &[f32],
    region: &[f32],
    channels: usize,
    hop: usize,
    search: usize,
) -> usize {
    let mut best = search;
    let mut best_score = f64::NEG_INFINITY;
    // At most 128 temporal probes, across all channels. Searching every source
    // sample avoids coarse phase quantisation, even for high-frequency tones.
    let stride = hop.div_ceil(128);
    for offset in 0..=2 * search {
        let (mut dot, mut energy, mut reference) = (0.0, 0.0, 0.0);
        for i in (0..hop).step_by(stride) {
            for channel in 0..channels {
                let a = f64::from(previous[i * channels + channel]);
                let b = f64::from(region[(offset + i) * channels + channel]);
                dot += a * b;
                energy += b * b;
                reference += a * a;
            }
        }
        // Silence chooses the nominal anchor rather than wandering to an edge.
        let score = if energy * reference > 1e-20 {
            dot / (energy * reference).sqrt()
        } else {
            0.0
        };
        if score > best_score
            || (score.total_cmp(&best_score).is_eq()
                && offset.abs_diff(search) < best.abs_diff(search))
        {
            best = offset;
            best_score = score;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eviction_and_oversized_segments_remain_bounded() {
        let mut cache = Cache::default();
        let entry = Arc::new(vec![0.0; CACHE_BYTES / 8]);
        cache.insert(1, 0, Arc::clone(&entry));
        cache.insert(1, 1, Arc::clone(&entry));
        cache.insert(1, 2, Arc::clone(&entry));
        assert!(cache.bytes <= CACHE_BYTES);
        assert!(cache.get(1, 0).is_none());
        assert!(cache.get(1, 2).is_some());
        let large = Arc::new(vec![0.0; CACHE_BYTES / 4 + 1]);
        cache.insert(1, 3, Arc::clone(&large));
        assert!(cache.get(1, 3).is_none());
        let mut local = Segments::default();
        local.insert(3, Arc::clone(&large));
        assert!(Arc::ptr_eq(&local.get(3).unwrap(), &large));
    }
}
