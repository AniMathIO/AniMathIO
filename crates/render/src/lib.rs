//! Rendering of evaluated scenes. The deterministic, GPU-free CPU backend is the
//! reference for tests and export; the optional `gpu` feature adds a Vello/wgpu
//! backend for interactive preview.
//! Media bytes and decoded images live in `RenderCtx`, never in the project.
mod cpu;
mod effects;
#[cfg(feature = "gpu")]
mod gpu;
pub mod sample;
mod transition;

use animathio_core::{
    AssetId, ClipKind, EffectProcessor, Project, Rounding, Scene, SceneClip, Time,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Write},
    path::Path,
    sync::Arc,
};

pub use cpu::CpuBackend;
#[cfg(feature = "gpu")]
pub use gpu::GpuBackend;

/// Output dimensions in pixels. The prototype limits frames to 32 megapixels.
pub type Size = (u32, u32);

/// Tightly packed, row-major, straight-alpha sRGB RGBA8 pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// Pixel dimensions.
    pub size: Size,
    /// Exactly width × height × 4 bytes.
    pub pixels: Vec<u8>,
}
impl Frame {
    /// Check dimensions and buffer length, including frames returned by a decoder.
    pub fn validate(&self) -> Result<(), Error> {
        check_size(self.size)?;
        if self.pixels.len() != self.size.0 as usize * self.size.1 as usize * 4 {
            return Err(Error::Invalid("invalid RGBA8 buffer length"));
        }
        Ok(())
    }
    /// Encode a PNG without involving the renderer.
    pub fn write_png(&self, writer: impl Write) -> Result<(), Error> {
        self.validate()?;
        image::ImageEncoder::write_image(
            image::codecs::png::PngEncoder::new(writer),
            &self.pixels,
            self.size.0,
            self.size.1,
            image::ExtendedColorType::Rgba8,
        )?;
        Ok(())
    }
    /// Save a PNG frame.
    pub fn save_png(&self, path: impl AsRef<Path>) -> Result<(), Error> {
        self.validate()?;
        let mut writer = std::io::BufWriter::new(std::fs::File::create(path)?);
        self.write_png(&mut writer)?;
        writer.flush()?;
        Ok(())
    }
}

/// Rendering, resource and encoding failures.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Invalid dimensions, pixels or rendering input.
    #[error("{0}")]
    Invalid(&'static str),
    /// No bytes have been registered for the requested image.
    #[error("missing image asset: {0}")]
    MissingAsset(String),
    /// Encoded media did not match its content address.
    #[error("asset hash mismatch")]
    AssetHash,
    /// Image decoding/encoding failure.
    #[error(transparent)]
    Image(#[from] image::ImageError),
    /// File failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Invalid project construction.
    #[error(transparent)]
    Model(#[from] animathio_core::Error),
    /// A glyph could not be rasterized.
    #[error("glyph rendering failed: {0}")]
    Glyph(String),
    /// A clip effect could not be applied.
    #[error("effect error: {0}")]
    Effect(String),
    /// No usable GPU adapter/device, or a GPU render failed. Callers fall back to CPU.
    #[error("GPU backend unavailable: {0}")]
    Gpu(String),
}

pub(crate) fn check_size(size: Size) -> Result<(u16, u16), Error> {
    let width = u16::try_from(size.0).map_err(|_| Error::Invalid("width exceeds 65535"))?;
    let height = u16::try_from(size.1).map_err(|_| Error::Invalid("height exceeds 65535"))?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 32 * 1024 * 1024 {
        return Err(Error::Invalid("frame must contain 1..=33554432 pixels"));
    }
    Ok((width, height))
}

/// Engine-owned, content-addressed resources. Images decode lazily once per asset.
#[derive(Default)]
pub struct ResourcePool {
    encoded: BTreeMap<AssetId, Arc<[u8]>>,
    images: BTreeMap<AssetId, Arc<Frame>>,
    /// Runs clip effects; set with [`RenderCtx::set_effect_processor`].
    pub(crate) effects: Option<Box<dyn EffectProcessor>>,
}
impl ResourcePool {
    /// Register verified bytes; re-registering identical content preserves its cache.
    pub fn insert(&mut self, id: AssetId, bytes: Vec<u8>) -> Result<(), Error> {
        if AssetId::of(&bytes) != id {
            return Err(Error::AssetHash);
        }
        self.encoded.entry(id).or_insert_with(|| bytes.into());
        Ok(())
    }
    /// Register content and return its hash.
    pub fn add(&mut self, bytes: Vec<u8>) -> AssetId {
        let id = AssetId::of(&bytes);
        self.encoded
            .entry(id.clone())
            .or_insert_with(|| bytes.into());
        id
    }
    /// Decode PNG/JPEG by probing bytes; metadata never selects the codec.
    pub fn image(&mut self, asset: &AssetId) -> Result<Arc<Frame>, Error> {
        if let Some(frame) = self.images.get(asset) {
            return Ok(frame.clone());
        }
        let bytes = self
            .encoded
            .get(asset)
            .ok_or_else(|| Error::MissingAsset(asset.as_str().to_owned()))?;
        let mut reader =
            image::ImageReader::new(Cursor::new(bytes.as_ref())).with_guessed_format()?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(u32::from(u16::MAX));
        limits.max_image_height = Some(u32::from(u16::MAX));
        limits.max_alloc = Some(128 * 1024 * 1024);
        reader.limits(limits);
        let image = reader.decode()?.to_rgba8();
        let frame = Arc::new(Frame {
            size: image.dimensions(),
            pixels: image.into_raw(),
        });
        frame.validate()?;
        self.images.insert(asset.clone(), frame.clone());
        Ok(frame)
    }
    /// Number of decoded image cache entries.
    #[must_use]
    pub fn decoded_images(&self) -> usize {
        self.images.len()
    }
    /// Number of registered encoded assets.
    #[must_use]
    pub fn len(&self) -> usize {
        self.encoded.len()
    }
    /// Whether all asset resources have been released.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.encoded.is_empty() && self.images.is_empty()
    }
    /// Release resources not referenced by any clip, even if metadata still exists.
    pub fn gc(&mut self, project: &Project) {
        let live: BTreeSet<_> = project
            .tracks()
            .iter()
            .flat_map(|t| t.clips().values())
            .filter_map(|c| match &c.kind {
                ClipKind::Image(id) | ClipKind::Video(id) | ClipKind::Audio(id) => Some(id.clone()),
                _ => None,
            })
            .collect();
        self.encoded.retain(|id, _| live.contains(id));
        self.images.retain(|id, _| live.contains(id));
    }
}

/// A decoded frame shared with its source without copying (the media pool keeps its
/// frames in `Arc`s). Only the blending path needs it.
#[derive(Clone)]
pub struct SharedFrame {
    /// Pixel dimensions.
    pub size: Size,
    /// Exactly width × height × 4 bytes.
    pub pixels: Arc<[u8]>,
}

/// Random-access video adapter. Results must depend only on asset and media time.
/// Implementations own their decoder resources and must not advance a playback clock.
pub trait FrameSource {
    /// Return the frame due at this exact source-local tick position, if available.
    fn frame_at(&mut self, asset: &AssetId, media_time: Time) -> Option<Frame>;
    /// The same frame as [`FrameSource::frame_at`], shared instead of copied. The default
    /// copies; sources that already hold their frames in an `Arc` override it.
    fn shared_frame_at(&mut self, asset: &AssetId, media_time: Time) -> Option<SharedFrame> {
        let frame = self.frame_at(asset, media_time)?;
        Some(SharedFrame {
            size: frame.size,
            pixels: frame.pixels.into(),
        })
    }
    /// Which decoded frame of the asset is due at `media_time` (its index at the source's
    /// own frame rate), when the source knows. Equal indices are the same picture, so
    /// the blending path decodes and weights them once. `None` means "unknown": the
    /// time is treated as distinct.
    fn frame_index(&mut self, _asset: &AssetId, _media_time: Time) -> Option<i64> {
        None
    }
}

/// Accumulator pixels handled per pass of the blender: a small fixed buffer instead of
/// one wide sum per pixel of the frame.
const BLEND_CHUNK_PIXELS: usize = 1 << 14;

/// The video frame a scene clip shows. Normally the frame due at `source_time`. When the
/// source clock outruns the output frame rate, `evaluate` lists `source_taps` instead and
/// the frames due at those times are averaged: a box filter over the source interval one
/// output frame covers, which turns the aliasing of dropped frames into motion blur. Taps
/// that land on the same decoded frame are fetched once and weighted by how many taps
/// they cover (so a source slower than the taps does not change the filter, only the
/// work); the blend reads the shared frames in place and accumulates a chunk of pixels at
/// a time. The result depends only on the scene and the frame source, so preview, export
/// and tests agree.
pub(crate) fn video_frame(
    video: &mut dyn FrameSource,
    asset: &AssetId,
    clip: &SceneClip,
) -> Option<Frame> {
    if clip.source_taps.len() < 2 {
        return video.frame_at(asset, clip.source_time);
    }
    // Distinct decoded frames with the number of taps each covers, in tap order.
    let mut frames: Vec<(SharedFrame, Option<i64>, u32)> = Vec::new();
    let mut size = None;
    for time in &clip.source_taps {
        let index = video.frame_index(asset, *time);
        if let Some(index) = index
            && let Some(entry) = frames.iter_mut().find(|entry| entry.1 == Some(index))
        {
            entry.2 += 1;
            continue;
        }
        let Some(frame) = video.shared_frame_at(asset, *time) else {
            continue;
        };
        let valid = frame.pixels.len() == frame.size.0 as usize * frame.size.1 as usize * 4
            && check_size(frame.size).is_ok();
        if !valid || size.is_some_and(|size| size != frame.size) {
            continue;
        }
        size = Some(frame.size);
        // Same allocation (a source returning one cached frame for nearby times).
        if let Some(entry) = frames
            .iter_mut()
            .find(|entry| Arc::ptr_eq(&entry.0.pixels, &frame.pixels))
        {
            entry.2 += 1;
        } else {
            frames.push((frame, index, 1));
        }
    }
    let size = size?;
    if let [(only, ..)] = frames.as_slice() {
        return Some(Frame {
            size,
            pixels: only.pixels.to_vec(),
        });
    }
    let count: u32 = frames.iter().map(|entry| entry.2).sum();
    let total = size.0 as usize * size.1 as usize;
    let mut pixels = vec![0_u8; total * 4];
    // Per pixel: alpha-weighted colour sums and the alpha sum, for one chunk at a time.
    let mut sums = vec![[0_u32; 4]; BLEND_CHUNK_PIXELS.min(total)];
    for (chunk_index, out) in pixels.chunks_mut(BLEND_CHUNK_PIXELS * 4).enumerate() {
        let first = chunk_index * BLEND_CHUNK_PIXELS * 4;
        let sums = &mut sums[..out.len() / 4];
        sums.fill([0; 4]);
        for (frame, _, weight) in &frames {
            let source = &frame.pixels[first..first + out.len()];
            for (sum, pixel) in sums.iter_mut().zip(source.as_chunks::<4>().0) {
                let alpha = u32::from(pixel[3]) * weight;
                sum[0] += u32::from(pixel[0]) * alpha;
                sum[1] += u32::from(pixel[1]) * alpha;
                sum[2] += u32::from(pixel[2]) * alpha;
                sum[3] += alpha;
            }
        }
        for (sum, out) in sums.iter().zip(out.as_chunks_mut::<4>().0) {
            let alpha = sum[3];
            for (total, out) in sum[..3].iter().zip(&mut out[..3]) {
                let value = (total + alpha / 2).checked_div(alpha).unwrap_or(0);
                *out = u8::try_from(value).unwrap_or(u8::MAX);
            }
            out[3] = u8::try_from((alpha + count / 2) / count).unwrap_or(u8::MAX);
        }
    }
    Some(Frame { size, pixels })
}

/// Video stand-in until the media spike supplies a decoder.
/// Generates a labelled checkerboard at each requested media time.
#[derive(Default)]
pub struct PlaceholderFrameSource {
    backend: CpuBackend,
}
impl FrameSource for PlaceholderFrameSource {
    fn frame_at(&mut self, asset: &AssetId, media_time: Time) -> Option<Frame> {
        self.backend.placeholder_frame(asset, media_time).ok()
    }
}

impl FrameSource for animathio_media::MediaPool {
    fn frame_at(&mut self, asset: &AssetId, media_time: Time) -> Option<Frame> {
        let frame = animathio_media::MediaPool::frame_at(self, asset.clone(), media_time).ok()?;
        Some(Frame {
            size: (frame.width, frame.height),
            pixels: frame.pixels.to_vec(),
        })
    }
    fn shared_frame_at(&mut self, asset: &AssetId, media_time: Time) -> Option<SharedFrame> {
        let frame = animathio_media::MediaPool::frame_at(self, asset.clone(), media_time).ok()?;
        Some(SharedFrame {
            size: (frame.width, frame.height),
            pixels: frame.pixels.clone(),
        })
    }
    fn frame_index(&mut self, asset: &AssetId, media_time: Time) -> Option<i64> {
        let rate = self.info(asset)?.video?.frame_rate;
        rate.time_to_frame(media_time, Rounding::Floor).ok()
    }
}

struct AssetFrameSource {
    pool: animathio_media::MediaPool,
    pending: BTreeMap<AssetId, Arc<[u8]>>,
}
impl AssetFrameSource {
    /// Register the asset's bytes with the pool on first use.
    fn open(&mut self, asset: &AssetId) -> bool {
        if let Some(bytes) = self.pending.remove(asset)
            && let Err(error) = self.pool.add_bytes(asset.clone(), bytes)
        {
            eprintln!("video asset {asset:?}: {error}");
            return false;
        }
        true
    }
}
impl FrameSource for AssetFrameSource {
    fn frame_at(&mut self, asset: &AssetId, media_time: Time) -> Option<Frame> {
        self.open(asset)
            .then(|| FrameSource::frame_at(&mut self.pool, asset, media_time))?
    }
    fn shared_frame_at(&mut self, asset: &AssetId, media_time: Time) -> Option<SharedFrame> {
        self.open(asset)
            .then(|| FrameSource::shared_frame_at(&mut self.pool, asset, media_time))?
    }
    fn frame_index(&mut self, asset: &AssetId, media_time: Time) -> Option<i64> {
        self.open(asset)
            .then(|| FrameSource::frame_index(&mut self.pool, asset, media_time))?
    }
}

/// Small domain-only seam shared by the CPU reference and GPU preview backends.
pub trait Backend {
    /// Render back-to-front clips, resolving resources through the engine pool.
    fn render(
        &mut self,
        scene: &Scene,
        pool: &mut ResourcePool,
        video: &mut dyn FrameSource,
        size: Size,
    ) -> Result<Frame, Error>;
}

/// Runtime resources shared by preview and frame-stepped export.
pub struct RenderCtx {
    /// Content-addressed media resources.
    pub resources: ResourcePool,
    backend: Box<dyn Backend>,
    video: Box<dyn FrameSource>,
}
impl Default for RenderCtx {
    fn default() -> Self {
        Self::new(CpuBackend::default(), PlaceholderFrameSource::default())
    }
}
impl RenderCtx {
    /// Build the shared preview/export context from verified encoded assets.
    /// Video decoders open lazily on the render worker; image assets decode through the image pool.
    pub fn from_assets(assets: BTreeMap<AssetId, Vec<u8>>) -> Result<Self, Error> {
        let mut resources = ResourcePool::default();
        let mut pending = BTreeMap::new();
        for (id, bytes) in assets {
            if AssetId::of(&bytes) != id {
                return Err(Error::AssetHash);
            }
            let bytes: Arc<[u8]> = bytes.into();
            resources.encoded.insert(id.clone(), bytes.clone());
            pending.insert(id, bytes);
        }
        let video = AssetFrameSource {
            pool: animathio_media::MediaPool::new(
                Arc::new(animathio_media::FilmCraftBackend::new()),
                animathio_media::MediaPoolConfig::default(),
            ),
            pending,
        };
        let mut ctx = Self::new(CpuBackend::default(), video);
        ctx.resources = resources;
        Ok(ctx)
    }

    /// Install the plugin runner for clip effects, replacing any previous one.
    /// Without a processor, rendering a clip that has effects fails with
    /// [`Error::Effect`]; clips without effects never need one.
    pub fn set_effect_processor(&mut self, processor: impl EffectProcessor + 'static) {
        self.resources.effects = Some(Box::new(processor));
    }

    /// Swap the rendering backend, keeping resources and video decoders.
    /// Returns the previous backend.
    pub fn set_backend(&mut self, backend: Box<dyn Backend>) -> Box<dyn Backend> {
        std::mem::replace(&mut self.backend, backend)
    }

    /// Select an adapter and video source without changing project data.
    #[must_use]
    pub fn new(backend: impl Backend + 'static, video: impl FrameSource + 'static) -> Self {
        Self {
            resources: ResourcePool::default(),
            backend: Box::new(backend),
            video: Box::new(video),
        }
    }
}

/// Rasterize a scene over its project background. Evaluation is shared with preview/export.
/// Rendering is fallible so missing assets and invalid dimensions become diagnostics.
pub fn render(scene: &Scene, ctx: &mut RenderCtx, size: Size) -> Result<Frame, Error> {
    let _timer = animathio_core::timing::Timer::new("render-inclusive");
    check_size(size)?;
    ctx.backend
        .render(scene, &mut ctx.resources, ctx.video.as_mut(), size)
}

/// Extra single-clip renders [`render_preview`] may spend per frame finding which clips
/// and effects still work.
const MAX_EFFECT_PROBES: usize = 8;

/// A preview frame plus the effects that had to be skipped to produce it.
#[derive(Debug)]
pub struct PreviewFrame {
    /// The rendered canvas, with failing effects passed through.
    pub frame: Frame,
    /// Clips whose effect chain failed, with the first error of each.
    pub effect_failures: Vec<(animathio_core::ClipId, String)>,
}

/// Preview-only variant of [`render`]: an effect that cannot run (plugin not loaded,
/// rejected parameters, sandbox violation) is passed through instead of failing the
/// whole frame, and is reported in [`PreviewFrame::effect_failures`] so the editor can
/// warn. Each clip keeps the effects that still work. Export and tests keep using
/// [`render`], which fails with [`Error::Effect`].
pub fn render_preview(
    scene: &Scene,
    ctx: &mut RenderCtx,
    size: Size,
) -> Result<PreviewFrame, Error> {
    let whole = match render(scene, ctx, size) {
        Ok(frame) => {
            return Ok(PreviewFrame {
                frame,
                effect_failures: Vec::new(),
            });
        }
        Err(Error::Effect(message)) => message,
        Err(error) => return Err(error),
    };
    let mut repaired = scene.clone();
    let mut effect_failures = Vec::new();
    // Every probe re-runs plugin code (each call is bounded by the sandbox limits), so a
    // failing frame gets one fixed budget for all single-clip renders instead of work
    // proportional to the number of clips or effects. Once it is spent, nothing more is
    // executed: remaining effect clips are drawn without effects and reported as failed.
    let mut probes_left = MAX_EFFECT_PROBES;
    let alone = |clip: &animathio_core::SceneClip| Scene {
        canvas: scene.canvas,
        background: scene.background,
        clips: vec![clip.clone()],
    };
    for clip in &mut repaired.clips {
        if !clip.has_active_effects() {
            continue;
        }
        if probes_left == 0 {
            clip.effects.clear();
            effect_failures.push((clip.id, whole.clone()));
            continue;
        }
        probes_left -= 1;
        let Err(Error::Effect(first)) = render(&alone(clip), ctx, size) else {
            continue;
        };
        let mut probe = clip.clone();
        probe.effects.clear();
        for instance in std::mem::take(&mut clip.effects) {
            if probes_left == 0 {
                break;
            }
            probes_left -= 1;
            probe.effects.push(instance);
            if matches!(render(&alone(&probe), ctx, size), Err(Error::Effect(_))) {
                probe.effects.pop();
            }
        }
        clip.effects = probe.effects;
        effect_failures.push((clip.id, first));
    }
    if effect_failures.is_empty() {
        // The failure is not attributable to a single clip's chain; do not run it again.
        return Err(Error::Effect(whole));
    }
    render(&repaired, ctx, size).map(|frame| PreviewFrame {
        frame,
        effect_failures,
    })
}

/// Which backend renders frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendKind {
    /// Deterministic Vello CPU reference.
    Cpu,
    /// Vello on wgpu; preview only.
    Gpu,
}
impl BackendKind {
    /// Parse an `ANIMATHIO_RENDER` value (`cpu` or `gpu`, case-insensitive).
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "cpu" => Some(Self::Cpu),
            "gpu" => Some(Self::Gpu),
            _ => None,
        }
    }
}

/// Outcome of backend selection.
pub struct SelectedBackend {
    /// The backend to render with.
    pub backend: Box<dyn Backend>,
    /// What was actually selected.
    pub kind: BackendKind,
    /// Why a GPU request fell back to CPU, if it did.
    pub fallback: Option<Error>,
}

/// Use the GPU when requested and `make_gpu` succeeds; otherwise fall back to CPU and
/// report why.
pub fn select_backend<B: Backend + 'static>(
    requested: BackendKind,
    make_gpu: impl FnOnce() -> Result<B, Error>,
) -> SelectedBackend {
    let cpu = |fallback| SelectedBackend {
        backend: Box::new(CpuBackend::default()),
        kind: BackendKind::Cpu,
        fallback,
    };
    match requested {
        BackendKind::Cpu => cpu(None),
        BackendKind::Gpu => match make_gpu() {
            Ok(gpu) => SelectedBackend {
                backend: Box::new(gpu),
                kind: BackendKind::Gpu,
                fallback: None,
            },
            Err(error) => cpu(Some(error)),
        },
    }
}

/// Preview backend selection with the real [`GpuBackend`].
#[cfg(feature = "gpu")]
#[must_use]
pub fn preview_backend(requested: BackendKind) -> SelectedBackend {
    select_backend(requested, GpuBackend::new)
}

/// Without the `gpu` feature, GPU requests always fall back to CPU.
#[cfg(not(feature = "gpu"))]
#[must_use]
pub fn preview_backend(requested: BackendKind) -> SelectedBackend {
    select_backend(requested, || -> Result<CpuBackend, Error> {
        Err(Error::Gpu("built without the `gpu` feature".into()))
    })
}

#[cfg(test)]
mod selection_tests {
    use super::*;

    #[test]
    fn parses_env_values() {
        assert_eq!(BackendKind::parse("GPU"), Some(BackendKind::Gpu));
        assert_eq!(BackendKind::parse(" cpu "), Some(BackendKind::Cpu));
        assert_eq!(BackendKind::parse("vulkan"), None);
    }

    #[test]
    fn cpu_request_never_builds_gpu() {
        let selected = select_backend(BackendKind::Cpu, || -> Result<CpuBackend, Error> {
            panic!("GPU factory must not run for a CPU request")
        });
        assert_eq!(selected.kind, BackendKind::Cpu);
        assert!(selected.fallback.is_none());
    }

    #[test]
    fn gpu_failure_falls_back_to_cpu_with_reason() {
        let selected = select_backend(BackendKind::Gpu, || -> Result<CpuBackend, Error> {
            Err(Error::Gpu("no GPU adapter".into()))
        });
        assert_eq!(selected.kind, BackendKind::Cpu);
        assert!(matches!(selected.fallback, Some(Error::Gpu(ref m)) if m == "no GPU adapter"));
    }

    #[test]
    fn gpu_success_is_used() {
        // Any backend stands in for a working GPU here.
        let selected = select_backend(BackendKind::Gpu, || Ok(CpuBackend::default()));
        assert_eq!(selected.kind, BackendKind::Gpu);
        assert!(selected.fallback.is_none());
    }
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
mod blend_bench {
    use super::*;
    use animathio_core::{ClipId, Fit, TrackId, Transform};

    /// The previous blender, kept only to measure against: a `Vec<[u32; 4]>` of sums the
    /// size of the frame, and every tap copied out of its source.
    fn legacy(video: &mut dyn FrameSource, asset: &AssetId, clip: &SceneClip) -> Option<Frame> {
        let mut sums: Vec<[u32; 4]> = Vec::new();
        let (mut size, mut count) = ((0, 0), 0_u32);
        for time in &clip.source_taps {
            let frame = video.frame_at(asset, *time)?;
            if count == 0 {
                size = frame.size;
                sums = vec![[0; 4]; frame.pixels.len() / 4];
            }
            for (sum, pixel) in sums.iter_mut().zip(frame.pixels.as_chunks::<4>().0) {
                let alpha = u32::from(pixel[3]);
                sum[0] += u32::from(pixel[0]) * alpha;
                sum[1] += u32::from(pixel[1]) * alpha;
                sum[2] += u32::from(pixel[2]) * alpha;
                sum[3] += alpha;
            }
            count += 1;
        }
        let mut pixels = Vec::with_capacity(sums.len() * 4);
        for sum in &sums {
            let alpha = sum[3];
            for total in &sum[..3] {
                let value = (total + alpha / 2).checked_div(alpha).unwrap_or(0);
                pixels.push(u8::try_from(value).unwrap_or(u8::MAX));
            }
            pixels.push(u8::try_from((alpha + count / 2) / count).unwrap_or(u8::MAX));
        }
        Some(Frame { size, pixels })
    }

    /// Distinct frames of one size held in `Arc`s, like the media pool's cache.
    struct Pool {
        size: Size,
        frames: Vec<Arc<[u8]>>,
    }
    impl Pool {
        fn new(size: Size, count: u8) -> Self {
            let bytes = size.0 as usize * size.1 as usize * 4;
            Self {
                size,
                frames: (0..count)
                    .map(|n| {
                        (0..bytes)
                            .map(|i| (i as u8).wrapping_mul(7).wrapping_add(n.wrapping_mul(13)))
                            .collect::<Vec<u8>>()
                            .into()
                    })
                    .collect(),
            }
        }
    }
    impl FrameSource for Pool {
        fn frame_at(&mut self, _: &AssetId, time: Time) -> Option<Frame> {
            Some(Frame {
                size: self.size,
                pixels: self.frames[time.0 as usize].to_vec(),
            })
        }
        fn shared_frame_at(&mut self, _: &AssetId, time: Time) -> Option<SharedFrame> {
            Some(SharedFrame {
                size: self.size,
                pixels: self.frames[time.0 as usize].clone(),
            })
        }
    }

    fn clip() -> SceneClip {
        SceneClip {
            track: TrackId(0),
            id: ClipId(0),
            kind: ClipKind::Error(String::new()),
            source_time: Time(0),
            source_taps: (0..16).map(Time).collect(),
            transform: Transform::default(),
            fit: Fit::default(),
            params: BTreeMap::new(),
            effects: Vec::new(),
            transition: None,
        }
    }

    #[test]
    fn blending_matches_the_previous_implementation_bit_for_bit() {
        // A size that is not a multiple of the accumulator chunk, so the last chunk is short.
        let mut pool = Pool::new((203, 97), 16);
        let (clip, asset) = (clip(), AssetId::of(b"blend"));
        let old = legacy(&mut pool, &asset, &clip).unwrap();
        let new = video_frame(&mut pool, &asset, &clip).unwrap();
        assert_eq!(old.size, new.size);
        assert_eq!(old.pixels, new.pixels);
    }

    /// `cargo test -p animathio-render --release --lib -- --ignored --nocapture blend_cost`
    #[test]
    #[ignore = "timing measurement"]
    fn blend_cost_of_one_16x_1080p_output_frame() {
        let (clip, asset) = (clip(), AssetId::of(b"bench"));
        let mut pool = Pool::new((1920, 1080), 16);
        let time = |f: &mut dyn FnMut()| {
            f(); // warm up
            let start = std::time::Instant::now();
            for _ in 0..5 {
                f();
            }
            start.elapsed() / 5
        };
        let old = time(&mut || {
            std::hint::black_box(legacy(&mut pool, &asset, &clip));
        });
        let new = time(&mut || {
            std::hint::black_box(video_frame(&mut pool, &asset, &clip));
        });
        eprintln!("blend 16 taps at 1080p: previous {old:?}, now {new:?}");
    }
}
