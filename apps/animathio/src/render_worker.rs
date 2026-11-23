use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

use animathio_core::{AssetId, Scene};
use animathio_plugin_host::CommunityStore;
use animathio_render::{BackendKind, RenderCtx, preview_backend, render_preview};
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
    pub(crate) failed_effect_clips: std::collections::BTreeSet<animathio_core::ClipId>,
    /// First effect failure of this frame. The frame itself still rendered, with the
    /// failing effects passed through, so the preview never goes black over a missing plugin.
    pub(crate) effect_error: Option<String>,
    pub(crate) image: Result<Arc<RenderImage>, String>,
    #[cfg(test)]
    pub(crate) scene: Scene,
}

pub(crate) enum WorkerMsg {
    Load(Arc<BTreeMap<AssetId, Vec<u8>>>),
    SyncCommunity(CommunityStore),
    /// Switch the preview backend; GPU falls back to CPU when unavailable.
    Backend(BackendKind),
    Render(RenderRequest),
}

/// Latest-request-wins render worker. The UI thread only ever posts jobs and
/// drains results; the heavy Vello CPU rasterization happens here.
pub(crate) struct RenderWorker {
    tx: Sender<WorkerMsg>,
    rx: Receiver<RenderResult>,
    active: Arc<AtomicU8>,
}

/// What the worker is really rendering the preview with, as opposed to what was requested.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActiveBackend {
    /// No backend has been selected yet.
    Unknown,
    Cpu,
    Gpu,
    /// The GPU was requested but is unavailable or failed, so the CPU renders.
    CpuFallback,
}

impl ActiveBackend {
    fn code(self) -> u8 {
        match self {
            Self::Unknown => 0,
            Self::Cpu => 1,
            Self::Gpu => 2,
            Self::CpuFallback => 3,
        }
    }
    fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Cpu,
            2 => Self::Gpu,
            3 => Self::CpuFallback,
            _ => Self::Unknown,
        }
    }
}

impl RenderWorker {
    /// Start the render thread. Fails when the OS refuses the thread (low
    /// resource hosts); callers fall back to [`RenderWorker::disabled`] and an
    /// error state instead of crashing.
    pub(crate) fn spawn() -> Result<Self, String> {
        let (tx, worker_rx) = channel::<WorkerMsg>();
        let (result_tx, rx) = channel::<RenderResult>();
        let active = Arc::new(AtomicU8::new(ActiveBackend::Unknown.code()));
        let worker_active = active.clone();
        std::thread::Builder::new()
            .name("animathio-render".into())
            .spawn(move || worker_loop(&worker_rx, &result_tx, &worker_active))
            .map_err(|error| format!("spawn render worker: {error}"))?;
        Ok(Self { tx, rx, active })
    }

    /// An inert worker for when the render thread could not start: posts are
    /// dropped and no frame ever arrives, so the editor shows its error state
    /// instead of a live preview.
    pub(crate) fn disabled() -> Self {
        let (tx, _) = channel::<WorkerMsg>();
        let (_, rx) = channel::<RenderResult>();
        Self {
            tx,
            rx,
            active: Arc::new(AtomicU8::new(ActiveBackend::Cpu.code())),
        }
    }

    /// The backend the preview is really rendered with right now.
    pub(crate) fn active_backend(&self) -> ActiveBackend {
        ActiveBackend::from_code(self.active.load(Ordering::Relaxed))
    }

    pub(crate) fn load(&self, assets: Arc<BTreeMap<AssetId, Vec<u8>>>) {
        let _ = self.tx.send(WorkerMsg::Load(assets));
    }

    pub(crate) fn set_backend(&self, kind: BackendKind) {
        let _ = self.tx.send(WorkerMsg::Backend(kind));
    }

    pub(crate) fn sync_community_plugins(&self, store: CommunityStore) {
        let _ = self.tx.send(WorkerMsg::SyncCommunity(store));
    }

    pub(crate) fn request(&self, request: RenderRequest) {
        let _ = self.tx.send(WorkerMsg::Render(request));
    }

    pub(crate) fn try_result(&self) -> Option<RenderResult> {
        self.rx.try_recv().ok()
    }

    #[cfg(test)]
    pub(crate) fn wait_for_result(&self, generation: u64) -> RenderResult {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let result = self
                .rx
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .expect("preview worker produced a frame");
            if result.generation == generation {
                return result;
            }
        }
    }
}

fn worker_loop(rx: &Receiver<WorkerMsg>, tx: &Sender<RenderResult>, active: &AtomicU8) {
    let mut ctx = RenderCtx::default();
    let (mut host, _, _) = crate::editor::load_plugins();
    if let Some(host) = &host {
        ctx.set_effect_processor(host.effect_processor().for_preview());
    }
    while let Ok(msg) = rx.recv() {
        // Drain whatever is queued now, keep only the newest render, and apply any
        // asset and plugin changes before it. Older requests are discarded unrendered.
        let mut pending = vec![msg];
        while let Ok(next) = rx.try_recv() {
            pending.push(next);
        }
        let mut assets = None;
        let mut backend = None;
        let mut community = None;
        let mut latest = None;
        for msg in pending {
            match msg {
                WorkerMsg::Load(loaded) => assets = Some(loaded),
                WorkerMsg::SyncCommunity(store) => community = Some(store),
                WorkerMsg::Backend(kind) => backend = Some(kind),
                WorkerMsg::Render(request) => latest = Some(request),
            }
        }
        if let (Some(store), Some(host)) = (community, &mut host) {
            // The effect processor shares this host's registry.
            host.sync_community_plugins(&store);
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
                        loaded.set_effect_processor(host.effect_processor().for_preview());
                    }
                    ctx = loaded;
                }
                Err(error) => eprintln!("render resources: {error}"),
            }
        }
        if let Some(requested) = backend {
            let selected = preview_backend(requested);
            let used = match (&selected.fallback, selected.kind) {
                (Some(error), _) => {
                    eprintln!("preview renderer: CPU ({error})");
                    ActiveBackend::CpuFallback
                }
                (None, BackendKind::Gpu) => ActiveBackend::Gpu,
                (None, BackendKind::Cpu) => ActiveBackend::Cpu,
            };
            eprintln!("preview renderer: {used:?}");
            active.store(used.code(), Ordering::Relaxed);
            ctx.set_backend(selected.backend);
        }
        if let Some(mut request) = latest {
            if let Some(host) = &mut host {
                request.scene = host.resolve(&request.scene, request.size);
            }
            let (image, effect_failures) = render_image(&mut ctx, &request, active);
            let failed_effect_clips = effect_failures.iter().map(|(id, _)| *id).collect();
            let effect_error = effect_failures.into_iter().next().map(|(_, error)| error);
            let _ = tx.send(RenderResult {
                failed_effect_clips,
                generation: request.generation,
                effect_error,
                image,
                #[cfg(test)]
                scene: request.scene,
            });
        }
    }
}

type EffectFailures = Vec<(animathio_core::ClipId, String)>;

fn render_image(
    ctx: &mut RenderCtx,
    request: &RenderRequest,
    active: &AtomicU8,
) -> (Result<Arc<RenderImage>, String>, EffectFailures) {
    let preview = match render_preview(&request.scene, ctx, request.size) {
        Ok(preview) => preview,
        Err(animathio_render::Error::Gpu(error)) => {
            // A GPU failure mid-session (device lost, size limit): fall back to CPU.
            eprintln!("preview renderer: GPU failed ({error}); using CPU");
            ctx.set_backend(Box::new(animathio_render::CpuBackend::default()));
            active.store(ActiveBackend::CpuFallback.code(), Ordering::Relaxed);
            match render_preview(&request.scene, ctx, request.size) {
                Ok(preview) => preview,
                Err(error) => return (Err(error.to_string()), Vec::new()),
            }
        }
        Err(error) => {
            eprintln!("preview render failed: {error}");
            return (Err(error.to_string()), Vec::new());
        }
    };
    let failures = preview.effect_failures;
    (to_image(preview.frame), failures)
}

fn to_image(frame: animathio_render::Frame) -> Result<Arc<RenderImage>, String> {
    let mut pixels = frame.pixels;
    // GPUI's RenderImage path expects BGRA bytes despite image::RgbaImage's name.
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    let image =
        image::RgbaImage::from_raw(frame.size.0, frame.size.1, pixels).ok_or("invalid frame")?;
    Ok(Arc::new(RenderImage::new([image::Frame::new(image)])))
}
