//! End-to-end MCP test for the high-level edit tools: `describe_timeline`,
//! `add_title`, `cut_at`, `remove_range`, `batch` and `set_export_preset`.

use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{ChildStdin, ChildStdout, Command, Stdio},
};

use animathio_media::MediaBackend;
use serde_json::{Value, json};
use tempfile::tempdir;

struct Session {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next: u64,
}

impl Session {
    #[allow(clippy::needless_pass_by_value)]
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
    #[allow(clippy::needless_pass_by_value)]
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
#[allow(clippy::too_many_lines)]
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

    // Importing media is one user edit, including its asset and track.
    let media = dir.path().join("sample.mp4");
    std::fs::write(
        &media,
        include_bytes!("../../../crates/media/tests/fixtures/sample.mp4"),
    )
    .unwrap();
    let before = session.json_tool("get_project_state", json!({}))["project"].clone();
    session.json_tool("add_media", json!({"path": media}));
    let imported = session.json_tool("get_project_state", json!({}))["project"].clone();
    session.json_tool("undo", json!({}));
    assert_eq!(
        session.json_tool("get_project_state", json!({}))["project"],
        before
    );
    session.json_tool("redo", json!({}));
    assert_eq!(
        session.json_tool("get_project_state", json!({}))["project"],
        imported
    );

    drop(session);
    assert!(child.wait().unwrap().success());
}

/// Exercise real container media through the public MCP surface, without external codecs.
#[test]
#[allow(clippy::too_many_lines)]
fn media_edit_save_reopen_and_export() {
    let dir = tempdir().unwrap();
    let media = dir.path().join("source.mp4");
    std::fs::write(
        &media,
        include_bytes!("../../../crates/media/tests/fixtures/sample.mp4"),
    )
    .unwrap();
    // A tiny PCM WAV generated in-process: mono, 48 kHz, two seconds.
    let mut wav = Vec::new();
    let data_len = 48_000_u32 * 2 * 2;
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt \x10\0\0\0\x01\0\x01\0");
    wav.extend_from_slice(&48_000_u32.to_le_bytes());
    wav.extend_from_slice(&96_000_u32.to_le_bytes());
    wav.extend_from_slice(b"\x02\0\x10\0data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for sample in 0..96_000 {
        let value: i16 = if sample % 100 < 50 { 2000 } else { -2000 };
        wav.extend_from_slice(&value.to_le_bytes());
    }
    let audio = dir.path().join("tone.wav");
    std::fs::write(&audio, wav).unwrap();
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
    session.request("initialize", json!({"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"workflow","version":"1"}}));
    let initial = session.json_tool("get_project_state", json!({}))["project"].clone();
    session.json_tool(
        "set_canvas",
        json!({"width":160,"height":90,"max_time_ms":2000}),
    );
    session.json_tool("add_media", json!({"path": media}));
    let imported = session.json_tool("get_project_state", json!({}));
    let clips: Vec<_> = imported["project"]["tracks"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|t| t["clips"].as_object().unwrap().values())
        .collect();
    assert!(
        clips.iter().any(|c| c["kind"].get("Audio").is_some()),
        "MP4 companion audio missing: {imported}"
    );
    let companion_export = dir.path().join("companion.mp4");
    session.json_tool("export_video", json!({"path":companion_export}));
    let decoder = animathio_media::FilmCraftBackend::new()
        .open_audio(&companion_export)
        .unwrap();
    assert!(
        decoder
            .samples(animathio_core::Time(0), 48000)
            .unwrap()
            .iter()
            .any(|sample| sample.abs() > 0.0001)
    );
    let sound = session.json_tool("add_media", json!({"path": audio}));
    let sound_id = sound["clip_id"].as_u64().unwrap().to_string();
    session.json_tool(
        "add_keyframe",
        json!({"id":sound_id,"target":"gain","time_ms":0,"value":1}),
    );
    session.json_tool(
        "add_keyframe",
        json!({"id":sound_id,"target":"gain","time_ms":2000,"value":0}),
    );
    session.json_tool("cut_at", json!({"times_ms":[1000]}));
    session.json_tool(
        "add_transition",
        json!({"track":"1","at_time_ms":1000,"duration_ms":200,"kind":"cross_dissolve"}),
    );
    session.json_tool("remove_range", json!({"from_ms":1500,"to_ms":1750}));
    session.json_tool(
        "add_title",
        json!({"text":"Real edit","start_ms":0,"duration_ms":1750}),
    );
    let edited = session.json_tool("get_project_state", json!({}))["project"].clone();
    // Every edit round-trips through the command history, including compound edits.
    let mut undo_count = 0;
    loop {
        let result = session.json_tool("undo", json!({}));
        if result["undone"] == false {
            break;
        }
        undo_count += 1;
        assert!(undo_count < 32, "history failed to reach its initial state");
    }
    assert_eq!(
        session.json_tool("get_project_state", json!({}))["project"],
        initial
    );
    for _ in 0..undo_count {
        session.json_tool("redo", json!({}));
    }
    assert_eq!(
        session.json_tool("get_project_state", json!({}))["project"],
        edited
    );
    let archive = dir.path().join("edit.amio");
    session.tool("save_project", json!({"path":archive}));
    let first = dir.path().join("first.mp4");
    session.tool("export_video", json!({"path":first}));
    let image_path = dir.path().join("portrait.png");
    image::RgbaImage::from_pixel(9, 16, image::Rgba([20, 80, 180, 255]))
        .save(&image_path)
        .unwrap();
    session.json_tool(
        "set_canvas",
        json!({"width":90,"height":160,"max_time_ms":200}),
    );
    session.json_tool("add_media", json!({"path":image_path}));
    let portrait = dir.path().join("portrait.mp4");
    session.tool("export_video", json!({"path":portrait}));
    let portrait_info =
        animathio_media::MediaBackend::probe(&animathio_media::FilmCraftBackend::new(), &portrait)
            .unwrap()
            .video
            .unwrap();
    assert_eq!((portrait_info.width, portrait_info.height), (90, 160));
    drop(session);
    assert!(child.wait().unwrap().success());
    let info = animathio_media::FilmCraftBackend::new()
        .probe(&first)
        .unwrap();
    let video = info.video.unwrap();
    assert_eq!((video.width, video.height), (160, 90));
    assert_eq!(
        video.frame_rate,
        animathio_core::FrameRate::new(30, 1).unwrap()
    );
    assert_eq!(info.audio.len(), 1);
    assert!(
        (info.duration.0 - animathio_core::TICKS_PER_SECOND * 7 / 4).abs()
            <= animathio_core::TICKS_PER_SECOND / 30
    );
    let reopened = animathio_format::read_amio(
        std::fs::File::open(&archive).unwrap(),
        animathio_format::Limits::default(),
    )
    .unwrap();
    assert_eq!(serde_json::to_value(reopened.project).unwrap(), edited);
    let second = dir.path().join("second.mp4");
    let result = Command::new(env!("CARGO_BIN_EXE_animathio-cli"))
        .arg("export")
        .arg(&archive)
        .arg("-o")
        .arg(&second)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        std::fs::read(first).unwrap(),
        std::fs::read(second).unwrap()
    );
}
