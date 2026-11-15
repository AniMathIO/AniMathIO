# Fuzzing untrusted parsers

The quality-gate fuzzing for untrusted-input parsers lives in the standalone `fuzz/` cargo
workspace (its own `[workspace]`, excluded from the main workspace like `spikes/`) and uses
[cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) (LLVM libFuzzer) through each crate's
public load API.

## Fuzz targets

| Fuzz target | Parser under test | Public API in the crate |
| --- | --- | --- |
| `project_json` | Versioned project JSON, including schema migrations | `animathio-format`, `read_json_with_migration` |
| `project_amio` | Versioned project `.amio` container | `animathio-format`, `read_amio` |
| `v1_import` | v1 `.animathio` project import | `animathio-import-v1`, `import_v1` |
| `manim_source` | Manim Python scene parser | `animathio-import-manim`, `import_manim` |
| `media_probe` | Media probe/open | `animathio-media`, `FilmCraftBackend` |

## Running

The nightly toolchain is required (the libFuzzer runtime needs the nightly compiler):

```bash
rustup toolchain install nightly --profile minimal
```

From the repository root, with the shared-machine resource limits:

```bash
CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=line-tables-only nice -n 19 ionice -c3 \
  cargo +nightly fuzz run <target> -- -max_total_time=300
```

Seed corpora live in `fuzz/corpus/<target>/` and are copied from the repository fixtures
(`crates/format/tests/fixtures`, `crates/import-v1/tests/fixtures`,
`crates/render/tests/fixtures`, `crates/media/tests/fixtures`, `crates/audio/tests/fixtures`).
New interesting inputs are written there by the fuzzer; minimized crash reproducers are
committed under `fuzz/regressions/<target>/`.

## Regressions

When a fuzz run crashes, the minimized reproducer is committed under
`fuzz/regressions/<target>/` and the crash is listed in the PR body with the target, the input
(base64 when small), the panic message and the stack. Crashes are not fixed in the fuzzing PR;
they are triaged in follow-up issues.

## CI

`.github/workflows/fuzz.yml` runs every fuzz target for 60 seconds. It is
`workflow_dispatch` only, so the nightly + cargo-fuzz setup is exercised manually before it is
scheduled.
