#![no_main]

use animathio_import_manim::import_manim;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let source = String::from_utf8_lossy(data);
    let _ = import_manim(&source);
});
