#!/usr/bin/env bash
# Build the first-party AniMathIO plugins for wasm32-wasip2 and package them
# as `.amplugin` bundles (a zip of plugin.toml + plugin.wasm).
set -euo pipefail

cd "$(dirname "$0")/.."

rustup target add wasm32-wasip2

mkdir -p target/plugins

build_plugin() {
    local name="$1"
    echo ">> building plugin: ${name}"
    cargo build \
        --manifest-path "plugins/${name}/Cargo.toml" \
        --target wasm32-wasip2 \
        --target-dir "$PWD/target" \
        --release \
        --locked

    python3 - "${name}" <<'PY'
import sys, zipfile

name = sys.argv[1]
wasm = f"target/wasm32-wasip2/release/{name}.wasm"
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
