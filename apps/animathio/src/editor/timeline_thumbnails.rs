//! Bounded, asynchronous low-resolution timeline thumbnail cache.
use super::*;
use animathio_media::{FilmCraftBackend, MediaBackend, VideoDecoder};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};

/// Retained decoded tiles.
const MAX_TILES: usize = 256;

/// A small least-recently-used map: `get` and `insert` refresh an entry, and
/// inserting into a full map evicts the entry that was used longest ago.
struct Lru<K: Ord + Clone, V> {
    capacity: usize,
    clock: u64,
    entries: BTreeMap<K, (u64, V)>,
}

impl<K: Ord + Clone, V> Lru<K, V> {
    fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            clock: 0,
            entries: BTreeMap::new(),
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    fn contains_key(&self, key: &K) -> bool {
        self.entries.contains_key(key)
    }

    #[cfg(test)]
    fn values(&self) -> impl Iterator<Item = &V> {
        self.entries.values().map(|(_, value)| value)
    }

    fn get(&mut self, key: &K) -> Option<&V> {
        self.clock += 1;
        let clock = self.clock;
        let (stamp, value) = self.entries.get_mut(key)?;
        *stamp = clock;
        Some(&*value)
    }

    fn insert(&mut self, key: K, value: V) {
        if self.entries.len() >= self.capacity && !self.entries.contains_key(&key) {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, (stamp, _))| *stamp)
                .map(|(key, _)| key.clone());
            if let Some(oldest) = oldest {
                self.entries.remove(&oldest);
            }
        }
        self.clock += 1;
        self.entries.insert(key, (self.clock, value));
    }
}

/// Opened decoders kept alive by the worker.
const MAX_DECODERS: usize = 6;
/// Small decoded stills kept by the worker.
const MAX_STILLS: usize = 16;

/// Worker-side caches. `bytes` holds one shared copy of each encoded video asset;
/// the decoders and any later re-open share that allocation instead of copying it.
struct WorkerCaches {
    bytes: Lru<AssetId, Arc<[u8]>>,
    decoders: Lru<AssetId, Arc<dyn VideoDecoder>>,
    stills: Lru<AssetId, (image::RgbaImage, (u32, u32))>,
}

impl WorkerCaches {
    fn new() -> Self {
        Self {
            bytes: Lru::new(MAX_DECODERS),
            decoders: Lru::new(MAX_DECODERS),
            stills: Lru::new(MAX_STILLS),
        }
    }
}

type Key = (AssetId, Time, u32);
#[derive(Clone)]
pub(super) struct Thumbnail {
    pub image: Arc<RenderImage>,
    pub source_size: (u32, u32),
}
type ResultImage = (Key, Option<Thumbnail>);
struct Request {
    key: Key,
    image: bool,
    assets: Arc<BTreeMap<AssetId, Vec<u8>>>,
}

pub(super) struct Thumbnails {
    tx: SyncSender<Request>,
    rx: Receiver<ResultImage>,
    /// Decoded tiles, least recently used evicted first.
    images: Lru<Key, Option<Thumbnail>>,
    pending: BTreeSet<Key>,
}
impl Default for Thumbnails {
    fn default() -> Self {
        let (tx, requests) = sync_channel::<Request>(16);
        let (results, rx) = sync_channel(16);
        std::thread::Builder::new()
            .name("timeline-thumbnails".into())
            .spawn(move || {
                let mut caches = WorkerCaches::new();
                while let Ok(request) = requests.recv() {
                    let image = decode(&request, &mut caches);
                    if results.send((request.key, image)).is_err() {
                        break;
                    }
                }
            })
            .expect("spawn timeline thumbnail worker");
        Self {
            tx,
            rx,
            images: Lru::new(MAX_TILES),
            pending: BTreeSet::new(),
        }
    }
}
impl Thumbnails {
    pub(super) fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok((key, image)) = self.rx.try_recv() {
            self.pending.remove(&key);
            self.insert(key, image);
            changed = true;
        }
        changed
    }
    /// Store a decoded tile, evicting the least recently used one when full.
    fn insert(&mut self, key: Key, image: Option<Thumbnail>) {
        self.images.insert(key, image);
    }

    pub(super) fn get(
        &mut self,
        asset: &AssetId,
        time: Time,
        height: u32,
        image: bool,
        assets: &Arc<BTreeMap<AssetId, Vec<u8>>>,
    ) -> Option<Thumbnail> {
        let key = (asset.clone(), if image { Time(0) } else { time }, height);
        if let Some(found) = self.images.get(&key) {
            return found.clone();
        }
        if !self.pending.contains(&key)
            && self
                .tx
                .try_send(Request {
                    key: key.clone(),
                    image,
                    assets: assets.clone(),
                })
                .is_ok()
        {
            self.pending.insert(key);
        }
        None
    }
}
fn decode(request: &Request, caches: &mut WorkerCaches) -> Option<Thumbnail> {
    let (asset, time, height) = &request.key;
    let bytes = request.assets.get(asset)?;
    let (rgba, source_size) = if request.image {
        if caches.stills.get(asset).is_none() {
            // Retain only a small source thumbnail, never the full decoded still.
            let image = image::load_from_memory(bytes).ok()?;
            let source_size = (image.width(), image.height());
            let small = image.thumbnail(256, 144).to_rgba8();
            caches.stills.insert(asset.clone(), (small, source_size));
        }
        caches.stills.get(asset)?.clone()
    } else {
        if caches.decoders.get(asset).is_none() {
            // One shared copy of the encoded asset, reused if the decoder is reopened.
            if caches.bytes.get(asset).is_none() {
                caches
                    .bytes
                    .insert(asset.clone(), Arc::from(bytes.as_slice()));
            }
            let shared = Arc::clone(caches.bytes.get(asset)?);
            let decoder = FilmCraftBackend.open_video_bytes("timeline", shared).ok()?;
            caches.decoders.insert(asset.clone(), Arc::from(decoder));
        }
        let decoder = Arc::clone(caches.decoders.get(asset)?);
        let frame = decoder.frame_at(*time).ok()?;
        (
            image::RgbaImage::from_raw(frame.width, frame.height, frame.pixels.to_vec())?,
            (frame.width, frame.height),
        )
    };
    let height = (*height).clamp(16, 96);
    let width = (u64::from(rgba.width()) * u64::from(height) / u64::from(rgba.height().max(1)))
        .clamp(1, 256) as u32;
    let mut small =
        image::imageops::resize(&rgba, width, height, image::imageops::FilterType::Triangle);
    for pixel in small.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Some(Thumbnail {
        image: Arc::new(RenderImage::new([image::Frame::new(small)])),
        source_size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    // `gpui::test` is glob-imported through `super::*`; keep the std attribute.
    use core::prelude::v1::test;

    #[test]
    fn native_dimensions_survive_the_bounded_still_thumbnail() {
        let image = image::RgbaImage::from_pixel(640, 480, image::Rgba([255, 0, 0, 255]));
        let mut encoded = std::io::Cursor::new(Vec::new());
        image
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        let bytes = encoded.into_inner();
        let asset = AssetId::of(&bytes);
        let request = Request {
            key: (asset.clone(), Time(0), 24),
            image: true,
            assets: Arc::new(BTreeMap::from([(asset, bytes)])),
        };
        let tile = decode(&request, &mut WorkerCaches::new()).unwrap();
        assert_eq!(tile.source_size, (640, 480));
    }

    #[test]
    fn lru_evicts_the_entry_used_longest_ago_not_the_smallest_key() {
        let mut lru = Lru::new(3);
        for key in [1, 2, 3] {
            lru.insert(key, key * 10);
        }
        assert_eq!(lru.get(&1), Some(&10)); // 2 is now the oldest
        lru.insert(4, 40);
        assert!(lru.contains_key(&1) && lru.contains_key(&3) && lru.contains_key(&4));
        assert!(!lru.contains_key(&2));
        // Re-inserting an existing key replaces it without evicting anything.
        lru.insert(3, 31);
        assert_eq!(lru.len(), 3);
        assert_eq!(lru.get(&3), Some(&31));
    }

    #[test]
    fn video_decoders_share_one_copy_of_the_asset() {
        let bytes = include_bytes!("../../../../crates/media/tests/fixtures/sample.mp4").to_vec();
        let video = AssetId::of(&bytes);
        let request = |time: i64| Request {
            key: (video.clone(), Time(time), 24),
            image: false,
            assets: Arc::new(BTreeMap::from([(video.clone(), bytes.clone())])),
        };
        let mut caches = WorkerCaches::new();
        assert!(decode(&request(0), &mut caches).is_some());
        assert!(decode(&request(TICKS_PER_SECOND / 2), &mut caches).is_some());
        assert_eq!(caches.decoders.len(), 1);
        assert_eq!(caches.bytes.len(), 1);
        // The cache and the open decoder reference the same allocation.
        let shared = Arc::clone(caches.bytes.get(&video).unwrap());
        assert!(
            Arc::strong_count(&shared) >= 3,
            "{}",
            Arc::strong_count(&shared)
        );
    }

    #[test]
    fn cache_evicts_least_recently_used_tile() {
        let mut cache = Thumbnails::default();
        let assets = Arc::new(BTreeMap::new());
        let asset = AssetId::of(b"lru");
        let key = |i: i64| (asset.clone(), Time(i), 24);
        for i in 0..i64::try_from(MAX_TILES).unwrap() {
            cache.insert(key(i), None);
        }
        // Touch the oldest tile, then overflow: tile 1 goes, tile 0 stays.
        cache.get(&asset, Time(0), 24, false, &assets);
        cache.insert(key(10_000), None);
        assert_eq!(cache.images.len(), MAX_TILES);
        assert!(cache.images.contains_key(&key(0)));
        assert!(!cache.images.contains_key(&key(1)));
    }

    #[test]
    fn image_tiles_repeat_and_video_samples_decode_on_worker() {
        let bytes = include_bytes!("../../../../crates/media/tests/fixtures/sample.mp4").to_vec();
        let video = AssetId::of(&bytes);
        let mut assets = BTreeMap::from([(video.clone(), bytes)]);
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(48, 24, image::Rgba([200, 100, 40, 255]))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let still = AssetId::of(png.get_ref());
        assets.insert(still.clone(), png.into_inner());
        let assets = Arc::new(assets);
        let mut cache = Thumbnails::default();
        assert!(cache.get(&still, Time(0), 24, true, &assets).is_none());
        cache.get(&still, Time(TICKS_PER_SECOND), 24, true, &assets);
        assert_eq!(
            cache.pending.len(),
            1,
            "stills share the same tile at every time"
        );
        cache.get(&video, Time(0), 24, false, &assets);
        cache.get(&video, Time(TICKS_PER_SECOND), 24, false, &assets);
        let deadline = Instant::now() + Duration::from_secs(20);
        while !cache.pending.is_empty() && Instant::now() < deadline {
            cache.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(cache.pending.is_empty(), "thumbnail worker did not finish");
        assert_eq!(cache.images.len(), 3);
        assert!(cache.images.values().all(Option::is_some));
    }
}
