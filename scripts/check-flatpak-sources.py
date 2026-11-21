#!/usr/bin/env python3
"""Fail when packaging/flatpak/cargo-sources.json is out of sync with the lockfiles.

The Flatpak build vendors every crate from cargo-sources.json (offline), so each
registry package in the root Cargo.lock, every plugins/*/Cargo.lock and the hello
example lockfile must appear there with the same version and checksum, and the
file must not carry crates no lockfile asks for. Needs only Python 3.11+ (no
flatpak-cargo-generator); the fix is `python packaging/flatpak/generate-sources.py`
(see docs/flatpak.md).
"""
import json
import pathlib
import sys
import tomllib

root = pathlib.Path(__file__).resolve().parents[1]
locks = [root / 'Cargo.lock', *sorted((root / 'plugins').glob('*/Cargo.lock')),
         root / 'plugins/examples/hello/Cargo.lock']

wanted = {}  # (name, version) -> checksum
for lock in locks:
    for package in tomllib.loads(lock.read_text())['package']:
        source = package.get('source', '')
        if not source:
            continue  # workspace member or path dependency, built from the checkout
        if not source.startswith('registry+'):
            sys.exit(f'{lock.relative_to(root)}: {package["name"]} uses source {source}; '
                     'check-flatpak-sources.py and generate-sources.py only handle registry crates')
        wanted[(package['name'], package['version'])] = package['checksum']

have = {}
for entry in json.loads((root / 'packaging/flatpak/cargo-sources.json').read_text()):
    if entry.get('type') != 'archive':
        continue
    stem = entry['dest'].rsplit('/', 1)[-1]  # cargo/vendor/<name>-<version>
    have[stem] = entry['sha256']

wanted_by_stem = {f'{name}-{version}': checksum for (name, version), checksum in wanted.items()}
missing = sorted(stem for stem in wanted_by_stem if stem not in have)
extra = sorted(stem for stem in have if stem not in wanted_by_stem)
mismatched = sorted(stem for stem in wanted_by_stem
                    if stem in have and have[stem] != wanted_by_stem[stem])

if missing or extra or mismatched:
    for label, stems in (('missing from cargo-sources.json', missing),
                         ('not in any lockfile (stale)', extra),
                         ('checksum differs from the lockfile', mismatched)):
        if stems:
            print(f'{len(stems)} crate(s) {label}: {", ".join(stems[:12])}'
                  f'{" ..." if len(stems) > 12 else ""}', file=sys.stderr)
    sys.exit('packaging/flatpak/cargo-sources.json is stale; regenerate it with '
             '`python packaging/flatpak/generate-sources.py` (docs/flatpak.md)')
print(f'cargo-sources.json matches {len(locks)} lockfiles ({len(wanted_by_stem)} crates)')
