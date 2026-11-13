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
- `docs/release-checklist.md` — manual and automatable steps left before the 2.0 swap

`scripts/check-docs.sh` fails when a document names a path, `animathio-cli` subcommand
or flag, MCP tool, control method, environment variable, cargo target, plugin id, core
`Command`, WIT item or Rust symbol that does not exist in the code, or when the docs omit
a tool, method, crate, plugin or editor module. CI runs it with `--require-binary` after
building `animathio-cli`; `docs/cli.md` embeds the real `--help` output between
`<!-- help:NAME -->` markers, regenerated with `scripts/check-docs.sh --fix-cli`. Run it
after any change to the CLI, MCP, control methods or crate layout.

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
- **release-plz** (`release-plz.toml` + `.github/workflows/release-plz.yml`) opens a release PR (version bump + git-cliff changelog) on pushes to `v2` **once a `v2.*` tag exists**; until the baseline `v2.0.0` tag is created at the swap it is a no-op (verified with a local `release-plz update`). Tagging, GitHub releases and crates.io publishing are disabled until the swap; the single tag is `v{{version}}`, starting at `2.0.0` (workspace version is already `2.0.0`, unreleased). Enablement and bootstrap steps are commented in both files and listed in `docs/release-checklist.md`.
  - **Changelog rules live in `cliff.toml`** (read by release-plz via `changelog_config`, runnable standalone): `feat`→Added, `fix`→Fixed, `perf`→Performance, `*(security)`→Security, breaking `type!:` kept; `chore`, `ci`, `build`, `docs`, `test`, `style`, `refactor`, clippy fixes and the `ci`/`dev`/`spikes`/`package`/`deps` scopes are hidden; v1 tags never count as releases. Preview with `git cliff --config cliff.toml --tag v2.0.0 origin/main..origin/v2` (install with `cargo install --locked git-cliff`).
- **Packaging** (`apps/animathio/Cargo.toml` `[package.metadata.packager]` + `.github/workflows/package.yml`) builds the installers for the canonical desktop package, which ships both `animathio` and `animathio-cli`: AppImage + deb on Linux, an NSIS installer on Windows, and an unsigned dmg on macOS. Each job builds the first-party plugin bundles with `scripts/build-plugins.sh` first (cargo-packager's `resources` target `plugins`), then runs `cargo packager --release --packages animathio`, installs/extracts the result, runs `animathio --version` and `animathio-cli --version` headlessly, counts the packaged `.amplugin` files, and attests build provenance. It runs on `workflow_dispatch`, on release PRs and on PRs touching packaging inputs.
  - Config paths are relative to `apps/animathio` (cargo-packager `cd`s there), so `out-dir` is `../../target/packages` and resources are `../../target/plugins`; `binaries` paths are relative to the workspace `target/release`.
  - Resources land in `plugins/` beside the executable on Windows, `usr/lib/animathio/plugins` in the deb/AppImage and `Contents/Resources/plugins` in the macOS app; `crates/plugin-host/src/discovery.rs` searches all of those relative to the running executable.
  - Artifacts: `animathio_<version>_<arch>.AppImage`, `animathio_<version>_amd64.deb`, `animathio_<version>_x64-setup.exe`, `AniMathIO_<version>_<arch>.dmg`, uploaded as `installers-{linux,windows,macos}`. The deb declares its runtime libraries in `[package.metadata.packager.deb] depends`; the AppImage still expects the host to provide them.
  - Icons are placeholders checked in under `apps/animathio/assets/icons` and regenerated by `apps/animathio/assets/gen-icons.py` (`.png` set for AppImage/deb/macOS, `.ico` for NSIS, `.icns` for the app bundle).
- **Swap rehearsal** (`scripts/swap-rehearsal.sh`) performs the documented 2.0 swap against a throwaway clone in a temp dir: tag the final v1, create `legacy`, land a simulated v1 hotfix on `main`, `git merge -s ours main` on `v2`, retarget workflows/Renovate to `main`, enable release-plz tagging, merge `v2` into `main` with a merge commit and delete `staging`/`release-please--branches--*`. Its "remote" is a bare repo in the temp dir and git is isolated from your global config, so it can never push to the real remote. It then verifies the result (README/LICENSE/AGENTS.md/CLAUDE.md present, no v1 files or workflows, workflows and Renovate scoped to `main`, one tag-producing release-plz package, TOML/YAML parse, workspace at 2.0.0, `scripts/check-docs.sh` passes) and prints WARN lines for manual follow-ups. Run `scripts/swap-rehearsal.sh --v2 HEAD` (add `--keep` to inspect the clone) whenever the release/branch automation changes. `docs/release-checklist.md` lists the manual steps.
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

## GUI testing: always use a virtual display

The maintainer keeps using this machine while agents work. **Never open windows on the
real desktop.** Run every GUI run, bench (`--bench-viewport`) and screenshot through
`scripts/virtual-display.sh`. It starts a private Xvfb display, forces X11 and unsets
`WAYLAND_DISPLAY`, then tears everything down afterwards:

```bash
ANIMATHIO_PLUGIN_DIR=target/plugins scripts/virtual-display.sh ./target/release/animathio --bench-viewport
ANIMATHIO_PLUGIN_DIR=target/plugins scripts/virtual-display.sh --screenshot docs/screenshots/editor-main.png 8 ./target/release/animathio
```

Don't use `spectacle`, `grim` or anything else that captures the user's session.
Vulkan (RADV) is available inside Xvfb. Benches there present through a software
path, so report those numbers as "Xvfb" and don't compare them with
native-compositor results.

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
Text, shapes and the clip effects are first-party WASM plugins (`org.animathio.text` /
`org.animathio.shapes` / `org.animathio.effects-basic`); build them with
`scripts/build-plugins.sh` before rendering. `render::effects` only runs the effect chain:
each `EffectInstance` (plugin id + effect id) goes to the `RenderCtx`'s `EffectProcessor`
(`ctx.set_effect_processor(host.effect_processor())`), so every `RenderCtx` or `Exporter`
that renders clips with effects needs one. Schema-1 builtin clips and schema-2 builtin
effects migrate in `format` to plugin clips/effects on load. Video frames come from the
`media` crate's `MediaPool` through `render::FrameSource`; a labelled checkerboard is only
the fallback when no frame source is attached. Do not add runtime
resources to `Project` or store evaluated animation state in the renderer.

Audio effects are `org.animathio.audio-basic` (build it with `scripts/build-plugins.sh`).
Clips and tracks carry ordered `audio_effects` chains (`EffectInstance`, with a `bypass`
flag) edited through `Command::{Add,Remove,Move}AudioEffect` /
`SetAudioEffectParam` / `SetAudioEffectBypass` on an `AudioChain::{Clip,Track}`; schema 4
introduced them. Chains run inside `Mixer::mix` on a fixed 2048-frame block lattice with
state carried as opaque plugin blobs and a bounded warm-up per 64-block segment, so output is
a pure function of the project; they need `Mixer::with_effect_processor(Arc::new(
host.audio_effect_processor()))` (editor playback registers one in `playback.rs`, `Exporter`
has `set_audio_effect_processor`). A mixer without a processor errors on a project with
active effects; `Mixer::dry()` ignores chains (silence detection). Effects run in the mix
worker, never in the device callback.

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
are prototype limits; streaming and surround layout matrices are deferred. Timeline waveforms come from
`AudioPool::waveform` (4096 downsampled peaks per asset, cached by the editor and
pruned with the pool).

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
