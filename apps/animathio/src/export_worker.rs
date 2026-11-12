//! Snapshot export on a worker; the editor keeps responding to input and cancellation.
use animathio_control::{ControlError, ExportParams};
use animathio_core::{AssetId, Project, TimeRange};
use animathio_export::{Exporter, Progress};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc::{Receiver, SyncSender, channel},
};

pub(crate) struct ExportTask {
    pub(crate) cancel: Arc<AtomicBool>,
    pub(crate) completed: Arc<AtomicU64>,
    pub(crate) total: Arc<AtomicU64>,
    pub(crate) result: Receiver<Result<PathBuf, String>>,
}
impl Drop for ExportTask {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
impl ExportTask {
    pub(crate) fn spawn(
        project: Project,
        assets: BTreeMap<AssetId, Vec<u8>>,
        params: ExportParams,
        reply: Option<SyncSender<Result<Value, ControlError>>>,
    ) -> Result<Self, String> {
        let cancel = Arc::new(AtomicBool::new(false));
        let completed = Arc::new(AtomicU64::new(0));
        let total = Arc::new(AtomicU64::new(0));
        let task_cancel = cancel.clone();
        let task_completed = completed.clone();
        let task_total = total.clone();
        let (tx, result) = channel();
        std::thread::Builder::new()
            .name("animathio-export".into())
            .spawn(move || {
                let result = (|| {
                    let output = animathio_control::validate_output_file(&params.output)
                        .map_err(|e| e.to_string())?;
                    let range = TimeRange::new(
                        params.from,
                        params
                            .to
                            .unwrap_or_else(|| animathio_export::duration(&project)),
                    )
                    .map_err(|e| e.to_string())?;
                    let mut exporter =
                        Exporter::new(&project, &assets).map_err(|e| e.to_string())?;
                    let (mut host, _, _) = crate::editor::load_plugins();
                    let destination = animathio_export::Destination {
                        path: output,
                        overwrite: params.overwrite,
                    };
                    let written = exporter
                        .export_to_file(
                            &project,
                            range,
                            params.settings,
                            &destination,
                            |p: Progress| {
                                task_completed.store(p.completed, Ordering::Relaxed);
                                task_total.store(p.total, Ordering::Relaxed);
                                !task_cancel.load(Ordering::Relaxed)
                            },
                            |scene| {
                                host.as_mut().map_or_else(
                                    || scene.clone(),
                                    |host| {
                                        host.resolve(&scene, animathio_export::resolution(&project))
                                    },
                                )
                            },
                        )
                        .map_err(|e| e.to_string())?;
                    if task_cancel.load(Ordering::Relaxed) {
                        return Err("export cancelled".into());
                    }
                    Ok(written)
                })();
                if let Some(reply) = reply {
                    let value = result
                        .as_ref()
                        .map(|path| serde_json::json!(path.display().to_string()))
                        .map_err(|e: &String| ControlError::Other(e.clone()));
                    let _ = reply.send(value);
                }
                let _ = tx.send(result);
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            cancel,
            completed,
            total,
            result,
        })
    }
}
