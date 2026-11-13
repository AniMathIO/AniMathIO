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

use animathio_core::{AssetId, ClipKind, EffectProcessor, Project, Scene, Time};
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

/// Random-access video adapter. Results must depend only on asset and media time.
/// Implementations own their decoder resources and must not advance a playback clock.
pub trait FrameSource {
    /// Return the frame due at this exact source-local tick position, if available.
    fn frame_at(&mut self, asset: &AssetId, media_time: Time) -> Option<Frame>;
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
}

struct AssetFrameSource {
    pool: animathio_media::MediaPool,
    pending: BTreeMap<AssetId, Arc<[u8]>>,
}
impl FrameSource for AssetFrameSource {
    fn frame_at(&mut self, asset: &AssetId, media_time: Time) -> Option<Frame> {
        if let Some(bytes) = self.pending.remove(asset)
            && let Err(error) = self.pool.add_bytes(asset.clone(), bytes)
        {
            eprintln!("video asset {asset:?}: {error}");
            return None;
        }
        FrameSource::frame_at(&mut self.pool, asset, media_time)
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

/// Rasterize a scene onto a transparent canvas. Evaluation is shared with preview/export.
/// Rendering is fallible so missing assets and invalid dimensions become diagnostics.
pub fn render(scene: &Scene, ctx: &mut RenderCtx, size: Size) -> Result<Frame, Error> {
    check_size(size)?;
    ctx.backend
        .render(scene, &mut ctx.resources, ctx.video.as_mut(), size)
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
