#![no_main]

use animathio_media::{FilmCraftBackend, MediaBackend};
use libfuzzer_sys::fuzz_target;
use std::sync::{Arc, Once};

/// libfuzzer-sys installs a panic hook that aborts before unwinding, so any panic —
/// including ones the audio stack deliberately catches (`guard_codec` turns hostile-input
/// panics inside third-party demuxers into ordinary errors) — kills the fuzzer. Restore
/// production panic semantics for this target: guarded panics unwind into `Err`, and
/// genuinely uncaught panics still abort the process and surface as crashes.
static RESET_PANIC_HOOK: Once = Once::new();

fuzz_target!(|data: &[u8]| {
    RESET_PANIC_HOOK.call_once(|| {
        std::panic::set_hook(Box::new(|info| {
            eprintln!("panic (expected to be caught by media guards): {info}");
        }));
    });
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
