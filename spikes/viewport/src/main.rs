use anyhow::{Context as _, Result};
use gpui::{prelude::*, *};
use skrifa::{MetadataProvider, raw::FontRef};
use std::{
    num::NonZeroUsize,
    sync::Arc,
    time::{Duration, Instant},
};
use vello::{
    AaConfig, AaSupport, Renderer, RendererOptions, Scene,
    kurbo::{Affine, Circle, Rect, Stroke},
    peniko::{Blob, Color, Fill, FontData, Gradient},
};

const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
const TICKS_PER_SECOND: i64 = 60_000;
const DURATION: i64 = 10 * TICKS_PER_SECOND;

struct Pipeline {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Renderer,
    texture: wgpu::Texture,
    readback: wgpu::Buffer,
    scene: Scene,
    font: FontData,
}
impl Pipeline {
    fn new() -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))?;
        println!("adapter: {:?}", adapter.get_info());
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;
        let renderer = Renderer::new(
            &device,
            RendererOptions {
                antialiasing_support: AaSupport::area_only(),
                num_init_threads: NonZeroUsize::new(1),
                ..Default::default()
            },
        )?;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("vello 1080p"),
            size: wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        // 1920*4 is already aligned to wgpu's 256-byte row requirement.
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: u64::from(WIDTH * HEIGHT * 4),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let font_path = std::env::var("VIEWPORT_FONT")
            .unwrap_or_else(|_| "/usr/share/fonts/noto/NotoSans-Regular.ttf".into());
        let font = FontData::new(
            Blob::new(Arc::new(std::fs::read(&font_path).with_context(|| {
                format!("font {font_path}; set VIEWPORT_FONT")
            })?)),
            0,
        );
        Ok(Self {
            device,
            queue,
            renderer,
            texture,
            readback,
            scene: Scene::new(),
            font,
        })
    }
    fn frame(&mut self, ticks: i64) -> Result<Arc<RenderImage>> {
        // Floating point is used only for drawing, never for timeline state.
        let t = ticks as f64 / TICKS_PER_SECOND as f64;
        self.scene.reset();
        let gradient = Gradient::new_linear((t.sin() * 500., 0.), (1400. + t.cos() * 500., 1080.))
            .with_stops([
                Color::from_rgb8(15, 25, 65),
                Color::from_rgb8(125, 30, 110),
                Color::from_rgb8(10, 160, 180),
            ]);
        self.scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            &gradient,
            None,
            &Rect::new(0., 0., 1920., 1080.),
        );
        for i in 0..24 {
            let a = t * 1.7 + i as f64 * 0.27;
            let x = 960. + a.cos() * (180. + i as f64 * 22.);
            let y = 540. + a.sin() * 310.;
            self.scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                Color::from_rgba8(240, 180, 60, 180),
                None,
                &Circle::new((x, y), 20. + i as f64),
            );
        }
        self.scene.stroke(
            &Stroke::new(9.),
            Affine::rotate_about(t, (960., 540.)),
            Color::WHITE,
            None,
            &Rect::new(620., 300., 1300., 780.),
        );
        let label = format!("AniMathIO / Vello GPU / 1920 x 1080 / t = {t:.3}");
        let font_ref = FontRef::from_index(self.font.data.as_ref(), 0)?;
        let charmap = font_ref.charmap();
        let metrics = font_ref.glyph_metrics(
            skrifa::instance::Size::new(48.),
            skrifa::instance::LocationRef::default(),
        );
        let mut x = 70.;
        let glyphs: Vec<_> = label
            .chars()
            .map(|c| {
                let id = charmap.map(c).unwrap_or_default();
                let glyph = vello::Glyph {
                    id: id.to_u32(),
                    x,
                    y: 120.,
                };
                x += metrics.advance_width(id).unwrap_or(24.);
                glyph
            })
            .collect();
        self.scene
            .draw_glyphs(&self.font)
            .font_size(48.)
            .brush(Color::WHITE)
            .draw(Fill::NonZero, glyphs.into_iter());
        self.renderer.render_to_texture(
            &self.device,
            &self.queue,
            &self.scene,
            &self.texture.create_view(&Default::default()),
            &vello::RenderParams {
                base_color: Color::BLACK,
                width: WIDTH,
                height: HEIGHT,
                antialiasing_method: AaConfig::Area,
            },
        )?;
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(WIDTH * 4),
                    rows_per_image: Some(HEIGHT),
                },
            },
            self.texture.size(),
        );
        self.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        self.readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });
        self.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(10)),
        })?;
        rx.recv()??;
        let mut pixels = self.readback.slice(..).get_mapped_range()?.to_vec();
        self.readback.unmap();
        // GPUI's RenderImage expects BGRA, despite image::RgbaImage's name.
        for p in pixels.as_chunks_mut::<4>().0 {
            p.swap(0, 2);
        }
        let image = image::RgbaImage::from_raw(WIDTH, HEIGHT, pixels).unwrap();
        Ok(Arc::new(RenderImage::new([image::Frame::new(image)])))
    }
}

#[derive(Default)]
struct Samples {
    cadence: Vec<f64>,
    pipeline: Vec<f64>,
}
fn percentiles(values: &[f64]) -> (f64, f64) {
    let mut values = values.to_vec();
    values.sort_by(f64::total_cmp);
    if values.is_empty() {
        return (0., 0.);
    }
    (
        values[((values.len() - 1) as f64 * 0.50).ceil() as usize],
        values[((values.len() - 1) as f64 * 0.95).ceil() as usize],
    )
}
impl Samples {
    fn report(&self, elapsed: Duration) {
        let (p50, p95) = percentiles(&self.cadence);
        let (r50, r95) = percentiles(&self.pipeline);
        let misses = self.cadence.iter().filter(|&&x| x > 1000. / 60.).count();
        println!(
            "BENCH random_scrub seed=0x123456789abcdef warmup=2s requested=10s actual={:.3}s samples={} fps={:.2} cadence_p50_ms={p50:.3} cadence_p95_ms={p95:.3} pipeline_p50_ms={r50:.3} pipeline_p95_ms={r95:.3} over_16.667ms={misses}",
            elapsed.as_secs_f64(),
            self.cadence.len(),
            self.cadence.len() as f64 / elapsed.as_secs_f64()
        );
    }
}
struct Viewport {
    pipeline: Pipeline,
    image: Option<Arc<RenderImage>>,
    playing: bool,
    dirty: bool,
    ticks: i64,
    previous: Instant,
    started: Instant,
    bench: bool,
    random: u64,
    samples: Samples,
    recent: Vec<f64>,
    last_pipeline_ms: f64,
    slider: Bounds<Pixels>,
}
impl Viewport {
    fn seek(&mut self, x: Pixels, cx: &mut Context<Self>) {
        let fraction =
            (f32::from(x - self.slider.origin.x) / f32::from(self.slider.size.width)).clamp(0., 1.);
        self.ticks = (fraction as f64 * DURATION as f64) as i64;
        self.playing = false;
        self.dirty = true;
        cx.notify();
    }
}
impl Render for Viewport {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        let interval = now.duration_since(self.previous);
        let cadence_ms = interval.as_secs_f64() * 1000.;
        let elapsed = now.duration_since(self.started);
        self.previous = now;
        if self.bench && elapsed >= Duration::from_secs(12) {
            self.samples.report(elapsed - Duration::from_secs(2));
            cx.quit();
        }
        if self.bench {
            self.random ^= self.random << 13;
            self.random ^= self.random >> 7;
            self.random ^= self.random << 17;
            self.ticks = (self.random % DURATION as u64) as i64;
        } else if self.playing {
            self.ticks = (self.ticks
                + (interval.as_nanos() * TICKS_PER_SECOND as u128 / 1_000_000_000) as i64)
                % DURATION;
        }
        if self.bench || self.playing || self.dirty || self.image.is_none() {
            let start = Instant::now();
            let next = self.pipeline.frame(self.ticks).expect("render frame");
            // Explicit eviction prevents an ever-growing 8 MB/frame sprite atlas.
            if let Some(old) = self.image.replace(next) {
                window.drop_image(old).expect("evict frame");
            }
            self.dirty = false;
            self.last_pipeline_ms = start.elapsed().as_secs_f64() * 1000.;
        }
        if self.bench && elapsed >= Duration::from_secs(2) && elapsed < Duration::from_secs(12) {
            self.samples.cadence.push(cadence_ms);
            self.samples.pipeline.push(self.last_pipeline_ms);
        }
        self.recent.push(cadence_ms);
        if self.recent.len() > 60 {
            self.recent.remove(0);
        }
        let average = self.recent.iter().sum::<f64>() / self.recent.len() as f64;
        if self.bench || self.playing {
            window.request_animation_frame();
        }
        let image = self.image.clone().unwrap();
        let fraction = self.ticks as f32 / DURATION as f32;
        let weak = cx.entity().downgrade();
        div().size_full().flex().flex_col().p_4().gap_3().bg(rgb(0x131927)).text_color(rgb(0xffffff))
            .child(format!("1080p Vello → wgpu readback → GPUI | {:.1} fps | cadence {:.2} ms | pipeline {:.2} ms", 1000./average, average, self.last_pipeline_ms))
            .child(img(image).w(px(1920.)).h(px(1080.)).object_fit(ObjectFit::Contain))
            .child(div().id("play").cursor_pointer().on_click(cx.listener(|this,_,_,cx| {this.playing = !this.playing;
                    this.previous = Instant::now(); cx.notify();})).child(if self.playing {"Pause"} else {"Play"}))
            .child(div().id("timeline").w(px(1920.)).h(px(28.)).cursor_pointer()
                .on_mouse_down(MouseButton::Left, cx.listener(|this,event: &MouseDownEvent,_,cx| this.seek(event.position.x,cx)))
                .on_mouse_move(cx.listener(|this,event: &MouseMoveEvent,_,cx| {if event.dragging() {this.seek(event.position.x,cx)}}))
                .child(canvas(move |bounds,_,cx| { let _ = weak.update(cx, |this,_| this.slider = bounds); }, move |bounds,_,window,_| {
                    window.paint_quad(fill(bounds, rgb(0x38435b)));
                    window.paint_quad(fill(Bounds::new(bounds.origin, size(bounds.size.width*fraction,bounds.size.height)),rgb(0x37cbbb)));
                }).size_full()))
            .child(format!("{:.3} / 10.000 s — click or drag timeline to scrub", self.ticks as f64/TICKS_PER_SECOND as f64))
    }
}
fn main() -> Result<()> {
    let bench = std::env::args().any(|a| a == "--bench");
    let pipeline = Pipeline::new()?;
    Application::new().run(move |cx: &mut App| {
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let bounds = Bounds::centered(None, size(px(1960.), px(1280.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            move |_, cx| {
                cx.new(|_| Viewport {
                    pipeline,
                    image: None,
                    playing: true,
                    dirty: true,
                    ticks: 0,
                    previous: Instant::now(),
                    started: Instant::now(),
                    bench,
                    random: 0x123456789abcdef,
                    samples: Samples::default(),
                    recent: Vec::new(),
                    last_pipeline_ms: 0.,
                    slider: Bounds::default(),
                })
            },
        )
        .expect("GPUI window");
        cx.activate(true);
    });
    Ok(())
}
