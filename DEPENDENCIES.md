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

  **Dependabot will keep proposing this one, bundled into security-group PRs
  where it is easy to miss** — it has arrived twice inside PRs whose body
  listed only the unrelated packages (and again in PR #128). Always diff `package.json` on a
  dependabot PR before merging, not just the summary table. Nextron is what
  builds the release binaries, so a surprise major here breaks packaging.

  PR #128's security versions of `@babel/core` (7.29.7) and `webpack`
  (5.109.2) are pinned as devDependencies with scoped `nextron` overrides
  referencing those pins. Direct pins alone leave Nextron's exact older
  dependencies installed in nested directories, so the overrides are needed
  to update the build pipeline without changing Nextron itself.

  The previously reported advisories for `serialize-javascript`,
  `@babel/runtime`, `terser-webpack-plugin`, and Nextron itself are separate
  from those two updates. These are build-toolchain devDependencies; do not
  merge a Nextron major purely to clear them. Recheck with an online audit
  when revisiting the deferred migration.
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
- **zod 3.x → 4.x** (advisory GHSA-mrhx-c7xc-m3w5, **known high, deferred**) —
  `npm audit` reports a HIGH for `zod@3.25.76`; the fix only exists in
  `zod >= 4.6.6`, so it cannot be fixed on 3.x. zod is pulled in transitively
  by `@imgly/background-removal`, which `npm audit` also flags via zod.
  `@imgly/background-removal@1.7.0` is already the latest release and pins
  zod 3, so a zod 4 override breaks it. **Risk accepted:** AniMathIO is a local
  desktop app and exploiting the advisory needs attacker-controlled oversized
  input to be fed into zod parsing. Revisit when `@imgly/background-removal`
  ships a zod 4 compatible release.

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

## `next` >= 16.4.1 / `@ffmpeg/ffmpeg` worker URL

`next` is pinned at `16.4.4` (with `eslint-config-next` in lockstep) because of
a critical security advisory fixed in 16.4.1. Next 16.4.1–16.4.4 make Turbopack
fail `next build` on `@ffmpeg/ffmpeg@0.12.15`:

```
./node_modules/@ffmpeg/ffmpeg/dist/esm/classes.js:105:28
Module not found: Can't resolve (<dynamic> | 'undefined')
  new Worker(new URL(classWorkerURL, import.meta.url), ...)
```

`FFmpeg#load()` has a fully dynamic `new URL(classWorkerURL, import.meta.url)`
branch, which older Turbopack tolerated and newer versions reject. 0.12.15 is
the latest stable `@ffmpeg/ffmpeg`, so there is no upgrade path, and passing
`classWorkerURL` ourselves does not help (the failure is at compile time, on
the unreachable branch). Fix: a small Turbopack loader
(`renderer/build/ffmpeg-static-worker-loader.js`, wired in
`renderer/next.config.js`) removes that branch so only the static
`new URL("./worker.js", import.meta.url)` remains; Turbopack then emits the
worker into `static/media/`. The loader throws if the upstream source changes
shape, and is covered by `tests/renderer/build/`. Drop it once an
`@ffmpeg/ffmpeg` release (or Next) fixes this.

## Landed from the deferred-majors trial (#138)

Three of the four bumps tried in draft PR #138 were verified and landed
(baseline: tsc 0 errors, `npm audit` 40 vulnerabilities: 2 low / 34 moderate / 4 high):

- **happy-dom 20.10.6 -> 21.0.4** — plain devDependency bump; tsc, unit,
  browser and build unchanged. Clears Dependabot #392/#393.
- **katex 0.16 -> 0.18.11** — `mafs@0.21.0` (still the latest) depends on
  `katex@^0.16`, and katex is not a direct dependency (the app imports it
  transitively), so this is a top-level `overrides.katex` entry. Dedupes to a
  single katex. Clears #366 (katex < 0.18.2). Latest katex is 0.19.1, not
  tried. Consider declaring katex as a direct dependency since the app imports it.
- **serialize-javascript 6.0.2 -> 7.1.4** — `overrides.serialize-javascript`
  only; nextron stays at 9.5.0. Only consumer is nextron's
  `terser-webpack-plugin@5.3.11`. Clears #98 and #212.

## zod 3 -> 4 stays deferred

The app has no direct zod dependency; everything resolves to one
`zod@3.25.76` (via `@imgly/background-removal@1.7.0`,
`@modelcontextprotocol/sdk`, and eslint-plugin-react-hooks). Forcing zod 4 via
`overrides.zod` passes tsc/unit/browser/build, but
`@imgly/background-removal@1.7.0` (latest, no newer release) evaluates
`z.function().args(...)` at module load, and zod 4 throws
`z.function(...).args is not a function`. Tests do not cover this import, so
it would crash the image background-removal feature at runtime. Alert #410
(zod < 4.6.6) has no 3.x fix, so it stays open until background-removal is
replaced or updated upstream.

## Open dependency items (2026-11-02 maintenance run)

- **zod** — alert stays open, blocked on `@imgly/background-removal` (latest 1.7.0 has no zod 4 support); see "zod 3 -> 4 stays deferred".
- **eslint 10 peer ranges** — `npm ls` reports `eslint-plugin-import`, `eslint-plugin-jsx-a11y` and `eslint-plugin-react` as invalid because their peer ranges stop below eslint 10. Linting works; accept until those plugins widen their ranges.
- **minimatch pins** — `overrides` pin minimatch 3.1.5 for `@electron/asar` and `dir-compare` (under `app-builder-lib`) and 5.1.9 for `ejs`, because those consumers need the older API. Do not bump these to 11.x without checking each consumer.
