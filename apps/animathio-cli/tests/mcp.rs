//! End-to-end MCP test: spawn `animathio-cli mcp --project …`, speak the MCP
//! stdio protocol, add a clip and render a frame.

use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

use serde_json::{Value, json};
use tempfile::tempdir;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_animathio-cli")
}

fn spawn_mcp(project: &Path) -> (Child, BufReader<ChildStdout>, ChildStdin) {
    let mut child = Command::new(bin())
        .args(["mcp", "--project"])
        .arg(project)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let stdout = BufReader::new(child.stdout.take().unwrap());
    let stdin = child.stdin.take().unwrap();
    (child, stdout, stdin)
}

fn send(stdin: &mut ChildStdin, id: u64, method: &str, params: impl serde::Serialize) {
    let request = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
    writeln!(stdin, "{request}").unwrap();
    stdin.flush().unwrap();
}

fn recv(stdout: &mut BufReader<ChildStdout>) -> Value {
    let mut line = String::new();
    let read = stdout.read_line(&mut line).unwrap();
    assert!(read > 0, "MCP server closed stdout early");
    serde_json::from_str(&line).unwrap()
}

#[test]
fn lists_tools_adds_a_clip_and_renders_a_frame() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("demo.amio");

    let sample = Command::new(bin())
        .args(["new-sample", "-o"])
        .arg(&archive)
        .output()
        .unwrap();
    assert!(
        sample.status.success(),
        "{}",
        String::from_utf8_lossy(&sample.stderr)
    );

    let (mut child, mut stdout, mut stdin) = spawn_mcp(&archive);

    // initialize handshake
    send(
        &mut stdin,
        1,
        "initialize",
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "animathio-mcp-test", "version": "0.0.0"},
        }),
    );
    let response = recv(&mut stdout);
    assert_eq!(response["id"], 1, "{response}");
    assert!(response["result"]["serverInfo"].is_object(), "{response}");

    // tools/list exposes the v1-parity surface plus the v2 additions
    send(&mut stdin, 2, "tools/list", json!({}));
    let response = recv(&mut stdout);
    let tools = response["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    for expected in [
        "get_project_state",
        "add_text",
        "add_media",
        "add_animation",
        "update_element",
        "remove_element",
        "set_canvas",
        "seek",
        "set_playing",
        "save_project",
        "export_video",
        "render_frame",
        "undo",
        "redo",
        "import_v1",
    ] {
        assert!(
            names.contains(&expected),
            "missing tool {expected}: {names:?}"
        );
    }

    // add_text creates a clip through the control command path
    send(
        &mut stdin,
        3,
        "tools/call",
        json!({"name": "add_text", "arguments": {"text": "hello", "x": 32.0, "y": 16.0}}),
    );
    let response = recv(&mut stdout);
    assert_ne!(response["result"]["isError"], true, "{response}");

    // render_frame returns a base64 PNG
    send(
        &mut stdin,
        4,
        "tools/call",
        json!({"name": "render_frame", "arguments": {"frame": 0, "width": 32, "height": 32}}),
    );
    let response = recv(&mut stdout);
    assert_ne!(response["result"]["isError"], true, "{response}");
    let text = response["result"]["content"][0]["text"].as_str().unwrap();
    let result: Value = serde_json::from_str(text).unwrap();
    let base64 = result["base64"]
        .as_str()
        .expect("render_frame must return base64");
    let decoded = base64_decode(base64);
    assert_eq!(&decoded[..8], b"\x89PNG\r\n\x1a\n");

    // undo/redo round-trip through history
    send(
        &mut stdin,
        5,
        "tools/call",
        json!({"name": "undo", "arguments": {}}),
    );
    let response = recv(&mut stdout);
    assert_ne!(response["result"]["isError"], true, "{response}");

    drop(stdin);
    let _ = child.wait();
}

fn base64_decode(value: &str) -> Vec<u8> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .unwrap()
}
