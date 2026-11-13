//! Bounded, asynchronous low-resolution timeline thumbnail cache.
use super::*;
use animathio_media::{FilmCraftBackend, MediaBackend, VideoDecoder};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};

type Key = (AssetId, Time, u32);
type ResultImage = (Key, Option<Arc<RenderImage>>);
struct Request {
    key: Key,
    image: bool,
    assets: Arc<BTreeMap<AssetId, Vec<u8>>>,
}

pub(super) struct Thumbnails {
    tx: SyncSender<Request>,
    rx: Receiver<ResultImage>,
    images: BTreeMap<Key, Option<Arc<RenderImage>>>,
    pending: BTreeSet<Key>,
}
impl Default for Thumbnails {
    fn default() -> Self {
        let (tx, requests) = sync_channel::<Request>(16);
        let (results, rx) = sync_channel(16);
        std::thread::Builder::new()
            .name("timeline-thumbnails".into())
            .spawn(move || {
                let mut decoders: BTreeMap<AssetId, Box<dyn VideoDecoder>> = BTreeMap::new();
                let mut stills = BTreeMap::new();
                while let Ok(request) = requests.recv() {
                    let image = decode(&request, &mut decoders, &mut stills);
                    if results.send((request.key, image)).is_err() {
                        break;
                    }
                }
            })
            .expect("spawn timeline thumbnail worker");
        Self {
            tx,
            rx,
            images: BTreeMap::new(),
            pending: BTreeSet::new(),
        }
    }
}
impl Thumbnails {
    pub(super) fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok((key, image)) = self.rx.try_recv() {
            self.pending.remove(&key);
            // 256 low-resolution tiles bound the retained CPU/GPU resources.
            if self.images.len() >= 256 {
                self.images.pop_first();
            }
            self.images.insert(key, image);
            changed = true;
        }
        changed
    }
    pub(super) fn get(
        &mut self,
        asset: &AssetId,
        time: Time,
        height: u32,
        image: bool,
        assets: &Arc<BTreeMap<AssetId, Vec<u8>>>,
    ) -> Option<Arc<RenderImage>> {
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
fn decode(
    request: &Request,
    decoders: &mut BTreeMap<AssetId, Box<dyn VideoDecoder>>,
    stills: &mut BTreeMap<AssetId, image::RgbaImage>,
) -> Option<Arc<RenderImage>> {
    let (asset, time, height) = &request.key;
    let bytes = request.assets.get(asset)?;
    let rgba = if request.image {
        if !stills.contains_key(asset) {
            if stills.len() >= 8 {
                stills.clear();
            }
            // Retain only a small source thumbnail, never the full decoded still.
            let small = image::load_from_memory(bytes)
                .ok()?
                .thumbnail(256, 144)
                .to_rgba8();
            stills.insert(asset.clone(), small);
        }
        stills.get(asset)?.clone()
    } else {
        if !decoders.contains_key(asset) {
            if decoders.len() >= 8 {
                decoders.clear();
            }
            let decoder = FilmCraftBackend
                .open_video_bytes("timeline", Arc::from(bytes.as_slice()))
                .ok()?;
            decoders.insert(asset.clone(), decoder);
        }
        let frame = decoders.get(asset)?.frame_at(*time).ok()?;
        image::RgbaImage::from_raw(frame.width, frame.height, frame.pixels.to_vec())?
    };
    let height = (*height).clamp(16, 96);
    let width = (u64::from(rgba.width()) * u64::from(height) / u64::from(rgba.height().max(1)))
        .clamp(1, 256) as u32;
    let mut small =
        image::imageops::resize(&rgba, width, height, image::imageops::FilterType::Triangle);
    for pixel in small.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Some(Arc::new(RenderImage::new([image::Frame::new(small)])))
}

#[cfg(test)]
mod tests {
    use super::*;
    // `gpui::test` is glob-imported through `super::*`; keep the std attribute.
    use core::prelude::v1::test;

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
