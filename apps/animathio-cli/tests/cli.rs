//! Exercise the installed command surface and the shared preview/export render path.
use animathio_format::{Limits, read_amio, write_json};
use std::{
    fs::{self, File},
    path::Path,
    process::{Command, Output},
};
use tempfile::tempdir;
fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_animathio-cli"))
        .args(args)
        .output()
        .unwrap()
}
fn ok(args: &[&str]) {
    let output = cli(args);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn path(path: &Path) -> &str {
    path.to_str().unwrap()
}
#[test]
fn sample_archive_validates_and_json_and_frame_time_inputs_match() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("demo.amio");
    ok(&["new-sample", "-o", path(&archive)]);
    ok(&["validate", path(&archive)]);
    let bundle = read_amio(File::open(&archive).unwrap(), Limits::default()).unwrap();
    assert_eq!(bundle.project.tracks().len(), 6);
    assert!(
        bundle
            .project
            .tracks()
            .iter()
            .flat_map(|t| t.clips().values())
            .all(|c| !c.animations.is_empty())
    );
    let json = dir.path().join("project.json");
    write_json(File::create(&json).unwrap(), &bundle.project).unwrap();
    fs::create_dir(dir.path().join("assets")).unwrap();
    for (id, bytes) in &bundle.assets {
        fs::write(dir.path().join("assets").join(id.as_str()), bytes).unwrap();
    }
    ok(&["validate", path(&json)]);
    let a = dir.path().join("frame.png");
    let b = dir.path().join("time.png");
    ok(&[
        "render",
        path(&archive),
        "--frame",
        "30",
        "--width",
        "160",
        "--height",
        "90",
        "-o",
        path(&a),
    ]);
    ok(&[
        "render",
        path(&json),
        "--time",
        "1.0",
        "--width",
        "160",
        "--height",
        "90",
        "-o",
        path(&b),
    ]);
    assert_eq!(fs::read(&a).unwrap(), fs::read(&b).unwrap());
    let mut ctx = animathio_render::RenderCtx::default();
    for (id, bytes) in bundle.assets {
        ctx.resources.insert(id, bytes).unwrap();
    }
    let frame = animathio_render::render(
        &animathio_core::evaluate(
            &bundle.project,
            animathio_core::Time(animathio_core::TICKS_PER_SECOND),
        ),
        &mut ctx,
        (160, 90),
    )
    .unwrap();
    let mut expected = Vec::new();
    frame.write_png(&mut expected).unwrap();
    assert_eq!(fs::read(&a).unwrap(), expected);
}
#[test]
fn sequence_steps_the_same_path_and_uses_exclusive_end() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("demo.amio");
    ok(&["new-sample", "-o", path(&archive)]);
    let sequence = dir.path().join("sequence");
    ok(&[
        "render-seq",
        path(&archive),
        "--from",
        "29",
        "--to",
        "32",
        "--width",
        "64",
        "--height",
        "36",
        "-o",
        path(&sequence),
    ]);
    assert_eq!(fs::read_dir(&sequence).unwrap().count(), 3);
    let single = dir.path().join("single.png");
    ok(&[
        "render",
        path(&archive),
        "--frame",
        "30",
        "--width",
        "64",
        "--height",
        "36",
        "-o",
        path(&single),
    ]);
    assert_eq!(
        fs::read(&single).unwrap(),
        fs::read(sequence.join("frame_000030.png")).unwrap()
    );
    assert_ne!(
        fs::read(sequence.join("frame_000029.png")).unwrap(),
        fs::read(sequence.join("frame_000031.png")).unwrap()
    );
}
#[test]
fn invalid_arguments_and_projects_fail_without_creating_output() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("demo.amio");
    ok(&["new-sample", "-o", path(&archive)]);
    let output = dir.path().join("output");
    for args in [
        vec!["render", path(&archive), "-o", path(&output)],
        vec![
            "render",
            path(&archive),
            "--frame",
            "1",
            "--time",
            "1",
            "-o",
            path(&output),
        ],
        vec![
            "render",
            path(&archive),
            "--time",
            "-1",
            "-o",
            path(&output),
        ],
        vec![
            "render",
            path(&archive),
            "--frame",
            "0",
            "--width",
            "0",
            "-o",
            path(&output),
        ],
        vec![
            "render-seq",
            path(&archive),
            "--from",
            "2",
            "--to",
            "1",
            "-o",
            path(&output),
        ],
        vec![
            "render-seq",
            path(&archive),
            "--to",
            "2",
            "--height",
            "0",
            "-o",
            path(&output),
        ],
    ] {
        assert!(!cli(&args).status.success(), "accepted {args:?}");
        assert!(!output.exists());
    }
    let invalid = dir.path().join("bad.json");
    fs::write(&invalid, b"{}").unwrap();
    assert!(!cli(&["validate", path(&invalid)]).status.success());
    let bundle = read_amio(File::open(&archive).unwrap(), Limits::default()).unwrap();
    write_json(File::create(&invalid).unwrap(), &bundle.project).unwrap();
    fs::create_dir(dir.path().join("assets")).unwrap();
    for id in bundle.assets.keys() {
        fs::write(dir.path().join("assets").join(id.as_str()), b"tampered").unwrap();
    }
    assert!(!cli(&["validate", path(&invalid)]).status.success());
}

#[test]
fn import_v1_writes_a_valid_archive() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("old.animathio");
    let output = dir.path().join("out.amio");
    // Uncompressed v1 JSON (the importer also accepts the pre-deflate format)
    // with a single text element, which needs no embedded media.
    fs::write(
        &input,
        br##"{
            "backgroundColor": "#111111",
            "editorElements": [{
                "id": "text-1",
                "type": "text",
                "name": "Text 1",
                "placement": { "x": 10, "y": 20, "width": 200, "height": 40, "rotation": 0, "scaleX": 1, "scaleY": 1 },
                "timeFrame": { "start": 0, "end": 2000 },
                "properties": { "text": "hello", "fontSize": 24, "fontWeight": 400, "color": "#ffffff" }
            }],
            "maxTime": 3000,
            "animations": [],
            "fps": 30,
            "canvas_width": 800,
            "canvas_height": 600,
            "mediaFiles": []
        }"##,
    )
    .unwrap();
    ok(&["import-v1", path(&input), "-o", path(&output)]);
    ok(&["validate", path(&output)]);
    let bundle = read_amio(File::open(&output).unwrap(), Limits::default()).unwrap();
    assert_eq!(bundle.project.tracks().len(), 1);
    assert_eq!(bundle.project.width(), 800);
    assert_eq!(bundle.project.height(), 600);
    assert_eq!(bundle.project.background(), [17, 17, 17, 255]);
}
