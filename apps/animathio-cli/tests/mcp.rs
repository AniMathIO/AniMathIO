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

    initialize(&mut stdin, &mut stdout);

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
        "add_math",
        "add_graph",
        "import_manim_scene",
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

fn initialize(stdin: &mut ChildStdin, stdout: &mut BufReader<ChildStdout>) {
    send(
        stdin,
        1,
        "initialize",
        json!({"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"animathio-mcp-test","version":"0.0.0"}}),
    );
    let response = recv(stdout);
    assert!(response["result"]["serverInfo"].is_object(), "{response}");
}

fn call(
    stdin: &mut ChildStdin,
    stdout: &mut BufReader<ChildStdout>,
    name: &str,
    arguments: Value,
) -> Value {
    send(
        stdin,
        10,
        "tools/call",
        json!({"name": name, "arguments": arguments}),
    );
    let response = recv(stdout);
    assert!(response.get("error").is_none(), "{response}");
    assert_ne!(response["result"]["isError"], true, "{response}");
    serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
}

fn empty_project(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("project.json");
    animathio_format::write_json(
        std::fs::File::create(&path).unwrap(),
        &animathio_core::Project::default(),
    )
    .unwrap();
    path
}

fn build_plugins() -> Option<std::path::PathBuf> {
    let targets = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output();
    if !targets.is_ok_and(|output| {
        output.status.success()
            && String::from_utf8_lossy(&output.stdout)
                .lines()
                .any(|target| target == "wasm32-wasip2")
    }) {
        eprintln!("skipping MCP math render test: wasm32-wasip2 is unavailable");
        return None;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let status = Command::new("bash")
        .arg("scripts/build-plugins.sh")
        .current_dir(root)
        .env("CARGO_BUILD_JOBS", "3")
        .env("CARGO_PROFILE_DEV_DEBUG", "line-tables-only")
        .status()
        .unwrap();
    assert!(status.success(), "failed to build first-party plugins");
    Some(root.join("target/plugins"))
}

#[test]
fn math_reveal_renders_visible_plugin_outlines_and_graph_is_added() {
    let Some(plugins) = build_plugins() else {
        return;
    };
    let dir = tempdir().unwrap();
    let project = empty_project(dir.path());
    let mut child = Command::new(bin())
        .args(["mcp", "--project"])
        .arg(project)
        .env("ANIMATHIO_PLUGIN_DIR", plugins)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut stdin = child.stdin.take().unwrap();
    initialize(&mut stdin, &mut stdout);
    call(
        &mut stdin,
        &mut stdout,
        "set_canvas",
        json!({"width":320,"height":180,"background_color":"#000000","max_time_ms":2000}),
    );
    call(
        &mut stdin,
        &mut stdout,
        "add_math",
        json!({"latex":"x^2 + y^2 = z^2","size":32,"x":160,"y":90,"duration_ms":2000,"reveal_duration_ms":1000}),
    );
    let first = call(
        &mut stdin,
        &mut stdout,
        "render_frame",
        json!({"time_ms":0,"width":320,"height":180}),
    );
    let last = call(
        &mut stdin,
        &mut stdout,
        "render_frame",
        json!({"time_ms":1500,"width":320,"height":180}),
    );
    let decode = |value: &Value| {
        image::load_from_memory(&base64_decode(value["base64"].as_str().unwrap()))
            .unwrap()
            .to_rgba8()
    };
    let first = decode(&first);
    let last = decode(&last);
    assert!(
        first.pixels().all(|pixel| pixel.0[..3] == [0, 0, 0]),
        "reveal=0 should be blank; unresolved plugins must not pass"
    );
    assert!(
        last.pixels()
            .any(|pixel| pixel[0] > 100 && pixel[1] > 100 && pixel[2] > 100),
        "math should produce visible outlines"
    );
    call(
        &mut stdin,
        &mut stdout,
        "add_graph",
        json!({"x_min":-2,"x_max":2,"y_min":-1,"y_max":4,"items":"plot y=x^2","draw_duration_ms":500}),
    );
    let state = call(&mut stdin, &mut stdout, "get_project_state", json!({}));
    let serialized = state.to_string();
    assert!(serialized.contains("org.animathio.latex"));
    assert!(serialized.contains("org.animathio.graph"));
    drop(stdin);
    assert!(child.wait().unwrap().success());
}

#[test]
fn bad_latex_is_a_tool_error_without_mutating_the_project() {
    let dir = tempdir().unwrap();
    let (mut child, mut stdout, mut stdin) = spawn_mcp(&empty_project(dir.path()));
    initialize(&mut stdin, &mut stdout);
    send(
        &mut stdin,
        1,
        "tools/call",
        json!({"name":"add_math","arguments":{"latex":"\\frac{"}}),
    );
    let response = recv(&mut stdout);
    assert_eq!(response["result"]["isError"], true, "{response}");
    let text = response["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("LaTeX") || text.contains("typesetting"),
        "{text}"
    );
    let state = call(&mut stdin, &mut stdout, "get_project_state", json!({}));
    assert!(state["project"]["tracks"].as_array().unwrap().is_empty());
    drop(stdin);
    assert!(child.wait().unwrap().success());
}

#[test]
fn manim_file_and_inline_import_append_replace_and_save_round_trip() {
    let dir = tempdir().unwrap();
    let source = "from manim import *\nclass Demo(Scene):\n    def construct(self):\n        c = Circle()\n        self.play(Create(c))\n        self.wait(1)\n";
    let script = dir.path().join("scene.py");
    std::fs::write(&script, source).unwrap();
    let archive = dir.path().join("imported.amio");
    let (mut child, mut stdout, mut stdin) = spawn_mcp(&empty_project(dir.path()));
    initialize(&mut stdin, &mut stdout);
    let report = call(
        &mut stdin,
        &mut stdout,
        "import_manim_scene",
        json!({"path":script,"scene":"Demo"}),
    );
    assert_eq!(report["tracks"], 1);
    assert!(
        report["warnings"].as_array().unwrap().is_empty(),
        "{report}"
    );
    call(
        &mut stdin,
        &mut stdout,
        "import_manim_scene",
        json!({"source":source,"script_dir":dir.path()}),
    );
    let appended = call(&mut stdin, &mut stdout, "get_project_state", json!({}));
    assert_eq!(appended["project"]["tracks"].as_array().unwrap().len(), 2);
    call(
        &mut stdin,
        &mut stdout,
        "import_manim_scene",
        json!({"source":source,"replace":true}),
    );
    let replaced = call(&mut stdin, &mut stdout, "get_project_state", json!({}));
    assert_eq!(replaced["project"]["tracks"].as_array().unwrap().len(), 1);
    call(
        &mut stdin,
        &mut stdout,
        "save_project",
        json!({"path":archive}),
    );
    drop(stdin);
    assert!(child.wait().unwrap().success());
    let (mut child, mut stdout, mut stdin) = spawn_mcp(&archive);
    initialize(&mut stdin, &mut stdout);
    let reopened = call(&mut stdin, &mut stdout, "get_project_state", json!({}));
    assert_eq!(reopened["project"], replaced["project"]);
    drop(stdin);
    assert!(child.wait().unwrap().success());
}

fn base64_decode(value: &str) -> Vec<u8> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .unwrap()
}
