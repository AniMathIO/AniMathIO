#!/usr/bin/env bash
# Benchmark AniMathIO v1 (Electron/TypeScript, origin/main) against v2 (this Rust
# rewrite) on this machine, and write machine-readable results:
#
#   scripts/bench-v1-v2.sh [--runs 3] [--out bench]
#
# Optional overrides (defaults make sense inside this repository):
#   --v1-dist DIR    built v1 unpacked app (defaults to building origin/main into
#                    /home/hunor/tmp/animathio-bench/v1 via a throwaway git worktree)
#   --v1-src DIR     v1 source tree with node_modules (the MCP SDK is loaded from it)
#   --v2-bin DIR     directory holding `animathio` and `animathio-cli` (default
#                    <repo>/target/release; build with scripts/build-plugins.sh and
#                    `cargo build --release` first)
#   --plugins DIR    v2 plugin bundles (default <repo>/target/plugins)
#   --runs N         timed runs per scenario; median and range are reported (default 3)
#   --out DIR        results directory (default bench/)
#   --max-load F     wait while the 1-minute load average exceeds F (default 4)
#   --max-wait MIN   give up waiting for low load after this many minutes (default 180)
#   --skip-seek      skip the seek-latency scenarios
#   --skip-medium    skip the 60-element "medium project" scenarios
#   --keep-going     keep leftover bench state on failure (debugging)
#   --no-build       never build v1 or the v2 deb; require existing artifacts
#
# All GUI work runs on a private Xvfb display via scripts/virtual-display.sh with
# the sound servers pointed at nonexistent sockets; nothing opens a window on the
# desktop and nothing plays audio. Every timed run is gated on a low load average
# and records the load it ran under. Results land in $OUT/raw/*.json and are
# aggregated into $OUT/results.json and $OUT/results.md.
#
# Methodology and caveats: docs/benchmarks.md.

set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"

RUNS=3
OUT="$REPO_ROOT/bench"
V2_BIN="$REPO_ROOT/target/release"
PLUGINS="$REPO_ROOT/target/plugins"
V1_DIST=""
V1_SRC=""
MAX_LOAD=4.0
MAX_WAIT_MIN=180
SKIP_SEEK=0
SKIP_MEDIUM=0
KEEP_GOING=0
INSIDE_XVFB=0
NO_BUILD=0
BENCH_BASE="${ANIMATHIO_BENCH_BASE:-/home/hunor/tmp/animathio-bench}"

usage() { sed -n '2,42p' "$0" | sed 's/^# \{0,1\}//'; exit 2; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --runs) RUNS="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --v1-dist) V1_DIST="$2"; shift 2 ;;
    --v1-src) V1_SRC="$2"; shift 2 ;;
    --v2-bin) V2_BIN="$2"; shift 2 ;;
    --plugins) PLUGINS="$2"; shift 2 ;;
    --max-load) MAX_LOAD="$2"; shift 2 ;;
    --max-wait) MAX_WAIT_MIN="$2"; shift 2 ;;
    --skip-seek) SKIP_SEEK=1; shift ;;
    --skip-medium) SKIP_MEDIUM=1; shift ;;
    --keep-going) KEEP_GOING=1; shift ;;
    --inside-xvfb) INSIDE_XVFB=1; shift ;;
    --no-build) NO_BUILD=1; shift ;;
    -h|--help) usage ;;
    *) echo "unknown option: $1" >&2; usage ;;
  esac
done

log() { printf '[bench %s] %s\n' "$(date +%H:%M:%S)" "$*"; }
die() { printf '[bench] ERROR: %s\n' "$*" >&2; exit 1; }

if [[ "$INSIDE_XVFB" != 1 ]]; then
  # ---------------------------------------------------------------- phase 1: builds, fixtures, sizes
  mkdir -p "$OUT/raw"
  RAW="$OUT/raw"

  [[ -x "$V2_BIN/animathio" && -x "$V2_BIN/animathio-cli" ]] || die "v2 binaries missing at $V2_BIN — run scripts/build-plugins.sh and cargo build --release"
  [[ -d "$PLUGINS" ]] || die "v2 plugins missing at $PLUGINS — run scripts/build-plugins.sh"
  command -v node >/dev/null || die "node is required"
  command -v Xvfb >/dev/null || die "Xvfb is required"
  command -v python3 >/dev/null || die "python3 is required"

  # v1: reuse an existing build, otherwise build origin/main in a throwaway worktree.
  V1_SRC="${V1_SRC:-$BENCH_BASE/v1}"
  V1_DIST="${V1_DIST:-$V1_SRC/dist/linux-unpacked}"
  if [[ ! -x "$V1_DIST/AniMathIO" ]]; then
    if [[ "$NO_BUILD" == 1 ]]; then
      die "v1 executable not found at $V1_DIST/AniMathIO and --no-build given"
    fi
    log "building v1 from origin/main into $V1_SRC (throwaway git worktree, ~15 min)"
    if [[ ! -d "$V1_SRC/.git" ]]; then
      git -C "$REPO_ROOT" worktree add --detach "$V1_SRC" origin/main
    fi
    (
      cd "$V1_SRC"
      # v1's postinstall uses `npm rebuild canvas --update-binary`; npm 12 removed
      # --update-binary, so its postinstall steps run individually here.
      CARGO_BUILD_JOBS=3 nice -n 19 ionice -c3 npm ci --ignore-scripts --no-audit --no-fund
      PATH="$PWD/node_modules/.bin:$PATH" nice -n 19 ionice -c3 electron-builder install-app-deps
      nice -n 19 ionice -c3 npm rebuild canvas
      PATH="$PWD/node_modules/.bin:$PATH" nice -n 19 ionice -c3 nextron build --no-pack
      PATH="$PWD/node_modules/.bin:$PATH" nice -n 19 ionice -c3 electron-builder --linux dir deb AppImage --config electron-builder.yml
    ) || die "v1 build failed"
  fi
  [[ -x "$V1_DIST/AniMathIO" ]] || die "v1 executable missing at $V1_DIST/AniMathIO"
  log "v1 build: $V1_DIST"

  # Machine facts.
  python3 - "$RAW/machine.json" <<'PY'
import json, platform, subprocess, sys
cpu = ""
for line in open("/proc/cpuinfo"):
    if line.startswith("model name"):
        cpu = line.split(":", 1)[1].strip()
        break
mem_kib = int(next(l.split()[1] for l in open("/proc/meminfo") if l.startswith("MemTotal")))
distro = ""
for line in open("/etc/os-release"):
    if line.startswith("PRETTY_NAME="):
        distro = line.split("=", 1)[1].strip().strip('"')
info = {
    "cpu": cpu,
    "cores": int(subprocess.check_output(["nproc"]).strip()),
    "memory_gib": round(mem_kib / 1024 / 1024, 1),
    "kernel": platform.release(),
    "os": "Linux",
    "distro": distro,
    "display": "Xvfb 1920x1080x24, software rendering, no compositor, no GPU acceleration",
    "audio": "none (sound servers pointed at nonexistent sockets)",
}
with open(sys.argv[1], "w") as fh:
    json.dump(info, fh, indent=1)
PY

  # Versions of both apps.
  V1_VERSION="$(node -p "require('$V1_SRC/package.json').version")"
  V1_ELECTRON="$(node -p "require('$V1_SRC/package.json').devDependencies.electron || require('$V1_SRC/package.json').dependencies.electron || 'unknown'")"
  V2_VERSION="$("$V2_BIN/animathio" --version | awk '{print $2}')"
  V2_COMMIT="$(git -C "$REPO_ROOT" rev-parse --short HEAD)"
  V1_COMMIT="$(git -C "$V1_SRC" rev-parse --short HEAD)"
  python3 - "$RAW/versions.json" <<PY
import json, sys
json.dump({
  "v1": {"version": "$V1_VERSION", "electron": "$V1_ELECTRON", "commit": "$V1_COMMIT"},
  "v2": {"version": "$V2_VERSION", "commit": "$V2_COMMIT"},
}, open(sys.argv[1], "w"), indent=1)
PY

  # Fixtures: graph raster, v1 seed + MCP step scripts, v2 .amio projects.
  log "generating fixtures (v1 seed/steps + v2 .amio projects via animathio-cli)"
  mkdir -p "$OUT/projects"
  ANIMATHIO_PLUGIN_DIR="$PLUGINS" python3 "$REPO_ROOT/scripts/bench/fixtures.py" build \
    --out "$OUT/fixtures" --cli "$V2_BIN/animathio-cli" --plugins "$PLUGINS" \
    --save-dir "$OUT/projects" >"$RAW/fixtures.json"
  log "fixtures: $(cat "$RAW/fixtures.json")"

  # On-disk sizes of the v2 install (binaries + plugin bundles).
  python3 - "$RAW/sizes.json" "$V2_BIN" "$PLUGINS" <<'PY'
import glob, json, subprocess, sys
_, sizes_path, v2bin, plugins = sys.argv
def du(path):
    r = subprocess.run(["du", "-sb", path], capture_output=True, text=True)
    return int(r.stdout.split()[0]) if r.returncode == 0 else 0
v2 = {
    "animathio_editor_bytes": du(f"{v2bin}/animathio"),
    "animathio_cli_bytes": du(f"{v2bin}/animathio-cli"),
    "plugins_bytes": du(plugins),
}
v2["on_disk_total_bytes"] = sum(v2.values())
json.dump({"v2": v2}, open(sizes_path, "w"), indent=1)
PY

  # Published v1 release asset sizes (the honest "download size"), best effort.
  (cd "$REPO_ROOT" && gh release view --repo AniMathIO/AniMathIO --json assets,tagName \
     --jq '{tag: .tagName, assets: [.assets[] | {name, size}]}' >"$RAW/v1_release_assets.json" 2>/dev/null) \
    || log "could not fetch v1 release assets (offline?) — recorded without them"

  log "handing over to a private Xvfb display for the timed scenarios"
  exec "$REPO_ROOT/scripts/virtual-display.sh" "$0" --inside-xvfb \
    --runs "$RUNS" --out "$OUT" --v1-dist "$V1_DIST" --v1-src "$V1_SRC" \
    --v2-bin "$V2_BIN" --plugins "$PLUGINS" --max-load "$MAX_LOAD" \
    --max-wait "$MAX_WAIT_MIN" \
    $( [[ "$SKIP_SEEK" == 1 ]] && echo --skip-seek ) \
    $( [[ "$SKIP_MEDIUM" == 1 ]] && echo --skip-medium ) \
    $( [[ "$KEEP_GOING" == 1 ]] && echo --keep-going )
fi

# ---------------------------------------------------------------- phase 2: under Xvfb
log "running timed scenarios (runs=$RUNS, out=$OUT)"

# The scenario runner does its own cleanup (it owns every process it starts).
ANIMATHIO_BENCH_SKIP_SEEK="$SKIP_SEEK" \
ANIMATHIO_BENCH_SKIP_MEDIUM="$SKIP_MEDIUM" \
ANIMATHIO_BENCH_KEEP_GOING="$KEEP_GOING" \
python3 "$REPO_ROOT/scripts/bench/scenarios.py" \
  --out "$OUT" --runs "$RUNS" \
  --v1-bin "$V1_DIST/AniMathIO" --v1-src "$V1_SRC" \
  --v2-bin "$V2_BIN/animathio" --v2-cli "$V2_BIN/animathio-cli" \
  --plugins "$PLUGINS" \
  --max-load "$MAX_LOAD" --max-wait-min "$MAX_WAIT_MIN"

log "aggregating results"
python3 "$REPO_ROOT/scripts/bench/report.py" --out "$OUT"
log "done: $OUT/results.json and $OUT/results.md"
