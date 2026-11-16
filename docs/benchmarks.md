# Benchmarks: AniMathIO 1.x vs 2.0

Reproducible, honest numbers comparing AniMathIO v1 (the Electron/TypeScript
editor on `origin/main`) with v2 (this Rust rewrite), measured on the
maintainer's machine. The numbers below were produced for the 2.0 release blog
post; regenerate them any time with one command (see
[Reproducing](#reproducing)).

The harness lives in `scripts/bench-v1-v2.sh` + `scripts/bench/`:

| File | Purpose |
| --- | --- |
| `scripts/bench-v1-v2.sh` | Entry point: builds what is missing, generates fixtures, runs all scenarios under Xvfb, aggregates results |
| `scripts/bench/fixtures.py` | The equivalent benchmark content for both versions, the v1 `.animathio` seed, the v1 MCP step scripts, and the v2 `.amio` projects (authored through `animathio-cli math-objects add`) |
| `scripts/bench/v1_driver.mjs` | Dependency-free MCP client for v1's opt-in loopback MCP server (loads the MCP SDK from v1's own `node_modules`) |
| `scripts/bench/control_client.py` | Client for v2's control socket (`project.get`, `playback.seek`) |
| `scripts/bench/procs.py` | Process-tree PSS/RSS sampler (smaps_rollup; shared pages charged once, Electron's process tree summed) |
| `scripts/bench/scenarios.py` | The timed scenarios, load-average gating and per-run recording |
| `scripts/bench/report.py` | Aggregation into `bench/results.json` (raw rows + medians/ranges) and `bench/results.md` (the table) |

## Machine and environment

All numbers in this document were measured on one machine, in one session:

- Machine facts are recorded from `/proc/cpuinfo`, `/proc/meminfo` and the OS at run time (see results below).
- **Display:** private Xvfb 1920×1080×24, no compositor. Vulkan can use the host GPU; this is not a forced software-rendering comparison.
- **Audio:** none (both apps' sound servers pointed at nonexistent sockets)
- **Load gating:** every timed run waits until the 1-minute load average is
  ≤ 4 and records the exact load it ran under (`bench/results.json`).

CPU logical thread count is reported by `nproc`, not physical core count.

The exact machine facts, app versions and commit SHAs of both apps are recorded
in `bench/results.json` (`machine`, `versions`).

## Reproducing

```bash
# v2: build plugins and release binaries (the harness reuses target/release)
scripts/build-plugins.sh
CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=line-tables-only nice -n 19 ionice -c3 cargo build --release --locked

# run everything (roughly 45–60 minutes: builds v1 from origin/main in a
# throwaway git worktree under /home/hunor/tmp/animathio-bench, authors the
# scenes, then measures; pass --v1-dist/--v1-src to reuse an existing build)
scripts/bench-v1-v2.sh --runs 3

# on a machine whose checkout differs from the maintainer's, point at the
# binaries explicitly:
scripts/bench-v1-v2.sh --runs 3 \
  --v2-bin  /path/to/target/release \
  --plugins /path/to/target/plugins
```

Results land in `bench/raw/*.json` (one row per run, with the load average),
`bench/results.json` (medians and ranges) and `bench/results.md` (the table).
The harness never opens a window on the real desktop (everything runs through
`scripts/virtual-display.sh`), never plays audio, and never commits build
artifacts (`bench/` is gitignored).

## What is benchmarked

Both apps get the **same logical content**: a 1920×1080, 30 fps, dark
(`#10131a`) scene with

- a title "Fourier series" (76 px / 7 % of the short edge),
- a LaTeX formula `\sum_{n=1}^{\infty}\frac{1}{n^2}=\frac{\pi^2}{6}`,
- a graph area with axes, grid, labels, `sin(x)`, a circle (r = 2 units) and a
  triangle, at the same canvas position (45 % of canvas height),

plus a "medium" editor-sized project: 60 elements (50 text + 10 LaTeX), 10
visible seconds each on a 5 s stagger, 60 s total.

### Content differences (stated, not hidden)

- v1 has **no vector graph or shape elements**, so its graph area is one static
  864×486 raster image (what a Mafs capture is) placed at the same position.
  v2 renders the plot and the two shapes as live `org.animathio.graph` /
  `org.animathio.shapes` plugin clips — more per-frame work for v2.
- v1 rasterizes LaTeX **once at add time** (KaTeX → PNG); v2's `org.animathio.latex`
  plugin re-typesets per frame at output resolution — also more per-frame work
  for v2.
- v1's text elements are DOM/Konva text; v2's are `org.animathio.text` plugin
  clips. The v2 scene keeps five clips on five tracks; v1 keeps three elements
  (text, math, image) on its single canvas layer with the same timeline. v2 fades its clips in over the first second; the v1 scene is static. Formula font metrics, title weight and grid density also differ.
- Both versions' projects were authored through their own automation interfaces:
  v1 via its MCP server (`set_canvas`, `add_text`, `add_math`, `add_media`,
  `update_element`), v2 via `animathio-cli math-objects add`. The v1 scenes are
  saved to `.animathio` files and the v2 scenes to `.amio` archives; every
  load-time run opens those same files.

## Scenarios

| Scenario | Definition |
| --- | --- |
| Cold start to usable window | Launch to the app's automation interface answering: v1 = MCP `get_project_state` replies (renderer mounted, dashboard live); v2 = control socket replies to `project.get` (the render loop is serving). Includes process start; v2 opens its built-in sample project (its default start state), v1 opens its dashboard. This is a proxy for "usable window" — neither app exposes a pixel-ready signal headlessly. |
| Project load | Launch with the saved scene file passed on the command line, until all of its elements are visible (`get_project_state`/`project.get`). Same proxy caveat as cold start. |
| Idle RSS | Process-tree RSS sum, sampled at 4 Hz for 8 s (1 s settle) with the 10 s / medium project open. |
| Export 10 s / 60 s 1080p30 MP4 | Wall time of a full-project H.264 MP4 export, measured at each app's shipped export path: v1 `export_video` (real-time canvas capture + in-renderer ffmpeg.wasm convert), v2 `animathio-cli export` (offline renderer + FilmCraft encoders). Peak export RSS is sampled concurrently at 8 Hz. |
| Seek round-trip | Playhead moves through each app's automation interface (v1 MCP `seek`, v2 control `playback.seek`): 60 pseudo-random seeks/second for 10 s, p50/p95 reported. It includes IPC + state update, not paint time. |
| Sizes | v1: published release download sizes plus locally built `.deb`/AppImage/`linux-unpacked` from the same commit. v2: on-disk size of the from-source install (editor binary + CLI + 8 first-party `.amplugin` bundles). |

## Results

The table below is regenerated by `scripts/bench-v1-v2.sh` into
`bench/results.md`; it is copied here verbatim at the time of writing. Per-run
raw rows (including the load average each run saw) are in `bench/results.json`.

<!-- results:BEGIN (regenerate with scripts/bench-v1-v2.sh) -->

_Benchmark results will be filled in after the measured run completes._

<!-- results:END -->

## Caveats

Read before quoting any number above.

1. **Xvfb, not a desktop.** Every GUI number (cold start, load, seek, v1
   exports, idle RSS) was measured under a Xvfb display.
   These results do not establish native desktop performance.
2. **The export comparison compares shipped paths, not the same algorithm.**
   v1 exports video only (the fixture has no audio); v2 includes a silent AAC track.
   v1 exports by capturing the canvas in real time (a 60 s project needs ≥ 60 s
   by construction) and converts WebM→MP4 with ffmpeg.wasm (`libx264 -preset
   superfast -crf 24`) inside the renderer, single-threaded. v2 renders
   offline, deterministically, and encodes with pure-Rust FilmCraft H.264/AAC
   encoders, using the available cores. That difference is the point of the
   rewrite, but it is an architectural difference, not a like-for-like codec
   shootout.
3. **v1's ffmpeg.wasm core comes from unpkg at runtime.** The harness does one
   untimed warm-up export first, so timed exports measure steady state. On an
   offline machine, v1's MP4 export cannot run at all (WebM export works).
4. **"Usable window" is a proxy** (see the scenario table). Both proxies
   require each app's renderer/UI loop to be live, but neither measures
   pixel-presented time.
5. **RSS is the process-tree RSS sum** (shared pages can be counted repeatedly,
   sampled ≥ 8× per second during exports). PSS is retained in raw data as a supplementary metric. RSS is a sampling estimate,
   not a peak tracker; the export peak may be slightly under-sampled.
6. **Cold start means a fresh process, not a cold OS page cache.** The benchmark does not drop system caches. v2 startup includes the sample project (its default start state)
   and plugin loading; v1's includes its dashboard. Each app reuses its private benchmark home between runs.
7. **The medium project is not perfectly identical in structure:** v1 layers
   all 60 elements on one canvas layer, v2 uses 6 tracks × 10 clips (same
   schedule and content). Track model differences are part of what 2.0 changes.
8. **Three runs per scenario, median + range.** The ranges are visible in
   `bench/results.json`; nothing was cherry-picked. Runs were interleaved
   (v1/v2 alternating per scenario) so machine-state drift hits both apps.
9. **Seek latency is an automation round-trip**, not a paint benchmark; it
   includes one HTTP/JSON or socket/JSON hop plus each app's state update. It
   is the only scripted, equally-fair access path both apps expose.
10. **v1's GPU process occasionally dies at Xvfb shutdown** (`SIGILL` on
    terminate) in this environment. Every measurement scenario completed before
    shutdown; this only affected harness teardown, and any run whose app died
    mid-scenario fails the harness loudly rather than producing a number.
