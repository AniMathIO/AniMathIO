use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use animathio_core::{AssetId, Scene};
use animathio_render::{CpuBackend, PlaceholderFrameSource, RenderCtx, render};
use gpui::RenderImage;

/// One immutable frame job. The UI thread evaluates and resolves the scene (running
/// plugin clips through the plugin host) so the worker stays free of any mutable
/// runtime state beyond its render resources.
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
    Load(BTreeMap<AssetId, Vec<u8>>),
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

    pub(crate) fn load(&self, assets: BTreeMap<AssetId, Vec<u8>>) {
        let _ = self.tx.send(WorkerMsg::Load(assets));
    }

    pub(crate) fn request(&self, request: RenderRequest) {
        let _ = self.tx.send(WorkerMsg::Render(request));
    }

    pub(crate) fn try_result(&self) -> Option<RenderResult> {
        self.rx.try_recv().ok()
    }
}

fn worker_loop(rx: &Receiver<WorkerMsg>, tx: &Sender<RenderResult>) {
    let mut ctx = RenderCtx::new(CpuBackend::default(), PlaceholderFrameSource::default());
    while let Ok(msg) = rx.recv() {
        // Drain whatever is queued now, keep only the newest render, and apply any
        // asset load before it. Older requests are discarded unrendered.
        let mut pending = vec![msg];
        while let Ok(next) = rx.try_recv() {
            pending.push(next);
        }
        let mut assets = None;
        let mut latest = None;
        for msg in pending {
            match msg {
                WorkerMsg::Load(loaded) => assets = Some(loaded),
                WorkerMsg::Render(request) => latest = Some(request),
            }
        }
        if let Some(assets) = assets {
            // Rebuilding the context drops resources from the previous project.
            ctx = RenderCtx::new(CpuBackend::default(), PlaceholderFrameSource::default());
            for (id, bytes) in assets {
                let _ = ctx.resources.insert(id, bytes);
            }
        }
        if let Some(request) = latest
            && let Some(image) = render_image(&mut ctx, &request)
        {
            let _ = tx.send(RenderResult {
                generation: request.generation,
                image,
            });
        }
    }
}

fn render_image(ctx: &mut RenderCtx, request: &RenderRequest) -> Option<Arc<RenderImage>> {
    let frame = render(&request.scene, ctx, request.size).ok()?;
    let mut pixels = frame.pixels;
    // GPUI's RenderImage path expects BGRA bytes despite image::RgbaImage's name.
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    let image = image::RgbaImage::from_raw(frame.size.0, frame.size.1, pixels)?;
    Some(Arc::new(RenderImage::new([image::Frame::new(image)])))
}
