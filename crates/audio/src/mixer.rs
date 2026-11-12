use crate::{
    AudioSource, Error, Pcm,
    source::{MAX_CHANNELS, MAX_INTERLEAVED_SAMPLES, MAX_SAMPLE_RATE, sample_bounds},
};
use animathio_core::{
    AssetId, Clip, ClipKind, Project, TICKS_PER_SECOND, Target, Time, TimeRange, Transition,
    TransitionKind, equal_power,
};
use rubato::{
    FftFixedInOut, Resampler, SincFixedIn, SincFixedOut, SincInterpolationParameters,
    SincInterpolationType, WindowFunction,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// FFT resampling is used only while both FFT lengths stay at or below this bound.
const MAX_FFT_FRAMES: usize = 16_384;
/// Default chunk size (in frames) for the bounded windowed-sinc fallback.
const SINC_CHUNK_FRAMES: usize = 4096;
/// Windowed-sinc filter length used by the fallback resampler.
const SINC_LEN: usize = 256;
/// Largest input chunk the sinc fallback accepts before rejecting the ratio.
const MAX_SINC_INPUT_FRAMES: usize = MAX_SAMPLE_RATE as usize + SINC_LEN + 1;

/// Engine-owned native decoded resources, keyed by the project's asset identities.
#[derive(Clone, Default)]
pub struct AudioPool {
    pub(crate) sources: BTreeMap<AssetId, Arc<Pcm>>,
}
impl AudioPool {
    /// Decode once, outside the real-time callback. Replacement is explicit by identity.
    pub fn insert(&mut self, id: AssetId, source: &impl AudioSource) -> Result<(), Error> {
        self.sources.insert(id, Arc::new(source.decode_all()?));
        Ok(())
    }
    /// Number of live native decoded resources.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sources.len()
    }
    /// Whether a decoded resource is present for this asset.
    #[must_use]
    pub fn contains(&self, id: &AssetId) -> bool {
        self.sources.contains_key(id)
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
        if rate == 0 || rate > MAX_SAMPLE_RATE || channels == 0 || channels > MAX_CHANNELS {
            return Err(Error::Invalid(
                "output rate must be within 1..=768000 Hz and channels within 1..=32".into(),
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
        let any_solo = project.tracks().iter().any(animathio_core::Track::is_solo);
        for track in project.tracks() {
            if track.gain() == 0.0 || track.is_muted() || (any_solo && !track.is_solo()) {
                continue;
            }
            for clip in track.clips().values() {
                let ClipKind::Audio(id) = &clip.kind else {
                    continue;
                };
                // Cross-dissolves extend both clips across the cut with equal-power
                // gains; other transition kinds keep a hard audio cut.
                let span = track.clip_span(clip);
                let (fade_in, fade_out) = (dissolve(span.incoming), dissolve(span.outgoing));
                let play = TimeRange {
                    start: fade_in.map_or(clip.range.start, |t| t.window(clip.range.start).start),
                    end: fade_out.map_or(clip.range.end, |t| t.window(clip.range.end).end),
                };
                if play.end <= range.start || play.start >= range.end {
                    continue;
                }
                let source = self
                    .sources
                    .get(id)
                    .ok_or_else(|| Error::MissingAsset(id.as_str().into()))?;
                let (clip_start, clip_end) = sample_bounds(play, self.rate)?;
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
                    let now = Time(
                        i64::try_from(timeline / i128::from(self.rate))
                            .map_err(|e| Error::Invalid(e.to_string()))?,
                    );
                    let gain = param(clip, "gain", time, 1.0)
                        * track.gain()
                        * crossfade(fade_in, clip.range.start, now, |(_, gain)| gain)
                        * crossfade(fade_out, clip.range.end, now, |(gain, _)| gain);
                    if !gain.is_finite() || gain < 0.0 {
                        return Err(Error::Invalid("nonfinite/negative gain".into()));
                    }
                    let ticks = i128::from(TICKS_PER_SECOND);
                    let source_ticks =
                        local + i128::from(clip.source_offset.0) * i128::from(self.rate);
                    if source_ticks < 0 {
                        // Before the source start (a dissolve without a handle): silence.
                        continue;
                    }
                    let source_frame = usize::try_from((source_ticks + ticks - 1) / ticks)
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
fn dissolve(transition: Option<&Transition>) -> Option<&Transition> {
    transition.filter(|t| t.kind == TransitionKind::CrossDissolve)
}
/// Equal-power gain for one side of a dissolve at `cut`, or 1 outside its window.
fn crossfade(
    transition: Option<&Transition>,
    cut: Time,
    now: Time,
    side: impl Fn((f64, f64)) -> f64,
) -> f64 {
    transition.map_or(1.0, |t| {
        let window = t.window(cut);
        if now < window.start || now >= window.end {
            1.0
        } else {
            side(equal_power(t.progress(window, now)))
        }
    })
}
fn param(clip: &Clip, name: &str, time: Time, fallback: f64) -> f64 {
    clip.animations
        .iter()
        .find(|a| matches!(&a.target, Target::Param(n) if n == name))
        .and_then(|a| a.keys.sample(time))
        .and_then(|v| v.as_number())
        .unwrap_or_else(|| {
            clip.params
                .get(name)
                .and_then(animathio_core::ParamValue::as_number)
                .unwrap_or(fallback)
        })
}

#[allow(clippy::cast_precision_loss)] // Channel count becomes an amplitude divisor only.
fn prepare(pcm: &Pcm, rate: u32, channels: u16) -> Result<Vec<f32>, Error> {
    let native_channels = usize::from(pcm.channels());
    let channels = usize::from(channels);
    if channels != native_channels && channels != 1 && native_channels != 1 {
        return Err(Error::Unsupported(format!(
            "cannot map {native_channels}-channel audio to {channels}-channel output; only equal layouts or mono conversion are supported"
        )));
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
        let wanted = output_frames(pcm.frames(), pcm.rate(), rate)?;
        let (mut resampler, delay) = build_resampler(pcm.rate(), rate, native_channels)?;
        let target = wanted
            .checked_add(delay)
            .ok_or_else(|| Error::Invalid("prepared audio too large".into()))?;
        if target
            .checked_mul(channels)
            .is_none_or(|total| total > MAX_INTERLEAVED_SAMPLES)
        {
            return Err(Error::Invalid("prepared audio too large".into()));
        }
        let mut result = vec![Vec::new(); native_channels];
        let mut offset = 0;
        while result[0].len() < target {
            let chunk = resampler.input_frames_next();
            let mut input = vec![vec![0.0; chunk]; native_channels];
            let count = pcm.frames().saturating_sub(offset).min(chunk);
            for (channel, input) in input.iter_mut().enumerate() {
                if count != 0 {
                    input[..count].copy_from_slice(&planar[channel][offset..offset + count]);
                }
            }
            let block = resampler.process(&input)?;
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

enum PreparedResampler {
    Fft(FftFixedInOut<f32>),
    SincIn(SincFixedIn<f32>),
    SincOut(SincFixedOut<f32>),
}
impl PreparedResampler {
    fn input_frames_next(&self) -> usize {
        match self {
            Self::Fft(resampler) => resampler.input_frames_next(),
            Self::SincIn(resampler) => resampler.input_frames_next(),
            Self::SincOut(resampler) => resampler.input_frames_next(),
        }
    }
    fn process(&mut self, input: &[Vec<f32>]) -> Result<Vec<Vec<f32>>, Error> {
        match self {
            Self::Fft(resampler) => resampler
                .process(input, None)
                .map_err(|e| Error::Decode(e.to_string())),
            Self::SincIn(resampler) => resampler
                .process(input, None)
                .map_err(|e| Error::Decode(e.to_string())),
            Self::SincOut(resampler) => resampler
                .process(input, None)
                .map_err(|e| Error::Decode(e.to_string())),
        }
    }
}

fn build_resampler(
    input_rate: u32,
    output_rate: u32,
    channels: usize,
) -> Result<(PreparedResampler, usize), Error> {
    let mut a = input_rate;
    let mut b = output_rate;
    while b != 0 {
        (a, b) = (b, a % b);
    }
    let gcd = a;
    let period = (input_rate / gcd) as usize;
    let chunk_in = 1024_usize.div_ceil(2 * period) * 2 * period;
    let fft_chunks = chunk_in.div_ceil(period);
    let fft_in = fft_chunks * period;
    let fft_out = fft_chunks * (output_rate / gcd) as usize;
    if fft_in <= MAX_FFT_FRAMES && fft_out <= MAX_FFT_FRAMES {
        let resampler = FftFixedInOut::<f32>::new(
            input_rate as usize,
            output_rate as usize,
            chunk_in,
            channels,
        )
        .map_err(|e| Error::Decode(e.to_string()))?;
        let delay = resampler.output_delay();
        Ok((PreparedResampler::Fft(resampler), delay))
    } else if output_rate < input_rate {
        let min_chunk = input_rate.div_ceil(output_rate) as usize + SINC_LEN + 1;
        let chunk = SINC_CHUNK_FRAMES.max(min_chunk);
        if chunk > MAX_SINC_INPUT_FRAMES {
            return Err(Error::Unsupported(
                "sample-rate ratio too extreme for resampling".into(),
            ));
        }
        let resampler = SincFixedIn::<f32>::new(
            f64::from(output_rate) / f64::from(input_rate),
            1.0,
            sinc_params(),
            chunk,
            channels,
        )
        .map_err(|e| Error::Decode(e.to_string()))?;
        let delay = resampler.output_delay();
        Ok((PreparedResampler::SincIn(resampler), delay))
    } else {
        let resampler = SincFixedOut::<f32>::new(
            f64::from(output_rate) / f64::from(input_rate),
            1.0,
            sinc_params(),
            SINC_CHUNK_FRAMES,
            channels,
        )
        .map_err(|e| Error::Decode(e.to_string()))?;
        let delay = resampler.output_delay();
        Ok((PreparedResampler::SincOut(resampler), delay))
    }
}

fn sinc_params() -> SincInterpolationParameters {
    SincInterpolationParameters {
        sinc_len: SINC_LEN,
        f_cutoff: 0.95,
        oversampling_factor: 256,
        interpolation: SincInterpolationType::Linear,
        window: WindowFunction::BlackmanHarris2,
    }
}

fn output_frames(input_frames: usize, input_rate: u32, output_rate: u32) -> Result<usize, Error> {
    usize::try_from(
        (input_frames as u128 * u128::from(output_rate)).div_ceil(u128::from(input_rate)),
    )
    .map_err(|e| Error::Invalid(e.to_string()))
}
