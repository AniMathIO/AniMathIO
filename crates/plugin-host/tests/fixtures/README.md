# Frozen component fixtures

These are checked-in component binaries, never rebuilt by tests or
`scripts/build-plugins.sh`. A test that recompiles against current WIT cannot
prove backwards compatibility.

- `hello-0.2.0.amplugin`: original fixture from #203, retained byte for byte.
- `hello-1.0.0.amplugin`: the frozen 1.0 source-world contract, including the
  open kind tag, advisory metadata, diagnostics and the host limit query. Built
  from this PR's documented example; retain it when adding future 1.x APIs.
- `effects-basic-0.2.1.amplugin`: plugin and WIT from #203 merge
  `f85a104f03c0483781092bb8a46ad6890aa333b8`.
- `audio-basic-0.2.2.amplugin`: plugin and WIT from #211 merge
  `17c92ea17f7d4de933629ce1e6d20ecabf14bac3`.

The latter two were compiled with `cargo build --target wasm32-wasip2 --release
--locked` against those revisions' standalone Cargo.lock and zipped with their
original plugin.toml. The host's matching historical contract is preserved in
`crates/plugin-api/wit-legacy/plugin.wit`; 0.2.1 and 0.2.2 added interfaces/worlds
without changing any 0.2.0 data layouts. Rust toolchain: stable 1.99.0.

`compatibility.rs` exercises schema/geometry, exact pixels, corrected audio kind,
state carry, and rejection of a falsified ABI declaration. With `test-support`,
`abi_conformance.rs` also compares all legacy filters to the 1.0 binaries.
