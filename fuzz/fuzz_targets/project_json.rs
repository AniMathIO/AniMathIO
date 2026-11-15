#![no_main]

use animathio_format::{Error, read_json_with_migration};
use libfuzzer_sys::fuzz_target;
use serde_json::Value;

fuzz_target!(|data: &[u8]| {
    let mut cursor = std::io::Cursor::new(data);
    let result = read_json_with_migration(
        &mut cursor,
        |_: u32, _: Value| -> std::result::Result<Value, Error> {
            Err(Error::Invalid(
                "fuzz: no migration registered for this schema".to_owned(),
            ))
        },
    );
    let _ = result;
});
