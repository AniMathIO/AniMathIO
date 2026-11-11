use crate::{AudioSource, Error, Pcm, source::sample_bounds};
use animathio_core::{AssetId, Clip, ClipKind, Project, TICKS_PER_SECOND, Target, Time, TimeRange};
use rubato::{FftFixedInOut, Resampler};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// Engine-owned native decoded resources, keyed by the project's asset identities.
#[derive(Default)]
pub struct AudioPool {
    sources: BTreeMap<AssetId, Pcm>,
}
impl AudioPool {
    /// Decode once, outside the real-time callback. Replacement is explicit by identity.
    pub fn insert(&mut self, id: AssetId, source: &impl AudioSource) -> Result<(), Error> {
        self.sources.insert(id, source.decode_all()?);
        Ok(())
    }
    /// Number of live native decoded resources.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sources.len()
    }
    /// Whether the pool is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }
    /// Drop resources no longer referenced by any audio clip. Existing mixer snapshots
    /// retain their immutable buffers until their playback/export job finishes.
    pub fn collect(&mut self, project: &Project) {
        let live: BTreeSet<_> = project
            .tracks()
            .iter()
            .flat_map(|t| t.clips().values())
            .filter_map(|c| match &c.kind {
                ClipKind::Audio(id) => Some(id),
                _ => None,
            })
            .collect();
        self.sources.retain(|id, _| live.contains(id));
    }
    /// Resample whole assets once and compensate Rubato's startup delay. Whole-asset
    /// preparation gives arbitrary mix ranges the same filter history and sample phase.
    pub fn prepare(&self, rate: u32, channels: u16) -> Result<Mixer, Error> {
        if rate == 0 || i64::from(rate) > TICKS_PER_SECOND || channels == 0 {
            return Err(Error::Invalid(
                "output rate must be within the tick resolution and channels nonzero".into(),
            ));
        }
        let sources = self
            .sources
            .iter()
            .map(|(id, pcm)| Ok((id.clone(), Arc::new(prepare(pcm, rate, channels)?))))
            .collect::<Result<_, Error>>()?;
        Ok(Mixer {
            sources,
            rate,
            channels,
        })
    }
    /// Convenience offline entry point; reuse `prepare` for repeated calls.
    pub fn mix(
        &self,
        project: &Project,
        range: TimeRange,
        rate: u32,
        channels: u16,
    ) -> Result<Vec<f32>, Error> {
        self.prepare(rate, channels)?.mix(project, range)
    }
}

/// Immutable prepared resources shared by preview and export. Mixing is pure:
/// its only inputs are project data, an explicit resource snapshot, and a range.
#[derive(Clone)]
pub struct Mixer {
    sources: BTreeMap<AssetId, Arc<Vec<f32>>>,
    rate: u32,
    channels: u16,
}
impl Mixer {
    /// Output frames per second.
    #[must_use]
    pub fn rate(&self) -> u32 {
        self.rate
    }
    /// Output channel count.
    #[must_use]
    pub fn channels(&self) -> u16 {
        self.channels
    }
    /// Mix every audio clip on the global output sample lattice in `[start,end)`.
    /// Boundaries use `ceil(ticks * rate / ticks_per_second)`, so concatenating adjacent
    /// ranges is exactly identical, even for boundaries between samples. Clip `gain`
    /// defaults to 1 and `muted` to 0; both accept ordinary clip-local keyframes.
    /// Track gain is linear. Output is unclipped interleaved f32; devices saturate it.
    #[allow(clippy::cast_possible_truncation)] // Amplitudes, never timeline positions.
    pub fn mix(&self, project: &Project, range: TimeRange) -> Result<Vec<f32>, Error> {
        let (first, end) = sample_bounds(range, self.rate)?;
        let channels = usize::from(self.channels);
        let len = (end - first)
            .checked_mul(channels)
            .ok_or_else(|| Error::Invalid("mix too large".into()))?;
        let mut output = vec![0.0_f32; len];
        for track in project.tracks() {
            if track.gain() == 0.0 {
                continue;
            }
            for clip in track.clips().values() {
                let ClipKind::Audio(id) = &clip.kind else {
                    continue;
                };
                if clip.range.end <= range.start || clip.range.start >= range.end {
                    continue;
                }
                let source = self
                    .sources
                    .get(id)
                    .ok_or_else(|| Error::MissingAsset(id.as_str().into()))?;
                let (clip_start, clip_end) = sample_bounds(clip.range, self.rate)?;
                for frame in first.max(clip_start)..end.min(clip_end) {
                    let timeline = frame as i128 * i128::from(TICKS_PER_SECOND);
                    let local = timeline - i128::from(clip.range.start.0) * i128::from(self.rate);
                    let time = Time(
                        i64::try_from(local / i128::from(self.rate))
                            .map_err(|e| Error::Invalid(e.to_string()))?,
                    );
                    if param(clip, "muted", time, 0.0) != 0.0 {
                        continue;
                    }
                    let gain = param(clip, "gain", time, 1.0) * track.gain();
                    if !gain.is_finite() || gain < 0.0 {
                        return Err(Error::Invalid("nonfinite/negative gain".into()));
                    }
                    let ticks = i128::from(TICKS_PER_SECOND);
                    let source_frame = usize::try_from(
                        (local + i128::from(clip.source_offset.0) * i128::from(self.rate) + ticks
                            - 1)
                            / ticks,
                    )
                    .map_err(|e| Error::Invalid(e.to_string()))?;
                    if source_frame >= source.len() / channels {
                        continue;
                    }
                    for channel in 0..channels {
                        output[(frame - first) * channels + channel] +=
                            (f64::from(source[source_frame * channels + channel]) * gain) as f32;
                    }
                }
            }
        }
        if output.iter().any(|x| !x.is_finite()) {
            return Err(Error::Invalid("mix amplitude overflow".into()));
        }
        Ok(output)
    }
}
fn param(clip: &Clip, name: &str, time: Time, fallback: f64) -> f64 {
    clip.animations
        .iter()
        .find(|a| matches!(&a.target, Target::Param(n) if n == name))
        .and_then(|a| a.keys.sample(time))
        .unwrap_or_else(|| clip.params.get(name).copied().unwrap_or(fallback))
}

#[allow(clippy::cast_precision_loss)] // Channel count becomes an amplitude divisor only.
fn prepare(pcm: &Pcm, rate: u32, channels: u16) -> Result<Vec<f32>, Error> {
    let native_channels = usize::from(pcm.channels());
    let channels = usize::from(channels);
    if channels != native_channels && channels != 1 && native_channels != 1 {
        return Err(Error::Unsupported(
            "channel mapping supports equal layouts or mono conversion".into(),
        ));
    }
    let mut planar: Vec<Vec<f32>> = (0..native_channels)
        .map(|channel| {
            pcm.samples()
                .iter()
                .skip(channel)
                .step_by(native_channels)
                .copied()
                .collect()
        })
        .collect();
    if rate != pcm.rate() && pcm.frames() != 0 {
        // Rubato 0.16 centres its sinc at floor(input_fft_size / 2).
        // An even number of rate periods makes both FFT sizes even, so its
        // reported integer output_delay is the exact filter centre.
        let mut a = pcm.rate();
        let mut b = rate;
        while b != 0 {
            (a, b) = (b, a % b);
        }
        let period = (pcm.rate() / a) as usize;
        let chunk_size = 1024_usize.div_ceil(2 * period) * 2 * period;
        let mut resampler = FftFixedInOut::<f32>::new(
            pcm.rate() as usize,
            rate as usize,
            chunk_size,
            native_channels,
        )
        .map_err(|e| Error::Decode(e.to_string()))?;
        let wanted = usize::try_from(
            (pcm.frames() as u128 * u128::from(rate)).div_ceil(u128::from(pcm.rate())),
        )
        .map_err(|e| Error::Invalid(e.to_string()))?;
        let delay = resampler.output_delay();
        let chunk = resampler.input_frames_next();
        let mut result = vec![Vec::new(); native_channels];
        let mut offset = 0;
        while result[0].len() < wanted + delay {
            let mut input = vec![vec![0.0; chunk]; native_channels];
            let count = pcm.frames().saturating_sub(offset).min(chunk);
            for (channel, input) in input.iter_mut().enumerate() {
                if count != 0 {
                    input[..count].copy_from_slice(&planar[channel][offset..offset + count]);
                }
            }
            let block = resampler
                .process(&input, None)
                .map_err(|e| Error::Decode(e.to_string()))?;
            for (out, data) in result.iter_mut().zip(block) {
                out.extend(data);
            }
            offset += chunk;
        }
        planar = result
            .into_iter()
            .map(|data| data[delay..delay + wanted].to_vec())
            .collect();
    }
    let mut output = Vec::with_capacity(planar[0].len() * channels);
    for frame in 0..planar[0].len() {
        for channel in 0..channels {
            output.push(if native_channels == channels {
                planar[channel][frame]
            } else if native_channels == 1 {
                planar[0][frame]
            } else {
                planar.iter().map(|c| c[frame]).sum::<f32>() / native_channels as f32
            });
        }
    }
    Ok(output)
}
