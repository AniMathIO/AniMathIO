#!/usr/bin/env bash
# Run a command on a private virtual X display (Xvfb), so GUI tests,
# benches and screenshots never open windows on the user's desktop.
#
#   scripts/virtual-display.sh cargo run -p animathio --release -- --bench-viewport
#   scripts/virtual-display.sh --screenshot docs/screenshots/editor-main.png 8 \
#       ./target/release/animathio
#
# With --screenshot <out.png> <seconds>, the command runs for <seconds>,
# the virtual screen is captured to <out.png>, and then the command is stopped.
# The window only appears once the editor has started its renderer and plugins, and until
# then the root window is solid black, so a capture is retried (up to 60 s more) until the
# screen is no longer black. A frame that stays black is a failed run, not a screenshot.
#
# --crop WxH+X+Y keeps only that rectangle of the capture. At 1920x1080 a first-run window is
# the 90% restore size, so `--crop 1728x972+96+54` is the window without the black margin.
# --size WxH sets the virtual screen (default 1920x1080; ANIMATHIO_XVFB_SCREEN overrides),
# e.g. `--size 1280x720` for the smallest supported window:
#   scripts/virtual-display.sh --size 1280x720 --screenshot out.png 10 ./target/release/animathio
set -euo pipefail

shot="" delay=0 crop="" size="${ANIMATHIO_XVFB_SCREEN:-1920x1080x24}"
while [[ "${1:-}" == --* ]]; do
  case "$1" in
    --screenshot) shot="$2"; delay="$3"; shift 3 ;;
    --crop) crop="$2"; shift 2 ;;
    --size)
      size="$2"; [[ "$size" =~ ^[0-9]+x[0-9]+$ ]] && size="${size}x24"
      shift 2 ;;
    *) break ;;
  esac
done

# Pick a free display number. Keep it away from the user's :0 and :1.
for n in $(seq 90 120); do
  [[ -e "/tmp/.X11-unix/X$n" || -e "/tmp/.X$n-lock" ]] || { disp=":$n"; break; }
done
: "${disp:?no free X display}"

Xvfb "$disp" -screen 0 "$size" -nolisten tcp >/dev/null 2>&1 &
xvfb=$!
trap 'kill "$xvfb" 2>/dev/null || true' EXIT
for _ in $(seq 50); do [[ -e "/tmp/.X11-unix/X${disp#:}" ]] && break; sleep 0.1; done

# Force X11 inside the virtual display: never touch the user's Wayland session.
export DISPLAY="$disp"
unset WAYLAND_DISPLAY WAYLAND_SOCKET
export XDG_SESSION_TYPE=x11
# Keep agent GUI runs silent: the maintainer's speakers stay theirs. The editor
# skips the audio device (wall-clock playback) and, as a second guard, any library
# that still tries the sound server gets a server that doesn't exist.
export ANIMATHIO_AUDIO=none
export PIPEWIRE_REMOTE=animathio-silent-no-such-remote
export PULSE_SERVER=unix:/nonexistent/animathio-silent
export SDL_AUDIODRIVER=dummy

if [[ -n "$shot" ]]; then
  "$@" &
  app=$!
  sleep "$delay"
  mkdir -p "$(dirname "$shot")"
  # Park the pointer over the title bar so no card or button shows a hover state (a pointer
  # that leaves the window would keep the last hover).
  if command -v xdotool >/dev/null; then
    xdotool mousemove $(( ${size%%x*} * 3 / 4 )) 75 || true
    sleep 1
  fi
  status=1
  for _ in $(seq 61); do
    kill -0 "$app" 2>/dev/null || { echo "the command exited before the capture" >&2; break; }
    import -window root "$shot"
    # `mean` is 0 for a solid black frame (a window that has not painted yet).
    if [[ "$(identify -format '%[fx:mean>0.005?1:0]' "$shot")" == 1 ]]; then status=0; break; fi
    sleep 1
  done
  kill "$app" 2>/dev/null || true
  wait "$app" 2>/dev/null || true
  if [[ -n "$crop" && $status == 0 ]]; then convert "$shot" -crop "$crop" +repage "$shot"; fi
  if (( status )); then
    echo "screenshot FAILED: $shot stayed black (or the command exited)" >&2
    exit 1
  fi
  echo "screenshot: $shot"
else
  "$@"
fi
