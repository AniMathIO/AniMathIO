# Dependency Notes

This file documents dependency decisions that aren't obvious from `package.json`
alone: majors we deliberately hold back, and how we handle a peer-dependency
conflict that npm currently resolves silently.

## Deliberately deferred majors

The following major upgrades are **intentionally not applied**, even though
`npm outdated` reports newer majors available. Do not bump these without
re-validating the specific incompatibility below first.

- **Konva 9.x → 10.x** — Konva 10 shipped as pure ESM. The renderer's
  Turbopack-based Next.js dev/build pipeline still has code paths that
  `require()` Konva during SSR (`react-konva` server-side no-op rendering),
  which breaks under a pure-ESM package. Needs an SSR/Turbopack compatibility
  pass before upgrading.
- **Nextron 9.x → 10.x** — Nextron 10 assumes a pnpm-based workspace/build
  workflow. This project is npm-based (`package-lock.json`, npm scripts,
  `postinstall` using `electron-builder install-app-deps`). Migrating package
  managers is out of scope for a routine dependency bump.
- **TypeScript 5.x → 7.x** — TypeScript 7 moves to a native (Go-based)
  compiler. Next.js's own dependency verification/tooling has not been
  validated against the native compiler yet, and a mismatch here risks
  breaking `next build`'s internal typechecking. Hold at 5.x until Next.js
  officially supports it.
- **mobx 6.x → 7.x** (and by extension `mobx-react` 9.x→10.x /
  `mobx-react-lite` 4.x→5.x, which only support mobx 7) — not yet vetted
  against this app's stores (`renderer/states/stores/*`). Needs a dedicated
  pass to check for API/behavior changes before moving the whole mobx family.
- **Vitest 4.x → 5.x** (and its `@vitest/*` siblings: `browser`,
  `browser-playwright`, `coverage-v8`, `ui`) — kept in lockstep at 4.x since
  these packages pin exact matching peer versions of each other. Bumping one
  without the rest breaks resolution; bumping all of them is a larger,
  separate validation effort.

Everything else outdated at the time of writing was a same-major patch/minor
bump and was applied directly (electron, react/react-dom, next +
eslint-config-next, @google/genai, eslint, playwright/@playwright/test, tar,
various `@types/*`, tailwindcss/@tailwindcss/postcss/postcss/autoprefixer,
etc.). `jsdom` was left at `29.1.1` — that is already the latest `29.x`
release; the newer `30.x` is a major bump and was treated the same as the
majors above (not attempted here).

## `mafs` / `use-resize-observer` / React peer conflict

`mafs@0.21.0` depends on `use-resize-observer@^9`, whose `9.1.0` release
peer-requires `react-dom@"16.8.0 - 18"`. This project runs `react-dom@19.x`,
so `npm install` reports an `ERESOLVE overriding peer dependency` warning and
proceeds anyway (npm treats this as a warning, not a hard failure, since
neither `mafs` nor `use-resize-observer` published a version compatible with
React 19 as of this writing).

We checked whether upgrading `mafs` itself resolves this: the latest published
`mafs` is still `0.21.0` (no newer release exists), so there's nothing to
upgrade there.

We also tried forcing the transitive `use-resize-observer` up to its `10.0.0`
release via `package.json` `overrides` (following the existing
`overrides.node-abi` precedent for ABI/peer mismatches). That failed harder
than the peer warning: `use-resize-observer@10.0.0` dropped its default export
in favor of named exports, and `mafs@0.21.0`'s bundled build
(`node_modules/mafs/build/index.js`) does `import useResizeObserver from
"use-resize-observer"` — a default import. That breaks at bundle time
(`MISSING_EXPORT` during Vite/Rolldown dependency optimization in
`test:browser`), not just at peer-resolution time. So forcing v10 is worse
than the status quo.

**Decision:** leave `use-resize-observer` resolved at its natural `^9` range
(currently `9.1.0`) and accept the peer warning. In practice this works fine:
`use-resize-observer@9.1.0`'s actual runtime code (a `ResizeObserver` hook)
has no real API dependency on the internals of React 16 vs 19 — the peer
range is just conservative/stale. `npm run test:unit` and `npm run
test:browser` both pass with this arrangement, and Mafs graphs render and
resize correctly in the app. If a future `mafs` or `use-resize-observer`
release relaxes the peer range or adds React 19 support, prefer picking that
up over adding a permanent override.

No `overrides` entry was added for this (unlike `node-abi`, where the pinned
version was strictly necessary) — the existing silent-override behavior is
the least-risk option available today.
