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

## Branch rules

- 2.0 PRs target **`v2`**. Never target `main` or `staging`: those serve v1 until the 2.0 swap.
- Don't merge `main`/`staging` into `v2`. The only planned merge is the one-time `git merge -s ours main` at the 2.0 swap (see #156, "Repository and branch plan").
- Use Conventional Commits. release-plz will derive versions and the changelog from them.

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
and engine resource pool; `animathio-cli` provides headless PNG rendering.
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
