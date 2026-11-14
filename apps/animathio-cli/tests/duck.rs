//! `duck` reviews without touching the input, applies atomically and is undoable over MCP.
#![allow(clippy::cast_possible_truncation)]
use animathio_core::{
    Asset, AssetId, Clip, ClipId, ClipKind, Command, Project, TICKS_PER_SECOND, Time, TimeRange,
    Track, TrackId,
};
use animathio_format::{Limits, read_amio, save_amio};
use std::{collections::BTreeMap, fs, process::Command as Process};
use tempfile::tempdir;

/// 16 kHz mono WAV: `(amplitude, milliseconds)` parts of alternating-sign samples.
fn wav(parts: &[(i16, usize)]) -> Vec<u8> {
    let data: Vec<u8> = parts
        .iter()
        .flat_map(|(amp, ms)| {
            (0..ms * 16).flat_map(move |i| (if i % 2 == 0 { *amp } else { -*amp }).to_le_bytes())
        })
        .collect();
    let len = u32::try_from(data.len()).unwrap();
    let mut wav = b"RIFF".to_vec();
    wav.extend((36 + len).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16_u32.to_le_bytes());
    wav.extend(1_u16.to_le_bytes());
    wav.extend(1_u16.to_le_bytes());
    wav.extend(16_000_u32.to_le_bytes());
    wav.extend(32_000_u32.to_le_bytes());
    wav.extend(2_u16.to_le_bytes());
    wav.extend(16_u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend(len.to_le_bytes());
    wav.extend(data);
    wav
}

fn source() -> (Project, BTreeMap<AssetId, Vec<u8>>) {
    let voice = wav(&[(16_000, 1000), (0, 1000), (16_000, 1000)]);
    let music = wav(&[(8_000, 3000)]);
    let (vid, mid) = (AssetId::of(&voice), AssetId::of(&music));
    let seconds = TimeRange::new(Time(0), Time(3 * TICKS_PER_SECOND)).unwrap();
    let mut project = Project::default();
    let mut commands = Vec::new();
    for (id, name) in [(&vid, "voice.wav"), (&mid, "music.wav")] {
        commands.push(Command::AddAsset {
            id: id.clone(),
            asset: Asset {
                name: name.into(),
                media_type: "audio/wav".into(),
            },
        });
    }
    for (i, (name, id)) in [("Voice", &vid), ("Music", &mid)].into_iter().enumerate() {
        commands.push(Command::AddTrack {
            track: Track::new(TrackId(i as u64 + 1), name),
            index: i,
        });
        commands.push(Command::AddClip {
            track: TrackId(i as u64 + 1),
            clip: Clip::new(ClipId(i as u64 + 1), ClipKind::Audio(id.clone()), seconds),
        });
    }
    Command::Batch { commands }.apply(&mut project).unwrap();
    (project, BTreeMap::from([(vid, voice), (mid, music)]))
}

fn gain_keys(project: &Project) -> usize {
    project
        .tracks()
        .iter()
        .flat_map(|t| t.clips().values())
        .filter(|c| c.id == ClipId(2))
        .flat_map(|c| &c.animations)
        .map(|a| a.keys.keys().len())
        .sum()
}

#[test]
fn dry_run_apply_output_protection_and_required_music_track() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("input.amio");
    let output = dir.path().join("ducked.amio");
    let (project, assets) = source();
    save_amio(&input, &project, &assets).unwrap();
    let original = fs::read(&input).unwrap();
    let cli = || {
        let mut c = Process::new(env!("CARGO_BIN_EXE_animathio-cli"));
        c.arg("duck").arg(&input);
        c
    };
    // A music track and an explicit mode are required.
    assert!(
        !cli()
            .args(["--voice-track", "1", "--dry-run"])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(
        !cli()
            .args(["--voice-track", "1", "--music-track", "2"])
            .output()
            .unwrap()
            .status
            .success()
    );
    let dry = cli()
        .args(["--voice-track", "1", "--music-track", "2", "--dry-run"])
        .output()
        .unwrap();
    assert!(
        dry.status.success(),
        "{}",
        String::from_utf8_lossy(&dry.stderr)
    );
    let proposal: serde_json::Value = serde_json::from_slice(&dry.stdout).unwrap();
    assert_eq!(proposal["voice"].as_array().unwrap().len(), 2);
    assert_eq!(proposal["clips"][0]["clip"], 2);
    assert_eq!(fs::read(&input).unwrap(), original);
    let args = ["--voice-track", "1", "--music-track", "2", "--apply", "-o"];
    let apply = cli().args(args).arg(&output).output().unwrap();
    assert!(
        apply.status.success(),
        "{}",
        String::from_utf8_lossy(&apply.stderr)
    );
    let edited = read_amio(fs::File::open(&output).unwrap(), Limits::default()).unwrap();
    assert!(gain_keys(&edited.project) > 2);
    assert_eq!(fs::read(&input).unwrap(), original);
    let bytes = fs::read(&output).unwrap();
    assert!(
        !cli()
            .args(args)
            .arg(&output)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(fs::read(&output).unwrap(), bytes);
}

#[test]
fn mcp_auto_duck_dry_run_apply_and_undo() {
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
        json!({"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"duck-test","version":"1"}}),
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
    let proposal = tool(
        "auto_duck",
        json!({"voice_tracks":[1],"music_tracks":[2],"dry_run":true,"depth_db":15}),
    );
    assert_eq!(proposal["voice"].as_array().unwrap().len(), 2);
    assert_eq!(tool("get_project_state", json!({}))["project"], original);
    let applied = tool("auto_duck", json!({"voice_tracks":[1],"music_tracks":[2]}));
    assert_eq!(applied["clips"], 1);
    assert_ne!(tool("get_project_state", json!({}))["project"], original);
    assert_eq!(tool("undo", json!({}))["undone"], true);
    assert_eq!(tool("get_project_state", json!({}))["project"], original);
    assert_eq!(tool("undo", json!({}))["undone"], false);
    child.kill().unwrap();
    child.wait().unwrap();
}
