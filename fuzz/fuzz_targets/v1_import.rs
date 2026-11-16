#![no_main]

use animathio_import_v1::import_v1;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = import_v1(data);
});
