//! Control-server bridge: the server thread forwards each call over a channel
//! and the UI thread drains it during the render loop, keeping all project
//! mutation on the same thread that owns the editor.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
        mpsc::{Receiver, RecvTimeoutError, Sender, SyncSender, channel, sync_channel},
    },
    time::{Duration, Instant},
};

use animathio_control::{Call, ControlError, ControlHandler, ControlReply, ControlServer, RunningServer};
use serde_json::Value;

const ACK_DEADLINE: Duration = Duration::from_secs(10);
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
            let _ = self.state.compare_exchange(PENDING, EXPIRED, Ordering::SeqCst, Ordering::SeqCst);
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
    tx: Sender<ControlMsg>,
    ack_deadline: Duration,
}

impl ControlBridge {
    pub(crate) fn new() -> (Self, Receiver<ControlMsg>) {
        let (tx, rx) = channel();
        (Self { tx, ack_deadline: ACK_DEADLINE }, rx)
    }
}

impl ControlHandler for ControlBridge {
    fn handle(&mut self, call: Call) -> Result<Value, ControlError> {
        self.defer(call)()
    }

    fn defer(&mut self, call: Call) -> ControlReply {
        let (reply_tx, reply_rx) = sync_channel(1);
        let (ack_tx, ack_rx) = sync_channel(1);
        let state = Arc::new(AtomicU8::new(PENDING));
        let deadline = Instant::now() + self.ack_deadline;
        let sent = self.tx.send(ControlMsg {
            call, reply: reply_tx, ack: ack_tx, state: state.clone(), deadline,
        });
        Box::new(move || {
            sent.map_err(|_| ControlError::EditorClosed)?;
            match ack_rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(()) => {},
                Err(RecvTimeoutError::Timeout) => {
                    // Only the winner may expire or execute. If acceptance won,
                    // wait for completion even if the ack arrived at the deadline.
                    if state.compare_exchange(PENDING, EXPIRED, Ordering::SeqCst, Ordering::SeqCst).is_ok()
                        || state.load(Ordering::SeqCst) == EXPIRED {
                        return Err(ControlError::NotAccepted);
                    }
                },
                Err(RecvTimeoutError::Disconnected) => {
                    if state.load(Ordering::SeqCst) == EXPIRED {
                        return Err(ControlError::NotAccepted);
                    }
                    return Err(ControlError::EditorClosed);
                },
            }
            // Accepted jobs have no completion deadline (exports and analysis
            // can take hours). A dropped sender wakes recv immediately.
            reply_rx.recv().map_err(|_| ControlError::EditorClosed)?
        })
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
