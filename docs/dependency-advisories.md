# Dependency advisory review

Reviewed 2026-11-16 against all 15 tracked Cargo lockfiles, including plugins,
fuzz targets and historical spikes. Run `scripts/check-advisories.sh` to repeat
both RustSec scanners using the reason-bearing exceptions in `deny.toml`.
GitHub-only advisories must also be checked with the Dependabot alerts API;
RustSec does not currently contain the grid GHSA.

## GitHub alerts before and proposed result

| Dependency | Open alerts before | Result on this branch |
| --- | ---: | --- |
| zip | 12 (four GHSA IDs across three locks) | Fixed by inherited PR #266, minimum 9.0.5; all three locks resolve 9.6.0 |
| grid | 1 | Unshipped viewport spike only; unreachable-function exception below |
| npm / other ecosystems | 0 | No legacy changes |

The zip IDs are GHSA-9hj8-73fh-9963, GHSA-8hxf-qr8p-62x4,
GHSA-q7v9-x93c-xqv3 and GHSA-m665-v7pm-7grj. Alert state is GitHub's
scan of the default branch: this PR does not dismiss alerts or claim that
GitHub has closed them before merge and rescan.

## Reviewed exceptions

The authoritative, reason-bearing exception list is in `deny.toml`. The
reachability evidence below supplements that policy.

There are 15 RustSec advisory IDs in the workspace (three vulnerability IDs
and twelve maintenance notices), and three additional maintenance IDs in the
viewport spike. All have explicit exceptions; none is silently disabled by
category. The grid GHSA adds one GitHub-only exception. Other lockfiles had
no RustSec findings. Exceptions are not patched dependency versions and must
be reassessed when the upstream graph or accepted input paths change.

`cargo tree --manifest-path spikes/viewport/Cargo.toml -i grid` resolves
grid 0.18.0 → taffy 0.9.0 → gpui 0.2.2 → viewport spike.
Taffy's `grid = "0.18.0"` constraint cannot resolve grid 1.0.1. Its sole grid
consumer, `cell_occupancy.rs`, creates storage with `Grid::new` and
`Grid::from_vec`; `expand_to_fit_range` builds a new vector rather than calling
`Grid::expand_rows`. No application code in the spike directly uses grid.
The shipped workspace uses gpui-pre and has no grid package in its lockfile.

The XML exceptions concern citationberg 0.7's quick-xml 0.38 dependency;
updating citationberg within the current constraint yields no newer release.
The math host's `MathWorld::source` and `MathWorld::file` reject every
file except its generated main math source (input capped at 4 KiB), so users
cannot supply external CSL styles or XML namespaces through this adapter.
MiTeX receives `convert_math(source, None)`: its default specification comes
from `mitex-spec-gen`'s compile-time `include_bytes!`, not project archive
bytes. An upstream API migration is required to remove these dependencies.
