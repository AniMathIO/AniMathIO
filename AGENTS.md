# AGENTS.md

Guidance for coding agents working on AniMathIO 2.x (`main`). `CLAUDE.md` imports this file.

## What this branch is

AniMathIO 2 is a from-scratch Rust rewrite of AniMathIO, an editor for Manim-like math animation videos. The full plan, architecture and roadmap are in [issue #156](https://github.com/AniMathIO/AniMathIO/issues/156). Read it before proposing structure. The short version:

- **The core is a simple video editor:** media clips, a multi-track timeline, transforms + keyframes, audio, preview and export. Nothing math-specific lives in the core.
- **Everything else is a plugin:** sandboxed WebAssembly components (wasmtime, Component Model, WIT). This includes text, LaTeX, graphs, Manim import, effects, the v1 importer and MCP.
- **Rendering is one pure function,** `render(&Project, Time) -> Frame`, shared by preview, export and tests.
- **Every mutation is a serializable, invertible `Command`.** Undo/redo, the control protocol, MCP and plugins all go through it.
- **Time is integer ticks + rational frame rates. Coordinates are resolution-independent.** No `f64` seconds and no absolute pixels in the model.
- **The core owns its domain types.** Third-party crates (FilmCraft codecs, vello, GPUI) sit behind adapters.

## Docs

Docs live in `docs/` and are kept accurate against the code — every command and
API they describe must exist. Update them alongside behaviour changes:

- `README.md` — what AniMathIO 2 is, status, build/run and import instructions
- `docs/architecture.md` — crate map, layering, time model, engine rules, data flow
- `docs/plugins.md` — plugin authoring guide (WIT world, host imports, schema, sandbox, example)
- `docs/cli.md` — every `animathio-cli` subcommand (regenerate from `--help`)
- `docs/mcp.md` — control protocol (JSON-RPC, socket, token) and the MCP tool surface
- `docs/migrating-from-v1.md` — what maps from 1.x and known gaps
- `docs/benchmarks.md` — v1 vs v2 benchmark methodology, results and caveats (`scripts/bench-v1-v2.sh`)
- `docs/dependency-advisories.md` — dependency advisory baseline and exception reachability evidence
- `docs/release-checklist.md` — historical 2.0 swap checklist and release automation follow-ups

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

- PRs target **`main`**. The 1.x Electron app is frozen in the read-only `legacy` branch (from tag `animathio-v1.8.3`); never merge it back. The 2.0 swap (see #156, "Repository and branch plan") is done.
- Use Conventional Commits. release-plz will derive versions and the changelog from them.

## Automation

Automation is configured in `renovate.json`, `release-plz.toml` and `.github/workflows/{release-plz,package,security-audit}.yml`. It is scoped to `main`.

- **Renovate** (`renovate.json`) targets `baseBranchPatterns: ["main"]` (Renovate's current name for what was formerly `baseBranches`). Patch/minor/security updates are grouped and auto-merged once CI passes (GitHub auto-merge); majors are never auto-merged and get a `needs-human` label. Lockfile maintenance runs weekly and vulnerability alerts are on. GitHub Actions are pinned by digest.
  - **Manual step:** the GitHub **Renovate** app must be enabled for the repo (an agent cannot enable it). Renovate reads config from the default branch (`main`). Auto-merge needs "Allow auto-merge" enabled in the repo settings and CI as a required status check.
- **No Dependabot.** Do not add `.github/dependabot.yml`; Renovate covers it.
- **release-plz** (`release-plz.toml` + `.github/workflows/release-plz.yml`) opens a release PR (version bump + git-cliff changelog) on pushes to `main`; merging it tags `v{{version}}` and creates the GitHub release (the `v2.0.0` baseline was tagged by hand at the swap). crates.io publishing stays off.
  - **Workspace changelog:** the pinned release-plz 0.4.3 only collects `changelog_include` histories from release-enabled packages that are publishable or in git-only mode. Keep workspace `git_only = true`, `git_tag_name = "v{{ version }}"` and `changelog_update = false`; the CLI overrides changelog updates and includes every other workspace member. Keep internal `git_tag_enable`/`git_release_enable` false, and do not set their `release = false` (it excludes their histories). Add new workspace members to the CLI include list. `cliff.toml` still filters non-user-facing commits.
  - **Changelog rules live in `cliff.toml`** (read by release-plz via `changelog_config`, runnable standalone): `feat`→Added, `fix`→Fixed, `perf`→Performance, `*(security)`→Security, breaking `type!:` kept; `chore`, `ci`, `build`, `docs`, `test`, `style`, `refactor`, clippy fixes and the `ci`/`dev`/`spikes`/`package`/`deps` scopes are hidden; v1 tags never count as releases. Preview with `git cliff --config cliff.toml --tag v2.0.0 origin/main..origin/v2` (install with `cargo install --locked git-cliff`).
- **Sub-workspace lockfiles** (`scripts/sync-sub-lockfiles.sh`): the nested Cargo workspaces that path-depend on workspace crates (`fuzz/`, `plugins/import-manim/`) keep their own `Cargo.lock`, which goes stale when release-plz bumps the workspace version on a release PR and breaks their `--locked` builds. `release-plz.yml` checks out the release-plz branch right after its `release-pr` step, runs the script (`cargo update -w` per affected nested workspace: workspace-crate entries only; a Python 3.11+ TOML guard rejects new registry/git versions) and pushes the lockfile changes as a `github-actions[bot]` commit (a bot commit keeps release-plz force-updating the branch instead of closing and re-opening the PR). `.github/workflows/ci.yml` runs the script with `--check` on non-docs-only PRs and pushes except same-repository release-plz PR branches (which the release-plz workflow syncs itself) and fails with the fix command when a lockfile is stale. Pushes made with the default `GITHUB_TOKEN` start CI on the release PR only after maintainer approval; the `RELEASE_PLZ_TOKEN` secret (see `docs/release-checklist.md`, section 2) removes that step.
- **Flatpak** (`packaging/flatpak/com.animathio.AniMathIO.yml`, `.github/workflows/flatpak.yml`) builds an offline Freedesktop 26.08 bundle with portal file access and eight plugins; runtime network access supports community plugin Browse/downloads, while the in-app update check is disabled; regenerate `cargo-sources.json` (`python packaging/flatpak/generate-sources.py`) whenever any lockfile changes; the `Flatpak cargo sources` step in `ci.yml` runs `scripts/check-flatpak-sources.py` on every non-docs run and fails when a lockfile crate is missing from, mismatched in or stale in that file (so Renovate lockfile bumps cannot silently stale it). See `docs/flatpak.md` for install and Flathub submission steps.
- **Packaging** (`apps/animathio/Cargo.toml` `[package.metadata.packager]` + `.github/workflows/package.yml`) builds the installers for the canonical desktop package, which ships both `animathio` and `animathio-cli`: AppImage + deb on Linux, an NSIS installer on Windows, and an unsigned dmg on macOS. Each job builds the first-party plugin bundles with `scripts/build-plugins.sh` first (cargo-packager's `resources` target `plugins`), then runs `cargo packager --release --packages animathio`, installs/extracts the result, runs `animathio --version` and `animathio-cli --version` headlessly, counts the packaged `.amplugin` files, and attests build provenance. It runs on `workflow_dispatch`, on release PRs and on PRs touching packaging inputs.
  - Config paths are relative to `apps/animathio` (cargo-packager `cd`s there), so `out-dir` is `../../target/packages` and resources are `../../target/plugins`; `binaries` paths are relative to the workspace `target/release`.
  - Resources land in `plugins/` beside the executable on Windows, `usr/lib/animathio/plugins` in the deb/AppImage and `Contents/Resources/plugins` in the macOS app; `crates/plugin-host/src/discovery.rs` searches all of those relative to the running executable.
  - Artifacts: `animathio_<version>_<arch>.AppImage`, `animathio_<version>_amd64.deb`, `animathio_<version>_x64-setup.exe`, `AniMathIO_<version>_<arch>.dmg`, uploaded as `installers-{linux,windows,macos}`. The deb declares its runtime libraries in `[package.metadata.packager.deb] depends`; the AppImage still expects the host to provide them.
  - Icons use the restored original logo, checked in under `apps/animathio/assets/icons` and regenerated by `apps/animathio/assets/gen-icons.py` (`.png` set for AppImage/deb/macOS, `.ico` for NSIS, `.icns` for the app bundle).
- **Swap rehearsal** (`scripts/swap-rehearsal.sh`) performs the documented 2.0 swap against a throwaway clone in a temp dir: tag the final v1, create `legacy`, land a simulated v1 hotfix on `main`, `git merge -s ours main` on `v2`, retarget workflows/Renovate to `main`, enable release-plz tagging, merge `v2` into `main` with a merge commit and delete `staging`/`release-please--branches--*`. Its "remote" is a bare repo in the temp dir and git is isolated from your global config, so it can never push to the real remote. It then verifies the result (README/LICENSE/AGENTS.md/CLAUDE.md present, no v1 files or workflows, workflows and Renovate scoped to `main`, one tag-producing release-plz package, TOML/YAML parse, workspace at 2.0.0, `scripts/check-docs.sh` passes) and prints WARN lines for manual follow-ups. Run `scripts/swap-rehearsal.sh --v2 HEAD` (add `--keep` to inspect the clone) whenever the release/branch automation changes. `docs/release-checklist.md` lists the manual steps.
- **Security** (`.github/workflows/security-audit.yml`) runs daily: `cargo deny check advisories` once `deny.toml` exists (added in the media PR), else `cargo audit`.
- **CI docs-only fast path** (`.github/workflows/ci.yml`): a PR or push that only changes documentation — Markdown anywhere, `docs/` other than the `docs/plugins.md` fixture that a plugin-api test embeds verbatim, root `LICENSE` files, and the `.github/` issue/pull-request templates — skips the toolchain, caches, builds, clippy, tests, whisper build, showcase E2E and license check. The `check (ubuntu-latest)`/`check (windows-latest)` jobs still run and report success (they are required checks); on Linux the fast path runs `scripts/check-docs.sh` without `--require-binary`, which checks subcommand names against the source instead of a built CLI.

## Porting from v1

The v1 implementation (Electron/TypeScript) is frozen on `legacy` and is a useful reference. Read it without checking it out:

```bash
git show origin/legacy:renderer/utils/manim/translator.ts    # Manim subset translator (to port to the manim-import plugin)
git show origin/legacy:renderer/types.ts                     # v1 element/animation model (input to the import-v1 plugin)
git show origin/legacy:renderer/states/stores/ProjectStore.ts # v1 .animathio (de)serialization
git show origin/legacy:main/mcp/tool-schemas.ts              # v1 MCP tool surface (parity target)
```

Port behavior, not structure. The v1 store/component layout is exactly what AniMathIO 2 replaces.

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
The same goes for sound: the script exports `ANIMATHIO_AUDIO=none`, so the editor never
opens the audio device and playback follows the wall clock, and it points PipeWire/Pulse
clients at a server that doesn't exist. Never play audio on the maintainer's speakers:
don't run the `play` example or the ignored device test unless asked, and use
`ANIMATHIO_AUDIO=none` for any editor run outside the script.
Vulkan (RADV) is available inside Xvfb. Benches there present through a software
path, so report those numbers as "Xvfb" and don't compare them with
native-compositor results.

## UI tests (headless editor windows)

`apps/animathio/src/editor/ui_tests/` runs the real `Editor` view in gpui's test platform
(`gpui/test-support` as a dev-dependency of `animathio`), so UI behaviour is tested in plain
`cargo test --workspace` on Linux and Windows with no display, GPU or audio device. These
tests never open a window on the desktop, so they do not go through `scripts/virtual-display.sh`.

- `Editor::new` is `Editor::with_env(.., EditorEnv::production())`. `EditorEnv`
  (`src/editor_env.rs`) injects the audio output (`AudioOutput::Null` = monotonic clock, no
  device), the preview backend (CPU), the plugin source (`PluginSource::Dir`/`None`) and the
  config directory (settings, recent files, plugin cache). `EditorEnv::headless` uses a null sink,
  the CPU backend, the plugin bundles in `target/plugins` when built (else none) and a private
  config dir under `target/ui-test-config`, so tests never touch the user's settings. Settings
  and plugin loading go through `editor_env::{config_file, plugin_source, plugin_cache_dir}`:
  new code must use them instead of `ProjectDirs`/`BaseDirs`.
- A test is a `#[gpui::test] fn name(cx: &mut TestAppContext)` in `ui_tests/scenarios.rs`.
  `Ui::open(cx, project, assets)` mounts the editor in a `Root`; then `ui.dispatch(Action)`
  (like a menu item), `ui.press("m")` (through the keymap), `ui.update(|editor, window, cx| ..)`
  to call editor methods, `ui.settle()` for background work (analysis, audio prep) and
  `ui.read(|editor| ..)` to assert. Assert on the project model and view state
  (`marker_panel`, `autocut_review`, `selected`, `dock`, `left_tab`), not on pixels.
  `fixtures.rs` has the shared sample, a synthetic voice + music mix and caption cues.
- To exercise shared handler logic directly, move it into an `Editor` method
  (like `set_autocut_scope` / `apply_autocut`) and have the closure call it.
- Scenarios that need first-party plugins call `require_plugins(..)` and skip when
  `scripts/build-plugins.sh` has not produced `target/plugins` (CI builds it before testing).
- Windows are rendered whenever the editor is updated, so render panics surface too.
- Coverage includes Premiere tool/split/ripple-trim keys and menu bindings, marker ruler
  selection, speed changes with ripple and undo, and the Math dialog apply path on an
  occupied layer. Some panel scenarios call shared handlers directly; they do not prove
  that every form field or button is reachable by mouse or keyboard.

## Commands

The workspace uses stable Rust, edition 2024. Standalone `spikes/` projects are excluded.
Run from the repository root (the resource limits protect other work on shared machines):

```bash
CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=line-tables-only nice -n 19 ionice -c3 cargo build --workspace --locked
CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=line-tables-only nice -n 19 ionice -c3 cargo fmt --all --check
CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=line-tables-only nice -n 19 ionice -c3 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=line-tables-only nice -n 19 ionice -c3 cargo test --workspace --locked
```

Fuzzing untrusted-input parsers with cargo-fuzz (libFuzzer) lives in the standalone `fuzz/`
workspace (own `[workspace]`, excluded from the main one, like `spikes/`). Targets:
`project_json` (project JSON + migrations), `project_amio` (.amio container), `v1_import`
(v1 importer), `manim_source` (Manim parser) and `media_probe` (media probe/open). Seeds are in
`fuzz/corpus/<target>/`, minimized crash repros in `fuzz/regressions/<target>/`. Run a target
for ~5 minutes with `nice -n 19 cargo +nightly fuzz run <target> -- -max_total_time=300`
(nightly is required); CI runs each target for 60 seconds via the manual `workflow_dispatch`
fuzz job.

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

`plugins/text` is its own Cargo workspace, so `cargo test --workspace` skips its native unit tests: run
`cargo test --manifest-path plugins/text/Cargo.toml --locked --target-dir target/plugin-tests` (CI has a
`Text plugin unit tests` step); behaviour that needs the host (limits, fuel, layout parity) is tested in
`crates/control/tests/text_weight.rs`. The text plugin's `font` parameter (`dejavu` default | `inter`) and
`font_weight` select the typeface: `dejavu` at weight 400 is the host `TextRun`, everything else is outlined in the
plugin from bundled Inter with per-character DejaVu fallback; typography presets and the v1 importer set `inter`.

Plugin ABI 1.1.0 adds two worlds: `animation-preset-plugin` (`org.animathio.animations`;
`PluginHost::animation_preset_command` turns a preset into one keyframe `Command::Batch`,
shared by `animations.apply`, MCP `apply_animation_preset`, `animathio-cli animations` and
the inspector) and `importer-plugin` (`org.animathio.import-manim`; `PluginHost::import`
replays the plugin's version-1 command document and grants bounded `read-sibling` reads).
`crates/import-manim` parses Python with its own pure-Rust `pyparse` (tree-sitter's C
runtime cannot target `wasm32-wasip2` here) and is linked only into that plugin;
`import-v1` stays native. Plugin ABI 1.2.0 adds the opt-in `colored-source-plugin` world (`math-colors`:
`typeset-math-colored`, per-path optional solid paint from `TypesetResult::colors`) used by
`org.animathio.latex` for inline `\color`/`\textcolor`. Keep 1.0 and 1.1 components loading
unchanged (`hello-1.0.0` fixture); never edit frozen WIT records, add a new world/interface instead.

Community plugins (`crates/plugin-host/src/community.rs`) are the third-party `.amplugin` files in the
per-user folder (`ANIMATHIO_USER_PLUGIN_DIR` overrides it). They are off by default: nothing in that folder
loads unless the global switch and the plugin's own flag in `community-plugins.json`
(`ANIMATHIO_PLUGIN_SETTINGS`, in the editor config dir; one source of truth read through `CommunityStore` by
the editor, `animathio-cli plugins` and the control handler) are both on. `PluginHost::load_plugin_directory`
never loads a `PluginDirectory { community: true }`; `PluginHost::sync_community_plugins` reconciles the
loaded set with the settings and `unload_plugin` drops a plugin's clip source, effects, audio effects, presets
and importer, which is how the editor hot loads and unloads without a restart. Installs verify a SHA-256
before writing, run the normal package/manifest/ABI validation and refuse reserved `org.animathio.*` ids; the
control protocol only has the read-only `plugins.list`.

Audio effects are `org.animathio.audio-basic` (build it with `scripts/build-plugins.sh`).
Clips and tracks carry ordered `audio_effects` chains (`EffectInstance`, with a `bypass`
flag) edited through `Command::{Add,Remove,Move}AudioEffect` /
`SetAudioEffectParam` / `SetAudioEffectBypass` on an `AudioChain::{Clip,Track}`; schema 4
introduced them. Chains run inside `Mixer::mix` on a fixed 2048-frame block lattice with
state carried as opaque plugin blobs and a bounded warm-up per 64-block segment, so output is
a pure function of the project; they need `Mixer::with_effect_processor(Arc::new(
host.audio_effect_processor()))` (editor playback registers one in `playback.rs`, `Exporter`
has `set_audio_effect_processor`). A strict mixer (export, the default) errors on a project with
active effects it cannot run; `Mixer::lenient(warnings)` (editor playback) skips the failing
effect with a warning instead; `Mixer::dry()` ignores chains (silence detection). Effects run in the mix
worker, never in the device callback.

Automatic subtitles live in `crates/transcribe` (whisper.cpp through whisper-rs behind the
opt-in `whisper` cargo feature, forwarded by `animathio-control`, `animathio-cli` and
`animathio`; building it needs cmake, a C++ toolchain and libclang, so the default build and
`cargo test --workspace` stay engine-free). Models (`tiny`/`base`/`small`) are never bundled:
they download on demand with a pinned SHA-256 into the user data dir (`ANIMATHIO_MODEL_DIR`
overrides it). Cue splitting and SRT/WebVTT are pure and unit-tested; the model-backed test is
ignored: `cargo test -p animathio-transcribe --features whisper --test whisper_tiny -- --ignored`.
The editor job (`subtitle_job.rs`, `editor/subtitles.rs`) runs on a worker with progress and cancel
and applies the cues as one `Command::Batch` through `control::subtitle_command`.
Caption styles (`control::captions`, `org.animathio.text` 1.1.0 parameters `caption_style`, `words`,
`accent`, ...) are plugin parameters on the caption clips, not format fields; per-word timings
ride on `Cue::words` and `subtitles.style` / `set_caption_style` restyle existing captions.

Audio clips use numeric `gain` (linear, default 1) and `muted` (nonzero means muted,
default 0) parameters, including clip-local keyframes. Track gain is changed through
`Command::SetTrackGain`. `AudioPool::prepare(rate, channels)` is cheap: it plans the output
format, and decoding, resampling and channel mapping then run lazily inside
`Mixer::mix(project, range)` (shared by preview and export) in fixed blocks held by an LRU cache
with a byte budget (`PrepareOptions`, default 256 MiB). Nothing holds a whole asset: the pool keeps
one seekable `PcmReader` per asset (`AudioSource::open_reader`; `SymphoniaSource` streams, `Pcm`
stays resident), so memory is independent of the length of the media. Output equals the old
whole-asset decode and resample bit for bit: the FFT resampler lets each block start one chunk
early (pre-roll), and the sinc fallback used only for extreme rate ratios replays sequentially. Insert
long assets with `pool.insert(id, &SymphoniaSource::open(..)?)`; do not call `decode_all` on them.
The device callback only consumes the ring and updates the latency-corrected master
clock; `Player::now()` is quantized to callbacks and freezes on underflow. Seek drops
the old stream/ring and joins its worker before starting a new session.

Linux builds need `libasound2-dev` (CI installs it). Normal tests need no device.
The ignored device test can be run explicitly; `cargo run -p animathio-audio
--example play` emits a 0.01-amplitude tone for at most 3 seconds and prints clock
drift. Apply the same resource limits to these cargo invocations. Symphonia 0.5
supports WAV/PCM, MP3, FLAC, AAC-LC/M4A and Vorbis; Opus (Ogg, Matroska/WebM, MP4) is decoded by
the pure-Rust `filmcraft-opus` crate (MIT/Apache, no system libraries) behind Symphonia's
demuxers; the pre-skip is removed, Matroska/MP4 keep up to one packet of trailing padding. Opening a
`SymphoniaSource` scans the asset once (bounded memory) for its exact length, the `DecodeLimits`
and one restart packet per 2^21-frame segment; reads then reopen the container, skip to that packet
without decoding and decode from there after a quarter-second pre-roll, so a frame's samples never
depend on what was read before. That is bit-identical to a front-to-back decode for PCM, FLAC,
MP3 and Vorbis, and within a small tolerance for AAC (random noise substitution) and Opus (energy
prediction), which the tests state. Channel mapping (`ChannelLayout`, `ChannelMatrix`):
identical layouts copy, mono duplicates, stereo averages to mono, 5.1/7.1/quad and other
surround layouts fold to stereo with ITU-R BS.775 coefficients (centre and surrounds -3 dB, LFE
dropped, no normalisation; both rear pairs of 7.1 at -6 dB) and to mono as the mean of that fold,
and stereo to a wider layout fills the front pair; layouts without a rule are `Unsupported`.
Timeline waveforms come from `AudioPool::waveform` (a streaming pass; 4096 downsampled peaks per
asset, cached by the editor and pruned with the pool). The export path wraps the media backend's
audio decoder as an `AudioSource` too, so exporting a long track never holds it decoded.

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
