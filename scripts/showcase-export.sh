#!/usr/bin/env bash
# Website deliveries: intentionally opt-in, network and full exports never run in CI.
set -euo pipefail
cd "$(dirname "$0")/.."
output="${1:-target/showcase-website}"
export ANIMATHIO_AUDIO=none PIPEWIRE_REMOTE=animathio-silent-no-such-remote \
    PULSE_SERVER=unix:/nonexistent/animathio-silent SDL_AUDIODRIVER=dummy
export ANIMATHIO_PLUGIN_DIR="${ANIMATHIO_PLUGIN_DIR:-$PWD/target/plugins}"
cli="${ANIMATHIO_CLI:-$PWD/target/debug/animathio-cli}"
python3 examples/showcase/fetch-music.py
for name in pythagorean unit-circle fourier; do
    python3 "examples/showcase/$name/build.py" --require-music --output "$output/$name"
    python3 "examples/showcase/$name/build.py" --require-music --vertical --output "$output/$name"
    for aspect in 16x9 9x16; do
        "$cli" export "$output/$name/$name-$aspect.amio" \
            --output "$output/$name-$aspect.mp4" --overwrite
    done
done
