//! Local JSON-RPC 2.0 control protocol over a user-only socket or named pipe.
//!
//! The control layer is the single API for the editor UI, the CLI, plugins and
//! the MCP sidecar. Mutations are [`animathio_core::Command`]s applied through
//! the same [`animathio_core::History`] as UI edits; reads resolve pure
//! evaluation. The transport is a Unix-domain socket in `$XDG_RUNTIME_DIR` on
//! Unix and a named pipe on Windows, never a listening TCP port.
//!
//! Authentication is a random token written to a `0600` file next to the
//! socket; clients must send it with every request and it is compared in
//! constant time.

mod client;
mod error;
mod handler;
mod headless;
mod method;
mod path;
mod server;
mod token;

pub use client::ControlClient;
pub use error::ControlError;
pub use handler::{ControlHandler, dispatch, from_value};
pub use headless::HeadlessHandler;
pub use method::{
    AssetInfo, Call, ClipInfo, ProjectState, RenderFrameParams, RenderFrameResult, Timeline,
    TrackInfo,
};
pub use path::{validate_existing_file, validate_output_file};
pub use server::{ControlServer, RunningServer, SocketSpec, default_socket_spec, MAX_REQUEST_BYTES};
