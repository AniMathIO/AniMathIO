#!/usr/bin/env python3
"""Merge lockfile package identities before running flatpak-cargo-generator (pip)."""
import pathlib
import subprocess
import tempfile
import tomlkit

root = pathlib.Path(__file__).resolve().parents[2]
locks = [root / 'Cargo.lock', *sorted((root / 'plugins').glob('*/Cargo.lock')),
         root / 'plugins/examples/hello/Cargo.lock']
packages = {}
for lock in locks:
    for package in tomlkit.loads(lock.read_text())['package']:
        key = (package['name'], package['version'], package.get('source', ''))
        packages[key] = {k: v for k, v in package.items() if k != 'dependencies'}
with tempfile.TemporaryDirectory() as directory:
    lock = pathlib.Path(directory) / 'Cargo.lock'
    lock.write_text(tomlkit.dumps({'version': 4, 'package': list(packages.values())}))
    subprocess.run(['flatpak-cargo-generator', str(lock), '-o',
                    str(root / 'packaging/flatpak/cargo-sources.json')], check=True, cwd=root)
