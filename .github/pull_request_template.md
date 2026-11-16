<!--
Target `v2` for 2.0 work (never `main` or `staging`). Use a Conventional Commit title,
for example `feat(timeline): snap to markers`.
-->

## What and why

## Checklist

- [ ] Base branch is `v2`
- [ ] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` pass
- [ ] Model changes go through invertible `Command`s; no `f64` seconds or absolute pixels in the model
- [ ] Docs updated (`README.md`, `docs/*.md`) and `scripts/check-docs.sh` passes if the CLI, MCP, control methods or crates changed
- [ ] GUI changes were run through `scripts/virtual-display.sh`, with a screenshot if the UI changed
- [ ] Tests added or updated; goldens regenerated only on purpose

## Deferred / not covered
