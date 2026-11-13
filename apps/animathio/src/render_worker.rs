use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use animathio_core::{AssetId, Scene};
use animathio_render::{BackendKind, RenderCtx, preview_backend, render};
use gpui::RenderImage;

/// One immutable frame job; plugin resolution and rasterization run on the worker.
pub(crate) struct RenderRequest {
    pub(crate) generation: u64,
    pub(crate) scene: Scene,
    pub(crate) size: (u32, u32),
}

/// A finished BGRA `RenderImage` ready to present, tagged with the request that
/// produced it so stale results can be dropped.
pub(crate) struct RenderResult {
    pub(crate) generation: u64,
    pub(crate) image: Arc<RenderImage>,
}

pub(crate) enum WorkerMsg {
    Load(Arc<BTreeMap<AssetId, Vec<u8>>>),
    /// Switch the preview backend; GPU falls back to CPU when unavailable.
    Backend(BackendKind),
    Render(RenderRequest),
}

/// Latest-request-wins render worker. The UI thread only ever posts jobs and
/// drains results; the heavy Vello CPU rasterization happens here.
pub(crate) struct RenderWorker {
    tx: Sender<WorkerMsg>,
    rx: Receiver<RenderResult>,
}

impl RenderWorker {
    pub(crate) fn spawn() -> Self {
        let (tx, worker_rx) = channel::<WorkerMsg>();
        let (result_tx, rx) = channel::<RenderResult>();
        std::thread::Builder::new()
            .name("animathio-render".into())
            .spawn(move || worker_loop(&worker_rx, &result_tx))
            .expect("spawn render worker");
        Self { tx, rx }
    }

    pub(crate) fn load(&self, assets: Arc<BTreeMap<AssetId, Vec<u8>>>) {
        let _ = self.tx.send(WorkerMsg::Load(assets));
    }

    pub(crate) fn set_backend(&self, kind: BackendKind) {
        let _ = self.tx.send(WorkerMsg::Backend(kind));
    }

    pub(crate) fn request(&self, request: RenderRequest) {
        let _ = self.tx.send(WorkerMsg::Render(request));
    }

    pub(crate) fn try_result(&self) -> Option<RenderResult> {
        self.rx.try_recv().ok()
    }
}

fn worker_loop(rx: &Receiver<WorkerMsg>, tx: &Sender<RenderResult>) {
    let mut ctx = RenderCtx::default();
    let (mut host, _, _) = crate::editor::load_plugins();
    if let Some(host) = &host {
        ctx.set_effect_processor(host.effect_processor());
    }
    while let Ok(msg) = rx.recv() {
        // Drain whatever is queued now, keep only the newest render, and apply any
        // asset load before it. Older requests are discarded unrendered.
        let mut pending = vec![msg];
        while let Ok(next) = rx.try_recv() {
            pending.push(next);
        }
        let mut assets = None;
        let mut backend = None;
        let mut latest = None;
        for msg in pending {
            match msg {
                WorkerMsg::Load(loaded) => assets = Some(loaded),
                WorkerMsg::Backend(kind) => backend = Some(kind),
                WorkerMsg::Render(request) => latest = Some(request),
            }
        }
        if let Some(assets) = assets {
            // Rebuilding the context drops resources from the previous project but
            // keeps the (possibly GPU) backend and its device.
            match RenderCtx::from_assets((*assets).clone()) {
                Ok(mut loaded) => {
                    let current =
                        ctx.set_backend(Box::new(animathio_render::CpuBackend::default()));
                    loaded.set_backend(current);
                    if let Some(host) = &host {
                        loaded.set_effect_processor(host.effect_processor());
                    }
                    ctx = loaded;
                }
                Err(error) => eprintln!("render resources: {error}"),
            }
        }
        if let Some(requested) = backend {
            let selected = preview_backend(requested);
            match (&selected.fallback, selected.kind) {
                (Some(error), _) => eprintln!("preview renderer: CPU ({error})"),
                (None, kind) => eprintln!("preview renderer: {kind:?}"),
            }
            ctx.set_backend(selected.backend);
        }
        if let Some(mut request) = latest {
            if let Some(host) = &mut host {
                request.scene = host.resolve(&request.scene, request.size);
            }
            let Some(image) = render_image(&mut ctx, &request) else {
                continue;
            };
            let _ = tx.send(RenderResult {
                generation: request.generation,
                image,
            });
        }
    }
}

fn render_image(ctx: &mut RenderCtx, request: &RenderRequest) -> Option<Arc<RenderImage>> {
    let frame = match render(&request.scene, ctx, request.size) {
        Ok(frame) => frame,
        Err(animathio_render::Error::Gpu(error)) => {
            // A GPU failure mid-session (device lost, size limit): fall back to CPU.
            eprintln!("preview renderer: GPU failed ({error}); using CPU");
            ctx.set_backend(Box::new(animathio_render::CpuBackend::default()));
            render(&request.scene, ctx, request.size).ok()?
        }
        Err(error) => {
            eprintln!("preview render failed: {error}");
            return None;
        }
    };
    let mut pixels = frame.pixels;
    // GPUI's RenderImage path expects BGRA bytes despite image::RgbaImage's name.
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    let image = image::RgbaImage::from_raw(frame.size.0, frame.size.1, pixels)?;
    Some(Arc::new(RenderImage::new([image::Frame::new(image)])))
}
