//! Reframing writes a new archive and protects existing output.
use animathio_format::{Limits, read_amio};
use std::{fs, process::Command};
use tempfile::tempdir;
#[test]
fn cli_reframe_preserves_input_and_requires_overwrite() {
    let dir = tempdir().unwrap();
    let media = dir.path().join("sample.mp4");
    let source = dir.path().join("landscape.amio");
    let output = dir.path().join("vertical.amio");
    fs::write(
        &media,
        include_bytes!("../../../crates/media/tests/fixtures/sample.mp4"),
    )
    .unwrap();
    let cli = env!("CARGO_BIN_EXE_animathio-cli");
    assert!(
        Command::new(cli)
            .arg("import-media")
            .arg(&media)
            .arg("-o")
            .arg(&source)
            .output()
            .unwrap()
            .status
            .success()
    );
    let original = fs::read(&source).unwrap();
    let run = || {
        let mut c = Command::new(cli);
        c.arg("reframe").arg(&source).arg("-o").arg(&output);
        c
    };
    let result = run()
        .args(["--aspect", "9:16", "--smoothness", "0.7"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(fs::read(&source).unwrap(), original);
    let bundle = read_amio(
        std::io::Cursor::new(fs::read(&output).unwrap()),
        Limits::default(),
    )
    .unwrap();
    assert_eq!(
        (bundle.project.width(), bundle.project.height()),
        (1080, 1920)
    );
    assert!(!run().output().unwrap().status.success());
    assert!(
        run()
            .args(["--overwrite", "--aspect", "4:5"])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(
        !run()
            .args(["--overwrite", "--smoothness", "2"])
            .output()
            .unwrap()
            .status
            .success()
    );
}
