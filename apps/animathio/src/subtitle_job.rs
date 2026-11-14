//! Automatic-subtitle job: download, transcription and cue splitting run on a worker
//! thread while the editor stays responsive. The job owns a project snapshot, reports
//! progress through shared state and can be cancelled; applying the result to the
//! project (one undoable batch) is the editor's job on the UI thread.
use animathio_control::{
    ControlError, SubtitleDraft, SubtitleGenerateParams, SubtitleProgress, transcribe_project,
};
use animathio_core::{AssetId, Project};
use animathio_transcribe::Cancel;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, SyncSender, TryRecvError, channel},
};
use std::time::Instant;

/// Where the worker currently is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Preparing,
    Downloading { downloaded: u64, total: u64 },
    Transcribing(u8),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum State {
    Running,
    Done(String),
    Failed(String),
    Cancelled,
}

pub(crate) struct SubtitleJob {
    pub(crate) label: String,
    /// Caption style the finished cues are created with.
    pub(crate) style: animathio_control::CaptionStyleParams,
    pub(crate) state: State,
    pub(crate) cancelling: bool,
    /// Project path when the job started; results are dropped if another project is open.
    pub(crate) project_path: Option<std::path::PathBuf>,
    /// Answer owed to a control client that started the job.
    pub(crate) reply: Option<SyncSender<Result<Value, ControlError>>>,
    cancel: Cancel,
    phase: Arc<Mutex<Phase>>,
    result: Receiver<Result<SubtitleDraft, ControlError>>,
    started: Instant,
}

impl Drop for SubtitleJob {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl SubtitleJob {
    pub(crate) fn spawn(
        project: Project,
        assets: Arc<BTreeMap<AssetId, Vec<u8>>>,
        params: SubtitleGenerateParams,
        label: String,
        project_path: Option<std::path::PathBuf>,
        reply: Option<SyncSender<Result<Value, ControlError>>>,
    ) -> Result<Self, String> {
        let style = params.style.clone();
        let cancel: Cancel = Arc::new(AtomicBool::new(false));
        let phase = Arc::new(Mutex::new(Phase::Preparing));
        let (tx, result) = channel();
        let (worker_cancel, worker_phase) = (cancel.clone(), phase.clone());
        std::thread::Builder::new()
            .name("animathio-subtitles".into())
            .spawn(move || {
                let progress: animathio_control::ProgressFn = Arc::new(move |event| {
                    let next = match event {
                        SubtitleProgress::Downloading { downloaded, total } => {
                            Phase::Downloading { downloaded, total }
                        }
                        SubtitleProgress::Transcribing { percent } => Phase::Transcribing(percent),
                    };
                    *worker_phase
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = next;
                });
                let _ = tx.send(transcribe_project(
                    &project,
                    &assets,
                    &params,
                    &worker_cancel,
                    &progress,
                ));
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            label,
            style,
            state: State::Running,
            cancelling: false,
            project_path,
            reply,
            cancel,
            phase,
            result,
            started: Instant::now(),
        })
    }

    pub(crate) fn phase(&self) -> Phase {
        *self
            .phase
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Progress of the current phase in `0.0..=1.0`.
    pub(crate) fn fraction(&self) -> f32 {
        match self.phase() {
            Phase::Preparing => 0.0,
            #[allow(clippy::cast_precision_loss)]
            Phase::Downloading { downloaded, total } => {
                (downloaded as f64 / total.max(1) as f64) as f32
            }
            Phase::Transcribing(percent) => f32::from(percent) / 100.0,
        }
    }

    pub(crate) fn elapsed_secs(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    pub(crate) fn request_cancel(&mut self) {
        self.cancelling = true;
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// The worker's result once it has finished.
    pub(crate) fn poll(&self) -> Option<Result<SubtitleDraft, ControlError>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(ControlError::Other(
                "the subtitle worker stopped unexpectedly".into(),
            ))),
        }
    }
}
