use std::fs;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use lru::LruCache;
use wasmtime::component::{Component, HasSelf, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

wasmtime::component::bindgen!({
    path: "../wit",
    world: "plugin",
});

use animathio::plugin::host::Host;
use animathio::plugin::types::{self as wit_types, DrawCommand, ParamEntry, ParamValue, PathVerb};

const FONT_PATH: &str = "/usr/share/fonts/noto/NotoSans-Regular.ttf";
const FALLBACK_FONT_PATH: &str = "/usr/share/fonts/TTF/DejaVuSans.ttf";

struct AppLimiter {
    max_memory_bytes: usize,
}

impl wasmtime::ResourceLimiter for AppLimiter {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> Result<bool, wasmtime::Error> {
        Ok(desired <= self.max_memory_bytes)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> Result<bool, wasmtime::Error> {
        Ok(desired <= 50_000)
    }
}

struct HostState {
    table: ResourceTable,
    wasi: WasiCtx,
    font_data: Vec<u8>,
    limiter: AppLimiter,
    logs: Vec<String>,
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl animathio::plugin::types::Host for HostState {}

impl Host for HostState {
    fn shape_text(&mut self, text: String, font_size: f32) -> Vec<Vec<PathVerb>> {
        shape_text_skrifa(&self.font_data, &text, font_size)
    }

    fn log(&mut self, message: String) {
        self.logs.push(message);
    }
}

fn load_font_bytes() -> Vec<u8> {
    if let Ok(bytes) = fs::read(FONT_PATH) {
        return bytes;
    }
    if let Ok(bytes) = fs::read(FALLBACK_FONT_PATH) {
        return bytes;
    }
    eprintln!("[warn] No system font found; text shaping will produce empty contours");
    Vec::new()
}

fn shape_text_skrifa(font_bytes: &[u8], text: &str, font_size: f32) -> Vec<Vec<PathVerb>> {
    if font_bytes.is_empty() || text.is_empty() {
        return Vec::new();
    }

    let Ok(font) = skrifa::FontRef::new(font_bytes) else {
        return Vec::new();
    };

    use skrifa::MetadataProvider;
    let outlines = font.outline_glyphs();
    let charmap = font.charmap();
    let metrics = font.glyph_metrics(
        skrifa::instance::Size::new(font_size),
        skrifa::instance::LocationRef::default(),
    );

    struct OutlineCollector {
        paths: Vec<Vec<PathVerb>>,
        current: Vec<PathVerb>,
        cursor_x: f32,
    }

    impl skrifa::outline::OutlinePen for OutlineCollector {
        fn move_to(&mut self, x: f32, y: f32) {
            if !self.current.is_empty() {
                self.paths.push(std::mem::take(&mut self.current));
            }
            // Font Y coordinates go up, standard screen coordinates go down
            self.current.push(PathVerb::MoveTo(wit_types::Point {
                x: self.cursor_x + x,
                y: -y,
            }));
        }

        fn line_to(&mut self, x: f32, y: f32) {
            self.current.push(PathVerb::LineTo(wit_types::Point {
                x: self.cursor_x + x,
                y: -y,
            }));
        }

        fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
            self.current.push(PathVerb::QuadTo((
                wit_types::Point {
                    x: self.cursor_x + cx0,
                    y: -cy0,
                },
                wit_types::Point {
                    x: self.cursor_x + x,
                    y: -y,
                },
            )));
        }

        fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
            self.current.push(PathVerb::CubicTo((
                wit_types::Point {
                    x: self.cursor_x + cx0,
                    y: -cy0,
                },
                wit_types::Point {
                    x: self.cursor_x + cx1,
                    y: -cy1,
                },
                wit_types::Point {
                    x: self.cursor_x + x,
                    y: -y,
                },
            )));
        }

        fn close(&mut self) {
            self.current.push(PathVerb::Close);
            if !self.current.is_empty() {
                self.paths.push(std::mem::take(&mut self.current));
            }
        }
    }

    let mut collector = OutlineCollector {
        paths: Vec::new(),
        current: Vec::new(),
        cursor_x: 0.0,
    };

    for ch in text.chars() {
        if ch == ' ' {
            collector.cursor_x += font_size * 0.32;
            continue;
        }

        let glyph_id = charmap.map(ch).unwrap_or_default();
        if let Some(glyph) = outlines.get(glyph_id) {
            let draw_settings = skrifa::outline::DrawSettings::unhinted(
                skrifa::instance::Size::new(font_size),
                skrifa::instance::LocationRef::default(),
            );
            let _ = glyph.draw(draw_settings, &mut collector);
        }
        let advance = metrics.advance_width(glyph_id).unwrap_or(font_size * 0.6);
        collector.cursor_x += advance;
    }

    if !collector.current.is_empty() {
        collector.paths.push(collector.current);
    }

    collector.paths
}

fn path_to_kurbo(verbs: &[PathVerb]) -> kurbo::BezPath {
    let mut path = kurbo::BezPath::new();
    for v in verbs {
        match v {
            PathVerb::MoveTo(p) => path.move_to((p.x as f64, p.y as f64)),
            PathVerb::LineTo(p) => path.line_to((p.x as f64, p.y as f64)),
            PathVerb::QuadTo((c, to)) => {
                path.quad_to((c.x as f64, c.y as f64), (to.x as f64, to.y as f64))
            }
            PathVerb::CubicTo((c1, c2, to)) => path.curve_to(
                (c1.x as f64, c1.y as f64),
                (c2.x as f64, c2.y as f64),
                (to.x as f64, to.y as f64),
            ),
            PathVerb::Close => path.close_path(),
        }
    }
    path
}

fn render_scene_to_rgba(
    scene: &Scene,
    width: u32,
    height: u32,
    font_bytes: &[u8],
) -> Vec<u8> {
    use vello_cpu::{Pixmap, RenderContext, Resources};

    let w16 = width.min(u16::MAX as u32) as u16;
    let h16 = height.min(u16::MAX as u32) as u16;
    let mut cx = RenderContext::new(w16, h16);
    let mut resources = Resources::new();

    // Dark slate background typical of video editor preview
    let bg_rect = kurbo::Rect::new(0.0, 0.0, width as f64, height as f64);
    cx.set_paint(peniko::Color::from_rgba8(24, 26, 32, 255));
    cx.fill_rect(&bg_rect);

    for cmd in &scene.elements {
        match cmd {
            DrawCommand::FillPath((verbs, color)) => {
                let bez = path_to_kurbo(verbs);
                cx.set_paint(peniko::Color::from_rgba8(color.r, color.g, color.b, color.a));
                cx.fill_path(&bez);
            }
            DrawCommand::StrokePath((verbs, stroke_style)) => {
                let bez = path_to_kurbo(verbs);
                let cap = match stroke_style.cap {
                    wit_types::StrokeCap::Butt => kurbo::Cap::Butt,
                    wit_types::StrokeCap::Round => kurbo::Cap::Round,
                    wit_types::StrokeCap::Square => kurbo::Cap::Square,
                };
                let join = match stroke_style.join {
                    wit_types::StrokeJoin::Miter => kurbo::Join::Miter,
                    wit_types::StrokeJoin::Round => kurbo::Join::Round,
                    wit_types::StrokeJoin::Bevel => kurbo::Join::Bevel,
                };
                let stroke = kurbo::Stroke::new(stroke_style.width as f64)
                    .with_caps(cap)
                    .with_join(join);

                cx.set_paint(peniko::Color::from_rgba8(
                    stroke_style.color.r,
                    stroke_style.color.g,
                    stroke_style.color.b,
                    stroke_style.color.a,
                ));
                cx.set_stroke(stroke);
                cx.stroke_path(&bez);
            }
            DrawCommand::TextRun(text_elem) => {
                let contours = shape_text_skrifa(font_bytes, &text_elem.text, text_elem.font_size);
                cx.set_paint(peniko::Color::from_rgba8(
                    text_elem.color.r,
                    text_elem.color.g,
                    text_elem.color.b,
                    text_elem.color.a,
                ));
                for contour in contours {
                    let mut bez = kurbo::BezPath::new();
                    for v in &contour {
                        match v {
                            PathVerb::MoveTo(p) => bez.move_to((
                                (p.x + text_elem.x) as f64,
                                (p.y + text_elem.y) as f64,
                            )),
                            PathVerb::LineTo(p) => bez.line_to((
                                (p.x + text_elem.x) as f64,
                                (p.y + text_elem.y) as f64,
                            )),
                            PathVerb::QuadTo((c, to)) => bez.quad_to(
                                ((c.x + text_elem.x) as f64, (c.y + text_elem.y) as f64),
                                ((to.x + text_elem.x) as f64, (to.y + text_elem.y) as f64),
                            ),
                            PathVerb::CubicTo((c1, c2, to)) => bez.curve_to(
                                ((c1.x + text_elem.x) as f64, (c1.y + text_elem.y) as f64),
                                ((c2.x + text_elem.x) as f64, (c2.y + text_elem.y) as f64),
                                ((to.x + text_elem.x) as f64, (to.y + text_elem.y) as f64),
                            ),
                            PathVerb::Close => bez.close_path(),
                        }
                    }
                    cx.fill_path(&bez);
                }
            }
        }
    }

    cx.flush();
    let mut target = Pixmap::new(w16, h16);
    cx.render(&mut target, &mut resources);

    // Unpremultiply and serialize pixels to standard 8-bit RGBA
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for px in target.data() {
        let (r, g, b) = if px.a == 0 {
            (0, 0, 0)
        } else {
            (
                ((px.r as u32 * 255 + (px.a as u32 / 2)) / px.a as u32).min(255) as u8,
                ((px.g as u32 * 255 + (px.a as u32 / 2)) / px.a as u32).min(255) as u8,
                ((px.b as u32 * 255 + (px.a as u32 / 2)) / px.a as u32).min(255) as u8,
            )
        };
        rgba.extend_from_slice(&[r, g, b, px.a]);
    }
    rgba
}

fn compute_cache_key(
    plugin_bytes: &[u8],
    params: &[ParamEntry],
    time_ticks: i64,
    size: &Size,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(plugin_bytes);
    hasher.update(&time_ticks.to_le_bytes());
    hasher.update(&size.width.to_le_bytes());
    hasher.update(&size.height.to_le_bytes());

    for entry in params {
        hasher.update(entry.id.as_bytes());
        match &entry.value {
            ParamValue::Number(n) => {
                hasher.update(&[1]);
                hasher.update(&n.to_le_bytes());
            }
            ParamValue::Color(c) => {
                hasher.update(&[2, c.r, c.g, c.b, c.a]);
            }
            ParamValue::String(s) => {
                hasher.update(&[3]);
                hasher.update(s.as_bytes());
            }
            ParamValue::EnumChoice(e) => {
                hasher.update(&[4]);
                hasher.update(e.as_bytes());
            }
            ParamValue::Bool(b) => {
                hasher.update(&[5, if *b { 1 } else { 0 }]);
            }
        }
    }
    *hasher.finalize().as_bytes()
}

fn main() -> Result<()> {
    println!("=== AniMathIO 2.0 WebAssembly Component Model Spike ===");

    let candidate_paths = [
        PathBuf::from("target/wasm32-wasip2/release/shapes.wasm"),
        PathBuf::from("../target/wasm32-wasip2/release/shapes.wasm"),
        PathBuf::from("spikes/plugin/target/wasm32-wasip2/release/shapes.wasm"),
        PathBuf::from("../plugins/shapes/target/wasm32-wasip2/release/shapes.wasm"),
    ];

    let wasm_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .or_else(|| candidate_paths.into_iter().find(|p| p.exists()))
        .context("Could not find shapes.wasm. Pass path as first argument or build shapes plugin first.")?;

    let plugin_bytes = fs::read(&wasm_path).context("reading plugin wasm")?;
    let plugin_size_bytes = plugin_bytes.len();
    println!("Plugin path: {:?}", wasm_path);
    println!("Plugin .wasm size: {} bytes ({:.2} KB)", plugin_size_bytes, plugin_size_bytes as f64 / 1024.0);

    // 1. Engine & Linker Configuration (with sandbox: fuel, memory limiter, no WASI fs/net)
    let mut config = Config::new();
    config.wasm_component_model(true);
    config.consume_fuel(true);
    let engine = Engine::new(&config)?;

    let mut linker: Linker<HostState> = Linker::new(&engine);
    // WASI Preview 2 without filesystem preopens and without network
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
    Plugin::add_to_linker::<HostState, HasSelf<HostState>>(&mut linker, |s| s)?;

    let font_data = load_font_bytes();

    // 2. Component Compilation Benchmarks
    let compile_start = Instant::now();
    let component = Component::from_binary(&engine, &plugin_bytes)?;
    let compile_time = compile_start.elapsed();

    // Component serialization cache test
    let serialized_bytes = component.serialize()?;
    let deser_start = Instant::now();
    let _deser_component = unsafe { Component::deserialize(&engine, &serialized_bytes)? };
    let deser_time = deser_start.elapsed();

    // Instantiate benchmark
    let create_store = || -> Result<Store<HostState>> {
        let wasi = WasiCtxBuilder::new().build();
        let state = HostState {
            table: ResourceTable::new(),
            wasi,
            font_data: font_data.clone(),
            limiter: AppLimiter {
                max_memory_bytes: 64 * 1024 * 1024, // 64 MB sandbox limit
            },
            logs: Vec::new(),
        };
        let mut store = Store::new(&engine, state);
        let _ = store.set_fuel(50_000_000);
        store.limiter(|s| &mut s.limiter);
        Ok(store)
    };

    let inst_start = Instant::now();
    let mut store = create_store()?;
    let plugin = Plugin::instantiate(&mut store, &component, &linker)?;
    let inst_time = inst_start.elapsed();

    println!("Component compile time (cold): {:.3} ms", compile_time.as_secs_f64() * 1000.0);
    println!("Component deserialize time (cached): {:.3} ms", deser_time.as_secs_f64() * 1000.0);
    println!("Plugin instantiate time: {:.3} ms", inst_time.as_secs_f64() * 1000.0);

    // 3. Plugin Describe
    let manifest: Manifest = plugin.call_describe(&mut store)?;
    println!("\nPlugin Manifest:");
    println!("  Name: {}", manifest.name);
    println!("  Version: {}", manifest.version);
    println!("  Description: {}", manifest.description);
    println!("  Params ({}):", manifest.params.len());
    for p in &manifest.params {
        println!("    - id: {}, label: {}", p.id, p.label);
    }

    // 4. Render 3 sample PNG frames into spikes/plugin/out/
    let out_dir = if Path::new("spikes/plugin/out").exists() {
        PathBuf::from("spikes/plugin/out")
    } else if Path::new("out").exists() {
        PathBuf::from("out")
    } else {
        PathBuf::from("out")
    };
    fs::create_dir_all(&out_dir)?;

    let size = Size { width: 960, height: 540 };
    let sample_times = [(0, "frame_000.png"), (30, "frame_030.png"), (60, "frame_060.png")];

    for (ticks, filename) in sample_times {
        let frame_path = out_dir.join(filename);
        let scene = plugin.call_render(&mut store, &vec![], ticks, size)?;
        let rgba = render_scene_to_rgba(&scene, size.width, size.height, &font_data);
        image::save_buffer(
            &frame_path,
            &rgba,
            size.width,
            size.height,
            image::ExtendedColorType::Rgba8,
        )?;
        println!("Rendered sample frame at t={} ticks to {:?}", ticks, frame_path);
    }

    // 5. Benchmark per-call overhead for `render` (1000 calls)
    println!("\nRunning 1,000 calls benchmark for `render`...");
    let mut render_latencies_us = Vec::with_capacity(1000);
    for i in 0..1000 {
        // Reset fuel periodically so store doesn't run out
        let _ = store.set_fuel(50_000_000);
        let t0 = Instant::now();
        let _scene = plugin.call_render(&mut store, &vec![], i as i64, size)?;
        let elapsed = t0.elapsed().as_micros() as u64;
        render_latencies_us.push(elapsed);
    }
    render_latencies_us.sort_unstable();
    let p50_render_us = render_latencies_us[500];
    let p95_render_us = render_latencies_us[950];
    let p99_render_us = render_latencies_us[990];
    println!("Per-call render overhead (component boundary + plugin logic):");
    println!("  p50: {:.2} µs ({:.4} ms)", p50_render_us as f64, p50_render_us as f64 / 1000.0);
    println!("  p95: {:.2} µs ({:.4} ms)", p95_render_us as f64, p95_render_us as f64 / 1000.0);
    println!("  p99: {:.2} µs ({:.4} ms)", p99_render_us as f64, p99_render_us as f64 / 1000.0);

    // 6. Benchmark LRU Cache-Hit Cost
    println!("\nBenchmarking LRU Cache-Hit Cost...");
    let mut lru_cache: LruCache<[u8; 32], Vec<u8>> = LruCache::new(NonZeroUsize::new(256).unwrap());

    // Populate cache with 100 entries
    let dummy_frame = vec![0u8; (size.width * size.height * 4) as usize];
    for i in 0..100 {
        let key = compute_cache_key(&plugin_bytes, &[], i, &size);
        lru_cache.put(key, dummy_frame.clone());
    }

    let mut cache_hit_latencies_ns = Vec::with_capacity(1000);
    for i in 0..1000 {
        let tick = (i % 100) as i64;
        let t0 = Instant::now();
        let key = compute_cache_key(&plugin_bytes, &[], tick, &size);
        let hit = lru_cache.get(&key).is_some();
        let elapsed_ns = t0.elapsed().as_nanos() as u64;
        assert!(hit);
        cache_hit_latencies_ns.push(elapsed_ns);
    }
    cache_hit_latencies_ns.sort_unstable();
    let p50_cache_ns = cache_hit_latencies_ns[500];
    let p95_cache_ns = cache_hit_latencies_ns[950];
    println!("Cache-hit cost (BLAKE3 key compute + LRU lookup):");
    println!("  p50: {:.2} ns ({:.3} µs)", p50_cache_ns as f64, p50_cache_ns as f64 / 1000.0);
    println!("  p95: {:.2} ns ({:.3} µs)", p95_cache_ns as f64, p95_cache_ns as f64 / 1000.0);

    // 7. Isolation & Failure Mode Verification:
    // (A) Infinite loop / fuel exhaustion -> host survives!
    println!("\nTesting Host Isolation & Safety:");
    {
        println!("Testing fuel exhaustion / infinite loop containment...");
        let mut loop_store = create_store()?;
        let loop_plugin = Plugin::instantiate(&mut loop_store, &component, &linker)?;
        // Set very low fuel so execution halts
        let _ = loop_store.set_fuel(5);
        let res = loop_plugin.call_render(&mut loop_store, &vec![], 0, size);
        match res {
            Err(e) => {
                println!("  [SUCCESS] Trapped cleanly on fuel exhaustion: {}", e);
                assert!(loop_store.get_fuel().unwrap_or(0) == 0 || e.to_string().contains("fuel"));
            }
            Ok(_) => {
                println!("  [FAIL] Did not trap on fuel limit!");
            }
        }
    }

    // (B) Host resilience after trap: store recovery
    {
        println!("Testing host resilience after trap...");
        let mut resilient_store = create_store()?;
        let resilient_plugin = Plugin::instantiate(&mut resilient_store, &component, &linker)?;
        let _ = resilient_store.set_fuel(1);
        let _ = resilient_plugin.call_render(&mut resilient_store, &vec![], 0, size);
        // Refill fuel and instantiate new store - host process is alive and healthy
        let mut next_store = create_store()?;
        let next_plugin = Plugin::instantiate(&mut next_store, &component, &linker)?;
        let manifest = next_plugin.call_describe(&mut next_store)?;
        println!("  [SUCCESS] Host safely recovered: plugin '{}' re-queried without issue.", manifest.name);
    }

    println!("\n=== Spike measurements completed successfully ===");
    Ok(())
}
