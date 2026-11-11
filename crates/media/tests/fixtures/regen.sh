#!/usr/bin/env bash
# Regenerate the tiny committed media fixture.
# The committed clip must stay <= 200 KB so the repository does not grow.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
OUT="$HERE/sample.mp4"

nice -n 19 ffmpeg -y -hide_banner -loglevel error \
  -f lavfi -i "testsrc2=size=320x180:rate=30:duration=2" \
  -f lavfi -i "sine=frequency=440:sample_rate=48000:duration=2" \
  -vf "format=yuv420p" \
  -c:v libx264 -preset slow -crf 30 -g 15 \
  -c:a aac -b:a 64k \
  "$OUT"

size=$(stat -c%s "$OUT" 2>/dev/null || stat -f%z "$OUT")
echo "wrote $OUT ($size bytes)"
