#!/usr/bin/env python3
"""Fetch only the pinned public-domain recordings, outside the source tree."""

import argparse

from common import recording

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument(
    "names", nargs="*", choices=["pythagorean", "unit-circle", "fourier"]
)
args = parser.parse_args()
for name in args.names or ["pythagorean", "unit-circle", "fourier"]:
    print(recording(name, fetch=True))
