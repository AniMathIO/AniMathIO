#!/usr/bin/env python3
"""Merge lockfile package identities before running flatpak-cargo-generator (pip)."""
import pathlib
import shutil
import subprocess
import sys
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
    # Prefer the generator installed next to this interpreter (a venv run as
    # `venv/bin/python generate-sources.py` does not put venv/bin on PATH).
    local = pathlib.Path(sys.executable).with_name('flatpak-cargo-generator')
    generator = str(local) if local.exists() else shutil.which('flatpak-cargo-generator')
    if generator is None:
        sys.exit('flatpak-cargo-generator not found: pip install flatpak-cargo-generator==0.1.5')
    subprocess.run([generator, str(lock), '-o',
                    str(root / 'packaging/flatpak/cargo-sources.json')], check=True, cwd=root)
