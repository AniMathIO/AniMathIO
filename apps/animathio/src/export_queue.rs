//! Sequential export queue. Each job owns a snapshot of the project taken when it
//! was queued, so later edits (or opening another project) never change what is
//! rendered. Jobs run one at a time on the export worker; the queue is polled from
//! the editor's UI loop and holds no GPUI types so it stays unit-testable.
use crate::export_worker::ExportTask;
use animathio_control::ExportParams;
use animathio_core::{AssetId, Project};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, atomic::Ordering};
use std::time::{Duration, Instant};

/// Everything an export needs, frozen at enqueue time.
pub(crate) struct JobSpec {
    pub(crate) project: Project,
    pub(crate) assets: Arc<BTreeMap<AssetId, Vec<u8>>>,
    pub(crate) params: ExportParams,
    /// Short label such as `1080p, 00:00:00:00 to 00:00:05:00`.
    pub(crate) summary: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum JobState {
    Queued,
    Running,
    Done(PathBuf),
    Failed(String),
    Cancelled,
}
impl JobState {
    pub(crate) fn is_finished(&self) -> bool {
        matches!(self, Self::Done(_) | Self::Failed(_) | Self::Cancelled)
    }
}

pub(crate) struct Job {
    pub(crate) id: u64,
    pub(crate) output: PathBuf,
    pub(crate) summary: String,
    pub(crate) state: JobState,
    /// Set once a running job was asked to stop; the worker may still be winding down.
    pub(crate) cancelling: bool,
    spec: Option<JobSpec>,
    task: Option<ExportTask>,
    started: Option<Instant>,
    elapsed: Option<Duration>,
}
impl Job {
    pub(crate) fn name(&self) -> String {
        self.output.file_name().map_or_else(
            || self.output.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        )
    }
    /// `(completed, total)` frames; `(0, 0)` until the worker reports.
    pub(crate) fn progress(&self) -> (u64, u64) {
        self.task.as_ref().map_or((0, 0), |t| {
            (
                t.completed.load(Ordering::Relaxed),
                t.total.load(Ordering::Relaxed),
            )
        })
    }
    pub(crate) fn fraction(&self) -> f32 {
        match &self.state {
            JobState::Done(_) => 1.0,
            JobState::Running => {
                let (done, total) = self.progress();
                if total == 0 {
                    0.0
                } else {
                    (done as f32 / total as f32).clamp(0.0, 1.0)
                }
            }
            _ => 0.0,
        }
    }
    /// Estimated time left, from the frame rate so far. `None` until some frames finish.
    pub(crate) fn eta(&self) -> Option<Duration> {
        if self.state != JobState::Running {
            return None;
        }
        let (done, total) = self.progress();
        if done == 0 || total <= done {
            return None;
        }
        let elapsed = self.started?.elapsed();
        Some(elapsed.mul_f64((total - done) as f64 / done as f64))
    }
    pub(crate) fn elapsed(&self) -> Option<Duration> {
        self.elapsed.or_else(|| self.started.map(|s| s.elapsed()))
    }
}

/// Something the UI should announce (toast) after a poll.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Event {
    Finished {
        id: u64,
        name: String,
        path: PathBuf,
    },
    Failed {
        id: u64,
        name: String,
        error: String,
    },
    Cancelled {
        id: u64,
        name: String,
    },
}

#[derive(Default)]
pub(crate) struct ExportQueue {
    jobs: Vec<Job>,
    next_id: u64,
}
impl ExportQueue {
    pub(crate) fn jobs(&self) -> &[Job] {
        &self.jobs
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }
    /// A job is running or waiting, i.e. an exit would lose work.
    pub(crate) fn is_busy(&self) -> bool {
        self.jobs.iter().any(|j| !j.state.is_finished())
    }
    pub(crate) fn running(&self) -> Option<&Job> {
        self.jobs.iter().find(|j| j.state == JobState::Running)
    }
    pub(crate) fn queued_count(&self) -> usize {
        self.jobs
            .iter()
            .filter(|j| j.state == JobState::Queued)
            .count()
    }
    /// Whether some unfinished job already writes `output`.
    pub(crate) fn targets(&self, output: &std::path::Path) -> bool {
        self.jobs
            .iter()
            .any(|j| !j.state.is_finished() && j.output == output)
    }
    /// Queue a job. `front` puts it ahead of other waiting jobs ("Export now");
    /// a running job is never preempted.
    pub(crate) fn enqueue(&mut self, spec: JobSpec, front: bool) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let job = Job {
            id,
            output: spec.params.output.clone(),
            summary: spec.summary.clone(),
            state: JobState::Queued,
            cancelling: false,
            spec: Some(spec),
            task: None,
            started: None,
            elapsed: None,
        };
        let index = if front {
            self.jobs
                .iter()
                .position(|j| j.state == JobState::Queued)
                .unwrap_or(self.jobs.len())
        } else {
            self.jobs.len()
        };
        self.jobs.insert(index, job);
        id
    }
    /// Start an already-spawned task (control-protocol exports) as the running job.
    pub(crate) fn adopt_running(&mut self, spec: &JobSpec, task: ExportTask) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.jobs.push(Job {
            id,
            output: spec.params.output.clone(),
            summary: spec.summary.clone(),
            state: JobState::Running,
            cancelling: false,
            spec: None,
            task: Some(task),
            started: Some(Instant::now()),
            elapsed: None,
        });
        id
    }
    /// Queued jobs are dropped; a running job gets its cancel flag set and turns
    /// `Cancelled` once the worker returns. Finished jobs are left alone.
    pub(crate) fn cancel(&mut self, id: u64) {
        let Some(job) = self.jobs.iter_mut().find(|j| j.id == id) else {
            return;
        };
        match job.state {
            JobState::Queued => {
                job.state = JobState::Cancelled;
                job.spec = None;
            }
            JobState::Running => {
                job.cancelling = true;
                if let Some(task) = &job.task {
                    task.cancel.store(true, Ordering::Relaxed);
                }
            }
            _ => {}
        }
    }
    /// Forget one finished job.
    pub(crate) fn remove(&mut self, id: u64) {
        self.jobs.retain(|j| j.id != id || !j.state.is_finished());
    }
    pub(crate) fn clear_finished(&mut self) {
        self.jobs.retain(|j| !j.state.is_finished());
    }
    /// Collect a finished running job and start the next queued one with `spawn`.
    pub(crate) fn pump(
        &mut self,
        mut spawn: impl FnMut(JobSpec) -> Result<ExportTask, String>,
    ) -> Vec<Event> {
        let mut events = Vec::new();
        if let Some(job) = self.jobs.iter_mut().find(|j| j.state == JobState::Running) {
            let outcome = job.task.as_ref().map(|task| task.result.try_recv());
            let finished = match outcome {
                Some(Ok(result)) => Some(result),
                Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                    Some(Err("export worker stopped unexpectedly".to_owned()))
                }
                _ => None,
            };
            if let Some(result) = finished {
                job.elapsed = job.started.map(|s| s.elapsed());
                job.state = match result {
                    Ok(path) => JobState::Done(path),
                    Err(_) if job.cancelling => JobState::Cancelled,
                    Err(error) => JobState::Failed(error),
                };
                let (id, name) = (job.id, job.name());
                events.push(match &job.state {
                    JobState::Done(path) => Event::Finished {
                        id,
                        name,
                        path: path.clone(),
                    },
                    JobState::Failed(error) => Event::Failed {
                        id,
                        name,
                        error: error.clone(),
                    },
                    _ => Event::Cancelled { id, name },
                });
                // Keep the final counters visible on the finished row.
                job.task = None;
            }
        }
        while self.running().is_none() {
            let Some(job) = self.jobs.iter_mut().find(|j| j.state == JobState::Queued) else {
                break;
            };
            let Some(spec) = job.spec.take() else {
                job.state = JobState::Cancelled;
                continue;
            };
            match spawn(spec) {
                Ok(task) => {
                    job.task = Some(task);
                    job.started = Some(Instant::now());
                    job.state = JobState::Running;
                }
                Err(error) => {
                    job.state = JobState::Failed(error.clone());
                    events.push(Event::Failed {
                        id: job.id,
                        name: job.name(),
                        error,
                    });
                }
            }
        }
        events
    }
}

/// `m:ss` (or `h:mm:ss`) for ETA and elapsed labels.
pub(crate) fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();
    if secs >= 3600 {
        format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
    } else {
        format!("{}:{:02}", secs / 60, secs % 60)
    }
}

/// Reveal `path` in the platform file manager (its folder for files on Linux).
pub(crate) fn reveal(path: &std::path::Path) -> Result<(), String> {
    let folder = path.parent().unwrap_or(path);
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn();
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("explorer")
        .arg(format!("/select,{}", path.display()))
        .spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let result = std::process::Command::new("xdg-open").arg(folder).spawn();
    let _ = folder;
    result
        .map(|_| ())
        .map_err(|e| format!("cannot open file manager: {e}"))
}

/// Open `path` with the platform default application.
pub(crate) fn open_file(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(path).spawn();
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let result = std::process::Command::new("xdg-open").arg(path).spawn();
    result
        .map(|_| ())
        .map_err(|e| format!("cannot open file: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use animathio_core::{FrameRate, Time};
    use std::sync::{
        atomic::{AtomicBool, AtomicU64},
        mpsc::{Sender, channel},
    };

    fn spec(name: &str) -> JobSpec {
        JobSpec {
            project: Project::new(FrameRate::new(30, 1).unwrap()),
            assets: Arc::default(),
            params: ExportParams {
                output: PathBuf::from(name),
                from: Time(0),
                to: None,
                settings: animathio_export::Settings::default(),
                overwrite: false,
            },
            summary: "test".into(),
        }
    }
    /// A fake worker whose result the test sends by hand.
    fn fake_task() -> (ExportTask, Sender<Result<PathBuf, String>>) {
        let (tx, result) = channel();
        (
            ExportTask {
                cancel: Arc::new(AtomicBool::new(false)),
                completed: Arc::new(AtomicU64::new(0)),
                total: Arc::new(AtomicU64::new(0)),
                result,
            },
            tx,
        )
    }

    #[test]
    fn jobs_run_one_at_a_time_in_order_and_report_events() {
        let mut queue = ExportQueue::default();
        let a = queue.enqueue(spec("a.mp4"), false);
        let b = queue.enqueue(spec("b.mp4"), false);
        let senders = std::cell::RefCell::new(Vec::new());
        let spawned = std::cell::RefCell::new(Vec::new());
        let mut spawn = |spec: JobSpec| {
            let (task, tx) = fake_task();
            spawned.borrow_mut().push(spec.params.output.clone());
            senders.borrow_mut().push(tx);
            Ok(task)
        };
        assert_eq!(queue.pump(&mut spawn).len(), 0);
        assert_eq!(queue.running().unwrap().id, a);
        assert_eq!(queue.queued_count(), 1);
        // Polling again must not start the second job while the first runs.
        assert_eq!(queue.pump(&mut spawn).len(), 0);
        assert_eq!(*spawned.borrow(), [PathBuf::from("a.mp4")]);
        senders.borrow()[0]
            .send(Ok(PathBuf::from("/out/a.mp4")))
            .unwrap();
        let events = queue.pump(&mut spawn);
        assert_eq!(
            events,
            [Event::Finished {
                id: a,
                name: "a.mp4".into(),
                path: PathBuf::from("/out/a.mp4")
            }]
        );
        assert_eq!(queue.running().unwrap().id, b);
        senders.borrow()[1].send(Err("disk full".into())).unwrap();
        let events = queue.pump(&mut spawn);
        assert!(matches!(&events[0], Event::Failed { error, .. } if error == "disk full"));
        assert!(!queue.is_busy());
        assert_eq!(
            queue.jobs()[1].state,
            JobState::Failed("disk full".into()),
            "error text stays on the row"
        );
    }

    #[test]
    fn export_now_goes_ahead_of_waiting_jobs_but_not_the_running_one() {
        let mut queue = ExportQueue::default();
        let first = queue.enqueue(spec("1.mp4"), false);
        queue.enqueue(spec("2.mp4"), false);
        let mut keep = Vec::new();
        queue.pump(|_| {
            let (task, tx) = fake_task();
            keep.push(tx);
            Ok(task)
        });
        let urgent = queue.enqueue(spec("now.mp4"), true);
        let order: Vec<_> = queue.jobs().iter().map(|j| j.id).collect();
        assert_eq!(order[0], first);
        assert_eq!(order[1], urgent);
        assert_eq!(queue.running().unwrap().id, first);
    }

    #[test]
    fn cancelling_a_queued_job_skips_it_and_a_running_one_sets_the_flag() {
        let mut queue = ExportQueue::default();
        let a = queue.enqueue(spec("a.mp4"), false);
        let b = queue.enqueue(spec("b.mp4"), false);
        let c = queue.enqueue(spec("c.mp4"), false);
        let flags = std::cell::RefCell::new(Vec::new());
        let senders = std::cell::RefCell::new(Vec::new());
        let mut spawn = |_: JobSpec| {
            let (task, tx) = fake_task();
            flags.borrow_mut().push(Arc::clone(&task.cancel));
            senders.borrow_mut().push(tx);
            Ok(task)
        };
        queue.pump(&mut spawn);
        queue.cancel(b);
        assert_eq!(queue.jobs()[1].state, JobState::Cancelled);
        queue.cancel(a);
        assert!(queue.jobs()[0].cancelling);
        assert!(flags.borrow()[0].load(Ordering::Relaxed));
        assert_eq!(queue.running().unwrap().id, a, "still winding down");
        senders.borrow()[0]
            .send(Err("export cancelled".into()))
            .unwrap();
        let events = queue.pump(&mut spawn);
        assert_eq!(
            events[0],
            Event::Cancelled {
                id: a,
                name: "a.mp4".into()
            }
        );
        assert_eq!(queue.jobs()[0].state, JobState::Cancelled);
        // The cancelled queued job was skipped, so c is next.
        assert_eq!(queue.running().unwrap().id, c);
    }

    #[test]
    fn spawn_failures_and_dead_workers_fail_the_job_not_the_queue() {
        let mut queue = ExportQueue::default();
        let bad = queue.enqueue(spec("bad.mp4"), false);
        let good = queue.enqueue(spec("good.mp4"), false);
        let mut first = true;
        let mut keep = Vec::new();
        let events = queue.pump(|_| {
            if std::mem::take(&mut first) {
                Err("no encoder".into())
            } else {
                let (task, tx) = fake_task();
                keep.push(tx);
                Ok(task)
            }
        });
        assert!(
            matches!(&events[0], Event::Failed { id, error, .. } if *id == bad && error == "no encoder")
        );
        assert_eq!(queue.running().unwrap().id, good);
        keep.clear(); // the worker dies without sending a result
        let events = queue.pump(|_| unreachable!());
        assert!(matches!(&events[0], Event::Failed { error, .. } if error.contains("stopped")));
    }

    #[test]
    fn progress_fraction_and_eta_follow_the_worker_counters() {
        let mut queue = ExportQueue::default();
        queue.enqueue(spec("a.mp4"), false);
        let mut counters = None;
        queue.pump(|_| {
            let (task, tx) = fake_task();
            counters = Some((Arc::clone(&task.completed), Arc::clone(&task.total), tx));
            Ok(task)
        });
        let job = queue.running().unwrap();
        assert_eq!(job.fraction(), 0.0);
        assert!(job.eta().is_none(), "no ETA before the first frame");
        let (completed, total, _tx) = counters.unwrap();
        total.store(100, Ordering::Relaxed);
        completed.store(25, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(10));
        let job = queue.running().unwrap();
        assert_eq!(job.progress(), (25, 100));
        assert!((job.fraction() - 0.25).abs() < 1e-6);
        assert!(job.eta().is_some());
    }

    #[test]
    fn remove_and_clear_only_touch_finished_jobs_and_targets_see_unfinished() {
        let mut queue = ExportQueue::default();
        let a = queue.enqueue(spec("a.mp4"), false);
        let b = queue.enqueue(spec("b.mp4"), false);
        assert!(queue.targets(std::path::Path::new("a.mp4")));
        queue.remove(a);
        assert_eq!(queue.jobs().len(), 2, "a queued job is not removable");
        queue.cancel(a);
        assert!(!queue.targets(std::path::Path::new("a.mp4")));
        queue.remove(a);
        assert_eq!(queue.jobs().len(), 1);
        queue.cancel(b);
        queue.clear_finished();
        assert!(queue.is_empty());
    }

    #[test]
    fn durations_format_as_clock_times() {
        assert_eq!(format_duration(Duration::from_secs(7)), "0:07");
        assert_eq!(format_duration(Duration::from_secs(125)), "2:05");
        assert_eq!(format_duration(Duration::from_secs(3725)), "1:02:05");
    }
}
