#![allow(clippy::cast_possible_truncation)] // Amplitudes, never timeline positions.
use crate::{
    AudioSource, Error,
    prepared::{Asset, BlockCache, Cursor, PrepareOptions, PreparedSource},
    source::{MAX_CHANNELS, MAX_SAMPLE_RATE, StreamInfo, sample_bounds},
};
use animathio_core::{
    AssetId, AudioBlock, AudioEffectProcessor, Clip, ClipKind, EffectInstance, Project,
    TICKS_PER_SECOND, Target, Time, TimeRange, Track, Transition, TransitionKind, equal_power,
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    sync::{Arc, Mutex},
};

/// Engine-owned native audio resources, keyed by the project's asset identities. Each entry is
/// a seekable reader: streaming sources (such as `SymphoniaSource`) never hold the decoded
/// asset in memory, in-memory sources (such as `Pcm`) keep their samples.
#[derive(Clone, Default)]
pub struct AudioPool {
    pub(crate) sources: BTreeMap<AssetId, Arc<Asset>>,
}
impl AudioPool {
    /// Open the source for reading, outside the real-time callback. Replacement is explicit by
    /// identity. A streaming source is not decoded here; its reader fetches blocks on demand.
    pub fn insert(&mut self, id: AssetId, source: &impl AudioSource) -> Result<(), Error> {
        self.sources
            .insert(id, Arc::new(Asset::new(source.open_reader()?)));
        Ok(())
    }
    /// Number of live native audio resources.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sources.len()
    }
    /// Whether a resource is present for this asset.
    #[must_use]
    pub fn contains(&self, id: &AssetId) -> bool {
        self.sources.contains_key(id)
    }
    /// Native rate, channels and exact length of a pooled asset.
    #[must_use]
    pub fn info(&self, id: &AssetId) -> Option<StreamInfo> {
        self.sources.get(id).map(|asset| asset.info.clone())
    }
    /// Whether the pool is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }
    /// Drop resources no longer referenced by any audio clip. Existing mixer snapshots
    /// retain their readers until their playback/export job finishes.
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
    /// Plan the preparation of every asset at the output format. Decoding, resampling and
    /// channel mapping happen lazily, block by block, inside [`Mixer::mix`], bounded by a
    /// shared cache; whole-asset resampling semantics (Rubato's start-up delay compensated,
    /// identical results for any mix range) are preserved exactly.
    pub fn prepare(&self, rate: u32, channels: u16) -> Result<Mixer, Error> {
        self.prepare_with(rate, channels, PrepareOptions::default())
    }
    /// [`AudioPool::prepare`] with explicit block size and cache budget.
    pub fn prepare_with(
        &self,
        rate: u32,
        channels: u16,
        options: PrepareOptions,
    ) -> Result<Mixer, Error> {
        if rate == 0 || rate > MAX_SAMPLE_RATE || channels == 0 || channels > MAX_CHANNELS {
            return Err(Error::Invalid(
                "output rate must be within 1..=768000 Hz and channels within 1..=32".into(),
            ));
        }
        let blocks = Arc::new(Mutex::new(BlockCache::new(options.cache_bytes)));
        let sources = self
            .sources
            .iter()
            .enumerate()
            .map(|(n, (id, asset))| {
                Ok((
                    id.clone(),
                    Arc::new(PreparedSource::new(
                        Arc::clone(asset),
                        Arc::clone(&blocks),
                        n as u64,
                        rate,
                        channels,
                        options,
                    )?),
                ))
            })
            .collect::<Result<_, Error>>()?;
        Ok(Mixer {
            sources,
            blocks,
            rate,
            channels,
            effects: None,
            dry: false,
            lenient: false,
            warnings: EffectWarnings::default(),
            cache: Arc::default(),
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

/// Frames per effect block. Plugin state is defined on this global lattice, so the output of
/// a block never depends on how a caller happened to slice its `mix` ranges.
const EFFECT_BLOCK_FRAMES: usize = 2048;
/// Blocks per state segment. Within a segment a chain's state flows exactly from block to
/// block; each segment starts from the state reached by a bounded warm-up (see below), which
/// bounds the work a seek costs while keeping the result a pure function of the project.
const SEGMENT_BLOCKS: usize = 64;
/// Upper bound on the bytes of processed blocks the mixer keeps for reuse.
const EFFECT_CACHE_BYTES: usize = 96 * 1024 * 1024;

/// Collects warnings from a lenient [`Mixer`] (see [`Mixer::lenient`]). Cloning shares the
/// underlying list, so a UI can keep a clone and drain it while the mix worker fills it.
#[derive(Clone, Default)]
pub struct EffectWarnings(Arc<Mutex<Vec<String>>>);
impl EffectWarnings {
    fn push(&self, message: String) {
        let mut list = self.0.lock().expect("effect warnings lock");
        // Every block of a broken chain repeats the same message; keep one.
        if !list.contains(&message) && list.len() < 32 {
            list.push(message);
        }
    }
    /// Take and clear the collected warnings.
    ///
    /// # Panics
    /// If a mixer thread panicked while holding the sink's lock.
    #[must_use]
    pub fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().expect("effect warnings lock"))
    }
}

/// Immutable prepared resources shared by preview and export. Mixing is pure:
/// its only inputs are project data, an explicit resource snapshot, and a range.
///
/// Audio effects (clip and track chains) run here, in the mix worker or export thread and
/// never in the device callback. They need an [`AudioEffectProcessor`] set with
/// [`Mixer::with_effect_processor`]; a project without active effects never touches it. Effect
/// state is defined on a fixed block lattice with bounded warm-up, so the result for any frame
/// is a pure function of the project, whatever ranges were mixed before. A small cache of
/// processed blocks (shared between clones) makes sequential playback and export cost one
/// pass of each chain instead of one pass per requested range.
#[derive(Clone)]
pub struct Mixer {
    sources: BTreeMap<AssetId, Arc<PreparedSource>>,
    /// Prepared-block cache shared by every asset and every clone of this mixer.
    blocks: Arc<Mutex<BlockCache>>,
    rate: u32,
    channels: u16,
    effects: Option<Arc<dyn AudioEffectProcessor>>,
    /// Ignore every audio-effect chain (dry mix); see [`Mixer::dry`].
    dry: bool,
    /// Skip failing effects with a warning instead of failing the mix; see [`Mixer::lenient`].
    lenient: bool,
    warnings: EffectWarnings,
    cache: Arc<Mutex<EffectCache>>,
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
    /// Bytes of prepared audio currently cached (bounded by [`PrepareOptions::cache_bytes`]).
    ///
    /// # Panics
    /// If a mixer thread panicked while holding the cache lock.
    #[must_use]
    pub fn prepared_cache_bytes(&self) -> usize {
        self.blocks
            .lock()
            .expect("prepared block cache lock")
            .bytes()
    }
    /// Attach the plugin runner used for clip and track audio effects. Returns a mixer with an
    /// empty block cache.
    #[must_use]
    pub fn with_effect_processor(mut self, processor: Arc<dyn AudioEffectProcessor>) -> Self {
        self.effects = Some(processor);
        self.cache = Arc::default();
        self
    }
    /// A mixer for preview: an effect that cannot run (plugin missing, trapped, over its
    /// limits, or no processor attached) is skipped, so the audio passes through that effect
    /// unprocessed, and a warning is recorded in the sink instead of failing the mix.
    /// Blocks that skipped an effect are not cached. Without this (the default, used by
    /// export) such a failure is an error naming the effect.
    #[must_use]
    pub fn lenient(mut self, warnings: EffectWarnings) -> Self {
        self.lenient = true;
        self.warnings = warnings;
        self
    }
    /// The sink collecting this mixer's warnings (empty unless [`Mixer::lenient`]).
    #[must_use]
    pub fn warnings(&self) -> EffectWarnings {
        self.warnings.clone()
    }
    /// A mixer that ignores all audio-effect chains, for analyses that want the raw source
    /// signal (for example silence detection) and need no effect processor.
    #[must_use]
    pub fn dry(mut self) -> Self {
        self.dry = true;
        self
    }
    /// Mix every audio clip on the global output sample lattice in `[start,end)`.
    /// Boundaries use `ceil(ticks * rate / ticks_per_second)`, so concatenating adjacent
    /// ranges is exactly identical, even for boundaries between samples. Clip `gain`
    /// defaults to 1 and `muted` to 0; both accept ordinary clip-local keyframes.
    /// Track gain is linear. Output is unclipped interleaved f32; devices saturate it.
    ///
    /// Signal flow per clip: source, clip `gain`/`muted`, clip audio effects, dissolve
    /// fades; per track: sum of clips, track audio effects, track gain.
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
            let chain_active = !self.dry && track.audio_effects().iter().any(|e| !e.bypass);
            if chain_active && end > first {
                let processed = self.track_chain(track, first, end)?;
                for (out, sample) in output.iter_mut().zip(processed) {
                    *out += (f64::from(sample) * track.gain()) as f32;
                }
            } else {
                for clip in track.clips().values() {
                    self.add_clip(track, clip, track.gain(), first, end, &mut output, first)?;
                }
            }
        }
        if output.iter().any(|x| !x.is_finite()) {
            return Err(Error::Invalid("mix amplitude overflow".into()));
        }
        Ok(output)
    }

    /// The clip's dissolve-extended play window and its fade transitions.
    fn clip_window<'a>(
        track: &'a Track,
        clip: &Clip,
    ) -> (TimeRange, Option<&'a Transition>, Option<&'a Transition>) {
        // Cross-dissolves extend both clips across the cut with equal-power
        // gains; other transition kinds keep a hard audio cut.
        let span = track.clip_span(clip);
        let (fade_in, fade_out) = (dissolve(span.incoming), dissolve(span.outgoing));
        let play = TimeRange {
            start: fade_in.map_or(clip.range.start, |t| t.window(clip.range.start).start),
            end: fade_out.map_or(clip.range.end, |t| t.window(clip.range.end).end),
        };
        (play, fade_in, fade_out)
    }

    /// Add one clip's contribution for global frames `[first, end)` into `out`, whose first
    /// element is global frame `out_first`. `track_gain` is the linear fader applied to it
    /// (`1.0` when the track fader is applied after a track chain).
    #[allow(clippy::too_many_arguments, clippy::cast_possible_truncation)]
    fn add_clip(
        &self,
        track: &Track,
        clip: &Clip,
        track_gain: f64,
        first: usize,
        end: usize,
        out: &mut [f32],
        out_first: usize,
    ) -> Result<(), Error> {
        let ClipKind::Audio(id) = &clip.kind else {
            return Ok(());
        };
        let channels = usize::from(self.channels);
        let (play, fade_in, fade_out) = Self::clip_window(track, clip);
        let (clip_start, clip_end) = sample_bounds(play, self.rate)?;
        let (from, to) = (first.max(clip_start), end.min(clip_end));
        if from >= to {
            return Ok(());
        }
        let source = self
            .sources
            .get(id)
            .ok_or_else(|| Error::MissingAsset(id.as_str().into()))?;
        let chain_active = !self.dry && clip.audio_effects.iter().any(|e| !e.bypass);
        let mut cursor = source.cursor();
        if chain_active {
            // The chain output is per block; blocks are cached and shared across calls.
            let key = clip_key(self, clip, play);
            let block_frames = EFFECT_BLOCK_FRAMES;
            for block in from / block_frames..=(to - 1) / block_frames {
                let entry = self.clip_chain_block(clip, source, play, key, block)?;
                let base = block * block_frames;
                for frame in from.max(base)..to.min(base + block_frames) {
                    let now = frame_ticks(frame, self.rate)?;
                    let gain = track_gain
                        * crossfade(fade_in, clip.range.start, now, |(_, gain)| gain)
                        * crossfade(fade_out, clip.range.end, now, |(gain, _)| gain);
                    if !gain.is_finite() || gain < 0.0 {
                        return Err(Error::Invalid("nonfinite/negative gain".into()));
                    }
                    for channel in 0..channels {
                        out[(frame - out_first) * channels + channel] +=
                            (f64::from(entry.samples[(frame - base) * channels + channel]) * gain)
                                as f32;
                    }
                }
            }
            return Ok(());
        }
        for frame in from..to {
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
                * track_gain
                * crossfade(fade_in, clip.range.start, now, |(_, gain)| gain)
                * crossfade(fade_out, clip.range.end, now, |(gain, _)| gain);
            if !gain.is_finite() || gain < 0.0 {
                return Err(Error::Invalid("nonfinite/negative gain".into()));
            }
            let ticks = i128::from(TICKS_PER_SECOND);
            let source_ticks = local + i128::from(clip.source_offset.0) * i128::from(self.rate);
            if source_ticks < 0 {
                // Before the source start (a dissolve without a handle): silence.
                continue;
            }
            let source_frame = usize::try_from((source_ticks + ticks - 1) / ticks)
                .map_err(|e| Error::Invalid(e.to_string()))?;
            let Some(samples) = cursor.frame(source_frame)? else {
                continue;
            };
            for (channel, sample) in samples.iter().enumerate() {
                out[(frame - out_first) * channels + channel] += (f64::from(*sample) * gain) as f32;
            }
        }
        Ok(())
    }

    /// The clip's signal before its effect chain for global frames `[base, base + len)`:
    /// source samples scaled by the clip `gain` parameter, muted frames silent, zero outside
    /// the play window.
    fn clip_chain_input(
        &self,
        clip: &Clip,
        source: &PreparedSource,
        play: TimeRange,
        block: usize,
    ) -> Result<Vec<f32>, Error> {
        let channels = usize::from(self.channels);
        let base = block * EFFECT_BLOCK_FRAMES;
        let mut input = vec![0.0_f32; EFFECT_BLOCK_FRAMES * channels];
        let mut cursor: Cursor<'_> = source.cursor();
        let (clip_start, clip_end) = sample_bounds(play, self.rate)?;
        let ticks = i128::from(TICKS_PER_SECOND);
        for frame in base.max(clip_start)..(base + EFFECT_BLOCK_FRAMES).min(clip_end) {
            let local =
                frame as i128 * ticks - i128::from(clip.range.start.0) * i128::from(self.rate);
            let time = Time(
                i64::try_from(local / i128::from(self.rate))
                    .map_err(|e| Error::Invalid(e.to_string()))?,
            );
            if param(clip, "muted", time, 0.0) != 0.0 {
                continue;
            }
            let gain = param(clip, "gain", time, 1.0);
            if !gain.is_finite() || gain < 0.0 {
                return Err(Error::Invalid("nonfinite/negative gain".into()));
            }
            let source_ticks = local + i128::from(clip.source_offset.0) * i128::from(self.rate);
            if source_ticks < 0 {
                continue;
            }
            let source_frame = usize::try_from((source_ticks + ticks - 1) / ticks)
                .map_err(|e| Error::Invalid(e.to_string()))?;
            let Some(samples) = cursor.frame(source_frame)? else {
                continue;
            };
            for (channel, sample) in samples.iter().enumerate() {
                input[(frame - base) * channels + channel] = (f64::from(*sample) * gain) as f32;
            }
        }
        Ok(input)
    }

    /// Processed block `block` of a clip's effect chain.
    fn clip_chain_block(
        &self,
        clip: &Clip,
        source: &PreparedSource,
        play: TimeRange,
        key: u64,
        block: usize,
    ) -> Result<Arc<BlockEntry>, Error> {
        let (clip_start, _) = sample_bounds(play, self.rate)?;
        let chain_len = clip.audio_effects.iter().filter(|e| !e.bypass).count();
        let start_time = |block: usize| -> Result<Time, Error> {
            let ticks = frame_ticks(block * EFFECT_BLOCK_FRAMES, self.rate)?;
            Ok(Time(ticks.0 - clip.range.start.0))
        };
        self.chain_block(
            key,
            block,
            clip_start / EFFECT_BLOCK_FRAMES,
            chain_len,
            &|block| self.clip_chain_input(clip, source, play, block),
            &|block| Ok(clip.audio_effects_at(Time(start_time(block)?.0.max(0)))),
            &start_time,
        )
    }

    /// The summed, track-chain-processed signal of `track` for global frames `[first, end)`
    /// (before the track fader).
    fn track_chain(&self, track: &Track, first: usize, end: usize) -> Result<Vec<f32>, Error> {
        let channels = usize::from(self.channels);
        let chain_len = track.audio_effects().iter().filter(|e| !e.bypass).count();
        let key = track_key(self, track);
        let input = |block: usize| -> Result<Vec<f32>, Error> {
            let base = block * EFFECT_BLOCK_FRAMES;
            let mut sum = vec![0.0_f32; EFFECT_BLOCK_FRAMES * channels];
            for clip in track.clips().values() {
                self.add_clip(
                    track,
                    clip,
                    1.0,
                    base,
                    base + EFFECT_BLOCK_FRAMES,
                    &mut sum,
                    base,
                )?;
            }
            Ok(sum)
        };
        let effects_at = |_block: usize| {
            Ok(track
                .audio_effects()
                .iter()
                .filter(|e| !e.bypass)
                .cloned()
                .collect())
        };
        let start_time = |block: usize| frame_ticks(block * EFFECT_BLOCK_FRAMES, self.rate);
        let mut output = vec![0.0_f32; (end - first) * channels];
        for block in first / EFFECT_BLOCK_FRAMES..=(end - 1) / EFFECT_BLOCK_FRAMES {
            let entry =
                self.chain_block(key, block, 0, chain_len, &input, &effects_at, &start_time)?;
            let base = block * EFFECT_BLOCK_FRAMES;
            for frame in first.max(base)..end.min(base + EFFECT_BLOCK_FRAMES) {
                let (from, to) = ((frame - base) * channels, (frame - first) * channels);
                output[to..to + channels].copy_from_slice(&entry.samples[from..from + channels]);
            }
        }
        Ok(output)
    }

    /// Output block `block` of a chain, from the cache or by processing.
    ///
    /// The state entering block `b` is defined as follows. Let `seg = b - b % SEGMENT_BLOCKS`
    /// (never before `first_block`). Block `seg` receives the state reached by running a fresh
    /// chain over the `warm` blocks preceding it (or over nothing when `seg` is the chain's first
    /// block); every other block receives the state left by block `b - 1`. That makes the
    /// result independent of cache contents and mixing order, bounds the cost of a seek to
    /// `warm + SEGMENT_BLOCKS` blocks and still lets sequential playback run each block once.
    #[allow(clippy::too_many_arguments)]
    fn chain_block(
        &self,
        key: u64,
        block: usize,
        first_block: usize,
        chain_len: usize,
        input: &dyn Fn(usize) -> Result<Vec<f32>, Error>,
        effects_at: &dyn Fn(usize) -> Result<Vec<EffectInstance>, Error>,
        start_time: &dyn Fn(usize) -> Result<Time, Error>,
    ) -> Result<Arc<BlockEntry>, Error> {
        if let Some(entry) = self
            .cache
            .lock()
            .expect("effect cache lock")
            .get(key, block)
        {
            return Ok(entry);
        }
        let segment = (block - block % SEGMENT_BLOCKS).max(first_block);
        let skipped = std::cell::Cell::new(false);
        let run = |index: usize, states: &mut Vec<Vec<u8>>| -> Result<Vec<f32>, Error> {
            let mut samples = input(index)?;
            for (position, effect) in effects_at(index)?.iter().enumerate() {
                let outcome = match &self.effects {
                    Some(processor) => processor
                        .process(
                            effect,
                            &AudioBlock {
                                sample_rate: self.rate,
                                channels: self.channels,
                                start: start_time(index)?,
                                samples: &samples,
                            },
                            &states[position],
                        )
                        .map_err(|e| {
                            format!("audio effect {}/{}: {e}", effect.plugin, effect.effect)
                        }),
                    None => Err(format!(
                        "audio effect {}/{} needs an effect processor, but the mixer has none",
                        effect.plugin, effect.effect
                    )),
                };
                match outcome {
                    Ok(out) => {
                        samples = out.samples;
                        states[position] = out.state;
                    }
                    Err(message) if self.lenient => {
                        self.warnings
                            .push(format!("{message}; audio passes through unprocessed"));
                        skipped.set(true);
                    }
                    Err(message) => return Err(Error::Invalid(message)),
                }
            }
            Ok(samples)
        };
        // Resume from the nearest cached predecessor inside the segment...
        let mut start = block;
        let mut states = vec![Vec::new(); chain_len];
        {
            let mut cache = self.cache.lock().expect("effect cache lock");
            while start > segment {
                if let Some(previous) = cache.get(key, start - 1) {
                    states.clone_from(&previous.state_out);
                    break;
                }
                start -= 1;
            }
        }
        // ...or build the segment's canonical entry state by warm-up.
        if start == segment && segment > first_block {
            let warm = (self.rate as usize).div_ceil(EFFECT_BLOCK_FRAMES);
            for index in segment.saturating_sub(warm).max(first_block)..segment {
                run(index, &mut states)?;
            }
        }
        let mut entry = None;
        for index in start..=block {
            skipped.set(false);
            let samples = run(index, &mut states)?;
            let fresh = Arc::new(BlockEntry {
                samples,
                state_out: states.clone(),
            });
            // A block that skipped an effect is not the canonical result: never reuse it.
            if !skipped.get() {
                self.cache.lock().expect("effect cache lock").insert(
                    key,
                    index,
                    Arc::clone(&fresh),
                );
            }
            entry = Some(fresh);
        }
        Ok(entry.expect("at least the requested block is processed"))
    }
}

/// One processed block of a chain and the state it leaves for the next block.
struct BlockEntry {
    samples: Vec<f32>,
    state_out: Vec<Vec<u8>>,
}

/// Bounded cache of processed blocks, keyed by a hash of everything a chain's output depends on.
#[derive(Default)]
struct EffectCache {
    entries: HashMap<(u64, usize), (u64, Arc<BlockEntry>)>,
    clock: u64,
    bytes: usize,
}
impl EffectCache {
    fn size(entry: &BlockEntry) -> usize {
        entry.samples.len() * 4 + entry.state_out.iter().map(Vec::len).sum::<usize>()
    }
    fn get(&mut self, key: u64, block: usize) -> Option<Arc<BlockEntry>> {
        self.clock += 1;
        let clock = self.clock;
        self.entries.get_mut(&(key, block)).map(|(used, entry)| {
            *used = clock;
            Arc::clone(entry)
        })
    }
    fn insert(&mut self, key: u64, block: usize, entry: Arc<BlockEntry>) {
        self.clock += 1;
        self.bytes += Self::size(&entry);
        if let Some((_, old)) = self.entries.insert((key, block), (self.clock, entry)) {
            self.bytes -= Self::size(&old);
        }
        if self.bytes > EFFECT_CACHE_BYTES {
            // Drop the least recently used half.
            let mut used: Vec<_> = self.entries.values().map(|(used, _)| *used).collect();
            used.sort_unstable();
            let cutoff = used[used.len() / 2];
            self.entries.retain(|_, (used, _)| *used > cutoff);
            self.bytes = self.entries.values().map(|(_, e)| Self::size(e)).sum();
        }
    }
}

fn frame_ticks(frame: usize, rate: u32) -> Result<Time, Error> {
    i64::try_from(frame as i128 * i128::from(TICKS_PER_SECOND) / i128::from(rate))
        .map(Time)
        .map_err(|e| Error::Invalid(e.to_string()))
}

/// Hash of everything that determines a clip chain's processed output, including the signal
/// feeding it (the clip, its asset and play window) and the output format.
fn clip_key(mixer: &Mixer, clip: &Clip, play: TimeRange) -> u64 {
    let mut hasher = DefaultHasher::new();
    (mixer.rate, mixer.channels, play.start.0, play.end.0).hash(&mut hasher);
    format!("{clip:?}").hash(&mut hasher);
    hasher.finish()
}

/// Hash of everything that determines a track chain's output: its clips (and their chains),
/// transitions and its own chain; the fader and mute/solo state are applied afterwards.
fn track_key(mixer: &Mixer, track: &Track) -> u64 {
    let mut hasher = DefaultHasher::new();
    (mixer.rate, mixer.channels, track.id().0).hash(&mut hasher);
    format!(
        "{:?}{:?}{:?}",
        track.clips(),
        track.transitions(),
        track.audio_effects()
    )
    .hash(&mut hasher);
    hasher.finish()
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
