//! Local socket / named pipe transport with newline-delimited JSON-RPC 2.0.
//!
//! The socket is a Unix-domain socket in `$XDG_RUNTIME_DIR` on Unix and a
//! named pipe on Windows. A random token is written to a `0600` file next to
//! the socket; every request must carry it and it is compared in constant
//! time. Requests are capped at [`MAX_REQUEST_BYTES`].

use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use interprocess::local_socket::{
    ListenerNonblockingMode, ListenerOptions, Name, Listener as LocalSocketListener,
    Stream as LocalSocketStream,
    traits::{Listener as _, Stream as _},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{Call, ControlHandler, token};

/// Maximum size of one incoming control request (16 MiB).
pub const MAX_REQUEST_BYTES: usize = 16 * 1024 * 1024;
/// Maximum size of one response line; renders can be several megabytes.
pub(crate) const MAX_RESPONSE_BYTES: usize = 64 * 1024 * 1024;

/// Filesystem anchor and token file for the control socket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SocketSpec {
    /// Unix socket path (or, on Windows, the anchor used to place the token).
    pub path: PathBuf,
    /// Token file created next to the socket.
    pub token_file: PathBuf,
}

/// The well-known per-user socket location.
pub fn default_socket_spec() -> io::Result<SocketSpec> {
    let dir = runtime_dir()?;
    ensure_runtime_dir(&dir)?;
    Ok(SocketSpec {
        path: dir.join("animathio-control.sock"),
        token_file: dir.join("animathio-control.token"),
    })
}

/// A bound control server. `serve` runs the accept loop; `spawn` moves it onto
/// a dedicated thread.
pub struct ControlServer {
    spec: SocketSpec,
    listener: LocalSocketListener,
    token: String,
}

impl ControlServer {
    /// Bind the socket, claim the well-known path and write the token file.
    pub fn bind(spec: SocketSpec) -> io::Result<Self> {
        if let Some(parent) = spec.path.parent().filter(|p| !p.as_os_str().is_empty()) {
            ensure_runtime_dir(parent)?;
        }
        let token = token::generate()?;
        let listener = bind_listener(&spec.path)?;
        token::write_file(&spec.token_file, &token)?;
        Ok(Self {
            spec,
            listener,
            token,
        })
    }

    /// The socket path this server listens on.
    #[must_use]
    pub fn socket_path(&self) -> &Path {
        &self.spec.path
    }

    /// The token file path.
    #[must_use]
    pub fn token_file(&self) -> &Path {
        &self.spec.token_file
    }

    /// Serve connections until `shutdown` is set. The listener is nonblocking
    /// so shutdown is observed promptly between connections.
    pub fn serve<H: ControlHandler>(
        &mut self,
        handler: &mut H,
        shutdown: &AtomicBool,
    ) -> io::Result<()> {
        self.listener
            .set_nonblocking(ListenerNonblockingMode::Accept)?;
        loop {
            if shutdown.load(Ordering::Relaxed) {
                return Ok(());
            }
            match self.listener.accept() {
                Ok(stream) => self.serve_connection(stream, handler)?,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return Err(error),
            }
        }
    }

    /// Serve exactly one connection, blocking until it closes.
    pub fn serve_once<H: ControlHandler>(&mut self, handler: &mut H) -> io::Result<()> {
        let stream = self.listener.accept()?;
        self.serve_connection(stream, handler)
    }

    /// Move the server onto a dedicated thread and return its handle.
    pub fn spawn<H: ControlHandler + Send + 'static>(self, handler: H) -> io::Result<RunningServer> {
        let path = self.spec.path.clone();
        let token_file = self.spec.token_file.clone();
        let shutdown = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&shutdown);
        let handle = thread::Builder::new()
            .name("animathio-control".to_owned())
            .spawn(move || {
                let mut handler = handler;
                let mut server = self;
                let _ = server.serve(&mut handler, &flag);
            })?;
        Ok(RunningServer {
            handle: Some(handle),
            shutdown,
            path,
            token_file,
        })
    }

    fn serve_connection<H: ControlHandler>(
        &mut self,
        stream: LocalSocketStream,
        handler: &mut H,
    ) -> io::Result<()> {
        let mut stream = stream;
        loop {
            let Some(message) = read_message(&mut stream, MAX_REQUEST_BYTES)? else {
                return Ok(());
            };
            let response = respond(handler, &self.token, &message);
            stream.write_all(&response)?;
            stream.write_all(b"\n")?;
            stream.flush()?;
        }
    }
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.spec.token_file);
    }
}

/// Handle to a control server running on its own thread.
pub struct RunningServer {
    handle: Option<JoinHandle<()>>,
    shutdown: Arc<AtomicBool>,
    path: PathBuf,
    token_file: PathBuf,
}

impl RunningServer {
    /// The socket path.
    #[must_use]
    pub fn socket_path(&self) -> &Path {
        &self.path
    }

    /// The token file path.
    #[must_use]
    pub fn token_file(&self) -> &Path {
        &self.token_file
    }

    /// Request that the accept loop stop.
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }

    /// Join the server thread.
    pub fn join(&mut self) {
        if let Some(handle) = self.handle.take()
            && let Err(error) = handle.join()
        {
            eprintln!("control server thread panicked: {error:?}");
        }
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        self.shutdown();
        self.join();
    }
}

#[derive(Deserialize)]
struct RawRequest {
    #[serde(default)]
    jsonrpc: Option<String>,
    #[serde(default)]
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
    #[serde(default)]
    token: Option<String>,
}

fn respond<H: ControlHandler>(handler: &mut H, expected_token: &str, message: &[u8]) -> Vec<u8> {
    let request = match serde_json::from_slice::<RawRequest>(message) {
        Ok(request) => request,
        Err(_) => {
            return json!({
                "jsonrpc": "2.0",
                "id": Value::Null,
                "error": {"code": -32_700, "message": "parse error"},
            })
            .to_string()
            .into_bytes();
        }
    };
    let id = request.id;
    if !token::matches(request.token.as_deref(), expected_token) {
        return error_response(id, -32_001, "authentication failed");
    }
    if request.jsonrpc.as_deref() != Some("2.0") {
        return error_response(id, -32_600, "invalid JSON-RPC version");
    }
    let call = serde_json::from_value::<Call>(json!({
        "method": request.method,
        "params": request.params,
    }));
    let call = match call {
        Ok(call) => call,
        Err(error) => {
            return error_response(id, -32_602, &format!("invalid params: {error}"));
        }
    };
    match handler.handle(call) {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result})
            .to_string()
            .into_bytes(),
        Err(error) => error_response(id, error.code(), &error.to_string()),
    }
}

fn error_response(id: Value, code: i64, message: &str) -> Vec<u8> {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": code, "message": message},
    })
    .to_string()
    .into_bytes()
}

/// Read one newline-delimited message without buffering more than `limit` bytes.
pub(crate) fn read_message(
    reader: &mut impl Read,
    limit: usize,
) -> io::Result<Option<Vec<u8>>> {
    let mut message = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            if message.is_empty() {
                return Ok(None);
            }
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "truncated control message",
            ));
        }
        message.extend_from_slice(&chunk[..read]);
        if message.len() > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "control message exceeds size limit",
            ));
        }
        if let Some(position) = message.iter().position(|&byte| byte == b'\n') {
            message.truncate(position);
            if message.last() == Some(&b'\r') {
                message.pop();
            }
            return Ok(Some(message));
        }
    }
}

fn bind_listener(path: &Path) -> io::Result<LocalSocketListener> {
    let name = local_name(path)?;
    match ListenerOptions::new().name(name.clone()).create_sync() {
        Ok(listener) => Ok(listener),
        Err(first) if first.kind() == io::ErrorKind::AddrInUse => {
            // Either a corpse socket from a crashed process or a live editor.
            // Connecting distinguishes them: a live server accepts the probe.
            if LocalSocketStream::connect(name.clone()).is_ok() {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "another editor already owns the control socket",
                ));
            }
            remove_stale_socket(path)?;
            ListenerOptions::new().name(name).create_sync()
        }
        Err(other) => Err(other),
    }
}

pub(crate) fn local_name(path: &Path) -> io::Result<Name<'static>> {
    #[cfg(unix)]
    {
        use interprocess::local_socket::{GenericFilePath, ToFsName};
        path.to_fs_name::<GenericFilePath>().map(Name::into_owned)
    }
    #[cfg(windows)]
    {
        use interprocess::local_socket::{GenericNamespaced, ToNsName};
        let _ = path;
        "animathio-control"
            .to_ns_name::<GenericNamespaced>()
            .map(Name::into_owned)
    }
}

fn remove_stale_socket(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
    #[cfg(windows)]
    {
        let _ = path;
        Ok(())
    }
}

#[cfg(unix)]
fn runtime_dir() -> io::Result<PathBuf> {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    Ok(base.join("animathio"))
}

#[cfg(windows)]
fn runtime_dir() -> io::Result<PathBuf> {
    Ok(std::env::temp_dir().join("animathio"))
}

fn ensure_runtime_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder.create(dir)
    }
    #[cfg(windows)]
    {
        fs::create_dir_all(dir)
    }
}
