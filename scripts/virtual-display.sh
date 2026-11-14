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
set -euo pipefail

shot="" delay=0
if [[ "${1:-}" == "--screenshot" ]]; then
  shot="$2"; delay="$3"; shift 3
fi

# Pick a free display number. Keep it away from the user's :0 and :1.
for n in $(seq 90 120); do
  [[ -e "/tmp/.X11-unix/X$n" || -e "/tmp/.X$n-lock" ]] || { disp=":$n"; break; }
done
: "${disp:?no free X display}"

Xvfb "$disp" -screen 0 1920x1080x24 -nolisten tcp >/dev/null 2>&1 &
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
  import -window root "$shot"
  kill "$app" 2>/dev/null || true
  wait "$app" 2>/dev/null || true
  echo "screenshot: $shot"
else
  "$@"
fi
