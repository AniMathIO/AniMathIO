#![no_main]

use animathio_media::{FilmCraftBackend, MediaBackend};
use libfuzzer_sys::fuzz_target;
use std::sync::Arc;

fuzz_target!(|data: &[u8]| {
    let backend = FilmCraftBackend::new();
    let bytes: Arc<[u8]> = Arc::from(data.to_vec().into_boxed_slice());
    let _ = backend.probe_bytes("media", bytes.clone());
    let _ = backend.probe_bytes("media", bytes);
    let path = std::env::temp_dir().join(format!("animathio-fuzz-{}-media", std::process::id()));
    let _ = std::fs::write(&path, data);
    let path = path.as_path();
    let _ = backend.probe(path);
    let _ = backend.open_video(path);
    let _ = backend.open_audio(path);
});
