#!/usr/bin/env bash
# Fail when the documentation names something that does not exist in the code.
#
#   scripts/check-docs.sh                    # source-only checks (+ CLI checks if a binary is found)
#   scripts/check-docs.sh --require-binary   # CI: also fail when animathio-cli is not built
#   scripts/check-docs.sh --fix-cli          # regenerate the `--help` blocks in docs/cli.md
#
# Checked against the tree (docs: README.md, AGENTS.md, docs/*.md):
#   * repo paths (`crates/..`, `scripts/..`, `docs/screenshots/..`) and relative
#     markdown links, including `#anchors`
#   * `animathio-cli` subcommands and flags, plus the `--help` blocks of docs/cli.md
#     (needs the built binary: $ANIMATHIO_CLI, target/debug or target/release)
#   * MCP tools (apps/animathio-cli/src/mcp.rs) and control methods
#     (crates/control/src/method.rs): every one documented, none invented
#   * `ANIMATHIO_*` environment variables, `cargo -p/--test/--example` targets,
#     `org.animathio.*` plugin ids, `Command::X` variants, WIT item names and
#     Rust symbols named in plugins.md/architecture.md
#
# Needs only bash, grep, sed and awk.
set -euo pipefail
cd "$(dirname "$0")/.."

require_binary=0 fix_cli=0
for arg in "$@"; do
  case "$arg" in
    --require-binary) require_binary=1 ;;
    --fix-cli) fix_cli=1 ;;
    -h|--help) sed -n '2,19p' "$0"; exit 0 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

DOCS=(README.md AGENTS.md docs/*.md)
shopt -s lastpipe
errors=0
fail() { echo "docs check: $*" >&2; errors=$((errors + 1)); }
note() { echo "docs check: note: $*" >&2; }
# Turn each stdin line into a failure (needs lastpipe so the counter survives).
report() { local l; while IFS= read -r l; do fail "$l"; done; }

# Backticked spans of a file as `line<TAB>token`.
spans() { grep -noE '`[^`]+`' "$1" | sed -E 's/^([0-9]+):`(.*)`$/\1\t\2/'; }
# Lowercased GitHub-style heading slugs of a markdown file.
slugs() {
  grep -E '^#{1,6} ' "$1" | sed -E 's/^#+ +//; s/`//g' | tr 'A-Z' 'a-z' \
    | sed -E 's/[^a-z0-9 _-]//g; s/ /-/g'
}
# Items of a newline list that are not in another newline list.
missing_from() { comm -23 <(sort -u <<<"$1") <(sort -u <<<"$2"); }

# --------------------------------------------------------------------------
# Paths and links
# --------------------------------------------------------------------------
root_files='README\.md|AGENTS\.md|CLAUDE\.md|LICENSE|release-plz\.toml|renovate\.json|deny\.toml|rust-toolchain\.toml|Cargo\.toml|Cargo\.lock|cliff\.toml'
path_re="((crates|apps|docs|scripts|plugins|assets|spikes|\\.github)/[A-Za-z0-9_./-]*[A-Za-z0-9_]|($root_files))"
for f in "${DOCS[@]}"; do
  # Paths named anywhere in the text (not preceded by a path character, so
  # URLs and `../../target/..` are skipped).
  grep -noE "(^|[^A-Za-z0-9_./-])$path_re" "$f" | sed -E 's/^([0-9]+):[^A-Za-z0-9_.\/-]?/\1:/' |
  while IFS=: read -r line path; do
    case "$path" in
      */target/*) continue ;;   # build outputs
      .github/dependabot.yml) continue ;;   # documented as living on main/staging only
    esac
    [[ -e "$path" ]] || echo "$f:$line: path does not exist: $path"
  done
  # Relative markdown links and their anchors.
  grep -noE '\]\([^)#][^)]*\)|\]\(#[^)]*\)' "$f" | sed -E 's/^([0-9]+):\]\((.*)\)$/\1\t\2/' |
  while IFS=$'\t' read -r line target; do
    case "$target" in http://*|https://*|mailto:*) continue ;; esac
    file="${target%%#*}" anchor=""
    [[ "$target" == *'#'* ]] && anchor="${target#*#}"
    if [[ -z "$file" ]]; then dest="$f"; else dest="$(dirname "$f")/$file"; fi
    if [[ ! -e "$dest" ]]; then echo "$f:$line: broken link: $target"; continue; fi
    if [[ -n "$anchor" && "$dest" == *.md ]] && ! slugs "$dest" | grep -qx -- "$anchor"; then
      echo "$f:$line: missing anchor #$anchor in $dest"
    fi
  done
done | report || true

# Screenshots on disk that no document shows are stale leftovers.
for shot in docs/screenshots/*; do
  grep -q -- "$shot" "${DOCS[@]}" || fail "unreferenced screenshot: $shot"
done

# --------------------------------------------------------------------------
# CLI subcommands, flags and `--help` blocks
# --------------------------------------------------------------------------
cli="${ANIMATHIO_CLI:-}"
if [[ -z "$cli" ]]; then
  for candidate in target/debug/animathio-cli target/release/animathio-cli; do
    [[ -x "$candidate" ]] && { cli="$candidate"; break; }
  done
fi

# Subcommands from the source (kebab-cased variants of `enum Action`).
src_subcommands=$(awk '/^enum Action \{/{on=1; next} on && /^}/{on=0} on && /^    [A-Z][A-Za-z0-9]*[ ({,]/{print $1}' \
  apps/animathio-cli/src/main.rs | sed -E 's/[({,]//g; s/([a-z0-9])([A-Z])/\1-\2/g' | tr 'A-Z' 'a-z')
[[ -n "$src_subcommands" ]] || fail "could not read the subcommand list from apps/animathio-cli/src/main.rs"

if [[ -n "$cli" ]]; then
  subcommands=$("$cli" --help | awk '/^Commands:/{on=1; next} on && /^$/{on=0} on{print $1}' | grep -vx help)
  [[ "$(sort <<<"$subcommands")" == "$(sort <<<"$src_subcommands")" ]] ||
    fail "binary subcommands ($(tr '\n' ' ' <<<"$subcommands")) differ from main.rs ($(tr '\n' ' ' <<<"$src_subcommands")); rebuild animathio-cli"
else
  subcommands="$src_subcommands"
  if ((require_binary)); then
    fail "animathio-cli is not built (set ANIMATHIO_CLI or run cargo build -p animathio-cli)"
  else
    note "animathio-cli not built: checking subcommand names against the source only"
  fi
fi

# Every `animathio-cli <word>` in the docs must be a real subcommand.
for f in "${DOCS[@]}"; do
  grep -noE 'animathio-cli +[a-z][a-z0-9-]*' "$f" | while IFS=: read -r line match; do
    word="${match##* }"
    grep -qx -- "$word" <<<"$subcommands" || echo "$f:$line: unknown animathio-cli subcommand: $word"
  done
done | report || true

# docs/cli.md documents every subcommand under its own heading.
for sub in $subcommands; do
  grep -qx "## \`$sub\`" docs/cli.md || fail "docs/cli.md has no '## \`$sub\`' section"
done

if [[ -n "$cli" ]]; then
  # `--help` text of one command ("" is the top level), as a fenced block.
  help_block() {
    printf '%s\n' '```text'
    if [[ -z "$1" ]]; then "$cli" --help; else "$cli" "$1" --help; fi
    printf '%s\n' '```'
  }
  if ((fix_cli)); then
    tmp=$(mktemp)
    awk -v cli="$cli" '
      /^<!-- help:[a-z0-9-]* -->$/ {
        name = $0; sub(/^<!-- help:/, "", name); sub(/ -->$/, "", name)
        print; print "```text"
        cmd = (name == "cli") ? cli " --help" : cli " " name " --help"
        while ((cmd | getline l) > 0) print l
        close(cmd); print "```"; skip = 1; next
      }
      /^<!-- \/help:[a-z0-9-]* -->$/ { skip = 0 }
      !skip { print }
    ' docs/cli.md > "$tmp"
    mv "$tmp" docs/cli.md
    echo "docs/cli.md help blocks regenerated"
  fi
  for sub in cli $subcommands; do
    marker="help:$sub"
    if ! grep -qx "<!-- $marker -->" docs/cli.md; then
      fail "docs/cli.md is missing a '<!-- $marker -->' block (run scripts/check-docs.sh --fix-cli after adding markers)"
      continue
    fi
    want=$(if [[ $sub == cli ]]; then help_block ""; else help_block "$sub"; fi)
    have=$(awk -v m="<!-- $marker -->" -v e="<!-- /$marker -->" '$0==m{on=1; next} $0==e{on=0} on' docs/cli.md)
    [[ "$want" == "$have" ]] || fail "docs/cli.md '$marker' block differs from 'animathio-cli ${sub/cli/} --help' (run scripts/check-docs.sh --fix-cli)"
  done

  # Every `--flag` on an `animathio-cli <sub>` command line must exist for that subcommand.
  for f in "${DOCS[@]}"; do
    grep -nE 'animathio-cli +[a-z][a-z0-9-]*.* --[a-z]' "$f" | while IFS= read -r hit; do
      line="${hit%%:*}" text="${hit#*:}"
      sub=$(sed -E 's/.*animathio-cli +([a-z][a-z0-9-]*).*/\1/' <<<"$text")
      grep -qx -- "$sub" <<<"$subcommands" || continue
      help=$("$cli" "$sub" --help)
      for flag in $(sed -E 's/.*animathio-cli +[a-z][a-z0-9-]*//' <<<"$text" | grep -oE -- '--[a-z][a-z0-9-]*'); do
        grep -q -- "$flag" <<<"$help" || echo "$f:$line: 'animathio-cli $sub' has no $flag flag"
      done
    done
  done | report || true
fi

# --------------------------------------------------------------------------
# MCP tools (docs/mcp.md <-> apps/animathio-cli/src/mcp.rs)
# --------------------------------------------------------------------------
src_tools=$(awk '/^    #\[tool\(/{t=1} t && /async fn [a-z_0-9]+/{match($0,/async fn [a-z_0-9]+/); print substr($0,RSTART+9,RLENGTH-9); t=0}' \
  apps/animathio-cli/src/mcp.rs)
[[ -n "$src_tools" ]] || fail "could not read the tool list from apps/animathio-cli/src/mcp.rs"
# First column of the `### Tools` table.
doc_tools=$(awk '/^### Tools$/{on=1; next} /^#/{on=0} on && /^\| `/{split($0, c, "|"); print c[2]}' docs/mcp.md |
  grep -oE '`[a-z_0-9]+`' | tr -d '`')
for tool in $(missing_from "$doc_tools" "$src_tools"); do fail "docs/mcp.md documents unknown MCP tool: $tool"; done
for tool in $(missing_from "$src_tools" "$doc_tools"); do fail "MCP tool '$tool' is not in the docs/mcp.md tool table"; done
# Tool names used in JSON examples (`{"name":"add_math","arguments":..}`) and `MCP \`tool\`` mentions anywhere.
for f in "${DOCS[@]}"; do
  { grep -nE '"name": *"[a-z_0-9]+" *, *"arguments"' "$f" | sed -E 's/^([0-9]+):.*"name": *"([a-z_0-9]+)" *, *"arguments".*/\1 \2/' || true
    grep -noE 'MCP `[a-z_0-9]+`' "$f" | sed -E 's/^([0-9]+):MCP `(.*)`$/\1 \2/' || true; } |
  while read -r line tool; do
    grep -qx -- "$tool" <<<"$src_tools" || echo "$f:$line: unknown MCP tool: $tool"
  done
done | report || true
# Slash-separated tool names in prose, e.g. `cut_at`/`remove_range`, are covered by the table check.

# --------------------------------------------------------------------------
# Control methods (docs/mcp.md <-> crates/control/src/method.rs)
# --------------------------------------------------------------------------
src_methods=$(awk '/^pub enum Call \{/{on=1; next} on && /^}/{on=0} on' crates/control/src/method.rs |
  grep -oE 'rename = "[a-z_.0-9]+"' | sed -E 's/rename = "(.*)"/\1/')
[[ -n "$src_methods" ]] || fail "could not read the method list from crates/control/src/method.rs"
doc_methods=$(awk '/^### Methods$/{on=1; next} /^#/{on=0} on && /^\| `/{split($0, c, "|"); print c[2]}' docs/mcp.md |
  grep -oE '`[a-z_.0-9]+`' | tr -d '`')
for m in $(missing_from "$doc_methods" "$src_methods"); do fail "docs/mcp.md documents unknown control method: $m"; done
for m in $(missing_from "$src_methods" "$doc_methods"); do fail "control method '$m' is not in the docs/mcp.md method table"; done
# Any backticked `namespace.method` elsewhere must use a real namespace.method.
prefixes=$(sed -E 's/\..*//' <<<"$src_methods" | sort -u | paste -sd'|')
for f in "${DOCS[@]}"; do
  spans "$f" | while IFS=$'\t' read -r line token; do
    [[ "$token" =~ ^($prefixes)\.[a-z_0-9]+$ ]] || continue
    [[ "$token" == *.json ]] && continue
    grep -qx -- "$token" <<<"$src_methods" || echo "$f:$line: unknown control method: $token"
  done
done | report || true

# --------------------------------------------------------------------------
# Environment variables, cargo targets, plugin ids, commands, packages
# --------------------------------------------------------------------------
packages=$(grep -h '^name = ' crates/*/Cargo.toml apps/*/Cargo.toml | sed -E 's/name = "(.*)"/\1/')
binaries="animathio animathio-cli"
for f in "${DOCS[@]}"; do
  grep -noE 'ANIMATHIO_[A-Z0-9_]+' "$f" | while IFS=: read -r line var; do
    grep -rqw --include='*.rs' --include='*.sh' --include='*.yml' --include='*.toml' -- "$var" crates apps scripts .github plugins ||
      echo "$f:$line: environment variable not used anywhere in the code: $var"
  done
  grep -noE -- '(-p|--package) +[a-z][a-z0-9_-]*' "$f" | while IFS=: read -r line match; do
    pkg="${match##* }"
    grep -qx -- "$pkg" <<<"$packages" || echo "$f:$line: unknown cargo package: $pkg"
  done
  grep -noE -- '--example +[a-z][a-z0-9_]*' "$f" | while IFS=: read -r line match; do
    ex="${match##* }"
    compgen -G "crates/*/examples/$ex.rs" >/dev/null || compgen -G "apps/*/examples/$ex.rs" >/dev/null || echo "$f:$line: unknown cargo example: $ex"
  done
  grep -noE -- 'cargo test .*--test +[a-z][a-z0-9_]*' "$f" | while IFS=: read -r line match; do
    t="${match##* }"
    compgen -G "crates/*/tests/$t.rs" >/dev/null || compgen -G "apps/*/tests/$t.rs" >/dev/null || compgen -G "crates/*/tests/$t" >/dev/null ||
      echo "$f:$line: unknown integration test: $t"
  done
  grep -noE '`?(animathio-[a-z0-9-]+)`?' "$f" | while IFS=: read -r line token; do
    token="${token//\`/}"
    grep -qx -- "$token" <<<"$packages"$'\n'"${binaries// /$'\n'}" ||
      grep -rqw --include='*.yml' --include='*.toml' --include='*.sh' -- "$token" .github crates apps scripts Cargo.toml ||
      echo "$f:$line: unknown crate or binary: $token"
  done
  grep -noE 'org\.animathio\.[a-z0-9-]+' "$f" | while IFS=: read -r line id; do
    grep -rqF "\"$id\"" plugins/*/plugin.toml plugins/examples/*/plugin.toml 2>/dev/null ||
      grep -rqF "$id" plugins/*/plugin.toml 2>/dev/null ||
      grep -rqF --include='*.rs' -- "$id" crates apps ||
      echo "$f:$line: unknown plugin id: $id"
  done
  grep -noE 'Command::[A-Z][A-Za-z0-9]*' "$f" | while IFS=: read -r line token; do
    variant="${token#Command::}"
    grep -qE "^    $variant[ ({,]|^    $variant\$" crates/core/src/command.rs 2>/dev/null ||
      grep -rqE "^    $variant[ ({,]" crates/core/src ||
      echo "$f:$line: unknown core command: $token"
  done
done | report || true

# --------------------------------------------------------------------------
# Plugin API: WIT package version, worlds, kebab-case items, Rust symbols
# --------------------------------------------------------------------------
features=$(awk '/^\[features\]/{on=1; next} /^\[/{on=0} on && /^[a-z][a-z0-9_-]* *=/{print $1}' crates/*/Cargo.toml apps/*/Cargo.toml)
wit=crates/plugin-api/wit/plugin.wit
wit_version=$(grep -oE '^package animathio:plugin@[0-9.]+' "$wit" | sed 's/^package //')
wit_words=$( { grep -oE '[a-z][a-z0-9]*(-[a-z0-9]+)*' "$wit"
  ls crates apps plugins plugins/examples | grep -oE '[a-z][a-z0-9]*(-[a-z0-9]+)*'
  cat <<EOF
$packages
$features
build-plugins
cargo-packager
cargo-deny
release-plz
git-cliff
wasm32-wasip2
no-std
EOF
} | sort -u)
for f in docs/plugins.md docs/architecture.md; do
  grep -noE 'animathio:plugin@[0-9][0-9.]*' "$f" | while IFS=: read -r line v; do
    [[ "$v" == "$wit_version" ]] || echo "$f:$line: WIT package is $wit_version, not $v"
  done
  spans "$f" | while IFS=$'\t' read -r line token; do
    # Pure kebab-case tokens must be WIT items, crates, plugins or tools we know.
    if [[ "$token" =~ ^[a-z][a-z0-9]*(-[a-z0-9]+)+$ ]]; then
      grep -qx -- "$token" <<<"$wit_words" || echo "$f:$line: unknown WIT/crate name: $token"
    fi
  done
  { spans "$f" | sed -nE 's/^([0-9]+)\tworld +([a-z][a-z0-9-]*)$/\1 \2/p'
    grep -noE 'world: *"[a-z][a-z0-9-]*"' "$f" | sed -E 's/^([0-9]+):world: *"(.*)"$/\1 \2/' || true; } |
  while read -r line w; do
    grep -qE "^world +$w " "$wit" || echo "$f:$line: no WIT world named $w"
  done
done | report || true

# Rust symbols (`Type::item`, `function()`, `a::b::{X, Y}`) in the technical docs must be defined somewhere.
# `AudioContext` is a v1 (browser) type named in the architecture rationale.
rust_allow='Err|Ok|Some|None|Vec|Option|Result|String|Box|Arc|Self|Clone|Copy|Default|Debug|Send|Sync|Guest|AudioContext'
for f in docs/plugins.md docs/architecture.md; do
  spans "$f" | while IFS=$'\t' read -r line token; do
    [[ "$token" =~ ^[A-Za-z_][A-Za-z0-9_]*(::[A-Za-z_{][A-Za-z0-9_,{} ]*)*(\(\))?$ ]] || continue
    [[ "$token" == *'::'* || "$token" == *'()' || "$token" =~ ^[A-Z][a-z]+[A-Z] ]] || continue
    # Names to look up: each `{A, B}` member, else the last path segment.
    if [[ "$token" == *'{'* ]]; then
      names=$(sed -E 's/.*\{(.*)\}.*/\1/; s/[ ,]+/\n/g' <<<"$token")
    else
      names="${token%%(*}"; names="${names##*::}"
    fi
    for name in $names; do
      [[ "$name" =~ ^($rust_allow)$ ]] && continue
      grep -rqw --include='*.rs' --include='*.wit' -- "$name" crates apps plugins ||
        echo "$f:$line: symbol not found in the code: $name (in \`$token\`)"
    done
  done
done | report || true

# --------------------------------------------------------------------------
# Completeness of the architecture map
# --------------------------------------------------------------------------
for dir in crates/*/; do
  name=$(basename "$dir")
  grep -qE "^\| \`$name\` \|" docs/architecture.md || fail "docs/architecture.md crate map is missing crates/$name"
done
for dir in plugins/*/; do
  name=$(basename "$dir")
  [[ "$name" == examples ]] && continue
  grep -E '^\| `plugins/\*` \|' docs/architecture.md | grep -q "\`$name\`" ||
    fail "docs/architecture.md plugins/* row is missing plugins/$name"
done
for file in apps/animathio/src/editor/*.rs; do
  name=$(basename "$file" .rs)
  grep -q "\`$name\`" docs/architecture.md || fail "docs/architecture.md does not mention the editor module \`$name\`"
done

# Every `Command` variant is named in docs/architecture.md.
for variant in $(awk '/^pub enum Command \{/{on=1; next} on && /^}/{on=0} on && /^    [A-Z][A-Za-z0-9]*/{print $1}' crates/core/src/command.rs | sed 's/[({,]//g'); do
  grep -qw -- "$variant" docs/architecture.md || fail "docs/architecture.md does not mention core Command::$variant"
done

if ((errors)); then
  echo "docs check: $errors problem(s)" >&2
  exit 1
fi
echo "docs check: ok (${#DOCS[@]} documents)"
