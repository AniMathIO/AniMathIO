#!/usr/bin/env bash
# Build the first-party AniMathIO plugins for wasm32-wasip2 and package them
# as `.amplugin` bundles (a zip of plugin.toml + plugin.wasm).
set -euo pipefail

cd "$(dirname "$0")/.."

rustup target add wasm32-wasip2

mkdir -p target/plugins

# Linux development machines are shared; keep builds at background priority.
# Git Bash on Windows has no ionice, so retain the portable build path there.
cargo_command=(cargo)
if [[ "$OSTYPE" == linux* ]]; then
    cargo_command=(nice -n 19 ionice -c3 cargo)
elif [[ "$OSTYPE" == darwin* ]]; then
    cargo_command=(nice -n 19 cargo)
fi
python_command=python3
if ! python3 --version >/dev/null 2>&1; then
    python_command=python
fi

build_plugin() {
    local name="$1"
    echo ">> building plugin: ${name}"
    CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG="${CARGO_PROFILE_DEV_DEBUG:-line-tables-only}" "${cargo_command[@]}" build \
        --manifest-path "plugins/${name}/Cargo.toml" \
        --target wasm32-wasip2 \
        --target-dir "$PWD/target" \
        --release \
        --locked

    "$python_command" - "${name}" <<'PY'
import sys, zipfile

name = sys.argv[1]
# Cargo names the artifact after the library, which swaps hyphens for underscores.
wasm = f"target/wasm32-wasip2/release/{name.replace('-', '_')}.wasm"
out = f"target/plugins/{name}.amplugin"
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
    z.write(f"plugins/{name}/plugin.toml", "plugin.toml")
    z.write(wasm, "plugin.wasm")
print(f"   wrote {out}")
PY
}

build_plugin shapes
build_plugin text
build_plugin latex
build_plugin graph
build_plugin effects-basic
build_plugin audio-basic
build_plugin animations
build_plugin import-manim

# Compile the documented SDK example too. It is not a shipped plugin bundle, so
# packaging still contains exactly the eight first-party plugins above.
CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG="${CARGO_PROFILE_DEV_DEBUG:-line-tables-only}" "${cargo_command[@]}" build \
    --manifest-path plugins/examples/hello/Cargo.toml \
    --target wasm32-wasip2 \
    --target-dir "$PWD/target" \
    --release --locked
