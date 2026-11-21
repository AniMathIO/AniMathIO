#!/usr/bin/env bash
# Wine smoke test for the AniMathIO Windows installer (NSIS setup .exe).
#
# A stand-in for, not a replacement of, the hands-on Windows test: it installs
# the setup .exe silently into a throwaway Wine prefix, exercises the packaged
# CLI and editor, and prints a PASS/FAIL/SKIP line per check. Logs and
# screenshots stay in the work directory; the prefix is removed afterwards
# unless --keep.
#
#   scripts/wine-smoke.sh animathio_2.1.0_x64-setup.exe
#   scripts/wine-smoke.sh --keep --delay 45 animathio_2.1.0_x64-setup.exe
#
# Requires wine, Xvfb, ImageMagick (import/identify) and scripts/virtual-display.sh.
# Never touches ~/.wine: every run uses its own WINEPREFIX, and audio is off
# (ANIMATHIO_AUDIO=none, plus silent PipeWire/Pulse stand-ins inside the virtual
# display; the variable is exported for the CLI checks too).
#
# Exit status is non-zero when any check failed.
set -euo pipefail

usage() { sed -n '2,18p' "$0"; exit "${1:-0}"; }

delay=30 keep=0 fixture="" workdir="${WINE_SMOKE_WORKDIR:-}"
while [[ "${1:-}" == -* ]]; do
  case "$1" in
    --keep) keep=1; shift ;;
    --delay) delay="$2"; shift 2 ;;
    --fixture) fixture="$2"; shift 2 ;;
    --workdir) workdir="$2"; shift 2 ;;
    -h|--help) usage 0 ;;
    *) echo "wine-smoke: unknown option: $1" >&2; usage 2 ;;
  esac
done
[[ $# == 1 ]] || usage 2
setup_exe=$(readlink -f "$1")
[[ -f "$setup_exe" ]] || { echo "wine-smoke: not a file: $1" >&2; exit 2; }

WINE="${WINE:-wine}"
repo_root="$(cd "$(dirname "$0")/.." && pwd)"
vdisplay="$repo_root/scripts/virtual-display.sh"
[[ -x "$vdisplay" ]] || { echo "wine-smoke: $vdisplay not found" >&2; exit 2; }

workdir=${workdir:-$(mktemp -d "${TMPDIR:-/tmp}/animathio-wine-smoke.XXXXXX")}
mkdir -p "$workdir"
prefix="$workdir/prefix"
logs="$workdir/logs"
mkdir -p "$logs" "$prefix"

# A private win64 prefix, quiet Wine, no Mono/Gecko installer dialogs, no audio.
export WINEPREFIX="$prefix" WINEARCH=win64 WINEDEBUG=-all
export WINEDLLOVERRIDES="mscoree,mshtml="
export ANIMATHIO_AUDIO=none

declare -a table=()
fails=0

# record <PASS|FAIL|SKIP> <name> <note>
record() { table+=("$1 $2 — $3"); [[ "$1" != FAIL ]] || fails=$((fails + 1)); }

# check <name> <expected-file-or-""> <cmd...>: run a command, time it, log it.
# Never aborts the run: failures are recorded and the remaining checks still run.
check() {
  local name=$1 expect=$2; shift 2
  local t0=$SECONDS status log="$logs/$name.log"
  if "$@" >"$log" 2>&1; then
    status=0
  else
    status=$?
  fi
  if (( status )); then
    record FAIL "$name" "exit $status — $log"
  elif [[ -n "$expect" && ! -s "$expect" ]]; then
    record FAIL "$name" "exit 0 but output missing/empty: $expect"
  else
    record PASS "$name" "$((SECONDS - t0))s"
  fi
  return 0
}

finish() {
  echo
  printf '%s\n' "${table[@]:-(no checks recorded)}" | tee "$workdir/RESULTS.txt"
  echo
  if (( fails )); then echo "RESULT: $fails check(s) FAILED"; else echo "RESULT: all checks passed"; fi
  echo "artifacts and logs: $workdir"
  if (( ! keep )); then
    WINEPREFIX="$prefix" "${WINESERVER:-wineserver}" -k 2>/dev/null || true
    rm -rf "$prefix"
    echo "prefix removed (use --keep to keep it)"
  else
    echo "prefix kept: $prefix"
  fi
  if (( fails )); then exit 1; fi
  exit 0
}

echo "work dir:  $workdir"
echo "installer: $setup_exe"
echo

# --- 1. install -----------------------------------------------------------
echo "initialising Wine prefix ..."
t0=$SECONDS
if "$WINE" cmd /c echo ok >"$logs/prefix-init.log" 2>&1; then
  record PASS wine-prefix "$((SECONDS - t0))s"
else
  record FAIL wine-prefix "initialisation failed — $logs/prefix-init.log"
  finish
fi

# The installers are MSVC builds and need the VC++ runtime, which a fresh
# prefix has no real copy of (Wine ships only builtin substitutes for older
# versions). Install it once while the prefix is still throwaway.
if [[ -f "$prefix/drive_c/windows/system32/vcruntime140.dll" ]]; then
  record PASS vcruntime "already present in the prefix"
elif command -v winetricks >/dev/null; then
  echo "installing VC++ runtime (winetricks vcrun2022) ..."
  if winetricks -q vcrun2022 >"$logs/vcruntime.log" 2>&1; then
    record PASS vcruntime "winetricks vcrun2022"
  else
    record FAIL vcruntime "winetricks vcrun2022 failed — $logs/vcruntime.log"
  fi
else
  record SKIP vcruntime "winetricks not installed; MSVC exes may fail to start"
fi

echo "installing (NSIS /S) ..."
t0=$SECONDS
if "$WINE" "$setup_exe" /S >"$logs/install.log" 2>&1; then
  record PASS install "$((SECONDS - t0))s"
else
  record FAIL install "exit $? — $logs/install.log"
  finish
fi

app_exe=$(find "$prefix/drive_c" -name animathio.exe -print 2>/dev/null | head -1)
cli_exe=$(find "$prefix/drive_c" -name animathio-cli.exe -print 2>/dev/null | head -1)
if [[ -n "$app_exe" && -n "$cli_exe" ]]; then
  echo "installed: $app_exe"
else
  record FAIL install-layout "animathio.exe/animathio-cli.exe not found under drive_c"
  finish
fi
plugins_dir=$(dirname "$app_exe")/plugins
n_plugins=$(find "$plugins_dir" -name '*.amplugin' 2>/dev/null | wc -l)
if (( n_plugins > 0 )); then
  record PASS plugins-bundle "$plugins_dir ($n_plugins .amplugin)"
else
  record FAIL plugins-bundle "no .amplugin files next to animathio.exe"
fi

# --- 2. CLI checks (args reach the exe verbatim; no shell re-parsing) ------
cli() { (cd "$workdir" && "$WINE" "$cli_exe" "$@"); }

check cli-version "" cli --version
grep -q '^animathio-cli ' "$logs/cli-version.log" \
  && record PASS cli-version-output "$(tr -d '\r' <"$logs/cli-version.log")" \
  || record FAIL cli-version-output "unexpected --version output"

check new-sample "$workdir/sample.amio" cli new-sample -o sample.amio
check render-png "$workdir/frame.png" cli render sample.amio --frame 30 -o frame.png
if command -v identify >/dev/null && [[ -s "$workdir/frame.png" ]]; then
  identify "$workdir/frame.png" >>"$logs/render-png.log" 2>&1 || true
  record PASS render-png-image "$(identify -format '%wx%h %m' "$workdir/frame.png" 2>/dev/null || echo 'png unreadable by identify')"
else
  record SKIP render-png-image "frame.png missing or identify unavailable"
fi

check export-mp4 "$workdir/sample.mp4" cli export sample.amio -o sample.mp4
if [[ -s "$workdir/sample.mp4" ]]; then
  record PASS export-mp4-size "$(wc -c <"$workdir/sample.mp4") bytes"
else
  record FAIL export-mp4-size "sample.mp4 missing/empty"
fi

check plugins-list "" cli plugins list --json
grep -qE '\{|\[' "$logs/plugins-list.log" \
  && record PASS plugins-list-output "$(wc -c <"$logs/plugins-list.log") bytes of JSON" \
  || record FAIL plugins-list-output "no JSON on stdout"

check math-latex "$workdir/formula.amio" cli math-objects add sample.amio --kind latex --field 'source=e^{i\pi}+1=0' -o formula.amio
check math-latex-render "$workdir/formula.png" cli render formula.amio --frame 30 -o formula.png

if [[ -z "$fixture" ]]; then
  fixture=$(ls "$repo_root"/crates/import-v1/tests/fixtures/*.animathio 2>/dev/null | head -1)
fi
if [[ -n "$fixture" && -f "$fixture" ]]; then
  check import-v1 "$workdir/imported.amio" cli import-v1 "$fixture" -o imported.amio
else
  record SKIP import-v1 "no .animathio fixture found"
fi

# --- 3. editor screenshots (virtual display, silent audio) ----------------
sample_win=$("$WINE" winepath -w "$workdir/sample.amio" 2>/dev/null | head -1 | tr -d '\r')
check editor-launcher "$workdir/launcher.png" \
  "$vdisplay" --screenshot "$workdir/launcher.png" "$delay" \
  env WINEPREFIX="$prefix" WINEDEBUG=-all ANIMATHIO_AUDIO=none "$WINE" "$app_exe"
check editor-sample "$workdir/editor-sample.png" \
  "$vdisplay" --screenshot "$workdir/editor-sample.png" "$delay" \
  env WINEPREFIX="$prefix" WINEDEBUG=-all ANIMATHIO_AUDIO=none "$WINE" "$app_exe" "$sample_win"

# A known Wine gap, not an app bug: gpui's Windows text system resolves every
# font through IDWriteFontSet::GetMatchingFonts(family, ...), which Wine (11.x
# and master) does not implement (E_NOTIMPL), so the editor panics at startup
# even with the fonts installed. Real Windows is unaffected.
if grep -lq "failed to resolve font" "$logs"/editor-*.log 2>/dev/null; then
  record SKIP editor-wine-note \
    "known wine gap: IDWriteFontSet::GetMatchingFonts is E_NOTIMPL, gpui apps cannot resolve any font under wine (see logs/editor-*.log); not an app bug"
fi

finish
