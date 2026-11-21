//! Bounded, seek-independent WSOLA. Grains advance at the native sample rate;
//! their source anchors follow the exact clip clock. Correlation moves each anchor
//! by at most 5 ms, using the same offset for every channel (stereo stays coherent).
//! Each 64-hop segment replays eight warm-up grains from a canonical entry point.
//! Adjacent segments overlap for one hop and crossfade their independent histories.
//! Two independent reconstructions of a tone differ by a phase, and crossfading them
//! straight would cancel it, so a segment is first slid onto its predecessor: the lag
//! (at most 5 ms, the smallest one that correlates best) is found from the two raw
//! segments alone, applied at full size at the join and ramped linearly to zero over the
//! segment (a pitch change of at most lag/segment, under 0.8% and typically a few cents),
//! so the next join sees an unshifted tail and no lag ever accumulates or depends on
//! earlier segments.
//! The cache changes cost only: every segment can be reconstructed on a cold seek.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::many_single_char_names
)]
use crate::{Error, prepared::Cursor};
use animathio_core::{Clip, ClipSpeed, TICKS_PER_SECOND, Time};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

const HOPS: usize = 64;
const WARM: i128 = 8;
const CACHE_BYTES: usize = 16 * 1024 * 1024;
/// Summed squares below this (about -120 dBFS over a probe) count as silence.
const SILENCE: f64 = 1e-12;

/// Whether `clip` plays through the stretcher. Pitch preservation only matters when the
/// source clock runs at a non-unit rate somewhere. A constant 1x clip (reversed or not)
/// keeps the exact-sample varispeed path: at 1x it already preserves pitch, reading
/// reversed or forward samples one for one, and a stretcher would only add grain artifacts.
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

/// A cached segment and the clock tick it was last used at.
type Entry = (u64, Arc<Vec<f32>>);

/// Bounded LRU cache of rendered segments. It changes cost only: eviction, an entry
/// larger than the budget and cold starts all reproduce the same samples.
#[derive(Default)]
pub(crate) struct Cache {
    entries: HashMap<(u64, usize), Entry>,
    clock: u64,
    bytes: usize,
}
impl Cache {
    pub(crate) fn get(&mut self, key: u64, segment: usize) -> Option<Arc<Vec<f32>>> {
        self.clock += 1;
        let clock = self.clock;
        self.entries.get_mut(&(key, segment)).map(|(used, entry)| {
            *used = clock;
            Arc::clone(entry)
        })
    }
    pub(crate) fn insert(&mut self, key: u64, segment: usize, samples: Arc<Vec<f32>>) {
        let size = samples.len() * 4;
        // An oversized segment is never kept, and never evicts its neighbours.
        if size > CACHE_BYTES {
            return;
        }
        self.clock += 1;
        self.bytes += size;
        if let Some((_, old)) = self.entries.insert((key, segment), (self.clock, samples)) {
            self.bytes -= old.len() * 4;
        }
        while self.bytes > CACHE_BYTES {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, (used, _))| *used)
                .map(|(k, _)| *k)
            else {
                break;
            };
            if let Some((_, old)) = self.entries.remove(&oldest) {
                self.bytes -= old.len() * 4;
            }
        }
    }
}

/// Source ticks the clip may read from `source_offset`: the recorded span, or (for a
/// clock that never recorded one) the integral up to the clip's end, which a tail trim
/// moves.
fn source_span(clip: &Clip) -> i64 {
    clip.time_map
        .source_length
        .map_or_else(|| clip.source_bounds().1.0 - clip.source_offset.0, |t| t.0)
}

/// Fingerprint of everything a clip's stretched signal depends on besides the shared
/// source and output format: the asset, the placement, the source clock and the source
/// end it may read up to.
pub(crate) fn key(rate: u32, channels: usize, clip: &Clip) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (
        rate,
        channels,
        clip.range.start.0,
        clip.source_offset.0,
        source_span(clip),
    )
        .hash(&mut hasher);
    format!("{:?}{:?}", clip.kind, clip.time_map).hash(&mut hasher);
    hasher.finish()
}

pub(crate) fn hop(rate: u32) -> usize {
    (rate as usize / 100).max(1)
}
pub(crate) fn segment_frames(rate: u32) -> usize {
    hop(rate) * HOPS
}

/// Reads stretched samples frame by frame for one clip. It keeps the segment being
/// played and its predecessor (needed for the crossfade at each reset), so neither a
/// shared-cache lookup nor an `Arc` clone happens per frame.
pub(crate) struct Reader<'a> {
    clip: &'a Clip,
    rate: u32,
    channels: usize,
    key: u64,
    /// First frame the clip plays (dissolve-extended). Segments wholly before it have no
    /// content to blend with.
    first_frame: usize,
    cache: &'a Mutex<Cache>,
    slots: [Option<(usize, Arc<Vec<f32>>)>; 2],
    /// The lag of the segment being played, with its index.
    lag: Option<(usize, i64)>,
}
impl<'a> Reader<'a> {
    pub(crate) fn new(
        clip: &'a Clip,
        rate: u32,
        channels: usize,
        key: u64,
        first_frame: usize,
        cache: &'a Mutex<Cache>,
    ) -> Self {
        Self {
            clip,
            rate,
            channels,
            key,
            first_frame,
            cache,
            slots: [None, None],
            lag: None,
        }
    }
    fn load(&mut self, index: usize, cursor: &mut Cursor<'_>) -> Result<(), Error> {
        if self.slots[index % 2]
            .as_ref()
            .is_some_and(|(held, _)| *held == index)
        {
            return Ok(());
        }
        let cached = self
            .cache
            .lock()
            .expect("stretch cache lock")
            .get(self.key, index);
        let entry = if let Some(entry) = cached {
            entry
        } else {
            let entry = Arc::new(render(self.clip, self.rate, self.channels, index, cursor)?);
            self.cache.lock().expect("stretch cache lock").insert(
                self.key,
                index,
                Arc::clone(&entry),
            );
            entry
        };
        self.slots[index % 2] = Some((index, entry));
        Ok(())
    }
    fn slot(&self, index: usize) -> &[f32] {
        &self.slots[index % 2]
            .as_ref()
            .expect("segment was loaded")
            .1
    }
    /// The lag that slides segment `segment` onto the tail of its predecessor, zero for
    /// the first segment and for one whose predecessor lies wholly before the clip.
    fn lag(&mut self, segment: usize, cursor: &mut Cursor<'_>) -> Result<i64, Error> {
        if segment == 0 || segment * segment_frames(self.rate) <= self.first_frame {
            return Ok(0);
        }
        if let Some((held, lag)) = self.lag
            && held == segment
        {
            return Ok(lag);
        }
        self.load(segment - 1, cursor)?;
        let lag = align(
            self.slot(segment - 1),
            self.slot(segment),
            self.channels,
            hop(self.rate),
            segment_frames(self.rate),
        );
        self.lag = Some((segment, lag));
        Ok(lag)
    }
    /// Append output frame `frame` (global frame index) to `samples`.
    pub(crate) fn frame(
        &mut self,
        frame: usize,
        cursor: &mut Cursor<'_>,
        samples: &mut Vec<f32>,
    ) -> Result<(), Error> {
        let channels = self.channels;
        let span = segment_frames(self.rate);
        let hop = hop(self.rate);
        let pre = preroll(self.rate);
        let (segment, offset) = (frame / span, frame % span);
        self.load(segment, cursor)?;
        let lag = self.lag(segment, cursor)?;
        samples.clear();
        let current = self.slot(segment);
        if lag == 0 {
            let at = (pre + offset) * channels;
            samples.extend_from_slice(&current[at..at + channels]);
        } else {
            // The slide fades out linearly across the segment: position
            // `offset + lag * (span - offset) / span`, read with a cubic.
            let numerator = i128::from(lag) * (span - offset) as i128;
            let whole = numerator.div_euclid(span as i128) as i64;
            let fraction = numerator.rem_euclid(span as i128) as f32 / span as f32;
            let base = (pre + offset) as i64 + whole;
            for channel in 0..channels {
                let tap = |k: i64| {
                    let frame = usize::try_from(base + k)
                        .unwrap_or(0)
                        .min(current.len() / channels - 1);
                    current[frame * channels + channel]
                };
                samples.push(if fraction == 0.0 {
                    tap(0)
                } else {
                    cubic([tap(-1), tap(0), tap(1), tap(2)], fraction)
                });
            }
        }
        // Blend with the predecessor's independent reconstruction of this hop, unless that
        // segment lies wholly before the clip starts: it would only add a spurious fade.
        if segment > 0 && offset < hop && segment * span > self.first_frame {
            self.load(segment - 1, cursor)?;
            let previous = self.slot(segment - 1);
            let weight = offset as f32 / hop as f32;
            for (channel, sample) in samples.iter_mut().enumerate() {
                *sample = previous[(pre + span + offset) * channels + channel] * (1.0 - weight)
                    + *sample * weight;
            }
        }
        Ok(())
    }
}

/// Catmull-Rom interpolation between `y[1]` and `y[2]` at `t` in `[0, 1)`.
fn cubic(y: [f32; 4], t: f32) -> f32 {
    let [a, b, c, d] = y;
    b + 0.5 * t * (c - a + t * (2.0 * a - 5.0 * b + 4.0 * c - d + t * (3.0 * (b - c) + d - a)))
}

/// Frames of lead-in kept before each segment (the largest slide plus the cubic's reach),
/// all inside the warm-up so they are fully reconstructed.
pub(crate) fn preroll(rate: u32) -> usize {
    (rate as usize / 200).max(1) + 2
}

/// How far a segment must slide to line up with its predecessor's tail: the lag within
/// the search range at which the two reconstructions of the shared hop correlate best,
/// the smallest such lag when several (periodic material) do.
fn align(previous: &[f32], current: &[f32], channels: usize, hop: usize, span: usize) -> i64 {
    let pre =
        (previous.len() / channels - (span + hop)).min(current.len() / channels - (span + hop));
    let search = i64::try_from(pre).expect("small") - 2;
    let tail =
        |i: usize, channel: usize| f64::from(previous[(pre + span + i) * channels + channel]);
    let reference: f64 = (0..hop)
        .flat_map(|i| (0..channels).map(move |c| (i, c)))
        .map(|(i, c)| tail(i, c).powi(2))
        .sum();
    if reference <= SILENCE {
        return 0;
    }
    let scores: Vec<f64> = (-search..=search)
        .map(|lag| {
            let (mut dot, mut energy) = (0.0, 0.0);
            for i in 0..hop {
                let at = usize::try_from((pre + i) as i64 + lag).expect("inside the lead-in");
                for channel in 0..channels {
                    let candidate = f64::from(current[at * channels + channel]);
                    dot += tail(i, channel) * candidate;
                    energy += candidate * candidate;
                }
            }
            if energy <= SILENCE {
                -1.0
            } else {
                dot / (energy * reference).sqrt()
            }
        })
        .collect();
    let best = scores.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    (-search..=search)
        .zip(&scores)
        .filter(|(_, score)| **score >= best - 0.02)
        .map(|(lag, _)| lag)
        .min_by_key(|lag| (lag.abs(), *lag))
        .unwrap_or(0)
}

/// Render one segment: a short lead-in before it (for the slide onto its predecessor),
/// the segment, and the following hop used to blend the next reset.
pub(crate) fn render(
    clip: &Clip,
    rate: u32,
    channels: usize,
    segment: usize,
    cursor: &mut Cursor<'_>,
) -> Result<Vec<f32>, Error> {
    let hop = hop(rate);
    let search = (rate as usize / 200).max(1);
    let ticks = i128::from(TICKS_PER_SECOND);
    let base = (segment * HOPS) as i128;
    let reverse = clip.time_map.reverse;
    // Source frames the clip may read: the first one containing `source_offset` through
    // the last one touching the end of the source span (the mixer's varispeed path
    // clamps to the same frames).
    let first = i128::from(clip.source_offset.0) * i128::from(rate) / ticks;
    let length = source_span(clip);
    let end = ((i128::from(clip.source_offset.0) + i128::from(length)) * i128::from(rate) + ticks
        - 1)
        / ticks;
    let width = 2 * hop + 2 * search;
    let pre = preroll(rate);
    let mut out = vec![0.0; (pre + (HOPS + 1) * hop) * channels];
    // Two grain buffers alternate as "previous" and "current"; no per-grain allocation.
    let mut region = vec![0.0; width * channels];
    let mut grains = [vec![0.0; 2 * hop * channels], vec![0.0; 2 * hop * channels]];
    let mut have_previous = false;
    for (n, grain) in (base - WARM..=base + HOPS as i128).enumerate() {
        let frame = grain * hop as i128;
        let local = frame * ticks - i128::from(clip.range.start.0) * i128::from(rate);
        let time = Time(
            i64::try_from(local.div_euclid(i128::from(rate)))
                .map_err(|e| Error::Invalid(e.to_string()))?,
        );
        let mut position = i128::from(clip.source_time(time).0) * i128::from(rate);
        if reverse {
            position -= ticks - i128::from(rate);
        }
        let anchor = position.div_euclid(ticks);
        // Fetch the candidate region in ascending source order (within one grain a reverse
        // clip then never walks backwards through prepared blocks, which would force the
        // sequential resampler fallback to restart when a block has been evicted) and store
        // it in playback order, so the same overlap/search code also handles reverse clips.
        let lowest = if reverse {
            anchor - (width - 1 - search) as i128
        } else {
            anchor - search as i128
        };
        for k in 0..width {
            let dst = if reverse { width - 1 - k } else { k };
            let dst = &mut region[dst * channels..(dst + 1) * channels];
            let position = lowest + k as i128;
            let sample = if position >= first && position < end {
                usize::try_from(position)
                    .ok()
                    .map(|index| cursor.frame(index))
                    .transpose()?
                    .flatten()
            } else {
                None
            };
            match sample {
                Some(src) => dst.copy_from_slice(src),
                None => dst.fill(0.0),
            }
        }
        let (previous, current) = {
            let [a, b] = &mut grains;
            if n % 2 == 0 { (&*b, a) } else { (&*a, b) }
        };
        let offset = if have_previous {
            best_overlap(&previous[hop * channels..], &region, channels, hop, search)
        } else {
            search
        };
        current.copy_from_slice(&region[offset * channels..(offset + 2 * hop) * channels]);
        for i in 0..2 * hop {
            let output = frame - base * hop as i128 + i as i128 + pre as i128;
            if let Ok(output) = usize::try_from(output)
                && output < pre + (HOPS + 1) * hop
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
        have_previous = true;
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
        // Silence has no waveform to match. A silent reference scores every candidate
        // alike, so the nominal anchor wins. A silent candidate against a real reference
        // scores the worst possible correlation, so it never beats a genuine match
        // (including an anti-correlated one) and ties resolve to the nominal anchor.
        let score = if reference <= SILENCE {
            0.0
        } else if energy <= SILENCE {
            -1.0
        } else {
            dot / (energy * reference).sqrt()
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
    fn eviction_is_lru_and_oversized_segments_leave_neighbours_alone() {
        let mut cache = Cache::default();
        let entry = Arc::new(vec![0.0; CACHE_BYTES / 8]);
        cache.insert(1, 0, Arc::clone(&entry));
        cache.insert(1, 1, Arc::clone(&entry));
        assert!(cache.get(1, 0).is_some()); // refresh 0: segment 1 is now the oldest
        cache.insert(1, 2, Arc::clone(&entry));
        assert!(cache.bytes <= CACHE_BYTES);
        assert!(cache.get(1, 1).is_none());
        assert!(cache.get(1, 0).is_some() && cache.get(1, 2).is_some());
        let large = Arc::new(vec![0.0; CACHE_BYTES / 4 + 1]);
        cache.insert(1, 3, large);
        assert!(cache.get(1, 3).is_none());
        // The oversized entry did not flush the rest of the cache.
        assert!(cache.get(1, 0).is_some() && cache.get(1, 2).is_some());
    }

    #[test]
    fn silence_never_beats_a_real_match_and_ties_keep_the_nominal_anchor() {
        let (hop, search, channels) = (16, 4, 1);
        let reference = vec![1.0_f32; hop];
        // Every candidate anti-correlated except a silent one: the silent one must lose.
        let mut region = vec![-1.0_f32; 2 * hop + 2 * search];
        for v in &mut region[..=hop] {
            *v = 0.0;
        }
        assert_ne!(best_overlap(&reference, &region, channels, hop, search), 0);
        // All silent: nominal.
        let region = vec![0.0_f32; 2 * hop + 2 * search];
        assert_eq!(
            best_overlap(&reference, &region, channels, hop, search),
            search
        );
        // Silent reference: nominal.
        let region = vec![0.5_f32; 2 * hop + 2 * search];
        assert_eq!(
            best_overlap(&vec![0.0; hop], &region, channels, hop, search),
            search
        );
    }
}
