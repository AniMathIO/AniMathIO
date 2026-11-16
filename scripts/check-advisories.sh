#!/usr/bin/env bash
# Audit every independent Cargo graph, including unshipped experiments.
set -euo pipefail
cd "$(dirname "$0")/.."
root=$PWD
export CARGO_BUILD_JOBS=3
export CARGO_PROFILE_DEV_DEBUG=${CARGO_PROFILE_DEV_DEBUG:-line-tables-only}
run_cargo=(nice -n 19 ionice -c3 cargo)
# Share the reviewed policy with cargo-audit, which has no reason-bearing ignores.
mapfile -t ignored < <(python3 - <<'PY'
import tomllib
with open('deny.toml', 'rb') as f:
    for entry in tomllib.load(f)['advisories']['ignore']:
        print(entry['id'])
PY
)
audit_args=()
for id in "${ignored[@]}"; do audit_args+=(--ignore "$id"); done
failed=0
while IFS= read -r lock; do
    echo ">> auditing $lock"
    "${run_cargo[@]}" deny --manifest-path "${lock%Cargo.lock}Cargo.toml" --config "$root/deny.toml" check advisories || failed=1
    "${run_cargo[@]}" audit --file "$lock" "${audit_args[@]}" || failed=1
done < <(git ls-files '*Cargo.lock')
exit "$failed"
