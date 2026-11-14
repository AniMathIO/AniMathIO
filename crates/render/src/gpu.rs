//! Vello GPU (wgpu compute) backend for interactive preview.
//!
//! It maps exactly the same scene items as [`CpuBackend`], which stays the reference
//! for tests and export. The device, queue, Vello renderer, scene encoding and the
//! target texture/readback buffer are reused across frames. Clips with effects are
//! rasterized on the CPU, filtered by the effect plugins (the same `EffectProcessor` path
//! the CPU backend uses) and composited as a full-canvas image; WGSL effect ports are a
//! follow-up.

use crate::cpu::{
    ShapedRun, check_vector_font_size, pixel_transform, relative_text_pixel_size, unit_transform,
    vector_path, vector_stroke,
};
use crate::{Backend, CpuBackend, Error, Frame, FrameSource, ResourcePool, Size, check_size};
use animathio_core::{
    AssetId, ClipKind, Color as CoreColor, Scene, SceneClip, VectorElement, VectorScene,
};
use std::{collections::BTreeMap, num::NonZeroUsize, sync::Arc, time::Duration};
use vello::{
    AaConfig, AaSupport, Glyph, RenderParams, Renderer, RendererOptions, Scene as VelloScene,
    kurbo::{Affine, Rect},
    peniko::{
        Blob, Color as VelloColor, Fill, ImageAlphaType, ImageBrush, ImageData, ImageFormat, Mix,
    },
    wgpu,
};

/// Reused render target: storage texture plus a row-padded readback buffer.
struct Target {
    size: Size,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    readback: wgpu::Buffer,
    padded_row: u32,
}

/// Headless Vello GPU backend. Construction fails with [`Error::Gpu`] when no adapter
/// or device is available, so callers can fall back to [`CpuBackend`].
pub struct GpuBackend {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Renderer,
    scene: VelloScene,
    target: Option<Target>,
    /// Text shaping, placeholder video frames and the CPU effect path.
    cpu: CpuBackend,
    /// Stable blobs for still images so the Vello image cache can key on them.
    images: BTreeMap<AssetId, (Arc<Frame>, ImageData)>,
    adapter: String,
}

impl GpuBackend {
    /// Create a headless device and Vello renderer.
    pub fn new() -> Result<Self, Error> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(|e| Error::Gpu(format!("no GPU adapter: {e}")))?;
        let info = adapter.get_info();
        // Software rasterizers (WARP / Microsoft Basic Render Driver, llvmpipe)
        // gain nothing over the CPU backend and the Vello compute pipeline
        // crashes on some of them (access violation on Windows runners), so
        // treat them as "no GPU" and let callers fall back to `CpuBackend`.
        if info.device_type == wgpu::DeviceType::Cpu {
            return Err(Error::Gpu(format!(
                "software adapter {} ({:?}) is not used for GPU preview",
                info.name, info.backend
            )));
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("animathio-render"),
            ..Default::default()
        }))
        .map_err(|e| Error::Gpu(format!("GPU device creation failed: {e}")))?;
        let renderer = Renderer::new(
            &device,
            RendererOptions {
                antialiasing_support: AaSupport::area_only(),
                num_init_threads: NonZeroUsize::new(1),
                ..Default::default()
            },
        )
        .map_err(|e| Error::Gpu(format!("Vello renderer creation failed: {e}")))?;
        Ok(Self {
            device,
            queue,
            renderer,
            scene: VelloScene::new(),
            target: None,
            cpu: CpuBackend::default(),
            images: BTreeMap::new(),
            adapter: format!("{} ({:?})", info.name, info.backend),
        })
    }

    /// Human-readable adapter name and graphics API.
    #[must_use]
    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    fn target(&mut self, size: Size) -> Result<&Target, Error> {
        let max = self.device.limits().max_texture_dimension_2d;
        if size.0 > max || size.1 > max {
            return Err(Error::Gpu(format!(
                "frame {}x{} exceeds GPU texture limit {max}",
                size.0, size.1
            )));
        }
        if self.target.as_ref().is_none_or(|t| t.size != size) {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("animathio frame"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let padded_row = (size.0 * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
            let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("animathio readback"),
                size: u64::from(padded_row) * u64::from(size.1),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            self.target = Some(Target {
                size,
                texture,
                view,
                readback,
                padded_row,
            });
        }
        Ok(self.target.as_ref().expect("target created above"))
    }

    fn encode(
        &mut self,
        scene: &Scene,
        pool: &mut ResourcePool,
        video: &mut dyn FrameSource,
        size: Size,
    ) -> Result<(), Error> {
        self.scene.reset();
        let canvas = Rect::new(0.0, 0.0, f64::from(size.0), f64::from(size.1));
        for clip in &scene.clips {
            if matches!(clip.kind, ClipKind::Audio(_)) || clip.transform.opacity == 0.0 {
                continue;
            }
            if clip.transform.scale_x == 0.0 || clip.transform.scale_y == 0.0 {
                continue;
            }
            let transform = match &clip.kind {
                ClipKind::Vector(scene)
                    if scene.coordinate_space == animathio_core::CoordinateSpace::Canvas =>
                {
                    pixel_transform(clip.transform, size)?
                }
                _ => unit_transform(clip.transform, size)?,
            };
            #[allow(clippy::cast_possible_truncation)] // Core validates opacity in [0, 1].
            let opacity = clip.transform.opacity as f32;
            self.scene.push_layer(
                Fill::NonZero,
                Mix::Normal,
                opacity,
                Affine::IDENTITY,
                &canvas,
            );
            if clip.effects.is_empty() {
                self.draw_clip(transform, size, clip, pool, video)?;
            } else {
                // Effects run through the plugin processor on CPU pixels for now (WGSL
                // ports are a follow-up): rasterize the clip alone, filter it, composite it.
                let mut layer = self
                    .cpu
                    .render_effect_layer(clip, pool, video, size, transform)?;
                crate::effects::apply_chain(&mut layer, clip, pool)?;
                let image = image_data(Arc::new(layer))?;
                self.scene
                    .draw_image(&ImageBrush::new(image), Affine::IDENTITY);
            }
            self.scene.pop_layer();
        }
        Ok(())
    }

    fn draw_clip(
        &mut self,
        t: Affine,
        size: Size,
        clip: &SceneClip,
        pool: &mut ResourcePool,
        video: &mut dyn FrameSource,
    ) -> Result<(), Error> {
        match &clip.kind {
            ClipKind::Image(id) => {
                let frame = pool.image(id)?;
                let image = match self.images.get(id) {
                    Some((cached, image)) if Arc::ptr_eq(cached, &frame) => image.clone(),
                    _ => {
                        let image = image_data(frame.clone())?;
                        self.images.insert(id.clone(), (frame, image.clone()));
                        image
                    }
                };
                self.draw_unit_image(t, image);
            }
            ClipKind::Video(id) => {
                if let Some(frame) = video.frame_at(id, clip.source_time) {
                    let image = image_data(Arc::new(frame))?;
                    self.draw_unit_image(t, image);
                } else {
                    let label =
                        format!("VIDEO {}\n{} ticks", &id.as_str()[..8], clip.source_time.0);
                    self.placeholder(t, size, &label)?;
                }
            }
            ClipKind::Plugin { plugin_id, .. } => {
                self.placeholder(t, size, &format!("PLUGIN\n{plugin_id}"))?;
            }
            ClipKind::Vector(scene) => self.vector_scene(t, size, scene)?,
            ClipKind::Error(message) => {
                self.placeholder(t, size, &format!("ERROR\n{message}"))?;
            }
            ClipKind::Audio(_) => {}
        }
        Ok(())
    }

    fn draw_unit_image(&mut self, t: Affine, image: ImageData) {
        let scale =
            Affine::scale_non_uniform(1.0 / f64::from(image.width), 1.0 / f64::from(image.height));
        self.scene.draw_image(&ImageBrush::new(image), t * scale);
    }

    fn glyphs(&mut self, t: Affine, runs: &[ShapedRun], fill: VelloColor) {
        for run in runs {
            self.scene
                .draw_glyphs(&run.font)
                .font_size(run.font_size)
                .transform(t)
                .hint(false)
                .brush(fill)
                .draw(
                    Fill::NonZero,
                    run.glyphs.iter().map(|&(id, x, y)| Glyph { id, x, y }),
                );
        }
    }

    #[allow(clippy::cast_possible_truncation)] // Pixel-space font dimensions are bounded.
    fn text(
        &mut self,
        t: Affine,
        size: Size,
        text: &str,
        font_size: f64,
        fill: [u8; 4],
    ) -> Result<(), Error> {
        let font_size = relative_text_pixel_size(font_size, size)?;
        let runs = self.cpu.shape(text, font_size as f32, size)?;
        let t = t * Affine::scale_non_uniform(1.0 / f64::from(size.0), 1.0 / f64::from(size.1));
        self.glyphs(t, &runs, color(fill));
        Ok(())
    }

    #[allow(clippy::cast_possible_truncation)] // Pixel-space font dimensions are bounded.
    fn vector_scene(&mut self, t: Affine, size: Size, scene: &VectorScene) -> Result<(), Error> {
        for element in &scene.elements {
            match element {
                VectorElement::FillPath { path, color } => {
                    self.scene.fill(
                        Fill::NonZero,
                        t,
                        color_core(*color),
                        None,
                        &vector_path(path),
                    );
                }
                VectorElement::StrokePath { path, style } => {
                    self.scene.stroke(
                        &vector_stroke(style),
                        t,
                        color_core(style.color),
                        None,
                        &vector_path(path),
                    );
                }
                VectorElement::TextRun(text) => {
                    if scene.coordinate_space == animathio_core::CoordinateSpace::Normalized {
                        let font_size = relative_text_pixel_size(text.font_size, size)?;
                        let runs = self.cpu.shape_centered(
                            &text.text,
                            font_size as f32,
                            size,
                            text.align,
                        )?;
                        self.glyphs(
                            t * Affine::translate((text.x, text.y))
                                * Affine::scale_non_uniform(
                                    1.0 / f64::from(size.0),
                                    1.0 / f64::from(size.1),
                                ),
                            &runs,
                            color_core(text.color),
                        );
                        continue;
                    }
                    check_vector_font_size(text.font_size)?;
                    let runs = self.cpu.shape_aligned(
                        &text.text,
                        text.font_size as f32,
                        size,
                        text.align,
                    )?;
                    self.glyphs(
                        t * Affine::translate((text.x, text.y)),
                        &runs,
                        color_core(text.color),
                    );
                }
            }
        }
        Ok(())
    }

    fn placeholder(&mut self, t: Affine, size: Size, label: &str) -> Result<(), Error> {
        for y in 0..6 {
            for x in 0..10 {
                let shade = if (x + y) % 2 == 0 { 70 } else { 110 };
                self.scene.fill(
                    Fill::NonZero,
                    t,
                    color([shade, shade, shade, 255]),
                    None,
                    &Rect::new(
                        f64::from(x) / 10.0,
                        f64::from(y) / 6.0,
                        f64::from(x + 1) / 10.0,
                        f64::from(y + 1) / 6.0,
                    ),
                );
            }
        }
        self.scene.fill(
            Fill::NonZero,
            t,
            color([0, 0, 0, 200]),
            None,
            &Rect::new(0.0, 0.25, 1.0, 0.75),
        );
        self.text(
            t * Affine::translate((0.04, 0.3)),
            size,
            label,
            0.12,
            [255; 4],
        )
    }

    fn readback(&mut self, size: Size) -> Result<Frame, Error> {
        let target = self.target.as_ref().expect("target prepared before render");
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            target.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &target.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(target.padded_row),
                    rows_per_image: Some(size.1),
                },
            },
            target.texture.size(),
        );
        self.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        target
            .readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(10)),
            })
            .map_err(|e| Error::Gpu(format!("GPU poll failed: {e}")))?;
        rx.recv()
            .map_err(|_| Error::Gpu("readback callback dropped".into()))?
            .map_err(|e| Error::Gpu(format!("readback map failed: {e}")))?;
        let row = size.0 as usize * 4;
        let mut pixels = Vec::with_capacity(row * size.1 as usize);
        {
            let mapped = target
                .readback
                .slice(..)
                .get_mapped_range()
                .map_err(|e| Error::Gpu(format!("readback range failed: {e}")))?;
            for line in mapped.chunks_exact(target.padded_row as usize) {
                pixels.extend_from_slice(&line[..row]);
            }
        }
        target.readback.unmap();
        Ok(Frame { size, pixels })
    }
}

impl Backend for GpuBackend {
    fn render(
        &mut self,
        scene: &Scene,
        pool: &mut ResourcePool,
        video: &mut dyn FrameSource,
        size: Size,
    ) -> Result<Frame, Error> {
        check_size(size)?;
        // Transitions composite on the CPU path for now (GPU port is a follow-up),
        // so frames inside a transition stay identical to export.
        if scene.clips.iter().any(|clip| clip.transition.is_some()) {
            return self.cpu.render(scene, pool, video, size);
        }
        self.target(size)?;
        self.encode(scene, pool, video, size)?;
        let target = self.target.as_ref().expect("target prepared above");
        self.renderer
            .render_to_texture(
                &self.device,
                &self.queue,
                &self.scene,
                &target.view,
                &RenderParams {
                    base_color: VelloColor::TRANSPARENT,
                    width: size.0,
                    height: size.1,
                    antialiasing_method: AaConfig::Area,
                },
            )
            .map_err(|e| Error::Gpu(format!("Vello render failed: {e}")))?;
        self.readback(size)
    }
}

/// Wrap straight-alpha RGBA8 pixels without copying them.
fn image_data(frame: Arc<Frame>) -> Result<ImageData, Error> {
    struct Pixels(Arc<Frame>);
    impl AsRef<[u8]> for Pixels {
        fn as_ref(&self) -> &[u8] {
            &self.0.pixels
        }
    }
    frame.validate()?;
    let (width, height) = frame.size;
    Ok(ImageData {
        data: Blob::new(Arc::new(Pixels(frame))),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::Alpha,
        width,
        height,
    })
}

fn color(value: [u8; 4]) -> VelloColor {
    VelloColor::from_rgba8(value[0], value[1], value[2], value[3])
}

fn color_core(value: CoreColor) -> VelloColor {
    color([value.r, value.g, value.b, value.a])
}
