//! The engine-owned media resource pool, keyed by [`AssetId`].

use std::collections::{BTreeMap, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use animathio_core::{AssetId, Rounding, Time};

use crate::backend::{AudioDecoder, MediaBackend, VideoDecoder};
use crate::frame::Frame;
use crate::info::MediaInfo;
use crate::{MediaError, Result};

type CacheKey = (AssetId, Time);
type Job = Box<dyn FnOnce() + Send + 'static>;

/// Tunables for a [`MediaPool`].
#[derive(Clone, Copy, Debug)]
pub struct MediaPoolConfig {
    /// Byte budget for the decoded-frame LRU cache.
    pub budget_bytes: usize,
    /// Number of frames to prefetch ahead/behind during sequential playback.
    pub prefetch: usize,
    /// Number of decode worker threads.
    pub threads: usize,
}

impl Default for MediaPoolConfig {
    fn default() -> Self {
        Self {
            budget_bytes: 128 * 1024 * 1024,
            prefetch: 8,
            threads: 2,
        }
    }
}

/// Per-asset decoder state. Kept behind [`Arc`] so in-flight decode jobs can finish even after
/// the asset is released; the `released` flag keeps them from repopulating the cache.
struct AssetState {
    id: AssetId,
    video: Option<Box<dyn VideoDecoder>>,
    audio: Option<Box<dyn AudioDecoder>>,
    info: MediaInfo,
    released: AtomicBool,
    playhead: AtomicI64,
}

struct FrameCache {
    budget: usize,
    bytes: usize,
    map: BTreeMap<CacheKey, Arc<Frame>>,
    order: VecDeque<CacheKey>,
}

impl FrameCache {
    fn new(budget: usize) -> Self {
        Self {
            budget,
            bytes: 0,
            map: BTreeMap::new(),
            order: VecDeque::new(),
        }
    }

    fn get(&mut self, key: &CacheKey) -> Option<Arc<Frame>> {
        let frame = self.map.get(key)?.clone();
        self.touch(key);
        Some(frame)
    }

    fn insert(&mut self, key: CacheKey, frame: Arc<Frame>) {
        let size = frame.byte_size();
        if let Some(old) = self.map.get(&key) {
            self.bytes = self.bytes.saturating_sub(old.byte_size());
        } else {
            self.order.push_back(key.clone());
        }
        self.map.insert(key.clone(), frame);
        self.bytes = self.bytes.saturating_add(size);
        self.touch(&key);
        self.evict();
    }

    fn touch(&mut self, key: &CacheKey) {
        if let Some(i) = self.order.iter().position(|k| k == key) {
            self.order.remove(i);
        }
        self.order.push_back(key.clone());
    }

    fn evict(&mut self) {
        while self.bytes > self.budget {
            let Some(key) = self.order.pop_front() else {
                break;
            };
            if let Some(frame) = self.map.remove(&key) {
                self.bytes = self.bytes.saturating_sub(frame.byte_size());
            }
        }
    }

    fn remove_asset(&mut self, asset: &AssetId) {
        self.order.retain(|(a, _)| a != asset);
        self.map.retain(|(a, _), _| a != asset);
        self.bytes = self.map.values().map(|f| f.byte_size()).sum();
    }

    fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
        self.bytes = 0;
    }
}

struct PoolInner {
    assets: BTreeMap<AssetId, Arc<AssetState>>,
    cache: FrameCache,
}

struct WorkerPool {
    tx: crossbeam_channel::Sender<Job>,
}

impl WorkerPool {
    fn new(threads: usize) -> Self {
        let threads = threads.clamp(1, 32);
        let (tx, rx) = crossbeam_channel::unbounded::<Job>();
        for _ in 0..threads {
            let rx = rx.clone();
            std::thread::Builder::new()
                .name("animathio-media-worker".to_owned())
                .spawn(move || {
                    while let Ok(job) = rx.recv() {
                        job();
                    }
                })
                .expect("failed to spawn media worker");
        }
        Self { tx }
    }

    fn spawn(&self, job: Job) {
        let _ = self.tx.send(job);
    }
}

/// The engine-owned pool of decoders and decoded frames, keyed by [`AssetId`].
pub struct MediaPool {
    backend: Arc<dyn MediaBackend>,
    workers: WorkerPool,
    inner: Arc<Mutex<PoolInner>>,
    prefetch: usize,
}

impl MediaPool {
    /// Create a pool using `backend` for all decoding.
    #[must_use]
    pub fn new(backend: Arc<dyn MediaBackend>, config: MediaPoolConfig) -> Self {
        Self {
            backend,
            workers: WorkerPool::new(config.threads),
            inner: Arc::new(Mutex::new(PoolInner {
                assets: BTreeMap::new(),
                cache: FrameCache::new(config.budget_bytes),
            })),
            prefetch: config.prefetch,
        }
    }

    /// Register an asset, opening its video and audio decoders. Assets that expose neither stream
    /// are rejected; a missing audio (or video) stream is not an error.
    pub fn add(&self, asset: AssetId, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let info = self.backend.probe(path)?;
        let video = match self.backend.open_video(path) {
            Ok(v) => Some(v),
            Err(MediaError::NoStream("video")) => None,
            Err(e) => return Err(e),
        };
        let audio = match self.backend.open_audio(path) {
            Ok(a) => Some(a),
            Err(MediaError::NoStream("audio")) => None,
            Err(e) => return Err(e),
        };
        if video.is_none() && audio.is_none() {
            return Err(MediaError::Format(format!(
                "{}: no decodable streams",
                path.display()
            )));
        }
        let state = Arc::new(AssetState {
            id: asset.clone(),
            video,
            audio,
            info,
            released: AtomicBool::new(false),
            playhead: AtomicI64::new(0),
        });
        let mut inner = crate::lock(self.inner.as_ref());
        inner.assets.insert(asset, state);
        Ok(())
    }

    /// Decode the frame displayed at `t` for `asset`, returning a shared RGBA frame.
    ///
    /// Sequential requests decode forward (prefetching ahead), random access falls back to
    /// FilmCraft's GOP-aware seek. Decoding happens on a worker thread.
    pub fn frame_at(&self, asset: AssetId, t: Time) -> Result<Arc<Frame>> {
        if let Some(frame) = self.cache_get(&asset, t) {
            if let Some(state) = self.state(&asset) {
                self.schedule_prefetch(&state, t);
            }
            return Ok(frame);
        }
        let state = self.state(&asset).ok_or(MediaError::UnknownAsset(asset))?;
        if state.video.is_none() {
            return Err(MediaError::NoStream("video"));
        }
        let (tx, rx) = crossbeam_channel::bounded::<Result<Arc<Frame>>>(1);
        let inner = self.inner.clone();
        let state2 = state.clone();
        self.workers.spawn(Box::new(move || {
            let result = decode_and_cache(&inner, &state2, t);
            let _ = tx.send(result);
        }));
        let frame = rx.recv().map_err(|_| MediaError::Shutdown)??;
        self.schedule_prefetch(&state, t);
        Ok(frame)
    }

    /// Decode `len` interleaved f32 PCM samples starting at `start` for `asset`, at the
    /// stream's sample rate.
    pub fn audio_samples(&self, asset: AssetId, start: Time, len: usize) -> Result<Arc<[f32]>> {
        let state = self.state(&asset).ok_or(MediaError::UnknownAsset(asset))?;
        let audio = state.audio.as_ref().ok_or(MediaError::NoStream("audio"))?;
        audio.samples(start, len)
    }

    /// Release one asset and drop its cached frames.
    pub fn release(&self, asset: AssetId) {
        let mut inner = crate::lock(self.inner.as_ref());
        if let Some(state) = inner.assets.remove(&asset) {
            state.released.store(true, Ordering::SeqCst);
            inner.cache.remove_asset(&asset);
        }
    }

    /// Release every asset and drop the frame cache.
    pub fn clear(&self) {
        let mut inner = crate::lock(self.inner.as_ref());
        for state in inner.assets.values() {
            state.released.store(true, Ordering::SeqCst);
        }
        inner.assets.clear();
        inner.cache.clear();
    }

    /// Number of registered assets.
    #[must_use]
    pub fn len(&self) -> usize {
        let inner = crate::lock(self.inner.as_ref());
        inner.assets.len()
    }

    /// True when no assets and no cached frames remain.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        let inner = crate::lock(self.inner.as_ref());
        inner.assets.is_empty() && inner.cache.is_empty()
    }

    fn state(&self, asset: &AssetId) -> Option<Arc<AssetState>> {
        let inner = crate::lock(self.inner.as_ref());
        inner.assets.get(asset).cloned()
    }

    fn cache_get(&self, asset: &AssetId, t: Time) -> Option<Arc<Frame>> {
        let mut inner = crate::lock(self.inner.as_ref());
        inner.cache.get(&(asset.clone(), t))
    }

    fn schedule_prefetch(&self, state: &Arc<AssetState>, t: Time) {
        if state.released.load(Ordering::SeqCst) {
            return;
        }
        let prev = state.playhead.swap(t.0, Ordering::SeqCst);
        let direction = if t.0 > prev { 1_i64 } else { -1_i64 };
        let Some(video) = &state.info.video else {
            return;
        };
        let dt = video
            .frame_rate
            .frame_to_time(1, Rounding::Floor)
            .unwrap_or_default()
            .0;
        if dt <= 0 {
            return;
        }
        let inner = self.inner.clone();
        for k in 1..=self.prefetch {
            let offset = dt.saturating_mul(i64::try_from(k).unwrap_or(0));
            let Some(target) = t.0.checked_add(direction.saturating_mul(offset)) else {
                break;
            };
            let key = (state.id.clone(), Time(target));
            let inner2 = inner.clone();
            let state2 = state.clone();
            self.workers.spawn(Box::new(move || {
                let mut guard = crate::lock(inner2.as_ref());
                if guard.cache.get(&key).is_some() {
                    return;
                }
                drop(guard);
                let _ = decode_and_cache(&inner2, &state2, Time(target));
            }));
        }
    }
}

fn decode_and_cache(
    inner: &Mutex<PoolInner>,
    state: &Arc<AssetState>,
    t: Time,
) -> Result<Arc<Frame>> {
    let frame = state
        .video
        .as_ref()
        .ok_or(MediaError::NoStream("video"))?
        .frame_at(t)?;
    insert_if_current(inner, state, t, frame.clone());
    Ok(frame)
}

fn insert_if_current(
    inner: &Mutex<PoolInner>,
    state: &Arc<AssetState>,
    t: Time,
    frame: Arc<Frame>,
) {
    let mut guard = crate::lock(inner);
    let current = guard
        .assets
        .get(&state.id)
        .is_some_and(|s| Arc::ptr_eq(s, state))
        && !state.released.load(Ordering::SeqCst);
    if current {
        guard.cache.insert((state.id.clone(), t), frame);
    }
}
