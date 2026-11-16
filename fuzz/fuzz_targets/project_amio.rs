#![no_main]

use animathio_format::{Limits, read_amio};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = read_amio(std::io::Cursor::new(data), Limits::default());
});
