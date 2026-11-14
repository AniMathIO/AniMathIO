//! Lazy, bounded preparation of assets at the mixer's output rate and channel count.
//!
//! Nothing here holds a whole asset. An asset is cut into fixed *blocks* of output frames; a
//! block is decoded (through the asset's seekable [`PcmReader`]), resampled and channel-mapped
//! on first use and kept in an LRU cache with a byte budget shared by every asset of a mixer.
//!
//! The blocks are bit-identical to what resampling the whole asset front to back produces:
//!
//! * Equal rates need no resampling, so a block is a straight copy of native frames.
//! * The FFT resampler (`FftFixedInOut`) lets output chunk `k` depend only on input chunks `k`
//!   and `k - 1` (overlap-add of one chunk). A block therefore starts a fresh resampler one
//!   chunk early and discards that chunk's output, which reproduces the sequential result
//!   exactly while allowing a seek anywhere.
//! * The windowed-sinc fallback (extreme rate ratios only) carries a fractional phase from the
//!   start of the stream, so it runs sequentially and replays from the start on a backward
//!   seek. It is correct and bounded in memory but a backward seek costs a full pass.
//!
//! Rubato's start-up delay is removed by addressing output frame `t` as raw frame
//! `t + delay`; the tail past the asset's end is the resampler's response to zero padding,
//! exactly as in the whole-asset implementation.

#![allow(clippy::cast_possible_truncation)] // Frame offsets within one block, never positions.
use crate::{
    Error, PcmReader, StreamInfo,
    layout::ChannelMatrix,
    source::{MAX_INTERLEAVED_SAMPLES, MAX_SAMPLE_RATE},
};
use rubato::{
    FftFixedInOut, Resampler, SincFixedIn, SincFixedOut, SincInterpolationParameters,
    SincInterpolationType, WindowFunction,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

/// FFT resampling is used only while both FFT lengths stay at or below this bound.
const MAX_FFT_FRAMES: usize = 16_384;
/// Default chunk size (in frames) for the bounded windowed-sinc fallback.
const SINC_CHUNK_FRAMES: usize = 4096;
/// Windowed-sinc filter length used by the fallback resampler.
const SINC_LEN: usize = 256;
/// Largest input chunk the sinc fallback accepts before rejecting the ratio.
const MAX_SINC_INPUT_FRAMES: usize = MAX_SAMPLE_RATE as usize + SINC_LEN + 1;
/// Native frames fetched per read when the sequential resampler pulls its input.
const FEED_FRAMES: usize = 1 << 16;

/// Tuning for [`AudioPool::prepare_with`](crate::AudioPool::prepare_with).
#[derive(Clone, Copy, Debug)]
pub struct PrepareOptions {
    /// Approximate output frames per cached block (rounded to the resampler's chunk size).
    pub block_frames: usize,
    /// Upper bound, in bytes, of prepared blocks kept by one mixer. A single block larger than
    /// the budget is still held while in use.
    pub cache_bytes: usize,
}
impl Default for PrepareOptions {
    fn default() -> Self {
        Self {
            block_frames: 1 << 17,
            cache_bytes: 256 * 1024 * 1024,
        }
    }
}

/// A pooled asset: its stream shape and a shared seekable reader.
pub(crate) struct Asset {
    pub(crate) info: StreamInfo,
    reader: Mutex<Box<dyn PcmReader>>,
}
impl Asset {
    pub(crate) fn new(reader: Box<dyn PcmReader>) -> Self {
        Self {
            info: reader.info().clone(),
            reader: Mutex::new(reader),
        }
    }
    /// Append native frames `[start, start + frames)` to `out` (fewer at the end of the asset).
    pub(crate) fn read(
        &self,
        start: u64,
        frames: usize,
        out: &mut Vec<f32>,
    ) -> Result<usize, Error> {
        self.reader
            .lock()
            .expect("audio reader lock")
            .read_at(start, frames, out)
    }
}

struct Block {
    samples: Vec<f32>,
}

/// LRU cache of prepared blocks keyed by `(source id, block index)`.
pub(crate) struct BlockCache {
    entries: HashMap<(u64, usize), (u64, Arc<Block>)>,
    clock: u64,
    bytes: usize,
    budget: usize,
}
impl BlockCache {
    pub(crate) fn new(budget: usize) -> Self {
        Self {
            entries: HashMap::new(),
            clock: 0,
            bytes: 0,
            budget,
        }
    }
    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }
    fn get(&mut self, key: (u64, usize)) -> Option<Arc<Block>> {
        self.clock += 1;
        let clock = self.clock;
        self.entries.get_mut(&key).map(|(used, block)| {
            *used = clock;
            Arc::clone(block)
        })
    }
    fn insert(&mut self, key: (u64, usize), block: Arc<Block>) {
        self.clock += 1;
        self.bytes += block.samples.len() * 4;
        if let Some((_, old)) = self.entries.insert(key, (self.clock, block)) {
            self.bytes -= old.samples.len() * 4;
        }
        while self.bytes > self.budget && self.entries.len() > 1 {
            let oldest = self
                .entries
                .iter()
                .filter(|(k, _)| **k != key)
                .min_by_key(|(_, (used, _))| *used)
                .map(|(k, _)| *k);
            let Some(oldest) = oldest else { break };
            if let Some((_, old)) = self.entries.remove(&oldest) {
                self.bytes -= old.samples.len() * 4;
            }
        }
    }
}

enum Plan {
    /// Same rate: blocks are native frames, channel-mapped.
    Direct,
    /// FFT resampling with random access (one chunk of pre-roll per block).
    Fft {
        in_rate: u32,
        out_rate: u32,
        chunk_in: usize,
        chunk_out: usize,
        chunks_per_block: usize,
    },
    /// Sinc resampling; sequential with replay on a backward seek.
    Sequential(Box<Mutex<Option<Sequential>>>),
}

/// One asset prepared at the mixer's rate and channel count.
pub(crate) struct PreparedSource {
    asset: Arc<Asset>,
    cache: Arc<Mutex<BlockCache>>,
    id: u64,
    matrix: ChannelMatrix,
    plan: Plan,
    in_rate: u32,
    out_rate: u32,
    /// Output frames in the prepared asset.
    frames: usize,
    /// Rubato's start-up delay, in raw output frames.
    delay: usize,
    /// Raw output frames per block.
    block_frames: usize,
}
impl PreparedSource {
    pub(crate) fn new(
        asset: Arc<Asset>,
        cache: Arc<Mutex<BlockCache>>,
        id: u64,
        out_rate: u32,
        out_channels: u16,
        options: PrepareOptions,
    ) -> Result<Self, Error> {
        let info = &asset.info;
        let matrix = ChannelMatrix::new(&info.layout, out_channels)?;
        let block_target = options.block_frames.max(64);
        let native_frames = usize::try_from(info.frames)
            .map_err(|_| Error::Invalid("prepared audio too large".into()))?;
        let (plan, frames, delay, block_frames) = if out_rate == info.rate || native_frames == 0 {
            (Plan::Direct, native_frames, 0, block_target)
        } else {
            let wanted = output_frames(native_frames, info.rate, out_rate)?;
            let channels = usize::from(info.channels);
            match build_resampler(info.rate, out_rate, channels)? {
                (PreparedResampler::Fft(resampler), delay) => {
                    let chunk_out = resampler.output_frames_next();
                    let chunks_per_block = (block_target / chunk_out).max(1);
                    (
                        Plan::Fft {
                            in_rate: info.rate,
                            out_rate,
                            chunk_in: resampler.input_frames_next(),
                            chunk_out,
                            chunks_per_block,
                        },
                        wanted,
                        delay,
                        chunk_out * chunks_per_block,
                    )
                }
                (_, delay) => (
                    Plan::Sequential(Box::default()),
                    wanted,
                    delay,
                    block_target,
                ),
            }
        };
        if frames
            .checked_add(delay)
            .and_then(|n| n.checked_mul(usize::from(out_channels)))
            .is_none_or(|total| total > MAX_INTERLEAVED_SAMPLES)
        {
            return Err(Error::Invalid("prepared audio too large".into()));
        }
        Ok(Self {
            in_rate: info.rate,
            asset,
            cache,
            id,
            matrix,
            plan,
            out_rate,
            frames,
            delay,
            block_frames,
        })
    }

    /// A reader that keeps the block it last touched.
    pub(crate) fn cursor(&self) -> Cursor<'_> {
        Cursor {
            source: self,
            held: None,
        }
    }

    fn block(&self, index: usize) -> Result<Arc<Block>, Error> {
        if let Some(block) = self
            .cache
            .lock()
            .expect("prepared block cache lock")
            .get((self.id, index))
        {
            return Ok(block);
        }
        let block = self.compute(index)?;
        self.cache
            .lock()
            .expect("prepared block cache lock")
            .insert((self.id, index), Arc::clone(&block));
        Ok(block)
    }

    fn compute(&self, index: usize) -> Result<Arc<Block>, Error> {
        match &self.plan {
            Plan::Direct => self.compute_direct(index).map(Arc::new),
            Plan::Fft {
                in_rate,
                out_rate,
                chunk_in,
                chunk_out,
                chunks_per_block,
            } => self
                .compute_fft(
                    index,
                    *in_rate,
                    *out_rate,
                    *chunk_in,
                    *chunk_out,
                    *chunks_per_block,
                )
                .map(Arc::new),
            Plan::Sequential(state) => self.compute_sequential(state, index),
        }
    }

    /// Interleave-map a planar raw block into output channels.
    fn map_planar(&self, planar: &[Vec<f32>], frames: usize, out: &mut Vec<f32>) {
        let inputs = planar.len();
        let outputs = self.matrix.outputs();
        let mut frame = vec![0.0_f32; inputs];
        let mut mapped = vec![0.0_f32; outputs];
        for n in 0..frames {
            for (slot, channel) in frame.iter_mut().zip(planar) {
                *slot = channel[n];
            }
            self.matrix.apply(&frame, &mut mapped);
            out.extend_from_slice(&mapped);
        }
    }

    fn compute_direct(&self, index: usize) -> Result<Block, Error> {
        let inputs = usize::from(self.asset.info.channels);
        let outputs = self.matrix.outputs();
        let mut native = Vec::new();
        let got = self.asset.read(
            (index * self.block_frames) as u64,
            self.block_frames,
            &mut native,
        )?;
        let mut samples = Vec::with_capacity(self.block_frames * outputs);
        let mut mapped = vec![0.0_f32; outputs];
        for frame in native.chunks_exact(inputs).take(got) {
            self.matrix.apply(frame, &mut mapped);
            samples.extend_from_slice(&mapped);
        }
        samples.resize(self.block_frames * outputs, 0.0);
        Ok(Block { samples })
    }

    fn compute_fft(
        &self,
        index: usize,
        in_rate: u32,
        out_rate: u32,
        chunk_in: usize,
        chunk_out: usize,
        chunks_per_block: usize,
    ) -> Result<Block, Error> {
        let channels = usize::from(self.asset.info.channels);
        let mut resampler =
            FftFixedInOut::<f32>::new(in_rate as usize, out_rate as usize, chunk_in, channels)
                .map_err(|e| Error::Decode(e.to_string()))?;
        let first_chunk = index * chunks_per_block;
        let warm = usize::from(first_chunk > 0);
        let start_chunk = first_chunk - warm;
        let chunks = warm + chunks_per_block;
        let mut native = Vec::new();
        self.asset.read(
            (start_chunk * chunk_in) as u64,
            chunks * chunk_in,
            &mut native,
        )?;
        native.resize(chunks * chunk_in * channels, 0.0);
        let mut raw = vec![Vec::with_capacity(chunks_per_block * chunk_out); channels];
        let mut input = vec![vec![0.0_f32; chunk_in]; channels];
        for chunk in 0..chunks {
            for (channel, plane) in input.iter_mut().enumerate() {
                for (n, slot) in plane.iter_mut().enumerate() {
                    *slot = native[(chunk * chunk_in + n) * channels + channel];
                }
            }
            let output = resampler
                .process(&input, None)
                .map_err(|e| Error::Decode(e.to_string()))?;
            if chunk >= warm {
                for (plane, data) in raw.iter_mut().zip(output) {
                    plane.extend(data);
                }
            }
        }
        let mut samples = Vec::with_capacity(self.block_frames * self.matrix.outputs());
        self.map_planar(&raw, self.block_frames, &mut samples);
        Ok(Block { samples })
    }

    fn compute_sequential(
        &self,
        state: &Mutex<Option<Sequential>>,
        index: usize,
    ) -> Result<Arc<Block>, Error> {
        let mut guard = state.lock().expect("sequential resampler lock");
        if guard.as_ref().is_none_or(|s| s.next_block > index) {
            let channels = usize::from(self.asset.info.channels);
            let (resampler, _) = build_resampler(self.in_rate, self.out_rate, channels)?;
            *guard = Some(Sequential {
                resampler,
                next_block: 0,
                native_pos: 0,
                feed: Vec::new(),
                feed_start: 0,
                pending: vec![Vec::new(); channels],
            });
        }
        let seq = guard.as_mut().expect("sequential state was just created");
        let channels = usize::from(self.asset.info.channels);
        loop {
            while seq.pending[0].len() < self.block_frames {
                let chunk = seq.resampler.input_frames_next();
                let input = seq.take_input(&self.asset, chunk, channels)?;
                let output = seq.resampler.process(&input)?;
                for (plane, data) in seq.pending.iter_mut().zip(output) {
                    plane.extend(data);
                }
            }
            let mut block_planar = Vec::with_capacity(channels);
            for plane in &mut seq.pending {
                let rest = plane.split_off(self.block_frames);
                block_planar.push(std::mem::replace(plane, rest));
            }
            let mut samples = Vec::with_capacity(self.block_frames * self.matrix.outputs());
            self.map_planar(&block_planar, self.block_frames, &mut samples);
            let block = Arc::new(Block { samples });
            let current = seq.next_block;
            seq.next_block += 1;
            if current == index {
                return Ok(block);
            }
            self.cache
                .lock()
                .expect("prepared block cache lock")
                .insert((self.id, current), block);
        }
    }
}

/// Position of the sequential sinc resampler.
struct Sequential {
    resampler: PreparedResampler,
    next_block: usize,
    /// Native frame index of the next input chunk.
    native_pos: u64,
    /// Interleaved native frames starting at `feed_start`, read ahead of `native_pos`.
    feed: Vec<f32>,
    feed_start: u64,
    /// Raw planar output not yet emitted as a block.
    pending: Vec<Vec<f32>>,
}
impl Sequential {
    /// The next `frames` native frames, planar and zero padded past the end of the asset.
    fn take_input(
        &mut self,
        asset: &Asset,
        frames: usize,
        channels: usize,
    ) -> Result<Vec<Vec<f32>>, Error> {
        let mut planar = vec![vec![0.0_f32; frames]; channels];
        let mut filled = 0;
        while filled < frames {
            let offset = self.native_pos - self.feed_start;
            let have = (self.feed.len() / channels) as u64;
            if offset >= have {
                self.feed.clear();
                self.feed_start = self.native_pos;
                let got = asset.read(self.native_pos, FEED_FRAMES, &mut self.feed)?;
                if got == 0 {
                    break; // past the end: the rest stays zero
                }
                continue;
            }
            let offset = offset as usize;
            let take = (have as usize - offset).min(frames - filled);
            for n in 0..take {
                for (channel, plane) in planar.iter_mut().enumerate() {
                    plane[filled + n] = self.feed[(offset + n) * channels + channel];
                }
            }
            filled += take;
            self.native_pos += take as u64;
        }
        // Zero padding past the end still advances the stream position.
        self.native_pos += (frames - filled) as u64;
        Ok(planar)
    }
}

/// Cursor over a [`PreparedSource`] that keeps the current block alive.
pub(crate) struct Cursor<'a> {
    source: &'a PreparedSource,
    held: Option<(usize, Arc<Block>)>,
}
impl Cursor<'_> {
    /// Output frame `index` (all channels), or `None` past the end of the asset.
    pub(crate) fn frame(&mut self, index: usize) -> Result<Option<&[f32]>, Error> {
        if index >= self.source.frames {
            return Ok(None);
        }
        let raw = index + self.source.delay;
        let (block, offset) = (
            raw / self.source.block_frames,
            raw % self.source.block_frames,
        );
        if self.held.as_ref().is_none_or(|(held, _)| *held != block) {
            self.held = Some((block, self.source.block(block)?));
        }
        let channels = self.source.matrix.outputs();
        let (_, held) = self.held.as_ref().expect("block was just loaded");
        Ok(Some(
            &held.samples[offset * channels..(offset + 1) * channels],
        ))
    }
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
