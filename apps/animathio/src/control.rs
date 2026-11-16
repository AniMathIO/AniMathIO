//! Control-server bridge: the server thread forwards each call over a channel
//! and the UI thread drains it during the render loop, keeping all project
//! mutation on the same thread that owns the editor.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
        mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel},
    },
    time::{Duration, Instant},
};

use animathio_control::{
    Call, ControlError, ControlHandler, ControlReply, ControlServer, RunningServer,
};
use serde_json::Value;

const ACK_DEADLINE: Duration = Duration::from_secs(10);
// Match the server's eight-client bound; expired requests cannot accumulate
// indefinitely when clients retry while the UI is wedged.
const MAX_QUEUED_REQUESTS: usize = 8;
const PENDING: u8 = 0;
const ACCEPTED: u8 = 1;
const EXPIRED: u8 = 2;

/// A queued call that must be accepted before touching editor state.
pub(crate) struct ControlMsg {
    call: Call,
    reply: SyncSender<Result<Value, ControlError>>,
    ack: SyncSender<()>,
    state: Arc<AtomicU8>,
    deadline: Instant,
}

impl ControlMsg {
    /// Atomically accept or discard an expired request, before any side effects.
    pub(crate) fn accept(self) -> Option<(Call, SyncSender<Result<Value, ControlError>>)> {
        self.accept_at(Instant::now())
    }

    fn accept_at(self, now: Instant) -> Option<(Call, SyncSender<Result<Value, ControlError>>)> {
        if now >= self.deadline {
            let _ =
                self.state
                    .compare_exchange(PENDING, EXPIRED, Ordering::SeqCst, Ordering::SeqCst);
            return None;
        }
        self.state
            .compare_exchange(PENDING, ACCEPTED, Ordering::SeqCst, Ordering::SeqCst)
            .ok()?;
        self.ack.send(()).ok()?;
        Some((self.call, self.reply))
    }
}

/// The server-side half; enqueue under the handler lock, then wait outside it.
pub(crate) struct ControlBridge {
    tx: SyncSender<ControlMsg>,
    ack_deadline: Duration,
}

impl ControlBridge {
    pub(crate) fn new() -> (Self, Receiver<ControlMsg>) {
        let (tx, rx) = sync_channel(MAX_QUEUED_REQUESTS);
        (
            Self {
                tx,
                ack_deadline: ACK_DEADLINE,
            },
            rx,
        )
    }

    fn defer_at(
        &mut self,
        call: Call,
        queued_at: Instant,
        wait_clock: impl FnOnce() -> Instant + Send + 'static,
    ) -> ControlReply {
        let (reply_tx, reply_rx) = sync_channel(1);
        let (ack_tx, ack_rx) = sync_channel(1);
        let state = Arc::new(AtomicU8::new(PENDING));
        let deadline = queued_at + self.ack_deadline;
        let sent = self.tx.try_send(ControlMsg {
            call,
            reply: reply_tx,
            ack: ack_tx,
            state: state.clone(),
            deadline,
        });
        Box::new(move || {
            sent.map_err(|error| match error {
                TrySendError::Full(_) => ControlError::NotAccepted,
                TrySendError::Disconnected(_) => ControlError::EditorClosed,
            })?;
            match ack_rx.recv_timeout(deadline.saturating_duration_since(wait_clock())) {
                Ok(()) => {}
                Err(RecvTimeoutError::Timeout) => {
                    // Only the winner may expire or execute. If acceptance won,
                    // wait for completion even if the ack arrived at the deadline.
                    if state
                        .compare_exchange(PENDING, EXPIRED, Ordering::SeqCst, Ordering::SeqCst)
                        .is_ok()
                        || state.load(Ordering::SeqCst) == EXPIRED
                    {
                        return Err(ControlError::NotAccepted);
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    if state.load(Ordering::SeqCst) == EXPIRED {
                        return Err(ControlError::NotAccepted);
                    }
                    return Err(ControlError::EditorClosed);
                }
            }
            // Accepted jobs have no completion deadline (exports and analysis
            // can take hours). A dropped sender wakes recv immediately.
            reply_rx.recv().map_err(|_| ControlError::EditorClosed)?
        })
    }
}

impl ControlHandler for ControlBridge {
    fn handle(&mut self, call: Call) -> Result<Value, ControlError> {
        self.defer(call)()
    }

    fn defer(&mut self, call: Call) -> ControlReply {
        self.defer_at(call, Instant::now(), Instant::now)
    }
}

/// Owned by the editor so the server thread and its socket live as long as the
/// window does. The receiver is drained on the UI thread during the render
/// loop.
pub(crate) struct ControlRuntime {
    pub(crate) rx: Receiver<ControlMsg>,
    /// Kept alive for its [`Drop`] side effect: shutting the server down when
    /// the editor exits and removing the socket and token files.
    #[allow(dead_code)]
    pub(crate) server: Option<RunningServer>,
    /// Startup failure shown in the MCP status chip instead of silently staying off.
    pub(crate) error: Option<String>,
}

/// Start the control server on the well-known per-user socket.
pub(crate) fn start(bridge: ControlBridge) -> Result<RunningServer, String> {
    start_with_spec(bridge, animathio_control::default_socket_spec())
}

fn start_with_spec(
    bridge: ControlBridge,
    spec: std::io::Result<animathio_control::SocketSpec>,
) -> Result<RunningServer, String> {
    let result = spec
        .and_then(ControlServer::bind)
        .and_then(|server| server.spawn(bridge));
    match result {
        Ok(server) => {
            eprintln!(
                "control server listening at {} (token {})",
                server.socket_path().display(),
                server.token_file().display()
            );
            Ok(server)
        }
        Err(error) => {
            let message = format!("control server unavailable: {error}");
            eprintln!("{message}");
            Err(message)
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn long_socket_paths_report_a_startup_failure() {
        let (bridge, _) = ControlBridge::new();
        // Binding fails before a worker is spawned. No process-wide env is changed.
        let root =
            std::env::temp_dir().join(format!("animathio-long-socket-{}", std::process::id()));
        let spec = animathio_control::SocketSpec {
            path: root.join("x".repeat(150)),
            token_file: root.join("token"),
        };
        let Err(error) = start_with_spec(bridge, Ok(spec)) else {
            panic!("overlong Unix socket unexpectedly bound");
        };
        assert!(error.starts_with("control server unavailable:"));
        assert!(!error.ends_with(':'));
        let _ = std::fs::remove_dir(&root);
    }
}

#[cfg(test)]
mod acknowledgement_tests {
    use super::*;
    use animathio_core::{Command, Track, TrackId};
    use std::sync::mpsc::channel;

    fn edit() -> Call {
        Call::CommandApply {
            command: Command::AddTrack {
                track: Track::new(TrackId(42), "accepted once"),
                index: 0,
            },
        }
    }

    #[test]
    fn busy_editor_times_out_and_cannot_apply_later_or_duplicate_retry() {
        let (mut bridge, rx) = ControlBridge::new();
        bridge.ack_deadline = Duration::from_millis(10);
        let wait = bridge.defer(edit());
        assert!(matches!(wait(), Err(ControlError::NotAccepted)));
        let mut editor = animathio_control::HeadlessHandler::new();
        assert!(rx.recv().unwrap().accept().is_none());
        assert!(editor.project_get().unwrap().project.tracks().is_empty());

        // Retry after UI recovery applies exactly once.
        bridge.ack_deadline = Duration::from_secs(5);
        let wait = bridge.defer(edit());
        let (call, reply) = rx.recv().unwrap().accept().unwrap();
        reply.send(editor.handle(call)).unwrap();
        wait().unwrap();
        assert_eq!(editor.project_get().unwrap().project.tracks().len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn busy_editor_returns_json_rpc_error_and_discards_the_queued_edit() {
        use std::os::unix::fs::DirBuilderExt;
        let root = std::env::temp_dir().join(format!("amio-ack-{}", std::process::id()));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .unwrap();
        let spec = animathio_control::SocketSpec {
            path: root.join("control.sock"),
            token_file: root.join("control.token"),
        };
        let (mut bridge, rx) = ControlBridge::new();
        bridge.ack_deadline = Duration::from_millis(10);
        let running = ControlServer::bind(spec.clone())
            .unwrap()
            .spawn(bridge)
            .unwrap();
        let mut client = animathio_control::ControlClient::connect(&spec).unwrap();
        let (result_tx, result_rx) = channel();
        let worker = std::thread::spawn(move || result_tx.send(client.call(edit())).unwrap());
        // No editor polling until the client has received its timeout response.
        let result = result_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(result, Err(ControlError::NotAccepted)));
        assert!(rx.recv().unwrap().accept().is_none());
        worker.join().unwrap();
        drop(running);
        let _ = std::fs::remove_file(spec.path);
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn editor_checks_deadline_even_before_server_waits() {
        let (mut bridge, rx) = ControlBridge::new();
        let wait = bridge.defer(edit());
        let request = rx.recv().unwrap();
        let deadline = request.deadline;
        assert!(request.accept_at(deadline).is_none());
        assert!(matches!(wait(), Err(ControlError::NotAccepted)));
    }

    #[test]
    fn accepted_job_completes_after_ack_deadline() {
        let (mut bridge, rx) = ControlBridge::new();
        // Simulate a client resuming its wait 45 minutes after acceptance.
        // Advancing a clock avoids sleeping or subtracting past Instant's epoch.
        let queued_at = Instant::now();
        let completed_at = queued_at + Duration::from_secs(45 * 60);
        let wait = bridge.defer_at(
            Call::DetectScenes {
                params: Default::default(),
            },
            queued_at,
            move || completed_at,
        );
        let request = rx.recv().unwrap();
        let (call, reply) = request.accept_at(queued_at).unwrap();
        assert!(matches!(call, Call::DetectScenes { .. }));
        let waiter = std::thread::spawn(wait);
        reply.send(Ok(Value::from("long job completed"))).unwrap();
        assert_eq!(waiter.join().unwrap().unwrap(), "long job completed");
    }

    #[test]
    fn acceptance_wins_timeout_race_even_before_ack_is_sent() {
        let (mut bridge, rx) = ControlBridge::new();
        bridge.ack_deadline = Duration::ZERO;
        let wait = bridge.defer(edit());
        let request = rx.recv().unwrap();
        // Pause the UI between the acceptance CAS and acknowledgement send.
        request.state.store(ACCEPTED, Ordering::SeqCst);
        request.reply.send(Ok(Value::Null)).unwrap();
        wait().unwrap();
    }

    #[test]
    fn dropped_queued_or_accepted_reply_fails_immediately() {
        for accepted in [false, true] {
            let (mut bridge, rx) = ControlBridge::new();
            bridge.ack_deadline = Duration::from_secs(60 * 60);
            let wait = bridge.defer(edit());
            let request = rx.recv().unwrap();
            if accepted {
                drop(request.accept().unwrap());
            } else {
                drop(request);
            }
            let (tx, rx) = channel();
            let worker = std::thread::spawn(move || tx.send(wait()).unwrap());
            assert!(matches!(
                rx.recv_timeout(Duration::from_secs(2)).unwrap(),
                Err(ControlError::EditorClosed)
            ));
            worker.join().unwrap();
        }
    }

    #[test]
    fn retry_queue_is_bounded_and_full_requests_are_never_applied() {
        let (mut bridge, rx) = ControlBridge::new();
        let waiting: Vec<_> = (0..MAX_QUEUED_REQUESTS)
            .map(|_| bridge.defer(edit()))
            .collect();
        assert!(matches!(
            bridge.handle(edit()),
            Err(ControlError::NotAccepted)
        ));
        for wait in waiting {
            let (_, reply) = rx.recv().unwrap().accept().unwrap();
            reply.send(Ok(Value::Null)).unwrap();
            wait().unwrap();
        }
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn closed_editor_fails_without_waiting_for_ack() {
        let (mut bridge, rx) = ControlBridge::new();
        drop(rx);
        assert!(matches!(
            bridge.handle(edit()),
            Err(ControlError::EditorClosed)
        ));
    }
}
