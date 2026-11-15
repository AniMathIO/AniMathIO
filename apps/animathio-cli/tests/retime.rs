//! File operations exercise rational/ramp arguments and preserve the input file.
use animathio_core::*;
use std::{collections::BTreeMap, process::Command as Process};
#[test]
fn speed_and_frame_hold_use_public_commands_and_refuse_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.amio");
    let speed = dir.path().join("speed.amio");
    let held = dir.path().join("held.amio");
    let mut p = Project::default();
    Command::AddTrack {
        track: Track::new(TrackId(1), "test"),
        index: 0,
    }
    .apply(&mut p)
    .unwrap();
    Command::AddClip {
        track: TrackId(1),
        clip: Clip::new(
            ClipId(1),
            ClipKind::Error("test".into()),
            TimeRange::new(Time(0), Time(TICKS_PER_SECOND)).unwrap(),
        ),
    }
    .apply(&mut p)
    .unwrap();
    animathio_format::save_amio(&input, &p, &BTreeMap::new()).unwrap();
    let original = std::fs::read(&input).unwrap();
    let run = |action: &str, source: &std::path::Path, output: &std::path::Path, args: &[&str]| {
        Process::new(env!("CARGO_BIN_EXE_animathio-cli"))
            .arg(action)
            .arg(source)
            .args(["--clip", "1", "-o"])
            .arg(output)
            .args(args)
            .output()
            .unwrap()
    };
    let result = run(
        "set-clip-speed",
        &input,
        &speed,
        &["--speed", "2", "--reverse"],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let load = |path: &std::path::Path| {
        animathio_format::read_amio(
            std::fs::File::open(path).unwrap(),
            animathio_format::Limits::default(),
        )
        .unwrap()
        .project
    };
    let edited = load(&speed);
    let clip = &edited.tracks()[0].clips()[&ClipId(1)];
    assert!(clip.time_map.reverse);
    assert_eq!(clip.range.end, Time(TICKS_PER_SECOND / 2));
    assert!(
        !run("set-clip-speed", &input, &speed, &["--speed", "1"])
            .status
            .success()
    );
    assert_eq!(load(&speed), edited);
    let result = run(
        "freeze-frame",
        &speed,
        &held,
        &["--at", "0.25", "--frames", "15"],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let held = load(&held);
    assert_eq!(held.tracks()[0].clips().len(), 3);
    let hold = held.tracks()[0]
        .clips()
        .values()
        .find(|c| c.time_map.hold)
        .unwrap();
    assert_eq!(hold.range.end.0 - hold.range.start.0, TICKS_PER_SECOND / 2);
    assert_eq!(std::fs::read(&input).unwrap(), original);
}
