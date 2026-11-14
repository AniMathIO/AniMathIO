//! Model download logic without a network: pinned hashes, resume and atomic placement are
//! exercised by feeding files straight into `verify_file` / `download_to`.

use animathio_transcribe::{
    ModelSize,
    models::{MODELS, ModelInfo, display_size, download_to, models_dir, verify_file},
};
use sha2::{Digest, Sha256};
use std::sync::atomic::AtomicBool;

fn info_for(bytes: &[u8]) -> ModelInfo {
    use std::fmt::Write as _;
    let hash = Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        });
    ModelInfo {
        name: "test",
        file: "ggml-test.bin",
        // Unroutable: a test that reaches the network would fail loudly.
        url: "http://127.0.0.1:9/ggml-test.bin",
        sha256: Box::leak(hash.into_boxed_str()),
        bytes: bytes.len() as u64,
    }
}

#[test]
fn model_table_is_consistent() {
    for size in ModelSize::ALL {
        let info = size.info();
        assert_eq!(ModelSize::parse(info.name).unwrap(), size);
        assert_eq!(info.sha256.len(), 64);
        assert!(
            info.sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        );
        assert!(info.url.starts_with("https://") && info.url.ends_with(info.file));
    }
    assert_eq!(MODELS.len(), 3);
    assert_eq!(ModelSize::default(), ModelSize::Base);
    assert!(ModelSize::parse("huge").is_err());
    assert_eq!(display_size(147_951_465), "148 MB");
    assert_eq!(display_size(2_100_000_000), "2.1 GB");
    assert!(models_dir().ends_with("whisper") || std::env::var_os("ANIMATHIO_MODEL_DIR").is_some());
}

#[test]
fn complete_verified_file_is_kept_without_touching_the_network() {
    let dir = tempfile::tempdir().unwrap();
    let data = vec![7u8; 4096];
    let info = info_for(&data);
    std::fs::write(dir.path().join(info.file), &data).unwrap();
    let mut seen = Vec::new();
    let path = download_to(&info, dir.path(), &AtomicBool::new(false), &mut |p| {
        seen.push(p);
    })
    .unwrap();
    assert_eq!(path, dir.path().join(info.file));
    assert_eq!(seen.last().unwrap().downloaded, 4096);
}

#[test]
fn full_part_file_is_verified_and_atomically_renamed() {
    let dir = tempfile::tempdir().unwrap();
    let data: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8).collect();
    let info = info_for(&data);
    // A previous run downloaded everything but crashed before the rename.
    std::fs::write(dir.path().join("ggml-test.bin.part"), &data).unwrap();
    let path = download_to(&info, dir.path(), &AtomicBool::new(false), &mut |_| {}).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), data);
    assert!(!dir.path().join("ggml-test.bin.part").exists());
}

#[test]
fn corrupt_part_file_is_rejected_and_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let data = vec![1u8; 2048];
    let info = info_for(&data);
    let mut bad = data.clone();
    bad[100] ^= 0xff;
    std::fs::write(dir.path().join("ggml-test.bin.part"), &bad).unwrap();
    let err = download_to(&info, dir.path(), &AtomicBool::new(false), &mut |_| {}).unwrap_err();
    assert!(err.to_string().contains("checksum"), "{err}");
    assert!(!dir.path().join("ggml-test.bin.part").exists());
    assert!(!dir.path().join(info.file).exists());
}

#[test]
fn verify_file_rejects_wrong_size() {
    let dir = tempfile::tempdir().unwrap();
    let data = vec![3u8; 100];
    let info = info_for(&data);
    let path = dir.path().join("x.bin");
    std::fs::write(&path, &data[..50]).unwrap();
    assert!(verify_file(&path, &info).is_err());
}

#[test]
fn missing_model_error_explains_how_to_get_it() {
    let text = animathio_transcribe::Error::ModelMissing {
        model: "tiny".into(),
        file: "ggml-tiny.bin".into(),
        size_mb: 78,
        dir: "/models".into(),
    }
    .to_string();
    assert!(
        text.contains("models --download tiny") && text.contains("78 MB"),
        "{text}"
    );
}
