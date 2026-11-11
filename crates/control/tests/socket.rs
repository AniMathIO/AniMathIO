//! Socket round-trip: auth rejection, a command through history, and a
//! rendered frame over the wire.

use animathio_control::{
    Call, ControlClient, ControlError, ControlServer, HeadlessHandler, ProjectState,
    RenderFrameParams, RenderFrameResult, SocketSpec, from_value,
};
use animathio_core::{Command, Time, Track, TrackId};

fn spec(dir: &std::path::Path) -> SocketSpec {
    SocketSpec {
        path: dir.join("control.sock"),
        token_file: dir.join("control.token"),
    }
}

#[test]
fn wrong_token_is_rejected_and_correct_token_round_trips() {
    let dir = tempfile::tempdir().unwrap();
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
