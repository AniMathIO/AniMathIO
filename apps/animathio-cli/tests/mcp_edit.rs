//! End-to-end MCP test for the high-level edit tools: `describe_timeline`,
//! `add_title`, `cut_at`, `remove_range`, `batch` and `set_export_preset`.

use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{ChildStdin, ChildStdout, Command, Stdio},
};

use serde_json::{Value, json};
use tempfile::tempdir;

struct Session {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next: u64,
}

impl Session {
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let request =
            json!({"jsonrpc": "2.0", "id": self.next, "method": method, "params": params});
        writeln!(self.stdin, "{request}").unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        assert!(
            self.stdout.read_line(&mut line).unwrap() > 0,
            "server closed"
        );
        serde_json::from_str(&line).unwrap()
    }

    /// Call a tool and return its first text block.
    fn tool(&mut self, name: &str, arguments: Value) -> String {
        let response = self.request("tools/call", json!({"name": name, "arguments": arguments}));
        assert!(response.get("error").is_none(), "{response}");
        assert_ne!(response["result"]["isError"], true, "{response}");
        response["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn json_tool(&mut self, name: &str, arguments: Value) -> Value {
        serde_json::from_str(&self.tool(name, arguments)).unwrap()
    }
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

#[test]
fn describe_cut_remove_batch_and_preset() {
    let dir = tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_animathio-cli"))
        .args(["mcp", "--project"])
        .arg(empty_project(dir.path()))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut session = Session {
        stdout: BufReader::new(child.stdout.take().unwrap()),
        stdin: child.stdin.take().unwrap(),
        next: 0,
    };
    let init = session.request(
        "initialize",
        json!({"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"mcp-edit-test","version":"0.0.0"}}),
    );
    assert!(init["result"]["serverInfo"].is_object(), "{init}");

    let tools = session.request("tools/list", json!({}));
    let names: Vec<_> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_owned())
        .collect();
    for expected in [
        "describe_timeline",
        "cut_at",
        "remove_range",
        "ripple_delete",
        "trim_silence",
        "add_title",
        "batch",
        "set_export_preset",
    ] {
        assert!(names.iter().any(|name| name == expected), "{names:?}");
    }

    let preset = session.json_tool("set_export_preset", json!({"preset": "youtube_1080p"}));
    assert_eq!(preset["width"], 1920);

    let added = session.json_tool(
        "add_title",
        json!({"text": "Welcome", "start_ms": 0, "duration_ms": 3000, "style": "title"}),
    );
    let clip = added["clip_id"].as_u64().unwrap();

    let text = session.tool("describe_timeline", json!({}));
    assert!(text.contains("1920x1080 @ 30 fps"), "{text}");
    assert!(text.contains("\"Titles\""), "{text}");
    assert!(
        text.contains(&format!(
            "clip {clip} 00:00:00.000 - 00:00:03.000 text \"Welcome\""
        )),
        "{text}"
    );

    let cut = session.json_tool("cut_at", json!({"times_ms": [1000, 5000]}));
    assert_eq!(cut["new_clips"], 1, "{cut}");
    let text = session.tool("describe_timeline", json!({}));
    assert!(text.contains("00:00:00.000 - 00:00:01.000"), "{text}");
    assert!(text.contains("00:00:01.000 - 00:00:03.000"), "{text}");

    session.json_tool("remove_range", json!({"from_ms": 500, "to_ms": 1500}));
    let text = session.tool("describe_timeline", json!({}));
    assert!(text.contains("duration 00:00:02.000"), "{text}");

    // Two ripple deletes in one batch are one undo step.
    let state = session.json_tool("get_project_state", json!({}));
    let ids: Vec<u64> = state["project"]["tracks"][0]["clips"]
        .as_object()
        .unwrap()
        .values()
        .map(|clip| clip["id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids.len(), 2);
    let ops: Vec<_> = ids
        .iter()
        .map(|id| json!({"RemoveClip": {"id": id}}))
        .collect();
    session.json_tool("batch", json!({ "ops": ops }));
    assert!(
        session
            .tool("describe_timeline", json!({}))
            .contains("0 clip(s)")
    );
    session.json_tool("undo", json!({}));
    assert!(
        session
            .tool("describe_timeline", json!({}))
            .contains("2 clip(s)")
    );

    drop(session);
    assert!(child.wait().unwrap().success());
}
