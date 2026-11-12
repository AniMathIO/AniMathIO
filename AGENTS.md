# AGENTS.md

Guidance for coding agents working on the `v2` branch. `CLAUDE.md` imports this file.

## What this branch is

AniMathIO 2.0 is a from-scratch Rust rewrite of AniMathIO, an editor for Manim-like math animation videos. The full plan, architecture and roadmap are in [issue #156](https://github.com/AniMathIO/AniMathIO/issues/156). Read it before proposing structure. The short version:

- **The core is a simple video editor:** media clips, a multi-track timeline, transforms + keyframes, audio, preview and export. Nothing math-specific lives in the core.
- **Everything else is a plugin:** sandboxed WebAssembly components (wasmtime, Component Model, WIT). This includes text, LaTeX, graphs, Manim import, effects, the v1 importer and MCP.
- **Rendering is one pure function,** `render(&Project, Time) -> Frame`, shared by preview, export and tests.
- **Every mutation is a serializable, invertible `Command`.** Undo/redo, the control protocol, MCP and plugins all go through it.
- **Time is integer ticks + rational frame rates. Coordinates are resolution-independent.** No `f64` seconds and no absolute pixels in the model.
- **The core owns its domain types.** Third-party crates (FilmCraft codecs, vello, GPUI) sit behind adapters.

## Docs

Docs live in `docs/` and are kept accurate against the code — every command and
API they describe must exist. Update them alongside behaviour changes:

- `README.md` — what 2.0 is, status, build/run and import instructions
- `docs/architecture.md` — crate map, layering, time model, engine rules, data flow
- `docs/plugins.md` — plugin authoring guide (WIT world, host imports, schema, sandbox, example)
- `docs/cli.md` — every `animathio-cli` subcommand (regenerate from `--help`)
- `docs/mcp.md` — control protocol (JSON-RPC, socket, token) and the MCP tool surface
- `docs/migrating-from-v1.md` — what maps from 1.x and known gaps

The minimal plugin example under `plugins/examples/hello` must keep compiling for
`wasm32-wasip2`; it is referenced verbatim by `docs/plugins.md`.

## Branch rules

- 2.0 PRs target **`v2`**. Never target `main` or `staging`: those serve v1 until the 2.0 swap.
- Don't merge `main`/`staging` into `v2`. The only planned merge is the one-time `git merge -s ours main` at the 2.0 swap (see #156, "Repository and branch plan").
- Use Conventional Commits. release-plz will derive versions and the changelog from them.

## Automation

Automation is configured in `renovate.json`, `release-plz.toml` and `.github/workflows/{release-plz,package,security-audit}.yml`. It is scoped to the `v2` branch and stays in "release-PR only" mode until the 2.0 swap.

- **Renovate** (`renovate.json`) targets `baseBranchPatterns: ["v2"]` (Renovate's current name for what was formerly `baseBranches`). Patch/minor/security updates are grouped and auto-merged once CI passes (GitHub auto-merge); majors are never auto-merged and get a `needs-human` label. Lockfile maintenance runs weekly and vulnerability alerts are on. GitHub Actions are pinned by digest.
  - **Manual step:** the GitHub **Renovate** app must be enabled for the repo (an agent cannot enable it). Renovate reads config from the default branch, so it activates when `v2` becomes the default at the swap; change `baseBranchPatterns` to `["main"]` then. Auto-merge needs "Allow auto-merge" enabled in the repo settings and CI as a required status check.
- **Dependabot is v1-only.** `.github/dependabot.yml` lives on `main`/`staging`; do not add one on `v2` — Renovate covers it.
- **release-plz** (`release-plz.toml` + `.github/workflows/release-plz.yml`) opens a release PR (version bump + git-cliff changelog) on pushes to `v2`. Tagging, GitHub releases and crates.io publishing are disabled until the swap; the single tag is `v{{version}}`, starting at `2.0.0` (workspace version is already `2.0.0`, unreleased). Enablement steps are commented in both files.
- **Packaging** (`apps/animathio-cli/Cargo.toml` `[package.metadata.packager]` + `.github/workflows/package.yml`) builds AppImage + deb on Linux and NSIS on Windows, runs on `workflow_dispatch` and release PRs, smoke-tests each installer (`animathio-cli --version`) and adds build-provenance attestations. It targets `animathio-cli` until `apps/animathio` lands (see the TODO in the Cargo.toml and workflow).
- **Security** (`.github/workflows/security-audit.yml`) runs daily: `cargo deny check advisories` once `deny.toml` exists (added in the media PR), else `cargo audit`.

## Porting from v1

The v1 implementation (Electron/TypeScript) is still on `main` and is a useful reference. Read it without checking it out:

```bash
git show origin/main:renderer/utils/manim/translator.ts    # Manim subset translator (to port to the manim-import plugin)
git show origin/main:renderer/types.ts                     # v1 element/animation model (input to the import-v1 plugin)
git show origin/main:renderer/states/stores/ProjectStore.ts # v1 .animathio (de)serialization
git show origin/main:main/mcp/tool-schemas.ts              # v1 MCP tool surface (parity target)
```

Port behavior, not structure. The v1 store/component layout is exactly what 2.0 replaces.

## Commands

The workspace uses stable Rust, edition 2024. Standalone `spikes/` projects are excluded.
Run from the repository root (the resource limits protect other work on shared machines):

```bash
CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=line-tables-only nice -n 19 ionice -c3 cargo build --workspace --locked
CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=line-tables-only nice -n 19 ionice -c3 cargo fmt --all --check
CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=line-tables-only nice -n 19 ionice -c3 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=line-tables-only nice -n 19 ionice -c3 cargo test --workspace --locked
```

`core` owns the plain project model and command-only mutation API; `format` owns
versioned JSON and `.amio` containers. `audio` owns decoded PCM pools, immutable
resampled mixer snapshots, and cpal/rtrb playback; no audio resources live in
`Project`. `render` owns the deterministic Vello CPU backend, embedded font shaping
and engine resource pool; its optional `gpu` feature adds the Vello/wgpu
`GpuBackend` used only for editor preview (CPU stays the reference for tests and
export; `tests/gpu.rs` compares the two and skips without an adapter).
`animathio-cli` provides headless PNG rendering.
Text/shapes under `core::builtin` are temporary plugin stand-ins. Video uses a
labelled checkerboard until a media decoder is integrated. Do not add runtime
resources to `Project` or store evaluated animation state in the renderer.

Audio clips use numeric `gain` (linear, default 1) and `muted` (nonzero means muted,
default 0) parameters, including clip-local keyframes. Track gain is changed through
`Command::SetTrackGain`. `AudioPool::prepare(rate, channels)` does decoding/resampling
work outside playback; `Mixer::mix(project, range)` is shared by preview and export.
The device callback only consumes the ring and updates the latency-corrected master
clock; `Player::now()` is quantized to callbacks and freezes on underflow. Seek drops
the old stream/ring and joins its worker before starting a new session.

Linux builds need `libasound2-dev` (CI installs it). Normal tests need no device.
The ignored device test can be run explicitly; `cargo run -p animathio-audio
--example play` emits a 0.01-amplitude tone for at most 3 seconds and prints clock
drift. Apply the same resource limits to these cargo invocations. Symphonia 0.5
supports WAV/PCM, MP3, FLAC, AAC-LC/M4A and Vorbis; Opus returns an explicit
unsupported-codec error. Full-asset decoding and equal-layout/mono channel mapping
are prototype limits; streaming, surround layout matrices and waveform caches are deferred.

Plugin compiled-component disk cache entries are stored as `MAC(32) || serialized`,
where the MAC is a `blake3::keyed_hash` over the serialized bytes using a per-install
32-byte key at `<cache>/key`. On Unix the cache directory must be private
(`0700`, current-uid, non-symlink) or the disk cache is disabled and components are
recompiled. The control socket and token file live under `$XDG_RUNTIME_DIR/animathio`
(never a shared `/tmp`) and are verified private before use; token writes go through a
`0600` `O_NOFOLLOW` temp file plus atomic rename.

CLI smoke run and intentional golden regeneration:

```bash
CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=line-tables-only nice -n 19 ionice -c3 cargo run -p animathio-cli --locked -- new-sample -o /tmp/sample.amio
CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=line-tables-only nice -n 19 ionice -c3 cargo run -p animathio-cli --locked -- render /tmp/sample.amio --frame 30 -o /tmp/frame.png
UPDATE_GOLDEN=1 CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=line-tables-only nice -n 19 ionice -c3 cargo test -p animathio-render --test golden --locked
```

Commit reviewed PNGs from `crates/render/tests/golden` together with their fixtures.
Goldens permit a maximum absolute error of 2 per RGBA channel; regular workspace
CI runs them without regeneration on Linux and Windows.
After a final push on disk-constrained machines, clean build artifacts with:

```bash
CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=line-tables-only nice -n 19 ionice -c3 cargo clean
```
