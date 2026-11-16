# Dependency advisory review

Reviewed 2026-11-16 against all 15 tracked Cargo lockfiles, including plugins,
fuzz targets and historical spikes. Run `scripts/check-advisories.sh` to repeat
both RustSec scanners using the reason-bearing exceptions in `deny.toml`.
GitHub-only advisories must also be checked with the Dependabot alerts API;
RustSec does not currently contain the grid GHSA.

## GitHub alerts before and proposed result

| Dependency | Open alerts before | Result on this branch |
| --- | ---: | --- |
| zip | 12 (four GHSA IDs across three locks) | Fixed by inherited PR #266, zip 9.0.5 |
| grid | 1 | Unshipped viewport spike only; unreachable-function exception below |
| npm / other ecosystems | 0 | No legacy changes |

The zip IDs are GHSA-9hj8-73fh-9963, GHSA-8hxf-qr8p-62x4,
GHSA-q7v9-x93c-xqv3 and GHSA-m665-v7pm-7grj. Alert state is GitHub's
scan of the default branch: this PR does not dismiss alerts or claim that
GitHub has closed them before merge and rescan.

## Reviewed exceptions

| Advisory | Disposition and reason |
| --- | --- |
| RUSTSEC-2024-0320 | yaml-rust is transitive via syntect in Typst; unmaintained notice, no vulnerability or compatible replacement. Retire with the upstream dependency. |
| RUSTSEC-2024-0384 | instant is transitive in the GUI stack; maintenance-only notice, no vulnerability. Replacement requires upstream migration to web-time. |
| RUSTSEC-2024-0436 | paste is used by transitive macros; maintenance-only notice, no vulnerability. Replacement requires upstream macro migration. |
| RUSTSEC-2024-0449 | unsafe-libyaml is pulled by serde_yaml in the typesetting stack; maintenance-only notice, no vulnerability. Await upstream parser migration. |
| RUSTSEC-2025-0057 | fxhash is transitive in the typesetting stack; maintenance-only notice, no vulnerability and no compatible replacement. |
| RUSTSEC-2025-0134 | rustls-pemfile is transitive in GUI HTTP dependencies; maintenance-only notice, no vulnerability. Await upstream rustls-pki-types migration. |
| RUSTSEC-2025-0141 | bincode is transitive in Typst; maintenance-only notice, no vulnerability. Format migration belongs upstream. |
| RUSTSEC-2026-0192 | ttf-parser is used directly and by font/layout dependencies; maintenance-only notice, no vulnerability. Replacing the font API requires a separate migration. |
| RUSTSEC-2026-0206 | rustybuzz is transitive in text shaping; maintenance-only notice, no vulnerability. Await upstream harfrust migration. |
| RUSTSEC-2026-0370 | zvariant 5 is required by the GPUI Linux D-Bus stack; maintenance-only notice. zbus 6 migration must happen upstream. |
| RUSTSEC-2026-0371 | zbus_names 4 is required by the GPUI Linux D-Bus stack; maintenance-only notice. zbus 6 migration must happen upstream. |
| RUSTSEC-2026-0372 | zvariant_derive 5 is required by the GPUI Linux D-Bus stack; maintenance-only notice. zbus 6 migration must happen upstream. |
| RUSTSEC-2025-0052 | async-std exists only in the historical unshipped viewport spike via gpui 0.2.2; maintenance-only notice, no vulnerability. |
| RUSTSEC-2026-0173 | proc-macro-error2 exists only in the historical unshipped viewport spike; maintenance-only build-time macro notice. |
| RUSTSEC-2026-0441 | pin-utils exists only in the historical unshipped viewport spike; maintenance-only notice, no vulnerability. |
| RUSTSEC-2026-0194 | quick-xml 0.38 is pinned by citationberg 0.7 via Typst. MathWorld denies all external files and accepts only a generated math source, so attacker-supplied CSL/XML is inaccessible. No compatible patched release. |
| RUSTSEC-2026-0195 | quick-xml 0.38 is pinned by citationberg 0.7 via Typst. MathWorld denies all external files; namespace parsing of attacker-supplied CSL/XML is inaccessible. No compatible patched release. |
| RUSTSEC-2026-0235 | rkyv 0.7 is pinned by mitex-spec 0.2.4. MiTeX reads only its compile-time embedded default command specification; user input is LaTeX text, never archive bytes. No compatible patched release. |
| GHSA-38c5-483c-4qqp | grid 0.18 is pinned by taffy 0.9 via gpui 0.2.2 only in the unshipped viewport spike. Taffy uses Grid::new/from_vec and rebuilds storage in expand_to_fit_range; it never calls expand_rows. Shipped workspace has no grid dependency. Reassess on spike dependency changes. |

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
