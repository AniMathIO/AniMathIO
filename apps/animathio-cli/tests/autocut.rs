//! The CLI requires an explicit mode and preserves source archives during review.
use animathio_core::{
    Asset, AssetId, Clip, ClipId, ClipKind, Command, Project, TICKS_PER_SECOND, Time, TimeRange,
    Track, TrackId,
};
use animathio_format::{Limits, read_amio, save_amio};
use std::{collections::BTreeMap, fs, process::Command as Process};
use tempfile::tempdir;

fn source() -> (Project, BTreeMap<AssetId, Vec<u8>>) {
    let mut wav = b"RIFF".to_vec();
    wav.extend(32_036_u32.to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16_u32.to_le_bytes());
    wav.extend(1_u16.to_le_bytes());
    wav.extend(1_u16.to_le_bytes());
    wav.extend(16_000_u32.to_le_bytes());
    wav.extend(32_000_u32.to_le_bytes());
    wav.extend(2_u16.to_le_bytes());
    wav.extend(16_u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend(32_000_u32.to_le_bytes());
    wav.extend(vec![0; 32_000]);
    let id = AssetId::of(&wav);
    let mut project = Project::default();
    Command::Batch {
        commands: vec![
            Command::AddAsset {
                id: id.clone(),
                asset: Asset {
                    name: "silence.wav".into(),
                    media_type: "audio/wav".into(),
                },
            },
            Command::AddTrack {
                track: Track::new(TrackId(1), "Voice"),
                index: 0,
            },
            Command::AddClip {
                track: TrackId(1),
                clip: Clip::new(
                    ClipId(1),
                    ClipKind::Audio(id.clone()),
                    TimeRange::new(Time(0), Time(TICKS_PER_SECOND)).unwrap(),
                ),
            },
        ],
    }
    .apply(&mut project)
    .unwrap();
    (project, BTreeMap::from([(id, wav)]))
}
#[test]
fn dry_run_apply_negative_threshold_and_output_protection() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("input.amio");
    let output = dir.path().join("edited.amio");
    let (project, assets) = source();
    save_amio(&input, &project, &assets).unwrap();
    let original = fs::read(&input).unwrap();
    let cli = || {
        let mut c = Process::new(env!("CARGO_BIN_EXE_animathio-cli"));
        c.arg("autocut").arg(&input);
        c
    };
    assert!(!cli().output().unwrap().status.success());
    let dry = cli()
        .args(["--silence-db", "-40", "--min-silence", "0.5", "--dry-run"])
        .output()
        .unwrap();
    assert!(
        dry.status.success(),
        "{}",
        String::from_utf8_lossy(&dry.stderr)
    );
    let proposal: serde_json::Value = serde_json::from_slice(&dry.stdout).unwrap();
    assert_eq!(proposal["cuts"].as_array().unwrap().len(), 1);
    assert_eq!(fs::read(&input).unwrap(), original);
    let apply = cli().args(["--apply", "-o"]).arg(&output).output().unwrap();
    assert!(
        apply.status.success(),
        "{}",
        String::from_utf8_lossy(&apply.stderr)
    );
    let edited = read_amio(fs::File::open(&output).unwrap(), Limits::default()).unwrap();
    assert_ne!(edited.project, project);
    let output_bytes = fs::read(&output).unwrap();
    assert!(
        !cli()
            .args(["--apply", "-o"])
            .arg(&output)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(fs::read(&output).unwrap(), output_bytes);
    assert_eq!(fs::read(&input).unwrap(), original);
    let in_place = cli().arg("--apply").output().unwrap();
    assert!(
        in_place.status.success(),
        "{}",
        String::from_utf8_lossy(&in_place.stderr)
    );
    let input_edited = read_amio(fs::File::open(&input).unwrap(), Limits::default()).unwrap();
    assert_eq!(input_edited.project, edited.project);
    if !animathio_transcribe::engine_available() {
        let fillers = cli().args(["--fillers", "--dry-run"]).output().unwrap();
        assert!(!fillers.status.success());
        assert!(String::from_utf8_lossy(&fillers.stderr).contains("no speech recognition engine"));
    }
}

#[test]
fn mcp_review_toggles_apply_and_undo() {
    use serde_json::{Value, json};
    use std::io::{BufRead, BufReader, Write};
    use std::process::Stdio;
    let dir = tempdir().unwrap();
    let input = dir.path().join("input.amio");
    let (project, assets) = source();
    save_amio(&input, &project, &assets).unwrap();
    let mut child = Process::new(env!("CARGO_BIN_EXE_animathio-cli"))
        .args(["mcp", "--project"])
        .arg(&input)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut id = 0;
    let mut request = |method: &str, params: Value| {
        id += 1;
        writeln!(
            stdin,
            "{}",
            json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
        )
        .unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        assert!(stdout.read_line(&mut line).unwrap() > 0);
        serde_json::from_str::<Value>(&line).unwrap()
    };
    let init = request(
        "initialize",
        json!({"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"autocut-test","version":"1"}}),
    );
    assert!(init.get("error").is_none(), "{init}");
    let mut tool = |name: &str, args: Value| {
        let response = request("tools/call", json!({"name":name,"arguments":args}));
        assert!(response.get("error").is_none(), "{response}");
        assert_ne!(response["result"]["isError"], true, "{response}");
        serde_json::from_str::<Value>(response["result"]["content"][0]["text"].as_str().unwrap())
            .unwrap()
    };
    let original = tool("get_project_state", json!({}))["project"].clone();
    let mut proposal = tool("propose_autocuts", json!({"silence_db":-40}));
    assert_eq!(proposal["cuts"].as_array().unwrap().len(), 1);
    assert_eq!(tool("get_project_state", json!({}))["project"], original);
    proposal["cuts"][0]["enabled"] = json!(false);
    assert_eq!(
        tool("apply_autocuts", json!({"proposal":proposal}))["total_removed"],
        0
    );
    proposal["cuts"][0]["enabled"] = json!(true);
    assert!(
        tool("apply_autocuts", json!({"proposal":proposal}))["total_removed"]
            .as_i64()
            .unwrap()
            > 0
    );
    assert_eq!(tool("undo", json!({}))["undone"], true);
    assert_eq!(tool("get_project_state", json!({}))["project"], original);
    assert_eq!(tool("undo", json!({}))["undone"], false);
    child.kill().unwrap();
    child.wait().unwrap();
}
