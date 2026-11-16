#!/usr/bin/env bash
# Keep the Cargo.lock files of the nested Cargo workspaces in sync with the
# root workspace.
#
# The nested workspaces that path-depend on workspace crates (`fuzz/`,
# `plugins/import-manim/`, ...) keep their own lockfiles. When release-plz
# bumps the workspace version on a release PR (or a dependency is added to a
# workspace crate), those lockfiles still record the old state and every
# `--locked` build of the nested workspace fails ("the lock file needs to be
# updated but --locked was passed"). This script runs `cargo update -w` in each
# affected nested workspace, which only rewrites the entries of the workspace
# crates themselves (their path-dependency closure): no registry dependency is
# bumped.
#
# Usage:
#   scripts/sync-sub-lockfiles.sh          # fix stale lockfiles (leave the changes in the tree)
#   scripts/sync-sub-lockfiles.sh --check  # verify only; exit 1 with a fix hint when stale
#
# Nested workspaces are found from the tracked Cargo.lock files (everything
# except the root one) and kept only when a manifest inside them path-depends
# on something outside the workspace directory (`path = "..`; the vendored
# `spikes/*/vendor` crates stay ignored). Workspaces without such a dependency
# cannot go stale, so running cargo there would be wasted work.
set -euo pipefail

cd "$(dirname "$0")/.."

check=0
for arg in "$@"; do
    case "$arg" in
        --check) check=1 ;;
        -h|--help) sed -n '2,25p' "$0"; exit 0 ;;
        *) echo "unknown argument: $arg (see --help)" >&2; exit 2 ;;
    esac
done

# Linux development machines are shared; keep cargo at background priority.
# Git Bash on Windows has no ionice, so retain the portable path there.
cargo_command=(cargo)
if [[ "$OSTYPE" == linux* ]]; then
    cargo_command=(nice -n 19 ionice -c3 cargo)
elif [[ "$OSTYPE" == darwin* ]]; then
    cargo_command=(nice -n 19 cargo)
fi

# Every tracked Cargo.lock below the repository root belongs to a nested workspace.
mapfile -t lockfiles < <(git ls-files | grep -E '(^|/)Cargo\.lock$' | grep -vx 'Cargo\.lock' || true)

candidates=()
for lock in "${lockfiles[@]}"; do
    dir="${lock%/Cargo.lock}"
    # Path deps on workspace crates must escape the nested workspace
    # directory, so they start with `../` in every in-repo layout.
    if git ls-files -- "$dir" | grep -E '(^|/)Cargo\.toml$' |
        xargs grep -lE 'path *= *"\.\.' >/dev/null 2>&1; then
        candidates+=("$dir")
    else
        echo ">> $dir: no path dependencies on workspace crates, skipping"
    fi
done

if (( ${#candidates[@]} == 0 )); then
    echo ">> no nested workspaces depend on workspace crates; nothing to do"
    exit 0
fi

# Snapshot actual working files, including uncommitted edits. Restore on check
# mode and on errors/signals, even if Cargo failed after writing a lockfile.
backup=$(mktemp -d)
restore=1
cleanup() {
    if ((restore)); then
        for dir in "${candidates[@]}"; do
            cp -- "$backup/$dir/Cargo.lock" "$dir/Cargo.lock"
        done
    fi
    rm -rf -- "$backup"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
for dir in "${candidates[@]}"; do
    mkdir -p -- "$backup/$dir"
    cp -- "$dir/Cargo.lock" "$backup/$dir/Cargo.lock"
done

stale=''
for dir in "${candidates[@]}"; do
    echo ">> $dir: cargo update --workspace"
    CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 \
        "${cargo_command[@]}" update --workspace --manifest-path "$dir/Cargo.toml"
    # Cargo -w preserves existing compatible dependencies, but can resolve new
    # ones. Fail closed rather than silently introduce registry/git versions.
    python3 - "$backup/$dir/Cargo.lock" "$dir/Cargo.lock" <<'PYTHON'
import sys
import tomllib

def external(path):
    with open(path, "rb") as stream:
        return {(p["name"], p["version"], p["source"], p.get("checksum"))
                for p in tomllib.load(stream)["package"] if "source" in p}

if external(sys.argv[2]) - external(sys.argv[1]):
    sys.exit("ERROR: sync would introduce external dependency versions; update dependencies explicitly first")
PYTHON
    if ! cmp -s -- "$backup/$dir/Cargo.lock" "$dir/Cargo.lock"; then
        stale+="$dir/Cargo.lock"$'\n'
    fi
done

if [[ -z "$stale" ]]; then
    echo ">> sub-workspace lockfiles are in sync"
    exit 0
fi

echo ">> stale sub-workspace lockfiles:"
while IFS= read -r lock; do
    echo ">>   $lock"
done <<<"$stale"

if ((check)); then
    echo "ERROR: sub-workspace Cargo.lock files are out of sync with the workspace version." >&2
    echo "ERROR: run scripts/sync-sub-lockfiles.sh and commit the updated lockfiles." >&2
    exit 1
fi

restore=0
echo ">> updated; commit the changed lockfiles"
