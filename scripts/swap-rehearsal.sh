#!/usr/bin/env bash
# Rehearse the 2.0 swap (issue #156, "Repository and branch plan") in a throwaway
# local clone. It NEVER pushes to a real remote: the "remote" is a bare repository in
# a temp directory, and git is isolated from your global/system config.
#
#   scripts/swap-rehearsal.sh                       # origin/main + origin/v2 of this checkout
#   scripts/swap-rehearsal.sh --main main --v2 HEAD # other refs (any ref `git rev-parse` accepts)
#   scripts/swap-rehearsal.sh --keep                # keep the temp directory for inspection
#
# Steps performed (numbers match the plan in #156):
#   0. Build a throwaway origin from the chosen main/v2 refs (+ tags, staging,
#      release-please branches; dummies are created when they do not exist)
#   1. Cut the final v1 tag from main (animathio-v1.9.0) and create `legacy` from it
#   2. Land a simulated v1 hotfix on main (a file v2 deleted + one that both sides
#      changed), show that a plain merge would conflict, then
#      `git merge -s ours main` on v2
#   3. Retarget automation (workflow branch filters, Renovate) from v2 to main and
#      enable release-plz tagging/releases (the documented manual flips)
#   4. Merge v2 into main with a merge commit
#   5. Delete `staging` and `release-please--branches--*`
# Then it verifies the resulting `main`: README/LICENSE/AGENTS.md/CLAUDE.md/SECURITY.md/CONTRIBUTING.md and the issue/PR templates present,
# no v1 files or workflows left, workflows and Renovate scoped to `main`, release-plz
# enabled with exactly one tag-producing package, TOML/YAML still parse, the Cargo
# workspace resolves at 2.0.0, and the docs checker still passes.
# Exit status is non-zero when any check FAILs; WARN lines are manual follow-ups.
set -euo pipefail

src_root=$(cd "$(dirname "$0")/.." && pwd)
main_ref=origin/main v2_ref=origin/v2 v1_version=1.9.0 keep=0
while (($#)); do
  case "$1" in
    --main) main_ref=$2; shift 2 ;;
    --v2) v2_ref=$2; shift 2 ;;
    --v1-version) v1_version=$2; shift 2 ;;
    --keep) keep=1; shift ;;
    -h|--help) sed -n '2,27p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

failures=0 warnings=0
say()  { printf '\n== %s\n' "$*"; }
pass() { printf '  PASS  %s\n' "$*"; }
warn() { printf '  WARN  %s\n' "$*"; warnings=$((warnings + 1)); }
fail() { printf '  FAIL  %s\n' "$*"; failures=$((failures + 1)); }
check() { local what=$1; shift; if "$@" >/dev/null 2>&1; then pass "$what"; else fail "$what"; fi; }

tmp=$(mktemp -d "${TMPDIR:-/tmp}/animathio-swap.XXXXXX")
cleanup() { if ((keep)); then echo "kept $tmp"; else rm -rf "$tmp"; fi; }
trap cleanup EXIT

# Isolate git: no user hooks, signing, URL rewrites or credentials can reach a real remote.
export GIT_CONFIG_GLOBAL="$tmp/gitconfig" GIT_CONFIG_SYSTEM=/dev/null GIT_TERMINAL_PROMPT=0
git config --global user.name "Swap Rehearsal"
git config --global user.email "swap-rehearsal@example.invalid"
git config --global commit.gpgsign false
git config --global tag.gpgsign false
git config --global init.defaultBranch main
git config --global protocol.file.allow always

full_ref() { git -C "$src_root" rev-parse --symbolic-full-name "$1" 2>/dev/null | grep . || git -C "$src_root" rev-parse --verify "$1"; }

# --------------------------------------------------------------------------
say "0. Throwaway origin (nothing leaves $tmp)"
remote="$tmp/origin.git" work="$tmp/work"
git init -q --bare "$remote"
main_full=$(full_ref "$main_ref") v2_full=$(full_ref "$v2_ref")
fetch_specs=("+$main_full:refs/heads/main" "+$v2_full:refs/heads/v2" "+refs/tags/*:refs/tags/*")
# Branches the swap deletes, when the source has them.
while read -r ref; do
  [[ -n "$ref" ]] || continue
  fetch_specs+=("+$ref:refs/heads/${ref#refs/remotes/origin/}")
done < <(git -C "$src_root" for-each-ref --format='%(refname)' \
  refs/remotes/origin/staging 'refs/remotes/origin/release-please--branches--*')
git -C "$remote" fetch -q "$src_root" "${fetch_specs[@]}"
git clone -q "$remote" "$work"
cd "$work"
git remote -v | grep -q "$tmp" || { echo "refusing to continue: origin is not the throwaway remote" >&2; exit 3; }
git checkout -q v2
pass "origin = $remote ($(git -C "$remote" for-each-ref refs/heads | wc -l | tr -d ' ') branches, $(git -C "$remote" tag | wc -l | tr -d ' ') tags)"
v2_tree_before=$(git rev-parse HEAD^{tree})
echo "  main $(git rev-parse --short origin/main)  v2 $(git rev-parse --short HEAD)"

# --------------------------------------------------------------------------
say "1. Final v1 tag and protected legacy branch"
v1_tag="animathio-v$v1_version"
if git rev-parse -q --verify "refs/tags/$v1_tag" >/dev/null; then
  warn "tag $v1_tag already exists in the source; reusing it"
else
  git tag -a "$v1_tag" origin/main -m "AniMathIO $v1_version (final v1 release, rehearsal)"
  pass "tagged $v1_tag at main $(git rev-parse --short origin/main)"
fi
git branch legacy "$v1_tag^{commit}"
git push -q origin legacy "$v1_tag"
check "legacy branch exists on the throwaway origin" git -C "$remote" rev-parse --verify refs/heads/legacy
check "legacy has the v1 tree (package.json)" git cat-file -e legacy:package.json

# --------------------------------------------------------------------------
say "2. A v1 commit lands on main after v2 branched; merge -s ours"
git checkout -q -b main-simulate origin/main
[[ -f package.json ]] && echo "// simulated v1 hotfix" >> package.json || echo "simulated v1 hotfix" > v1-hotfix.txt
echo "simulated v1 hotfix" >> README.md
git add -A
git commit -q -m "fix: simulated v1 hotfix touching a v1-only file and README.md"
git branch -f main HEAD
git checkout -q v2
git push -q origin main
conflicts=$(git merge --no-commit --no-ff main 2>&1 | grep -c '^CONFLICT' || true)
git merge --abort 2>/dev/null || git reset -q --hard
if ((conflicts > 0)); then
  pass "control: a plain 'git merge main' would conflict in $conflicts file(s), as the plan expects"
else
  warn "control: a plain merge did not conflict; the rehearsal does not exercise the -s ours advantage"
fi
git merge -q -s ours main -m "chore: record v1 as superseded (2.0 swap)"
check "merge -s ours keeps the v2 tree byte-for-byte" test "$(git rev-parse HEAD^{tree})" = "$v2_tree_before"
check "main is now an ancestor of v2 (v1 history is in the log)" git merge-base --is-ancestor main HEAD
check "the v1 tag is reachable from v2" git merge-base --is-ancestor "$v1_tag" HEAD

# --------------------------------------------------------------------------
say "3. Retarget automation to main and enable release-plz"
# Branch filters in every workflow, plus Renovate.
for wf in .github/workflows/*.yml; do
  sed -i -E 's/^(\s*branches:\s*)\[v2\]/\1[main]/' "$wf"
done
sed -i -E 's/"baseBranchPatterns": \["v2"\]/"baseBranchPatterns": ["main"]/' renovate.json
# release-plz: flip the tag/release switches on the single canonical [[package]] only.
awk '
  /^\[\[package\]\]/ {pkg=1; print; next}
  /^\[/ {pkg=0}
  pkg && /^git_tag_enable *= *false/ {print "git_tag_enable = true"; next}
  pkg && /^git_release_enable *= *false/ {print "git_release_enable = true"; next}
  pkg && /^# git_only = true/ {print "git_only = true"; next}
  {print}
' release-plz.toml > release-plz.toml.new && mv release-plz.toml.new release-plz.toml
# release-plz workflow: uncomment the `release` job.
if grep -q '^  # release:$' .github/workflows/release-plz.yml; then
  awk '/^  # release:$/{on=1} on{sub(/^  # ?/, "  ")} {print}' .github/workflows/release-plz.yml > rp.new && mv rp.new .github/workflows/release-plz.yml
  sed -i -E 's/^  +$//' .github/workflows/release-plz.yml
else
  warn "release-plz.yml no longer has the commented '# release:' job the swap steps describe"
fi
git add -A
git commit -q -m "chore: retarget automation to main and enable releases (rehearsal)"
pass "committed the retarget/enable changes on v2"

# --------------------------------------------------------------------------
say "4. Merge v2 into main with a merge commit"
git checkout -q main
git merge -q --no-ff v2 -m "feat: AniMathIO 2.0 (rehearsal merge of v2)"
check "main now has the v2 tree" test -z "$(git diff --name-only main v2)"
check "the merge commit has two parents" test "$(git rev-list --parents -n 1 HEAD | wc -w)" = 3
git push -q origin main
check "the throwaway origin main equals local main" test "$(git -C "$remote" rev-parse refs/heads/main)" = "$(git rev-parse main)"

# --------------------------------------------------------------------------
say "5. Delete staging and release-please branches"
mapfile -t doomed < <(git -C "$remote" for-each-ref --format='%(refname:short)' refs/heads/staging 'refs/heads/release-please--branches--*')
if ((${#doomed[@]} == 0)); then
  git push -q origin "main:refs/heads/staging" "main:refs/heads/release-please--branches--main"
  doomed=(staging release-please--branches--main)
  warn "source had no staging/release-please branches; dummies were created and deleted"
fi
for branch in "${doomed[@]}"; do git push -q origin --delete "$branch"; done
check "staging is gone" test -z "$(git -C "$remote" for-each-ref refs/heads/staging)"
check "release-please branches are gone" test -z "$(git -C "$remote" for-each-ref 'refs/heads/release-please--branches--*')"
check "legacy survived the cleanup" git -C "$remote" rev-parse --verify refs/heads/legacy

# --------------------------------------------------------------------------
say "Verify main as 2.0"
git checkout -q main
for f in README.md LICENSE AGENTS.md CLAUDE.md SECURITY.md CONTRIBUTING.md .github/pull_request_template.md .github/ISSUE_TEMPLATE/bug_report.yml .github/ISSUE_TEMPLATE/feature_request.yml .github/ISSUE_TEMPLATE/config.yml Cargo.toml Cargo.lock release-plz.toml renovate.json deny.toml rust-toolchain.toml docs/architecture.md; do
  check "present: $f" test -f "$f"
done
for f in package.json .release-please-manifest.json release-please-config.json .github/dependabot.yml electron-builder.yml renderer; do
  check "v1 leftover absent: $f" test ! -e "$f"
done
if grep -lEi 'release-please|electron|npm ' .github/workflows/*.yml >/dev/null 2>&1; then
  fail "a workflow still mentions release-please/electron/npm: $(grep -lEi 'release-please|electron|npm ' .github/workflows/*.yml | tr '\n' ' ')"
else
  pass "no v1 workflow logic in .github/workflows"
fi
# Workflows run from main.
if grep -nE 'branches:\s*\[[^]]*\bv2\b' .github/workflows/*.yml >/dev/null; then
  fail "a workflow still filters on the v2 branch"
else
  pass "no workflow filters on v2"
fi
for wf in ci release-plz; do
  check "$wf.yml triggers on main" grep -qE 'branches:\s*\[main\]' ".github/workflows/$wf.yml"
done
check "renovate.json targets main" grep -q '"baseBranchPatterns": \["main"\]' renovate.json
check "release job present and uncommented in release-plz.yml" grep -qE '^  release:$' .github/workflows/release-plz.yml
check "release-plz runs the tagged package in git-only mode" grep -qE '^git_only *= *true' release-plz.toml
check "release-plz tags exactly one package" test "$(grep -cE '^git_tag_enable *= *true' release-plz.toml)" = 1
check "release-plz creates exactly one GitHub release" test "$(grep -cE '^git_release_enable *= *true' release-plz.toml)" = 1
check "the workspace default stays untagged" grep -qE '^git_tag_enable *= *false' release-plz.toml
check "no v2.0.0 tag exists yet (release-plz creates it)" test -z "$(git tag -l v2.0.0)"
check "v1 tags are not matched by the changelog v2 tag_pattern" test -z "$(git tag -l | grep -E '^v2\.' || true)"

if command -v python3 >/dev/null; then
  check "release-plz.toml parses" python3 -I -c 'import sys, tomllib; tomllib.load(open("release-plz.toml", "rb"))'
  check "renovate.json parses" python3 -I -c 'import json; json.load(open("renovate.json"))'
  if python3 -I -c 'import yaml' 2>/dev/null; then
    for wf in .github/workflows/*.yml; do
      check "$wf parses as YAML" python3 -I -c 'import sys, yaml; yaml.safe_load(open(sys.argv[1]))' "$wf"
    done
  else
    warn "python3 has no PyYAML: workflow YAML was not parsed (job structure checked with grep only)"
  fi
else
  warn "python3 not found: TOML/JSON/YAML parse checks skipped"
fi

version=$(nice -n 19 cargo metadata --no-deps --format-version 1 --offline 2>/dev/null | python3 -I -c 'import json,sys; print({p["version"] for p in json.load(sys.stdin)["packages"]})' 2>/dev/null || echo "?")
if [[ "$version" == "{'2.0.0'}" ]]; then pass "cargo workspace resolves; every package is at 2.0.0"; else warn "cargo metadata did not confirm 2.0.0 for every package (got $version)"; fi

if [[ -x scripts/check-docs.sh ]]; then
  if scripts/check-docs.sh >/dev/null 2>&1; then pass "scripts/check-docs.sh passes on main"; else fail "scripts/check-docs.sh fails on main"; fi
else
  warn "scripts/check-docs.sh is not in this v2 ref; docs were not re-checked"
fi

# Manual follow-ups the swap PR still has to carry (the script cannot know the new wording).
stale=$(grep -nE 'This is the `v2` branch|Pre-release, rewrite in progress|target \*\*`v2`\*\*|Never target `main`' README.md AGENTS.md | head -8 || true)
if [[ -n "$stale" ]]; then
  warn "docs still describe the pre-swap branch layout; rewrite in the swap PR:"
  sed 's/^/          /' <<<"$stale"
fi
if grep -nE 'baseBranchPatterns|`v2`' AGENTS.md | grep -q Renovate; then
  warn "AGENTS.md automation notes still say Renovate targets v2"
fi

# --------------------------------------------------------------------------
say "Bootstrap v2.0.0 and dry-run the first release PR"
cliff="${GIT_CLIFF:-$(command -v git-cliff || true)}" rplz="${RELEASE_PLZ:-$(command -v release-plz || true)}"
if [[ -z "$cliff" || -z "$rplz" ]]; then
  warn "git-cliff and release-plz not found (set GIT_CLIFF / RELEASE_PLZ or put them on PATH): changelog bootstrap skipped"
else
  # The baseline: CHANGELOG.md for 2.0.0 from the v2-only range, then the v2.0.0 tag.
  "$cliff" --config cliff.toml --tag v2.0.0 "$v1_tag..HEAD" -o CHANGELOG.md 2>/dev/null
  check "git-cliff wrote a 2.0.0 changelog with Added entries" grep -q '^### Added' CHANGELOG.md
  check "the 2.0.0 changelog has no chore/ci/docs noise" test -z "$(grep -ciE '^- .*(\*\((ci|deps|dev|spikes)\)\*|clippy)' CHANGELOG.md | grep -v '^0$' || true)"
  check "the 2.0.0 changelog does not leak v1 history (only the v2 range)" test "$(grep -c '^- ' CHANGELOG.md)" -lt 200
  echo "  2.0.0 changelog: $(grep -c '^- ' CHANGELOG.md) entries"
  git add CHANGELOG.md && git commit -q -m "docs: changelog for 2.0.0"
  git tag -a v2.0.0 -m "AniMathIO 2.0.0 (rehearsal)"
  # A follow-up feat + fix + noise, then what release-plz would put in the release PR.
  git checkout -q -b rehearsal-next
  echo "// rehearsal" >> apps/animathio-cli/src/main.rs
  git commit -q -am "feat(cli): rehearsal feature (#900)"
  echo "// rehearsal" >> apps/animathio-cli/src/main.rs
  git commit -q -am "fix(render): rehearsal fix (#901)"
  echo "// rehearsal" >> apps/animathio-cli/src/main.rs
  git commit -q -am "chore(deps): rehearsal noise (#902)"
  if nice -n 19 "$rplz" update --config release-plz.toml >"$tmp/release-plz.log" 2>&1; then
    check "release-plz proposes 2.0.0 -> 2.1.0 for a feat" grep -q '2.0.0 -> 2.1.0' "$tmp/release-plz.log"
    check "release PR changelog lists the feat and the fix" sh -c 'grep -qi "rehearsal feature" CHANGELOG.md && grep -qi "rehearsal fix" CHANGELOG.md'
    check "release PR changelog hides the chore" test -z "$(grep -i "rehearsal noise" CHANGELOG.md || true)"
    check "workspace version bumped to 2.1.0" grep -qE '^version = "2.1.0"' Cargo.toml
  else
    fail "release-plz update failed: $(tail -3 "$tmp/release-plz.log" | tr '\n' ' ')"
  fi
  git checkout -q -f main
fi

say "Result"
git log --oneline --graph --first-parent -n 6 main | sed 's/^/  /'
echo "  branches on the throwaway origin: $(git -C "$remote" for-each-ref --format='%(refname:short)' refs/heads | tr '\n' ' ')"
echo "  $failures failure(s), $warnings warning(s)"
((failures == 0))
