//! Control-server bridge: the server thread forwards each call over a channel
//! and the UI thread drains it during the render loop, keeping all project
//! mutation on the same thread that owns the editor.

use std::sync::mpsc::{Receiver, Sender, SyncSender, channel, sync_channel};

use animathio_control::{Call, ControlError, ControlHandler, ControlServer, RunningServer};
use serde_json::Value;

/// One routed call and the channel for its serialized reply.
pub(crate) enum ControlMsg {
    Invoke(Call, SyncSender<Result<Value, ControlError>>),
}

/// The server-side half of the bridge; it implements [`ControlHandler`] by
/// forwarding every call to the UI thread and waiting for the reply.
pub(crate) struct ControlBridge {
    tx: Sender<ControlMsg>,
}

impl ControlBridge {
    pub(crate) fn new() -> (Self, Receiver<ControlMsg>) {
        let (tx, rx) = channel();
        (Self { tx }, rx)
    }
}

impl ControlHandler for ControlBridge {
    fn handle(&mut self, call: Call) -> Result<Value, ControlError> {
        let (reply_tx, reply_rx) = sync_channel(1);
        self.tx
            .send(ControlMsg::Invoke(call, reply_tx))
            .map_err(|_| ControlError::Other("editor shut down".to_owned()))?;
        reply_rx
            .recv()
            .map_err(|_| ControlError::Other("editor did not respond".to_owned()))?
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
