//! Exercise the installed command surface and the shared preview/export render path.
use animathio_format::{Limits, read_amio, write_json};
use std::{
    fs::{self, File},
    path::Path,
    process::{Command, Output},
};
use tempfile::tempdir;

/// A minimal valid 1x1 transparent PNG (67 bytes).
const MINIMAL_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_animathio-cli"))
        .args(args)
        .env(
            "ANIMATHIO_PLUGIN_DIR",
            animathio_plugin_host::test_support::plugin_dir(),
        )
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
        &animathio_plugin_host::test_support::resolve(
            &animathio_core::evaluate(
                &bundle.project,
                animathio_core::Time(animathio_core::TICKS_PER_SECOND),
            ),
            (160, 90),
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

#[test]
fn import_manim_writes_a_valid_archive_and_selects_scenes() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("scene.py");
    let output = dir.path().join("out.amio");
    fs::write(
        &input,
        r#"
from manim import *

class First(Scene):
    def construct(self):
        self.wait(1)

class Second(Scene):
    def construct(self):
        title = Text("hello", color=WHITE)
        self.play(Write(title), run_time=2)
        self.wait(1)
"#,
    )
    .unwrap();

    ok(&[
        "import-manim",
        path(&input),
        "--scene",
        "Second",
        "-o",
        path(&output),
    ]);
    ok(&["validate", path(&output)]);
    let bundle = read_amio(File::open(&output).unwrap(), Limits::default()).unwrap();
    assert_eq!(bundle.project.tracks().len(), 1);
    assert_eq!(bundle.project.width(), 1920);
    assert_eq!(bundle.project.height(), 1080);
    assert_eq!(bundle.project.background(), [0, 0, 0, 255]);
    assert_eq!(
        bundle.project.duration(),
        animathio_core::Time(animathio_core::TICKS_PER_SECOND * 3)
    );
}

#[test]
fn import_manim_reports_embedded_images() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("scene.py");
    let output = dir.path().join("out.amio");
    fs::write(dir.path().join("photo.png"), MINIMAL_PNG).unwrap();
    fs::write(
        &input,
        r#"
class S(Scene):
    def construct(self):
        img = ImageMobject("photo.png")
        self.add(img)
        self.wait(1)
"#,
    )
    .unwrap();

    let out = cli(&["import-manim", path(&input), "-o", path(&output)]);
    assert!(
        out.status.success(),
        "{}:
{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("info:"), "expected an info line: {stderr}");
    assert!(
        stderr.contains("photo.png"),
        "expected the embedded file name: {stderr}"
    );
}

#[test]
fn export_creates_missing_parents_and_never_overwrites_silently() {
    let dir = tempdir().unwrap();
    let archive = dir.path().join("demo.amio");
    ok(&["new-sample", "-o", path(&archive)]);
    let output = dir.path().join("deep/nested/clip.mp4");
    let base = ["export", path(&archive), "--to", "0.5"];

    let mut args = base.to_vec();
    args.extend(["-o", path(&output)]);
    ok(&args);
    assert!(output.is_file());
    // Only the finished MP4 is in the generated directory: no temp file survives.
    let entries: Vec<_> = fs::read_dir(output.parent().unwrap()).unwrap().collect();
    assert_eq!(entries.len(), 1);
    let original = fs::read(&output).unwrap();

    let rejected = cli(&args);
    assert!(!rejected.status.success());
    let stderr = String::from_utf8_lossy(&rejected.stderr);
    assert!(stderr.contains("already exists"), "{stderr}");
    assert_eq!(fs::read(&output).unwrap(), original);

    let mut args = base.to_vec();
    args.extend(["-o", path(&output), "--overwrite"]);
    ok(&args);
    assert!(output.is_file());
}
