#!/usr/bin/env bash
# Generate the spike test corpus into spikes/media/corpus/.
# Frame counter burned in via drawtext so frame identity is checkable.
set -euo pipefail

HERE="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$HERE/corpus"
FONT="/usr/share/fonts/TTF/DejaVuSansMono-Bold.ttf"
mkdir -p "$OUT"

W=1280
H=720
DUR=5

# Drawtext: big frame number (n) plus the stream pts/timecode.
DRAW="drawtext=fontfile=$FONT:text='n=%{n}':fontcolor=white:fontsize=64:x=40:y=40:box=1:boxcolor=black@0.6:boxborderw=10,drawtext=fontfile=$FONT:text='pts=%{pts\:hms}':fontcolor=yellow:fontsize=40:x=40:y=130:box=1:boxcolor=black@0.6:boxborderw=10"

echo "== h264 30fps + aac =="
nice -n 19 ffmpeg -y -hide_banner -loglevel error \
  -f lavfi -i "testsrc2=size=${W}x${H}:rate=30:duration=$DUR" \
  -f lavfi -i "sine=frequency=440:sample_rate=48000:duration=$DUR" \
  -vf "$DRAW,format=yuv420p" \
  -c:v libx264 -preset medium -crf 23 -g 30 -c:a aac -b:a 128k \
  "$OUT/h264_30fps.mp4"

echo "== h264 29.97fps + aac =="
nice -n 19 ffmpeg -y -hide_banner -loglevel error \
  -f lavfi -i "testsrc2=size=${W}x${H}:rate=30000/1001:duration=$DUR" \
  -f lavfi -i "sine=frequency=550:sample_rate=48000:duration=$DUR" \
  -vf "$DRAW,format=yuv420p" \
  -c:v libx264 -preset medium -crf 23 -g 60 -c:a aac -b:a 128k \
  "$OUT/h264_2997.mp4"

echo "== hevc 30fps + aac =="
nice -n 19 ffmpeg -y -hide_banner -loglevel error \
  -f lavfi -i "testsrc2=size=${W}x${H}:rate=30:duration=$DUR" \
  -f lavfi -i "sine=frequency=660:sample_rate=48000:duration=$DUR" \
  -vf "$DRAW,format=yuv420p" \
  -c:v libx265 -preset medium -crf 28 -tag:v hvc1 -c:a aac -b:a 128k \
  "$OUT/hevc_30fps.mp4"

echo "== vp9 30fps webm + opus =="
nice -n 19 ffmpeg -y -hide_banner -loglevel error \
  -f lavfi -i "testsrc2=size=${W}x${H}:rate=30:duration=$DUR" \
  -f lavfi -i "sine=frequency=330:sample_rate=48000:duration=$DUR" \
  -vf "$DRAW,format=yuv420p" \
  -c:v libvpx-vp9 -crf 32 -b:v 0 -c:a libopus -b:a 96k \
  "$OUT/vp9_30fps.webm"

echo "== aac audio only =="
nice -n 19 ffmpeg -y -hide_banner -loglevel error \
  -f lavfi -i "sine=frequency=440:sample_rate=48000:duration=$DUR" \
  -af "volume=0.8" -c:a aac -b:a 128k \
  "$OUT/aac_48k.m4a"

echo "== opus audio only =="
nice -n 19 ffmpeg -y -hide_banner -loglevel error \
  -f lavfi -i "sine=frequency=440:sample_rate=48000:duration=$DUR" \
  -af "volume=0.8" -c:a libopus -b:a 96k \
  "$OUT/opus_48k.opus"

echo "== h264 variable frame rate =="
# Every 6th and 7th frame is delayed by ~1/30s, producing genuinely variable
# frame durations. -fps_mode vfr writes the real (uneven) durations.
nice -n 19 ffmpeg -y -hide_banner -loglevel error \
  -f lavfi -i "testsrc2=size=${W}x${H}:rate=30:duration=$DUR" \
  -vf "$DRAW,setpts='N/30/TB + 0.0333/TB*gte(mod(N\,6)\,4)',format=yuv420p" \
  -fps_mode vfr -c:v libx264 -preset medium -crf 23 -g 60 \
  "$OUT/h264_vfr.mp4"

echo "== done =="
ls -la "$OUT"
