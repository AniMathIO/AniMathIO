# Contributing to AniMathIO 2.0

Thanks for helping. This is the 2.0 Rust rewrite. The plan and architecture are in
[issue #156](https://github.com/AniMathIO/AniMathIO/issues/156); [`AGENTS.md`](AGENTS.md) is the
working guide for commands, automation and conventions (coding agents and humans alike), and
[`docs/architecture.md`](docs/architecture.md) is the crate map.

## Branches

- 2.0 pull requests target **`v2`**. Never target `main` or `staging`: they serve v1 until the
  2.0 swap. Do not merge `main` or `staging` into `v2`.
- Work on a topic branch in your fork (or a branch of the repo), named for what it does, for
  example `v2-timeline-snap`.
- Fixes for the 1.x Electron app go to `main` until the swap; ask in an issue first.

## Build and check

Stable Rust, edition 2024; Linux also needs `libasound2-dev`. Build the first-party plugins
before rendering text or shapes:

```bash
scripts/build-plugins.sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
scripts/check-docs.sh          # after changing the CLI, MCP, control methods or crate layout
```

On a shared machine use the resource limits from `AGENTS.md` (`CARGO_BUILD_JOBS=3`, `nice`,
`ionice`). CI runs the same checks on Linux and Windows, and the golden render tests allow at
most 2 per RGBA channel; regenerate goldens only on purpose (`UPDATE_GOLDEN=1`) and commit the
reviewed PNGs with their fixtures.

## Testing the editor GUI

Never open windows on your real desktop from scripts or agents. Run every GUI launch, bench and
screenshot through the virtual display wrapper, which starts a private Xvfb:

```bash
ANIMATHIO_PLUGIN_DIR=target/plugins scripts/virtual-display.sh ./target/release/animathio --bench-viewport
```

## Code rules (short version)

- Every model mutation is a serializable, invertible `Command`; use `Command::Batch` so a
  compound edit is one undo step.
- Time is integer ticks with rational frame rates; coordinates are resolution-independent. No
  `f64` seconds or absolute pixels in the model.
- Rendering is the pure `render(&Project, Time) -> Frame`; never put runtime resources in
  `Project`.
- UI icons come from gpui-component's Lucide set, never emoji or Unicode glyphs.
- Add tests for new commands and logic, and update `README.md` / `docs/*.md` with behaviour
  changes. Docs must match the code (`scripts/check-docs.sh`).

## Writing a plugin

Text, shapes, effects and audio effects are WebAssembly components. Start with
[`docs/plugins.md`](docs/plugins.md); the minimal example under `plugins/examples/hello` must keep
compiling for `wasm32-wasip2`.

## Commits and pull requests

- Use [Conventional Commits](https://www.conventionalcommits.org/): `feat(scope): ...`,
  `fix(scope): ...`, `perf`, `docs`, `test`, `refactor`, `chore`, `ci`, `build`; add `!` for
  breaking changes. release-plz builds the version and changelog from them (see `cliff.toml`).
- Keep a PR focused, describe what changed and why, link the issue, and list anything you left
  out. Fill in the pull request template.
- Squash or merge as the maintainer prefers; PRs are reviewed before merging.

## Reporting bugs and vulnerabilities

Use the issue templates for bugs and feature requests. Report security problems privately, as
described in [`SECURITY.md`](SECURITY.md).

By contributing you agree that your work is released under the project's [MIT license](LICENSE).
