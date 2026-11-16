# AniMathIO 1.x versus 2.0 benchmarks

Measured on 2026-11-16 for the 2.0 release. These are Xvfb measurements on one
Linux machine, not native-desktop performance claims. Every timed metric uses
three runs; values are medians with the observed minimum–maximum.

## Machine and builds

AMD Ryzen 7 5800X (8 physical cores, 16 logical CPUs), 31.2 GiB usable RAM,
CachyOS, kernel 7.3.4-1-cachyos. Every GUI ran through
`scripts/virtual-display.sh` on a private 1920×1080×24 Xvfb display, with no
compositor and audio disabled. Vulkan/RADV can use the host GPU; Xvfb is not a
forced software-rendering configuration. The editor reported GPU preview.

v1 is 1.8.3 / Electron 44.9.1, checkout `147f522d4` (`origin/main`). Its existing
local unpacked build was reused. v2 is the release build at `b10489a4f`, with
all eight first-party plugin bundles built in this worktree. Subsequent commits
change only the harness/docs. Artifact SHA-256 hashes and all per-run readings
are retained in the result JSON alongside machine/build metadata.

Builds/checks finished before timed runs. Commands run at nice 19 / idle I/O
priority. Each scenario waits for the one-minute load average to be at most 4;
raw rows record load at completion. Compilation and untimed preflight timings
are excluded. The load gate refuses measurement on timeout.

## Results

<!-- results:BEGIN -->

| Metric | Unit | v1 (Electron/TypeScript) | v2 (Rust) |
| --- | --- | --- | --- |
| cold start to usable window | s | 0.81 (0.74–1.08) | 0.20 (0.20–5.20) |
| project load, 10 s scene open | s | 1.08 (1.07–1.13) | 0.21 (0.20–0.23) |
| idle memory, 10 s scene open (RSS sum median) | MiB | 923 (919–926) | 487 (485–526) |
| project load, 60-element project | s | 1.11 (1.07–1.21) | 0.20 (0.20–0.22) |
| idle memory, 60-element project (RSS sum median) | MiB | 931 (930–933) | 495 (494–495) |
| export, 10 s 1080p30 MP4 | s | 20.43 (20.38–20.52) | 6.47 (6.42–6.47) |
| peak memory during 10 s export (RSS sum max) | MiB | 1252 (1250–1286) | 128 (127–129) |
| exported MP4 size, 10 s project | KiB | 503 (478–534) | 343 |
| export, 60 s 1080p30 MP4 | s | 116.75 (116.09–116.75) | 37.42 (37.42–37.67) |
| peak memory during 60 s export (RSS sum max) | MiB | 1321 (1288–1335) | 129 (129–129) |
| exported MP4 size, 60 s project | KiB | 1533 (1531–1552) | 1762 |
| seek round-trip p50 | ms | 2.3 (2.3–2.3) | 24.1 (23.8–24.1) |
| seek round-trip p95 | ms | 6.1 (5.5–6.2) | 36.8 (36.7–37.7) |

## Sizes

Published v1 animathio-v1.8.3 download sizes: AniMathIO-1.8.3.AppImage 221 MiB, AniMathIO-1.8.3.flatpak 138 MiB, AniMathIO.Setup.1.8.3.exe 181 MiB, AniMathIO.Setup.1.8.3.exe.blockmap 0 MiB, animathio_1.8.3_amd64.deb 163 MiB, animathio_1.8.3_amd64.snap 188 MiB.
- v1 local artifacts (reused origin/main checkout build): .deb 166 MiB, AppImage 221 MiB, on disk (linux-unpacked) 658 MiB.
- v2 on disk (from-source install): editor 175 MiB + CLI 118 MiB + first-party plugin bundles 0.7 MiB = **293 MiB**.
- v2 reproducible gzip payload (editor + CLI + plugins, not an official installer): 90.4 MiB.

<!-- results:END -->

Raw evidence: [benchmark-results.json](benchmark-results.json). Completion load ranged
from 1.05 to 5.31; the gate applies before each scenario, not throughout it.
One v1 60-second attempt produced **60 fps / 3,600 frames** despite the source
requesting 30 fps. It is preserved with `valid: false` and excluded from the
1080p30 statistics. A replacement attempt passed validation, giving three valid
runs. Do not omit this exclusion when interpreting reproducibility.

## Methodology and limits

- **Startup:** fresh process to an automation reply: v1 MCP project state,
  v2 control `project.get`. This is a UI-loop readiness proxy, not a measurement
  of the first presented pixel. OS caches are retained; private benchmark homes
  are reused. v1 starts at its dashboard; v2 opens its sample and loads plugins.
  The first v2 launch includes plugin compilation, so keep its full range.
- **Project opening:** launch with the saved project until automation reports
  the expected element/clip count. Includes process startup. Does not isolate
  parsing time or prove that every image has painted.
- **Idle memory:** process-tree RSS sum, sampled at 4 Hz for eight seconds after
  a one-second settle. Each run contributes its sample median. Shared pages can
  be counted more than once for Electron. PSS is also retained in raw data.
- **Export memory:** sampled process-tree peak RSS, approximately 8 Hz. This can
  miss short spikes. v1 includes its whole GUI; v2 measures the shipped headless
  CLI export process, not an editor plus exporter.
- **Export:** wall time for the shipped MP4 path. v1 captures in real time and
  converts through ffmpeg.wasm (downloaded at runtime, warmed before timing).
  v2 renders offline with FilmCraft. These are workflow comparisons, not equal
  codec-quality comparisons. v1 uses CRF 24 / superfast; v2 defaults to CRF 23.
  v1 produces video only for this silent fixture; v2 also writes silent AAC.
  Validate codec, resolution, frame rate and duration with ffprobe. The v1
  real-time capture can yield an extra frame and variable output bytes.
- **Content:** both scenes contain the title “Fourier series”, the same Basel
  sum LaTeX formula, sine graph, circle and triangle, at 1920×1080 and 30 fps,
  for 10 or 60 seconds. v1 uses three elements (text, KaTeX raster, graph/shapes
  raster); v2 uses five live plugin clips. Formula metrics, title weight, grid
  density and exact layout differ. v2 fades clips over the first second; v1's
  scene is static. Frames were inspected before accepting measurements.
- **Extra scenarios:** the harness also supports a 60-element text/LaTeX
  project and automation seek round trips. Neither is a paint-latency test.
- **Sizes:** byte counts are deterministic artifact measurements, not timed
  samples. Local v1 packages and published downloads are distinct. v2's gzip
  payload contains editor, CLI and plugin bundles; it is not an official
  installer. Dependencies provided by the OS are excluded on both sides.
- **Shutdown:** Electron sometimes logs GPU-process failures during termination
  under Xvfb. Readiness and export results were collected before termination;
  these logs must not be represented as a clean native-desktop stability test.

## Reproduce

Prerequisites: stable Rust and wasm32-wasip2, the repository's native build
libraries, Python 3 with Pillow, Node/npm, Xvfb, ffprobe, and GitHub CLI for
published release metadata. The v1 source tree needs its npm dependencies.

```bash
scripts/build-plugins.sh
CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=line-tables-only nice -n 19 ionice -c3 cargo build --release --locked -p animathio -p animathio-cli
scripts/bench-v1-v2.sh --runs 3 --out "$PWD/bench/reproduction"
```

The entry point requires built v2 binaries/plugins. It reuses the v1 checkout at
`~/tmp/animathio-bench/v1`, or builds `origin/main` there when missing.
Use `--v1-src`, `--v1-dist`, `--v2-bin`, and `--plugins` to override locations.
Use a fresh output directory: existing measurement rows are rejected. `--no-build`
requires an existing v1 executable; `--skip-medium` and `--skip-seek` omit the
optional scenarios. GUI runs always use the virtual display; no audio is played.

`scripts/bench/fixtures.py` builds scenes; `scripts/bench/scenarios.py` runs them;
`scripts/bench/procs.py` samples memory. The v1 MCP driver and v2 control client
live alongside them. `scripts/bench/report.py --out DIR` aggregates saved rows.
The output directory contains raw JSON, `results.json`, `results.md`, logs,
fixtures and exported MP4s. On scenario failure the entry point still writes
partial reports and returns a nonzero status. Do not quote partial reports as
completed three-run comparisons. Generated projects, packages and MP4s stay
untracked; no v1 artifacts belong in this repository.
