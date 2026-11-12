//! Socket round-trip: auth rejection, a command through history, and a
//! rendered frame over the wire.

use animathio_control::{
    Call, ControlClient, ControlError, ControlHandler, ControlServer, HeadlessHandler,
    ProjectState, RenderFrameParams, RenderFrameResult, SocketSpec, from_value,
};
use animathio_core::{Command, Time, Track, TrackId};

fn spec(dir: &std::path::Path) -> SocketSpec {
    SocketSpec {
        path: dir.join("control.sock"),
        token_file: dir.join("control.token"),
    }
}

#[test]
fn authoring_methods_round_trip_over_the_authenticated_socket() {
    let dir = private_dir();
    let spec = spec(dir.path());
    let server = ControlServer::bind(spec.clone()).unwrap();
    let mut running = server.spawn(HeadlessHandler::new()).unwrap();
    let mut client = ControlClient::connect(&spec).unwrap();
    for (method, params) in [
        ("math.add", serde_json::json!({"latex":"x^2"})),
        ("graph.add", serde_json::json!({"items":"plot y=x"})),
        (
            "import.manim",
            serde_json::json!({"source":"from manim import *\nclass Demo(Scene):\n    def construct(self):\n        c = Circle()\n        self.add(c)\n        self.wait(1)\n"}),
        ),
    ] {
        let call =
            serde_json::from_value(serde_json::json!({"method":method,"params":{"params":params}}))
                .unwrap();
        assert!(client.call(call).is_ok(), "{method}");
    }
    let state = client.project_get().unwrap();
    assert_eq!(state.project.tracks().len(), 3);
    state.project.validate().unwrap();
    drop(client);
    running.shutdown();
    running.join();
}

fn private_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    dir
}

#[test]
fn wrong_token_is_rejected_and_correct_token_round_trips() {
    let dir = private_dir();
    let spec = spec(dir.path());

    let server = ControlServer::bind(spec.clone()).unwrap();
    let mut running = server.spawn(HeadlessHandler::new()).unwrap();

    // A wrong token is rejected with the auth error code.
    let mut bad = ControlClient::connect_with_token(&spec, "wrong-token").unwrap();
    let rejected = bad.call(Call::ProjectGet);
    assert!(
        matches!(rejected, Err(ControlError::Unauthorized)),
        "{rejected:?}"
    );
    drop(bad);

    // A correct token performs a command through history and reads it back.
    let mut client = ControlClient::connect(&spec).unwrap();
    client
        .call(Call::CommandApply {
            command: Command::AddTrack {
                track: Track::new(TrackId(7), "Control"),
                index: 0,
            },
        })
        .unwrap();
    let state: ProjectState = from_value(client.call(Call::ProjectGet).unwrap()).unwrap();
    assert_eq!(state.project.tracks().len(), 1);
    assert_eq!(state.project.tracks()[0].id(), TrackId(7));
    assert_eq!(state.project.tracks()[0].name(), "Control");

    let rendered: RenderFrameResult = from_value(
        client
            .call(Call::RenderFrame {
                params: RenderFrameParams {
                    time: Time(0),
                    width: 16,
                    height: 16,
                    output: None,
                },
            })
            .unwrap(),
    )
    .unwrap();
    let decoded = base64_decode(rendered.base64.unwrap());
    assert_eq!(&decoded[..8], b"\x89PNG\r\n\x1a\n");
    drop(client);

    running.shutdown();
    running.join();
}

fn base64_decode(value: String) -> Vec<u8> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .unwrap()
}

#[cfg(unix)]
#[test]
fn bind_refuses_pre_created_world_writable_dir() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let socket_dir = dir.path().join("run");
    std::fs::create_dir(&socket_dir).unwrap();
    std::fs::set_permissions(&socket_dir, std::fs::Permissions::from_mode(0o777)).unwrap();

    let spec = SocketSpec {
        path: socket_dir.join("control.sock"),
        token_file: socket_dir.join("control.token"),
    };
    assert!(ControlServer::bind(spec).is_err());
}

#[cfg(unix)]
#[test]
fn bind_does_not_follow_symlinked_token_path() {
    let dir = private_dir();
    let target = dir.path().join("target");
    std::fs::write(&target, b"original").unwrap();
    let token_file = dir.path().join("control.token");
    std::os::unix::fs::symlink(&target, &token_file).unwrap();

    let spec = SocketSpec {
        path: dir.path().join("control.sock"),
        token_file,
    };
    let server = ControlServer::bind(spec).unwrap();
    let token_file = server.token_file().to_path_buf();

    // The symlink itself is replaced by the server, not the file it pointed at.
    let meta = std::fs::symlink_metadata(&token_file).unwrap();
    assert!(!meta.file_type().is_symlink());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "original");
    drop(server);
}

#[test]
fn concurrent_clients_disconnect_and_idle_shutdown() {
    let dir = private_dir();
    let spec = spec(dir.path());
    let running = ControlServer::bind(spec.clone())
        .unwrap()
        .spawn(HeadlessHandler::new())
        .unwrap();
    let mut first = ControlClient::connect(&spec).unwrap();
    let mut second = ControlClient::connect(&spec).unwrap();
    first.project_get().unwrap();
    second.project_get().unwrap();
    drop(first);
    let mut third = ControlClient::connect(&spec).unwrap();
    third.project_get().unwrap();
    // Both remaining clients stay connected and idle during shutdown.
    let start = std::time::Instant::now();
    drop(running);
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
}

#[cfg(unix)]
#[test]
fn truncated_request_does_not_stop_accepting() {
    use std::io::Write;
    let dir = private_dir();
    let spec = spec(dir.path());
    let running = ControlServer::bind(spec.clone())
        .unwrap()
        .spawn(HeadlessHandler::new())
        .unwrap();
    let mut broken = std::os::unix::net::UnixStream::connect(&spec.path).unwrap();
    broken.write_all(b"{truncated").unwrap();
    drop(broken);
    let mut client = ControlClient::connect(&spec).unwrap();
    client.project_get().unwrap();
    drop(running);
}
