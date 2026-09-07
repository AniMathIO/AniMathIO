# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

AniMathIO is an Electron desktop video editor (via Nextron: Next.js renderer + Electron main process) for creating Manim-like mathematical animation videos — text, images, math graphs (Mafs), and background audio, composited on a Konva canvas and exported to MP4/WebM.

## Commands

```bash
npm install                      # also runs postinstall: electron-builder install-app-deps + rebuild canvas
npm run dev                      # launch the Electron app (Next.js renderer on Turbopack, port 8888)
npm run dev:8889                 # same, alternate renderer port (for running two instances)

npm run test:unit                # vitest, jsdom environment — tests/renderer/**, tests/main/**
npm run test:unit:watch
npm run test:browser             # vitest browser mode, real headless Chromium via @vitest/browser-playwright — tests/browser/**
npm run test:browser:watch
npm test                         # test:unit && test:browser (this is what CI runs)
npx vitest run --config vitest.unit.config.mts <path/to/file.test.ts>   # single test file
npx tsc -p renderer/tsconfig.json --noEmit   # type-check (use this config, not the bare root tsconfig.json —
                                              # the root one lacks path-alias resolution and always shows noise)

npm run build                    # nextron build — local/dev electron-builder output (dist/), all installed targets
npm run build:linux              # AppImage + deb + snap (what CI's release workflow actually runs)
npm run build:win                # NSIS installer
npm run build:mac                # unsupported since 1.3.0 — do not spend time debugging mac-specific build issues
```

If `canvas` fails to rebuild against the current Electron ABI: `npm rebuild canvas --update-binary`.

`npm run build`/`build:linux` are slow (they download/package a full Electron runtime) — don't run them reflexively after every change; unit/browser tests are the fast feedback loop. `build:linux` is worth running before anything that touches `package.json`, `electron-builder.yml`, or export/packaging logic, since it's not exercised by the push-triggered CI workflow (only unit+browser tests are) and packaging regressions there go unnoticed until an actual release build.

## Architecture

### State: RootStore + sub-stores (MobX)

`renderer/states/RootStore.ts` is the single source of truth, injected via React context (`renderer/states/index.tsx`) and consumed as `useContext(StateContext)`. It's a composition root over focused sub-stores in `renderer/states/stores/`:

- `CanvasStore` — Konva `stage`/`layer` refs, canvas dimensions, background color. `setCanvasSize` proportionally rescales every element's `placement` when dimensions change (don't reintroduce absolute-pixel drift here — this was a real, previously-shipped bug).
- `MediaStore` — raw media resource URL lists (videos/images/audios) shown in the resource panels.
- `ElementStore` — the `editorElements` array (the canvas content model), selection, clipboard, and the `konvaNodes` map (element id → live Konva node instance).
- `AnimationStore` — the `animations` timeline array and animation-timeline math.
- `PlaybackStore` — play/pause, current time/keyframe, fps, seeking. `updateAudioElements`/`updateVideoElements` gate playback on whether the current time falls inside the element's `TimeFrame` — don't call `.play()`/reassign `.currentTime` unconditionally, or you'll reintroduce the "audio ignores its timeline position" bug.
- `ExportStore` — MP4/WebM export via `saveCanvasToVideoWithAudio`.
- `ProjectStore` — file path/handle, load/save (`serialize`/`deserialize`) for `.animathio` project files.
- `UIStore`, `AudioContextStore`, `KeyboardShortcutService` — selected side-panel option, the Web Audio `AudioContext`/source-node cache (one per audio element id — must be released via `releaseAudioContext` when an element is removed, or a reused id inherits a stale source node bound to a detached DOM node), and global keydown shortcut dispatch, respectively.

`RootStore` re-exposes every sub-store's state/actions as flat getters/setters/methods (e.g. `get stage()`, `addVideo()`, `updateAudioSettings()`) so the rest of the app — and most existing components — never talks to sub-stores directly. **Keep extending that flat forwarding surface** when you add sub-store state, rather than having components reach into `state.someSubStore.thing`, unless you're touching a call site anyway.

There is no undo/redo/history store. If you're asked to add one, it's new state, not an extraction from existing code.

### Canvas rendering: Konva (not fabric.js)

The canvas was migrated from fabric.js to Konva (`react-konva`) — `renderer/pages/Editor.tsx` renders per-type Konva node components (Text/Image/Video/Mafs element nodes), backed by shared helpers in `renderer/utils/konva-utils.ts`:

- `clampNodeToStage` — keeps a dragged node's bounding box inside canvas bounds (skips clamping an axis where the node itself exceeds the stage size on that axis).
- `getLineGuideStops` / `snapNodeToGuides` — the ~6px snap-to-edge/-center guide system (fabric's old guideline plugin has no Konva equivalent; this is a from-scratch reimplementation).
- `makeImageSceneFunc` — custom `sceneFunc` for image/video-backed nodes: draws a dashed placeholder (never throws) for a broken/unloaded/unready image or a video below `HAVE_CURRENT_DATA`, instead of letting a `drawImage()` exception abort the rest of that layer's synchronous draw pass (a genuinely broken element used to silently hide every element ordered after it on the same layer).

### Manim import

`renderer/utils/manim/` (`parser.ts` → `translator.ts`) parses a subset of Manim Community Python scene scripts (Text, Tex/MathTex, Circle, Rectangle, ImageMobject, Write/Create/FadeIn/FadeOut, `self.play`/`self.wait`) and translates them into `EditorElement`s + `Animation`s. This is a JS reimplementation, not a shelled-out Python/Manim process — there is no Python runtime dependency anywhere in this repo. The UI is `renderer/pages/components/panels/ManimImportPanel.tsx`. Tex/MathTex render via `renderer/utils/katex-render.ts` (KaTeX → rasterized image), not as literal LaTeX source text.

### Mafs (interactive math graphs)

`renderer/pages/components/partials/MafsModal.tsx` lets a user configure a Mafs graph (line/point/circle/polygon/plot/etc.) and rasterizes it to a PNG resource via `html-to-image`. Mafs's axis/grid/label colors are CSS custom properties (`--mafs-*`) declared in a stylesheet rule that `html-to-image`'s DOM clone doesn't carry over (it's nested inside a Tailwind v4 `@layer` block); `renderer/utils/mafs-capture.ts#applyMafsCaptureStyles` sets them as inline styles before capture instead of relying on the cascade. Both Manim text and Mafs graphs auto-contrast against the canvas's own background color via `renderer/utils/color.ts#getContrastColor`, independent of the app UI's own light/dark theme.

## Dependency policy

See `DEPENDENCIES.md` for the current list of majors that are deliberately deferred (and why) versus what's safe to bump. In short: don't jump Konva/Nextron/TypeScript/mobx/Vitest to their next major without reading that file first — each has a specific, currently-real incompatibility with this stack (Turbopack SSR, the build tool's package-manager assumption, Next's dependency verification, etc.), not just "hasn't been tried yet."

## Testing conventions

- `tests/renderer/**` (unit, jsdom): pure-logic tests for utils and store methods. Prefer extracting pure functions out of components for testability (see `mafs-capture.ts`, `color.ts`, `konva-utils.ts`) over testing through component internals.
- `tests/browser/**` (real headless Chromium via vitest browser mode): a mix of real component/DOM tests and lighter `isValidComponent`-style import smoke tests. Match whichever style the file you're extending already uses.
- `npm run test:browser` uses a Playwright-managed Chromium binary cached at `~/.cache/ms-playwright/`. If it fails at startup with a missing/corrupted browser binary, that's an environment issue (especially if working across multiple parallel git worktrees sharing the same cache path), not a code problem — don't chase it as a regression.
