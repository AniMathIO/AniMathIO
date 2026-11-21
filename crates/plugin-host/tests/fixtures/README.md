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
- `animations-1.0.0.amplugin`: the frozen 1.1 contract (`animation-preset-plugin` world, ABI
  `animathio:plugin@1.1.0`). Plugin and WIT from #230 merge
  `9524f4d17` (`plugins/animations` 1.0.0).
- `latex-1.3.0.amplugin`: the frozen 1.2 contract (`colored-source-plugin` world, ABI
  `animathio:plugin@1.2.0`, per-path inline colour). Plugin and WIT from `origin/main` at
  `4280d70a6148bd93cab10accd1ddfa209f5897b9`, the last commit before ABI 1.3
  (`plugins/latex` 1.3.0).

The last two (1.1 and 1.2) were produced like this, from a detached `git worktree` of the
named revision so the plugin and its WIT are exactly that revision's:

```bash
git worktree add --detach /tmp/abi12 4280d70a6148bd93cab10accd1ddfa209f5897b9
cd /tmp/abi12 && cargo build --manifest-path plugins/latex/Cargo.toml \
  --target wasm32-wasip2 --target-dir /tmp/wasm-abi12 --release --locked
# zip plugin.toml + plugin.wasm (target/wasm32-wasip2/release/latex.wasm) as .amplugin
```

(`9524f4d17` and `plugins/animations` for the 1.1 fixture). Rust toolchain: stable 1.99.0.
`compatibility.rs` loads both on the current 1.3 host, checks that the 1.1 presets are exported
and that the 1.2 binary renders its inline `\textcolor` paths with distinct colours.

The `effects-basic` and `audio-basic` 0.2.x fixtures were compiled with `cargo build --target wasm32-wasip2 --release
--locked` against those revisions' standalone Cargo.lock and zipped with their
original plugin.toml. The host's matching historical contract is preserved in
`crates/plugin-api/wit-legacy/plugin.wit`; 0.2.1 and 0.2.2 added interfaces/worlds
without changing any 0.2.0 data layouts. Rust toolchain: stable 1.99.0.

`compatibility.rs` exercises schema/geometry, exact pixels, corrected audio kind,
state carry, and rejection of a falsified ABI declaration. With `test-support`,
`abi_conformance.rs` also compares all legacy filters to the 1.0 binaries.
